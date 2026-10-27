use aws_sdk_s3::{
    primitives::ByteStream,
    types::{CompletedMultipartUpload, CompletedPart},
    Client,
};
use aws_smithy_types::byte_stream::Length;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, HashMap},
    future::Future,
    path::{Path, PathBuf},
    sync::{Arc, Mutex as StdMutex},
    time::{Duration, Instant, UNIX_EPOCH},
};
use tokio::{
    io::{AsyncReadExt, AsyncSeekExt, AsyncWriteExt},
    sync::{mpsc, watch, Mutex, Notify},
    task::JoinSet,
};
use uuid::Uuid;

const STOPPED: &str = "__transfer_stopped__";
const CHUNK: u64 = 16 * 1024 * 1024;
pub type Events = Arc<dyn Fn(Transfer) + Send + Sync>;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Direction {
    Upload,
    Download,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Transfer {
    pub id: String,
    pub name: String,
    pub account_id: String,
    pub bucket: String,
    pub key: String,
    pub direction: Direction,
    pub status: String,
    pub bytes: u64,
    pub total: u64,
    pub error: Option<String>,
    #[serde(default)]
    pub speed: u64,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct Settings {
    pub concurrency: usize,
}
impl Default for Settings {
    fn default() -> Self {
        Self { concurrency: 8 }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
struct Stamp {
    size: u64,
    modified: u128,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
struct Part {
    length: u64,
    etag: Option<String>,
    hash: Option<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Job {
    pub view: Transfer,
    local_path: PathBuf,
    temporary: PathBuf,
    chunk: u64,
    source_stamp: Option<Stamp>,
    source_hash: Option<String>,
    destination_stamp: Option<Stamp>,
    upload_id: Option<String>,
    etag: Option<String>,
    version_id: Option<String>,
    completed: BTreeMap<u32, Part>,
}
#[derive(Clone, Serialize, Deserialize)]
struct Database {
    version: u32,
    #[serde(default)]
    settings: Settings,
    jobs: Vec<Job>,
}
impl Default for Database {
    fn default() -> Self {
        Self {
            version: 1,
            settings: Settings::default(),
            jobs: vec![],
        }
    }
}
#[derive(Clone, Copy, PartialEq, Eq)]
enum Stop {
    Run,
    Pause,
    Cancel,
}
struct Gate {
    active: usize,
    limit: usize,
}
struct Limiter {
    gate: StdMutex<Gate>,
    changed: Notify,
}
struct Permit(Arc<Limiter>);
impl Drop for Permit {
    fn drop(&mut self) {
        self.0.gate.lock().unwrap().active -= 1;
        self.0.changed.notify_waiters();
    }
}
impl Limiter {
    async fn acquire(self: &Arc<Self>, stop: &mut watch::Receiver<Stop>) -> Result<Permit, String> {
        loop {
            let changed = self.changed.notified();
            {
                let mut gate = self.gate.lock().unwrap();
                if gate.active < gate.limit {
                    gate.active += 1;
                    return Ok(Permit(self.clone()));
                }
            }
            if *stop.borrow() != Stop::Run {
                return Err(STOPPED.into());
            }
            tokio::select! { _ = changed => {}, _ = stop.changed() => return Err(STOPPED.into()) }
        }
    }
}
struct Inner {
    updating: std::sync::atomic::AtomicBool,
    path: PathBuf,
    db: Mutex<Database>,
    active: Mutex<HashMap<String, watch::Sender<Stop>>>,
    limiter: Arc<Limiter>,
    settings_changed: Notify,
    event: Events,
}
#[derive(Clone)]
pub struct Engine(Arc<Inner>);

pub struct UpdateGuard(Engine);
impl Drop for UpdateGuard {
    fn drop(&mut self) {
        self.0
             .0
            .updating
            .store(false, std::sync::atomic::Ordering::Release);
    }
}
struct Receipt {
    number: u32,
    part: Part,
}
struct Progress {
    number: u32,
    bytes: u64,
}

pub fn part_size(size: u64) -> u64 {
    CHUNK.max(size.div_ceil(10_000))
}
fn count(job: &Job) -> u32 {
    job.view.total.div_ceil(job.chunk) as u32
}
fn range(job: &Job, number: u32) -> (u64, u64) {
    let offset = (number as u64 - 1) * job.chunk;
    (offset, job.chunk.min(job.view.total - offset))
}
fn committed(job: &Job) -> u64 {
    job.completed.values().map(|p| p.length).sum()
}
fn terminal(status: &str) -> bool {
    matches!(status, "done" | "cancelled")
}
pub fn failure(e: impl std::error::Error) -> String {
    let mut value = e.to_string();
    let mut source = e.source();
    while let Some(e) = source {
        let next = e.to_string();
        if !value.contains(&next) {
            value.push_str(": ");
            value.push_str(&next);
        }
        source = e.source();
    }
    value
}
async fn stamp(path: &Path) -> Result<Stamp, String> {
    let info = tokio::fs::metadata(path).await.map_err(failure)?;
    if !info.is_file() {
        return Err("请选择普通文件。".into());
    }
    Ok(Stamp {
        size: info.len(),
        modified: info
            .modified()
            .map_err(failure)?
            .duration_since(UNIX_EPOCH)
            .map_err(failure)?
            .as_nanos(),
    })
}
async fn optional_stamp(path: &Path) -> Result<Option<Stamp>, String> {
    match tokio::fs::metadata(path).await {
        Ok(_) => stamp(path).await.map(Some),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(failure(e)),
    }
}
async fn hash_range(path: &Path, offset: u64, length: u64) -> Result<String, String> {
    let mut file = tokio::fs::File::open(path).await.map_err(failure)?;
    file.seek(std::io::SeekFrom::Start(offset))
        .await
        .map_err(failure)?;
    let mut hash = Sha256::new();
    let mut left = length;
    let mut buffer = vec![0; 256 * 1024];
    while left > 0 {
        let limit = left.min(buffer.len() as u64) as usize;
        let size = file.read(&mut buffer[..limit]).await.map_err(failure)?;
        if size == 0 {
            return Err("文件长度变化或断点文件不完整。".into());
        }
        hash.update(&buffer[..size]);
        left -= size as u64;
    }
    Ok(format!("{:x}", hash.finalize()))
}
async fn cancellable<T>(
    future: impl Future<Output = Result<T, String>>,
    stop: &mut watch::Receiver<Stop>,
) -> Result<T, String> {
    if *stop.borrow() != Stop::Run {
        return Err(STOPPED.into());
    }
    tokio::select! { biased; _ = stop.changed() => Err(STOPPED.into()), result = future => result }
}

impl Engine {
    pub fn open(path: PathBuf, event: Events) -> Result<Self, Box<dyn std::error::Error>> {
        let mut db: Database = match std::fs::read(&path) {
            Ok(bytes) => serde_json::from_slice(&bytes)?,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Database::default(),
            Err(e) => return Err(e.into()),
        };
        if db.version != 1 || !(1..=32).contains(&db.settings.concurrency) {
            return Err("传输记录版本或并发设置无效。".into());
        }
        for job in &mut db.jobs {
            job.view.speed = 0;
            if !terminal(&job.view.status) {
                job.view.bytes = committed(job);
            }
            if matches!(
                job.view.status.as_str(),
                "running" | "pausing" | "preparing" | "finalizing" | "queued"
            ) {
                job.view.status = "paused".into();
            }
        }
        let concurrency = db.settings.concurrency;
        Ok(Self(Arc::new(Inner {
            updating: std::sync::atomic::AtomicBool::new(false),
            path,
            db: Mutex::new(db),
            active: Mutex::new(HashMap::new()),
            limiter: Arc::new(Limiter {
                gate: StdMutex::new(Gate {
                    active: 0,
                    limit: concurrency,
                }),
                changed: Notify::new(),
            }),
            settings_changed: Notify::new(),
            event,
        })))
    }
    async fn persist(&self, db: &Database) -> Result<(), String> {
        let temporary = self.0.path.with_extension("json.tmp");
        let bytes = serde_json::to_vec_pretty(db).map_err(failure)?;
        let mut file = tokio::fs::File::create(&temporary).await.map_err(failure)?;
        file.write_all(&bytes).await.map_err(failure)?;
        file.sync_all().await.map_err(failure)?;
        drop(file);
        tokio::fs::rename(temporary, &self.0.path)
            .await
            .map_err(failure)
    }
    async fn edit(
        &self,
        id: &str,
        durable: bool,
        update: impl FnOnce(&mut Job),
    ) -> Result<Job, String> {
        let mut db = self.0.db.lock().await;
        let backup = db.clone();
        let job = db
            .jobs
            .iter_mut()
            .find(|j| j.view.id == id)
            .ok_or("传输任务不存在。")?;
        update(job);
        let snapshot = job.clone();
        if durable {
            if let Err(e) = self.persist(&db).await {
                *db = backup;
                return Err(format!("无法保存传输断点：{e}"));
            }
        }
        drop(db);
        (self.0.event)(snapshot.view.clone());
        Ok(snapshot)
    }
    pub async fn job(&self, id: &str) -> Result<Job, String> {
        self.0
            .db
            .lock()
            .await
            .jobs
            .iter()
            .find(|j| j.view.id == id)
            .cloned()
            .ok_or("传输任务不存在。".into())
    }
    pub async fn list(&self) -> Vec<Transfer> {
        self.0
            .db
            .lock()
            .await
            .jobs
            .iter()
            .rev()
            .map(|j| j.view.clone())
            .collect()
    }
    pub async fn settings(&self) -> Settings {
        self.0.db.lock().await.settings.clone()
    }
    pub async fn has_unfinished_account(&self, account_id: &str) -> bool {
        self.0
            .db
            .lock()
            .await
            .jobs
            .iter()
            .any(|j| j.view.account_id == account_id && !terminal(&j.view.status))
    }
    pub async fn set_settings(&self, concurrency: usize) -> Result<Settings, String> {
        if !(1..=32).contains(&concurrency) {
            return Err("并发数量需为 1–32 的整数。".into());
        }
        let settings = Settings { concurrency };
        let mut db = self.0.db.lock().await;
        let previous = db.settings.clone();
        db.settings = settings.clone();
        if let Err(e) = self.persist(&db).await {
            db.settings = previous;
            return Err(e);
        }
        self.0.limiter.gate.lock().unwrap().limit = concurrency;
        self.0.limiter.changed.notify_waiters();
        self.0.settings_changed.notify_waiters();
        Ok(settings)
    }
    pub async fn enqueue(
        &self,
        direction: Direction,
        account_id: String,
        bucket: String,
        key: String,
        local_path: PathBuf,
    ) -> Result<String, String> {
        if key.is_empty() || key.len() > 1024 || key.ends_with('/') {
            return Err("对象键无效。".into());
        }
        let id = Uuid::new_v4().to_string();
        let local_path = if direction == Direction::Upload {
            tokio::fs::canonicalize(local_path).await.map_err(failure)?
        } else {
            let parent = local_path.parent().ok_or("保存路径无效。")?;
            tokio::fs::canonicalize(parent)
                .await
                .map_err(failure)?
                .join(local_path.file_name().ok_or("保存文件名无效。")?)
        };
        let source_stamp = if direction == Direction::Upload {
            Some(stamp(&local_path).await?)
        } else {
            None
        };
        let total = source_stamp.as_ref().map(|s| s.size).unwrap_or(0);
        if total > 5 * 1024_u64.pow(4) {
            return Err("文件超过 5 TiB 上传上限。".into());
        }
        let destination_stamp = if direction == Direction::Download {
            optional_stamp(&local_path).await?
        } else {
            None
        };
        let temporary = local_path.with_file_name(format!(".s3studio-{id}.part"));
        let name = local_path
            .file_name()
            .unwrap()
            .to_string_lossy()
            .into_owned();
        let mut db = self.0.db.lock().await;
        if self.0.updating.load(std::sync::atomic::Ordering::Acquire) {
            return Err("正在更新应用，请完成更新后再添加传输任务。".into());
        }
        if db.jobs.iter().any(|j| {
            !terminal(&j.view.status)
                && ((direction == Direction::Download
                    && j.view.direction == direction
                    && paths_equal(&j.local_path, &local_path))
                    || (direction == Direction::Upload
                        && j.view.direction == direction
                        && j.view.account_id == account_id
                        && j.view.bucket == bucket
                        && j.view.key == key))
        }) {
            return Err("此文件已有未结束的传输任务，请继续或取消原任务。".into());
        }
        let job = Job {
            view: Transfer {
                id: id.clone(),
                name,
                account_id,
                bucket,
                key,
                direction,
                status: "queued".into(),
                bytes: 0,
                total,
                speed: 0,
                error: None,
            },
            local_path,
            temporary,
            chunk: part_size(total),
            source_stamp,
            source_hash: None,
            destination_stamp,
            upload_id: None,
            etag: None,
            version_id: None,
            completed: BTreeMap::new(),
        };
        db.jobs.push(job.clone());
        if let Err(e) = self.persist(&db).await {
            db.jobs.pop();
            return Err(e);
        }
        drop(db);
        (self.0.event)(job.view);
        Ok(id)
    }
    pub async fn start(&self, id: &str, client: Client) -> Result<(), String> {
        let mut active = self.0.active.lock().await;
        if self.0.updating.load(std::sync::atomic::Ordering::Acquire) {
            return Err("正在更新应用，请完成更新后再继续传输。".into());
        }
        if active.contains_key(id) {
            return Err("任务正在运行或停止中，请稍后重试。".into());
        }
        let job = self.job(id).await?;
        if terminal(&job.view.status) {
            return Err("任务已经结束。".into());
        }
        self.edit(id, true, |job| {
            job.view.status = "preparing".into();
            job.view.error = None;
            job.view.speed = 0;
            job.view.bytes = committed(job);
        })
        .await?;
        let (sender, receiver) = watch::channel(Stop::Run);
        active.insert(id.into(), sender.clone());
        let engine = self.clone();
        let id = id.to_owned();
        tokio::spawn(async move {
            engine.run(id, client, sender, receiver).await;
        });
        Ok(())
    }
    pub async fn reserve_for_update(&self) -> Result<UpdateGuard, String> {
        let active = self.0.active.lock().await;
        let db = self.0.db.lock().await;
        if !active.is_empty()
            || db.jobs.iter().any(|j| {
                matches!(
                    j.view.status.as_str(),
                    "queued" | "preparing" | "running" | "pausing" | "finalizing"
                )
            })
        {
            return Err("请先暂停或完成正在运行的传输任务，再安装应用更新。".into());
        }
        if self
            .0
            .updating
            .swap(true, std::sync::atomic::Ordering::AcqRel)
        {
            return Err("应用更新已经在进行中。".into());
        }
        Ok(UpdateGuard(self.clone()))
    }
    pub async fn pause(&self, id: &str) -> Result<(), String> {
        let active = self.0.active.lock().await;
        let job = self.job(id).await?;
        if job.view.status == "finalizing" {
            return Err("正在提交文件，完成后即可结束。".into());
        }
        let sender = active.get(id).ok_or("任务当前没有运行。")?;
        sender.send(Stop::Pause).map_err(failure)?;
        self.edit(id, true, |j| {
            if matches!(j.view.status.as_str(), "running" | "preparing") {
                j.view.status = "pausing".into();
            }
        })
        .await?;
        Ok(())
    }
    pub async fn cancel(&self, id: &str, client: Client) -> Result<(), String> {
        let mut active = self.0.active.lock().await;
        let job = self.job(id).await?;
        if terminal(&job.view.status) {
            return Ok(());
        }
        if job.view.status == "finalizing" {
            return Err("正在提交文件，无法取消。".into());
        }
        if let Some(sender) = active.get(id) {
            sender.send(Stop::Cancel).map_err(failure)?;
        } else {
            self.edit(id, true, |j| {
                j.view.status = "pausing".into();
            })
            .await?;
            let (sender, receiver) = watch::channel(Stop::Cancel);
            active.insert(id.into(), sender.clone());
            let engine = self.clone();
            let id = id.to_owned();
            tokio::spawn(async move {
                engine.run(id, client, sender, receiver).await;
            });
        }
        Ok(())
    }
    pub async fn clear_finished(&self) -> Result<(), String> {
        let mut db = self.0.db.lock().await;
        let previous = db.jobs.clone();
        db.jobs.retain(|j| !terminal(&j.view.status));
        if let Err(e) = self.persist(&db).await {
            db.jobs = previous;
            return Err(e);
        }
        Ok(())
    }
    async fn checkpoint(&self, id: &str, receipt: Receipt) -> Result<(), String> {
        self.edit(id, true, |j| {
            j.completed.insert(receipt.number, receipt.part);
            j.view.bytes = committed(j);
        })
        .await?;
        Ok(())
    }
    async fn run(
        &self,
        id: String,
        client: Client,
        sender: watch::Sender<Stop>,
        mut stop: watch::Receiver<Stop>,
    ) {
        let mut workers = JoinSet::new();
        let result = self.perform(&id, &client, &mut stop, &mut workers).await;
        if !workers.is_empty() && *stop.borrow() == Stop::Run {
            let _ = sender.send(Stop::Pause);
        }
        let mut result = result;
        // Workers observe stop at network/read boundaries and finish pending disk writes.
        // No new run can start until every worker has relinquished its file handle.
        while let Some(receipt) = workers.join_next().await {
            if let Ok(Ok(receipt)) = receipt {
                if let Err(e) = self.checkpoint(&id, receipt).await {
                    result = Err(e);
                }
            }
        }
        let was_cancelled = *stop.borrow() == Stop::Cancel;
        if was_cancelled {
            result = self.cleanup(&id, &client).await;
        }
        let status = if was_cancelled && result.is_ok() {
            "cancelled"
        } else if result.is_ok() {
            "done"
        } else if result.as_ref().err().is_some_and(|e| e == STOPPED) {
            "paused"
        } else {
            "error"
        };
        let message = result.err().filter(|e| e != STOPPED);
        // Remove the active guard before publishing a resumable state.
        let mut active = self.0.active.lock().await;
        active.remove(&id);
        let final_result = self
            .edit(&id, true, |j| {
                j.view.status = status.into();
                j.view.speed = 0;
                j.view.error = message;
                j.view.bytes = if status == "done" {
                    j.view.total
                } else {
                    committed(j)
                };
            })
            .await;
        if let Err(error) = final_result {
            let _ = self
                .edit(&id, false, |j| {
                    j.view.status = "error".into();
                    j.view.error = Some(error);
                    j.view.speed = 0;
                })
                .await;
        }
    }
    async fn cleanup(&self, id: &str, client: &Client) -> Result<(), String> {
        let job = self.job(id).await?;
        if job.view.direction == Direction::Upload {
            if let Some(upload_id) = job.upload_id {
                match client
                    .abort_multipart_upload()
                    .bucket(&job.view.bucket)
                    .key(&job.view.key)
                    .upload_id(upload_id)
                    .send()
                    .await
                {
                    Ok(_) => {}
                    Err(e) if e.raw_response().is_some_and(|r| r.status().as_u16() == 404) => {}
                    Err(e) => return Err(failure(e)),
                }
            }
        } else {
            match tokio::fs::remove_file(&job.temporary).await {
                Ok(()) => {}
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => return Err(failure(e)),
            }
        }
        Ok(())
    }
    async fn prepare_upload(
        &self,
        id: &str,
        client: &Client,
        stop: &mut watch::Receiver<Stop>,
    ) -> Result<bool, String> {
        let job = self.job(id).await?;
        if Some(stamp(&job.local_path).await?) != job.source_stamp {
            return Err("本地源文件已变化，请取消任务后重新上传。".into());
        }
        let digest = cancellable(hash_range(&job.local_path, 0, job.view.total), stop).await?;
        if job.source_hash.as_ref().is_some_and(|hash| hash != &digest) {
            return Err("源文件内容已变化，不能继续原上传。".into());
        }
        self.edit(id, true, |j| {
            j.source_hash = Some(digest);
        })
        .await?;
        if let Some(upload_id) = &job.upload_id {
            let mut marker = None;
            let mut parts = BTreeMap::new();
            loop {
                let response = cancellable(
                    async {
                        client
                            .list_parts()
                            .bucket(&job.view.bucket)
                            .key(&job.view.key)
                            .upload_id(upload_id)
                            .set_part_number_marker(marker.clone())
                            .send()
                            .await
                            .map_err(failure)
                    },
                    stop,
                )
                .await;
                let response = match response {
                    Ok(response) => response,
                    Err(error) => {
                        // A lost CompleteMultipartUpload acknowledgement must not duplicate data.
                        if error != STOPPED && self.upload_already_complete(id, client).await? {
                            return Ok(true);
                        }
                        return Err(error);
                    }
                };
                for part in response.parts() {
                    let number = part.part_number().unwrap_or(0);
                    if number <= 0 || number as u32 > count(&job) {
                        return Err("服务端分片编号无效。".into());
                    }
                    let (_, length) = range(&job, number as u32);
                    if part.size().unwrap_or(-1) != length as i64 {
                        return Err("服务端分片长度与源文件不一致。".into());
                    }
                    parts.insert(
                        number as u32,
                        Part {
                            length,
                            etag: Some(part.e_tag().ok_or("服务端分片缺少 ETag。")?.into()),
                            hash: None,
                        },
                    );
                }
                marker = response.next_part_number_marker().map(str::to_owned);
                if !response.is_truncated().unwrap_or(false) {
                    break;
                }
                if marker.is_none() {
                    return Err("服务端未返回分片分页标记。".into());
                }
            }
            self.edit(id, true, |j| {
                j.completed = parts;
                j.view.bytes = committed(j);
            })
            .await?;
        } else if job.view.total > 0 {
            if *stop.borrow() != Stop::Run {
                return Err(STOPPED.into());
            }
            // Do not drop this request on pause: keep its UploadId before stopping.
            let created = client
                .create_multipart_upload()
                .bucket(&job.view.bucket)
                .key(&job.view.key)
                .content_type(
                    mime_guess::from_path(&job.local_path)
                        .first_or_octet_stream()
                        .to_string(),
                )
                .metadata("s3studio-transfer-id", id)
                .send()
                .await
                .map_err(failure)?;
            let upload_id = created
                .upload_id()
                .ok_or("服务未返回 multipart UploadId。")?
                .to_owned();
            if let Err(error) = self
                .edit(id, true, |j| {
                    j.upload_id = Some(upload_id.clone());
                })
                .await
            {
                let cleanup = client
                    .abort_multipart_upload()
                    .bucket(&job.view.bucket)
                    .key(&job.view.key)
                    .upload_id(upload_id)
                    .send()
                    .await;
                return Err(match cleanup {
                    Ok(_) => error,
                    Err(e) => format!("{error}；分片清理失败：{}", failure(e)),
                });
            }
        }
        Ok(false)
    }
    async fn upload_already_complete(&self, id: &str, client: &Client) -> Result<bool, String> {
        let job = self.job(id).await?;
        match client
            .head_object()
            .bucket(&job.view.bucket)
            .key(&job.view.key)
            .send()
            .await
        {
            Ok(response) => Ok(response.content_length() == Some(job.view.total as i64)
                && response
                    .metadata()
                    .and_then(|m| m.get("s3studio-transfer-id"))
                    .is_some_and(|v| v == id)),
            Err(_) => Ok(false),
        }
    }
    async fn prepare_download(
        &self,
        id: &str,
        client: &Client,
        stop: &mut watch::Receiver<Stop>,
    ) -> Result<bool, String> {
        let mut job = self.job(id).await?;
        let resuming = job.etag.is_some();
        let head = cancellable(
            async {
                client
                    .head_object()
                    .bucket(&job.view.bucket)
                    .key(&job.view.key)
                    .set_version_id(job.version_id.clone())
                    .send()
                    .await
                    .map_err(failure)
            },
            stop,
        )
        .await?;
        let total = head.content_length().ok_or("服务未返回对象大小。")?;
        if total < 0 {
            return Err("对象大小无效。".into());
        }
        let etag = head
            .e_tag()
            .ok_or("服务未返回对象 ETag，无法安全进行断点下载。")?
            .to_owned();
        if job.etag.as_ref().is_some_and(|e| e != &etag)
            || (job.etag.is_some() && job.view.total != total as u64)
        {
            return Err("远端对象已变化，请取消任务后重新下载。".into());
        }
        job = self
            .edit(id, true, |j| {
                j.etag = Some(etag);
                j.version_id = head.version_id().map(str::to_owned);
                j.view.total = total as u64;
                j.chunk = part_size(total as u64);
            })
            .await?;
        let existing = optional_stamp(&job.temporary).await?;
        if let Some(stamp) = existing {
            if stamp.size != job.view.total {
                return Err("下载临时文件大小变化，请取消后重新下载。".into());
            }
        } else {
            // Recover a crash after rename but before the final "done" checkpoint.
            if resuming
                && committed(&job) == job.view.total
                && optional_stamp(&job.local_path)
                    .await?
                    .is_some_and(|s| s.size == job.view.total)
            {
                let mut verified = true;
                for (number, part) in &job.completed {
                    let (offset, length) = range(&job, *number);
                    if part.hash.as_ref()
                        != Some(
                            &cancellable(hash_range(&job.local_path, offset, length), stop).await?,
                        )
                    {
                        verified = false;
                        break;
                    }
                }
                if verified {
                    return Ok(true);
                }
            }
            if !job.completed.is_empty() {
                return Err("下载断点文件已丢失，请取消后重新下载。".into());
            }
            let file = tokio::fs::OpenOptions::new()
                .create_new(true)
                .write(true)
                .open(&job.temporary)
                .await
                .map_err(failure)?;
            file.set_len(job.view.total).await.map_err(failure)?;
            file.sync_all().await.map_err(failure)?;
        }
        let mut valid = BTreeMap::new();
        for (number, part) in &job.completed {
            if *number == 0 || *number > count(&job) {
                return Err("下载断点分片编号无效。".into());
            }
            let (offset, length) = range(&job, *number);
            let digest = cancellable(hash_range(&job.temporary, offset, length), stop).await?;
            if part.length == length && part.hash.as_ref() == Some(&digest) {
                valid.insert(*number, part.clone());
            }
        }
        self.edit(id, true, |j| {
            j.completed = valid;
            j.view.bytes = committed(j);
        })
        .await?;
        Ok(false)
    }
    async fn perform(
        &self,
        id: &str,
        client: &Client,
        stop: &mut watch::Receiver<Stop>,
        workers: &mut JoinSet<Result<Receipt, String>>,
    ) -> Result<(), String> {
        if *stop.borrow() != Stop::Run {
            return Err(STOPPED.into());
        }
        let direction = self.job(id).await?.view.direction;
        let prepare_permit = self.0.limiter.acquire(stop).await?;
        if direction == Direction::Upload {
            if self.prepare_upload(id, client, stop).await? {
                return Ok(());
            }
        } else if self.prepare_download(id, client, stop).await? {
            return Ok(());
        }
        drop(prepare_permit);
        if *stop.borrow() != Stop::Run {
            return Err(STOPPED.into());
        }
        let job = self
            .edit(id, true, |j| {
                j.view.status = "running".into();
            })
            .await?;
        let mut pending = (1..=count(&job)).filter(|n| !job.completed.contains_key(n));
        let (progress_sender, mut progress_receiver) = mpsc::channel::<Progress>(64);
        let mut progress = BTreeMap::<u32, u64>::new();
        let mut previous_bytes = committed(&job);
        let mut previous_time = Instant::now();
        let mut last_event = Instant::now();
        loop {
            let settings_changed = self.0.settings_changed.notified();
            let concurrency = self.0.limiter.gate.lock().unwrap().limit;
            while workers.len() < concurrency {
                let Some(number) = pending.next() else {
                    break;
                };
                let client = client.clone();
                let job = job.clone();
                let limiter = self.0.limiter.clone();
                let stop = stop.clone();
                let progress = progress_sender.clone();
                workers.spawn(async move {
                    transfer_part(client, job, number, limiter, stop, progress).await
                });
            }
            if workers.is_empty() {
                break;
            }
            tokio::select! {
                biased;
                _ = stop.changed() => return Err(STOPPED.into()),
                receipt = workers.join_next() => {
                    let receipt = receipt.ok_or("分片任务意外结束。")?.map_err(failure)??;
                    progress.remove(&receipt.number); self.checkpoint(id, receipt).await?;
                    if last_event.elapsed() >= Duration::from_millis(180) {
                        let base = self.job(id).await?;
                        let bytes = committed(&base) + progress.iter().filter(|(n, _)| !base.completed.contains_key(n)).map(|(_, v)| *v).sum::<u64>();
                        let speed = (bytes.saturating_sub(previous_bytes) as f64 / previous_time.elapsed().as_secs_f64().max(0.001)) as u64;
                        self.edit(id, false, |j| { j.view.bytes = bytes.min(j.view.total); j.view.speed = speed; }).await?;
                        previous_bytes = bytes; previous_time = Instant::now(); last_event = Instant::now();
                    }
                },
                Some(update) = progress_receiver.recv() => {
                    progress.insert(update.number, update.bytes);
                    if last_event.elapsed() >= Duration::from_millis(180) {
                        let base = self.job(id).await?;
                        let bytes = committed(&base) + progress.iter().filter(|(n, _)| !base.completed.contains_key(n)).map(|(_, v)| *v).sum::<u64>();
                        let speed = ((bytes.saturating_sub(previous_bytes)) as f64 / previous_time.elapsed().as_secs_f64().max(0.001)) as u64;
                        self.edit(id, false, |j| { j.view.bytes = bytes.min(j.view.total); j.view.speed = speed; }).await?;
                        previous_bytes = bytes; previous_time = Instant::now(); last_event = Instant::now();
                    }
                },
                _ = settings_changed => {},
            }
        }
        if *stop.borrow() != Stop::Run {
            return Err(STOPPED.into());
        }
        let job = self.job(id).await?;
        if committed(&job) != job.view.total {
            return Err("已完成分片长度不完整。".into());
        }
        if direction == Direction::Upload {
            if Some(stamp(&job.local_path).await?) != job.source_stamp {
                return Err("上传期间源文件已变化，请取消任务后重新上传。".into());
            }
            let digest = cancellable(hash_range(&job.local_path, 0, job.view.total), stop).await?;
            if job.source_hash.as_ref() != Some(&digest) {
                return Err("上传期间源文件内容已变化，已阻止提交。".into());
            }
        }
        self.edit(id, true, |j| {
            j.view.status = "finalizing".into();
            j.view.speed = 0;
        })
        .await?;
        if *stop.borrow() != Stop::Run {
            return Err(STOPPED.into());
        }
        if direction == Direction::Upload {
            if job.view.total == 0 {
                client
                    .put_object()
                    .bucket(&job.view.bucket)
                    .key(&job.view.key)
                    .metadata("s3studio-transfer-id", id)
                    .body(ByteStream::from_static(b""))
                    .send()
                    .await
                    .map_err(failure)?;
            } else {
                let parts = job
                    .completed
                    .iter()
                    .map(|(number, part)| {
                        CompletedPart::builder()
                            .part_number(*number as i32)
                            .set_e_tag(part.etag.clone())
                            .build()
                    })
                    .collect();
                let result = client
                    .complete_multipart_upload()
                    .bucket(&job.view.bucket)
                    .key(&job.view.key)
                    .upload_id(job.upload_id.as_deref().ok_or("UploadId 丢失。")?)
                    .multipart_upload(
                        CompletedMultipartUpload::builder()
                            .set_parts(Some(parts))
                            .build(),
                    )
                    .send()
                    .await;
                if let Err(error) = result {
                    if !self.upload_already_complete(id, client).await? {
                        return Err(failure(error));
                    }
                }
            }
        } else {
            if optional_stamp(&job.local_path).await? != job.destination_stamp {
                return Err("保存位置的文件已变化，为防止覆盖新文件，已保留下载断点。请取消后重新选择保存位置。".into());
            }
            tokio::fs::rename(&job.temporary, &job.local_path)
                .await
                .map_err(failure)?;
        }
        Ok(())
    }
}

#[cfg(test)]
#[path = "transfers_tests.rs"]
mod tests;

fn paths_equal(a: &Path, b: &Path) -> bool {
    if cfg!(windows) {
        a.to_string_lossy().to_lowercase() == b.to_string_lossy().to_lowercase()
    } else {
        a == b
    }
}
async fn transfer_part(
    client: Client,
    job: Job,
    number: u32,
    limiter: Arc<Limiter>,
    mut stop: watch::Receiver<Stop>,
    progress: mpsc::Sender<Progress>,
) -> Result<Receipt, String> {
    if *stop.borrow() != Stop::Run {
        return Err(STOPPED.into());
    }
    let _permit = limiter.acquire(&mut stop).await?;
    let (offset, length) = range(&job, number);
    if job.view.direction == Direction::Upload {
        let body = ByteStream::read_from()
            .path(&job.local_path)
            .offset(offset)
            .length(Length::Exact(length))
            .buffer_size(256 * 1024)
            .build()
            .await
            .map_err(failure)?;
        let result = cancellable(
            async {
                client
                    .upload_part()
                    .bucket(&job.view.bucket)
                    .key(&job.view.key)
                    .upload_id(job.upload_id.as_deref().ok_or("UploadId 丢失。")?)
                    .part_number(number as i32)
                    .body(body)
                    .send()
                    .await
                    .map_err(failure)
            },
            &mut stop,
        )
        .await?;
        let _ = progress.try_send(Progress {
            number,
            bytes: length,
        });
        Ok(Receipt {
            number,
            part: Part {
                length,
                etag: Some(result.e_tag().ok_or("上传分片未返回 ETag。")?.into()),
                hash: None,
            },
        })
    } else {
        let response = cancellable(
            async {
                client
                    .get_object()
                    .bucket(&job.view.bucket)
                    .key(&job.view.key)
                    .range(format!("bytes={offset}-{}", offset + length - 1))
                    .if_match(job.etag.as_deref().ok_or("ETag 丢失。")?)
                    .set_version_id(job.version_id.clone())
                    .send()
                    .await
                    .map_err(failure)
            },
            &mut stop,
        )
        .await?;
        let expected_range = format!("bytes {offset}-{}/{}", offset + length - 1, job.view.total);
        if response.content_range() != Some(expected_range.as_str())
            || response.content_length() != Some(length as i64)
            || response.e_tag() != job.etag.as_deref()
        {
            return Err("服务端返回了不正确的分片范围、长度或 ETag，已阻止写入。".into());
        }
        let mut stream = response.body.into_async_read();
        let mut file = tokio::fs::OpenOptions::new()
            .write(true)
            .open(&job.temporary)
            .await
            .map_err(failure)?;
        file.seek(std::io::SeekFrom::Start(offset))
            .await
            .map_err(failure)?;
        let mut hash = Sha256::new();
        let mut bytes = 0_u64;
        let mut buffer = vec![0; 256 * 1024];
        let result: Result<(), String> = async {
            loop {
                let size = cancellable(
                    async { stream.read(&mut buffer).await.map_err(failure) },
                    &mut stop,
                )
                .await?;
                if size == 0 {
                    break;
                }
                if bytes + size as u64 > length {
                    return Err("下载分片超出预期长度。".into());
                }
                // Await writes without cancellation, so pause cannot leave a writer alive.
                file.write_all(&buffer[..size]).await.map_err(failure)?;
                hash.update(&buffer[..size]);
                bytes += size as u64;
                let _ = progress.try_send(Progress { number, bytes });
            }
            if bytes != length {
                return Err("下载分片不完整，可以重试继续。".into());
            }
            Ok(())
        }
        .await;
        file.flush().await.map_err(failure)?;
        file.sync_all().await.map_err(failure)?;
        drop(file);
        result?;
        Ok(Receipt {
            number,
            part: Part {
                length,
                etag: None,
                hash: Some(format!("{:x}", hash.finalize())),
            },
        })
    }
}

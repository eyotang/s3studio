use super::*;
use aws_credential_types::Credentials;
use aws_sdk_s3::config::{
    retry::RetryConfig, BehaviorVersion, Region, RequestChecksumCalculation,
    ResponseChecksumValidation,
};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use tiny_http::{Header, Method, Request, Response, StatusCode};

struct Workspace(PathBuf);
impl Workspace {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!("s3studio-test-{}", Uuid::new_v4()));
        std::fs::create_dir(&path).unwrap();
        Self(path)
    }
    fn engine(&self) -> Engine {
        Engine::open(self.0.join("transfers.json"), Arc::new(|_| {})).unwrap()
    }
}
impl Drop for Workspace {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
#[derive(Default)]
struct Remote {
    uploads: HashMap<String, BTreeMap<u32, Vec<u8>>>,
    uploaded: Vec<u8>,
    upload_counts: BTreeMap<u32, usize>,
    download_counts: BTreeMap<u32, usize>,
    etag: String,
    aborted: usize,
    list_parts: usize,
    marker: String,
}
struct Mock {
    endpoint: String,
    data: Arc<Vec<u8>>,
    state: Arc<StdMutex<Remote>>,
    maximum: Arc<AtomicUsize>,
    shutdown: Arc<AtomicBool>,
    hold_downloads: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}
fn header(name: &str, value: impl ToString) -> Header {
    Header::from_bytes(name, value.to_string()).unwrap()
}
fn request_header(request: &Request, name: &'static str) -> Option<String> {
    request
        .headers()
        .iter()
        .find(|h| h.field.equiv(name))
        .map(|h| h.value.as_str().to_owned())
}
impl Mock {
    fn new(size: usize) -> Self {
        let data = Arc::new((0..size).map(|n| (n % 251) as u8).collect::<Vec<_>>());
        let state = Arc::new(StdMutex::new(Remote {
            etag: "\"remote-v1\"".into(),
            ..Default::default()
        }));
        let server = tiny_http::Server::http("127.0.0.1:0").unwrap();
        let endpoint = format!("http://{}", server.server_addr());
        let shutdown = Arc::new(AtomicBool::new(false));
        let hold_downloads = Arc::new(AtomicBool::new(false));
        let active = Arc::new(AtomicUsize::new(0));
        let maximum = Arc::new(AtomicUsize::new(0));
        let thread = {
            let (data, state, shutdown, maximum, hold_downloads) = (
                data.clone(),
                state.clone(),
                shutdown.clone(),
                maximum.clone(),
                hold_downloads.clone(),
            );
            std::thread::spawn(move || {
                let mut handlers = vec![];
                while !shutdown.load(Ordering::SeqCst) {
                    if let Some(request) = server.recv_timeout(Duration::from_millis(20)).unwrap() {
                        let (data, state, active, maximum) =
                            (data.clone(), state.clone(), active.clone(), maximum.clone());
                        let hold_downloads = hold_downloads.clone();
                        handlers.push(std::thread::spawn(move || {
                            handle(request, data, state, active, maximum, hold_downloads)
                        }));
                    }
                }
                for handler in handlers {
                    handler.join().unwrap();
                }
            })
        };
        Self {
            endpoint,
            data,
            state,
            maximum,
            shutdown,
            hold_downloads,
            thread: Some(thread),
        }
    }
    fn client(&self) -> Client {
        Client::from_conf(
            aws_sdk_s3::Config::builder()
                .behavior_version(BehaviorVersion::latest())
                .region(Region::new("us-east-1"))
                .credentials_provider(Credentials::new(
                    "test-access",
                    "test-secret",
                    None,
                    None,
                    "test",
                ))
                .request_checksum_calculation(RequestChecksumCalculation::WhenRequired)
                .response_checksum_validation(ResponseChecksumValidation::WhenRequired)
                .retry_config(RetryConfig::standard().with_max_attempts(1))
                .force_path_style(true)
                .endpoint_url(&self.endpoint)
                .build(),
        )
    }
}
impl Drop for Mock {
    fn drop(&mut self) {
        self.shutdown.store(true, Ordering::SeqCst);
        self.hold_downloads.store(false, Ordering::SeqCst);
        if let Some(thread) = self.thread.take() {
            thread.join().unwrap();
        }
    }
}
struct Active(Arc<AtomicUsize>);
impl Drop for Active {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::SeqCst);
    }
}
fn handle(
    mut request: Request,
    data: Arc<Vec<u8>>,
    state: Arc<StdMutex<Remote>>,
    active: Arc<AtomicUsize>,
    maximum: Arc<AtomicUsize>,
    hold_downloads: Arc<AtomicBool>,
) {
    let url = url::Url::parse(&format!("http://localhost{}", request.url())).unwrap();
    let query = url.query_pairs().into_owned().collect::<HashMap<_, _>>();
    let method = request.method().clone();
    if method == Method::Head {
        let remote = state.lock().unwrap();
        let response = Response::new(
            StatusCode(200),
            vec![
                header("ETag", &remote.etag),
                header("x-amz-meta-s3studio-transfer-id", &remote.marker),
            ],
            std::io::empty(),
            Some(data.len()),
            None,
        );
        let _ = request.respond(response.with_chunked_threshold(usize::MAX));
    } else if method == Method::Post && query.contains_key("uploads") {
        let id = Uuid::new_v4().to_string();
        let mut remote = state.lock().unwrap();
        remote.marker =
            request_header(&request, "x-amz-meta-s3studio-transfer-id").unwrap_or_default();
        remote.uploads.insert(id.clone(), BTreeMap::new());
        drop(remote);
        let _ = request.respond(Response::from_string(format!("<InitiateMultipartUploadResult><Bucket>test-bucket</Bucket><Key>file.bin</Key><UploadId>{id}</UploadId></InitiateMultipartUploadResult>")));
    } else if method == Method::Put && query.contains_key("uploadId") {
        let current = active.fetch_add(1, Ordering::SeqCst) + 1;
        maximum.fetch_max(current, Ordering::SeqCst);
        let _guard = Active(active);
        let number: u32 = query["partNumber"].parse().unwrap();
        *state
            .lock()
            .unwrap()
            .upload_counts
            .entry(number)
            .or_default() += 1;
        let mut body = vec![];
        if request.as_reader().read_to_end(&mut body).is_err() {
            return;
        }
        // Finish part one promptly; other parts leave time to pause deterministically.
        std::thread::sleep(Duration::from_millis(if number == 1 { 220 } else { 700 }));
        if let Some(parts) = state.lock().unwrap().uploads.get_mut(&query["uploadId"]) {
            parts.insert(number, body);
        }
        let _ = request.respond(
            Response::empty(200).with_header(header("ETag", format!("\"part-{number}\""))),
        );
    } else if method == Method::Get && query.contains_key("uploadId") {
        let mut remote = state.lock().unwrap();
        remote.list_parts += 1;
        let Some(parts) = remote.uploads.get(&query["uploadId"]) else {
            drop(remote);
            let _ = request.respond(
                Response::from_string("<Error><Code>NoSuchUpload</Code></Error>")
                    .with_status_code(404),
            );
            return;
        };
        let parts = parts.iter().map(|(number, body)| format!("<Part><PartNumber>{number}</PartNumber><ETag>\"part-{number}\"</ETag><Size>{}</Size></Part>", body.len())).collect::<String>();
        drop(remote);
        let _ = request.respond(Response::from_string(format!(
            "<ListPartsResult><IsTruncated>false</IsTruncated>{parts}</ListPartsResult>"
        )));
    } else if method == Method::Post && query.contains_key("uploadId") {
        let mut body = String::new();
        let _ = request.as_reader().read_to_string(&mut body);
        let mut remote = state.lock().unwrap();
        let parts = remote.uploads.remove(&query["uploadId"]).unwrap();
        // Ensure the completion payload includes every part in order.
        let mut previous = 0;
        for number in parts.keys() {
            let position = body
                .find(&format!("<PartNumber>{number}</PartNumber>"))
                .unwrap();
            assert!(position >= previous);
            previous = position;
        }
        remote.uploaded = parts.into_values().flatten().collect();
        drop(remote);
        let _ = request.respond(Response::from_string("<CompleteMultipartUploadResult><ETag>\"completed\"</ETag></CompleteMultipartUploadResult>"));
    } else if method == Method::Delete && query.contains_key("uploadId") {
        let mut remote = state.lock().unwrap();
        remote.uploads.remove(&query["uploadId"]);
        remote.aborted += 1;
        drop(remote);
        let _ = request.respond(Response::empty(204));
    } else if method == Method::Get {
        let range = request_header(&request, "Range").unwrap();
        let (begin, end) = range
            .strip_prefix("bytes=")
            .unwrap()
            .split_once('-')
            .unwrap();
        let (begin, end) = (
            begin.parse::<usize>().unwrap(),
            end.parse::<usize>().unwrap(),
        );
        let number = (begin as u64 / CHUNK + 1) as u32;
        let current = active.fetch_add(1, Ordering::SeqCst) + 1;
        maximum.fetch_max(current, Ordering::SeqCst);
        let _guard = Active(active);
        let etag = {
            let mut remote = state.lock().unwrap();
            *remote.download_counts.entry(number).or_default() += 1;
            remote.etag.clone()
        };
        assert_eq!(request_header(&request, "If-Match"), Some(etag.clone()));
        // Keep later ranges incomplete until the test has observed "paused".
        // A fixed delay races with part-one hashing on slower CI machines.
        while number > 1 && hold_downloads.load(Ordering::SeqCst) {
            std::thread::sleep(Duration::from_millis(10));
        }
        std::thread::sleep(Duration::from_millis(if number == 1 { 220 } else { 700 }));
        let _ = request.respond(
            Response::from_data(data[begin..=end].to_vec())
                .with_chunked_threshold(usize::MAX)
                .with_status_code(206)
                .with_header(header("ETag", etag))
                .with_header(header(
                    "Content-Range",
                    format!("bytes {begin}-{end}/{}", data.len()),
                )),
        );
    } else if method == Method::Put {
        let mut body = vec![];
        let _ = request.as_reader().read_to_end(&mut body);
        state.lock().unwrap().uploaded = body;
        let _ = request.respond(Response::empty(200).with_header(header("ETag", "\"empty\"")));
    } else {
        panic!("unexpected {method} {}", request.url());
    }
}
async fn until(engine: &Engine, id: &str, predicate: impl Fn(&Job) -> bool) -> Job {
    tokio::time::timeout(Duration::from_secs(30), async {
        loop {
            let job = engine.job(id).await.unwrap();
            if predicate(&job) {
                return job;
            }
            assert_ne!(job.view.status, "error", "{:?}", job.view.error);
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("transfer timed out")
}
async fn enqueue(engine: &Engine, direction: Direction, path: PathBuf, key: &str) -> String {
    engine
        .enqueue(
            direction,
            "account".into(),
            "test-bucket".into(),
            key.into(),
            path,
        )
        .await
        .unwrap()
}
async fn paused_download(workspace: &Workspace, mock: &Mock) -> (Engine, String, Job) {
    mock.hold_downloads.store(true, Ordering::SeqCst);
    let engine = workspace.engine();
    engine.set_settings(2).await.unwrap();
    let id = enqueue(
        &engine,
        Direction::Download,
        workspace.0.join("saved.bin"),
        "file.bin",
    )
    .await;
    engine.start(&id, mock.client()).await.unwrap();
    until(&engine, &id, |j| j.completed.contains_key(&1)).await;
    engine.pause(&id).await.unwrap();
    let job = until(&engine, &id, |j| j.view.status == "paused").await;
    mock.hold_downloads.store(false, Ordering::SeqCst);
    (engine, id, job)
}

#[tokio::test]
async fn default_eight_settings_persist_and_reject_invalid_values() {
    let workspace = Workspace::new();
    let engine = workspace.engine();
    assert_eq!(engine.settings().await.concurrency, 8);
    assert!(engine.set_settings(0).await.is_err());
    assert!(engine.set_settings(33).await.is_err());
    engine.set_settings(12).await.unwrap();
    assert_eq!(workspace.engine().settings().await.concurrency, 12);
    assert_eq!(engine.0.limiter.gate.lock().unwrap().limit, 12);
}
#[tokio::test]
async fn live_global_limit_waits_on_reduction_and_wakes_on_increase() {
    let workspace = Workspace::new();
    let engine = workspace.engine();
    engine.set_settings(2).await.unwrap();
    let (_sender, mut stop) = watch::channel(Stop::Run);
    let first = engine.0.limiter.acquire(&mut stop).await.unwrap();
    let second = engine.0.limiter.acquire(&mut stop).await.unwrap();
    engine.set_settings(1).await.unwrap();
    let limiter = engine.0.limiter.clone();
    let mut waiting = tokio::spawn(async move { limiter.acquire(&mut stop).await });
    assert!(
        tokio::time::timeout(Duration::from_millis(60), &mut waiting)
            .await
            .is_err()
    );
    drop(first);
    assert!(
        tokio::time::timeout(Duration::from_millis(60), &mut waiting)
            .await
            .is_err()
    );
    engine.set_settings(2).await.unwrap();
    let third = tokio::time::timeout(Duration::from_secs(1), waiting)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert_eq!(engine.0.limiter.gate.lock().unwrap().active, 2);
    drop(second);
    drop(third);
}
#[tokio::test]
async fn upload_pauses_reopens_and_reuses_server_parts() {
    let workspace = Workspace::new();
    let mock = Mock::new(CHUNK as usize * 2 + 137);
    let source = workspace.0.join("source.bin");
    tokio::fs::write(&source, mock.data.as_slice())
        .await
        .unwrap();
    let engine = workspace.engine();
    engine.set_settings(2).await.unwrap();
    let id = enqueue(&engine, Direction::Upload, source, "file.bin").await;
    engine.start(&id, mock.client()).await.unwrap();
    until(&engine, &id, |j| j.completed.contains_key(&1)).await;
    engine.pause(&id).await.unwrap();
    let paused = until(&engine, &id, |j| j.view.status == "paused").await;
    assert!(paused.upload_id.is_some());
    assert_eq!(paused.view.bytes, committed(&paused));
    drop(engine);
    // Let abandoned server requests settle, as happens while an application restarts.
    tokio::time::sleep(Duration::from_millis(800)).await;
    let engine = workspace.engine();
    assert_eq!(engine.job(&id).await.unwrap().view.status, "paused");
    engine.start(&id, mock.client()).await.unwrap();
    until(&engine, &id, |j| j.view.status == "done").await;
    let remote = mock.state.lock().unwrap();
    assert_eq!(remote.upload_counts[&1], 1);
    assert_eq!(remote.list_parts, 1);
    assert_eq!(remote.uploaded, *mock.data);
}
#[tokio::test]
async fn download_resume_skips_verified_parts_and_replaces_only_after_completion() {
    let workspace = Workspace::new();
    let mock = Mock::new(CHUNK as usize + 137);
    let (engine, id, job) = paused_download(&workspace, &mock).await;
    assert!(!job.local_path.exists());
    assert!(job.temporary.exists());
    drop(engine);
    tokio::time::sleep(Duration::from_millis(800)).await;
    let engine = workspace.engine();
    engine.start(&id, mock.client()).await.unwrap();
    until(&engine, &id, |j| j.view.status == "done").await;
    assert_eq!(tokio::fs::read(&job.local_path).await.unwrap(), *mock.data);
    assert!(!job.temporary.exists());
    assert_eq!(mock.state.lock().unwrap().download_counts[&1], 1);
}
#[tokio::test]
async fn corrupted_download_checkpoint_is_downloaded_again() {
    let workspace = Workspace::new();
    let mock = Mock::new(CHUNK as usize + 137);
    let (engine, id, job) = paused_download(&workspace, &mock).await;
    let mut file = tokio::fs::OpenOptions::new()
        .write(true)
        .open(&job.temporary)
        .await
        .unwrap();
    file.write_all(b"corrupted").await.unwrap();
    file.sync_all().await.unwrap();
    drop(file);
    engine.start(&id, mock.client()).await.unwrap();
    until(&engine, &id, |j| j.view.status == "done").await;
    assert_eq!(tokio::fs::read(&job.local_path).await.unwrap(), *mock.data);
    assert_eq!(mock.state.lock().unwrap().download_counts[&1], 2);
}
#[tokio::test]
async fn changed_remote_is_rejected_and_cancel_cleans_checkpoint() {
    let workspace = Workspace::new();
    let mock = Mock::new(CHUNK as usize + 137);
    let (engine, id, job) = paused_download(&workspace, &mock).await;
    mock.state.lock().unwrap().etag = "\"remote-v2\"".into();
    engine.start(&id, mock.client()).await.unwrap();
    until(&engine, &id, |j| j.view.status == "error").await;
    assert!(engine
        .job(&id)
        .await
        .unwrap()
        .view
        .error
        .unwrap()
        .contains("远端对象已变化"));
    assert!(!job.local_path.exists());
    assert!(engine.has_unfinished_account("account").await);
    engine.clear_finished().await.unwrap();
    assert_eq!(engine.list().await.len(), 1);
    engine.cancel(&id, mock.client()).await.unwrap();
    until(&engine, &id, |j| j.view.status == "cancelled").await;
    assert!(!job.temporary.exists());
    assert!(!engine.has_unfinished_account("account").await);
    engine.clear_finished().await.unwrap();
    assert!(engine.list().await.is_empty());
}
#[tokio::test]
async fn several_downloads_share_the_application_limit() {
    let workspace = Workspace::new();
    let mock = Mock::new(1024 * 64);
    let engine = workspace.engine();
    engine.set_settings(2).await.unwrap();
    let mut ids = vec![];
    for n in 0..5 {
        let id = enqueue(
            &engine,
            Direction::Download,
            workspace.0.join(format!("file{n}.bin")),
            "file.bin",
        )
        .await;
        engine.start(&id, mock.client()).await.unwrap();
        ids.push(id);
    }
    for id in ids {
        until(&engine, &id, |j| j.view.status == "done").await;
    }
    assert_eq!(mock.maximum.load(Ordering::SeqCst), 2);
}
#[tokio::test]
async fn cancel_paused_upload_aborts_remote_multipart() {
    let workspace = Workspace::new();
    let mock = Mock::new(CHUNK as usize + 137);
    let source = workspace.0.join("source.bin");
    tokio::fs::write(&source, mock.data.as_slice())
        .await
        .unwrap();
    let engine = workspace.engine();
    let id = enqueue(&engine, Direction::Upload, source, "file.bin").await;
    engine.start(&id, mock.client()).await.unwrap();
    until(&engine, &id, |j| j.completed.contains_key(&1)).await;
    engine.pause(&id).await.unwrap();
    until(&engine, &id, |j| j.view.status == "paused").await;
    engine.cancel(&id, mock.client()).await.unwrap();
    until(&engine, &id, |j| j.view.status == "cancelled").await;
    assert_eq!(mock.state.lock().unwrap().aborted, 1);
    assert!(mock.state.lock().unwrap().uploads.is_empty());
}
#[tokio::test]
async fn empty_upload_and_download_finish_without_parts() {
    let workspace = Workspace::new();
    let mock = Mock::new(0);
    let source = workspace.0.join("empty.bin");
    tokio::fs::write(&source, b"").await.unwrap();
    let engine = workspace.engine();
    let up = enqueue(&engine, Direction::Upload, source, "empty.bin").await;
    engine.start(&up, mock.client()).await.unwrap();
    until(&engine, &up, |j| j.view.status == "done").await;
    let down = enqueue(
        &engine,
        Direction::Download,
        workspace.0.join("saved.bin"),
        "empty.bin",
    )
    .await;
    engine.start(&down, mock.client()).await.unwrap();
    let job = until(&engine, &down, |j| j.view.status == "done").await;
    assert_eq!(tokio::fs::metadata(job.local_path).await.unwrap().len(), 0);
}

#[tokio::test]
async fn changed_source_prevents_upload_resume() {
    let workspace = Workspace::new();
    let mock = Mock::new(CHUNK as usize + 137);
    let source = workspace.0.join("source.bin");
    tokio::fs::write(&source, mock.data.as_slice())
        .await
        .unwrap();
    let engine = workspace.engine();
    let id = enqueue(&engine, Direction::Upload, source.clone(), "file.bin").await;
    engine.start(&id, mock.client()).await.unwrap();
    until(&engine, &id, |j| j.completed.contains_key(&1)).await;
    engine.pause(&id).await.unwrap();
    until(&engine, &id, |j| j.view.status == "paused").await;
    tokio::fs::write(source, b"changed source").await.unwrap();
    engine.start(&id, mock.client()).await.unwrap();
    let failed = until(&engine, &id, |j| j.view.status == "error").await;
    assert!(failed.view.error.unwrap().contains("源文件已变化"));
    assert!(mock.state.lock().unwrap().uploaded.is_empty());
    engine.cancel(&id, mock.client()).await.unwrap();
    until(&engine, &id, |j| j.view.status == "cancelled").await;
}

#[tokio::test]
async fn changed_destination_is_preserved_instead_of_overwritten() {
    let workspace = Workspace::new();
    let mock = Mock::new(CHUNK as usize + 137);
    let destination = workspace.0.join("saved.bin");
    tokio::fs::write(&destination, b"original file")
        .await
        .unwrap();
    let (engine, id, job) = paused_download(&workspace, &mock).await;
    tokio::fs::write(&destination, b"new content from another application")
        .await
        .unwrap();
    engine.start(&id, mock.client()).await.unwrap();
    let failed = until(&engine, &id, |j| j.view.status == "error").await;
    assert!(failed.view.error.unwrap().contains("防止覆盖"));
    assert_eq!(
        tokio::fs::read(&destination).await.unwrap(),
        b"new content from another application"
    );
    assert!(job.temporary.exists());
    engine.cancel(&id, mock.client()).await.unwrap();
    until(&engine, &id, |j| j.view.status == "cancelled").await;
    assert!(!job.temporary.exists());
    assert!(destination.exists());
}

#[tokio::test]
async fn restart_after_download_rename_recovers_without_redownloading() {
    let workspace = Workspace::new();
    let mock = Mock::new(64 * 1024);
    let engine = workspace.engine();
    let id = enqueue(
        &engine,
        Direction::Download,
        workspace.0.join("saved.bin"),
        "file.bin",
    )
    .await;
    engine.start(&id, mock.client()).await.unwrap();
    until(&engine, &id, |j| j.view.status == "done").await;
    // Model a successful rename followed by a crash before persisting "done".
    engine
        .edit(&id, true, |j| j.view.status = "finalizing".into())
        .await
        .unwrap();
    drop(engine);
    let engine = workspace.engine();
    assert_eq!(engine.job(&id).await.unwrap().view.status, "paused");
    engine.start(&id, mock.client()).await.unwrap();
    until(&engine, &id, |j| j.view.status == "done").await;
    assert_eq!(mock.state.lock().unwrap().download_counts[&1], 1);
}

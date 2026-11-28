use crate::{
    accounts::AccountStore,
    i18n::t,
    s3::{
        bucket_client,
        files::{self, Filter, Selection},
        S3Object,
    },
    transfers::{Direction, Engine},
};
use aws_sdk_s3::Client;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs, io,
    path::{Path, PathBuf},
};
use tokio::io::AsyncReadExt;

fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}

fn valid_component(part: &str) -> bool {
    let stem = part.split('.').next().unwrap_or("").to_ascii_uppercase();
    !part.is_empty()
        && part != "."
        && part != ".."
        && !part.ends_with(['.', ' '])
        && !part
            .chars()
            .any(|c| c.is_control() || r#"\/:*?"<>|"#.contains(c))
        && !matches!(
            stem.as_str(),
            "CON" | "CONIN$" | "CONOUT$" | "PRN" | "AUX" | "NUL"
        )
        && !(stem.len() == 4
            && (stem.starts_with("COM") || stem.starts_with("LPT"))
            && stem.as_bytes()[3].is_ascii_digit())
}
/// Resolve each component beneath the chosen root; never descend through a symlink.
fn local_path(root: &Path, relative: &str) -> Result<PathBuf, String> {
    let mut path = root.to_path_buf();
    for part in relative.split('/') {
        if !valid_component(part) {
            return Err(format!("{}: {relative}", t("保存路径无效。")));
        }
        path.push(part);
        match fs::symlink_metadata(&path) {
            Ok(m) if m.file_type().is_symlink() || is_reparse(&m) => {
                return Err(format!("{}: {}", t("保存路径无效。"), path.display()))
            }
            Err(e) if e.kind() != io::ErrorKind::NotFound => return Err(err(e)),
            _ => {}
        }
    }
    Ok(path)
}
#[cfg(windows)]
fn is_reparse(m: &fs::Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;
    m.file_attributes() & 0x400 != 0
}
#[cfg(not(windows))]
fn is_reparse(_: &fs::Metadata) -> bool {
    false
}

#[derive(Clone)]
struct Local {
    path: PathBuf,
    size: u64,
    modified: String,
    folder: bool,
}

fn walk(root: &Path) -> Result<BTreeMap<String, Local>, String> {
    let mut result = BTreeMap::new();
    let mut todo = vec![(root.to_owned(), String::new())];
    while let Some((dir, prefix)) = todo.pop() {
        for entry in fs::read_dir(&dir).map_err(err)? {
            let entry = entry.map_err(err)?;
            let meta = fs::symlink_metadata(entry.path()).map_err(err)?;
            if meta.is_symlink() || is_reparse(&meta) || !(meta.is_dir() || meta.is_file()) {
                continue;
            }
            let name = entry
                .file_name()
                .into_string()
                .map_err(|_| t("对象键无效。"))?;
            let mut key = format!("{prefix}{name}");
            if meta.is_dir() {
                key.push('/');
                todo.push((entry.path(), key.clone()));
            }
            result.insert(
                key,
                Local {
                    path: entry.path(),
                    size: if meta.is_file() { meta.len() } else { 0 },
                    modified: chrono::DateTime::<chrono::Utc>::from(meta.modified().map_err(err)?)
                        .to_rfc3339(),
                    folder: meta.is_dir(),
                },
            );
        }
    }
    Ok(result)
}
fn root_path(path: &Path) -> Result<PathBuf, String> {
    let root = path.canonicalize().map_err(err)?;
    if !root.is_dir() {
        return Err(t("保存路径无效。").into());
    }
    Ok(root)
}
async fn queue(
    engine: &Engine,
    client: &Client,
    account: &str,
    bucket: &str,
    key: String,
    path: PathBuf,
    dir: Direction,
    condition: Option<String>,
) -> Result<String, String> {
    let id = engine
        .enqueue_pinned(
            dir,
            account.into(),
            bucket.into(),
            key,
            path,
            None,
            condition,
        )
        .await?;
    engine.start(&id, client.clone()).await?;
    Ok(id)
}
async fn mkdir_remote(client: &Client, bucket: &str, key: &str) -> Result<(), String> {
    match client
        .put_object()
        .bucket(bucket)
        .key(key)
        .if_none_match("*")
        .send()
        .await
    {
        Ok(_) => Ok(()),
        Err(e) if e.raw_response().is_some_and(|r| r.status().as_u16() == 412) => Ok(()),
        Err(e) => Err(err(e)),
    }
}
pub async fn upload(
    engine: &Engine,
    store: &AccountStore,
    account: &str,
    bucket: &str,
    prefix: &str,
    folder: &Path,
    filter: &Filter,
) -> Result<usize, String> {
    let root = root_path(folder)?;
    let name = root
        .file_name()
        .and_then(|v| v.to_str())
        .ok_or(t("对象键无效。"))?;
    let base = format!("{prefix}{name}/");
    let matcher = filter.compile()?;
    let contents = walk(&root)?;
    let _change = store.connection_changes.lock().await;
    let client = bucket_client(store, account, bucket).await?;
    let mut count = 0;
    if *filter == Filter::default() {
        mkdir_remote(&client, bucket, &base).await?;
    }
    for (rel, file) in contents {
        let key = format!("{base}{rel}");
        if file.folder {
            if *filter == Filter::default() {
                mkdir_remote(&client, bucket, &key).await?;
            }
        } else if matcher.matches(&rel, file.size, &file.modified, "") {
            queue(
                engine,
                &client,
                account,
                bucket,
                key.clone(),
                file.path,
                Direction::Upload,
                None,
            )
            .await
            .map_err(|e| format!("{} {count}\n{key}: {e}", t("已完成")))?;
            count += 1;
        }
    }
    Ok(count)
}

fn mapped<'a>(
    root: &Path,
    prefix: &str,
    objects: &'a [S3Object],
) -> Result<Vec<(&'a S3Object, PathBuf)>, String> {
    let mut paths = BTreeMap::new();
    let mut output = Vec::new();
    for o in objects {
        let key = o.key.strip_prefix(prefix).ok_or(t("保存路径无效。"))?;
        let rel = if o.folder {
            key.strip_suffix('/').unwrap_or(key)
        } else {
            key
        };
        if rel.is_empty() && o.folder {
            continue;
        }
        let path = local_path(root, rel)?;
        let mut component = String::new();
        for (i, piece) in rel.split('/').enumerate() {
            if i > 0 {
                component.push('/');
            }
            component.push_str(piece);
            let folder = component != rel || o.folder;
            let local_type = fs::symlink_metadata(root.join(&component));
            if local_type.is_ok_and(|m| m.is_dir() != folder) {
                return Err(format!("{}: {component}", t("多个对象映射到同一目标")));
            }
            let identity = icu_normalizer::ComposingNormalizerBorrowed::new_nfc()
                .normalize(&component.to_lowercase())
                .into_owned();
            if let Some((existing, existing_dir)) =
                paths.insert(identity, (component.clone(), folder))
            {
                if existing != component || existing_dir != folder {
                    return Err(format!("{}: {rel}", t("多个对象映射到同一目标")));
                }
            }
        }
        output.push((o, path));
    }
    Ok(output)
}

pub async fn download(
    engine: &Engine,
    store: &AccountStore,
    account: &str,
    bucket: &str,
    prefix: &str,
    items: &[Selection],
    folder: &Path,
    filter: &Filter,
) -> Result<usize, String> {
    let root = root_path(folder)?;
    let _change = store.connection_changes.lock().await;
    let client = bucket_client(store, account, bucket).await?;
    let objects = files::expand(&client, bucket, items, filter).await?;
    let targets = mapped(&root, prefix, &objects)?;
    let mut count = 0;
    for (o, path) in targets {
        let dir = if o.folder {
            &path
        } else {
            path.parent().ok_or(t("保存路径无效。"))?
        };
        fs::create_dir_all(dir).map_err(err)?;
        if !o.folder {
            queue(
                engine,
                &client,
                account,
                bucket,
                o.key.clone(),
                path,
                Direction::Download,
                None,
            )
            .await
            .map_err(|e| format!("{} {count}\n{}: {e}", t("已完成"), o.key))?;
            count += 1;
        }
    }
    Ok(count)
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncJob {
    pub name: String,
    pub account: String,
    pub bucket: String,
    pub prefix: String,
    pub local: PathBuf,
    pub direction: Direction,
    #[serde(default)]
    pub filter: Filter,
}
pub fn jobs(dir: &Path) -> Result<BTreeMap<String, SyncJob>, String> {
    match fs::read(dir.join("sync-jobs.json")) {
        Ok(b) => serde_json::from_slice(&b).map_err(err),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(BTreeMap::new()),
        Err(e) => Err(err(e)),
    }
}
pub fn save_job(dir: &Path, job: &SyncJob) -> Result<(), String> {
    if job.name.trim().is_empty() {
        return Err(t("请填写此字段。").into());
    }
    let _lock = lock(dir, "sync-jobs.lock")?;
    let mut all = jobs(dir)?;
    if all.get(&job.name).is_some_and(|old| {
        (
            old.account.as_str(),
            old.bucket.as_str(),
            old.prefix.as_str(),
        ) != (
            job.account.as_str(),
            job.bucket.as_str(),
            job.prefix.as_str(),
        )
    }) {
        return Err(format!("{}: {}", t("任务名称已存在"), job.name));
    }
    all.insert(job.name.clone(), job.clone());
    let mut tmp = tempfile::NamedTempFile::new_in(dir).map_err(err)?;
    serde_json::to_writer_pretty(tmp.as_file_mut(), &all).map_err(err)?;
    tmp.as_file().sync_all().map_err(err)?;
    tmp.persist(dir.join("sync-jobs.json")).map_err(err)?;
    Ok(())
}
fn lock(dir: &Path, name: &str) -> Result<fs::File, String> {
    let f = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(dir.join(name))
        .map_err(err)?;
    f.try_lock().map_err(err)?;
    Ok(f)
}
pub async fn digest(mut reader: impl tokio::io::AsyncRead + Unpin) -> Result<Vec<u8>, String> {
    let mut hash = Sha256::new();
    let mut buf = vec![0; 128 * 1024];
    loop {
        let n = reader.read(&mut buf).await.map_err(err)?;
        if n == 0 {
            break;
        }
        hash.update(&buf[..n]);
    }
    Ok(hash.finalize().to_vec())
}

#[derive(Default, Serialize)]
pub struct SyncResult {
    pub files: Vec<String>,
    pub tasks: Vec<String>,
    pub unchanged: usize,
}
/// Analyze both sides without mutating either. Recheck at execution, then let the
/// transfer engine conditionally commit against the remote revision we compared.
pub async fn sync(
    dir: &Path,
    engine: &Engine,
    store: &AccountStore,
    job: &SyncJob,
    run: bool,
) -> Result<SyncResult, String> {
    let _change = store.connection_changes.lock().await;
    let client = bucket_client(store, &job.account, &job.bucket).await?;
    let namespace = crate::s3::directory_cache_namespace(store, &job.account, &job.bucket).await?;
    sync_with_client(dir, engine, &client, job, run, &namespace).await
}
async fn sync_with_client(
    dir: &Path,
    engine: &Engine,
    client: &Client,
    job: &SyncJob,
    run: bool,
    namespace: &str,
) -> Result<SyncResult, String> {
    let root = root_path(&job.local)?;
    let filter = job.filter.compile()?;
    let scope = serde_json::to_vec(&(&job.account, &job.bucket, &job.prefix, &root, &namespace))
        .map_err(err)?;
    let fingerprint = format!("{:x}", Sha256::digest(scope));
    let cache_path = dir.join(format!("sync-{fingerprint}.json"));
    // Separate graphical and command-line processes can open the same saved job.
    let mut run_lock = if run {
        Some(lock(dir, &format!("sync-{fingerprint}.lock"))?)
    } else {
        None
    };
    // Only reuse byte comparisons made locally against the identical remote revision.
    let verified: BTreeMap<String, (String, Vec<u8>)> = fs::read(&cache_path)
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default();
    let mut next_cache = BTreeMap::new();
    let local = walk(&root)?;
    let remote = files::expand(
        &client,
        &job.bucket,
        &[Selection {
            key: job.prefix.clone(),
            folder: true,
        }],
        &Filter::default(),
    )
    .await?;
    let mut directories = Vec::new();
    if job.direction == Direction::Download {
        let matched: Vec<_> = remote
            .iter()
            .filter(|o| {
                filter.matches(
                    &o.key[job.prefix.len()..],
                    o.size as u64,
                    &o.modified,
                    &o.storage_class,
                )
            })
            .cloned()
            .collect();
        for (o, path) in mapped(&root, &job.prefix, &matched)? {
            if o.folder && !path.is_dir() {
                directories.push((o.key[job.prefix.len()..].to_owned(), path));
            }
        }
    } else if job.filter == Filter::default() {
        let existing: BTreeSet<_> = remote.iter().map(|o| o.key.as_str()).collect();
        for (rel, f) in &local {
            if f.folder {
                let key = format!("{}{rel}", job.prefix);
                if !local
                    .range((
                        std::ops::Bound::Excluded(rel.clone()),
                        std::ops::Bound::Unbounded,
                    ))
                    .next()
                    .is_some_and(|(child, _)| child.starts_with(rel))
                    && !existing
                        .range(key.as_str()..)
                        .next()
                        .is_some_and(|v| v.starts_with(&key))
                {
                    directories.push((rel.clone(), f.path.clone()));
                }
            }
        }
    }
    let remote: BTreeMap<_, _> = remote
        .iter()
        .filter(|o| !o.folder)
        .map(|o| (&o.key[job.prefix.len()..], o))
        .collect();
    let candidates: BTreeSet<&str> = match job.direction {
        Direction::Upload => local
            .iter()
            .filter(|(_, f)| !f.folder)
            .map(|(k, _)| k.as_str())
            .collect(),
        Direction::Download => remote.keys().copied().collect(),
    };
    let mut result = SyncResult::default();
    let mut work = Vec::new();
    for rel in candidates {
        let l = local.get(rel);
        let r = remote.get(rel);
        let key = format!("{}{rel}", job.prefix);
        let (size, date, class) = match job.direction {
            Direction::Upload => {
                let f = l.unwrap();
                (f.size, f.modified.as_str(), "")
            }
            Direction::Download => {
                let o = r.unwrap();
                (o.size as u64, o.modified.as_str(), o.storage_class.as_str())
            }
        };
        if !filter.matches(rel, size, date, class) {
            continue;
        }
        let path = match l {
            Some(f) => f.path.clone(),
            None => local_path(&root, rel)?,
        };
        let mut condition = "*".to_owned();
        let mut version = None;
        if let Some(r) = r {
            let h = client
                .head_object()
                .bucket(&job.bucket)
                .key(&key)
                .send()
                .await
                .map_err(err)?;
            condition = h.e_tag().ok_or(t("服务端分片缺少 ETag。"))?.into();
            version = h.version_id().map(str::to_owned);
            if l.is_some_and(|l| l.size == r.size as u64) && h.content_length() == Some(r.size) {
                let local_hash = digest(tokio::fs::File::open(&path).await.map_err(err)?).await?;
                let revision = serde_json::to_string(&(
                    h.version_id(),
                    h.e_tag(),
                    h.last_modified().map(ToString::to_string),
                    h.content_length(),
                ))
                .map_err(err)?;
                let checked = (revision, local_hash.clone());
                if verified.get(rel) == Some(&checked) {
                    next_cache.insert(rel.to_owned(), checked);
                    result.unchanged += 1;
                    continue;
                }
                let read = client
                    .get_object()
                    .bucket(&job.bucket)
                    .key(&key)
                    .if_match(&condition)
                    .set_version_id(version.clone())
                    .send()
                    .await
                    .map_err(err)?;
                if digest(read.body.into_async_read()).await? == local_hash {
                    next_cache.insert(rel.to_owned(), checked);
                    result.unchanged += 1;
                    continue;
                }
            }
        }
        result.files.push(rel.to_owned());
        work.push((
            key,
            path,
            version,
            condition,
            l.map(|f| (f.size, f.modified.clone())),
        ));
    }
    if let Ok(mut tmp) = tempfile::NamedTempFile::new_in(dir) {
        if serde_json::to_writer(tmp.as_file_mut(), &next_cache).is_ok() {
            let _ = tmp.persist(&cache_path);
        }
    }
    result
        .files
        .extend(directories.iter().map(|(rel, _)| rel.clone()));
    if run {
        for (rel, path) in directories {
            if job.direction == Direction::Upload {
                mkdir_remote(&client, &job.bucket, &format!("{}{rel}", job.prefix)).await?;
            } else {
                fs::create_dir_all(path).map_err(err)?;
            }
        }
        // Keep the lock until the queued transfers settle, including partial failures.
        let (queued, mut collect) = tokio::sync::mpsc::unbounded_channel::<String>();
        let guard = run_lock.take();
        let worker = engine.clone();
        tokio::spawn(async move {
            let _guard = guard;
            let mut ids = BTreeSet::new();
            while let Some(id) = collect.recv().await {
                ids.insert(id);
            }
            while worker.list().await.iter().any(|t| {
                ids.contains(&t.id)
                    && matches!(
                        t.status.as_str(),
                        "queued" | "preparing" | "running" | "pausing" | "finalizing"
                    )
            }) {
                tokio::time::sleep(std::time::Duration::from_millis(200)).await;
            }
        });
        for (key, path, version, condition, observed) in work {
            let meta = match fs::symlink_metadata(&path) {
                Ok(m) if m.is_file() => Some((
                    m.len(),
                    chrono::DateTime::<chrono::Utc>::from(m.modified().map_err(err)?).to_rfc3339(),
                )),
                Err(e) if e.kind() == io::ErrorKind::NotFound => None,
                _ => return Err(format!("{}: {}", t("保存路径无效。"), path.display())),
            };
            if observed != meta {
                return Err(format!(
                    "{}: {}",
                    t("本地文件已变化，请重新比较目录"),
                    path.display()
                ));
            }
            if job.direction == Direction::Download {
                fs::create_dir_all(path.parent().ok_or(t("保存路径无效。"))?).map_err(err)?;
            }
            let id = engine
                .enqueue_pinned(
                    job.direction,
                    job.account.clone(),
                    job.bucket.clone(),
                    key.clone(),
                    path,
                    if job.direction == Direction::Download {
                        version
                    } else {
                        None
                    },
                    Some(condition),
                )
                .await
                .map_err(|e| format!("{key}: {e}"))?;
            engine.start(&id, client.clone()).await?;
            let _ = queued.send(id.clone());
            result.tasks.push(id);
        }
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    fn file(key: &str) -> S3Object {
        S3Object {
            key: key.into(),
            size: 1,
            folder: false,
            modified: String::new(),
            storage_class: String::new(),
        }
    }
    #[test]
    fn bulk_download_preflights_paths_and_collisions() {
        let root = tempfile::tempdir().unwrap();
        for keys in [
            vec!["p/a", "p/../outside"],
            vec!["p/a", "p/a/b"],
            vec!["p/File", "p/file"],
            vec!["p/con.txt"],
            vec!["p/é", "p/e\u{301}"],
        ] {
            let objects: Vec<_> = keys.into_iter().map(file).collect();
            assert!(mapped(root.path(), "p/", &objects).is_err());
        }
        let plan = mapped(root.path(), "p/", &[file("p/a/b")])
            .unwrap()
            .pop()
            .unwrap()
            .1;
        assert_eq!(plan, root.path().join("a").join("b"));
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(root.path(), root.path().join("link")).unwrap();
            assert!(mapped(root.path(), "p/", &[file("p/link/file")]).is_err());
        }
        assert!(!root.path().join("a").exists());
    }
    #[tokio::test]
    async fn sync_compares_equal_sizes_and_caches_verified_content() {
        let root = tempfile::tempdir().unwrap();
        let state = tempfile::tempdir().unwrap();
        fs::write(root.path().join("a"), b"aaaa").unwrap();
        fs::write(root.path().join("b"), b"bbbb").unwrap();
        let server = tiny_http::Server::http("127.0.0.1:0").unwrap();
        let client = crate::s3::tests::test_client(format!("http://{}", server.server_addr()));
        let worker = std::thread::spawn(move || {
            let requests = [
                "GET /bucket/?list",
                "HEAD /bucket/a",
                "GET /bucket/a",
                "HEAD /bucket/b",
                "GET /bucket/b",
                "GET /bucket/?list",
                "HEAD /bucket/a",
                "GET /bucket/a",
                "HEAD /bucket/b",
            ];
            for expected in requests {
                let req = server
                    .recv_timeout(std::time::Duration::from_secs(5))
                    .unwrap()
                    .unwrap();
                let actual = format!("{} {}", req.method().as_str(), req.url());
                assert!(actual.starts_with(expected), "{expected} != {actual}");
                let body = if expected.ends_with("?list") {
                    "<ListBucketResult><IsTruncated>false</IsTruncated><Contents><Key>a</Key><Size>4</Size></Contents><Contents><Key>b</Key><Size>4</Size></Contents></ListBucketResult>"
                } else if expected.ends_with("/a") {
                    "old!"
                } else {
                    "bbbb"
                };
                req.respond(
                    tiny_http::Response::from_string(body)
                        .with_header(tiny_http::Header::from_bytes("ETag", "\"test\"").unwrap()),
                )
                .unwrap();
            }
        });
        let engine = Engine::open(state.path().join("queue.json"), Arc::new(|_| {})).unwrap();
        let job = SyncJob {
            name: "test".into(),
            account: "a".into(),
            bucket: "bucket".into(),
            prefix: "".into(),
            local: root.path().into(),
            direction: Direction::Upload,
            filter: Filter::default(),
        };
        for _ in 0..2 {
            let plan = sync_with_client(
                state.path(),
                &engine,
                &client,
                &job,
                false,
                "account-revision",
            )
            .await
            .unwrap();
            assert_eq!(plan.files, ["a"]);
            assert_eq!(plan.unchanged, 1);
            assert!(plan.tasks.is_empty());
        }
        worker.join().unwrap();
    }
}

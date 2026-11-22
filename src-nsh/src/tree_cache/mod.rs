mod db;
#[cfg(test)]
mod tests;

use crate::object_tree::{Totals, TreeListing, TreeOptions};
use anyhow::{Context, Result, ensure};
use rusqlite::Connection;
use s3_studio_core::s3::S3Object;
use sha2::{Digest, Sha256};
use std::{
    collections::HashMap,
    fs::{self, File, OpenOptions},
    io::{ErrorKind, Write},
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
};
use tempfile::{NamedTempFile, TempDir, TempPath};
use uuid::Uuid;

pub const CACHE_LIFETIME_MS: i64 = 24 * 60 * 60 * 1000;
const CACHE_EXTENSION: &str = "sqlite3";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TreeScope {
    pub account: String,
    pub bucket: String,
    pub prefix: String,
    pub namespace: String,
}

impl TreeScope {
    pub fn new(account: String, bucket: String, prefix: String) -> Self {
        Self {
            account,
            bucket,
            prefix,
            namespace: String::new(),
        }
    }

    fn same_bucket(&self, scope: &Self) -> bool {
        self.account == scope.account
            && self.bucket == scope.bucket
            && self.namespace == scope.namespace
    }
}

pub enum TreeChange {
    Upsert(S3Object),
    Delete {
        keys: Vec<String>,
        folders: Vec<String>,
    },
    Unknown {
        keys: Vec<String>,
        folders: Vec<String>,
    },
}

impl TreeChange {
    pub fn overlaps(&self, prefix: &str) -> bool {
        match self {
            Self::Upsert(item) => item.key.starts_with(prefix),
            Self::Delete { keys, folders } | Self::Unknown { keys, folders } => {
                keys.iter().any(|key| key.starts_with(prefix))
                    || folders
                        .iter()
                        .any(|folder| folder.starts_with(prefix) || prefix.starts_with(folder))
            }
        }
    }
}

struct RunningScan {
    scope: TreeScope,
    epoch: String,
    changes: Vec<Arc<TreeChange>>,
}

pub struct TreeCache {
    root: PathBuf,
    _temporary: Option<TempDir>,
    // Cache updates following completed S3 mutations must be ordered with each other.
    pub mutations: tokio::sync::Mutex<()>,
    scans: Mutex<HashMap<PathBuf, RunningScan>>,
}

impl TreeCache {
    pub fn new(root: PathBuf) -> Self {
        Self {
            root,
            _temporary: None,
            mutations: tokio::sync::Mutex::new(()),
            scans: Mutex::default(),
        }
    }

    pub fn preview() -> Result<Self> {
        let tmp = tempfile::tempdir()?;
        let mut cache = Self::new(tmp.path().to_owned());
        cache._temporary = Some(tmp);
        Ok(cache)
    }

    fn lock(&self, write: bool) -> Result<File> {
        fs::create_dir_all(&self.root)?;
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(self.root.join("access.lock"))?;
        if write {
            file.lock()?
        } else {
            file.lock_shared()?
        };
        Ok(file)
    }

    fn account_path(&self, account: &str) -> PathBuf {
        self.root.join(format!("account-{}", hash(account)))
    }

    fn bucket_path(&self, scope: &TreeScope) -> PathBuf {
        self.account_path(&scope.account)
            .join(hash(&scope.bucket))
            .join(hash(&scope.namespace))
    }

    fn open_snapshot(&self, scope: &TreeScope) -> Result<Option<(Connection, db::Metadata, bool)>> {
        let bucket = self.bucket_path(scope);
        let epoch = read_epoch(&bucket)?;
        let mut best = None;
        let mut best_order = None;
        for prefix in [""].into_iter().chain(
            scope
                .prefix
                .match_indices('/')
                .map(|(at, _)| &scope.prefix[..at + 1]),
        ) {
            let path = snapshot(&bucket, prefix);
            if !path.is_file() {
                continue;
            }
            let Ok(conn) = db::open(&path, true) else {
                continue;
            };
            let Ok(meta) = db::metadata(&conn) else {
                continue;
            };
            if meta.prefix != prefix || !db::covers(&conn, &scope.prefix).unwrap_or(false) {
                continue;
            }
            let stale_epoch = epoch.as_deref() != Some(&meta.epoch);
            let order = (
                !stale_epoch && !meta.dirty && !expired(meta.scanned_at),
                meta.scanned_at,
            );
            if best_order.is_none_or(|other| order >= other) {
                best_order = Some(order);
                best = Some((conn, meta, stale_epoch));
            }
        }
        Ok(best)
    }

    pub fn available(&self, scope: &TreeScope) -> Result<bool> {
        let _lock = self.lock(false)?;
        Ok(self.open_snapshot(scope)?.is_some())
    }

    pub fn rows(
        &self,
        scope: &TreeScope,
        options: &TreeOptions,
        cancel: Arc<AtomicBool>,
    ) -> Result<Option<TreeListing>> {
        let _lock = self.lock(false)?;
        ensure!(!cancel.load(Ordering::Relaxed), "query canceled");
        let Some((conn, meta, stale_epoch)) = self.open_snapshot(scope)? else {
            return Ok(None);
        };
        db::interruptible(&conn, cancel);
        db::listing(&conn, &scope.prefix, meta, stale_epoch, options)
    }

    pub fn begin(self: &Arc<Self>, scope: TreeScope, cancel: Arc<AtomicBool>) -> Result<TreeScan> {
        ensure!(
            scope.prefix.is_empty() || scope.prefix.ends_with('/'),
            "invalid directory prefix"
        );
        let _lock = self.lock(true)?;
        ensure!(!cancel.load(Ordering::Relaxed), "scan canceled");
        let bucket = self.bucket_path(&scope);
        fs::create_dir_all(&bucket)?;
        let epoch = match read_epoch(&bucket)? {
            Some(epoch) => epoch,
            None => advance_epoch(&bucket)?,
        };
        let staging = self.root.join("staging");
        fs::create_dir_all(&staging)?;
        remove_abandoned_staging(&staging);
        let path = NamedTempFile::new_in(staging)?.into_temp_path();
        let scanned_at = chrono::Utc::now().timestamp_millis();
        let connection = db::create(&path, &scope.prefix, scanned_at, &epoch)?;
        db::interruptible(&connection, cancel.clone());
        self.scans.lock().unwrap().insert(
            path.to_path_buf(),
            RunningScan {
                scope: scope.clone(),
                epoch,
                changes: Vec::new(),
            },
        );
        Ok(TreeScan {
            cache: self.clone(),
            path: Some(path),
            scope,
            cancel,
            scanned_at,
            connection: Some(connection),
            folders: HashMap::new(),
            scanned: Totals::default(),
        })
    }

    pub fn wants_key(&self, scope: &TreeScope, key: &str) -> bool {
        let Ok(_lock) = self.lock(false) else {
            return false;
        };
        let bucket = self.bucket_path(scope);
        [""].into_iter()
            .chain(key.match_indices('/').map(|(at, _)| &key[..at + 1]))
            .any(|prefix| snapshot(&bucket, prefix).is_file())
            || self.scans.lock().unwrap().values().any(|running| {
                running.scope.same_bucket(scope) && key.starts_with(&running.scope.prefix)
            })
    }

    pub fn update(&self, scope: &TreeScope, change: TreeChange) -> Result<()> {
        let _lock = self.lock(true)?;
        let bucket = self.bucket_path(scope);
        if !bucket.is_dir() {
            return Ok(());
        }
        let previous = read_epoch(&bucket)?;
        // A file whose epoch does not match is visibly stale after a crash or an I/O error.
        let epoch = advance_epoch(&bucket)?;
        let change = Arc::new(change);
        for running in self.scans.lock().unwrap().values_mut() {
            if running.scope.same_bucket(scope) && previous.as_deref() == Some(&running.epoch) {
                if change.overlaps(&running.scope.prefix) {
                    running.changes.push(change.clone());
                }
                running.epoch.clone_from(&epoch);
            }
        }

        let mut first_error = None;
        for entry in fs::read_dir(&bucket)? {
            let path = entry?.path();
            if !is_snapshot(&path) {
                continue;
            }
            let result = (|| -> Result<()> {
                let mut conn = db::open(&path, false)?;
                let meta = db::metadata(&conn)?;
                let overlaps = change.overlaps(&meta.prefix);
                let dirty = meta.dirty
                    || previous.as_deref() != Some(&meta.epoch)
                    || (overlaps && matches!(change.as_ref(), TreeChange::Unknown { .. }));
                let tx = conn.transaction()?;
                if overlaps {
                    db::apply(&tx, &meta.prefix, &change)?;
                }
                db::set_epoch(&tx, &epoch, dirty)?;
                tx.commit()?;
                Ok(())
            })();
            if let Err(error) = result {
                first_error.get_or_insert(error);
            }
        }
        first_error.map_or(Ok(()), Err)
    }

    pub fn invalidate(&self, account: Option<&str>, bucket: Option<&str>) -> Result<()> {
        let _lock = self.lock(true)?;
        if let Some(account) = account {
            let path = self.account_path(account);
            let path = bucket.map_or(path.clone(), |bucket| path.join(hash(bucket)));
            return remove_directory(&path);
        }
        for entry in fs::read_dir(&self.root)? {
            let entry = entry?;
            if entry.file_name().to_string_lossy().starts_with("account-") {
                remove_directory(&entry.path())?;
            }
        }
        Ok(())
    }
}

pub struct TreeScan {
    cache: Arc<TreeCache>,
    path: Option<TempPath>,
    scope: TreeScope,
    cancel: Arc<AtomicBool>,
    scanned_at: i64,
    connection: Option<Connection>,
    folders: HashMap<String, i64>,
    scanned: Totals,
}

impl TreeScan {
    pub fn append(&mut self, objects: &[S3Object]) -> Result<Totals> {
        ensure!(!self.cancel.load(Ordering::Relaxed), "scan canceled");
        self.scanned += db::append(
            self.connection.as_mut().unwrap(),
            &self.scope.prefix,
            objects,
            &mut self.folders,
        )?;
        Ok(self.scanned)
    }

    pub fn publish(mut self) -> Result<()> {
        let mut conn = self.connection.take().unwrap();
        db::finish(&conn)?;
        let _lock = self.cache.lock(true)?;
        ensure!(!self.cancel.load(Ordering::Relaxed), "scan canceled");
        let bucket = self.cache.bucket_path(&self.scope);
        let path = self.path.as_ref().unwrap();
        let running = self
            .cache
            .scans
            .lock()
            .unwrap()
            .remove(path.as_ref() as &Path)
            .context("missing directory scan")?;
        ensure!(
            read_epoch(&bucket)?.as_deref() == Some(&running.epoch),
            crate::i18n::t("扫描期间缓存已变更，请重新扫描。")
        );

        let target = snapshot(&bucket, &self.scope.prefix);
        if let Ok(old) = db::open(&target, true).and_then(|conn| db::metadata(&conn))
            && old.scanned_at > self.scanned_at
            && old.scanned_at <= chrono::Utc::now().timestamp_millis()
        {
            return Ok(());
        }
        let dirty = running
            .changes
            .iter()
            .any(|change| matches!(change.as_ref(), TreeChange::Unknown { .. }));
        let tx = conn.transaction()?;
        for change in running.changes {
            db::apply(&tx, &self.scope.prefix, &change)?;
        }
        db::set_epoch(&tx, &running.epoch, dirty)?;
        tx.commit()?;
        drop(conn);
        ensure!(!self.cancel.load(Ordering::Relaxed), "scan canceled");
        // The temporary file lives on the same volume. Replace the previous
        // complete snapshot only after the new index and its changes are durable.
        OpenOptions::new().write(true).open(path)?.sync_all()?;
        self.path.take().unwrap().persist(&target)?;
        if !dirty {
            remove_covered(&bucket, &target, &self.scope.prefix, self.scanned_at);
        }
        Ok(())
    }
}

impl Drop for TreeScan {
    fn drop(&mut self) {
        drop(self.connection.take());
        if let Some(path) = &self.path {
            self.cache
                .scans
                .lock()
                .unwrap()
                .remove(path.as_ref() as &Path);
        }
    }
}

pub fn expired(scanned_at: i64) -> bool {
    let time = chrono::Utc::now().timestamp_millis();
    scanned_at > time || time.saturating_sub(scanned_at) >= CACHE_LIFETIME_MS
}

fn hash(text: &str) -> String {
    // Keep nested SQLite paths within the ordinary Windows path limit.
    let mut hash = format!("{:x}", Sha256::digest(text.as_bytes()));
    hash.truncate(32);
    hash
}

fn snapshot(bucket: &Path, prefix: &str) -> PathBuf {
    bucket.join(hash(prefix)).with_extension(CACHE_EXTENSION)
}

fn is_snapshot(path: &Path) -> bool {
    path.extension()
        .is_some_and(|extension| extension == CACHE_EXTENSION)
}

fn read_epoch(bucket: &Path) -> Result<Option<String>> {
    match fs::read_to_string(bucket.join("epoch")) {
        Ok(data) => Ok(Some(data)),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error.into()),
    }
}

fn advance_epoch(bucket: &Path) -> Result<String> {
    let epoch = Uuid::new_v4().to_string();
    let mut temp = NamedTempFile::new_in(bucket)?;
    temp.write_all(epoch.as_bytes())?;
    temp.as_file().sync_all()?;
    temp.persist(bucket.join("epoch"))?;
    Ok(epoch)
}

fn remove_directory(path: &Path) -> Result<()> {
    match fs::remove_dir_all(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.into()),
    }
}

fn remove_covered(bucket: &Path, current: &Path, prefix: &str, scanned_at: i64) {
    if let Ok(entries) = fs::read_dir(bucket) {
        for path in entries.flatten().map(|entry| entry.path()) {
            if path != current
                && is_snapshot(&path)
                && let Ok(meta) = db::open(&path, true).and_then(|conn| db::metadata(&conn))
                && meta.prefix.starts_with(prefix)
                && meta.scanned_at <= scanned_at
            {
                let _ = fs::remove_file(path);
            }
        }
    }
}

fn remove_abandoned_staging(staging: &Path) {
    if let Ok(entries) = fs::read_dir(staging) {
        for entry in entries.flatten() {
            if let Ok(age) = entry
                .metadata()
                .and_then(|meta| meta.modified())
                .map(|time| time.elapsed())
                && age.is_ok_and(|elapsed| elapsed.as_secs() >= 7 * 24 * 60 * 60)
            {
                let _ = fs::remove_file(entry.path());
            }
        }
    }
}

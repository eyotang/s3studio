use crate::{
    object_tree::{Totals, TreeListing, TreeOptions},
    tree_cache::{TreeCache, TreeChange, TreeScope},
};
use futures::channel::mpsc::{UnboundedReceiver, UnboundedSender, unbounded};
use s3_studio_core::{
    accounts::{self, AccountStore},
    s3, transfer_commands as transfers,
    transfers::{Direction, Engine, Transfer},
};
use serde::Serialize;
use serde_json::{Value, json};
use std::{
    collections::{HashMap, HashSet},
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

pub enum Message {
    Response(u64, Result<Value, String>),
    TreeScanning(u64),
    TreeProgress(u64, Totals),
    TreeReady(u64, Result<(), String>),
    ListSizes(u64, Result<HashMap<String, u128>, String>),
    TreeRows(u64, Result<Option<TreeListing>, String>),
    TreeUpload(TreeScope, String),
    Transfer(Transfer),
    Update(crate::updater::Status),
    Restart(s3_studio_core::transfers::UpdateGuard),
}
struct Services {
    store: Arc<AccountStore>,
    engine: Engine,
    cache: Arc<TreeCache>,
}
#[derive(Clone)]
pub struct Backend {
    runtime: Arc<tokio::runtime::Runtime>,
    services: Option<Arc<Services>>,
    cache: Arc<TreeCache>,
    sender: UnboundedSender<Message>,
    pub preview: bool,
    preview_defaults: Arc<Mutex<accounts::S3Defaults>>,
    updater: Arc<crate::updater::UpdaterState>,
    #[cfg(test)]
    preview_scan_gate: Option<Arc<tokio::sync::Semaphore>>,
}

pub struct TreeTask {
    cancel: Arc<AtomicBool>,
    task: tokio::task::AbortHandle,
}

impl Drop for TreeTask {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
        self.task.abort();
    }
}

impl Backend {
    pub fn open(preview: bool) -> anyhow::Result<(Self, UnboundedReceiver<Message>)> {
        let runtime = Arc::new(tokio::runtime::Runtime::new()?);
        let (sender, receiver) = unbounded();
        let (services, cache) = if preview {
            (None, Arc::new(TreeCache::preview()?))
        } else {
            let dir = data_directory()?;
            std::fs::create_dir_all(&dir)?;
            let store = Arc::new(
                AccountStore::open(dir.join("accounts.json"))
                    .map_err(|e| anyhow::anyhow!(e.to_string()))?,
            );
            let cache_dir = dirs::cache_dir()
                .map(|dir| dir.join("com.s3studio.desktop"))
                .unwrap_or_else(|| dir.join("cache"));
            let cache = Arc::new(TreeCache::new(cache_dir.join("object-tree-v1")));
            let sink = sender.clone();
            let handle = runtime.handle().clone();
            let upload_cache = cache.clone();
            let upload_store = store.clone();
            let engine = Engine::open(
                dir.join("transfers.json"),
                Arc::new(move |t| {
                    if t.status == "done" && t.direction == Direction::Upload {
                        let scope =
                            TreeScope::new(t.account_id.clone(), t.bucket.clone(), String::new());
                        let (store, cache, sink, key) = (
                            upload_store.clone(),
                            upload_cache.clone(),
                            sink.clone(),
                            t.key.clone(),
                        );
                        handle.spawn(async move {
                            let _changes = cache.mutations.lock().await;
                            if let Ok(scope) = cache_scope(Some(&store), scope).await
                                && tokio::task::block_in_place(|| cache.wants_key(&scope, &key))
                            {
                                update_cached_object(&store, &cache, &scope, key.clone()).await;
                                let _ = sink.unbounded_send(Message::TreeUpload(scope, key));
                            }
                        });
                    }
                    let _ = sink.unbounded_send(Message::Transfer(t));
                }),
            )
            .map_err(|e| anyhow::anyhow!(e.to_string()))?;
            (
                Some(Arc::new(Services {
                    store,
                    engine,
                    cache: cache.clone(),
                })),
                cache,
            )
        };
        let updater = crate::updater::UpdaterState::new(preview, sender.clone());
        let _ = sender.unbounded_send(Message::Update(updater.snapshot()));
        runtime.spawn(crate::updater::schedule(updater.clone()));
        Ok((
            Self {
                runtime,
                services,
                cache,
                sender,
                preview,
                preview_defaults: Default::default(),
                updater,
                #[cfg(test)]
                preview_scan_gate: None,
            },
            receiver,
        ))
    }
    #[cfg(test)]
    pub fn event_sender(&self) -> UnboundedSender<Message> {
        self.sender.clone()
    }

    #[cfg(test)]
    pub fn hold_preview_scan(&mut self, gate: Arc<tokio::sync::Semaphore>) {
        self.preview_scan_gate = Some(gate);
    }
    pub fn request(&self, id: u64, command: &'static str, args: Value) {
        let services = self.services.clone();
        let sender = self.sender.clone();
        let updater = self.updater.clone();
        let preview_defaults = self.preview_defaults.clone();
        self.runtime.spawn(async move {
            let result = match command {
                "get_app_update_status" => {
                    let _ = sender.unbounded_send(Message::Update(updater.snapshot()));
                    Ok(Value::Null)
                }
                "check_app_update" => crate::updater::check(updater.clone(), true)
                    .await
                    .map(|_| Value::Null),
                "install_app_update" => match &services {
                    Some(service) => {
                        crate::updater::install_app_update(updater.clone(), &service.engine)
                            .await
                            .map(|_| Value::Null)
                    }
                    None => Err(crate::i18n::t(
                        "开发模式与网页预览不检查更新。安装发布版后即可使用。",
                    )
                    .into()),
                },
                "open_update_download_page" => crate::updater::download_page(&updater.snapshot())
                    .and_then(|url| {
                        open::that(url)
                            .map(|_| Value::Null)
                            .map_err(|e| e.to_string())
                    }),
                _ => match services {
                    Some(services) => dispatch(&services, command, args).await,
                    None => preview(command, args, &mut preview_defaults.lock().unwrap()),
                },
            };
            let _ = sender.unbounded_send(Message::Response(id, result));
        });
    }

    pub fn scan_tree(&self, revision: u64, scope: TreeScope, force: bool) -> TreeTask {
        let services = self.services.clone();
        let cache = self.cache.clone();
        let sender = self.sender.clone();
        let cancel = Arc::new(AtomicBool::new(false));
        let scan_cancel = cancel.clone();
        #[cfg(test)]
        let preview_gate = self.preview_scan_gate.clone();
        let task = self
            .runtime
            .spawn(async move {
                let result = async {
                    let scope = cache_scope(
                        services.as_ref().map(|service| service.store.as_ref()),
                        scope,
                    )
                    .await?;
                    if !force {
                        let (cache, scope) = (cache.clone(), scope.clone());
                        if cache_job(move || cache.available(&scope)).await? {
                            return Ok(());
                        }
                    }
                    let _ = sender.unbounded_send(Message::TreeScanning(revision));
                    let (work_cache, work_scope) = (cache.clone(), scope.clone());
                    let mut scan =
                        cache_job(move || work_cache.begin(work_scope, scan_cancel)).await?;
                    let mut last_report: Option<Instant> = None;
                    if let Some(services) = services {
                        s3::scan_objects(
                            &services.store,
                            &scope.account,
                            &scope.bucket,
                            &scope.prefix,
                            |objects| {
                                let progress =
                                    tokio::task::block_in_place(|| scan.append(&objects))
                                        .map_err(cache_error)?;
                                if last_report
                                    .is_none_or(|last| last.elapsed() >= Duration::from_millis(200))
                                {
                                    last_report = Some(Instant::now());
                                    let _ = sender
                                        .unbounded_send(Message::TreeProgress(revision, progress));
                                }
                                Ok(())
                            },
                        )
                        .await?;
                    } else {
                        #[cfg(test)]
                        if let Some(gate) = preview_gate {
                            gate.acquire().await.map_err(cache_error)?.forget();
                        }
                        tokio::task::block_in_place(|| {
                            scan.append(&preview_objects(&scope.prefix, true))
                        })
                        .map_err(cache_error)?;
                    }
                    cache_job(move || scan.publish()).await
                }
                .await;
                let _ = sender.unbounded_send(Message::TreeReady(revision, result));
            })
            .abort_handle();
        TreeTask { cancel, task }
    }

    pub fn query_list_sizes(&self, revision: u64, scope: TreeScope, keys: Vec<String>) -> TreeTask {
        let (cache, sender, services) = (
            self.cache.clone(),
            self.sender.clone(),
            self.services.clone(),
        );
        let cancel = Arc::new(AtomicBool::new(false));
        let query_cancel = cancel.clone();
        let task = self
            .runtime
            .spawn(async move {
                let result = async {
                    let scope = cache_scope(
                        services.as_ref().map(|service| service.store.as_ref()),
                        scope,
                    )
                    .await?;
                    cache_job(move || cache.folder_sizes(&scope, &keys, query_cancel)).await
                }
                .await;
                let _ = sender.unbounded_send(Message::ListSizes(revision, result));
            })
            .abort_handle();
        TreeTask { cancel, task }
    }

    pub fn query_tree(&self, revision: u64, scope: TreeScope, options: TreeOptions) -> TreeTask {
        let (cache, sender) = (self.cache.clone(), self.sender.clone());
        let services = self.services.clone();
        let cancel = Arc::new(AtomicBool::new(false));
        let query_cancel = cancel.clone();
        let task = self
            .runtime
            .spawn(async move {
                if !options.search.is_empty() {
                    tokio::time::sleep(Duration::from_millis(140)).await;
                }
                let result = async {
                    let scope = cache_scope(
                        services.as_ref().map(|service| service.store.as_ref()),
                        scope,
                    )
                    .await?;
                    cache_job(move || cache.rows(&scope, &options, query_cancel)).await
                }
                .await;
                let _ = sender.unbounded_send(Message::TreeRows(revision, result));
            })
            .abort_handle();
        TreeTask { cancel, task }
    }
}

async fn cache_scope(
    store: Option<&AccountStore>,
    mut scope: TreeScope,
) -> Result<TreeScope, String> {
    if let Some(store) = store {
        scope.namespace =
            s3::directory_cache_namespace(store, &scope.account, &scope.bucket).await?;
    }
    Ok(scope)
}

fn cache_error(error: impl std::fmt::Display) -> String {
    crate::lf!("目录缓存操作失败：{}", error)
}

async fn cache_job<T: Send + 'static>(
    job: impl FnOnce() -> anyhow::Result<T> + Send + 'static,
) -> Result<T, String> {
    tokio::task::spawn_blocking(job)
        .await
        .map_err(cache_error)?
        .map_err(cache_error)
}

async fn record_tree_change(cache: &Arc<TreeCache>, scope: &TreeScope, change: TreeChange) {
    let (cache, scope) = (cache.clone(), scope.clone());
    if let Err(error) = cache_job(move || cache.update(&scope, change)).await {
        eprintln!("{error}");
    }
}

async fn update_cached_object(
    store: &AccountStore,
    cache: &Arc<TreeCache>,
    scope: &TreeScope,
    key: String,
) {
    let change =
        match s3::object_listing_entry(store, &scope.account, &scope.bucket, key.clone()).await {
            Ok(entry) => TreeChange::Upsert(entry),
            Err(_) => TreeChange::Unknown {
                keys: vec![key],
                folders: vec![],
            },
        };
    record_tree_change(cache, scope, change).await;
}

async fn invalidate_tree_cache(
    cache: &Arc<TreeCache>,
    account: Option<String>,
    bucket: Option<String>,
) {
    let cache = cache.clone();
    if let Err(error) =
        cache_job(move || cache.invalidate(account.as_deref(), bucket.as_deref())).await
    {
        eprintln!("{error}");
    }
}

fn data_directory() -> anyhow::Result<PathBuf> {
    // Keep the data directory and keyring service used by earlier installations.
    Ok(dirs::data_dir()
        .ok_or_else(|| anyhow::anyhow!(crate::i18n::t("无法定位应用数据目录")))?
        .join("com.s3studio.desktop"))
}
fn value<T: Serialize>(result: Result<T, String>) -> Result<Value, String> {
    serde_json::to_value(result?).map_err(|e| e.to_string())
}
async fn dispatch(service: &Services, command: &str, args: Value) -> Result<Value, String> {
    let text = |key: &str| args[key].as_str().unwrap_or_default().to_owned();
    let store = service.store.as_ref();
    let engine = &service.engine;
    let cache = &service.cache;
    let account = text("accountId");
    let bucket = text("bucket");
    match command {
        "get_s3_defaults" => value(Ok(accounts::get_s3_defaults(store).await)),
        "set_s3_defaults" => {
            let _changes = cache.mutations.lock().await;
            let previous = accounts::get_s3_defaults(store).await;
            let defaults = accounts::set_s3_defaults(
                store,
                serde_json::from_value(args["input"].clone()).map_err(|e| e.to_string())?,
            )
            .await?;
            if previous.endpoint != defaults.endpoint || previous.region != defaults.region {
                invalidate_tree_cache(cache, None, None).await;
            }
            value(Ok(defaults))
        }
        "add_s3_option" => value(
            accounts::add_s3_option(
                store,
                serde_json::from_value(args["field"].clone()).map_err(|e| e.to_string())?,
                text("value"),
            )
            .await,
        ),
        "list_accounts" => value(accounts::list_accounts(store).await),
        "reorder_accounts" => {
            value(accounts::reorder_accounts(store, text("source"), text("target")).await)
        }
        "save_account" => {
            let _changes = cache.mutations.lock().await;
            let account = accounts::save_account(
                store,
                serde_json::from_value(args["input"].clone()).map_err(|e| e.to_string())?,
            )
            .await?;
            invalidate_tree_cache(cache, Some(account.id.clone()), None).await;
            value(Ok(account))
        }
        "remove_account" => {
            let _changes = cache.mutations.lock().await;
            accounts::remove_account(store, engine, account.clone()).await?;
            invalidate_tree_cache(cache, Some(account), None).await;
            value(Ok(()))
        }
        "list_buckets" => value(s3::list_buckets(store, account).await),
        "list_objects" => value(
            s3::list_objects(
                store,
                account,
                bucket,
                text("prefix"),
                args["token"].as_str().map(str::to_owned),
            )
            .await,
        ),
        "list_tree_level" => value(
            s3::list_tree_level(
                store,
                account,
                bucket,
                text("prefix"),
                args["token"].as_str().map(str::to_owned),
            )
            .await,
        ),
        "locate_path" => value(
            s3::locate_path(
                store,
                &account,
                &bucket,
                &text("key"),
                args["objectOnly"].as_bool().unwrap_or_default(),
            )
            .await,
        ),
        "create_bucket" => {
            let _changes = cache.mutations.lock().await;
            let name = text("name");
            let result = s3::create_bucket(
                store,
                account.clone(),
                name.clone(),
                serde_json::from_value(args["connection"].clone()).map_err(|e| e.to_string())?,
            )
            .await?;
            invalidate_tree_cache(cache, Some(account), Some(name)).await;
            value(Ok(result))
        }
        "set_bucket_connection" => {
            let _changes = cache.mutations.lock().await;
            let connection = s3::set_bucket_connection(
                store,
                engine,
                account.clone(),
                bucket.clone(),
                serde_json::from_value(args["connection"].clone()).map_err(|e| e.to_string())?,
            )
            .await?;
            invalidate_tree_cache(cache, Some(account), Some(bucket)).await;
            value(Ok(connection))
        }
        "create_folder" => {
            let _changes = cache.mutations.lock().await;
            let key = text("key");
            s3::create_folder(store, account.clone(), bucket.clone(), key.clone()).await?;
            let scope =
                cache_scope(Some(store), TreeScope::new(account, bucket, String::new())).await;
            if let Ok(scope) = scope
                && tokio::task::block_in_place(|| cache.wants_key(&scope, &key))
            {
                record_tree_change(
                    cache,
                    &scope,
                    TreeChange::Upsert(s3::S3Object {
                        key,
                        size: 0,
                        folder: true,
                        modified: String::new(),
                        storage_class: String::new(),
                    }),
                )
                .await;
            }
            value(Ok(()))
        }
        "copy_object" => {
            let _changes = cache.mutations.lock().await;
            let destination = text("destination");
            s3::copy_object(
                store,
                account.clone(),
                bucket.clone(),
                text("key"),
                destination.clone(),
            )
            .await?;
            let scope =
                cache_scope(Some(store), TreeScope::new(account, bucket, String::new())).await;
            if let Ok(scope) = scope
                && tokio::task::block_in_place(|| cache.wants_key(&scope, &destination))
            {
                update_cached_object(store, cache, &scope, destination).await;
            }
            value(Ok(()))
        }
        "list_trash" => value(
            s3::trash::list(
                store,
                account,
                bucket,
                serde_json::from_value(args["cursor"].clone()).map_err(|e| e.to_string())?,
            )
            .await,
        ),
        "restore_trash" => {
            let _changes = cache.mutations.lock().await;
            let objects =
                serde_json::from_value(args["objects"].clone()).map_err(|e| e.to_string())?;
            let result = s3::trash::restore(store, account.clone(), bucket.clone(), objects).await;
            // Even a partial restore can expose data; invalidate both listings and totals.
            invalidate_tree_cache(cache, Some(account), Some(bucket)).await;
            value(result)
        }
        "delete_objects" => {
            let _changes = cache.mutations.lock().await;
            let keys: Vec<String> =
                serde_json::from_value(args["keys"].clone()).map_err(|e| e.to_string())?;
            let folders: Vec<String> =
                serde_json::from_value(args["folders"].clone()).map_err(|e| e.to_string())?;
            let result = s3::delete_objects(
                store,
                account.clone(),
                bucket.clone(),
                keys.clone(),
                folders.clone(),
            )
            .await;
            let scope =
                cache_scope(Some(store), TreeScope::new(account, bucket, String::new())).await;
            let change = if result.is_ok() {
                TreeChange::Delete { keys, folders }
            } else {
                TreeChange::Unknown { keys, folders }
            };
            if let Ok(scope) = scope {
                record_tree_change(cache, &scope, change).await;
            }
            value(result)
        }
        "object_info" => value(s3::object_info(store, account, bucket, text("key")).await),
        "presign_object" => {
            value(s3::presign_object(store, account, bucket, text("key"), 3600).await)
        }
        "upload_files" => {
            value(transfers::upload_files(engine, store, account, bucket, text("prefix")).await)
        }
        "download_object" => {
            value(transfers::download_object(engine, store, account, bucket, text("key")).await)
        }
        "list_transfers" => value(transfers::list_transfers(engine).await),
        "get_transfer_settings" => value(transfers::get_transfer_settings(engine).await),
        "set_transfer_settings" => value(
            transfers::set_transfer_settings(
                engine,
                args["concurrency"].as_u64().unwrap_or_default() as usize,
            )
            .await,
        ),
        "pause_transfer" => value(transfers::pause_transfer(engine, text("transferId")).await),
        "resume_transfer" => {
            value(transfers::resume_transfer(engine, store, text("transferId")).await)
        }
        "cancel_transfer" => {
            value(transfers::cancel_transfer(engine, store, text("transferId")).await)
        }
        "clear_finished_transfers" => value(transfers::clear_finished_transfers(engine).await),
        _ => Err(crate::lf!("未知操作：{command}", command = command)),
    }
}
fn preview(
    command: &str,
    args: Value,
    defaults: &mut accounts::S3Defaults,
) -> Result<Value, String> {
    match command {
        "get_s3_defaults" => value(Ok(&*defaults)),
        "set_s3_defaults" => {
            let input: accounts::S3Defaults =
                serde_json::from_value(args["input"].clone()).map_err(|e| e.to_string())?;
            *defaults = input.normalized()?;
            value(Ok(&*defaults))
        }
        "add_s3_option" => {
            let field: accounts::S3Option =
                serde_json::from_value(args["field"].clone()).map_err(|e| e.to_string())?;
            let option = field.normalize(args["value"].as_str().unwrap_or_default().into())?;
            defaults.add_option(field, option);
            value(Ok(&*defaults))
        }
        "set_bucket_connection" => {
            let connection: accounts::S3Connection =
                serde_json::from_value(args["connection"].clone()).map_err(|e| e.to_string())?;
            value(connection.normalized())
        }
        "list_trash" => Ok(json!({"objects":[
            {"key":"deleted/report.pdf","versionId":"preview-delete","deleted":"2026-11-20T08:00:00Z"}
        ],"cursor":null,"versioning":true})),
        "restore_trash" => Ok(
            json!({"restored": args["objects"].as_array().into_iter().flatten()
            .filter_map(|o| o["key"].as_str()).collect::<Vec<_>>(), "failed":[]}),
        ),
        "reorder_accounts" => Ok(Value::Null),
        "list_accounts" => Ok(json!([
            {"id":"demo-aws","name":"Design workspace","region":"ap-southeast-1","endpoint":"","pathStyle":false},
            {"id":"demo-minio","name":"Local development","region":"us-east-1","endpoint":"http://localhost:9000","pathStyle":true}
        ])),
        "list_buckets" => Ok(json!([
            {"name":"studio-assets","created":"2026-08-16T08:00:00Z"},
            {"name":"application-backups","created":"2026-07-10T08:00:00Z"},
            {"name":"team-documents","created":"2026-06-12T08:00:00Z"}
        ])),
        "list_objects" | "list_tree_level" => {
            let prefix = args["prefix"].as_str().unwrap_or_default();
            let objects = preview_objects(prefix, false);
            #[cfg(test)]
            if command == "list_objects" && prefix == "Brand assets/Guides/" {
                let second_page = args["token"].as_str() == Some("preview-guides-next");
                return value(Ok(s3::ObjectPage {
                    objects: objects
                        .into_iter()
                        .filter(|object| object.key.ends_with("Usage.pdf") == second_page)
                        .collect(),
                    next_token: (!second_page).then(|| "preview-guides-next".into()),
                }));
            }
            value(Ok(s3::ObjectPage {
                objects,
                next_token: None,
            }))
        }
        "locate_path" => value(preview_location(
            args["key"].as_str().unwrap_or_default(),
            args["objectOnly"].as_bool().unwrap_or_default(),
        )),
        "list_transfers" => Ok(json!([])),
        "get_transfer_settings" => Ok(json!({"concurrency":8})),
        "object_info" => Ok(
            json!({"size":2488000,"contentType":"application/octet-stream","etag":"preview-object","modified":"2026-10-09T08:30:00Z","metadata":{}}),
        ),
        _ => Err(crate::i18n::t("当前是界面预览。请在桌面应用中连接真实账户。").into()),
    }
}

fn preview_location(key: &str, object_only: bool) -> Result<s3::PathTarget, String> {
    let objects = preview_objects("", true);
    if !key.is_empty()
        && (object_only || !key.ends_with('/'))
        && let Some(object) = objects.iter().find(|object| object.key == key)
    {
        return Ok(s3::PathTarget::Object {
            object: s3::S3Object {
                folder: false,
                ..object.clone()
            },
            info: Some(s3::ObjectInfo {
                size: object.size,
                content_type: "application/octet-stream".into(),
                etag: "preview-object".into(),
                modified: object.modified.clone(),
                metadata: Default::default(),
            }),
        });
    }
    if !object_only {
        let prefix = if key.is_empty() || key.ends_with('/') {
            key.to_owned()
        } else {
            format!("{key}/")
        };
        if objects.iter().any(|object| object.key.starts_with(&prefix)) {
            return Ok(s3::PathTarget::Directory { prefix });
        }
    }
    Err(crate::i18n::t("找不到指定的目录或对象。").into())
}

fn preview_objects(prefix: &str, recursive: bool) -> Vec<s3::S3Object> {
    let mut seen = HashSet::new();
    [
        ("Brand guidelines.pdf", 2_488_000),
        ("Homepage exploration.fig", 18_400_000),
        ("Mountain retreat.jpg", 6_720_000),
        ("Project manifest.json", 2_140),
        ("Summer collection.zip", 48_700_000),
        ("Welcome.md", 1_820),
        ("Brand assets/Logo.svg", 650_000),
        ("Brand assets/Guides/Usage.pdf", 25_165_824),
        ("Brand assets/Guides/Typography.txt", 4_096),
        ("Photography/Mountain.jpg", 8_388_608),
        ("Product design/Archive/", 0),
        ("Product design/Prototype.fig", 16_777_216),
    ]
    .into_iter()
    .enumerate()
    .filter_map(|(i, (key, size))| {
        let suffix = key.strip_prefix(prefix)?;
        if !recursive && suffix.is_empty() {
            return None;
        }
        let (key, size) = if !recursive && let Some(slash) = suffix.find('/') {
            (&key[..prefix.len() + slash + 1], 0)
        } else {
            (key, size)
        };
        seen.insert(key).then(|| s3::S3Object {
            key: key.to_owned(),
            size,
            folder: size == 0 && key.ends_with('/'),
            modified: format!("2026-10-0{}T08:30:00Z", 9 - i % 5),
            storage_class: "STANDARD".into(),
        })
    })
    .collect()
}

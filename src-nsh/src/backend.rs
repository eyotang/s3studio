use crate::object_tree::{ObjectTree, Totals, TreeBuilder, TreeSort};
use futures::channel::mpsc::{UnboundedReceiver, UnboundedSender, unbounded};
use s3_studio_core::{
    accounts::{self, AccountStore},
    s3, transfer_commands as transfers,
    transfers::{Engine, Transfer},
};
use serde::Serialize;
use serde_json::{Value, json};
use std::{
    collections::HashSet,
    path::PathBuf,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

pub enum Message {
    Response(u64, Result<Value, String>),
    TreeProgress(u64, Totals),
    TreeReady(u64, Result<ObjectTree, String>),
    Transfer(Transfer),
    Update(crate::updater::Status),
    Restart(s3_studio_core::transfers::UpdateGuard),
}
struct Services {
    store: AccountStore,
    engine: Engine,
}
#[derive(Clone)]
pub struct Backend {
    runtime: Arc<tokio::runtime::Runtime>,
    services: Option<Arc<Services>>,
    sender: UnboundedSender<Message>,
    pub preview: bool,
    preview_defaults: Arc<Mutex<accounts::S3Defaults>>,
    updater: Arc<crate::updater::UpdaterState>,
}
impl Backend {
    pub fn open(preview: bool) -> anyhow::Result<(Self, UnboundedReceiver<Message>)> {
        let runtime = Arc::new(tokio::runtime::Runtime::new()?);
        let (sender, receiver) = unbounded();
        let services = if preview {
            None
        } else {
            let dir = data_directory()?;
            std::fs::create_dir_all(&dir)?;
            let store = AccountStore::open(dir.join("accounts.json"))
                .map_err(|e| anyhow::anyhow!(e.to_string()))?;
            let sink = sender.clone();
            let engine = Engine::open(
                dir.join("transfers.json"),
                Arc::new(move |t| {
                    let _ = sink.unbounded_send(Message::Transfer(t));
                }),
            )
            .map_err(|e| anyhow::anyhow!(e.to_string()))?;
            Some(Arc::new(Services { store, engine }))
        };
        let updater = crate::updater::UpdaterState::new(preview, sender.clone());
        let _ = sender.unbounded_send(Message::Update(updater.snapshot()));
        runtime.spawn(crate::updater::schedule(updater.clone()));
        Ok((
            Self {
                runtime,
                services,
                sender,
                preview,
                preview_defaults: Default::default(),
                updater,
            },
            receiver,
        ))
    }
    #[cfg(test)]
    pub fn event_sender(&self) -> UnboundedSender<Message> {
        self.sender.clone()
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

    pub fn scan_tree(
        &self,
        revision: u64,
        account_id: String,
        bucket: String,
        prefix: String,
        sort: (TreeSort, bool),
    ) -> tokio::task::AbortHandle {
        let services = self.services.clone();
        let sender = self.sender.clone();
        self.runtime
            .spawn(async move {
                let mut builder = TreeBuilder::new(&prefix);
                let mut last_report: Option<Instant> = None;
                let result = if let Some(services) = services {
                    s3::scan_objects(&services.store, &account_id, &bucket, &prefix, |objects| {
                        builder.extend(objects);
                        if last_report
                            .is_none_or(|last| last.elapsed() >= Duration::from_millis(200))
                        {
                            last_report = Some(Instant::now());
                            let _ = sender
                                .unbounded_send(Message::TreeProgress(revision, builder.scanned()));
                        }
                    })
                    .await
                } else {
                    builder.extend(preview_objects(&prefix, true));
                    Ok(())
                };
                let result = result.map(|()| builder.finish(sort.0, sort.1));
                let _ = sender.unbounded_send(Message::TreeReady(revision, result));
            })
            .abort_handle()
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
    let store = &service.store;
    let engine = &service.engine;
    let account = text("accountId");
    let bucket = text("bucket");
    match command {
        "get_s3_defaults" => value(Ok(accounts::get_s3_defaults(store).await)),
        "set_s3_defaults" => value(
            accounts::set_s3_defaults(
                store,
                serde_json::from_value(args["input"].clone()).map_err(|e| e.to_string())?,
            )
            .await,
        ),
        "add_s3_option" => value(
            accounts::add_s3_option(
                store,
                serde_json::from_value(args["field"].clone()).map_err(|e| e.to_string())?,
                text("value"),
            )
            .await,
        ),
        "list_accounts" => value(accounts::list_accounts(store).await),
        "save_account" => value(
            accounts::save_account(
                store,
                serde_json::from_value(args["input"].clone()).map_err(|e| e.to_string())?,
            )
            .await,
        ),
        "remove_account" => value(accounts::remove_account(store, engine, account).await),
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
        "create_bucket" => value(
            s3::create_bucket(
                store,
                account,
                text("name"),
                serde_json::from_value(args["connection"].clone()).map_err(|e| e.to_string())?,
            )
            .await,
        ),
        "set_bucket_connection" => value(
            s3::set_bucket_connection(
                store,
                engine,
                account,
                bucket,
                serde_json::from_value(args["connection"].clone()).map_err(|e| e.to_string())?,
            )
            .await,
        ),
        "create_folder" => value(s3::create_folder(store, account, bucket, text("key")).await),
        "copy_object" => {
            value(s3::copy_object(store, account, bucket, text("key"), text("destination")).await)
        }
        "delete_objects" => value(
            s3::delete_objects(
                store,
                account,
                bucket,
                serde_json::from_value(args["keys"].clone()).map_err(|e| e.to_string())?,
                serde_json::from_value(args["folders"].clone()).map_err(|e| e.to_string())?,
            )
            .await,
        ),
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
        "list_accounts" => Ok(json!([
            {"id":"demo-aws","name":"Design workspace","region":"ap-southeast-1","endpoint":"","pathStyle":false},
            {"id":"demo-minio","name":"Local development","region":"us-east-1","endpoint":"http://localhost:9000","pathStyle":true}
        ])),
        "list_buckets" => Ok(json!([
            {"name":"studio-assets","created":"2026-08-16T08:00:00Z"},
            {"name":"application-backups","created":"2026-07-10T08:00:00Z"},
            {"name":"team-documents","created":"2026-06-12T08:00:00Z"}
        ])),
        "list_objects" => {
            let prefix = args["prefix"].as_str().unwrap_or_default();
            value(Ok(s3::ObjectPage {
                objects: preview_objects(prefix, false),
                next_token: None,
            }))
        }
        "list_transfers" => Ok(json!([])),
        "get_transfer_settings" => Ok(json!({"concurrency":8})),
        "object_info" => Ok(
            json!({"size":2488000,"contentType":"application/octet-stream","etag":"preview-object","modified":"2026-10-09T08:30:00Z","metadata":{}}),
        ),
        _ => Err(crate::i18n::t("当前是界面预览。请在桌面应用中连接真实账户。").into()),
    }
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

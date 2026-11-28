use super::*;
use crate::i18n::t;
use s3_studio_core::{
    folders::{self, SyncJob},
    s3::files::{self, Destination, Filter, Properties, Selection},
};

pub(super) async fn dispatch(s: &Services, command: &str, args: Value) -> Result<Value, String> {
    let text = |k: &str| args[k].as_str().unwrap_or("");
    let account = text("accountId");
    let bucket = text("bucket");
    let prefix = text("prefix");
    let key = text("key");
    let dir = data_directory().map_err(|e| e.to_string())?;
    match command {
        "files_jobs" => return value(folders::jobs(&dir)),
        "files_choose_folder" => {
            return Ok(json!(
                rfd::AsyncFileDialog::new()
                    .pick_folder()
                    .await
                    .map(|f| f.path().to_owned())
            ));
        }
        "files_sync_plan" | "files_sync_run" | "files_sync_save" => {
            let job: SyncJob =
                serde_json::from_value(args["job"].clone()).map_err(|e| e.to_string())?;
            if command == "files_sync_save" {
                job.filter.compile()?;
                if !job.local.is_dir() {
                    return Err(t("保存路径无效。").into());
                }
                return value(folders::save_job(&dir, &job));
            }
            return value(
                folders::sync(&dir, &s.engine, &s.store, &job, command == "files_sync_run").await,
            );
        }
        _ => {}
    }
    let client = s3::bucket_client(&s.store, account, bucket).await?;
    let filter: Filter = serde_json::from_value(args.get("filter").cloned().unwrap_or(json!({})))
        .map_err(|e| e.to_string())?;
    let items: Vec<Selection> =
        serde_json::from_value(args.get("items").cloned().unwrap_or(json!([])))
            .map_err(|e| e.to_string())?;
    match command {
        "files_upload_folder" => {
            let Some(path) = rfd::AsyncFileDialog::new().pick_folder().await else {
                return Ok(Value::Null);
            };
            let res = folders::upload(
                &s.engine,
                &s.store,
                account,
                bucket,
                prefix,
                path.path(),
                &filter,
            )
            .await;
            invalidate_tree_cache(&s.cache, Some(account.into()), Some(bucket.into())).await;
            value(res)
        }
        "files_download" => {
            let Some(folder) = rfd::AsyncFileDialog::new().pick_folder().await else {
                return Ok(Value::Null);
            };
            value(
                folders::download(
                    &s.engine,
                    &s.store,
                    account,
                    bucket,
                    prefix,
                    &items,
                    folder.path(),
                    &filter,
                )
                .await,
            )
        }
        "files_relocate" => {
            let target: Destination =
                serde_json::from_value(args["target"].clone()).map_err(|e| e.to_string())?;
            let _changes = s.cache.mutations.lock().await;
            let res = files::relocate(
                &s.store,
                account,
                bucket,
                &items,
                &target,
                args["moving"] == true,
                args["exact"] == true,
            )
            .await;
            invalidate_tree_cache(&s.cache, Some(account.into()), Some(bucket.into())).await;
            invalidate_tree_cache(&s.cache, Some(target.account), Some(target.bucket)).await;
            value(res)
        }
        "files_properties_read" => value(files::properties(&client, bucket, key).await),
        "files_properties" => {
            let patch: Properties =
                serde_json::from_value(args["patch"].clone()).map_err(|e| e.to_string())?;
            let _changes = s.cache.mutations.lock().await;
            let res = files::update_properties(&client, bucket, &items, &patch).await;
            invalidate_tree_cache(&s.cache, Some(account.into()), Some(bucket.into())).await;
            value(res)
        }
        "files_filter" => value(
            files::expand(
                &client,
                bucket,
                &[Selection {
                    key: prefix.into(),
                    folder: true,
                }],
                &filter,
            )
            .await
            .map(|objects| {
                objects
                    .into_iter()
                    .filter(|o| !o.folder)
                    .collect::<Vec<_>>()
            }),
        ),
        "files_share" => {
            let expires = args["expires"].as_u64().unwrap_or_default();
            let until = match text("until") {
                "" => None,
                s => Some(
                    chrono::DateTime::parse_from_rfc3339(s)
                        .map_err(|e| e.to_string())?
                        .timestamp(),
                ),
            };
            let lifetime = || {
                let n = until.map_or(expires, |v| {
                    v.saturating_sub(chrono::Utc::now().timestamp()).max(0) as u64
                });
                if (60..=604800).contains(&n) {
                    Ok(n)
                } else {
                    Err(t("有效期需为 60 秒到 7 天。").to_owned())
                }
            };
            lifetime()?;
            let mut links = Vec::new();
            for object in files::expand(&client, bucket, &items, &filter).await? {
                if !object.folder {
                    let config =
                        s3::presign_with_client(&client, bucket, &object.key, lifetime()?).await?;
                    links.push(config);
                }
            }
            Ok(json!(links.join("\n")))
        }
        "files_preview" => value(
            s3::preview::read(
                &client,
                bucket,
                key,
                args["page"].as_u64().unwrap_or(0),
                args["hex"] == true,
            )
            .await,
        ),
        "files_edit" => {
            let scope = (account.to_owned(), bucket.to_owned(), key.to_owned());
            if let Some(watcher) = s.editors.lock().unwrap().remove(&scope) {
                let _ = watcher.send(true);
                return Ok(json!(t("已停止编辑回传")));
            }
            // Pass the document as data to an explicitly chosen application.
            let Some(editor) = rfd::AsyncFileDialog::new()
                .set_title(t("选择本地编辑器"))
                .pick_file()
                .await
            else {
                return Ok(Value::Null);
            };
            let namespace = s3::directory_cache_namespace(&s.store, account, bucket).await?;
            let mut session =
                s3::edit::EditSession::prepare(&client, bucket, key, &dir.join("edits")).await?;
            #[cfg(target_os = "macos")]
            let launched = std::process::Command::new("/usr/bin/open")
                .arg("-a")
                .arg(editor.path())
                .arg(&session.path)
                .spawn();
            #[cfg(not(target_os = "macos"))]
            let launched = std::process::Command::new(editor.path())
                .arg(&session.path)
                .spawn();
            let mut child = launched.map_err(|e| e.to_string())?;
            tokio::task::spawn_blocking(move || child.wait());
            let (tx, mut rx) = tokio::sync::watch::channel(false);
            s.editors.lock().unwrap().insert(scope.clone(), tx);
            let (store, cache, editors, sender) = (
                s.store.clone(),
                s.cache.clone(),
                s.editors.clone(),
                s.sender.clone(),
            );
            let path = session.path.display().to_string();
            tokio::spawn(async move {
                loop {
                    tokio::select! {
                        _=rx.changed()=>break,
                        _=tokio::time::sleep(Duration::from_secs(2))=>{}
                    }
                    let result = async {
                        if !session.changed().await? {
                            return Ok(false);
                        }
                        if s3::directory_cache_namespace(&store, &scope.0, &scope.1).await?
                            != namespace
                        {
                            return Err(t("连接设置已变化，请重新打开文件").into());
                        }
                        let client = s3::bucket_client(&store, &scope.0, &scope.1).await?;
                        session.save_if_changed(&client).await
                    }
                    .await;
                    match result {
                        Ok(false) => continue,
                        Ok(true) => {
                            invalidate_tree_cache(
                                &cache,
                                Some(scope.0.clone()),
                                Some(scope.1.clone()),
                            )
                            .await;
                            let _ = sender.unbounded_send(Message::FileNotice(
                                format!("{} · {}", t("已上传"), scope.2),
                                false,
                            ));
                        }
                        Err(e) => {
                            let _ = sender.unbounded_send(Message::FileNotice(
                                format!(
                                    "{}\n{}\n{e}",
                                    t("编辑回传已停止，本地副本保存在"),
                                    session.path.display()
                                ),
                                true,
                            ));
                            let mut active = editors.lock().unwrap();
                            if active
                                .get(&scope)
                                .is_some_and(|tx| tx.subscribe().same_channel(&rx))
                            {
                                active.remove(&scope);
                            }
                            break;
                        }
                    }
                }
            });
            Ok(json!(format!(
                "{}\n{path}",
                t("保存后自动回传，再次选择编辑可停止")
            )))
        }
        "files_version_download" | "files_version_delete" => {
            let ids: Vec<String> =
                serde_json::from_value(args["versions"].clone()).map_err(|e| e.to_string())?;
            if ids.is_empty() || ids.iter().any(|id| id.is_empty()) {
                return Err(t("请选择版本").into());
            }
            if command == "files_version_download" {
                let name = key
                    .rsplit('/')
                    .next()
                    .filter(|n| !n.is_empty())
                    .unwrap_or("download");
                let name: String = name
                    .chars()
                    .map(|c| {
                        if c.is_control() || r#"\/:*?"<>|"#.contains(c) {
                            '_'
                        } else {
                            c
                        }
                    })
                    .collect();
                let mut paths = Vec::new();
                if ids.len() == 1 {
                    let Some(f) = rfd::AsyncFileDialog::new()
                        .set_file_name(&name)
                        .save_file()
                        .await
                    else {
                        return Ok(Value::Null);
                    };
                    paths.push(f.path().to_owned());
                } else {
                    let Some(f) = rfd::AsyncFileDialog::new().pick_folder().await else {
                        return Ok(Value::Null);
                    };
                    for _ in &ids {
                        paths.push(f.path().join(format!("{}-{name}", uuid::Uuid::new_v4())));
                    }
                }
                for (version, path) in ids.iter().zip(paths) {
                    let id = s
                        .engine
                        .enqueue_pinned(
                            Direction::Download,
                            account.into(),
                            bucket.into(),
                            key.into(),
                            path,
                            Some(version.clone()),
                            None,
                        )
                        .await?;
                    s.engine.start(&id, client.clone()).await?;
                }
            } else {
                let _changes = s.cache.mutations.lock().await;
                invalidate_tree_cache(&s.cache, Some(account.into()), Some(bucket.into())).await;
                for (count, version) in ids.iter().enumerate() {
                    client
                        .delete_object()
                        .bucket(bucket)
                        .key(key)
                        .version_id(version)
                        .send()
                        .await
                        .map_err(|e| format!("{} {count}\n{version}: {e}", t("已完成")))?;
                }
            }
            Ok(json!(ids.len()))
        }
        _ => Err(t("对象键无效。").into()),
    }
}
pub(super) fn preview(command: &str, args: &Value) -> Result<Value, String> {
    match command {
        "files_filter" => {
            let filter: Filter =
                serde_json::from_value(args["filter"].clone()).map_err(|e| e.to_string())?;
            let matcher = filter.compile()?;
            value(Ok(preview_objects(
                args["prefix"].as_str().unwrap_or(""),
                true,
            )
            .into_iter()
            .filter(|o| {
                !o.folder && matcher.matches(&o.key, o.size as u64, &o.modified, &o.storage_class)
            })
            .collect::<Vec<_>>()))
        }
        "files_preview" => Ok(json!({"text":"S3 Studio","next":false,"image":null})),
        "files_jobs" => Ok(json!({})),
        "files_sync_plan" | "files_sync_run" => {
            Ok(json!({"files":["reports/example.csv"],"tasks":[],"unchanged":0}))
        }
        _ => Ok(Value::Null),
    }
}

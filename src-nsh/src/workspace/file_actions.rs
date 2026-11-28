use super::*;
use crate::{i18n::t, model::FileAction};
use s3_studio_core::{
    folders::SyncJob,
    s3::{
        files::{Filter, Selection},
        preview::Preview,
    },
    transfers::Direction,
};
use std::collections::BTreeMap;

#[derive(Default)]
pub struct FileScope {
    pub account: String,
    pub bucket: String,
    pub prefix: String,
    pub items: Vec<Selection>,
    pub whole_folder: bool,
    complete: bool,
    pub version_key: String,
    pub versions: BTreeSet<String>,
}
impl FileScope {
    fn capture(workspace: &Workspace, action: FileAction) -> Self {
        let count = workspace.selection_count();
        let range_action = matches!(
            action,
            FileAction::CopyTo | FileAction::MoveTo | FileAction::Download
        );
        let whole_folder = range_action
            && count == 0
            && workspace.files.filter.is_none()
            && workspace.search_query().is_empty();
        let chosen = workspace.chosen();
        let complete = chosen.len() == count
            && !(range_action
                && count == 0
                && workspace.object_view == ObjectView::Tree
                && !workspace.search_query().is_empty());
        let objects = if range_action && count == 0 && !whole_folder {
            workspace
                .visible()
                .into_iter()
                .filter(|o| !o.folder)
                .collect()
        } else {
            chosen
        };
        let items = if whole_folder {
            vec![Selection {
                key: workspace.prefix.clone(),
                folder: true,
            }]
        } else {
            objects
                .into_iter()
                .map(|o| Selection {
                    key: o.key,
                    folder: o.folder,
                })
                .collect()
        };
        Self {
            account: workspace.active_id.clone(),
            bucket: workspace.bucket.clone(),
            prefix: workspace.prefix.clone(),
            items,
            whole_folder,
            complete,
            version_key: workspace.versions.key.clone(),
            versions: workspace.versions.marked.clone(),
        }
    }
    pub fn first_key(&self) -> &str {
        if let [item] = self.items.as_slice() {
            &item.key
        } else {
            ""
        }
    }
    fn args(&self) -> Value {
        json!({"accountId":self.account,"bucket":self.bucket,"prefix":self.prefix,"key":self.first_key(),"items":self.items})
    }
}
#[derive(Default)]
pub struct FileState {
    pub scope: FileScope,
    pub filter: Option<Filter>,
    pub preview: Option<Preview>,
    pub page: u64,
    pub hex: bool,
    pub notice: String,
    pub jobs: BTreeMap<String, SyncJob>,
    pub sync_download: bool,
}
impl Workspace {
    pub fn prepare_files(&mut self, action: FileAction) {
        self.files.scope = FileScope::capture(self, action);
        self.files.notice.clear();
        self.files.page = 0;
        self.files.hex = false;
        self.files.preview = None;
        let filter = self.files.filter.clone().unwrap_or_default();
        for (name, value, hint) in [
            ("pattern", filter.pattern, "*.csv"),
            ("after", filter.after, "YYYY-MM-DD"),
            ("before", filter.before, "YYYY-MM-DD"),
            (
                "minSize",
                filter.min_size.map(|n| n.to_string()).unwrap_or_default(),
                "0",
            ),
            (
                "maxSize",
                filter.max_size.map(|n| n.to_string()).unwrap_or_default(),
                "",
            ),
            ("storageClass", filter.storage_class, "STANDARD"),
        ] {
            self.add_input(name, &value, hint, false);
        }
        match action {
            FileAction::CopyTo | FileAction::MoveTo | FileAction::Rename => {
                let base = if action == FileAction::Rename {
                    self.first()
                        .map(|o| {
                            o.key
                                .trim_end_matches('/')
                                .rsplit('/')
                                .next()
                                .unwrap_or("")
                                .to_owned()
                        })
                        .unwrap_or_default()
                } else {
                    self.prefix.clone()
                };
                self.add_input("targetAccount", &self.active_id.clone(), "", false);
                self.add_input("targetBucket", &self.bucket.clone(), "", false);
                self.add_input("path", &base, "", false);
            }
            FileAction::Share => {
                self.add_input("minutes", "60", "1–10080", false);
                self.add_input("until", "", "YYYY-MM-DDThh:mm:ssZ", false);
            }
            FileAction::Properties => {
                for (key, hint) in [
                    ("contentType", "image/png"),
                    ("cacheControl", "max-age=3600"),
                    ("class", "STANDARD_IA"),
                    ("metadata", r#"{"key":"value"}"#),
                    ("tags", r#"{"team":"design"}"#),
                ] {
                    self.add_input(key, "", hint, false);
                }
                if self.first().is_some_and(|o| !o.folder) {
                    self.file_request("files_properties_read", Value::Null);
                }
            }
            FileAction::Sync => {
                self.add_input("job", "", "", false);
                self.add_input("local", "", "", false);
                self.files.sync_download = false;
                self.file_request("files_jobs", Value::Null);
            }
            FileAction::Preview => self.fetch_preview(),
            _ => {}
        }
    }

    fn file_request(&mut self, command: &'static str, extra: Value) {
        if self.is_busy() {
            return;
        }
        if matches!(
            command,
            "files_relocate" | "files_download" | "files_properties" | "files_share"
        ) && (!self.files.scope.complete || self.files.scope.items.is_empty())
        {
            self.dialog_error = t("请选择文件或文件夹").into();
            return;
        }
        let mut args = self.files.scope.args();
        if let Value::Object(extra) = extra {
            for (k, v) in extra {
                args[k] = v;
            }
        }
        self.dialog_error.clear();
        self.busy = true;
        self.request(
            command,
            args,
            Pending::FileAction(command, self.dialog_revision),
        );
    }

    pub(super) fn draft_filter(&self) -> Result<Filter, String> {
        let number = |key| -> Result<Option<u64>, String> {
            let s = self.input(key).trim();
            if s.is_empty() {
                Ok(None)
            } else {
                s.parse().map(Some).map_err(|_| t("范围无效").into())
            }
        };
        let f = Filter {
            pattern: self.input("pattern").into(),
            after: self.input("after").trim().into(),
            before: self.input("before").trim().into(),
            min_size: number("minSize")?,
            max_size: number("maxSize")?,
            storage_class: if matches!(
                self.dialog,
                Some(Dialog::Upload | Dialog::Files(FileAction::UploadFolder))
            ) || self.dialog == Some(Dialog::Files(FileAction::Sync))
                && !self.files.sync_download
            {
                String::new()
            } else {
                self.input("storageClass").trim().into()
            },
        };
        f.compile()?;
        Ok(f)
    }
    pub fn clear_file_filter(&mut self) {
        self.files.filter = None;
        self.close_dialog();
        self.load_objects(false);
    }
    pub fn choose_local(&mut self) {
        self.file_request("files_choose_folder", Value::Null);
    }
    pub fn fetch_preview(&mut self) {
        self.files.preview = None;
        self.file_request(
            "files_preview",
            json!({"page":self.files.page,"hex":self.files.hex}),
        );
    }
    pub fn sync_action(&mut self, command: &'static str) {
        let filter = match self.draft_filter() {
            Ok(f) => f,
            Err(e) => {
                self.dialog_error = e;
                return;
            }
        };
        let job = SyncJob {
            name: self.input("job").trim().into(),
            account: self.files.scope.account.clone(),
            bucket: self.files.scope.bucket.clone(),
            prefix: self.files.scope.prefix.clone(),
            local: self.input("local").into(),
            direction: if self.files.sync_download {
                Direction::Download
            } else {
                Direction::Upload
            },
            filter,
        };
        if job.name.is_empty() || job.local.as_os_str().is_empty() {
            self.dialog_error = t("请填写此字段。").into();
            return;
        }
        self.file_request(command, json!({"job":job}));
    }
    pub fn load_sync_job(&mut self, name: &str) {
        if let Some(job) = self.files.jobs.get(name).cloned() {
            self.set_input("job", &job.name);
            self.set_input("local", &job.local.to_string_lossy());
            self.files.sync_download = job.direction == Direction::Download;
            self.set_input("pattern", &job.filter.pattern);
            self.set_input("after", &job.filter.after);
            self.set_input("before", &job.filter.before);
            self.set_input(
                "minSize",
                &job.filter
                    .min_size
                    .map(|v| v.to_string())
                    .unwrap_or_default(),
            );
            self.set_input(
                "maxSize",
                &job.filter
                    .max_size
                    .map(|v| v.to_string())
                    .unwrap_or_default(),
            );
            self.set_input("storageClass", &job.filter.storage_class);
            self.files.notice.clear();
        }
    }

    pub fn submit_files(&mut self, action: FileAction) {
        use FileAction::*;
        if self.is_busy() {
            return;
        }
        let r = (|| -> Result<_, String> {
            Ok(match action {
                Filter => {
                    let f = self.draft_filter()?;
                    self.close_dialog();
                    self.object_view = ObjectView::List;
                    self.files.filter = Some(f);
                    self.clear_search();
                    self.load_objects(false);
                    return Ok(None);
                }
                Sync => {
                    self.sync_action("files_sync_plan");
                    return Ok(None);
                }
                Preview => {
                    self.close_dialog();
                    return Ok(None);
                }
                UploadFolder => Some((
                    "files_upload_folder",
                    json!({"filter":self.draft_filter()?}),
                )),
                Download => Some(("files_download", json!({"filter":self.draft_filter()?}))),
                Share => {
                    let until = self.input("until").trim();
                    let expires = if until.is_empty() {
                        self.input("minutes")
                            .trim()
                            .parse::<u64>()
                            .ok()
                            .and_then(|n| n.checked_mul(60))
                            .ok_or(t("有效期需为 60 秒到 7 天。"))?
                    } else {
                        0
                    };
                    Some(("files_share", json!({"expires":expires,"until":until})))
                }
                CopyTo | MoveTo | Rename => {
                    let mut path = self.input("path").to_owned();
                    if action == Rename {
                        if path.is_empty() || path.contains('/') {
                            return Err(t("对象键无效。").into());
                        }
                        let source = self
                            .files
                            .scope
                            .items
                            .first()
                            .ok_or(t("请选择文件或文件夹"))?;
                        if let Some((parent, _)) = source.key.trim_end_matches('/').rsplit_once('/')
                        {
                            path = format!("{parent}/{path}");
                        }
                    }
                    let args = json!({"target":{"account":self.input("targetAccount"),"bucket":self.input("targetBucket"),"path":path},"moving":action!=CopyTo,"exact":action==Rename});
                    Some(("files_relocate", args))
                }
                Edit => Some(("files_edit", Value::Null)),
                Properties => {
                    let mut patch = json!({});
                    for (key, input) in [
                        ("contentType", "contentType"),
                        ("cacheControl", "cacheControl"),
                        ("storageClass", "class"),
                    ] {
                        if !self.input(input).is_empty() {
                            patch[key] = json!(self.input(input));
                        }
                    }
                    for field in ["metadata", "tags"] {
                        if !self.input(field).trim().is_empty() {
                            patch[field] = serde_json::from_str(self.input(field))
                                .map_err(|e| e.to_string())?;
                        }
                    }
                    Some(("files_properties", json!({"patch":patch})))
                }
                DownloadVersions | DeleteVersions => Some((
                    if action == DeleteVersions {
                        "files_version_delete"
                    } else {
                        "files_version_download"
                    },
                    json!({"key":self.files.scope.version_key,"versions":self.files.scope.versions}),
                )),
            })
        })();
        match r {
            Ok(Some((cmd, args))) => self.file_request(cmd, args),
            Err(e) => self.dialog_error = e,
            _ => {}
        }
    }

    pub fn toggle_version(&mut self, id: &str, checked: bool) {
        if checked {
            self.versions.marked.insert(id.into());
        } else {
            self.versions.marked.remove(id);
        }
        self.versions.selected = if self.versions.marked.len() == 1 {
            self.versions
                .entries
                .iter()
                .find(|v| !v.latest && !v.deleted && self.versions.marked.contains(&v.id))
                .map(|v| v.id.clone())
        } else {
            None
        };
    }

    pub fn finish_files(&mut self, command: &str, result: Result<Value, String>) {
        let data = match result {
            Ok(v) => v,
            Err(e) => {
                self.dialog_error = e;
                self.list_cache.clear();
                if matches!(
                    command,
                    "files_properties"
                        | "files_relocate"
                        | "files_version_delete"
                        | "files_upload_folder"
                ) {
                    self.load_objects(false);
                }
                return;
            }
        };
        match command {
            "files_choose_folder" => {
                if let Some(path) = data.as_str() {
                    self.set_input("local", path);
                }
            }
            "files_jobs" => {
                self.files.jobs = serde_json::from_value(data).unwrap_or_default();
            }
            "files_sync_save" => {
                self.files.notice = t("设置保存成功").into();
                self.file_request("files_jobs", Value::Null);
            }
            "files_sync_plan" | "files_sync_run" => {
                let files = data["files"]
                    .as_array()
                    .map(Vec::as_slice)
                    .unwrap_or_default();
                self.files.notice = format!(
                    "{}: {}   ·   {}: {}\n{}",
                    if command == "files_sync_run" {
                        t("已加入传输队列")
                    } else {
                        t("待传输")
                    },
                    files.len(),
                    t("内容相同"),
                    data["unchanged"],
                    files
                        .iter()
                        .take(100)
                        .filter_map(|f| f.as_str())
                        .collect::<Vec<_>>()
                        .join("\n")
                );
            }
            "files_properties_read" => {
                self.files.notice = serde_json::to_string_pretty(&data).unwrap_or_default();
            }
            "files_preview" => match serde_json::from_value(data) {
                Ok(p) => self.files.preview = Some(p),
                Err(e) => self.dialog_error = e.to_string(),
            },
            "files_share" => {
                if let Some(links) = data.as_str() {
                    if links.is_empty() {
                        self.dialog_error = t("没有匹配的文件").into();
                    } else {
                        set_clipboard(links);
                        self.close_dialog();
                        self.notify(format!(
                            "{} · {}",
                            t("已复制分享链接"),
                            links.lines().count()
                        ));
                    }
                }
            }
            "files_edit" => {
                if let Some(text) = data.as_str() {
                    self.files.notice = text.into();
                } else {
                    self.close_dialog();
                }
            }
            "files_version_delete" | "files_version_download" => {
                self.list_cache.clear();
                self.load_objects(false);
                let key = self.files.scope.version_key.clone();
                let back = self.versions.from_trash;
                self.open_versions(key);
                self.versions.from_trash = back;
            }
            _ => {
                self.list_cache.clear();
                self.close_dialog();
                self.load_objects(false);
                if let Some(n) = data.as_u64() {
                    self.notify(format!("{} · {n}", t("已完成")));
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn workspace() -> Workspace {
        let (backend, _) = Backend::open(true).unwrap();
        let mut ws = Workspace::new(backend);
        ws.active_id = "a".into();
        ws.bucket = "bucket".into();
        ws.prefix = "dir/".into();
        for key in ["dir/one", "dir/two"] {
            ws.objects.push(S3Object {
                key: key.into(),
                size: 1,
                folder: false,
                modified: String::new(),
                storage_class: String::new(),
            });
        }
        ws
    }
    #[test]
    fn failed_move_retry_keeps_the_original_object_scope_after_refresh() {
        let mut ws = workspace();
        ws.selected.insert("dir/one".into());
        ws.open_dialog(Dialog::Files(FileAction::MoveTo));
        let args = ws.files.scope.args();
        assert_eq!(args["items"], json!([{"key":"dir/one","folder":false}]));
        ws.finish_files("files_relocate", Err("conflict".into()));
        assert!(ws.selected.is_empty());
        ws.active_id = "other".into();
        ws.bucket = "other".into();
        ws.prefix.clear();
        assert_eq!(args, ws.files.scope.args());
        assert!(!ws.files.scope.whole_folder);
    }
    #[test]
    fn directory_scope_is_explicit_and_empty_filtered_results_stay_empty() {
        let mut ws = workspace();
        ws.open_dialog(Dialog::Files(FileAction::CopyTo));
        assert!(ws.files.scope.whole_folder);
        assert_eq!(
            ws.files.scope.args()["items"],
            json!([{"key":"dir/","folder":true}])
        );
        ws.close_dialog();
        ws.files.filter = Some(Filter::default());
        ws.objects.clear();
        ws.open_dialog(Dialog::Files(FileAction::CopyTo));
        ws.submit();
        assert!(!ws.files.scope.whole_folder);
        assert!(!ws.busy);
        assert!(!ws.dialog_error.is_empty());
    }
}

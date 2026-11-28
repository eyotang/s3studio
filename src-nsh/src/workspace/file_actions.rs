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
pub struct FileState {
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
        let mut args = self.args();
        args["key"] = json!(self.first().map(|o| o.key).unwrap_or_default());
        args["prefix"] = json!(self.prefix);
        args["items"] = json!(
            self.chosen()
                .into_iter()
                .map(|o| Selection {
                    key: o.key,
                    folder: o.folder
                })
                .collect::<Vec<_>>()
        );
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
            account: self.active_id.clone(),
            bucket: self.bucket.clone(),
            prefix: self.prefix.clone(),
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
                Download => {
                    let mut args = json!({"filter":self.draft_filter()?});
                    if self.selection_count() == 0 {
                        args["items"] = json!([{"key":self.prefix,"folder":true}]);
                    }
                    Some(("files_download", args))
                }
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
                        let source = self.first().ok_or(t("请选择文件或文件夹"))?;
                        if let Some((parent, _)) = source.key.trim_end_matches('/').rsplit_once('/')
                        {
                            path = format!("{parent}/{path}");
                        }
                    }
                    let mut args = json!({"target":{"account":self.input("targetAccount"),"bucket":self.input("targetBucket"),"path":path},"moving":action!=CopyTo,"exact":action==Rename});
                    if self.selection_count() == 0 {
                        args["items"] = json!([{"key":self.prefix,"folder":true}]);
                    }
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
                    json!({"key":self.versions.key,"versions":self.versions.marked}),
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
                let key = self.versions.key.clone();
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

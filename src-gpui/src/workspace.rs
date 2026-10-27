use crate::{
    backend::{Backend, Message},
    model::{self, Dialog, Sort},
};
use futures::{StreamExt, channel::mpsc::UnboundedReceiver};
use gpui::component::input::{InputEvent, InputState};
use gpui::{prelude::*, *};
use s3_studio_core::{
    accounts::Account,
    s3::{Bucket, ObjectInfo, ObjectPage, S3Object},
    transfers::{Settings, Transfer},
};
use serde_json::{Value, json};
use std::collections::{HashMap, HashSet};

#[derive(Clone)]
pub enum Pending {
    Accounts(Option<String>),
    Buckets(u64, Option<String>),
    Objects(u64, bool),
    Info(u64),
    Transfers,
    Settings,
    Submit(Dialog),
    Download,
    Share,
    Control(String),
    Clear,
    Update,
}
pub struct Form {
    pub inputs: HashMap<&'static str, Entity<InputState>>,
    pub path_style: bool,
    pub subscriptions: Vec<Subscription>,
}
pub struct Workspace {
    pub backend: Backend,
    pub accounts: Vec<Account>,
    pub active_id: String,
    pub buckets: Vec<Bucket>,
    pub bucket: String,
    pub prefix: String,
    pub objects: Vec<S3Object>,
    pub next_token: Option<String>,
    pub search: Entity<InputState>,
    pub bucket_search: Entity<InputState>,
    pub sort: Sort,
    pub ascending: bool,
    pub selected: HashSet<String>,
    pub grid: bool,
    pub show_info: bool,
    pub info: Option<ObjectInfo>,
    pub info_error: String,
    pub transfers: Vec<Transfer>,
    pub settings: Settings,
    pub expanded: bool,
    pub transfer_error: String,
    pub pending_transfers: HashSet<String>,
    pub loading: bool,
    pub bucket_loading: bool,
    pub busy: bool,
    pub update_status: crate::updater::Status,
    pub update_pending: bool,
    update_guard: Option<s3_studio_core::transfers::UpdateGuard>,
    pub update_error: String,
    pub error: String,
    pub dialog_error: String,
    pub toast: String,
    pub dialog: Option<Dialog>,
    pub form: Form,
    pub focus: FocusHandle,
    pub modal_focus: FocusHandle,
    pub previous_focus: Option<FocusHandle>,
    pub _subscriptions: Vec<Subscription>,
    pending: HashMap<u64, Pending>,
    next_request: u64,
    bucket_revision: u64,
    object_revision: u64,
    info_revision: u64,
    refreshed_uploads: HashSet<String>,
    transfer_changes: HashSet<String>,
    toast_revision: u64,
}
impl Workspace {
    pub fn new(
        backend: Backend,
        mut receiver: UnboundedReceiver<Message>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let search = cx.new(|cx| InputState::new(window, cx).placeholder("搜索当前目录中的文件…"));
        let bucket_search = cx.new(|cx| InputState::new(window, cx).placeholder("查找存储桶"));
        let subscriptions = vec![
            cx.subscribe(&search, |_, _, _: &InputEvent, cx| cx.notify()),
            cx.subscribe(&bucket_search, |_, _, _: &InputEvent, cx| cx.notify()),
        ];
        cx.spawn_in(window, async move |this, cx| {
            while let Some(message) = receiver.next().await {
                if this
                    .update_in(cx, |this, window, cx| this.receive(message, window, cx))
                    .is_err()
                {
                    break;
                }
            }
        })
        .detach();
        let mut this = Self {
            backend,
            accounts: vec![],
            active_id: String::new(),
            buckets: vec![],
            bucket: String::new(),
            prefix: String::new(),
            objects: vec![],
            next_token: None,
            search,
            bucket_search,
            sort: Sort::Name,
            ascending: true,
            selected: HashSet::new(),
            grid: false,
            show_info: false,
            info: None,
            info_error: String::new(),
            transfers: vec![],
            settings: Settings::default(),
            expanded: true,
            transfer_error: String::new(),
            pending_transfers: HashSet::new(),
            loading: false,
            bucket_loading: false,
            busy: false,
            update_status: Default::default(),
            update_pending: false,
            update_guard: None,
            update_error: String::new(),
            error: String::new(),
            dialog_error: String::new(),
            toast: String::new(),
            dialog: None,
            form: Form {
                inputs: HashMap::new(),
                path_style: true,
                subscriptions: vec![],
            },
            focus: cx.focus_handle(),
            modal_focus: cx.focus_handle(),
            previous_focus: None,
            _subscriptions: subscriptions,
            pending: HashMap::new(),
            next_request: 0,
            bucket_revision: 0,
            object_revision: 0,
            info_revision: 0,
            refreshed_uploads: HashSet::new(),
            transfer_changes: HashSet::new(),
            toast_revision: 0,
        };
        this.request("list_accounts", json!({}), Pending::Accounts(None));
        this.request("list_transfers", json!({}), Pending::Transfers);
        this.request("get_transfer_settings", json!({}), Pending::Settings);
        window.focus(&this.focus, cx);
        this
    }
    pub fn is_busy(&self) -> bool {
        self.busy || self.update_status.installing()
    }
    pub fn can_open_app_dialog(&self) -> bool {
        !self.is_busy()
            && !self.update_pending
            && matches!(
                self.dialog,
                None | Some(Dialog::About | Dialog::Help | Dialog::Update)
            )
    }
    pub fn show_about(&mut self, _: &crate::About, window: &mut Window, cx: &mut Context<Self>) {
        if self.can_open_app_dialog() {
            self.open_dialog(Dialog::About, window, cx);
        }
    }
    pub fn check_for_updates(
        &mut self,
        _: &crate::CheckForUpdates,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.can_open_app_dialog() {
            return;
        }
        self.open_dialog(Dialog::Update, window, cx);
        if self.update_status.mode != "disabled" && self.update_status.phase != "checking" {
            self.update_action("check_app_update", cx);
        }
    }
    pub fn update_action(&mut self, command: &'static str, cx: &mut Context<Self>) {
        if self.update_pending || self.update_status.installing() {
            return;
        }
        self.update_pending = true;
        self.update_error.clear();
        self.request(command, json!({}), Pending::Update);
        cx.notify();
    }
    pub fn accept_update(&mut self, status: crate::updater::Status) {
        if status.revision >= self.update_status.revision {
            self.update_status = status;
        }
    }
    pub fn account(&self) -> Option<&Account> {
        self.accounts.iter().find(|a| a.id == self.active_id)
    }
    pub fn visible(&self, cx: &App) -> Vec<S3Object> {
        model::visible(
            &self.objects,
            &self.search.read(cx).value(),
            self.sort,
            self.ascending,
        )
    }
    pub fn chosen(&self) -> Vec<S3Object> {
        self.objects
            .iter()
            .filter(|o| self.selected.contains(&o.key))
            .cloned()
            .collect()
    }
    pub fn first(&self) -> Option<S3Object> {
        let chosen = self.chosen();
        if chosen.len() == 1 {
            chosen.into_iter().next()
        } else {
            None
        }
    }
    pub fn disabled(&self) -> bool {
        self.bucket.is_empty() || self.loading || self.is_busy()
    }
    fn args(&self) -> Value {
        json!({"accountId":self.active_id,"bucket":self.bucket})
    }
    fn request(&mut self, command: &'static str, args: Value, pending: Pending) {
        self.next_request += 1;
        let id = self.next_request;
        self.pending.insert(id, pending);
        self.backend.request(id, command, args);
    }
    pub fn select_account(&mut self, id: String, window: &mut Window, cx: &mut Context<Self>) {
        if self.is_busy() || id == self.active_id {
            return;
        }
        self.active_id = id;
        self.load_buckets(None, window, cx);
    }
    fn load_buckets(
        &mut self,
        select: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.bucket_revision += 1;
        self.object_revision += 1;
        self.info_revision += 1;
        self.buckets.clear();
        self.bucket.clear();
        self.objects.clear();
        self.selected.clear();
        self.prefix.clear();
        self.next_token = None;
        self.error.clear();
        self.info = None;
        self.search.update(cx, |s, cx| s.set_value("", window, cx));
        self.loading = false;
        self.bucket_loading = !self.active_id.is_empty();
        if self.bucket_loading {
            self.request(
                "list_buckets",
                json!({"accountId":self.active_id}),
                Pending::Buckets(self.bucket_revision, select),
            );
        }
        cx.notify();
    }
    pub fn navigate(
        &mut self,
        bucket: String,
        prefix: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.bucket == bucket && self.prefix == prefix {
            return;
        }
        self.bucket = bucket;
        self.prefix = prefix;
        self.search.update(cx, |s, cx| s.set_value("", window, cx));
        self.load_objects(false, cx);
    }
    pub fn load_objects(&mut self, append: bool, cx: &mut Context<Self>) {
        self.object_revision += 1;
        self.info_revision += 1;
        if !append {
            self.objects.clear();
            self.selected.clear();
            self.next_token = None;
            self.info = None;
            self.info_error.clear();
        }
        if self.active_id.is_empty() || self.bucket.is_empty() {
            self.loading = false;
            cx.notify();
            return;
        }
        self.loading = true;
        self.error.clear();
        let mut args = self.args();
        args["prefix"] = json!(self.prefix);
        args["token"] = json!(if append {
            self.next_token.clone()
        } else {
            None
        });
        self.request(
            "list_objects",
            args,
            Pending::Objects(self.object_revision, append),
        );
        cx.notify();
    }
    pub fn selection_changed(&mut self, cx: &mut Context<Self>) {
        self.info_revision += 1;
        self.info = None;
        self.info_error.clear();
        if let Some(first) = self.first().filter(|o| self.show_info && !o.folder) {
            let mut args = self.args();
            args["key"] = json!(first.key);
            self.request("object_info", args, Pending::Info(self.info_revision));
        }
        cx.notify();
    }
    pub fn toggle(&mut self, key: &str, cx: &mut Context<Self>) {
        if !self.selected.remove(key) {
            self.selected.insert(key.into());
        }
        self.selection_changed(cx);
    }
    pub fn inspect(&mut self, key: String, cx: &mut Context<Self>) {
        self.selected = HashSet::from([key]);
        self.show_info = true;
        self.selection_changed(cx);
    }
    pub fn sort_by(&mut self, sort: Sort, cx: &mut Context<Self>) {
        if self.sort == sort {
            self.ascending = !self.ascending;
        } else {
            self.sort = sort;
            self.ascending = true;
        }
        cx.notify();
    }
    pub fn close_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.is_busy() || (self.dialog == Some(Dialog::Update) && self.update_pending) {
            return;
        }
        self.dialog = None;
        self.form.inputs.clear();
        self.form.subscriptions.clear();
        if let Some(focus) = self.previous_focus.take() {
            window.focus(&focus, cx);
        }
        cx.notify();
    }
    pub fn open_dialog(&mut self, dialog: Dialog, window: &mut Window, cx: &mut Context<Self>) {
        if self.dialog.is_none() {
            self.previous_focus = window.focused(cx);
        }
        self.dialog_error.clear();
        self.form.inputs.clear();
        self.form.subscriptions.clear();
        self.form.path_style = true;
        match &dialog {
            Dialog::Account(id) => {
                let account = id
                    .as_ref()
                    .and_then(|id| self.accounts.iter().find(|a| &a.id == id))
                    .cloned();
                self.form.path_style = account.as_ref().map(|a| a.path_style).unwrap_or(true);
                self.add_input(
                    "name",
                    account.as_ref().map(|a| a.name.as_str()).unwrap_or(""),
                    "例如：生产环境 / 个人存储",
                    false,
                    window,
                    cx,
                );
                self.add_input(
                    "accessKey",
                    "",
                    if account.is_some() {
                        "留空保留现有凭据"
                    } else {
                        "AKIA…"
                    },
                    false,
                    window,
                    cx,
                );
                self.add_input(
                    "region",
                    account
                        .as_ref()
                        .map(|a| a.region.as_str())
                        .filter(|s| !s.is_empty())
                        .unwrap_or("us-east-1"),
                    "us-east-1",
                    false,
                    window,
                    cx,
                );
                self.add_input(
                    "secretKey",
                    "",
                    if account.is_some() {
                        "留空保留现有凭据"
                    } else {
                        "你的 Secret Access Key"
                    },
                    true,
                    window,
                    cx,
                );
                self.add_input(
                    "sessionToken",
                    "",
                    "使用临时凭据时填写；更新凭据时留空可清除",
                    true,
                    window,
                    cx,
                );
                self.add_input(
                    "endpoint",
                    account.as_ref().map(|a| a.endpoint.as_str()).unwrap_or(""),
                    "https://s3.example.com",
                    false,
                    window,
                    cx,
                );
            }
            Dialog::Folder => self.add_input("name", "", "新文件夹", false, window, cx),
            Dialog::Bucket => self.add_input("name", "", "my-new-bucket", false, window, cx),
            Dialog::Copy => {
                let name = format!(
                    "{}copy-{}",
                    self.prefix,
                    self.first()
                        .map(|o| model::object_name(&o.key, &self.prefix).to_owned())
                        .unwrap_or_default()
                );
                self.add_input("name", &name, &name, false, window, cx);
            }
            Dialog::TransferSettings => self.add_input(
                "concurrency",
                &self.settings.concurrency.to_string(),
                "",
                false,
                window,
                cx,
            ),
            _ => {}
        }
        self.dialog = Some(dialog);
        let input = self
            .form
            .inputs
            .get("name")
            .or_else(|| self.form.inputs.get("concurrency"))
            .cloned();
        if let Some(input) = input {
            input.update(cx, |s, cx| s.focus(window, cx));
        } else {
            window.focus(&self.modal_focus, cx);
        }
        cx.notify();
    }
    fn add_input(
        &mut self,
        key: &'static str,
        value: &str,
        placeholder: &str,
        masked: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let state = cx.new(|cx| {
            InputState::new(window, cx)
                .default_value(value.to_owned())
                .placeholder(placeholder.to_owned())
                .masked(masked)
        });
        self.form.subscriptions.push(cx.subscribe_in(
            &state,
            window,
            |this, _, event, window, cx| {
                if matches!(event, InputEvent::PressEnter { .. }) {
                    this.submit(window, cx);
                }
            },
        ));
        self.form.inputs.insert(key, state);
    }
    fn input(&self, key: &str, cx: &App) -> String {
        self.form
            .inputs
            .get(key)
            .map(|s| s.read(cx).value().to_string())
            .unwrap_or_default()
    }
    pub fn submit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.is_busy() {
            return;
        }
        let Some(dialog) = self.dialog.clone() else {
            return;
        };
        if matches!(dialog, Dialog::Help | Dialog::About) {
            self.close_dialog(window, cx);
            return;
        }
        let mut args = self.args();
        let name = self.input("name", cx).trim().to_owned();
        if matches!(
            dialog,
            Dialog::Account(_) | Dialog::Folder | Dialog::Bucket | Dialog::Copy
        ) && name.is_empty()
        {
            self.dialog_error = "请填写此字段。".into();
            cx.notify();
            return;
        }
        let command = match &dialog {
            Dialog::Account(id) => {
                let access = self.input("accessKey", cx).trim().to_owned();
                let secret = self.input("secretKey", cx);
                let region = self.input("region", cx).trim().to_owned();
                if name.chars().count() > 80
                    || region.is_empty()
                    || (id.is_none() && (access.is_empty() || secret.is_empty()))
                {
                    self.dialog_error =
                        "请填写完整的账户名称、Access Key、Secret Key 和 Region。".into();
                    cx.notify();
                    return;
                }
                args = json!({"input":{"id":id.clone().unwrap_or_default(),"name":name,"region":region,"endpoint":self.input("endpoint",cx).trim(),"pathStyle":self.form.path_style,"accessKey":access,"secretKey":secret,"sessionToken":self.input("sessionToken",cx)}});
                "save_account"
            }
            Dialog::Folder => {
                if name.contains(['/', '\\']) {
                    self.dialog_error = "文件夹名称不能包含 / 或 \\。".into();
                    cx.notify();
                    return;
                }
                args["key"] = json!(format!("{}{name}/", self.prefix));
                "create_folder"
            }
            Dialog::Bucket => {
                args["name"] = json!(name);
                "create_bucket"
            }
            Dialog::Copy => {
                let Some(first) = self.first() else {
                    return;
                };
                args["key"] = json!(first.key);
                args["destination"] = json!(name);
                "copy_object"
            }
            Dialog::Delete => {
                let chosen = self.chosen();
                args["keys"] = json!(
                    chosen
                        .iter()
                        .filter(|o| !o.folder)
                        .map(|o| &o.key)
                        .collect::<Vec<_>>()
                );
                args["folders"] = json!(
                    chosen
                        .iter()
                        .filter(|o| o.folder)
                        .map(|o| &o.key)
                        .collect::<Vec<_>>()
                );
                "delete_objects"
            }
            Dialog::Upload => {
                args["prefix"] = json!(self.prefix);
                self.close_dialog(window, cx);
                "upload_files"
            }
            Dialog::RemoveAccount => "remove_account",
            Dialog::TransferSettings => {
                let n = self.input("concurrency", cx).parse::<usize>();
                if !n.as_ref().is_ok_and(|n| (1..=32).contains(n)) {
                    self.dialog_error = "并发数量需为 1–32 的整数。".into();
                    cx.notify();
                    return;
                }
                args["concurrency"] = json!(n.unwrap_or(8));
                "set_transfer_settings"
            }
            Dialog::CancelTransfer(id) => {
                let id = id.clone();
                self.close_dialog(window, cx);
                self.control("cancel_transfer", id, cx);
                return;
            }
            Dialog::Update | Dialog::Help | Dialog::About => return,
        };
        self.busy = true;
        self.dialog_error.clear();
        self.request(command, args, Pending::Submit(dialog));
        cx.notify();
    }
    pub fn download(&mut self, cx: &mut Context<Self>) {
        if self.is_busy() {
            return;
        }
        if let Some(first) = self.first().filter(|o| !o.folder) {
            let mut args = self.args();
            args["key"] = json!(first.key);
            self.busy = true;
            self.error.clear();
            self.request("download_object", args, Pending::Download);
            cx.notify();
        }
    }
    pub fn share(&mut self, cx: &mut Context<Self>) {
        if self.is_busy() {
            return;
        }
        if let Some(first) = self.first().filter(|o| !o.folder) {
            let mut args = self.args();
            args["key"] = json!(first.key);
            self.busy = true;
            self.error.clear();
            self.request("presign_object", args, Pending::Share);
            cx.notify();
        }
    }
    pub fn control(&mut self, command: &'static str, id: String, cx: &mut Context<Self>) {
        if self.pending_transfers.contains(&id) {
            return;
        }
        self.pending_transfers.insert(id.clone());
        self.transfer_error.clear();
        self.request(command, json!({"transferId":id}), Pending::Control(id));
        cx.notify();
    }
    pub fn clear_finished(&mut self, cx: &mut Context<Self>) {
        self.request("clear_finished_transfers", json!({}), Pending::Clear);
        cx.notify();
    }
    fn notify(&mut self, message: String, window: &Window, cx: &mut Context<Self>) {
        self.toast = message;
        self.toast_revision += 1;
        let revision = self.toast_revision;
        cx.spawn_in(window, async move |this, cx| {
            cx.background_executor()
                .timer(std::time::Duration::from_millis(4500))
                .await;
            let _ = this.update(cx, |this, cx| {
                if this.toast_revision == revision {
                    this.toast.clear();
                    cx.notify();
                }
            });
        })
        .detach();
    }
    fn receive(&mut self, message: Message, window: &mut Window, cx: &mut Context<Self>) {
        let message = match message {
            Message::Update(status) => {
                self.accept_update(status);
                cx.notify();
                return;
            }
            Message::Restart(guard) => {
                self.update_guard = Some(guard);
                cx.restart();
                return;
            }
            message => message,
        };
        let Message::Response(id, result) = message else {
            if let Message::Transfer(t) = message {
                self.transfer_changes.insert(t.id.clone());
                if t.status == "done"
                    && t.direction == s3_studio_core::transfers::Direction::Upload
                    && self.refreshed_uploads.insert(t.id.clone())
                    && t.account_id == self.active_id
                    && t.bucket == self.bucket
                {
                    self.load_objects(false, cx);
                }
                if let Some(old) = self.transfers.iter_mut().find(|old| old.id == t.id) {
                    *old = t;
                } else {
                    self.transfers.insert(0, t);
                }
                cx.notify();
            }
            return;
        };
        let Some(pending) = self.pending.remove(&id) else {
            return;
        };
        if matches!(pending,Pending::Buckets(rev,_) if rev!=self.bucket_revision)
            || matches!(pending,Pending::Objects(rev,_) if rev!=self.object_revision)
            || matches!(pending,Pending::Info(rev) if rev!=self.info_revision)
        {
            return;
        }
        match &pending {
            Pending::Update => self.update_pending = false,
            Pending::Buckets(..) => self.bucket_loading = false,
            Pending::Objects(..) => self.loading = false,
            Pending::Submit(_) | Pending::Download | Pending::Share => self.busy = false,
            Pending::Control(id) => {
                self.pending_transfers.remove(id);
            }
            _ => {}
        }
        let data = match result {
            Ok(data) => data,
            Err(error) => {
                match pending {
                    Pending::Update => self.update_error = error,
                    Pending::Submit(Dialog::Upload) => {
                        self.load_objects(false, cx);
                        self.error = error;
                    }
                    Pending::Submit(Dialog::Delete) => {
                        self.load_objects(false, cx);
                        self.dialog_error = error;
                    }
                    Pending::Submit(_) => self.dialog_error = error,
                    Pending::Info(_) => self.info_error = error,
                    Pending::Control(_)
                    | Pending::Clear
                    | Pending::Transfers
                    | Pending::Settings => self.transfer_error = error,
                    _ => self.error = error,
                }
                cx.notify();
                return;
            }
        };
        let handled: Result<(), String> = (|| {
            match pending {
                Pending::Accounts(select) => {
                    self.accounts = serde_json::from_value(data).map_err(|e| e.to_string())?;
                    self.active_id = select
                        .or_else(|| {
                            self.accounts
                                .iter()
                                .find(|a| a.id == self.active_id)
                                .map(|a| a.id.clone())
                        })
                        .or_else(|| self.accounts.first().map(|a| a.id.clone()))
                        .unwrap_or_default();
                    self.load_buckets(None, window, cx);
                }
                Pending::Buckets(_, select) => {
                    self.buckets = serde_json::from_value(data).map_err(|e| e.to_string())?;
                    self.bucket = select
                        .or_else(|| self.buckets.first().map(|b| b.name.clone()))
                        .unwrap_or_default();
                    self.load_objects(false, cx);
                }
                Pending::Objects(_, append) => {
                    let page: ObjectPage =
                        serde_json::from_value(data).map_err(|e| e.to_string())?;
                    if append {
                        for o in page.objects {
                            if !self.objects.iter().any(|old| old.key == o.key) {
                                self.objects.push(o);
                            }
                        }
                    } else {
                        self.objects = page.objects;
                    }
                    self.next_token = page.next_token;
                }
                Pending::Info(_) => {
                    self.info = Some(serde_json::from_value(data).map_err(|e| e.to_string())?)
                }
                Pending::Transfers => {
                    let list: Vec<Transfer> =
                        serde_json::from_value(data).map_err(|e| e.to_string())?;
                    self.transfers
                        .retain(|t| self.transfer_changes.contains(&t.id));
                    self.transfers.extend(
                        list.into_iter()
                            .filter(|t| !self.transfer_changes.contains(&t.id)),
                    );
                }
                Pending::Settings => {
                    self.settings = serde_json::from_value(data).map_err(|e| e.to_string())?
                }
                Pending::Submit(dialog) => {
                    let message = match dialog {
                        Dialog::Account(_) => {
                            let a: Account =
                                serde_json::from_value(data).map_err(|e| e.to_string())?;
                            self.request("list_accounts", json!({}), Pending::Accounts(Some(a.id)));
                            "账户已安全保存".into()
                        }
                        Dialog::RemoveAccount => {
                            self.request("list_accounts", json!({}), Pending::Accounts(None));
                            "账户已移除".into()
                        }
                        Dialog::Bucket => {
                            let name = self.input("name", cx).trim().to_owned();
                            self.load_buckets(Some(name), window, cx);
                            "存储桶已创建".into()
                        }
                        Dialog::TransferSettings => {
                            self.settings =
                                serde_json::from_value(data).map_err(|e| e.to_string())?;
                            "传输并发设置已保存".into()
                        }
                        Dialog::Delete => {
                            self.load_objects(false, cx);
                            format!("已删除 {} 个对象", data.as_u64().unwrap_or_default())
                        }
                        Dialog::Upload => {
                            self.load_objects(false, cx);
                            let count = data.as_u64().unwrap_or_default();
                            if count > 0 {
                                format!("已添加 {count} 个上传任务，可在传输活动中暂停或继续")
                            } else {
                                String::new()
                            }
                        }
                        Dialog::Folder => {
                            self.load_objects(false, cx);
                            "文件夹已创建".into()
                        }
                        Dialog::Copy => {
                            self.load_objects(false, cx);
                            "对象已复制".into()
                        }
                        _ => String::new(),
                    };
                    self.close_dialog(window, cx);
                    if !message.is_empty() {
                        self.notify(message, window, cx);
                    }
                }
                Pending::Share => {
                    if let Some(url) = data.as_str() {
                        cx.write_to_clipboard(ClipboardItem::new_string(url.into()));
                        self.notify("已复制分享链接，1 小时内有效".into(), window, cx);
                    }
                }
                Pending::Clear => self
                    .transfers
                    .retain(|t| !matches!(t.status.as_str(), "done" | "cancelled")),
                Pending::Control(_) | Pending::Download | Pending::Update => {}
            }
            Ok(())
        })();
        if let Err(error) = handled {
            self.error = error;
        }
        cx.notify();
    }
}
impl Focusable for Workspace {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

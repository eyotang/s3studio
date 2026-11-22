use crate::{
    backend::{Backend, Message, TreeTask},
    model::{self, Dialog, Sort},
    object_tree::{
        LiveTree, Totals, TreeEntry, TreeKey, TreeListing, TreeOptions, TreePage, TreeRow, TreeSort,
    },
    tree_cache::{CACHE_LIFETIME_MS, TreeScope},
};
use futures::{StreamExt, channel::mpsc::UnboundedReceiver};
use nsh_core::prelude::*;
use s3_studio_core::{
    accounts::{Account, S3Connection, S3Defaults, S3Option},
    s3::{Bucket, ObjectInfo, ObjectPage, S3Object},
    transfers::{Settings, Transfer},
};
use serde_json::{Value, json};
use std::{
    cell::RefCell,
    collections::{BTreeSet, HashMap, HashSet},
    rc::Rc,
    time::Duration,
};

enum Pending {
    Accounts(Option<String>),
    Buckets(u64, Option<String>),
    Objects(u64, bool),
    TreeLevel(u64, String),
    Info(u64),
    Transfers,
    Settings,
    S3Defaults,
    AddS3Option,
    Submit(Dialog),
    Download,
    Share,
    Control(String),
    Clear,
    Update,
}
pub struct Field {
    pub value: String,
    pub placeholder: String,
    pub masked: bool,
}
pub struct Form {
    pub inputs: HashMap<&'static str, Field>,
    pub path_style: bool,
    pub s3_options: S3Defaults,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ObjectView {
    List,
    Grid,
    Tree,
}

pub enum TreeStatus {
    Idle,
    Opening,
    Scanning(Totals),
    Cancelled,
    Failed(String),
}

pub struct TreeBrowser {
    pub status: TreeStatus,
    pub options: TreeOptions,
    pub rows: Rc<Vec<TreeRow>>,
    pub selected: BTreeSet<TreeKey>,
    pub totals: Option<Totals>,
    pub scanned_at: Option<i64>,
    pub dirty: bool,
    pub query_error: String,
    live: LiveTree,
    live_needs_refresh: bool,
    cache_available: bool,
    scope: Option<TreeScope>,
    task: Option<TreeTask>,
    query: Option<TreeTask>,
    revision: u64,
    query_revision: u64,
}

impl TreeBrowser {
    fn new() -> Self {
        Self {
            status: TreeStatus::Idle,
            options: TreeOptions::default(),
            rows: Rc::default(),
            selected: BTreeSet::new(),
            totals: None,
            scanned_at: None,
            dirty: false,
            query_error: String::new(),
            live: LiveTree::default(),
            live_needs_refresh: false,
            cache_available: false,
            scope: None,
            task: None,
            query: None,
            revision: 0,
            query_revision: 0,
        }
    }

    fn cancel_scan(&mut self) {
        if self.task.take().is_some() {
            self.revision += 1;
            self.status = TreeStatus::Cancelled;
        }
    }

    fn cancel_query(&mut self) {
        if self.query.take().is_some() {
            self.query_revision += 1;
        }
    }

    fn clear(&mut self) {
        self.cancel_scan();
        self.cancel_query();
        self.status = TreeStatus::Idle;
        self.scope = None;
        self.live.clear();
        self.live_needs_refresh = false;
        self.clear_result();
        self.options = TreeOptions {
            sort: self.options.sort,
            ascending: self.options.ascending,
            ..Default::default()
        };
    }

    fn clear_result(&mut self) {
        self.rows = Rc::default();
        self.selected.clear();
        self.totals = None;
        self.scanned_at = None;
        self.dirty = false;
        self.query_error.clear();
        self.cache_available = false;
    }

    pub fn querying(&self) -> bool {
        self.query.is_some()
    }

    pub fn calculating(&self) -> bool {
        self.totals.is_none() && (self.task.is_some() || self.querying())
    }

    pub fn entry(&self, key: &TreeKey) -> Option<&TreeEntry> {
        self.rows
            .iter()
            .filter_map(TreeRow::entry)
            .find(|entry| &entry.key == key)
    }
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
    pub search: String,
    pub bucket_search: String,
    pub sort: Sort,
    pub ascending: bool,
    pub selected: HashSet<String>,
    pub object_view: ObjectView,
    pub tree: TreeBrowser,
    pub appearance: Appearance,
    pub show_info: bool,
    pub info: Option<ObjectInfo>,
    pub info_error: String,
    pub transfers: Vec<Transfer>,
    pub settings: Settings,
    pub s3_defaults: Option<S3Defaults>,
    pub expanded: bool,
    pub transfer_error: String,
    pub pending_transfers: HashSet<String>,
    pub loading: bool,
    list_loaded: bool,
    pub bucket_loading: bool,
    pub busy: bool,
    pub update_status: crate::updater::Status,
    pub update_pending: bool,
    pub update_error: String,
    pub error: String,
    pub dialog_error: String,
    pub toast: String,
    pub dialog: Option<Dialog>,
    pub form: Form,
    pub dialog_revision: u64,
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
    fn new(backend: Backend) -> Self {
        let mut this = Self {
            backend,
            accounts: vec![],
            active_id: String::new(),
            buckets: vec![],
            bucket: String::new(),
            prefix: String::new(),
            objects: vec![],
            next_token: None,
            search: String::new(),
            bucket_search: String::new(),
            sort: Sort::Name,
            ascending: true,
            selected: HashSet::new(),
            object_view: ObjectView::List,
            tree: TreeBrowser::new(),
            appearance: crate::appearance::preference(),
            show_info: false,
            info: None,
            info_error: String::new(),
            transfers: vec![],
            settings: Settings::default(),
            s3_defaults: None,
            expanded: true,
            transfer_error: String::new(),
            pending_transfers: HashSet::new(),
            loading: false,
            list_loaded: false,
            bucket_loading: false,
            busy: false,
            update_status: Default::default(),
            update_pending: false,
            update_error: String::new(),
            error: String::new(),
            dialog_error: String::new(),
            toast: String::new(),
            dialog: None,
            form: Form {
                inputs: HashMap::new(),
                path_style: true,
                s3_options: S3Defaults::default(),
            },
            dialog_revision: 0,
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
        this.request("get_s3_defaults", json!({}), Pending::S3Defaults);
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
    pub fn check_for_updates(&mut self) {
        if !self.can_open_app_dialog() {
            return;
        }
        self.open_dialog(Dialog::Update);
        if self.update_status.mode != "disabled" && self.update_status.phase != "checking" {
            self.update_action("check_app_update");
        }
    }
    pub fn update_action(&mut self, command: &'static str) {
        if self.update_pending || self.update_status.installing() {
            return;
        }
        self.update_pending = true;
        self.update_error.clear();
        self.request(command, json!({}), Pending::Update);
    }
    pub fn accept_update(&mut self, status: crate::updater::Status) {
        if status.revision >= self.update_status.revision {
            self.update_status = status;
        }
    }
    pub fn account(&self) -> Option<&Account> {
        self.accounts.iter().find(|a| a.id == self.active_id)
    }
    pub fn current_bucket(&self) -> Option<&Bucket> {
        self.buckets
            .iter()
            .find(|bucket| bucket.name == self.bucket)
    }
    pub fn visible(&self) -> Vec<S3Object> {
        model::visible(&self.objects, &self.search, self.sort, self.ascending)
    }
    pub fn chosen(&self) -> Vec<S3Object> {
        if self.object_view == ObjectView::Tree {
            return self
                .tree
                .rows
                .iter()
                .filter_map(TreeRow::entry)
                .filter(|entry| self.tree.selected.contains(&entry.key))
                .map(TreeEntry::object)
                .collect();
        }
        self.objects
            .iter()
            .filter(|o| self.selected.contains(&o.key))
            .cloned()
            .collect()
    }
    pub fn first(&self) -> Option<S3Object> {
        if self.selection_count() != 1 {
            return None;
        }
        let mut chosen = self.chosen().into_iter();
        let first = chosen.next();
        if chosen.len() == 0 { first } else { None }
    }
    pub fn selection_count(&self) -> usize {
        if self.object_view == ObjectView::Tree {
            self.tree.selected.len()
        } else {
            self.selected.len()
        }
    }
    pub fn clear_selection(&mut self) {
        self.selected.clear();
        self.tree.selected.clear();
        self.selection_changed();
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
    pub fn select_account(&mut self, id: String) {
        if self.is_busy() || id == self.active_id {
            return;
        }
        self.active_id = id;
        self.load_buckets(None);
    }
    fn load_buckets(&mut self, select: Option<String>) {
        self.bucket_revision += 1;
        self.object_revision += 1;
        self.info_revision += 1;
        self.buckets.clear();
        self.bucket.clear();
        self.objects.clear();
        self.list_loaded = false;
        self.selected.clear();
        self.tree.clear();
        self.prefix.clear();
        self.next_token = None;
        self.error.clear();
        self.info = None;
        self.search.clear();
        self.loading = false;
        self.bucket_loading = !self.active_id.is_empty();
        if self.bucket_loading {
            self.request(
                "list_buckets",
                json!({"accountId":self.active_id}),
                Pending::Buckets(self.bucket_revision, select),
            );
        }
    }
    pub fn navigate(&mut self, bucket: String, prefix: String) {
        if self.is_busy() || (self.bucket == bucket && self.prefix == prefix) {
            return;
        }
        self.bucket = bucket;
        self.prefix = prefix;
        self.search.clear();
        self.load_objects(false);
    }
    pub fn load_objects(&mut self, append: bool) {
        self.object_revision += 1;
        self.info_revision += 1;
        if !append {
            self.clear_list_page();
            self.selected.clear();
            self.tree.selected.clear();
            self.info = None;
            self.info_error.clear();
        }
        self.loading = false;
        self.error.clear();
        if self.active_id.is_empty() || self.bucket.is_empty() {
            self.tree.clear();
            return;
        }
        let scope = self.tree_scope();
        if self.tree.scope.as_ref().is_some_and(|old| old != &scope) {
            self.tree.clear();
        }
        if self.object_view == ObjectView::Tree {
            if self.tree.scope.is_none() {
                self.start_tree_scan(false);
            } else if self.tree.cache_available {
                self.update_tree_rows();
            } else {
                self.refresh_tree_levels(None);
            }
            return;
        }
        if !append && self.tree.scope.is_some() && !self.tree.cache_available {
            self.tree.live_needs_refresh = true;
        }
        self.load_list_page(append);
    }

    fn clear_list_page(&mut self) {
        self.objects.clear();
        self.list_loaded = false;
        self.next_token = None;
    }

    fn load_list_page(&mut self, append: bool) {
        self.loading = true;
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
    }

    pub fn refresh_objects(&mut self) {
        if self.object_view == ObjectView::Tree {
            self.clear_list_page();
            self.clear_selection();
            self.error.clear();
            self.start_tree_scan(true);
            if !self.tree.cache_available {
                self.refresh_tree_levels(None);
            }
        } else {
            self.load_objects(false);
        }
    }

    pub fn set_object_view(&mut self, view: ObjectView) {
        if self.object_view == view {
            return;
        }
        let tree_changed = self.object_view == ObjectView::Tree || view == ObjectView::Tree;
        self.object_view = view;
        if !tree_changed {
            return;
        }
        self.object_revision += 1;
        self.loading = false;
        self.error.clear();
        self.clear_selection();
        if view == ObjectView::Tree {
            if self.tree.scope.is_none()
                || (matches!(self.tree.status, TreeStatus::Idle) && !self.tree.cache_available)
            {
                self.start_tree_scan(false);
            } else {
                if !self.tree.cache_available && !matches!(self.tree.status, TreeStatus::Opening) {
                    if self.tree.live_needs_refresh {
                        self.refresh_tree_levels(None);
                        return;
                    } else {
                        self.ensure_tree_level(self.prefix.clone());
                    }
                }
                self.update_tree_rows();
            }
        } else {
            self.tree.cancel_query();
            if !self.list_loaded && !self.bucket.is_empty() {
                self.load_list_page(false);
            }
        }
    }

    fn tree_scope(&self) -> TreeScope {
        TreeScope::new(
            self.active_id.clone(),
            self.bucket.clone(),
            self.prefix.clone(),
        )
    }

    fn start_tree_scan(&mut self, force: bool) {
        if self.active_id.is_empty() || self.bucket.is_empty() {
            return;
        }
        let scope = self.tree_scope();
        if self.tree.scope.as_ref() != Some(&scope) {
            self.tree.clear();
            self.tree.scope = Some(scope.clone());
        }
        self.tree.cancel_scan();
        if force {
            self.tree.cancel_query();
        }
        self.tree.revision += 1;
        self.tree.status = TreeStatus::Opening;
        self.tree.task = Some(self.backend.scan_tree(self.tree.revision, scope, force));
        if !self.tree.cache_available && self.list_loaded {
            self.tree.live.seed(&self.prefix, &self.objects);
            self.update_tree_rows();
        }
    }

    pub fn cancel_tree_scan(&mut self) {
        self.tree.cancel_scan();
    }

    pub fn set_search(&mut self, value: &str) {
        if self.search == value {
            return;
        }
        self.search = value.into();
        self.tree.options.search_collapsed.clear();
        self.tree.options.search_pages.clear();
        if self.object_view == ObjectView::Tree {
            self.update_tree_rows();
        }
    }

    pub fn sort_tree(&mut self, sort: TreeSort) {
        let options = &mut self.tree.options;
        options.ascending = if options.sort == sort {
            !options.ascending
        } else {
            sort == TreeSort::Name
        };
        options.sort = sort;
        options.pages.clear();
        self.update_tree_rows();
    }

    pub fn toggle_tree_folder(&mut self, key: String) {
        let is_open = self.tree.rows.iter().any(|row| {
            matches!(row, TreeRow::Entry { item, expanded: true, .. } if item.key.key == key && item.key.folder)
        });
        if self.search.is_empty() {
            if is_open {
                self.tree.options.expanded.remove(&key);
            } else {
                self.tree.options.expanded.insert(key.clone());
            }
        } else if is_open {
            self.tree.options.search_collapsed.insert(key.clone());
        } else {
            self.tree.options.search_collapsed.remove(&key);
        }
        if !is_open && !self.tree.cache_available {
            self.ensure_tree_level(key);
        }
        self.update_tree_rows();
    }

    pub fn page_tree(&mut self, page: TreePage) {
        if !self.tree.cache_available {
            if let TreePage::Directory { parent, page } = page {
                self.request_tree_level(parent, page, false);
                self.update_tree_rows();
            }
            return;
        }
        self.tree.options.change_page(page);
        self.update_tree_rows();
    }

    fn ensure_tree_level(&mut self, prefix: String) {
        if self.tree.live.needs_page(&prefix) {
            self.request_tree_level(prefix, 0, false);
        }
    }

    fn request_tree_level(&mut self, prefix: String, page: usize, refreshing: bool) {
        if self.tree.scope.is_none() {
            return;
        }
        let Some(token) = self.tree.live.request(&prefix, page, refreshing) else {
            return;
        };
        let mut args = self.args();
        args["prefix"] = json!(prefix);
        args["token"] = json!(token);
        self.request(
            "list_tree_level",
            args,
            Pending::TreeLevel(self.tree.live.revision, prefix),
        );
    }

    fn refresh_tree_levels(&mut self, changed_key: Option<&str>) {
        self.tree.live_needs_refresh = false;
        let children = self
            .tree
            .rows
            .iter()
            .filter_map(|row| match row {
                TreeRow::Entry {
                    item,
                    expanded: true,
                    ..
                } => Some(item.key.key.clone()),
                _ => None,
            })
            .collect::<Vec<_>>();
        for prefix in std::iter::once(self.prefix.clone()).chain(children) {
            if changed_key.is_none_or(|key| key.starts_with(&prefix)) {
                self.request_tree_level(prefix, 0, true);
            }
        }
        self.update_tree_rows();
    }

    fn receive_tree_level(&mut self, revision: u64, prefix: String, result: Result<Value, String>) {
        if self.tree.live.revision != revision || self.tree.totals.is_some() {
            return;
        }
        let result =
            result.and_then(|data| serde_json::from_value(data).map_err(|error| error.to_string()));
        let queued = self.tree.live.receive(&prefix, result);
        // Keep the listing available as a fallback until the SQLite query
        // returns, while leaving the active query's pagination alone.
        if self.tree.cache_available {
            return;
        }
        if let Some(page) = self.tree.live.page_number(&prefix) {
            self.tree.options.pages.insert(prefix.clone(), page);
        }
        if queued {
            self.request_tree_level(prefix, 0, true);
        }
        self.update_tree_rows();
    }

    pub fn select_tree_node(&mut self, key: TreeKey) {
        if !self.tree.selected.remove(&key) {
            self.tree.selected.insert(key);
        }
        self.selection_changed();
    }

    pub fn inspect_tree_node(&mut self, key: TreeKey) {
        self.tree.selected = BTreeSet::from([key]);
        self.show_info = true;
        self.selection_changed();
    }

    fn update_tree_rows(&mut self) {
        if self.object_view != ObjectView::Tree {
            return;
        }
        let Some(scope) = self.tree.scope.clone() else {
            return;
        };
        self.tree.cancel_query();
        self.tree.query_error.clear();
        self.tree.options.search.clone_from(&self.search);
        if !self.tree.cache_available {
            self.set_tree_rows(self.tree.live.rows(&scope.prefix, &self.tree.options));
            return;
        }
        self.tree.query_revision += 1;
        self.tree.query = Some(self.backend.query_tree(
            self.tree.query_revision,
            scope,
            self.tree.options.clone(),
        ));
    }

    fn receive_tree_scanning(&mut self, revision: u64) {
        if self.tree.revision == revision {
            self.receive_tree_progress(revision, Totals::default());
            if self.object_view == ObjectView::Tree && !self.tree.cache_available {
                if self.tree.live_needs_refresh {
                    self.refresh_tree_levels(None);
                } else {
                    self.ensure_tree_level(self.prefix.clone());
                    self.update_tree_rows();
                }
            }
        }
    }

    fn receive_tree_progress(&mut self, revision: u64, totals: Totals) {
        if self.tree.revision == revision {
            self.tree.status = TreeStatus::Scanning(totals);
        }
    }

    fn receive_tree_ready(&mut self, revision: u64, result: Result<(), String>) {
        if self.tree.revision != revision {
            return;
        }
        self.tree.task = None;
        match result {
            Ok(()) => {
                self.tree.cache_available = true;
                self.tree.status = TreeStatus::Idle;
                self.update_tree_rows();
            }
            Err(error) => {
                self.tree.status = TreeStatus::Failed(error);
                if self.object_view == ObjectView::Tree && !self.tree.cache_available {
                    self.ensure_tree_level(self.prefix.clone());
                    self.update_tree_rows();
                }
            }
        }
    }

    fn set_tree_rows(&mut self, rows: Vec<TreeRow>) {
        let selected = self.tree.selected.len();
        if selected != 0 {
            self.tree.selected = rows
                .iter()
                .filter_map(TreeRow::entry)
                .filter(|entry| self.tree.selected.contains(&entry.key))
                .map(|entry| entry.key.clone())
                .collect();
        }
        self.tree.rows = Rc::new(rows);
        if selected != self.tree.selected.len() {
            self.selection_changed();
        }
    }

    fn receive_tree_rows(&mut self, revision: u64, result: Result<Option<TreeListing>, String>) {
        if self.tree.query_revision != revision {
            return;
        }
        self.tree.query = None;
        match result {
            Ok(Some(listing)) => {
                self.tree.totals = Some(listing.totals);
                self.tree.scanned_at = Some(listing.scanned_at);
                self.tree.dirty = listing.dirty;
                self.tree.live.clear();
                self.tree.live_needs_refresh = false;
                self.set_tree_rows(listing.rows);
            }
            Ok(None) => {
                self.tree.clear_result();
                if self.tree.task.is_none() {
                    self.start_tree_scan(false);
                }
                self.ensure_tree_level(self.prefix.clone());
                self.update_tree_rows();
            }
            Err(error) => {
                if self.tree.totals.is_none() {
                    self.tree.cache_available = false;
                    self.ensure_tree_level(self.prefix.clone());
                    self.update_tree_rows();
                }
                self.tree.query_error = error;
            }
        }
    }

    fn receive_tree_upload(&mut self, scope: TreeScope, key: &str) {
        if scope.account != self.active_id
            || scope.bucket != self.bucket
            || !key.starts_with(&self.prefix)
        {
            return;
        }
        if self.object_view == ObjectView::Tree && self.tree.totals.is_some() {
            self.clear_list_page();
            self.update_tree_rows();
        }
    }

    pub fn selection_changed(&mut self) {
        self.info_revision += 1;
        self.info = None;
        self.info_error.clear();
        if let Some(first) = self.first().filter(|o| self.show_info && !o.folder) {
            let mut args = self.args();
            args["key"] = json!(first.key);
            self.request("object_info", args, Pending::Info(self.info_revision));
        }
    }
    pub fn toggle(&mut self, key: &str) {
        if !self.selected.remove(key) {
            self.selected.insert(key.into());
        }
        self.selection_changed();
    }
    pub fn inspect(&mut self, key: String) {
        self.selected = HashSet::from([key]);
        self.show_info = true;
        self.selection_changed();
    }
    pub fn sort_by(&mut self, sort: Sort) {
        if self.sort == sort {
            self.ascending = !self.ascending;
        } else {
            self.sort = sort;
            self.ascending = true;
        }
    }
    pub fn close_dialog(&mut self) {
        if self.is_busy() || (self.dialog == Some(Dialog::Update) && self.update_pending) {
            return;
        }
        self.dialog = None;
        self.form.inputs.clear();
    }
    pub fn open_dialog(&mut self, dialog: Dialog) {
        if self.is_busy() {
            return;
        }
        self.dialog_revision += 1;
        self.dialog_error.clear();
        self.form.inputs.clear();
        self.form.path_style = true;
        self.form.s3_options = self.s3_defaults.clone().unwrap_or_default();
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
                    crate::i18n::t("例如：生产环境 / 个人存储"),
                    false,
                );
                let defaults = self.s3_defaults.clone().unwrap_or_default();
                self.add_input(
                    "endpoint",
                    account.as_ref().map(|a| a.endpoint.as_str()).unwrap_or(""),
                    if defaults.endpoint.is_empty() {
                        "https://s3.example.com"
                    } else {
                        &defaults.endpoint
                    },
                    false,
                );
                self.add_input(
                    "region",
                    account.as_ref().map(|a| a.region.as_str()).unwrap_or(""),
                    if defaults.region.is_empty() {
                        "us-east-1"
                    } else {
                        &defaults.region
                    },
                    false,
                );
                self.add_input(
                    "accessKey",
                    "",
                    if account.is_some() {
                        crate::i18n::t("留空保留现有凭据")
                    } else {
                        "AKIA…"
                    },
                    false,
                );
                self.add_input(
                    "secretKey",
                    "",
                    if account.is_some() {
                        crate::i18n::t("留空保留现有凭据")
                    } else {
                        crate::i18n::t("你的 Secret Access Key")
                    },
                    true,
                );
                self.add_input(
                    "sessionToken",
                    "",
                    crate::i18n::t("使用临时凭据时填写；更新凭据时留空可清除"),
                    true,
                );
            }
            Dialog::Folder => self.add_input("name", "", crate::i18n::t("新文件夹"), false),
            Dialog::Bucket | Dialog::BucketSettings => {
                if dialog == Dialog::Bucket {
                    self.add_input("name", "", "my-new-bucket", false);
                }
                let connection = self
                    .current_bucket()
                    .filter(|_| dialog == Dialog::BucketSettings)
                    .map(|bucket| bucket.connection.clone())
                    .unwrap_or_default();
                let defaults = &self.form.s3_options;
                let endpoint = self
                    .account()
                    .map(|account| defaults.endpoint_for(account))
                    .filter(|endpoint| !endpoint.is_empty())
                    .unwrap_or("https://s3.example.com")
                    .to_owned();
                let region = self
                    .account()
                    .map(|account| defaults.region_for(account))
                    .unwrap_or("us-east-1")
                    .to_owned();
                self.add_input("endpoint", &connection.endpoint, &endpoint, false);
                self.add_input("region", &connection.region, &region, false);
            }
            Dialog::Copy => {
                let name = format!(
                    "{}copy-{}",
                    self.prefix,
                    self.first()
                        .map(|o| model::object_name(&o.key, &self.prefix).to_owned())
                        .unwrap_or_default()
                );
                self.add_input("name", &name, &name, false);
            }
            Dialog::S3Defaults => {
                let defaults = self.s3_defaults.clone().unwrap_or_default();
                self.add_input(
                    "endpoint",
                    &defaults.endpoint,
                    "https://s3.example.com",
                    false,
                );
                self.add_input("region", &defaults.region, "us-east-1", false);
            }
            Dialog::TransferSettings => self.add_input(
                "concurrency",
                &self.settings.concurrency.to_string(),
                "",
                false,
            ),
            _ => {}
        }
        self.dialog = Some(dialog);
    }
    fn add_input(&mut self, key: &'static str, value: &str, placeholder: &str, masked: bool) {
        self.form.inputs.insert(
            key,
            Field {
                value: value.into(),
                placeholder: placeholder.into(),
                masked,
            },
        );
    }
    pub fn input(&self, key: &str) -> &str {
        self.form
            .inputs
            .get(key)
            .map_or("", |field| field.value.as_str())
    }
    pub fn set_input(&mut self, key: &str, value: &str) {
        if let Some(field) = self.form.inputs.get_mut(key) {
            field.value = value.to_owned();
        }
    }
    pub fn set_connection_value(&mut self, field: S3Option, value: &str) {
        self.set_input(field.key(), value);
        self.dialog_error.clear();
    }

    pub fn add_s3_option(&mut self, field: S3Option) {
        if self.is_busy() {
            return;
        }
        match field.normalize(self.input(field.key()).to_owned()) {
            Ok(value) => {
                self.set_connection_value(field, &value);
                if self.dialog == Some(Dialog::S3Defaults) {
                    self.form.s3_options.add_option(field, value);
                } else {
                    self.busy = true;
                    self.request(
                        "add_s3_option",
                        json!({"field":field,"value":value}),
                        Pending::AddS3Option,
                    );
                }
            }
            Err(error) => self.dialog_error = error,
        }
    }

    pub fn remove_s3_option(&mut self, field: S3Option, value: &str) {
        self.form.s3_options.remove_option(field, value);
        if field
            .normalize(self.input(field.key()).to_owned())
            .as_deref()
            == Ok(value)
        {
            self.set_connection_value(field, "");
        }
    }
    pub fn submit(&mut self) {
        if self.is_busy() {
            return;
        }
        let Some(dialog) = self.dialog.clone() else {
            return;
        };
        if matches!(dialog, Dialog::Help | Dialog::About) {
            self.close_dialog();
            return;
        }
        let mut args = self.args();
        let name = self.input("name").trim().to_owned();
        if matches!(
            dialog,
            Dialog::Account(_) | Dialog::Folder | Dialog::Bucket | Dialog::Copy
        ) && name.is_empty()
        {
            self.dialog_error = crate::i18n::t("请填写此字段。").into();
            return;
        }
        let command = match &dialog {
            Dialog::Account(id) => {
                let access = self.input("accessKey").trim().to_owned();
                let secret = self.input("secretKey");
                let region = self.input("region").trim().to_owned();
                if name.chars().count() > 80
                    || (id.is_none() && (access.is_empty() || secret.is_empty()))
                {
                    self.dialog_error =
                        crate::i18n::t("请填写完整的账户名称、Access Key 和 Secret Key。").into();
                    return;
                }
                args = json!({"input":{"id":id.clone().unwrap_or_default(),"name":name,"region":region,"endpoint":self.input("endpoint").trim(),"pathStyle":self.form.path_style,"accessKey":access,"secretKey":secret,"sessionToken":self.input("sessionToken")}});
                "save_account"
            }
            Dialog::Folder => {
                if name.contains(['/', '\\']) {
                    self.dialog_error = crate::i18n::t("文件夹名称不能包含 / 或 \\。").into();
                    return;
                }
                args["key"] = json!(format!("{}{name}/", self.prefix));
                "create_folder"
            }
            Dialog::Bucket | Dialog::BucketSettings => {
                if dialog == Dialog::Bucket && self.buckets.iter().any(|bucket| bucket.name == name)
                {
                    self.dialog_error =
                        crate::i18n::t("此账户已有同名存储桶，请编辑现有存储桶的连接设置。").into();
                    return;
                }
                args["name"] = json!(name);
                args["connection"] = json!(S3Connection {
                    endpoint: self.input("endpoint").to_owned(),
                    region: self.input("region").to_owned(),
                });
                if dialog == Dialog::Bucket {
                    "create_bucket"
                } else {
                    "set_bucket_connection"
                }
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
                self.close_dialog();
                "upload_files"
            }
            Dialog::RemoveAccount => "remove_account",
            Dialog::S3Defaults => {
                args["input"] = json!(S3Defaults {
                    endpoint: self.input("endpoint").to_owned(),
                    region: self.input("region").to_owned(),
                    ..self.form.s3_options.clone()
                });
                "set_s3_defaults"
            }
            Dialog::TransferSettings => {
                let n = self.input("concurrency").parse::<usize>();
                if !n.as_ref().is_ok_and(|n| (1..=32).contains(n)) {
                    self.dialog_error = crate::i18n::t("并发数量需为 1–32 的整数。").into();
                    return;
                }
                args["concurrency"] = json!(n.unwrap_or(8));
                "set_transfer_settings"
            }
            Dialog::CancelTransfer(id) => {
                let id = id.clone();
                self.close_dialog();
                self.control("cancel_transfer", id);
                return;
            }
            Dialog::Update | Dialog::Help | Dialog::About => return,
        };
        self.busy = true;
        self.dialog_error.clear();
        self.request(command, args, Pending::Submit(dialog));
    }
    pub fn download(&mut self) {
        if self.is_busy() {
            return;
        }
        if let Some(first) = self.first().filter(|o| !o.folder) {
            let mut args = self.args();
            args["key"] = json!(first.key);
            self.busy = true;
            self.error.clear();
            self.request("download_object", args, Pending::Download);
        }
    }
    pub fn share(&mut self) {
        if self.is_busy() {
            return;
        }
        if let Some(first) = self.first().filter(|o| !o.folder) {
            let mut args = self.args();
            args["key"] = json!(first.key);
            self.busy = true;
            self.error.clear();
            self.request("presign_object", args, Pending::Share);
        }
    }
    pub fn control(&mut self, command: &'static str, id: String) {
        if self.pending_transfers.contains(&id) {
            return;
        }
        self.pending_transfers.insert(id.clone());
        self.transfer_error.clear();
        self.request(command, json!({"transferId":id}), Pending::Control(id));
    }
    pub fn clear_finished(&mut self) {
        self.request("clear_finished_transfers", json!({}), Pending::Clear);
    }
    fn notify(&mut self, message: String) {
        self.toast = message;
        self.toast_revision += 1;
    }

    fn receive_transfer(&mut self, t: Transfer) -> bool {
        self.transfer_changes.insert(t.id.clone());
        let refresh = t.status == "done"
            && t.direction == s3_studio_core::transfers::Direction::Upload
            && self.refreshed_uploads.insert(t.id.clone())
            && t.account_id == self.active_id
            && t.bucket == self.bucket
            && (self.object_view != ObjectView::Tree || t.key.starts_with(&self.prefix));
        if refresh {
            if self.object_view == ObjectView::Tree {
                self.clear_list_page();
                if !self.tree.cache_available {
                    self.refresh_tree_levels(Some(&t.key));
                }
            } else {
                self.load_objects(false);
            }
        }
        if let Some(old) = self.transfers.iter_mut().find(|old| old.id == t.id) {
            *old = t;
        } else {
            self.transfers.insert(0, t);
        }
        refresh
    }

    fn receive_response(&mut self, id: u64, result: Result<Value, String>) {
        let Some(pending) = self.pending.remove(&id) else {
            return;
        };
        if let Pending::TreeLevel(revision, prefix) = pending {
            self.receive_tree_level(revision, prefix, result);
            return;
        }
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
            Pending::Submit(_) | Pending::AddS3Option | Pending::Download | Pending::Share => {
                self.busy = false
            }
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
                        self.load_objects(false);
                        self.error = error;
                    }
                    Pending::Submit(Dialog::Delete) => {
                        self.load_objects(false);
                        self.dialog_error = error;
                    }
                    Pending::Submit(_) | Pending::AddS3Option => self.dialog_error = error,
                    Pending::Info(_) => self.info_error = error,
                    Pending::Control(_)
                    | Pending::Clear
                    | Pending::Transfers
                    | Pending::Settings => self.transfer_error = error,
                    _ => self.error = error,
                }
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
                    self.load_buckets(None);
                }
                Pending::Buckets(_, select) => {
                    self.buckets = serde_json::from_value(data).map_err(|e| e.to_string())?;
                    self.bucket = select
                        .or_else(|| self.buckets.first().map(|b| b.name.clone()))
                        .unwrap_or_default();
                    self.load_objects(false);
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
                    self.list_loaded = true;
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
                Pending::S3Defaults => {
                    let defaults: S3Defaults =
                        serde_json::from_value(data).map_err(|e| e.to_string())?;
                    if self.dialog != Some(Dialog::S3Defaults) {
                        self.form.s3_options = defaults.clone();
                    }
                    self.s3_defaults = Some(defaults);
                }
                Pending::AddS3Option => {
                    let defaults: S3Defaults =
                        serde_json::from_value(data).map_err(|e| e.to_string())?;
                    self.form.s3_options = defaults.clone();
                    self.s3_defaults = Some(defaults);
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
                            crate::i18n::t("账户已安全保存").into()
                        }
                        Dialog::RemoveAccount => {
                            self.request("list_accounts", json!({}), Pending::Accounts(None));
                            crate::i18n::t("账户已移除").into()
                        }
                        Dialog::Bucket => {
                            let name = self.input("name").trim().to_owned();
                            self.load_buckets(Some(name));
                            crate::i18n::t("存储桶已创建").into()
                        }
                        Dialog::BucketSettings => {
                            let connection: S3Connection =
                                serde_json::from_value(data).map_err(|e| e.to_string())?;
                            if let Some(bucket) = self
                                .buckets
                                .iter_mut()
                                .find(|bucket| bucket.name == self.bucket)
                            {
                                bucket.connection = connection;
                            }
                            self.prefix.clear();
                            self.search.clear();
                            self.tree.clear();
                            self.load_objects(false);
                            crate::i18n::t("设置保存成功").into()
                        }
                        Dialog::S3Defaults => {
                            let defaults: S3Defaults =
                                serde_json::from_value(data).map_err(|e| e.to_string())?;
                            if let (Some(account), Some(old)) = (
                                self.accounts
                                    .iter()
                                    .find(|account| account.id == self.active_id),
                                self.s3_defaults.as_ref(),
                            ) {
                                if old.endpoint_for(account) != defaults.endpoint_for(account)
                                    || old.region_for(account) != defaults.region_for(account)
                                    || (account.region.is_empty()
                                        && old.region.is_empty() != defaults.region.is_empty())
                                {
                                    self.load_buckets(None);
                                }
                            }
                            self.s3_defaults = Some(defaults);
                            crate::i18n::t("设置保存成功").into()
                        }
                        Dialog::TransferSettings => {
                            self.settings =
                                serde_json::from_value(data).map_err(|e| e.to_string())?;
                            crate::i18n::t("设置保存成功").into()
                        }
                        Dialog::Delete => {
                            self.load_objects(false);
                            crate::lf!("已删除 {} 个对象", data.as_u64().unwrap_or_default())
                        }
                        Dialog::Upload => {
                            self.load_objects(false);
                            let count = data.as_u64().unwrap_or_default();
                            if count > 0 {
                                crate::lf!(
                                    "已添加 {count} 个上传任务，可在传输活动中暂停或继续",
                                    count = count
                                )
                            } else {
                                String::new()
                            }
                        }
                        Dialog::Folder => {
                            self.load_objects(false);
                            crate::i18n::t("文件夹已创建").into()
                        }
                        Dialog::Copy => {
                            self.load_objects(false);
                            crate::i18n::t("对象已复制").into()
                        }
                        _ => String::new(),
                    };
                    self.close_dialog();
                    if !message.is_empty() {
                        self.notify(message);
                    }
                }
                Pending::Share => {
                    if let Some(url) = data.as_str() {
                        set_clipboard(url);
                        self.notify(crate::i18n::t("已复制分享链接，1 小时内有效").into());
                    }
                }
                Pending::Clear => self
                    .transfers
                    .retain(|t| !matches!(t.status.as_str(), "done" | "cancelled")),
                Pending::Control(_) | Pending::Download | Pending::Update => {}
                Pending::TreeLevel(..) => unreachable!(),
            }
            Ok(())
        })();
        if let Err(error) = handled {
            self.error = error;
        }
    }
}

/// Workspace state belongs to the NSH engine thread. Progress ticks invalidate
/// the transfer panel without rebuilding the object browser or open form.
#[derive(Clone, Copy)]
pub struct Store {
    workspace: Stored<Rc<RefCell<Workspace>>>,
    revision: Signal<u64>,
    draft_revision: Signal<u64>,
    transfer_revision: Signal<u64>,
}
impl Store {
    pub fn new(backend: Backend, mut receiver: UnboundedReceiver<Message>) -> Self {
        let store = Self {
            workspace: stored(Rc::new(RefCell::new(Workspace::new(backend)))),
            revision: signal(0),
            draft_revision: signal(0),
            transfer_revision: signal(0),
        };
        spawn(async move {
            while let Some(message) = receiver.next().await {
                store.receive(message);
            }
        });
        store
    }

    pub fn read<T>(self, f: impl FnOnce(&Workspace) -> T) -> T {
        self.revision.get();
        // NSH can evaluate a control's reactive properties while building it.
        f(&self.workspace.get().borrow())
    }

    pub fn read_transfers<T>(self, f: impl FnOnce(&Workspace) -> T) -> T {
        self.transfer_revision.get();
        self.read(f)
    }

    pub fn read_draft<T>(self, f: impl FnOnce(&Workspace) -> T) -> T {
        self.draft_revision.get();
        self.read(f)
    }

    pub fn edit(self, key: &str, value: &str) {
        self.write(|workspace| workspace.set_input(key, value));
        self.draft_revision.update(|revision| *revision += 1);
    }

    pub fn change(self, f: impl FnOnce(&mut Workspace)) {
        self.write(f);
        self.revision.update(|revision| *revision += 1);
    }

    fn write<T>(self, f: impl FnOnce(&mut Workspace) -> T) -> T {
        f(&mut self.workspace.get().borrow_mut())
    }

    pub fn action(self, f: impl Fn(&mut Workspace) + 'static) -> impl Fn() + 'static {
        move || self.change(&f)
    }

    pub fn open(self, dialog: Dialog) -> impl Fn() + 'static {
        self.action(move |workspace| workspace.open_dialog(dialog.clone()))
    }

    fn receive(self, message: Message) {
        match message {
            Message::Transfer(transfer) => {
                let refresh = self.write(|workspace| workspace.receive_transfer(transfer));
                self.transfer_revision.update(|revision| *revision += 1);
                if refresh {
                    self.revision.update(|revision| *revision += 1);
                }
            }
            Message::Response(id, result) => {
                let added_toast = self.write(|workspace| {
                    let previous = workspace.toast_revision;
                    workspace.receive_response(id, result);
                    (workspace.toast_revision != previous).then_some(workspace.toast_revision)
                });
                self.revision.update(|revision| *revision += 1);
                if let Some(revision) = added_toast {
                    set_timeout(Duration::from_millis(4500), move || {
                        self.change(|workspace| {
                            if workspace.toast_revision == revision {
                                workspace.toast.clear();
                            }
                        });
                    });
                }
            }
            Message::Update(status) => self.change(|workspace| workspace.accept_update(status)),
            Message::TreeScanning(revision) => {
                self.change(|workspace| workspace.receive_tree_scanning(revision))
            }
            Message::TreeProgress(revision, totals) => {
                let visible = self.write(|workspace| {
                    workspace.receive_tree_progress(revision, totals);
                    workspace.object_view == ObjectView::Tree
                });
                if visible {
                    self.revision.update(|revision| *revision += 1);
                }
            }
            Message::TreeReady(revision, result) => {
                self.change(|workspace| workspace.receive_tree_ready(revision, result))
            }
            Message::TreeRows(revision, result) => {
                let scan_time = self.write(|workspace| {
                    let previous = workspace.tree.scanned_at;
                    workspace.receive_tree_rows(revision, result);
                    workspace
                        .tree
                        .scanned_at
                        .filter(|time| Some(*time) != previous)
                });
                self.revision.update(|revision| *revision += 1);
                if let Some(time) = scan_time {
                    let delay = time
                        .saturating_add(CACHE_LIFETIME_MS)
                        .saturating_sub(chrono::Utc::now().timestamp_millis());
                    if delay > 0 {
                        set_timeout(Duration::from_millis(delay as u64), move || {
                            self.revision.update(|revision| *revision += 1);
                        });
                    }
                }
            }
            Message::TreeUpload(scope, key) => {
                self.change(|workspace| workspace.receive_tree_upload(scope, &key))
            }
            Message::Restart(guard) => crate::restart_after_update(guard),
        }
    }
}

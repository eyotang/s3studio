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
    s3::{Bucket, ObjectInfo, ObjectPage, PathTarget, S3Object},
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
    Versions(u64, bool),
    RestoreVersion,
    Trash(u64, bool),
    RestoreTrash,
    ReorderAccounts(String, String),
    Accounts(Option<String>),
    Buckets(u64, Option<String>),
    Objects(u64, bool),
    LocatePath(String),
    AutoLocatePath { revision: u64, bucket: String },
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
    pub locate_object_only: bool,
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

struct BackgroundScan {
    scope: TreeScope,
    revision: u64,
    status: TreeStatus,
    task: TreeTask,
}

pub struct TreeBrowser {
    pub status: TreeStatus,
    pub options: TreeOptions,
    pub rows: Rc<Vec<TreeRow>>,
    pub list_sizes: HashMap<String, u128>,
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
            list_sizes: HashMap::new(),
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
            self.revision = 0;
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
        self.list_sizes.clear();
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

#[derive(Default)]
pub struct TrashBrowser {
    pub objects: Vec<s3_studio_core::s3::trash::TrashObject>,
    pub selected: HashSet<String>,
    pub cursor: Option<s3_studio_core::s3::trash::Cursor>,
    pub versioning: Option<bool>,
    pub loading: bool,
    pub notice: String,
}

#[derive(Default)]
pub struct VersionBrowser {
    pub key: String,
    pub entries: Vec<s3_studio_core::s3::versions::ObjectVersion>,
    pub selected: Option<String>,
    pub expected: String,
    pub cursor: Option<s3_studio_core::s3::trash::Cursor>,
    pub loading: bool,
    pub notice: String,
    pub from_trash: bool,
}

pub struct Workspace {
    pub versions: VersionBrowser,
    pub trash: TrashBrowser,
    list_cache: crate::list_cache::ListCache,
    pub backend: Backend,
    pub accounts: Vec<Account>,
    pub account_reordering: bool,
    pub active_id: String,
    pub buckets: Vec<Bucket>,
    pub bucket: String,
    pub prefix: String,
    pub objects: Vec<S3Object>,
    // Exact key and listing revision for the pinned location result.
    pub located: Option<(String, u64)>,
    pub next_token: Option<String>,
    pub search: String,
    search_revision: u64,
    search_is_location: bool,
    pub bucket_search: String,
    pub sort: Sort,
    pub ascending: bool,
    pub selected: HashSet<String>,
    pub object_view: ObjectView,
    pub tree: TreeBrowser,
    background_scans: Vec<BackgroundScan>,
    scan_revision: u64,
    pub appearance: Appearance,
    pub show_info: bool,
    pub info: Option<ObjectInfo>,
    pub info_error: String,
    pub transfers: Vec<Transfer>,
    pub settings: Settings,
    pub s3_defaults: Option<S3Defaults>,
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
            versions: VersionBrowser::default(),
            trash: TrashBrowser::default(),
            list_cache: Default::default(),
            backend,
            accounts: vec![],
            account_reordering: false,
            active_id: String::new(),
            buckets: vec![],
            bucket: String::new(),
            prefix: String::new(),
            objects: vec![],
            located: None,
            next_token: None,
            search: String::new(),
            search_revision: 0,
            search_is_location: false,
            bucket_search: String::new(),
            sort: Sort::Name,
            ascending: true,
            selected: HashSet::new(),
            object_view: ObjectView::List,
            tree: TreeBrowser::new(),
            background_scans: Vec::new(),
            scan_revision: 0,
            appearance: crate::appearance::preference(),
            show_info: false,
            info: None,
            info_error: String::new(),
            transfers: vec![],
            settings: Settings::default(),
            s3_defaults: None,
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
                locate_object_only: false,
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
        self.account_by_id(&self.active_id)
    }
    pub fn account_by_id(&self, id: &str) -> Option<&Account> {
        self.accounts.iter().find(|account| account.id == id)
    }

    pub fn visible(&self) -> Vec<S3Object> {
        let mut rows = model::visible(
            &self.objects,
            self.search_query(),
            self.sort,
            self.ascending,
        );
        if let Some((key, _)) = &self.located
            && let Some(position) = rows.iter().position(|object| &object.key == key)
        {
            rows[..=position].rotate_right(1);
        }
        rows
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
        self.cancel_auto_location();
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
    pub fn reorder_accounts(&mut self, source: String, target: String) {
        if self.is_busy() || self.account_reordering || source == target {
            return;
        }
        self.account_reordering = true;
        self.request(
            "reorder_accounts",
            json!({"source":source,"target":target}),
            Pending::ReorderAccounts(source, target),
        );
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
        self.clear_list_page();
        self.selected.clear();
        self.park_tree_scan();
        self.tree.clear();
        self.prefix.clear();
        self.error.clear();
        self.info = None;
        self.clear_search();
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
        self.clear_search();
        self.load_objects_cached(false, true);
    }
    pub fn load_objects(&mut self, append: bool) {
        if !append {
            self.list_cache
                .invalidate_bucket(&self.active_id, &self.bucket);
        }
        self.load_objects_cached(append, false);
    }
    fn load_objects_cached(&mut self, append: bool, reuse: bool) {
        if append {
            self.cancel_auto_location();
        }
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
            self.park_tree_scan();
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
        if self.tree.scope.is_none() {
            self.start_tree_scan(false);
        }
        if reuse && let Some(page) = self.list_cache.get(&scope) {
            self.objects = page.objects;
            self.next_token = page.next_token;
            self.list_loaded = true;
            self.update_tree_rows();
            return;
        }
        self.load_list_page(append);
    }

    fn clear_list_page(&mut self) {
        self.tree.cancel_query();
        self.tree.list_sizes.clear();
        self.objects.clear();
        self.located = None;
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
        self.list_cache
            .invalidate_bucket(&self.active_id, &self.bucket);
        self.cancel_auto_location();
        if self.object_view == ObjectView::Tree {
            self.clear_list_page();
            self.clear_selection();
            self.error.clear();
            self.start_tree_scan(true);
            if !self.tree.cache_available {
                self.refresh_tree_levels(None);
            }
        } else {
            self.start_tree_scan(true);
            self.load_objects(false);
        }
    }

    pub fn set_object_view(&mut self, view: ObjectView) {
        if self.object_view == view {
            return;
        }
        self.cancel_auto_location();
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
            self.update_tree_rows();
        }
    }

    fn tree_scope(&self) -> TreeScope {
        TreeScope::new(
            self.active_id.clone(),
            self.bucket.clone(),
            self.prefix.clone(),
        )
    }

    fn park_tree_scan(&mut self) {
        if let (Some(scope), Some(task)) = (self.tree.scope.clone(), self.tree.task.take()) {
            self.background_scans.push(BackgroundScan {
                scope,
                revision: self.tree.revision,
                status: std::mem::replace(&mut self.tree.status, TreeStatus::Idle),
                task,
            });
            self.tree.revision = 0;
        }
    }

    fn start_tree_scan(&mut self, force: bool) {
        if self.active_id.is_empty() || self.bucket.is_empty() {
            return;
        }
        let scope = self.tree_scope();
        if self.tree.scope.as_ref() != Some(&scope) {
            self.park_tree_scan();
            self.tree.clear();
            self.tree.scope = Some(scope.clone());
        }
        self.tree.cancel_scan();
        if let Some(index) = self
            .background_scans
            .iter()
            .position(|scan| scan.scope == scope)
        {
            let scan = self.background_scans.swap_remove(index);
            if !force {
                self.tree.revision = scan.revision;
                self.tree.status = scan.status;
                self.tree.task = Some(scan.task);
                return;
            }
        }
        if force {
            self.tree.cancel_query();
        }
        self.scan_revision += 1;
        self.tree.revision = self.scan_revision;
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

    pub fn search_query(&self) -> &str {
        if self.search_is_location {
            ""
        } else {
            &self.search
        }
    }

    fn cancel_auto_location(&mut self) {
        self.search_revision += 1;
    }

    fn clear_search(&mut self) {
        self.search.clear();
        self.search_is_location = false;
        self.cancel_auto_location();
    }

    fn set_search(&mut self, value: &str) -> bool {
        if self.search == value {
            return false;
        }
        self.search = value.into();
        self.search_is_location = false;
        self.cancel_auto_location();
        self.tree.options.search_collapsed.clear();
        self.tree.options.search_pages.clear();
        if self.object_view == ObjectView::Tree {
            self.update_tree_rows();
        }
        true
    }

    pub fn sort_tree(&mut self, sort: TreeSort) {
        self.cancel_auto_location();
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
        self.cancel_auto_location();
        let is_open = self.tree.rows.iter().any(|row| {
            matches!(row, TreeRow::Entry { item, expanded: true, .. } if item.key.key == key && item.key.folder)
        });
        if self.search_query().is_empty() {
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
        self.cancel_auto_location();
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
        self.cancel_auto_location();
        if !self.tree.selected.remove(&key) {
            self.tree.selected.insert(key);
        }
        self.selection_changed();
    }

    pub fn inspect_tree_node(&mut self, key: TreeKey) {
        self.cancel_auto_location();
        self.tree.selected = BTreeSet::from([key]);
        self.show_info = true;
        self.selection_changed();
    }

    fn update_tree_rows(&mut self) {
        if self.object_view != ObjectView::Tree {
            if self.tree.cache_available && self.list_loaded {
                self.tree.cancel_query();
                self.tree.query_revision += 1;
                let keys = self
                    .objects
                    .iter()
                    .filter(|o| o.folder)
                    .map(|o| o.key.clone())
                    .collect();
                self.tree.query = Some(self.backend.query_list_sizes(
                    self.tree.query_revision,
                    self.tree_scope(),
                    keys,
                ));
            }
            return;
        }
        let Some(scope) = self.tree.scope.clone() else {
            return;
        };
        self.tree.cancel_query();
        self.tree.query_error.clear();
        self.tree.options.search = self.search_query().to_owned();
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
        if let Some(scan) = self
            .background_scans
            .iter_mut()
            .find(|scan| scan.revision == revision)
        {
            scan.status = TreeStatus::Scanning(Totals::default());
            return;
        }
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
        if let Some(scan) = self
            .background_scans
            .iter_mut()
            .find(|scan| scan.revision == revision)
        {
            scan.status = TreeStatus::Scanning(totals);
            return;
        }
        if self.tree.revision == revision {
            self.tree.status = TreeStatus::Scanning(totals);
        }
    }

    fn receive_tree_ready(&mut self, revision: u64, result: Result<(), String>) {
        if let Some(index) = self
            .background_scans
            .iter()
            .position(|scan| scan.revision == revision)
        {
            self.background_scans.swap_remove(index);
            return;
        }
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
        self.list_cache
            .invalidate_bucket(&scope.account, &scope.bucket);
        if scope.account != self.active_id
            || scope.bucket != self.bucket
            || !key.starts_with(&self.prefix)
        {
            return;
        }
        if self.object_view == ObjectView::Tree && self.tree.totals.is_some() {
            self.clear_list_page();
            self.update_tree_rows();
        } else if self.tree.cache_available {
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
        self.cancel_auto_location();
        if !self.selected.remove(key) {
            self.selected.insert(key.into());
        }
        self.selection_changed();
    }
    pub fn inspect(&mut self, key: String) {
        self.cancel_auto_location();
        self.selected = HashSet::from([key]);
        self.show_info = true;
        self.selection_changed();
    }
    pub fn sort_by(&mut self, sort: Sort) {
        self.cancel_auto_location();
        if self.sort == sort {
            self.ascending = !self.ascending;
        } else {
            self.sort = sort;
            self.ascending = true;
        }
    }
    pub fn open_versions(&mut self, key: String) {
        if self.is_busy() || self.bucket.is_empty() || key.is_empty() {
            return;
        }
        let from_trash = self.dialog == Some(Dialog::Trash);
        self.open_dialog(Dialog::Versions);
        self.versions = VersionBrowser {
            key,
            from_trash,
            ..Default::default()
        };
        self.load_versions(false);
    }
    pub fn load_versions(&mut self, append: bool) {
        if self.versions.loading || self.is_busy() || self.dialog != Some(Dialog::Versions) {
            return;
        }
        self.versions.loading = true;
        self.dialog_error.clear();
        let cursor = if append {
            self.versions.cursor.clone()
        } else {
            self.versions.selected = None;
            None
        };
        self.request("list_versions", json!({"accountId":self.active_id,"bucket":self.bucket,"key":self.versions.key,"cursor":cursor}), Pending::Versions(self.dialog_revision, append));
    }
    pub fn restore_version(&mut self) {
        if self.is_busy() || self.versions.loading || self.dialog != Some(Dialog::Versions) {
            return;
        }
        let Some(version) = self.versions.selected.clone().filter(|id| {
            self.versions
                .entries
                .iter()
                .any(|v| &v.id == id && !v.deleted && !v.latest)
        }) else {
            return;
        };
        if self.versions.expected.is_empty() {
            return;
        }
        self.busy = true;
        self.versions.notice.clear();
        self.dialog_error.clear();
        self.request(
            "restore_version",
            json!({"accountId":self.active_id,"bucket":self.bucket,"key":self.versions.key,
            "version":version,"expected":self.versions.expected}),
            Pending::RestoreVersion,
        );
    }

    pub fn open_trash(&mut self) {
        if self.is_busy() || self.bucket.is_empty() {
            return;
        }
        self.open_dialog(Dialog::Trash);
        self.trash = TrashBrowser::default();
        self.load_trash(false);
    }
    pub fn load_trash(&mut self, append: bool) {
        if self.trash.loading || self.is_busy() || self.dialog != Some(Dialog::Trash) {
            return;
        }
        self.trash.loading = true;
        self.dialog_error.clear();
        let cursor = if append {
            self.trash.cursor.clone()
        } else {
            self.trash.selected.clear();
            None
        };
        self.request(
            "list_trash",
            json!({"accountId":self.active_id,"bucket":self.bucket,"cursor":cursor}),
            Pending::Trash(self.dialog_revision, append),
        );
    }
    pub fn restore_trash(&mut self) {
        if self.is_busy() || self.trash.loading || self.dialog != Some(Dialog::Trash) {
            return;
        }
        let objects: Vec<_> = self
            .trash
            .objects
            .iter()
            .filter(|o| self.trash.selected.contains(&o.key))
            .cloned()
            .collect();
        if objects.is_empty() {
            return;
        }
        self.busy = true;
        self.trash.notice.clear();
        self.dialog_error.clear();
        self.request(
            "restore_trash",
            json!({"accountId":self.active_id,"bucket":self.bucket,"objects":objects}),
            Pending::RestoreTrash,
        );
    }

    pub fn close_dialog(&mut self) {
        if self.is_busy() || (self.dialog == Some(Dialog::Update) && self.update_pending) {
            return;
        }
        self.dialog = None;
        self.form.inputs.clear();
    }

    pub fn locate_search(&mut self, path: &str) {
        if self.dialog.is_some() || self.is_busy() || self.bucket.is_empty() || path.is_empty() {
            return;
        }
        self.open_dialog(Dialog::LocatePath);
        self.set_input("path", path);
        self.submit();
    }

    fn locate_path(&mut self) {
        let (bucket, args) =
            match self.path_request(self.input("path"), self.form.locate_object_only) {
                Ok(request) => request,
                Err(error) => {
                    self.dialog_error = error.into();
                    return;
                }
            };
        self.busy = true;
        self.dialog_error.clear();
        self.request("locate_path", args, Pending::LocatePath(bucket));
    }

    fn path_request(&self, path: &str, object_only: bool) -> Result<(String, Value), &'static str> {
        if path.is_empty() {
            return Err(crate::i18n::t("请填写此字段。"));
        }
        let (bucket, key) = match path.strip_prefix("s3://") {
            Some(uri) => uri.split_once('/').unwrap_or((uri, "")),
            None => (self.bucket.as_str(), path),
        };
        if !self
            .buckets
            .iter()
            .any(|candidate| candidate.name == bucket)
        {
            return Err(crate::i18n::t("路径中的存储桶不在当前账户的列表中。"));
        }
        if key.len() > 1024 || (object_only && key.is_empty()) {
            return Err(crate::i18n::t("对象键无效。"));
        }
        let args = json!({
            "accountId": self.active_id, "bucket": bucket, "key": key, "objectOnly": object_only,
        });
        Ok((bucket.to_owned(), args))
    }

    fn automatic_path(&self) -> Option<(String, Value)> {
        let query = self.search_query();
        let object_only = if query.starts_with("s3://") {
            false
        } else if query.contains("://") {
            return None;
        } else if query.contains('/') {
            false
        } else if self.prefix.is_empty()
            && query.rsplit_once('.').is_some_and(|(name, extension)| {
                !name.is_empty()
                    && !extension.is_empty()
                    && !extension.contains(char::is_whitespace)
            })
        {
            true
        } else {
            return None;
        };
        self.path_request(query, object_only).ok()
    }

    fn can_auto_locate(&self, revision: u64) -> bool {
        self.search_revision == revision && self.dialog.is_none() && !self.is_busy()
    }

    fn start_auto_location(&mut self, revision: u64, bucket: String, args: Value) {
        if self.can_auto_locate(revision) {
            self.request(
                "locate_path",
                args,
                Pending::AutoLocatePath { revision, bucket },
            );
        }
    }

    fn reveal_path(&mut self, bucket: String, target: PathTarget, preserve_search: bool) {
        let (prefix, located) = match target {
            PathTarget::Directory { prefix } => (prefix, None),
            PathTarget::Object { object, info } => {
                let prefix = object
                    .key
                    .rfind('/')
                    .map_or("", |slash| &object.key[..=slash]);
                (prefix.to_owned(), Some((object, info)))
            }
        };
        self.object_view = ObjectView::List;
        self.tree.cancel_query();
        self.bucket = bucket;
        self.prefix = prefix;
        if preserve_search {
            self.search_is_location = true;
            self.cancel_auto_location();
        } else {
            self.clear_search();
        }
        self.load_objects_cached(false, located.is_none());
        if let Some((object, info)) = located {
            self.selected.insert(object.key.clone());
            self.located = Some((object.key.clone(), self.object_revision));
            self.objects.push(object);
            self.show_info = true;
            self.info = info;
        }
    }

    pub fn open_dialog(&mut self, dialog: Dialog) {
        if self.is_busy() {
            return;
        }
        if let Dialog::Account(Some(id)) | Dialog::RemoveAccount(id) = &dialog
            && self.account_by_id(id).is_none()
        {
            return;
        }
        self.cancel_auto_location();
        self.dialog_revision += 1;
        self.dialog_error.clear();
        self.form.inputs.clear();
        self.form.path_style = true;
        self.form.locate_object_only = false;
        self.form.s3_options = self.s3_defaults.clone().unwrap_or_default();
        match &dialog {
            Dialog::Account(id) => {
                let account = id.as_ref().and_then(|id| self.account_by_id(id)).cloned();
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
            Dialog::Bucket => {
                self.add_input("name", "", "my-new-bucket", false);
                let region = self
                    .account()
                    .map(|account| self.form.s3_options.region_for(account))
                    .unwrap_or("us-east-1")
                    .to_owned();
                self.add_input("region", "", &region, false);
            }
            Dialog::LocatePath => self.add_input(
                "path",
                &self.search.clone(),
                "s3://my-bucket/photos/",
                false,
            ),
            Dialog::Copy => {
                let name = format!(
                    "{}copy-{}",
                    self.prefix,
                    self.first()
                        .map(|o| model::object_name(&o.key, &self.prefix)
                            .trim_end_matches('/')
                            .to_owned())
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
            Dialog::LocatePath => {
                self.locate_path();
                return;
            }
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
            Dialog::Bucket => {
                if self.buckets.iter().any(|bucket| bucket.name == name) {
                    self.dialog_error = crate::i18n::t("此工作空间已有同名存储桶。").into();
                    return;
                }
                args["name"] = json!(name);
                args["connection"] = json!(S3Connection {
                    endpoint: String::new(),
                    region: self.input("region").to_owned(),
                });
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
                self.close_dialog();
                "upload_files"
            }
            Dialog::RemoveAccount(id) => {
                args = json!({"accountId":id});
                "remove_account"
            }
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
            Dialog::Versions => {
                self.restore_version();
                return;
            }
            Dialog::Trash => {
                self.restore_trash();
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
        let uploaded = t.status == "done"
            && t.direction == s3_studio_core::transfers::Direction::Upload
            && self.refreshed_uploads.insert(t.id.clone());
        if uploaded {
            self.list_cache.invalidate_bucket(&t.account_id, &t.bucket);
        }
        self.transfer_changes.insert(t.id.clone());
        let refresh = uploaded
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
        if let Pending::AutoLocatePath { revision, bucket } = pending {
            if self.can_auto_locate(revision)
                && let Ok(data) = result
                && let Ok(target) = serde_json::from_value(data)
            {
                self.reveal_path(bucket, target, true);
            }
            return;
        }
        if matches!(&pending, Pending::Versions(rev, _) if *rev != self.dialog_revision || self.dialog != Some(Dialog::Versions))
        {
            return;
        }
        if matches!(&pending, Pending::Trash(rev, _) if *rev != self.dialog_revision || self.dialog != Some(Dialog::Trash))
        {
            return;
        }
        if matches!(pending,Pending::Buckets(rev,_) if rev!=self.bucket_revision)
            || matches!(pending,Pending::Objects(rev,_) if rev!=self.object_revision)
            || matches!(pending,Pending::Info(rev) if rev!=self.info_revision)
        {
            return;
        }
        match &pending {
            Pending::Versions(..) => self.versions.loading = false,
            Pending::RestoreVersion => self.busy = false,
            Pending::Trash(..) => self.trash.loading = false,
            Pending::RestoreTrash => self.busy = false,
            Pending::ReorderAccounts(..) => self.account_reordering = false,
            Pending::Update => self.update_pending = false,
            Pending::Buckets(..) => self.bucket_loading = false,
            Pending::Objects(..) => self.loading = false,
            Pending::Submit(_)
            | Pending::LocatePath(_)
            | Pending::AddS3Option
            | Pending::Download
            | Pending::Share => self.busy = false,
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
                    Pending::Versions(..)
                    | Pending::RestoreVersion
                    | Pending::Trash(..)
                    | Pending::RestoreTrash
                    | Pending::Submit(_)
                    | Pending::LocatePath(_)
                    | Pending::AddS3Option => self.dialog_error = error,
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
                Pending::Versions(_, append) => {
                    let page: s3_studio_core::s3::versions::VersionPage =
                        serde_json::from_value(data).map_err(|e| e.to_string())?;
                    if !append {
                        self.versions.entries.clear();
                        self.versions.expected = page
                            .versions
                            .iter()
                            .find(|v| v.latest)
                            .map(|v| v.id.clone())
                            .unwrap_or_default();
                    }
                    for version in page.versions {
                        if !self.versions.entries.iter().any(|v| v.id == version.id) {
                            self.versions.entries.push(version);
                        }
                    }
                    self.versions.cursor = page.cursor;
                }
                Pending::RestoreVersion => {
                    self.versions.notice = crate::i18n::t("已还原为新的当前版本。").into();
                    self.list_cache.clear();
                    self.refresh_objects();
                    self.load_versions(false);
                }

                Pending::Trash(_, append) => {
                    let page: s3_studio_core::s3::trash::TrashPage =
                        serde_json::from_value(data).map_err(|e| e.to_string())?;
                    self.trash.versioning = Some(page.versioning);
                    if !append {
                        self.trash.objects.clear();
                    }
                    for object in page.objects {
                        if !self.trash.objects.iter().any(|old| old.key == object.key) {
                            self.trash.objects.push(object);
                        }
                    }
                    self.trash.cursor = page.cursor;
                }
                Pending::RestoreTrash => {
                    let result: s3_studio_core::s3::trash::RestoreResult =
                        serde_json::from_value(data).map_err(|e| e.to_string())?;
                    self.trash.notice =
                        format!("{}: {}", crate::i18n::t("已还原"), result.restored.len());
                    self.trash
                        .objects
                        .retain(|o| !result.restored.contains(&o.key));
                    self.trash
                        .selected
                        .retain(|key| !result.restored.contains(key));
                    self.dialog_error = result
                        .failed
                        .into_iter()
                        .map(|(key, error)| format!("{key}: {error}"))
                        .collect::<Vec<_>>()
                        .join("\n");
                    self.list_cache.clear();
                    self.refresh_objects();
                    // Version cursors may refer to markers just removed by this restore.
                    if self.trash.cursor.is_some() {
                        let failures = self.dialog_error.clone();
                        self.trash.cursor = None;
                        self.load_trash(false);
                        self.dialog_error = failures;
                    }
                }

                Pending::ReorderAccounts(source, target) => {
                    if let (Some(from), Some(to)) = (
                        self.accounts.iter().position(|a| a.id == source),
                        self.accounts.iter().position(|a| a.id == target),
                    ) {
                        let account = self.accounts.remove(from);
                        self.accounts.insert(to, account);
                    }
                }
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
                        let pinned = self.located.as_ref().and_then(|(key, _)| {
                            self.objects
                                .iter()
                                .find(|object| &object.key == key)
                                .cloned()
                        });
                        self.objects = page.objects;
                        if let Some(pinned) = pinned {
                            if let Some(entry) = self
                                .objects
                                .iter_mut()
                                .find(|entry| entry.key == pinned.key)
                            {
                                if entry.folder {
                                    *entry = pinned;
                                }
                            } else {
                                self.objects.push(pinned);
                            }
                        }
                    }
                    self.next_token = page.next_token;
                    self.list_loaded = true;
                    if self.located.is_none() {
                        self.list_cache.insert(
                            self.tree_scope(),
                            ObjectPage {
                                objects: self.objects.clone(),
                                next_token: self.next_token.clone(),
                            },
                        );
                    }
                    self.update_tree_rows();
                }
                Pending::Info(_) => {
                    self.info = Some(serde_json::from_value(data).map_err(|e| e.to_string())?)
                }
                Pending::LocatePath(bucket) => {
                    let target: PathTarget =
                        serde_json::from_value(data).map_err(|e| e.to_string())?;
                    self.close_dialog();
                    self.reveal_path(bucket, target, false);
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
                    self.list_cache.clear();
                    if matches!(
                        &dialog,
                        Dialog::Account(_) | Dialog::RemoveAccount(_) | Dialog::S3Defaults
                    ) {
                        self.background_scans.clear();
                        self.tree.cancel_scan();
                    }
                    let message = match dialog {
                        Dialog::Account(id) => {
                            let saved: Account =
                                serde_json::from_value(data).map_err(|e| e.to_string())?;
                            if id.is_some() && saved.id != self.active_id {
                                if let Some(account) =
                                    self.accounts.iter_mut().find(|a| a.id == saved.id)
                                {
                                    *account = saved;
                                }
                            } else {
                                self.request(
                                    "list_accounts",
                                    json!({}),
                                    Pending::Accounts(Some(saved.id)),
                                );
                            }
                            crate::i18n::t("账户已安全保存").into()
                        }
                        Dialog::RemoveAccount(id) => {
                            self.accounts.retain(|account| account.id != id);
                            if self.active_id == id {
                                self.active_id = self
                                    .accounts
                                    .first()
                                    .map(|a| a.id.clone())
                                    .unwrap_or_default();
                                self.load_buckets(None);
                            }
                            crate::i18n::t("工作空间配置已删除").into()
                        }
                        Dialog::Bucket => {
                            let name = self.input("name").trim().to_owned();
                            self.load_buckets(Some(name));
                            crate::i18n::t("存储桶已创建").into()
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
                Pending::TreeLevel(..) | Pending::AutoLocatePath { .. } => unreachable!(),
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
    sizes_revision: Signal<u64>,
    transfer_revision: Signal<u64>,
}
impl Store {
    pub fn new(backend: Backend, mut receiver: UnboundedReceiver<Message>) -> Self {
        let store = Self {
            workspace: stored(Rc::new(RefCell::new(Workspace::new(backend)))),
            revision: signal(0),
            draft_revision: signal(0),
            sizes_revision: signal(0),
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

    pub fn read_sizes<T>(self, f: impl FnOnce(&Workspace) -> T) -> T {
        self.sizes_revision.get();
        self.read(f)
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

    pub fn set_search(self, value: &str) {
        let Some((revision, location)) = self.write(|workspace| {
            workspace
                .set_search(value)
                .then(|| (workspace.search_revision, workspace.automatic_path()))
        }) else {
            return;
        };
        self.revision.update(|revision| *revision += 1);
        if let Some((bucket, args)) = location {
            set_timeout(Duration::from_millis(550), move || {
                self.write(|workspace| workspace.start_auto_location(revision, bucket, args));
            });
        }
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
                let tree = self.write(|workspace| {
                    workspace.receive_tree_scanning(revision);
                    workspace.object_view == ObjectView::Tree && workspace.tree.revision == revision
                });
                if tree {
                    self.revision.update(|v| *v += 1);
                } else {
                    self.sizes_revision.update(|v| *v += 1);
                }
            }
            Message::TreeProgress(revision, totals) => {
                let visible = self.write(|workspace| {
                    workspace.receive_tree_progress(revision, totals);
                    workspace.object_view == ObjectView::Tree && workspace.tree.revision == revision
                });
                if visible {
                    self.revision.update(|revision| *revision += 1);
                }
            }
            Message::TreeReady(revision, result) => {
                let tree = self.write(|workspace| {
                    workspace.receive_tree_ready(revision, result);
                    workspace.object_view == ObjectView::Tree && workspace.tree.revision == revision
                });
                if tree {
                    self.revision.update(|v| *v += 1);
                } else {
                    self.sizes_revision.update(|v| *v += 1);
                }
            }
            Message::ListSizes(revision, result) => {
                self.write(|workspace| {
                    if workspace.tree.query_revision == revision
                        && workspace.object_view != ObjectView::Tree
                    {
                        workspace.tree.query = None;
                        if let Ok(sizes) = result {
                            workspace.tree.list_sizes = sizes;
                        }
                    }
                });
                self.sizes_revision.update(|v| *v += 1);
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cached_navigation_does_not_request_objects_but_refresh_does() {
        let (backend, _receiver) = Backend::open(true).unwrap();
        let mut workspace = Workspace::new(backend);
        workspace.active_id = "demo-aws".into();
        workspace.bucket = "studio-assets".into();
        let root = workspace.tree_scope();
        workspace.list_cache.insert(
            root,
            ObjectPage {
                objects: vec![],
                next_token: Some("next-page".into()),
            },
        );
        workspace.prefix = "child/".into();
        let before = workspace.next_request;
        workspace.navigate("studio-assets".into(), "".into());
        assert_eq!(workspace.next_request, before);
        assert!(!workspace.loading);
        assert!(workspace.list_loaded);
        assert_eq!(workspace.next_token.as_deref(), Some("next-page"));
        workspace.refresh_objects();
        assert!(workspace.loading);
        assert!(workspace.next_request > before);
        assert!(workspace.list_cache.get(&workspace.tree_scope()).is_none());
    }

    #[test]
    fn workspace_removal_keeps_the_open_bucket_when_removing_another_account() {
        let (backend, _receiver) = Backend::open(true).unwrap();
        let mut workspace = Workspace::new(backend);
        workspace.accounts = serde_json::from_value(json!([
            {"id": "active", "name": "Open workspace", "pathStyle": false},
            {"id": "other", "name": "Other workspace", "pathStyle": true}
        ]))
        .unwrap();
        workspace.active_id = "active".into();
        workspace.bucket = "current-bucket".into();
        workspace.prefix = "current-folder/".into();

        workspace.open_dialog(Dialog::RemoveAccount("other".into()));
        workspace.submit();
        workspace.receive_response(workspace.next_request, Err("Failed to remove".into()));
        assert!(workspace.account_by_id("other").is_some());
        assert_eq!(
            workspace.dialog,
            Some(Dialog::RemoveAccount("other".into()))
        );

        workspace.submit();
        workspace.receive_response(workspace.next_request, Ok(Value::Null));
        assert!(workspace.account_by_id("other").is_none());
        assert_eq!(workspace.active_id, "active");
        assert_eq!(workspace.bucket, "current-bucket");
        assert_eq!(workspace.prefix, "current-folder/");
        assert!(workspace.dialog.is_none());
    }

    #[test]
    fn automatic_location_ignores_a_response_after_search_text_changes() {
        let (backend, _receiver) = Backend::open(true).unwrap();
        let mut workspace = Workspace::new(backend);
        workspace.bucket = "studio-assets".into();
        workspace.buckets.push(Bucket {
            name: workspace.bucket.clone(),
            created: String::new(),
            connection: S3Connection::default(),
        });
        workspace.set_search("Brand assets/");
        let (bucket, args) = workspace.automatic_path().unwrap();
        workspace.start_auto_location(workspace.search_revision, bucket, args);
        let request = workspace.next_request;
        assert!(matches!(
            workspace.pending.get(&request),
            Some(Pending::AutoLocatePath { .. })
        ));
        assert!(!workspace.is_busy());

        workspace.set_search("guidelines");
        workspace.receive_response(
            request,
            Ok(json!({"kind": "directory", "prefix": "Brand assets/"})),
        );
        assert!(workspace.prefix.is_empty());
        assert_eq!(workspace.search_query(), "guidelines");
    }
}

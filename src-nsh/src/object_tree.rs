use crate::model::natural_cmp;
use s3_studio_core::s3::S3Object;
use std::{
    cmp::Ordering,
    collections::{HashMap, HashSet},
    ops::AddAssign,
};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Totals {
    pub bytes: u128,
    pub objects: u64,
}

impl AddAssign for Totals {
    fn add_assign(&mut self, other: Self) {
        self.bytes += other.bytes;
        self.objects += other.objects;
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum TreeSort {
    Name,
    Size,
    Objects,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TreeKey {
    pub key: String,
    pub folder: bool,
}

impl TreeKey {
    pub fn name(&self) -> &str {
        let key = if self.folder {
            self.key.strip_suffix('/').unwrap_or(&self.key)
        } else {
            &self.key
        };
        key.rsplit('/')
            .next()
            .filter(|name| !name.is_empty())
            .unwrap_or("/")
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct TreeEntry {
    pub key: TreeKey,
    pub totals: Totals,
    pub modified: String,
    pub storage_class: String,
}

impl TreeEntry {
    pub fn object(&self) -> S3Object {
        S3Object {
            key: self.key.key.clone(),
            folder: self.key.folder,
            size: if self.key.folder {
                0
            } else {
                self.totals.bytes as i64
            },
            modified: self.modified.clone(),
            storage_class: self.storage_class.clone(),
        }
    }

    pub fn compare(&self, other: &Self, sort: TreeSort, ascending: bool) -> Ordering {
        let order = match sort {
            TreeSort::Name => natural_cmp(self.key.name(), other.key.name()),
            TreeSort::Size => self.totals.bytes.cmp(&other.totals.bytes),
            TreeSort::Objects => self.totals.objects.cmp(&other.totals.objects),
        };
        let order = if ascending { order } else { order.reverse() };
        order.then_with(|| self.key.cmp(&other.key))
    }
}

#[derive(Clone, PartialEq, Eq)]
pub enum TreePage {
    Directory { parent: String, page: usize },
    SearchNext(TreeKey),
    SearchPrevious,
}

#[derive(Clone, PartialEq, Eq)]
pub enum TreeRow {
    Entry {
        item: TreeEntry,
        parent_bytes: u128,
        depth: usize,
        expandable: bool,
        expanded: bool,
    },
    Pager {
        parent: String,
        depth: usize,
        previous: Option<TreePage>,
        next: Option<TreePage>,
    },
}

impl TreeRow {
    pub fn entry(&self) -> Option<&TreeEntry> {
        match self {
            Self::Entry { item, .. } => Some(item),
            Self::Pager { .. } => None,
        }
    }
}

#[derive(Clone)]
pub struct TreeOptions {
    pub sort: TreeSort,
    pub ascending: bool,
    pub expanded: HashSet<String>,
    pub pages: HashMap<String, usize>,
    pub search: String,
    pub search_collapsed: HashSet<String>,
    pub search_pages: Vec<TreeKey>,
}

impl Default for TreeOptions {
    fn default() -> Self {
        Self {
            sort: TreeSort::Size,
            ascending: false,
            expanded: HashSet::new(),
            pages: HashMap::new(),
            search: String::new(),
            search_collapsed: HashSet::new(),
            search_pages: Vec::new(),
        }
    }
}

impl TreeOptions {
    pub fn change_page(&mut self, page: TreePage) {
        match page {
            TreePage::Directory { parent, page } => {
                self.pages.insert(parent, page);
            }
            TreePage::SearchNext(after) => self.search_pages.push(after),
            TreePage::SearchPrevious => {
                self.search_pages.pop();
            }
        }
    }
}

pub struct TreeListing {
    pub rows: Vec<TreeRow>,
    pub totals: Totals,
    pub scanned_at: i64,
    pub dirty: bool,
}

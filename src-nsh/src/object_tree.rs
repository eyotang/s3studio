use crate::model::natural_cmp;
use s3_studio_core::s3::S3Object;
use std::{
    collections::{HashMap, HashSet},
    ops::AddAssign,
    sync::Arc,
};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Totals {
    pub bytes: u128,
    pub objects: usize,
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

struct File {
    size: i64,
    modified: String,
    storage_class: String,
}

pub struct Node {
    pub key: Arc<str>,
    pub totals: Totals,
    parent: usize,
    name_start: usize,
    children: Vec<usize>,
    file: Option<File>,
}

impl Node {
    pub fn folder(&self) -> bool {
        self.file.is_none()
    }

    pub fn name(&self) -> &str {
        let name = &self.key[self.name_start..];
        let name = if self.folder() {
            name.strip_suffix('/').unwrap_or(name)
        } else {
            name
        };
        if name.is_empty() { "/" } else { name }
    }

    pub fn object(&self) -> S3Object {
        S3Object {
            key: self.key.to_string(),
            size: self.file.as_ref().map_or(0, |file| file.size),
            modified: self
                .file
                .as_ref()
                .map_or_else(String::new, |file| file.modified.clone()),
            storage_class: self
                .file
                .as_ref()
                .map_or_else(String::new, |file| file.storage_class.clone()),
            folder: self.folder(),
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub struct TreeRow {
    pub id: usize,
    pub depth: usize,
    pub expandable: bool,
    pub expanded: bool,
}

pub struct ObjectTree {
    nodes: Vec<Node>,
}

impl ObjectTree {
    pub fn totals(&self) -> Totals {
        self.nodes[0].totals
    }

    pub fn node(&self, id: usize) -> &Node {
        &self.nodes[id]
    }

    pub fn parent_fraction(&self, id: usize) -> f64 {
        let node = self.node(id);
        let parent_size = self.node(node.parent).totals.bytes;
        if parent_size == 0 {
            0.0
        } else {
            node.totals.bytes as f64 / parent_size as f64
        }
    }

    pub fn sort(&mut self, sort: TreeSort, ascending: bool) {
        for id in 0..self.nodes.len() {
            if self.nodes[id].children.len() < 2 {
                continue;
            }
            let mut children = std::mem::take(&mut self.nodes[id].children);
            children.sort_unstable_by(|&left, &right| {
                let a = self.node(left);
                let b = self.node(right);
                let order = match sort {
                    TreeSort::Name => natural_cmp(a.name(), b.name()),
                    TreeSort::Size => a.totals.bytes.cmp(&b.totals.bytes),
                    TreeSort::Objects => a.totals.objects.cmp(&b.totals.objects),
                };
                let order = if ascending { order } else { order.reverse() };
                order.then_with(|| a.key.cmp(&b.key))
            });
            self.nodes[id].children = children;
        }
    }

    pub fn rows(
        &self,
        query: &str,
        expanded: &HashSet<usize>,
        search_collapsed: &HashSet<usize>,
    ) -> Vec<TreeRow> {
        let query = query.to_lowercase();
        let searching = !query.is_empty();
        let matches = searching.then(|| {
            let prefix_len = self.nodes[0].key.len();
            let mut matches = vec![false; self.nodes.len()];
            for id in (1..self.nodes.len()).rev() {
                let node = self.node(id);
                if matches[id] || node.key[prefix_len..].to_lowercase().contains(&query) {
                    matches[id] = true;
                    matches[node.parent] = true;
                }
            }
            matches
        });
        let included = |id: usize| matches.as_ref().is_none_or(|matches| matches[id]);
        let mut rows = Vec::new();
        let mut levels = vec![(0, self.nodes[0].children.iter())];
        while let Some((depth, siblings)) = levels.last_mut() {
            let Some(&id) = siblings.next() else {
                levels.pop();
                continue;
            };
            if !included(id) {
                continue;
            }
            let depth = *depth;
            let node = self.node(id);
            let expandable = node.children.iter().copied().any(included);
            let expanded = expandable
                && if searching {
                    !search_collapsed.contains(&id)
                } else {
                    expanded.contains(&id)
                };
            rows.push(TreeRow {
                id,
                depth,
                expandable,
                expanded,
            });
            if expanded {
                levels.push((depth + 1, node.children.iter()));
            }
        }
        rows
    }
}

pub struct TreeBuilder {
    tree: ObjectTree,
    folders: HashMap<Arc<str>, usize>,
    files: HashSet<Arc<str>>,
    scanned: Totals,
}

impl TreeBuilder {
    pub fn new(prefix: &str) -> Self {
        Self {
            tree: ObjectTree {
                nodes: vec![Node {
                    key: Arc::from(prefix),
                    totals: Totals::default(),
                    parent: 0,
                    name_start: 0,
                    children: vec![],
                    file: None,
                }],
            },
            folders: HashMap::new(),
            files: HashSet::new(),
            scanned: Totals::default(),
        }
    }

    pub fn extend(&mut self, objects: Vec<S3Object>) {
        for object in objects {
            let mut parent = 0;
            let prefix_len = self.tree.nodes[0].key.len();
            let mut start = prefix_len;
            for (offset, _) in object.key[prefix_len..].match_indices('/') {
                let end = prefix_len + offset + 1;
                let key = &object.key[..end];
                parent = if let Some(&id) = self.folders.get(key) {
                    id
                } else {
                    let key = Arc::from(key);
                    let id = self.tree.nodes.len();
                    self.folders.insert(Arc::clone(&key), id);
                    self.push_node(parent, start, key, Totals::default(), None);
                    id
                };
                start = end;
            }
            if object.folder || self.files.contains(object.key.as_str()) {
                continue;
            }
            let key = Arc::from(object.key);
            self.files.insert(Arc::clone(&key));
            let totals = Totals {
                bytes: object.size.max(0) as u128,
                objects: 1,
            };
            self.scanned += totals;
            self.push_node(
                parent,
                start,
                key,
                totals,
                Some(File {
                    size: object.size,
                    modified: object.modified,
                    storage_class: object.storage_class,
                }),
            );
        }
    }

    fn push_node(
        &mut self,
        parent: usize,
        name_start: usize,
        key: Arc<str>,
        totals: Totals,
        file: Option<File>,
    ) {
        let id = self.tree.nodes.len();
        self.tree.nodes[parent].children.push(id);
        self.tree.nodes.push(Node {
            key,
            totals,
            parent,
            name_start,
            children: vec![],
            file,
        });
    }

    pub fn scanned(&self) -> Totals {
        self.scanned
    }

    pub fn finish(mut self, sort: TreeSort, ascending: bool) -> ObjectTree {
        // Parents are inserted before their children, even across page boundaries.
        for id in (1..self.tree.nodes.len()).rev() {
            let node = self.tree.node(id);
            let (parent, totals) = (node.parent, node.totals);
            self.tree.nodes[parent].totals += totals;
        }
        self.tree.sort(sort, ascending);
        self.tree
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn object(key: &str, size: i64) -> S3Object {
        S3Object {
            key: key.into(),
            size,
            modified: String::new(),
            storage_class: String::new(),
            folder: key.ends_with('/') && size == 0,
        }
    }

    #[test]
    fn rolls_up_pages_without_counting_empty_markers_or_duplicates() {
        let mut builder = TreeBuilder::new("design/");
        builder.extend(vec![
            object("design/", 0),
            object("design/a", 40),
            object("design/a/", 0),
            object("design/a/banner.png", 20),
        ]);
        builder.extend(vec![
            object("design/a/banner.png", 20),
            object("design/a//raw", 30),
            object("design/a/notes", 0),
            object("design/a/", 10),
            object("design/empty/", 0),
        ]);
        assert_eq!(
            builder.scanned(),
            Totals {
                bytes: 100,
                objects: 5
            }
        );
        let tree = builder.finish(TreeSort::Size, false);
        assert_eq!(
            tree.totals(),
            Totals {
                bytes: 100,
                objects: 5
            }
        );
        let roots = tree.rows("", &HashSet::new(), &HashSet::new());
        assert_eq!(roots.len(), 3);
        let folder = tree.node(roots[0].id);
        assert!(folder.folder());
        assert_eq!(&*folder.key, "design/a/");
        assert_eq!(
            folder.totals,
            Totals {
                bytes: 60,
                objects: 4
            }
        );
        assert_eq!(tree.parent_fraction(roots[0].id), 0.6);
        assert_eq!(tree.node(roots[2].id).totals, Totals::default());

        let rows = tree.rows("RAW", &HashSet::new(), &HashSet::new());
        assert_eq!(rows.iter().map(|r| r.depth).collect::<Vec<_>>(), [0, 1, 2]);
        let empty_segment = tree.node(rows[1].id);
        assert_eq!(empty_segment.name(), "/");
        assert_eq!(&*empty_segment.key, "design/a//");
        assert_eq!(tree.parent_fraction(rows[1].id), 0.5);
        assert_eq!(&*tree.node(rows[2].id).key, "design/a//raw");
        let rows = tree.rows("", &HashSet::from([roots[0].id]), &HashSet::new());
        assert!(rows.iter().any(|r| {
            let node = tree.node(r.id);
            !node.folder() && &*node.key == "design/a/" && node.object().size == 10
        }));
    }

    #[test]
    fn totals_exceed_u64_without_overflow() {
        let mut builder = TreeBuilder::new("");
        builder.extend(vec![
            object("a", i64::MAX),
            object("b", i64::MAX),
            object("c", i64::MAX),
        ]);
        let tree = builder.finish(TreeSort::Size, false);
        assert_eq!(tree.totals().bytes, 3 * i64::MAX as u128);
        assert_eq!(tree.totals().objects, 3);
    }
}

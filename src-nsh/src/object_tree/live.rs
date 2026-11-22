use super::{LevelStatus, Totals, TreeEntry, TreeKey, TreeOptions, TreePage, TreeRow, TreeSort};
use s3_studio_core::s3::{ObjectPage, S3Object, TREE_PAGE_SIZE};
use std::collections::{HashMap, HashSet};

#[derive(Default)]
struct Level {
    entries: Option<Vec<TreeEntry>>,
    confirmed: bool,
    page: usize,
    // Each token begins the next page after its position.
    tokens: Vec<String>,
    requested: Option<usize>,
    error: Option<(usize, String)>,
    refresh_queued: bool,
}

#[derive(Default)]
pub struct LiveTree {
    pub revision: u64,
    levels: HashMap<String, Level>,
}

impl LiveTree {
    pub fn clear(&mut self) {
        self.levels.clear();
        self.revision += 1;
    }

    pub fn seed(&mut self, prefix: &str, objects: &[S3Object]) {
        self.levels
            .entry(prefix.to_owned())
            .or_insert_with(|| Level {
                entries: Some(
                    objects
                        .iter()
                        .take(TREE_PAGE_SIZE)
                        .cloned()
                        .map(entry)
                        .collect(),
                ),
                ..Default::default()
            });
    }

    pub fn needs_page(&self, prefix: &str) -> bool {
        self.levels.get(prefix).is_none_or(|level| {
            !level.confirmed && level.requested.is_none() && level.error.is_none()
        })
    }

    pub fn page_number(&self, prefix: &str) -> Option<usize> {
        self.levels.get(prefix).map(|level| level.page)
    }

    pub fn request(
        &mut self,
        prefix: &str,
        page: usize,
        refreshing: bool,
    ) -> Option<Option<String>> {
        let level = self.levels.entry(prefix.to_owned()).or_default();
        if level.requested.is_some() {
            level.refresh_queued |= refreshing;
            return None;
        }
        let token = if page == 0 {
            None
        } else {
            Some(level.tokens.get(page - 1)?.clone())
        };
        level.requested = Some(page);
        level.error = None;
        Some(token)
    }

    pub fn receive(&mut self, prefix: &str, result: Result<ObjectPage, String>) -> bool {
        let Some(level) = self.levels.get_mut(prefix) else {
            return false;
        };
        let Some(page) = level.requested.take() else {
            return false;
        };
        match result {
            Ok(result) => {
                level.confirmed = true;
                level.page = page;
                level.tokens.truncate(page);
                if let Some(next) = result
                    .next_token
                    .filter(|next| !level.tokens.contains(next))
                {
                    level.tokens.push(next);
                }
                level.entries = Some(result.objects.into_iter().map(entry).collect());
            }
            Err(error) => level.error = Some((page, error)),
        }
        std::mem::take(&mut level.refresh_queued)
    }

    pub fn rows(&self, prefix: &str, options: &TreeOptions) -> Vec<TreeRow> {
        let query = options.search.to_lowercase();
        // During the scan, search covers the directory pages already listed.
        // Include their ancestors so a known match can be shown in context.
        let matches = (!query.is_empty()).then(|| self.matching_keys(prefix, &query));
        enum Visit {
            Children(String, usize),
            Row(TreeRow),
        }
        let mut stack = vec![Visit::Children(prefix.into(), 0)];
        let mut rows = Vec::new();
        while let Some(visit) = stack.pop() {
            let (parent, depth) = match visit {
                Visit::Children(parent, depth) => (parent, depth),
                Visit::Row(row) => {
                    rows.push(row);
                    continue;
                }
            };
            let Some(level) = self.levels.get(&parent) else {
                continue;
            };
            let status = if let Some((page, error)) = &level.error {
                Some(LevelStatus::Failed {
                    page: *page,
                    error: error.clone(),
                })
            } else if level.requested.is_some() {
                Some(LevelStatus::Loading)
            } else if depth > 0 && level.entries.as_ref().is_some_and(Vec::is_empty) {
                Some(LevelStatus::Empty)
            } else {
                None
            };
            if let Some(status) = status {
                stack.push(Visit::Row(TreeRow::Level {
                    parent: parent.clone(),
                    depth,
                    status,
                }));
            }
            if level.requested.is_none()
                && level.error.is_none()
                && (level.page > 0 || level.tokens.len() > level.page)
            {
                stack.push(Visit::Row(TreeRow::Pager {
                    parent: parent.clone(),
                    depth,
                    previous: (level.page > 0).then(|| TreePage::Directory {
                        parent: parent.clone(),
                        page: level.page - 1,
                    }),
                    next: (level.tokens.len() > level.page).then(|| TreePage::Directory {
                        parent: parent.clone(),
                        page: level.page + 1,
                    }),
                }));
            }
            let mut entries = level
                .entries
                .iter()
                .flatten()
                .filter(|item| {
                    matches
                        .as_ref()
                        .is_none_or(|matches| matches.contains(&item.key))
                })
                .collect::<Vec<_>>();
            entries.sort_unstable_by(|a, b| {
                if options.sort != TreeSort::Name && a.key.folder != b.key.folder {
                    return b.key.folder.cmp(&a.key.folder);
                }
                let (sort, ascending) = if options.sort != TreeSort::Name && a.key.folder {
                    (TreeSort::Name, true)
                } else {
                    (options.sort, options.ascending)
                };
                a.compare(b, sort, ascending)
            });
            for item in entries.into_iter().rev() {
                let key = &item.key.key;
                let expanded = item.key.folder
                    && if matches.is_some() {
                        self.levels.contains_key(key) && !options.search_collapsed.contains(key)
                    } else {
                        options.expanded.contains(key)
                    };
                if expanded {
                    stack.push(Visit::Children(key.clone(), depth + 1));
                }
                stack.push(Visit::Row(TreeRow::Entry {
                    item: item.clone(),
                    parent_bytes: None,
                    depth,
                    expandable: item.key.folder,
                    expanded,
                }));
            }
        }
        rows
    }

    fn matching_keys(&self, prefix: &str, query: &str) -> HashSet<TreeKey> {
        let mut matches = HashSet::new();
        for item in self
            .levels
            .values()
            .filter_map(|level| level.entries.as_ref())
            .flatten()
        {
            let key = &item.key.key;
            let Some(suffix) = key.strip_prefix(prefix) else {
                continue;
            };
            if suffix.to_lowercase().contains(query) {
                matches.insert(item.key.clone());
                for (slash, _) in suffix.match_indices('/') {
                    matches.insert(TreeKey {
                        key: key[..prefix.len() + slash + 1].into(),
                        folder: true,
                    });
                }
            }
        }
        matches
    }
}

fn entry(object: S3Object) -> TreeEntry {
    TreeEntry {
        key: TreeKey {
            key: object.key,
            folder: object.folder,
        },
        totals: Totals {
            bytes: if object.folder {
                0
            } else {
                object.size.max(0) as u128
            },
            objects: u64::from(!object.folder),
        },
        modified: object.modified,
        storage_class: object.storage_class,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn live_pages_keep_the_cursor_and_coalesce_a_pending_refresh() {
        let mut tree = LiveTree::default();
        assert_eq!(tree.request("", 0, false), Some(None));
        assert!(!tree.receive(
            "",
            Ok(ObjectPage {
                objects: vec![],
                next_token: Some("p2".into()),
            })
        ));
        assert_eq!(tree.request("", 1, false), Some(Some("p2".into())));
        assert_eq!(tree.request("", 0, true), None);
        assert!(tree.receive(
            "",
            Ok(ObjectPage {
                objects: vec![],
                next_token: Some("p2".into()),
            })
        ));
        let rows = tree.rows("", &TreeOptions::default());
        assert!(matches!(
            rows.last(),
            Some(TreeRow::Pager {
                previous: Some(TreePage::Directory { page: 0, .. }),
                next: None,
                ..
            })
        ));
        assert_eq!(tree.request("", 0, true), Some(None));
    }
}

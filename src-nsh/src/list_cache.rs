use crate::tree_cache::TreeScope;
use s3_studio_core::s3::ObjectPage;
use std::{
    collections::VecDeque,
    time::{Duration, Instant},
};

const MAX_DIRECTORIES: usize = 32;
const MAX_OBJECTS: usize = 20_000;
const LIFETIME: Duration = Duration::from_secs(300);

#[derive(Default)]
pub struct ListCache(VecDeque<(TreeScope, Instant, ObjectPage)>);

impl ListCache {
    pub fn clear(&mut self) {
        self.0.clear();
    }

    pub fn invalidate_bucket(&mut self, account: &str, bucket: &str) {
        self.0
            .retain(|(scope, _, _)| scope.account != account || scope.bucket != bucket);
    }

    pub fn insert(&mut self, scope: TreeScope, page: ObjectPage) {
        self.0.retain(|(old, _, _)| old != &scope);
        if page.objects.len() > MAX_OBJECTS {
            return;
        }
        self.0.push_back((scope, Instant::now(), page));
        while self.0.len() > MAX_DIRECTORIES
            || self
                .0
                .iter()
                .map(|(_, _, page)| page.objects.len())
                .sum::<usize>()
                > MAX_OBJECTS
        {
            self.0.pop_front();
        }
    }

    pub fn get(&mut self, scope: &TreeScope) -> Option<ObjectPage> {
        self.0.retain(|(_, saved, _)| saved.elapsed() < LIFETIME);
        let index = self.0.iter().position(|(old, _, _)| old == scope)?;
        let entry = self.0.remove(index)?;
        let page = entry.2.clone();
        self.0.push_back(entry);
        Some(page)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scope(account: &str, prefix: &str) -> TreeScope {
        TreeScope::new(account.into(), "bucket".into(), prefix.into())
    }

    #[test]
    fn cached_pages_are_scoped_expire_and_keep_the_next_cursor() {
        let mut cache = ListCache::default();
        let root = scope("a", "");
        cache.insert(
            root.clone(),
            ObjectPage {
                objects: vec![],
                next_token: Some("page-3".into()),
            },
        );
        assert!(cache.get(&scope("b", "")).is_none());
        assert!(cache.get(&scope("a", "child/")).is_none());
        assert_eq!(
            cache.get(&root).unwrap().next_token.as_deref(),
            Some("page-3")
        );
        cache.0[0].1 = Instant::now() - LIFETIME;
        assert!(cache.get(&root).is_none());
    }

    #[test]
    fn least_recent_directories_are_evicted_and_mutations_invalidate_the_bucket() {
        let mut cache = ListCache::default();
        for i in 0..MAX_DIRECTORIES {
            cache.insert(
                scope("a", &i.to_string()),
                ObjectPage {
                    objects: vec![],
                    next_token: None,
                },
            );
        }
        assert!(cache.get(&scope("a", "0")).is_some());
        cache.insert(
            scope("b", ""),
            ObjectPage {
                objects: vec![],
                next_token: None,
            },
        );
        assert!(cache.get(&scope("a", "1")).is_none());
        cache.invalidate_bucket("a", "bucket");
        assert!(cache.get(&scope("a", "0")).is_none());
        assert!(cache.get(&scope("b", "")).is_some());
    }
}

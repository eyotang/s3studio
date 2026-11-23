use super::*;
use crate::object_tree::{TreeEntry, TreePage, TreeRow};

fn canceled() -> Arc<AtomicBool> {
    Arc::new(AtomicBool::new(false))
}

fn scope(prefix: &str) -> TreeScope {
    TreeScope::new("account".into(), "bucket".into(), prefix.into())
}

fn object(key: &str, size: i64) -> S3Object {
    S3Object {
        key: key.into(),
        size,
        folder: size == 0 && key.ends_with('/'),
        modified: "2026-11-22T08:30:00Z".into(),
        storage_class: "STANDARD".into(),
    }
}

fn build(cache: &Arc<TreeCache>, scope: TreeScope, objects: &[S3Object]) {
    let mut scan = cache.begin(scope, canceled()).unwrap();
    for page in objects.chunks(2) {
        scan.append(page).unwrap();
    }
    scan.publish().unwrap();
}

fn rows(cache: &TreeCache, prefix: &str, options: &TreeOptions) -> TreeListing {
    cache
        .rows(&scope(prefix), options, canceled())
        .unwrap()
        .unwrap()
}

fn entry<'a>(listing: &'a TreeListing, key: &str, folder: bool) -> &'a TreeEntry {
    listing
        .rows
        .iter()
        .filter_map(TreeRow::entry)
        .find(|entry| entry.key.key == key && entry.key.folder == folder)
        .unwrap()
}

#[test]
fn persists_and_reuses_parent_totals_for_subdirectories() {
    let directory = tempfile::tempdir().unwrap();
    let cache = Arc::new(TreeCache::new(directory.path().to_owned()));
    build(
        &cache,
        scope("design/"),
        &[
            object("design/", 0),
            object("design/a", 40),
            object("design/a/", 10),
            object("design/a/banner.png", 20),
            object("design/a//raw", 30),
            object("design/a/banner.png", 20),
            object("design/a/notes", 0),
            object("design/empty/", 0),
        ],
    );
    drop(cache);

    let cache = TreeCache::new(directory.path().to_owned());
    let options = TreeOptions::default();
    let parent = rows(&cache, "design/", &options);
    assert_eq!(
        parent.totals,
        Totals {
            bytes: 100,
            objects: 5
        }
    );
    assert_eq!(
        entry(&parent, "design/a/", true).totals,
        Totals {
            bytes: 60,
            objects: 4
        }
    );
    assert_eq!(
        entry(&parent, "design/empty/", true).totals,
        Totals::default()
    );

    let nested = rows(&cache, "design/a/", &options);
    assert_eq!(nested.scanned_at, parent.scanned_at);
    assert_eq!(
        nested.totals,
        Totals {
            bytes: 60,
            objects: 4
        }
    );
    assert_eq!(entry(&nested, "design/a/", false).key.name(), "/");
    assert_eq!(entry(&nested, "design/a//", true).totals.bytes, 30);

    let other_account = TreeScope::new("other".into(), "bucket".into(), "design/".into());
    assert!(!cache.available(&other_account).unwrap());
    let mut other_connection = scope("design/");
    other_connection.namespace = "changed connection".into();
    assert!(!cache.available(&other_connection).unwrap());
}

#[test]
fn expired_snapshot_is_retained_if_a_refresh_is_canceled() {
    let cache = Arc::new(TreeCache::preview().unwrap());
    let scope = scope("");
    build(&cache, scope.clone(), &[object("old.bin", 10)]);
    let old_time = chrono::Utc::now().timestamp_millis() - CACHE_LIFETIME_MS - 1;
    db::open(&snapshot(&cache.bucket_path(&scope), ""), false)
        .unwrap()
        .execute("UPDATE scan SET scanned_at=?1", [old_time])
        .unwrap();
    assert!(cache.available(&scope).unwrap());
    assert!(expired(old_time));

    let cancel = canceled();
    let mut refresh = cache.begin(scope, cancel.clone()).unwrap();
    refresh.append(&[object("new.bin", 50)]).unwrap();
    cancel.store(true, Ordering::Relaxed);
    assert!(refresh.publish().is_err());

    let old = rows(&cache, "", &TreeOptions::default());
    assert_eq!(old.scanned_at, old_time);
    assert_eq!(old.totals.bytes, 10);
    entry(&old, "old.bin", false);
}

#[test]
fn applies_known_changes_to_cache_and_to_a_concurrent_scan() {
    let cache = Arc::new(TreeCache::preview().unwrap());
    let scope = scope("");
    let old = [
        object("photos/cover", 100),
        object("photos/old/img", 200),
        object("backup/", 0),
    ];
    build(&cache, scope.clone(), &old);
    let saved = rows(&cache, "", &TreeOptions::default()).scanned_at;

    let mut scan = cache.begin(scope.clone(), canceled()).unwrap();
    scan.append(&old).unwrap();
    cache
        .update(&scope, TreeChange::Upsert(object("photos/cover", 30)))
        .unwrap();
    cache
        .update(&scope, TreeChange::Upsert(object("backup/new", 50)))
        .unwrap();
    cache
        .update(&scope, TreeChange::Upsert(object("photos/empty/", 0)))
        .unwrap();
    cache
        .update(
            &scope,
            TreeChange::Delete {
                keys: vec![],
                folders: vec!["photos/old/".into()],
            },
        )
        .unwrap();

    let current = rows(&cache, "", &TreeOptions::default());
    assert_eq!(current.scanned_at, saved);
    assert!(!current.dirty);
    assert_eq!(
        current.totals,
        Totals {
            bytes: 80,
            objects: 2
        }
    );

    scan.publish().unwrap();
    let options = TreeOptions {
        expanded: ["photos/".into()].into(),
        ..Default::default()
    };
    let updated = rows(&cache, "", &options);
    assert_eq!(updated.totals, current.totals);
    assert_eq!(entry(&updated, "photos/", true).totals.bytes, 30);
    assert_eq!(entry(&updated, "photos/cover", false).totals.bytes, 30);
    entry(&updated, "photos/empty/", true);
    assert!(
        !updated
            .rows
            .iter()
            .filter_map(TreeRow::entry)
            .any(|entry| entry.key.key == "photos/old/")
    );

    cache
        .update(
            &scope,
            TreeChange::Unknown {
                keys: vec!["photos/cover".into()],
                folders: vec![],
            },
        )
        .unwrap();
    let dirty = rows(&cache, "", &options);
    assert!(dirty.dirty);
    assert_eq!(dirty.totals.bytes, 80);
}

#[test]
fn reads_one_directory_page_and_paginates_search_with_ancestors() {
    let cache = Arc::new(TreeCache::preview().unwrap());
    let mut scan = cache.begin(scope(""), canceled()).unwrap();
    let batch = (0..450)
        .map(|index| object(&format!("photos/2026/Ä-{index:03}.JPG"), index + 1))
        .collect::<Vec<_>>();
    scan.append(&batch).unwrap();
    scan.publish().unwrap();

    let mut options = TreeOptions {
        expanded: ["photos/".into(), "photos/2026/".into()].into(),
        ..Default::default()
    };
    let first = rows(&cache, "", &options);
    assert_eq!(first.rows.len(), 203);
    entry(&first, "photos/2026/Ä-449.JPG", false);
    let Some(TreeRow::Pager {
        next: Some(next), ..
    }) = first.rows.last()
    else {
        panic!("missing next directory page");
    };
    options.change_page(next.clone());
    let second = rows(&cache, "", &options);
    entry(&second, "photos/2026/Ä-249.JPG", false);

    options.search = "ä-".into();
    let hits = rows(&cache, "", &options);
    entry(&hits, "photos/", true);
    entry(&hits, "photos/2026/", true);
    entry(&hits, "photos/2026/Ä-000.JPG", false);
    let Some(TreeRow::Pager {
        next: Some(TreePage::SearchNext(after)),
        ..
    }) = hits.rows.last()
    else {
        panic!("missing search cursor");
    };
    options.change_page(TreePage::SearchNext(after.clone()));
    let next = rows(&cache, "", &options);
    entry(&next, "photos/2026/Ä-200.JPG", false);
    assert!(
        !next
            .rows
            .iter()
            .filter_map(TreeRow::entry)
            .any(|entry| entry.key.key == "photos/2026/Ä-000.JPG")
    );

    options.search = "photos".into();
    options.search_pages.clear();
    let nested = rows(&cache, "photos/2026/", &options);
    assert!(nested.rows.is_empty());
    assert_eq!(nested.totals.objects, 450);
}

#[test]
fn outdated_scans_cannot_overwrite_a_change_from_another_cache_instance() {
    let dir = tempfile::tempdir().unwrap();
    let first = Arc::new(TreeCache::new(dir.path().to_owned()));
    let second = TreeCache::new(dir.path().to_owned());
    let scope = scope("");
    let old = [object("a", 15)];
    build(&first, scope.clone(), &old);

    let mut scan = first.begin(scope.clone(), canceled()).unwrap();
    scan.append(&old).unwrap();
    second
        .update(&scope, TreeChange::Upsert(object("a", 25)))
        .unwrap();
    assert!(scan.publish().is_err());
    assert_eq!(rows(&first, "", &TreeOptions::default()).totals.bytes, 25);

    let scan = first.begin(scope.clone(), canceled()).unwrap();
    second
        .invalidate(Some(&scope.account), Some(&scope.bucket))
        .unwrap();
    assert!(scan.publish().is_err());
    assert!(!first.available(&scope).unwrap());
}

#[test]
fn directory_totals_and_sorting_cross_the_unsigned_64_bit_limit() {
    let cache = Arc::new(TreeCache::preview().unwrap());
    build(
        &cache,
        scope(""),
        &[
            object("a/1", i64::MAX),
            object("a/2", i64::MAX),
            object("a/3", i64::MAX),
            object("b/1", i64::MAX),
        ],
    );
    let result = rows(&cache, "", &TreeOptions::default());
    assert_eq!(result.totals.bytes, 4 * i64::MAX as u128);
    assert_eq!(result.rows[0].entry().unwrap().key.key, "a/");
    assert_eq!(
        result.rows[0].entry().unwrap().totals.bytes,
        3 * i64::MAX as u128
    );
}

#[test]
fn list_sizes_include_nested_content_and_keep_unknown_distinct_from_empty() {
    let cache = Arc::new(TreeCache::preview().unwrap());
    build(
        &cache,
        scope(""),
        &[
            object("folder/a", 12),
            object("folder/nested/b", 30),
            object("empty/", 0),
        ],
    );
    let sizes = cache
        .folder_sizes(
            &scope(""),
            &["folder/".into(), "empty/".into(), "missing/".into()],
            canceled(),
        )
        .unwrap();
    assert_eq!(sizes.get("folder/"), Some(&42));
    assert_eq!(sizes.get("empty/"), Some(&0));
    assert!(!sizes.contains_key("missing/"));
}

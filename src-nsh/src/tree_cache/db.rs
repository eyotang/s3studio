use super::TreeChange;
use crate::object_tree::{
    Totals, TreeEntry, TreeKey, TreeListing, TreeOptions, TreePage, TreeRow, TreeSort,
};
use anyhow::{Context, Result, ensure};
use rusqlite::{Connection, OpenFlags, OptionalExtension, Row, functions::FunctionFlags, params};
use s3_studio_core::s3::S3Object;
use std::{
    collections::{HashMap, hash_map::Entry},
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

const PAGE_SIZE: usize = 200;
const ROOT_ID: i64 = 1;
const ENTRY_FIELDS: &str =
    "n.id, n.parent, n.key, n.folder, n.bytes, n.objects, n.modified, n.storage_class";
const SCHEMA: &str = "
    CREATE TABLE scan (
        prefix TEXT NOT NULL,
        scanned_at INTEGER NOT NULL,
        epoch TEXT NOT NULL,
        dirty INTEGER NOT NULL DEFAULT 0
    );
    CREATE TABLE entry (
        id INTEGER PRIMARY KEY,
        parent INTEGER NOT NULL,
        key TEXT NOT NULL,
        folder INTEGER NOT NULL,
        bytes BLOB NOT NULL,
        objects INTEGER NOT NULL,
        marker INTEGER NOT NULL DEFAULT 0,
        modified TEXT NOT NULL DEFAULT '',
        storage_class TEXT NOT NULL DEFAULT '',
        UNIQUE(key, folder)
    );
";

pub(super) struct Metadata {
    pub prefix: String,
    pub scanned_at: i64,
    pub epoch: String,
    pub dirty: bool,
}

#[derive(Clone)]
struct Record {
    id: i64,
    parent: i64,
    item: TreeEntry,
}

pub(super) fn open(path: &Path, read_only: bool) -> Result<Connection> {
    let conn = Connection::open_with_flags(
        path,
        if read_only {
            OpenFlags::SQLITE_OPEN_READ_ONLY
        } else {
            OpenFlags::SQLITE_OPEN_READ_WRITE
        },
    )?;
    conn.busy_timeout(Duration::from_secs(5))?;
    // Bound SQLite's page cache and sort large result sets on disk.
    conn.execute_batch("PRAGMA cache_size=-16384; PRAGMA temp_store=FILE;")?;
    Ok(conn)
}

pub(super) fn interruptible(conn: &Connection, cancel: Arc<AtomicBool>) {
    conn.progress_handler(8_000, Some(move || cancel.load(Ordering::Relaxed)));
}

pub(super) fn create(path: &Path, prefix: &str, start: i64, epoch: &str) -> Result<Connection> {
    let conn = open(path, false)?;
    // The staging file is disposable until a complete scan has been committed
    // and published. Batch inserts need no rollback journal on that file.
    conn.execute_batch("PRAGMA journal_mode=OFF; PRAGMA synchronous=OFF;")?;
    conn.execute_batch(SCHEMA)?;
    conn.execute(
        "INSERT INTO scan(prefix, scanned_at, epoch) VALUES(?1, ?2, ?3)",
        params![prefix, start, epoch],
    )?;
    conn.execute(
        "INSERT INTO entry(id, parent, key, folder, bytes, objects) VALUES(1, 0, ?1, 1, ?2, 0)",
        params![prefix, 0u128.to_be_bytes().as_slice()],
    )?;
    Ok(conn)
}

pub(super) fn finish(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "
        CREATE INDEX children_by_size ON entry(parent, bytes DESC, key);
        PRAGMA journal_mode=DELETE;
        PRAGMA synchronous=FULL;
    ",
    )?;
    Ok(())
}

pub(super) fn metadata(conn: &Connection) -> Result<Metadata> {
    Ok(conn.query_row(
        "SELECT prefix, scanned_at, epoch, dirty FROM scan",
        [],
        |row| {
            Ok(Metadata {
                prefix: row.get(0)?,
                scanned_at: row.get(1)?,
                epoch: row.get(2)?,
                dirty: row.get(3)?,
            })
        },
    )?)
}

pub(super) fn set_epoch(conn: &Connection, epoch: &str, dirty: bool) -> Result<()> {
    conn.execute("UPDATE scan SET epoch=?1, dirty=?2", params![epoch, dirty])?;
    Ok(())
}

fn totals(row: &Row<'_>, bytes: usize, count: usize) -> rusqlite::Result<Totals> {
    Ok(Totals {
        bytes: u128::from_be_bytes(row.get::<_, [u8; 16]>(bytes)?),
        objects: row.get(count)?,
    })
}

fn record(row: &Row<'_>) -> rusqlite::Result<Record> {
    Ok(Record {
        id: row.get(0)?,
        parent: row.get(1)?,
        item: TreeEntry {
            key: TreeKey {
                key: row.get(2)?,
                folder: row.get(3)?,
            },
            totals: totals(row, 4, 5)?,
            modified: row.get(6)?,
            storage_class: row.get(7)?,
        },
    })
}

fn find(conn: &Connection, key: &str, folder: bool) -> Result<Option<Record>> {
    let sql = format!("SELECT {ENTRY_FIELDS} FROM entry n WHERE key=?1 AND folder=?2");
    Ok(conn
        .prepare_cached(&sql)?
        .query_row(params![key, folder], record)
        .optional()?)
}

pub(super) fn covers(conn: &Connection, prefix: &str) -> Result<bool> {
    Ok(conn
        .prepare_cached("SELECT 1 FROM entry WHERE key=?1 AND folder=1")?
        .exists([prefix])?)
}

fn ancestors(
    conn: &Connection,
    root: &str,
    key: &str,
    known: &mut HashMap<String, i64>,
) -> Result<Vec<i64>> {
    let suffix = key
        .strip_prefix(root)
        .context("object key outside cache scope")?;
    let mut ids = vec![ROOT_ID];
    let mut parent = ROOT_ID;
    for (offset, _) in suffix.match_indices('/') {
        let prefix = &key[..root.len() + offset + 1];
        if let Some(&id) = known.get(prefix) {
            parent = id;
        } else {
            conn.prepare_cached(
                "INSERT OR IGNORE INTO entry(parent, key, folder, bytes, objects) VALUES(?1, ?2, 1, ?3, 0)"
            )?.execute(params![parent, prefix, 0u128.to_be_bytes().as_slice()])?;
            parent = conn
                .prepare_cached("SELECT id FROM entry WHERE key=?1 AND folder=1")?
                .query_row([prefix], |row| row.get(0))?;
            if known.len() >= 8192 {
                known.clear();
            }
            known.insert(prefix.to_owned(), parent);
        }
        ids.push(parent);
    }
    Ok(ids)
}

fn add_object(conn: &Connection, parent: i64, object: &S3Object) -> Result<Totals> {
    if object.folder {
        conn.prepare_cached("UPDATE entry SET marker=1 WHERE id=?1 AND marker=0")?
            .execute([parent])?;
        return Ok(Totals::default());
    }
    let bytes = u128::try_from(object.size).context("invalid object size")?;
    let inserted = conn.prepare_cached(
        "INSERT OR IGNORE INTO entry(parent, key, folder, bytes, objects, modified, storage_class)
         VALUES(?1, ?2, 0, ?3, 1, ?4, ?5)"
    )?.execute(params![parent, object.key, bytes.to_be_bytes().as_slice(), object.modified, object.storage_class])?;
    Ok(if inserted == 1 {
        Totals { bytes, objects: 1 }
    } else {
        Totals::default()
    })
}

fn update_totals(conn: &Connection, id: i64, delta: Totals, subtract: bool) -> Result<i64> {
    let (mut value, parent) = conn
        .prepare_cached("SELECT bytes, objects, parent FROM entry WHERE id=?1")?
        .query_row([id], |row| Ok((totals(row, 0, 1)?, row.get(2)?)))?;
    if subtract {
        value.bytes = value
            .bytes
            .checked_sub(delta.bytes)
            .context("invalid cached directory size")?;
        value.objects = value
            .objects
            .checked_sub(delta.objects)
            .context("invalid cached object count")?;
    } else {
        value += delta;
    }
    conn.prepare_cached("UPDATE entry SET bytes=?1, objects=?2 WHERE id=?3")?
        .execute(params![
            value.bytes.to_be_bytes().as_slice(),
            value.objects,
            id
        ])?;
    Ok(parent)
}

pub(super) fn append(
    conn: &mut Connection,
    scope: &str,
    objects: &[S3Object],
    known: &mut HashMap<String, i64>,
) -> Result<Totals> {
    let tx = conn.transaction()?;
    let mut deltas = HashMap::<i64, Totals>::new();
    for object in objects {
        let parents = ancestors(&tx, scope, &object.key, known)?;
        let added = add_object(&tx, *parents.last().unwrap(), object)?;
        if added.objects > 0 {
            for id in parents {
                *deltas.entry(id).or_default() += added;
            }
        }
    }
    for (&id, &delta) in &deltas {
        update_totals(&tx, id, delta, false)?;
    }
    tx.commit()?;
    Ok(deltas.get(&ROOT_ID).copied().unwrap_or_default())
}

fn subtract_and_prune(conn: &Connection, mut id: i64, removed: Totals) -> Result<()> {
    while id != 0 {
        let parent = update_totals(conn, id, removed, true)?;
        if id != ROOT_ID {
            conn.prepare_cached(
                "DELETE FROM entry WHERE id=?1 AND marker=0 AND NOT EXISTS(SELECT 1 FROM entry WHERE parent=?1)"
            )?.execute([id])?;
        }
        id = parent;
    }
    Ok(())
}

fn delete_file(conn: &Connection, key: &str) -> Result<()> {
    if let Some(entry) = find(conn, key, false)? {
        conn.prepare_cached("DELETE FROM entry WHERE id=?1")?
            .execute([entry.id])?;
        subtract_and_prune(conn, entry.parent, entry.item.totals)?;
    }
    Ok(())
}

fn upper_bound(prefix: &str) -> String {
    // A folder prefix ends in ASCII '/'. Its next lexicographic boundary is '0'.
    format!("{}0", prefix.strip_suffix('/').expect("directory prefix"))
}

fn delete_folder(conn: &Connection, scope: &str, prefix: &str) -> Result<()> {
    if scope.starts_with(prefix) {
        conn.execute("DELETE FROM entry WHERE id<>1", [])?;
        conn.execute(
            "UPDATE entry SET marker=0, bytes=?1, objects=0 WHERE id=1",
            [0u128.to_be_bytes().as_slice()],
        )?;
    } else if prefix.starts_with(scope)
        && let Some(entry) = find(conn, prefix, true)?
    {
        conn.prepare_cached("DELETE FROM entry WHERE key>=?1 AND key<?2")?
            .execute(params![prefix, upper_bound(prefix)])?;
        subtract_and_prune(conn, entry.parent, entry.item.totals)?;
    }
    Ok(())
}

pub(super) fn apply(conn: &Connection, prefix: &str, change: &TreeChange) -> Result<()> {
    match change {
        TreeChange::Upsert(object) if object.key.starts_with(prefix) => {
            delete_file(conn, &object.key)?;
            let parents = ancestors(conn, prefix, &object.key, &mut HashMap::new())?;
            let parent = *parents.last().unwrap();
            if !object.folder && object.key.ends_with('/') {
                conn.prepare_cached("UPDATE entry SET marker=0 WHERE id=?1")?
                    .execute([parent])?;
            }
            let added = add_object(conn, parent, object)?;
            if added.objects > 0 {
                for id in parents {
                    update_totals(conn, id, added, false)?;
                }
            }
        }
        TreeChange::Delete { keys, folders } => {
            for folder in folders {
                ensure!(
                    !folder.is_empty() && folder.ends_with('/'),
                    "invalid folder deletion"
                );
                delete_folder(conn, prefix, folder)?;
            }
            for key in keys.iter().filter(|key| key.starts_with(prefix)) {
                delete_file(conn, key)?;
            }
        }
        _ => {}
    }
    Ok(())
}

pub(super) fn listing(
    conn: &Connection,
    prefix: &str,
    meta: Metadata,
    stale_epoch: bool,
    options: &TreeOptions,
) -> Result<Option<TreeListing>> {
    let Some(root) = find(conn, prefix, true)? else {
        return Ok(None);
    };
    let totals = root.item.totals;
    let rows = if options.search.is_empty() {
        browse(conn, root, options)?
    } else {
        search(conn, root, options)?
    };
    Ok(Some(TreeListing {
        rows,
        totals,
        scanned_at: meta.scanned_at,
        dirty: meta.dirty || stale_epoch,
    }))
}

fn directory_page(
    conn: &Connection,
    parent: i64,
    page: usize,
    options: &TreeOptions,
) -> Result<Vec<(Record, bool)>> {
    let column = match options.sort {
        TreeSort::Name => "key COLLATE natural_name",
        TreeSort::Size => "bytes",
        TreeSort::Objects => "objects",
    };
    let direction = if options.ascending { "ASC" } else { "DESC" };
    let sql = format!(
        "SELECT {ENTRY_FIELDS}, n.folder AND EXISTS(SELECT 1 FROM entry WHERE parent=n.id)
         FROM entry n WHERE n.parent=?1 ORDER BY n.{column} {direction}, n.key
         LIMIT ?2 OFFSET ?3"
    );
    let mut statement = conn.prepare_cached(&sql)?;
    let offset = page
        .checked_mul(PAGE_SIZE)
        .context("invalid directory page")?;
    let rows = statement.query_map(params![parent, PAGE_SIZE + 1, offset], |row| {
        Ok((record(row)?, row.get(8)?))
    })?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

fn browse(conn: &Connection, root: Record, options: &TreeOptions) -> Result<Vec<TreeRow>> {
    if options.sort == TreeSort::Name {
        conn.create_collation("natural_name", crate::model::natural_cmp)?;
    }
    enum Visit {
        Directory(Record, usize),
        Row(TreeRow),
    }
    let mut rows = Vec::new();
    let mut pending = vec![Visit::Directory(root, 0)];
    while let Some(visit) = pending.pop() {
        let (branch, depth) = match visit {
            Visit::Directory(branch, depth) => (branch, depth),
            Visit::Row(row) => {
                rows.push(row);
                continue;
            }
        };
        let key = &branch.item.key.key;
        let mut page = options.pages.get(key).copied().unwrap_or_default();
        let mut children = directory_page(conn, branch.id, page, options)?;
        if children.is_empty() && page > 0 {
            page = 0;
            children = directory_page(conn, branch.id, page, options)?;
        }
        if page > 0 || children.len() > PAGE_SIZE {
            pending.push(Visit::Row(TreeRow::Pager {
                parent: key.clone(),
                depth,
                previous: (page > 0).then(|| TreePage::Directory {
                    parent: key.clone(),
                    page: page - 1,
                }),
                next: (children.len() > PAGE_SIZE).then(|| TreePage::Directory {
                    parent: key.clone(),
                    page: page + 1,
                }),
            }));
        }
        children.truncate(PAGE_SIZE);
        for (child, expandable) in children.into_iter().rev() {
            let expanded = expandable && options.expanded.contains(&child.item.key.key);
            if expanded {
                pending.push(Visit::Directory(child.clone(), depth + 1));
            }
            pending.push(Visit::Row(TreeRow::Entry {
                item: child.item,
                parent_bytes: branch.item.totals.bytes,
                depth,
                expandable,
                expanded,
            }));
        }
    }
    Ok(rows)
}

fn search(conn: &Connection, root: Record, options: &TreeOptions) -> Result<Vec<TreeRow>> {
    let prefix = &root.item.key.key;
    let skip_bytes = prefix.len();
    let query = options.search.to_lowercase();
    conn.create_scalar_function(
        "key_matches",
        1,
        FunctionFlags::SQLITE_DETERMINISTIC,
        move |ctx| {
            let key = ctx.get_raw(0).as_str()?;
            Ok(key
                .get(skip_bytes..)
                .is_some_and(|key| key.to_lowercase().contains(&query)))
        },
    )?;
    let (after_key, after_folder) = options
        .search_pages
        .last()
        .map_or((prefix.as_str(), -1_i64), |after| {
            (after.key.as_str(), i64::from(after.folder))
        });
    let upper = (!prefix.is_empty()).then(|| upper_bound(prefix));
    let boundary = if upper.is_some() { "AND n.key<?5" } else { "" };
    let sql = format!(
        "SELECT {ENTRY_FIELDS} FROM entry n
         WHERE (n.key, n.folder)>(?1, ?2) AND n.id<>?3 {boundary} AND key_matches(n.key)
         ORDER BY n.key, n.folder LIMIT ?4"
    );
    let mut statement = conn.prepare_cached(&sql)?;
    let mut page = if let Some(upper) = upper {
        statement
            .query_map(
                params![after_key, after_folder, root.id, PAGE_SIZE + 1, upper],
                record,
            )?
            .collect::<rusqlite::Result<Vec<_>>>()?
    } else {
        statement
            .query_map(
                params![after_key, after_folder, root.id, PAGE_SIZE + 1],
                record,
            )?
            .collect::<rusqlite::Result<Vec<_>>>()?
    };
    let has_next = page.len() > PAGE_SIZE;
    page.truncate(PAGE_SIZE);
    let next = has_next.then(|| TreePage::SearchNext(page.last().unwrap().item.key.clone()));
    let mut included = HashMap::<i64, Record>::new();
    let by_id = format!("SELECT {ENTRY_FIELDS} FROM entry n WHERE id=?1");
    let mut lookup = conn.prepare_cached(&by_id)?;
    for mut hit in page {
        loop {
            let Entry::Vacant(slot) = included.entry(hit.id) else {
                break;
            };
            let parent = hit.parent;
            slot.insert(hit);
            if parent == root.id || parent == 0 {
                break;
            }
            hit = lookup.query_row([parent], record)?;
        }
    }
    let mut children = HashMap::<i64, Vec<Record>>::new();
    for node in included.into_values() {
        children.entry(node.parent).or_default().push(node);
    }
    let sorted = |mut siblings: Vec<Record>| {
        siblings.sort_unstable_by(|a, b| b.item.compare(&a.item, options.sort, options.ascending));
        siblings
    };
    let mut pending = vec![(
        0,
        root.item.totals.bytes,
        sorted(children.remove(&root.id).unwrap_or_default()),
    )];
    let mut rows = Vec::new();
    while let Some((depth, parent_bytes, siblings)) = pending.last_mut() {
        let Some(record) = siblings.pop() else {
            pending.pop();
            continue;
        };
        let depth = *depth;
        let parent_bytes = *parent_bytes;
        let expandable = children.contains_key(&record.id);
        let expanded = expandable && !options.search_collapsed.contains(&record.item.key.key);
        let bytes = record.item.totals.bytes;
        rows.push(TreeRow::Entry {
            item: record.item,
            parent_bytes,
            depth,
            expandable,
            expanded,
        });
        if expanded {
            pending.push((
                depth + 1,
                bytes,
                sorted(children.remove(&record.id).unwrap()),
            ));
        }
    }
    if !options.search_pages.is_empty() || has_next {
        rows.push(TreeRow::Pager {
            parent: prefix.clone(),
            depth: 0,
            previous: (!options.search_pages.is_empty()).then_some(TreePage::SearchPrevious),
            next,
        });
    }
    Ok(rows)
}

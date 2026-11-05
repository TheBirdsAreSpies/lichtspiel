//! Where the library is kept: an SQLite file (data/lichtspiel.db).
//!
//! The whole library is held in memory as a [`Db`] (all lookups, filters and
//! smart playlists work on that). A save writes only what changed since the
//! last one: the items and watch states that were touched, as single rows,
//! plus the small parts (settings, users, playlists, …) whose content differs
//! from what was written before. A progress report from a player is one row,
//! not the whole library.
//!
//! Items and watch states sit in [`Tracked`] maps, which remember every key
//! that was handed out for changing. They give no `&mut` access to the map
//! itself, so a change that bypasses the bookkeeping does not compile.
//!
//! Each row keeps the record as JSON (`data`); a few columns are copied out
//! next to it so the file can be looked at with any SQLite tool.

use crate::db::{Db, Item, ViewState};
use rusqlite::{Connection, params};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::collections::btree_map::{Entry, ValuesMut};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::ops::Deref;
use std::path::{Path, PathBuf};

/// The library file's name inside the data folder.
pub const FILE_NAME: &str = "lichtspiel.db";
/// The file older versions kept everything in; read once and then renamed.
pub const OLD_JSON: &str = "library.json";

const SCHEMA_VERSION: i64 = 1;

// ---------------------------------------------------------------- tracked map

/// A map that remembers which keys were changed (or removed) since the last
/// [`Tracked::take_changes`]. Reading works like a `BTreeMap` (through `Deref`).
#[derive(Clone, Debug)]
pub struct Tracked<K: Ord + Clone, V> {
    map: BTreeMap<K, V>,
    touched: BTreeSet<K>,
    all: bool,
}

/// What changed in a [`Tracked`] map.
pub enum Changed<K> {
    /// Everything: the stored rows are replaced by the map's content.
    All,
    /// These keys were changed, added or removed.
    Keys(Vec<K>),
}

impl<K: Ord + Clone, V> Default for Tracked<K, V> {
    fn default() -> Self {
        Tracked { map: BTreeMap::new(), touched: BTreeSet::new(), all: false }
    }
}

impl<K: Ord + Clone, V> Deref for Tracked<K, V> {
    type Target = BTreeMap<K, V>;
    fn deref(&self) -> &BTreeMap<K, V> {
        &self.map
    }
}

impl<K: Ord + Clone, V> From<BTreeMap<K, V>> for Tracked<K, V> {
    fn from(map: BTreeMap<K, V>) -> Self {
        Tracked { map, touched: BTreeSet::new(), all: false }
    }
}

impl<K: Ord + Clone, V> Tracked<K, V> {
    pub fn get_mut(&mut self, k: &K) -> Option<&mut V> {
        let v = self.map.get_mut(k)?;
        self.touched.insert(k.clone());
        Some(v)
    }

    pub fn insert(&mut self, k: K, v: V) -> Option<V> {
        self.touched.insert(k.clone());
        self.map.insert(k, v)
    }

    pub fn remove(&mut self, k: &K) -> Option<V> {
        let v = self.map.remove(k)?;
        self.touched.insert(k.clone());
        Some(v)
    }

    pub fn entry(&mut self, k: K) -> Entry<'_, K, V> {
        self.touched.insert(k.clone());
        self.map.entry(k)
    }

    /// Keeps the entries `keep` says yes to. The kept ones are not changed.
    pub fn retain(&mut self, mut keep: impl FnMut(&K, &V) -> bool) {
        let touched = &mut self.touched;
        self.map.retain(|k, v| {
            let yes = keep(k, v);
            if !yes {
                touched.insert(k.clone());
            }
            yes
        });
    }

    /// Changing every value: the next save writes them all.
    pub fn values_mut(&mut self) -> ValuesMut<'_, K, V> {
        self.all = true;
        self.map.values_mut()
    }

    pub fn mark_all(&mut self) {
        self.all = true;
    }

    pub fn take_changes(&mut self) -> Changed<K> {
        let keys = std::mem::take(&mut self.touched);
        if std::mem::take(&mut self.all) {
            Changed::All
        } else {
            Changed::Keys(keys.into_iter().collect())
        }
    }
}

impl<K: Ord + Clone + Serialize, V: Serialize> Serialize for Tracked<K, V> {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        self.map.serialize(s)
    }
}

impl<'de, K: Ord + Clone + Deserialize<'de>, V: Deserialize<'de>> Deserialize<'de> for Tracked<K, V> {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        BTreeMap::deserialize(d).map(Tracked::from)
    }
}

// ---------------------------------------------------------------- changes

/// One item as it is written.
pub struct ItemRow {
    id: u64,
    section_id: u64,
    parent_id: Option<u64>,
    kind: &'static str,
    title: String,
    data: String,
}

impl ItemRow {
    fn of(it: &Item) -> ItemRow {
        ItemRow {
            id: it.id,
            section_id: it.section_id,
            parent_id: it.parent_id,
            kind: it.kind.as_str(),
            title: it.title.clone(),
            data: serde_json::to_string(it).unwrap_or_default(),
        }
    }
}

/// Watch-state keys are "user_id:item_id".
fn view_key(k: &str) -> Option<(u64, u64)> {
    let (u, i) = k.split_once(':')?;
    Some((u.parse().ok()?, i.parse().ok()?))
}

/// Everything a save writes, taken out of the [`Db`] while it is locked, so
/// the writing itself can happen after the lock is let go.
pub struct Changes {
    parts: Vec<(&'static str, String)>,
    items_all: bool,
    items: Vec<(u64, Option<ItemRow>)>,
    views_all: bool,
    views: Vec<(u64, u64, Option<String>)>,
}

impl Changes {
    pub fn take(db: &mut Db) -> Changes {
        fn js<T: Serialize>(v: &T) -> String {
            serde_json::to_string(v).unwrap_or_default()
        }
        let parts = vec![
            ("version", js(&db.version)),
            ("machine_id", js(&db.machine_id)),
            ("next_id", js(&db.next_id)),
            ("settings", js(&db.settings)),
            ("sections", js(&db.sections)),
            ("users", js(&db.users)),
            ("invites", js(&db.invites)),
            ("tokens", js(&db.tokens)),
            ("playlists", js(&db.playlists)),
            ("plugins", js(&db.plugins)),
            ("history", js(&db.history)),
            ("sessions", js(&db.sessions)),
            ("reports", js(&db.reports)),
            ("downloads", js(&db.downloads)),
        ];
        let (items_all, items) = match db.items.take_changes() {
            Changed::All => (true, db.items.values().map(|it| (it.id, Some(ItemRow::of(it)))).collect()),
            Changed::Keys(keys) => (false, keys.into_iter().map(|id| (id, db.items.get(&id).map(ItemRow::of))).collect()),
        };
        let (views_all, keys): (bool, Vec<String>) = match db.views.take_changes() {
            Changed::All => (true, db.views.keys().cloned().collect()),
            Changed::Keys(keys) => (false, keys),
        };
        let views = keys
            .into_iter()
            .filter_map(|k| {
                let (u, i) = view_key(&k)?;
                Some((u, i, db.views.get(&k).map(js)))
            })
            .collect();
        Changes { parts, items_all, items, views_all, views }
    }
}

// ---------------------------------------------------------------- the file

pub struct Store {
    conn: Connection,
    path: PathBuf,
    /// The small parts as they were last written (unchanged ones are skipped).
    written: HashMap<&'static str, String>,
}

impl Store {
    /// How much memory the library file may use for its pages (in MB; SQLite's own default is about 2).
    pub fn set_cache_mb(&self, mb: u32) {
        let mb = mb.clamp(2, 4096);
        let _ = self.conn.pragma_update(None, "cache_size", -(mb as i64) * 1024);
    }

    pub fn open(path: &Path) -> Result<Store, String> {
        let conn = Connection::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
        let fail = |e: rusqlite::Error| format!("{}: {e}", path.display());
        // WAL: readers never wait, and a crash can't leave a half-written file.
        conn.pragma_update(None, "journal_mode", "WAL").map_err(fail)?;
        conn.pragma_update(None, "synchronous", "NORMAL").map_err(fail)?;
        conn.busy_timeout(std::time::Duration::from_secs(10)).map_err(fail)?;
        let version: i64 = conn.pragma_query_value(None, "user_version", |r| r.get(0)).map_err(fail)?;
        if version > SCHEMA_VERSION {
            return Err(format!(
                "{} was written by a newer version of this server (layout {version}, this one knows {SCHEMA_VERSION})",
                path.display()
            ));
        }
        conn.execute_batch(
            "create table if not exists meta (key text primary key, value text not null);
             create table if not exists items (
                 id integer primary key,
                 section_id integer not null,
                 parent_id integer,
                 kind text not null,
                 title text not null,
                 data text not null);
             create index if not exists items_section on items(section_id);
             create index if not exists items_parent on items(parent_id);
             create table if not exists views (
                 user_id integer not null,
                 item_id integer not null,
                 data text not null,
                 primary key (user_id, item_id)) without rowid;",
        )
        .map_err(fail)?;
        conn.pragma_update(None, "user_version", SCHEMA_VERSION).map_err(fail)?;
        Ok(Store { conn, path: path.to_path_buf(), written: HashMap::new() })
    }

    /// The stored library, or None when the file is new (nothing saved yet).
    /// A record that can't be read is left in the file untouched and skipped.
    pub fn load(&mut self) -> Result<Option<Db>, String> {
        let fail = |e: rusqlite::Error| format!("{}: {e}", self.path.display());
        let mut parts: HashMap<String, String> = HashMap::new();
        {
            let mut st = self.conn.prepare("select key, value from meta").map_err(fail)?;
            let rows = st.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))).map_err(fail)?;
            for row in rows {
                let (k, v) = row.map_err(fail)?;
                parts.insert(k, v);
            }
        }
        if !parts.contains_key("machine_id") {
            return Ok(None);
        }
        fn part<T: serde::de::DeserializeOwned + Default>(parts: &HashMap<String, String>, key: &str) -> Result<T, String> {
            match parts.get(key) {
                None => Ok(T::default()),
                Some(s) => serde_json::from_str(s).map_err(|e| format!("the stored {key} can't be read: {e}")),
            }
        }
        let mut items = BTreeMap::new();
        {
            let mut st = self.conn.prepare("select id, data from items").map_err(fail)?;
            let rows = st.query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?))).map_err(fail)?;
            for row in rows {
                let (id, data) = row.map_err(fail)?;
                match serde_json::from_str::<Item>(&data) {
                    Ok(it) => {
                        items.insert(id as u64, it);
                    }
                    Err(e) => elog!("library item {id} can't be read ({e}); it is skipped and kept in the file as it is"),
                }
            }
        }
        let mut views = BTreeMap::new();
        {
            let mut st = self.conn.prepare("select user_id, item_id, data from views").map_err(fail)?;
            let rows = st
                .query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?, r.get::<_, String>(2)?)))
                .map_err(fail)?;
            for row in rows {
                let (u, i, data) = row.map_err(fail)?;
                match serde_json::from_str::<ViewState>(&data) {
                    Ok(v) => {
                        views.insert(format!("{u}:{i}"), v);
                    }
                    Err(e) => elog!("watch state {u}:{i} can't be read ({e}); it is skipped"),
                }
            }
        }
        let db = Db {
            version: part(&parts, "version")?,
            machine_id: part(&parts, "machine_id")?,
            next_id: part(&parts, "next_id")?,
            settings: part(&parts, "settings")?,
            sections: part(&parts, "sections")?,
            items: items.into(),
            users: part(&parts, "users")?,
            invites: part(&parts, "invites")?,
            tokens: part(&parts, "tokens")?,
            playlists: part(&parts, "playlists")?,
            views: views.into(),
            plugins: part(&parts, "plugins")?,
            history: part(&parts, "history")?,
            sessions: part(&parts, "sessions")?,
            reports: part(&parts, "reports")?,
            downloads: part(&parts, "downloads")?,
        };
        // An id handed out must never come back, even if a stored counter is behind.
        let mut db = db;
        let top = db.items.keys().next_back().copied().unwrap_or(0);
        db.next_id = db.next_id.max(top + 1);
        self.written = parts
            .into_iter()
            .filter_map(|(k, v)| PART_NAMES.iter().find(|p| **p == k).map(|p| (*p, v)))
            .collect();
        Ok(Some(db))
    }

    /// Writes the changes in one transaction.
    /// Compacts the file and refreshes the query statistics.
    pub fn optimize(&mut self) -> Result<(), String> {
        let e = |e: rusqlite::Error| e.to_string();
        self.conn.execute_batch("pragma wal_checkpoint(truncate); vacuum; pragma optimize;").map_err(e)
    }

    /// A consistent copy of the library in one file.
    pub fn copy_to(&mut self, dest: &Path) -> Result<(), String> {
        let _ = std::fs::remove_file(dest);
        self.conn.execute("vacuum into ?1", [dest.to_string_lossy().as_ref()]).map(|_| ()).map_err(|e| e.to_string())
    }

    pub fn write(&mut self, ch: &Changes) -> rusqlite::Result<()> {
        let tx = self.conn.transaction()?;
        {
            let mut put = tx.prepare_cached("insert or replace into meta (key, value) values (?1, ?2)")?;
            for (k, v) in &ch.parts {
                if self.written.get(k) != Some(v) {
                    put.execute(params![k, v])?;
                }
            }
        }
        if ch.items_all {
            tx.execute("delete from items", [])?;
        }
        {
            let mut put = tx.prepare_cached(
                "insert or replace into items (id, section_id, parent_id, kind, title, data) values (?1, ?2, ?3, ?4, ?5, ?6)",
            )?;
            let mut del = tx.prepare_cached("delete from items where id = ?1")?;
            for (id, row) in &ch.items {
                match row {
                    Some(r) => put.execute(params![r.id as i64, r.section_id as i64, r.parent_id.map(|p| p as i64), r.kind, r.title, r.data])?,
                    None => del.execute(params![*id as i64])?,
                };
            }
        }
        if ch.views_all {
            tx.execute("delete from views", [])?;
        }
        {
            let mut put = tx.prepare_cached("insert or replace into views (user_id, item_id, data) values (?1, ?2, ?3)")?;
            let mut del = tx.prepare_cached("delete from views where user_id = ?1 and item_id = ?2")?;
            for (u, i, data) in &ch.views {
                match data {
                    Some(d) => put.execute(params![*u as i64, *i as i64, d])?,
                    None => del.execute(params![*u as i64, *i as i64])?,
                };
            }
        }
        tx.commit()?;
        for (k, v) in &ch.parts {
            self.written.insert(k, v.clone());
        }
        Ok(())
    }

    /// After a failed write: the next one writes everything again.
    pub fn forget_written(&mut self) {
        self.written.clear();
    }
}

const PART_NAMES: [&str; 14] = [
    "version", "machine_id", "next_id", "settings", "sections", "users", "invites", "tokens", "playlists", "plugins", "history", "sessions", "reports", "downloads",
];

/// The library in a data folder without starting a server (for the
/// screenshot): the SQLite file, or an older version's JSON file.
#[cfg(feature = "console")]
pub fn read_library(data_dir: &Path) -> Option<Db> {
    let file = data_dir.join(FILE_NAME);
    if file.exists() {
        return Store::open(&file).ok()?.load().ok()?.map(|mut db| {
            db.after_load();
            db
        });
    }
    let json = data_dir.join(OLD_JSON);
    json.exists().then(|| Db::load_json(&json))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn item(id: u64, title: &str) -> Item {
        serde_json::from_value(json!({"id": id, "section_id": 1, "kind": "movie", "title": title,
            "sort_title": title.to_lowercase(), "added_at": 0, "updated_at": 0})).unwrap()
    }

    #[test]
    fn saves_only_what_changed_and_reads_it_back() {
        let path = std::env::temp_dir().join(format!("lichtspiel-store-test-{}.db", std::process::id()));
        for ext in ["", "-wal", "-shm"] {
            let _ = std::fs::remove_file(format!("{}{ext}", path.display()));
        }
        let mut db = Db::default();
        for id in 1..=3 {
            db.items.insert(id, item(id, &format!("Film {id}")));
        }
        db.view_mut(7, 2).view_offset_ms = 1000;
        let mut st = Store::open(&path).unwrap();
        assert!(st.load().unwrap().is_none(), "a new file holds no library");
        st.write(&Changes::take(&mut db)).unwrap();

        // A progress report: one watch state, no items, only unchanged small parts.
        db.view_mut(7, 2).view_offset_ms = 5000;
        let ch = Changes::take(&mut db);
        assert!(ch.items.is_empty() && !ch.items_all);
        assert_eq!(ch.views.len(), 1);
        assert!(ch.parts.iter().all(|(k, v)| st.written.get(k) == Some(v)));
        st.write(&ch).unwrap();

        // One item changed, one removed.
        db.items.get_mut(&1).unwrap().title = "Renamed".into();
        db.items.remove(&3);
        db.settings.server_name = "Test".into();
        let ch = Changes::take(&mut db);
        assert_eq!(ch.items.len(), 2);
        st.write(&ch).unwrap();
        drop(st);

        let back = Store::open(&path).unwrap().load().unwrap().unwrap();
        assert_eq!(back.items.keys().copied().collect::<Vec<_>>(), vec![1, 2]);
        assert_eq!(back.items[&1].title, "Renamed");
        assert_eq!(back.view(7, 2).view_offset_ms, 5000);
        assert_eq!(back.settings.server_name, "Test");
        assert_eq!(back.machine_id, db.machine_id);
        assert!(back.next_id >= db.next_id);
        for ext in ["", "-wal", "-shm"] {
            let _ = std::fs::remove_file(format!("{}{ext}", path.display()));
        }
    }
}

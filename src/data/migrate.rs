//! Import from the old server (the one Lichtspiel replaces): its
//! libraries and folders, every account's watched state, resume points and
//! ratings, manual playlists, credits markers, and titles/plots.
//!
//! It reads the old server's library database (an SQLite file). A copy is
//! made first, so the old server's own file is never changed and it may keep
//! running. Files are matched by their path; folder paths can be translated
//! (e.g. `D:\Filme` on the old PC → `/media/Filme` in Docker).

use crate::data::db::{self, now, Playlist, PlaylistKind, Section, SectionKind};
use crate::server::{apply_metadata, App};
use rusqlite::types::Value as Sql;
use rusqlite::Connection;
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// The old server's library database file name.
pub const DB_NAME: &str = "com.plexapp.plugins.library.db";

/// Where the old server keeps its data on the usual systems (and where the
/// Docker setup mounts it).
pub fn candidates() -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = vec![];
    if let Some(p) = crate::env("OLD_SERVER") {
        v.push(p.into());
    }
    if let Ok(l) = std::env::var("LOCALAPPDATA") {
        v.push(Path::new(&l).join("Plex Media Server"));
    }
    for p in [
        "/oldserver",
        "/volume1/PlexMediaServer/AppData/Plex Media Server",
        "/volume1/Plex/Library/Application Support/Plex Media Server",
        "/var/packages/PlexMediaServer/shares/PlexMediaServer/AppData/Plex Media Server",
        "/var/lib/plexmediaserver/Library/Application Support/Plex Media Server",
    ] {
        v.push(p.into());
    }
    v
}

/// The database for a path that is the file itself or a folder above it.
pub fn find_db(p: &Path) -> Option<PathBuf> {
    if p.is_file() {
        return Some(p.to_path_buf());
    }
    ["", "Databases", "Plug-in Support/Databases", "Plex Media Server/Plug-in Support/Databases", "Library/Application Support/Plex Media Server/Plug-in Support/Databases"]
        .iter()
        .map(|r| p.join(r).join(DB_NAME))
        .find(|f| f.is_file())
}

pub fn detect() -> Option<PathBuf> {
    candidates().iter().find_map(|c| find_db(c))
}

// ------------------------------------------------------------------ reading

#[derive(Debug, Default, Clone)]
pub struct OldSection {
    pub id: i64,
    pub name: String,
    /// 1 movies, 2 shows (8 music and 13 photos are not imported).
    pub kind: i64,
    pub language: String,
    pub paths: Vec<String>,
}

#[derive(Debug, Default, Clone)]
pub struct OldItem {
    pub id: i64,
    pub parent: Option<i64>,
    /// 1 movie, 2 show, 3 season, 4 episode, 15 playlist.
    pub kind: i64,
    pub guid: String,
    pub title: String,
    pub original_title: String,
    pub summary: String,
    pub tagline: String,
    pub studio: String,
    pub content_rating: String,
    pub rating: Option<f64>,
    pub year: Option<i64>,
    pub aired: String,
    /// When the old server added it (unix seconds).
    pub added: i64,
}

#[derive(Debug, Default, Clone)]
pub struct OldSetting {
    pub account: i64,
    pub guid: String,
    pub rating: Option<f64>,
    pub offset_ms: i64,
    pub views: i64,
    pub last_viewed: i64,
}

#[derive(Debug, Default, Clone)]
pub struct OldPlaylist {
    pub id: i64,
    pub title: String,
    pub summary: String,
    pub account: i64,
    pub smart: bool,
    pub items: Vec<i64>,
}

#[derive(Debug, Default)]
pub struct Old {
    pub sections: Vec<OldSection>,
    pub accounts: Vec<(i64, String)>,
    pub items: HashMap<i64, OldItem>,
    /// (item id, file path)
    pub parts: Vec<(i64, String)>,
    pub settings: Vec<OldSetting>,
    /// (account, item guid, viewed at) for every single viewing the old server logged.
    pub views: Vec<(i64, String, i64)>,
    pub genres: HashMap<i64, Vec<String>>,
    /// Tag type 314: the external ids the new Plex agent keeps, like "imdb://tt0133093" or "tmdb://603".
    pub external: HashMap<i64, Vec<String>>,
    /// Actors in order: (name, role, photo address). Tag type 6 "Role".
    pub cast: HashMap<i64, Vec<(String, String, String)>>,
    /// Tag type 4 "Director" and 5 "Writer".
    pub directors: HashMap<i64, Vec<String>>,
    pub writers: HashMap<i64, Vec<String>>,
    /// Tag types 2 "Collection", 7 "Producer" and 8 "Country".
    pub collections: HashMap<i64, Vec<String>>,
    pub producers: HashMap<i64, Vec<String>>,
    pub countries: HashMap<i64, Vec<String>>,
    /// (item id, kind "credits"/"intro", start ms, end ms)
    pub markers: Vec<(i64, String, i64, i64)>,
    pub playlists: Vec<OldPlaylist>,
    /// Tables that could not be read (other versions of the old server).
    pub warnings: Vec<String>,
}

fn int(v: &Sql) -> Option<i64> {
    match v {
        Sql::Integer(i) => Some(*i),
        Sql::Real(f) => Some(*f as i64),
        Sql::Text(s) => s.trim().parse().ok().or_else(|| date_to_epoch(s)),
        _ => None,
    }
}

fn real(v: &Sql) -> Option<f64> {
    match v {
        Sql::Integer(i) => Some(*i as f64),
        Sql::Real(f) => Some(*f),
        Sql::Text(s) => s.trim().parse().ok(),
        _ => None,
    }
}

fn text(v: &Sql) -> String {
    match v {
        Sql::Text(s) => s.clone(),
        Sql::Integer(i) => i.to_string(),
        Sql::Real(f) => f.to_string(),
        _ => String::new(),
    }
}

/// "2021-03-04 05:06:07" (older databases keep dates as text) → seconds.
fn date_to_epoch(s: &str) -> Option<i64> {
    let s = s.trim();
    let (y, m, d) = (s.get(0..4)?.parse::<i64>().ok()?, s.get(5..7)?.parse::<i64>().ok()?, s.get(8..10)?.parse::<i64>().ok()?);
    let (hh, mm, ss) = (s.get(11..13).and_then(|x| x.parse::<i64>().ok()).unwrap_or(0), s.get(14..16).and_then(|x| x.parse::<i64>().ok()).unwrap_or(0), s.get(17..19).and_then(|x| x.parse::<i64>().ok()).unwrap_or(0));
    Some(crate::library::query::days_from_civil(y, u32::try_from(m).ok()?, u32::try_from(d).ok()?) * 86400 + hh * 3600 + mm * 60 + ss)
}

fn rows(c: &Connection, sql: &str, n: usize, warnings: &mut Vec<String>) -> Vec<Vec<Sql>> {
    let run = || -> rusqlite::Result<Vec<Vec<Sql>>> {
        let mut st = c.prepare(sql)?;
        let mut r = st.query([])?;
        let mut out = vec![];
        while let Some(row) = r.next()? {
            out.push((0..n).map(|i| row.get::<_, Sql>(i).unwrap_or(Sql::Null)).collect());
        }
        Ok(out)
    };
    run().unwrap_or_else(|e| {
        warnings.push(format!("{} ({e})", sql.split_whitespace().take(6).collect::<Vec<_>>().join(" ")));
        vec![]
    })
}

/// Copies the database (with its journal files) into `work` and opens the copy.
fn open_copy(src: &Path, work: &Path) -> Result<Connection, String> {
    std::fs::create_dir_all(work).map_err(|e| e.to_string())?;
    let dst = work.join("library.db");
    // The old database rarely changes after the move: a copy newer than it (and its journal) is used as it is.
    let modified = |p: &Path| std::fs::metadata(p).and_then(|m| m.modified()).ok();
    let source_time = ["", "-wal"].iter().filter_map(|e| modified(Path::new(&format!("{}{e}", src.display())))).max();
    if !matches!((modified(&dst), source_time), (Some(d), Some(s)) if d > s) {
        for ext in ["-wal", "-shm"] {
            let _ = std::fs::remove_file(work.join(format!("library.db{ext}")));
        }
        // Copied under another name first, so a copy cut off half-way is never taken as up to date.
        let part = work.join("library.db.part");
        std::fs::copy(src, &part).and_then(|_| std::fs::rename(&part, &dst)).map_err(|e| format!("could not copy {}: {e}", src.display()))?;
        for ext in ["-wal", "-shm"] {
            let s = PathBuf::from(format!("{}{ext}", src.display()));
            if s.is_file() {
                let _ = std::fs::copy(&s, work.join(format!("library.db{ext}")));
            }
        }
    }
    let c = Connection::open(&dst).map_err(|e| format!("not a database the import can read: {e}"))?;
    // The old server sorts with its own "naturalsort" collation; plain order is enough here.
    let _ = c.create_collation("naturalsort", |a: &str, b: &str| a.cmp(b));
    c.query_row("select count(*) from metadata_items", [], |r| r.get::<_, i64>(0))
        .map_err(|e| format!("this is not the old server's library database ({e})"))?;
    Ok(c)
}

/// Only the old server's credits markers, by file path (`path_key`): so the
/// credits detection can take them instead of searching the picture.
pub fn credits_by_path(src: &Path, work: &Path) -> Result<CreditsIndex, String> {
    let c = open_copy(src, work)?;
    let mut warnings = vec![];
    let sql = "select mp.file, tg.time_offset from taggings tg join tags t on t.id = tg.tag_id \
               join media_items mi on mi.metadata_item_id = tg.metadata_item_id join media_parts mp on mp.media_item_id = mi.id \
               where t.tag_type = 12 and lower(coalesce(nullif(tg.text, ''), t.tag)) = 'credits' and tg.time_offset > 0 and mp.file is not null";
    let mut by_file: HashMap<String, Vec<u64>> = HashMap::new();
    for r in rows(&c, sql, 2, &mut warnings) {
        if let Some(ms) = int(&r[1]).filter(|m| *m > 0) {
            by_file.entry(path_key(&text(&r[0]))).or_default().push(ms as u64);
        }
    }
    Ok(index_by_path(by_file))
}

/// Only the old server's intro markers, by file path: (start, end) in ms as a two-item list.
pub fn intros_by_path(src: &Path, work: &Path) -> Result<CreditsIndex, String> {
    let c = open_copy(src, work)?;
    let mut warnings = vec![];
    let sql = "select mp.file, tg.time_offset, tg.end_time_offset from taggings tg join tags t on t.id = tg.tag_id \n               join media_items mi on mi.metadata_item_id = tg.metadata_item_id join media_parts mp on mp.media_item_id = mi.id \n               where t.tag_type = 12 and lower(coalesce(nullif(tg.text, ''), t.tag)) = 'intro' and tg.end_time_offset > tg.time_offset and mp.file is not null order by tg.id";
    let mut by_file: HashMap<String, Vec<u64>> = HashMap::new();
    for r in rows(&c, sql, 3, &mut warnings) {
        if let (Some(s), Some(e)) = (int(&r[1]).filter(|m| *m >= 0), int(&r[2])) {
            by_file.entry(path_key(&text(&r[0]))).or_insert_with(|| vec![s as u64, e as u64]);
        }
    }
    Ok(index_by_path(by_file))
}

fn index_by_path(by_file: HashMap<String, Vec<u64>>) -> CreditsIndex {
    let mut idx = CreditsIndex::default();
    for (k, v) in by_file {
        // The same last folders + file name are also a match when the old server saw the files under another
        // root (a share path, a mount). A tail that two different files share is left out.
        for (n, tails) in [&mut idx.tail3, &mut idx.tail2].into_iter().enumerate() {
            let t = path_tail(&k, 3 - n);
            match tails.get(&t) {
                Some(Some(old)) if *old != v => {
                    tails.insert(t, None);
                }
                Some(_) => {}
                None => {
                    tails.insert(t, Some(v.clone()));
                }
            }
        }
        idx.exact.insert(k, v);
    }
    idx
}

/// The old server's credits markers, found by full path or, failing that, by the last path parts.
#[derive(Default)]
pub struct CreditsIndex {
    exact: HashMap<String, Vec<u64>>,
    tail3: HashMap<String, Option<Vec<u64>>>,
    tail2: HashMap<String, Option<Vec<u64>>>,
}

impl CreditsIndex {
    pub fn get(&self, path: &str) -> Option<&Vec<u64>> {
        let k = path_key(path);
        self.exact.get(&k).or_else(|| self.tail3.get(&path_tail(&k, 3)).and_then(|v| v.as_ref())).or_else(|| self.tail2.get(&path_tail(&k, 2)).and_then(|v| v.as_ref()))
    }
    pub fn len(&self) -> usize {
        self.exact.len()
    }
}

/// The last `n` parts of a `path_key`.
fn path_tail(k: &str, n: usize) -> String {
    let parts: Vec<&str> = k.split('/').filter(|p| !p.is_empty()).collect();
    parts[parts.len().saturating_sub(n)..].join("/")
}

pub fn read(src: &Path, work: &Path) -> Result<Old, String> {
    let c = open_copy(src, work)?;
    let mut old = Old::default();
    let w = &mut old.warnings;
    let mut secs: Vec<OldSection> = rows(&c, "select id, name, section_type, language from library_sections", 4, w)
        .iter()
        .map(|r| OldSection { id: int(&r[0]).unwrap_or(0), name: text(&r[1]), kind: int(&r[2]).unwrap_or(0), language: text(&r[3]), paths: vec![] })
        .collect();
    for r in rows(&c, "select library_section_id, root_path from section_locations order by id", 2, w) {
        if let Some(s) = secs.iter_mut().find(|s| Some(s.id) == int(&r[0])) {
            s.paths.push(text(&r[1]));
        }
    }
    old.sections = secs;
    old.accounts = rows(&c, "select id, name from accounts order by id", 2, w).iter().map(|r| (int(&r[0]).unwrap_or(0), text(&r[1]))).collect();
    for r in rows(&c, "select id, parent_id, metadata_type, guid, title, original_title, summary, tagline, studio, content_rating, rating, year, originally_available_at, added_at from metadata_items where metadata_type in (1, 2, 3, 4, 15)", 14, w) {
        let it = OldItem {
            id: int(&r[0]).unwrap_or(0),
            parent: int(&r[1]),
            kind: int(&r[2]).unwrap_or(0),
            guid: text(&r[3]),
            title: text(&r[4]),
            original_title: text(&r[5]),
            summary: text(&r[6]),
            tagline: text(&r[7]),
            studio: text(&r[8]),
            content_rating: text(&r[9]),
            rating: real(&r[10]).filter(|x| *x > 0.0),
            year: int(&r[11]).filter(|y| *y > 1800),
            aired: match &r[12] {
                Sql::Integer(t) => db_date(*t),
                other => text(other).chars().take(10).collect(),
            },
            added: int(&r[13]).unwrap_or(0).max(0),
        };
        old.items.insert(it.id, it);
    }
    old.parts = rows(&c, "select mi.metadata_item_id, mp.file from media_parts mp join media_items mi on mi.id = mp.media_item_id where mp.file is not null and mp.file <> ''", 2, w)
        .iter()
        .filter_map(|r| Some((int(&r[0])?, text(&r[1]))))
        .collect();
    old.settings = rows(&c, "select account_id, guid, rating, view_offset, view_count, last_viewed_at from metadata_item_settings", 6, w)
        .iter()
        .map(|r| OldSetting {
            account: int(&r[0]).unwrap_or(0),
            guid: text(&r[1]),
            rating: real(&r[2]).filter(|x| *x > 0.0),
            offset_ms: int(&r[3]).unwrap_or(0).max(0),
            views: int(&r[4]).unwrap_or(0).max(0),
            last_viewed: int(&r[5]).unwrap_or(0).max(0),
        })
        .collect();
    old.views = rows(&c, "select account_id, guid, viewed_at from metadata_item_views where viewed_at > 0 and guid is not null", 3, w)
        .iter()
        .map(|r| (int(&r[0]).unwrap_or(0), text(&r[1]), int(&r[2]).unwrap_or(0)))
        .collect();
    for r in rows(&c, "select tg.metadata_item_id, t.tag from taggings tg join tags t on t.id = tg.tag_id where t.tag_type = 314", 2, w) {
        if let Some(id) = int(&r[0]) {
            old.external.entry(id).or_default().push(text(&r[1]));
        }
    }
    for r in rows(&c, "select tg.metadata_item_id, t.tag from taggings tg join tags t on t.id = tg.tag_id where t.tag_type = 1 order by tg.metadata_item_id, tg.\"index\"", 2, w) {
        if let Some(id) = int(&r[0]) {
            old.genres.entry(id).or_default().push(text(&r[1]));
        }
    }
    // People are shared tags in the old server (one per type and name,
    // gives the types); each item links to them with a role text and an order.
    let people_sql = "select tg.metadata_item_id, t.tag_type, t.tag, coalesce(tg.text, ''), coalesce(t.user_thumb_url, '') from taggings tg join tags t on t.id = tg.tag_id where t.tag_type in (2, 4, 5, 6, 7, 8) and t.tag <> '' order by tg.metadata_item_id, t.tag_type, tg.\"index\"";
    // Older databases have no photo column.
    let has_photos = c.prepare("select user_thumb_url from tags limit 0").is_ok();
    let people = if has_photos { rows(&c, people_sql, 5, w) } else { rows(&c, &people_sql.replace("coalesce(t.user_thumb_url, '')", "''"), 5, w) };
    for r in people {
        let (Some(id), Some(kind)) = (int(&r[0]), int(&r[1])) else { continue };
        let name = text(&r[2]).trim().to_string();
        match kind {
            6 => old.cast.entry(id).or_default().push((name, text(&r[3]).trim().to_string(), text(&r[4]))),
            4 => old.directors.entry(id).or_default().push(name),
            5 => old.writers.entry(id).or_default().push(name),
            2 => old.collections.entry(id).or_default().push(name),
            7 => old.producers.entry(id).or_default().push(name),
            8 => old.countries.entry(id).or_default().push(name),
            _ => {}
        }
    }
    old.markers = rows(&c, "select tg.metadata_item_id, coalesce(nullif(tg.text, ''), t.tag), tg.time_offset, tg.end_time_offset from taggings tg join tags t on t.id = tg.tag_id where t.tag_type = 12", 4, w)
        .iter()
        .filter_map(|r| Some((int(&r[0])?, text(&r[1]).to_lowercase(), int(&r[2])?, int(&r[3]).unwrap_or(0))))
        .collect();
    let owners: HashMap<i64, i64> = rows(&c, "select metadata_item_id, account_id from metadata_item_accounts", 2, w).iter().filter_map(|r| Some((int(&r[0])?, int(&r[1])?))).collect();
    let mut pls: Vec<OldPlaylist> = old
        .items
        .values()
        .filter(|i| i.kind == 15)
        .map(|i| OldPlaylist { id: i.id, title: i.title.clone(), summary: i.summary.clone(), account: owners.get(&i.id).copied().unwrap_or(1), smart: false, items: vec![] })
        .collect();
    pls.sort_by_key(|p| p.id);
    for r in rows(&c, "select playlist_id, metadata_item_id, uri from play_queue_generators order by playlist_id, \"order\"", 3, w) {
        let Some(p) = pls.iter_mut().find(|p| Some(p.id) == int(&r[0])) else { continue };
        match int(&r[1]) {
            Some(i) if i > 0 => p.items.push(i),
            _ if !text(&r[2]).is_empty() => p.smart = true,
            _ => {}
        }
    }
    old.playlists = pls;
    Ok(old)
}

fn db_date(t: i64) -> String {
    if t <= 0 {
        return String::new();
    }
    let (y, m, d) = crate::connect::rss::civil(t.div_euclid(86400));
    format!("{y:04}-{m:02}-{d:02}")
}

// ------------------------------------------------------------------ paths

/// The online entry Plex had picked for a movie or show, as (source, id) like a
/// file name tag: from the external ids of the new agent ("imdb://tt0133093",
/// "tmdb://603", "tvdb://81189"), or from the guid of an old agent
/// ("com.plexapp.agents.imdb://tt0133093?lang=en", "...themoviedb://603", "...thetvdb://81189").
pub fn plex_external_id(guid: &str, tags: &[String]) -> Option<(&'static str, String)> {
    fn digits_or_tt(src: &str, v: &str) -> Option<String> {
        let id: String = v.chars().take_while(|c| c.is_ascii_alphanumeric()).collect();
        let ok = if src == "imdb" { id.len() > 2 && id.starts_with("tt") && id[2..].chars().all(|c| c.is_ascii_digit()) } else { !id.is_empty() && id.chars().all(|c| c.is_ascii_digit()) };
        ok.then_some(id)
    }
    let mut found: Vec<(&'static str, String)> = vec![];
    for t in tags {
        if let Some((s, v)) = t.split_once("://") {
            let src = match s.trim().to_lowercase().as_str() { "imdb" => "imdb", "tmdb" => "tmdb", "tvdb" => "tvdb", _ => continue };
            if let Some(id) = digits_or_tt(src, v.trim()) { found.push((src, id)); }
        }
    }
    for (prefix, src) in [("com.plexapp.agents.imdb://", "imdb"), ("com.plexapp.agents.themoviedb://", "tmdb"), ("com.plexapp.agents.thetvdb://", "tvdb")] {
        if let Some(v) = guid.strip_prefix(prefix) {
            if let Some(id) = digits_or_tt(src, v) { found.push((src, id)); }
        }
    }
    // TMDB is the most exact for a movie and works with and without a key; then IMDb, then TheTVDB.
    ["tmdb", "imdb", "tvdb"].iter().find_map(|s| found.iter().find(|(f, _)| f == s).cloned())
}

/// Comparable form of a path (separators and letter case don't matter).
pub fn path_key(p: &str) -> String {
    p.replace('\\', "/").trim_end_matches('/').to_lowercase()
}

/// Translates an old path with the longest matching "from → to" rule.
pub fn map_path(p: &str, maps: &[(String, String)]) -> String {
    let kp = path_key(p);
    let best = maps
        .iter()
        .filter(|(f, _)| !f.trim().is_empty())
        .filter(|(f, _)| {
            let kf = path_key(f);
            kp == kf || kp.starts_with(&format!("{kf}/"))
        })
        .max_by_key(|(f, _)| f.len());
    let Some((f, t)) = best else { return p.to_string() };
    let skip = f.trim_end_matches(['/', '\\']).chars().count();
    let rest: String = p.chars().skip(skip).collect();
    let to = t.trim_end_matches(['/', '\\']);
    let joined = format!("{to}{rest}");
    if to.contains('/') && !to.contains('\\') {
        joined.replace('\\', "/")
    } else if to.contains('\\') {
        joined.replace('/', "\\")
    } else {
        joined
    }
}

/// A folder on this server for an old library folder: the same path if it
/// exists here, else a folder with the same name under /media or next to an
/// existing library folder.
pub(crate) fn suggest(old: &str, ours: &[String]) -> String {
    if Path::new(old).is_dir() {
        return old.to_string();
    }
    let name = old.replace('\\', "/").trim_end_matches('/').rsplit('/').next().unwrap_or("").to_string();
    if name.is_empty() {
        return String::new();
    }
    let mut places: Vec<PathBuf> = vec![PathBuf::from("/media").join(&name), PathBuf::from("/data").join(&name)];
    for o in ours {
        if path_key(o).ends_with(&format!("/{}", name.to_lowercase())) {
            places.insert(0, PathBuf::from(o));
        }
        if let Some(parent) = Path::new(o).parent() {
            places.push(parent.join(&name));
        }
    }
    places.iter().find(|p| p.is_dir()).map(|p| p.to_string_lossy().to_string()).unwrap_or_default()
}

// ------------------------------------------------------------------ preview

/// What the old database holds, with suggested folders and account matches.
pub fn preview(app: &App, src: &Path) -> Result<Value, String> {
    let old = read(src, &app.data_dir.join("cache").join("old-server"))?;
    let db = app.db.lock().unwrap();
    let ours: Vec<String> = db.sections.iter().flat_map(|s| s.paths.clone()).collect();
    let count = |k: i64| old.items.values().filter(|i| i.kind == k).count();
    let watched_by = |a: i64| old.settings.iter().filter(|s| s.account == a && s.views > 0).count();
    Ok(json!({
        "db": src.to_string_lossy(),
        "libraries": old.sections.iter().map(|s| json!({
            "id": s.id, "name": s.name, "type": match s.kind { 1 => "movie", 2 => "show", 8 => "music", 13 => "photo", _ => "other" },
            "imported": matches!(s.kind, 1 | 2),
            "folders": s.paths.iter().map(|p| json!({"old": p, "suggested": suggest(p, &ours), "exists_here": Path::new(p).is_dir()})).collect::<Vec<_>>(),
        })).collect::<Vec<_>>(),
        "accounts": old.accounts.iter().filter(|(_, n)| !n.trim().is_empty() || true).map(|(id, name)| {
            let matched = if *id == 1 { None } else { db.users.iter().find(|u| u.username.eq_ignore_ascii_case(name.trim())).map(|u| u.id) };
            json!({"id": id, "name": if name.trim().is_empty() { format!("account {id}") } else { name.clone() }, "owner": *id == 1,
                "watched": watched_by(*id), "suggested_user": matched})
        }).filter(|a| a["owner"] == true || a["watched"].as_u64().unwrap_or(0) > 0 || a["suggested_user"].is_number()).collect::<Vec<_>>(),
        "counts": {"movies": count(1), "shows": count(2), "episodes": count(4), "files": old.parts.len(),
            "watched_states": old.settings.iter().filter(|s| s.views > 0).count(),
            "resume_points": old.settings.iter().filter(|s| s.offset_ms > 0).count(),
            "ratings": old.settings.iter().filter(|s| s.rating.is_some()).count(),
            "playlists": old.playlists.iter().filter(|p| !p.smart).count(), "smart_playlists": old.playlists.iter().filter(|p| p.smart).count(),
            "credits_markers": old.markers.iter().filter(|m| m.1 == "credits").count()},
        "warnings": old.warnings,
    }))
}

// ------------------------------------------------------------------ import

#[derive(Debug, Clone)]
pub struct Options {
    pub db: PathBuf,
    pub path_map: Vec<(String, String)>,
    pub libraries: bool,
    pub watched: bool,
    pub playlists: bool,
    pub markers: bool,
    pub details: bool,
    /// old account id → user here (missing = skipped)
    pub accounts: HashMap<i64, u64>,
}

/// The error of an import that was cancelled between two of its steps.
pub const CANCELLED: &str = "Cancelled";

fn set_job(app: &App, label: &str, done: usize, total: usize) {
    *app.job.lock().unwrap() = Some((format!("Importing: {label}"), done, total));
}

/// Runs the whole import (call it on a background thread). Returns a report.
pub fn run(app: &Arc<App>, o: &Options) -> Result<Value, String> {
    set_job(app, "reading the old database", 0, 6);
    let old = read(&o.db, &app.data_dir.join("cache").join("old-server"))?;
    let mut report = json!({"started_at": now(), "db": o.db.to_string_lossy(), "warnings": old.warnings});

    // 1. Libraries: make the missing ones and scan them (only new files are read).
    let mut created = vec![];
    let mut scanned = vec![];
    if o.libraries {
        let todo: Vec<&OldSection> = old.sections.iter().filter(|s| matches!(s.kind, 1 | 2)).collect();
        for (n, s) in todo.iter().enumerate() {
            set_job(app, &format!("library \"{}\"", s.name), n, todo.len());
            let paths: Vec<String> = s.paths.iter().map(|p| map_path(p, &o.path_map)).collect();
            let kind = if s.kind == 2 { SectionKind::Show } else { SectionKind::Movie };
            let id = {
                let mut db = app.db.lock().unwrap();
                let keys: HashSet<String> = paths.iter().map(|p| path_key(p)).collect();
                match db.sections.iter().find(|x| x.kind == kind && x.paths.iter().any(|p| keys.contains(&path_key(p)))) {
                    Some(x) => x.id,
                    None => {
                        let id = db.new_id();
                        let lang: String = s.language.chars().take(2).collect::<String>().to_lowercase();
                        let language = if db::LANGUAGES.iter().any(|l| l.0 == lang) && lang != db.settings.metadata_language { lang } else { String::new() };
                        db.sections.push(Section { id, name: s.name.clone(), kind, paths: paths.clone(), created_at: now(), scanned_at: 0, credits_detection: true, intro_detection: true, language, scan_interval_minutes: None });
                        created.push(json!({"id": id, "name": s.name, "folders": paths}));
                        id
                    }
                }
            };
            app.save();
            // Another scan of this library may be running (folder watching): wait for it.
            for _ in 0..600 {
                if app.scan_now(id).is_some() {
                    break;
                }
                std::thread::sleep(std::time::Duration::from_millis(500));
            }
            scanned.push(s.name.clone());
        }
    }
    report["libraries_created"] = json!(created);
    report["libraries_scanned"] = json!(scanned);

    // 2. Match files: old item → item here.
    if app.job_cancelled() { return Err(CANCELLED.into()); }
    set_job(app, "matching files", 2, 6);
    let mut map: HashMap<i64, u64> = HashMap::new();
    let mut missing = vec![];
    {
        let db = app.db.lock().unwrap();
        let by_path: HashMap<String, u64> = db.items.values().filter_map(|i| i.file.as_ref().map(|f| (path_key(&f.path), i.id))).collect();
        for (oid, file) in &old.parts {
            let here = map_path(file, &o.path_map);
            match by_path.get(&path_key(&here)) {
                Some(id) => {
                    map.insert(*oid, *id);
                }
                None => missing.push(here),
            }
        }
        // Seasons and shows follow from their episodes.
        let leaves: Vec<(i64, u64)> = map.iter().map(|(a, b)| (*a, *b)).collect();
        for (oid, id) in leaves {
            let (mut o_cur, mut h_cur) = (oid, id);
            for _ in 0..2 {
                let (Some(op), Some(hp)) = (old.items.get(&o_cur).and_then(|i| i.parent), db.items.get(&h_cur).and_then(|i| i.parent_id)) else { break };
                if !old.items.get(&op).is_some_and(|i| matches!(i.kind, 2 | 3)) {
                    break;
                }
                map.entry(op).or_insert(hp);
                (o_cur, h_cur) = (op, hp);
            }
        }
    }
    let matched_files = old.parts.len() - missing.len();
    report["files"] = json!({"in_old_database": old.parts.len(), "found_here": matched_files, "not_found": missing.len(),
        "not_found_examples": missing.iter().take(12).collect::<Vec<_>>()});
    // One guid can stand for several items here (the same movie in "Movies" and "Movies 4K").
    let mut by_guid: HashMap<&str, Vec<u64>> = HashMap::new();
    for i in old.items.values().filter(|i| !i.guid.is_empty()) {
        if let Some(h) = map.get(&i.id) { by_guid.entry(i.guid.as_str()).or_default().push(*h); }
    }
    let items_of = |guid: &str| by_guid.get(guid).cloned().unwrap_or_default();

    let mut db = app.db.lock().unwrap();
    // The day the old server added things (not the day of this import): "Recently added" and the
    // season premieres in Continue Watching depend on it.
    for (oid, id) in &map {
        let added = old.items.get(oid).map_or(0, |i| i.added);
        if let Some(it) = db.items.get_mut(id).filter(|_| added > 0) {
            it.added_at = it.added_at.min(added as u64);
        }
    }

    // 3. Watched state, resume points, ratings.
    if app.job_cancelled() { return Err(CANCELLED.into()); }
    set_job(app, "watched state and ratings", 3, 6);
    let (mut watched, mut resume, mut rated, mut skipped_accounts) = (0, 0, 0, HashSet::new());
    if o.watched {
        for s in &old.settings {
            let Some(user) = o.accounts.get(&s.account).copied() else {
                skipped_accounts.insert(s.account);
                continue;
            };
            if !db.users.iter().any(|u| u.id == user) {
                continue;
            }
            for item in items_of(&s.guid) {
                let leaf = db.items.get(&item).is_some_and(|i| i.kind.playable());
                let v = db.view_mut(user, item);
                if leaf && s.views > 0 && (s.views as u32) > v.view_count {
                    v.view_count = s.views as u32;
                    watched += 1;
                }
                if leaf && s.offset_ms > 0 && s.last_viewed >= v.last_viewed_at as i64 {
                    v.view_offset_ms = s.offset_ms as u64;
                    resume += 1;
                }
                // Finished there after it was last played here: the resume point here is from before.
                if leaf && s.views > 0 && s.offset_ms == 0 && v.view_offset_ms > 0 && s.last_viewed >= v.last_viewed_at as i64 {
                    v.view_offset_ms = 0;
                }
                if s.last_viewed as u64 > v.last_viewed_at {
                    v.last_viewed_at = s.last_viewed as u64;
                }
                if let Some(r) = s.rating {
                    v.rating = Some((r as f32).clamp(0.0, 10.0));
                    rated += 1;
                }
            }
        }
    }
    // The play history: every viewing the old server logged; for watched items it has no log of, one viewing at the last time.
    let mut history = 0;
    if o.watched {
        let mut logged: HashSet<(u64, u64)> = HashSet::new();
        // Viewings already in the history (an earlier import) are not added again.
        let mut had: HashSet<(u64, u64, u64)> = db.history.iter().map(|e| (e.user_id, e.item_id, e.at)).collect();
        for (account, guid, at) in &old.views {
            let Some(user) = o.accounts.get(account).copied() else { continue };
            for item in items_of(guid) {
                if !db.users.iter().any(|u| u.id == user) || !db.items.get(&item).is_some_and(|i| i.kind.playable()) { continue; }
                logged.insert((user, item));
                if !had.insert((user, item, *at as u64)) { continue; }
                db.record_play_at(user, item, false, *at as u64);
                history += 1;
            }
        }
        for s in old.settings.iter().filter(|s| s.views > 0 && s.last_viewed > 0) {
            let Some(user) = o.accounts.get(&s.account).copied() else { continue };
            for item in items_of(&s.guid) {
                if logged.contains(&(user, item)) || !db.users.iter().any(|u| u.id == user) || !db.items.get(&item).is_some_and(|i| i.kind.playable()) { continue; }
                if !had.insert((user, item, s.last_viewed as u64)) { continue; }
                db.record_play_at(user, item, false, s.last_viewed as u64);
                history += 1;
            }
        }
    }
    report["watched"] = json!({"watched": watched, "history_entries": history, "resume_points": resume, "ratings": rated,
        "accounts_skipped": old.accounts.iter().filter(|(id, _)| skipped_accounts.contains(id)).map(|(_, n)| n.clone()).collect::<Vec<_>>()});

    // 4. Credits markers the old server found (it uses a trained model for that).
    if app.job_cancelled() { return Err(CANCELLED.into()); }
    set_job(app, "credits markers", 4, 6);
    let mut markers = 0;
    if o.markers {
        let mut per_item: HashMap<u64, Vec<u64>> = HashMap::new();
        for (oid, kind, start, _end) in &old.markers {
            if kind == "credits" && *start > 0 {
                if let Some(id) = map.get(oid) {
                    per_item.entry(*id).or_default().push(*start as u64);
                }
            }
        }
        for (id, mut c) in per_item {
            c.sort();
            c.dedup();
            if let Some(f) = db.items.get_mut(&id).and_then(|i| i.file.as_mut()) {
                f.credits_ms = c;
                f.credits_source = "imported".into();
                f.credits_checked = db::CREDITS_DETECTOR_VERSION;
                f.credits_errors = 0;
                markers += 1;
            }
        }
    }
    report["credits_markers"] = json!(markers);

    // 5. Titles, plots, genres … (kept: online details won't replace them).
    if app.job_cancelled() { return Err(CANCELLED.into()); }
    set_job(app, "titles and plots", 5, 6);
    let mut details = 0;
    let (mut people, mut people_photos): (usize, Vec<(String, String)>) = (0, vec![]);
    if o.details {
        for (oid, id) in &map {
            let Some(oi) = old.items.get(oid) else { continue };
            let Some(it) = db.items.get_mut(id) else { continue };
            let mut m = json!({});
            for (k, v) in [("title", &oi.title), ("summary", &oi.summary), ("tagline", &oi.tagline), ("studio", &oi.studio), ("content_rating", &oi.content_rating)] {
                if !v.trim().is_empty() {
                    m[k] = json!(v.trim());
                }
            }
            // Season names stay "Season 1"; their plot is taken.
            if oi.kind == 3 {
                m.as_object_mut().map(|o| o.remove("title"));
            }
            if let Some(y) = oi.year {
                m["year"] = json!(y);
            }
            if let Some(r) = oi.rating {
                m["rating"] = json!((r * 10.0).round() / 10.0);
            }
            if let Some(g) = old.genres.get(oid).filter(|g| !g.is_empty()) {
                m["genres"] = json!(g);
            }
            // Actors, directors and writers (shared people).
            if let Some(cast) = old.cast.get(oid).filter(|c| !c.is_empty()) {
                let list: Vec<Value> = cast
                    .iter()
                    .map(|(name, role, photo)| {
                        let mut c = json!({"name": name, "role": role});
                        c["key"] = json!(crate::server::person_key(&c));
                        if photo.starts_with("http://") || photo.starts_with("https://") {
                            c["photo"] = json!(photo);
                            people_photos.push((c["key"].as_str().unwrap_or("").to_string(), photo.clone()));
                        }
                        c
                    })
                    .collect();
                people += list.len();
                m["extra"]["cast"] = json!(list);
            }
            for (k, src) in [("directors", &old.directors), ("writers", &old.writers), ("collections", &old.collections), ("producers", &old.producers), ("countries", &old.countries)] {
                if let Some(v) = src.get(oid).filter(|v| !v.is_empty()) {
                    m["extra"][k] = json!(v);
                }
            }
            if m.as_object().is_some_and(|o| o.is_empty()) {
                continue;
            }
            if !it.extra.contains_key("file_title") {
                it.extra.insert("file_title".into(), json!(it.title));
            }
            apply_metadata(it, &m, true);
            if !oi.original_title.trim().is_empty() && oi.original_title != oi.title {
                it.extra.insert("original_title".into(), json!(oi.original_title));
            }
            if !oi.aired.is_empty() {
                it.extra.insert("originally_available".into(), json!(oi.aired));
            }
            it.extra.insert("imported_from_old_server".into(), json!(now()));
            details += 1;
        }
    }
    report["details"] = json!(details);
    report["cast_entries"] = json!(people);

    // The match Plex made: the online lookup takes exactly that entry instead of searching by title.
    let mut linked = 0;
    let mut fresh: Vec<u64> = vec![];
    let mut without: Vec<String> = vec![];
    let mut candidates = 0;
    for (oid, id) in &map {
        let Some(oi) = old.items.get(oid) else { continue };
        if !matches!(oi.kind, 1 | 2) { continue; }
        candidates += 1;
        let tags = old.external.get(oid).map(|v| v.as_slice()).unwrap_or(&[]);
        let Some((src, eid)) = plex_external_id(&oi.guid, tags) else {
            if without.len() < 5 { without.push(format!("{} \u{2192} {}{}", oi.title, oi.guid, if tags.is_empty() { String::new() } else { format!(" {tags:?}") })); }
            continue;
        };
        let Some(it) = db.items.get_mut(id) else { continue };
        it.extra.insert("plex_id".into(), json!({"source": src, "id": eid}));
        linked += 1;
        // Not matched here yet: its details are fetched at once, whatever an earlier miss was.
        if !it.extra.contains_key("metadata_source") && it.extra.get("unmatched_by_hand").and_then(|v| v.as_bool()) != Some(true) {
            fresh.push(*id);
        }
    }
    report["matches"] = json!(linked);
    report["match_candidates"] = json!(candidates);
    report["match_examples_without_id"] = json!(without);

    // 6. Playlists (manual ones; smart ones use the old server's own rule format).
    if app.job_cancelled() { return Err(CANCELLED.into()); }
    set_job(app, "playlists", 6, 6);
    let (mut made, mut updated) = (vec![], vec![]);
    let mut smart_skipped = vec![];
    if o.playlists {
        for p in &old.playlists {
            if p.smart {
                smart_skipped.push(p.title.clone());
                continue;
            }
            let Some(owner) = o.accounts.get(&p.account).copied() else { continue };
            let mut items: Vec<u64> = vec![];
            for oi in &p.items {
                let Some(h) = map.get(oi) else { continue };
                for l in db.leaves(*h) {
                    if !items.contains(&l) {
                        items.push(l);
                    }
                }
            }
            if items.is_empty() {
                continue;
            }
            let t = now();
            if let Some(ex) = db.playlists.iter_mut().find(|x| x.owner_id == owner && x.kind == PlaylistKind::Manual && x.title == p.title) {
                ex.items = items;
                ex.updated_at = t;
                updated.push(p.title.clone());
            } else {
                let id = db.new_id();
                db.playlists.push(Playlist { id, owner_id: owner, title: p.title.clone(), summary: p.summary.clone(), kind: PlaylistKind::Manual, items, query: None, default_key: None, shared: false, pinned: false, created_at: t, updated_at: t });
                made.push(p.title.clone());
            }
        }
    }
    report["playlists"] = json!({"created": made, "updated": updated, "smart_not_imported": smart_skipped});
    drop(db);
    app.save();
    // Actor photos the old server knew: one per person, shared by everything they're in.
    people_photos.sort();
    people_photos.dedup_by(|a, b| a.0 == b.0);
    let mut photos = 0;
    let total = people_photos.len();
    for (n, (key, url)) in people_photos.iter().enumerate() {
        if key.is_empty() {
            continue;
        }
        if n % 20 == 0 {
            set_job(app, &format!("actor photos {n} of {total}"), 5, 6);
        }
        let dest = app.person_photo(key);
        if crate::library::metadata::file_image_type(&dest).is_some() || crate::library::metadata::download(url, &dest) {
            photos += 1;
        }
    }
    if photos > 0 {
        let mut db = app.db.lock().unwrap();
        for id in map.values() {
            let Some(it) = db.items.get_mut(id) else { continue };
            let Some(Value::Array(cast)) = it.extra.get_mut("cast") else { continue };
            for c in cast.iter_mut() {
                let key = c["key"].as_str().unwrap_or("").to_string();
                if !key.is_empty() && c["thumb"].is_null() && crate::library::metadata::file_image_type(&app.person_photo(&key)).is_some() {
                    c["thumb"] = json!(format!("/library/people/{key}/thumb"));
                }
            }
        }
        drop(db);
        app.save();
    }
    report["actor_photos"] = json!(photos);
    report["finished_at"] = json!(now());
    let _ = std::fs::write(app.data_dir.join("cache").join("old-server").join("last-import.json"), serde_json::to_string_pretty(&report).unwrap_or_default());
    // Online details for what's new (posters, cast); imported titles and plots stay.
    let ids: Vec<u64> = map.values().copied().collect();
    app.queue_online_details(&ids, false);
    // Titles with Plex's number are looked up for sure (a miss from before does not hold them back).
    let online = app.db.lock().unwrap().settings.metadata_enabled;
    if online && !fresh.is_empty() {
        app.queue_online_details(&fresh, true);
    }
    Ok(report)
}

/// A one-line summary of a report.
pub fn summary(r: &Value) -> String {
    format!(
        "Import finished: {} of {} files found, {} libraries added, {} watched, {} resume points, {} ratings, {} playlists, {} credits markers, details for {} items",
        r["files"]["found_here"], r["files"]["in_old_database"], r["libraries_created"].as_array().map(|a| a.len()).unwrap_or(0),
        r["watched"]["watched"], r["watched"]["resume_points"], r["watched"]["ratings"],
        r["playlists"]["created"].as_array().map(|a| a.len()).unwrap_or(0) + r["playlists"]["updated"].as_array().map(|a| a.len()).unwrap_or(0),
        r["credits_markers"], r["details"]
    )
}

pub fn options_from(body: &Value, me: u64) -> Result<Options, String> {
    let given = body["db"].as_str().map(str::trim).filter(|s| !s.is_empty()).map(PathBuf::from);
    let db = match given {
        Some(p) => find_db(&p).ok_or_else(|| format!("No {DB_NAME} found at {}", p.display()))?,
        None => detect().ok_or("The old server's data folder was not found; enter where it is")?,
    };
    let path_map: Vec<(String, String)> = body["path_map"]
        .as_array()
        .map(|a| a.iter().filter_map(|m| Some((m["from"].as_str()?.to_string(), m["to"].as_str()?.to_string()))).filter(|(f, t)| !f.trim().is_empty() && !t.trim().is_empty()).collect())
        .unwrap_or_default();
    let mut accounts: HashMap<i64, u64> = HashMap::new();
    if let Some(m) = body["accounts"].as_object() {
        for (k, v) in m {
            if let (Ok(old), Some(here)) = (k.parse::<i64>(), v.as_u64().filter(|u| *u > 0)) {
                accounts.insert(old, here);
            }
        }
    } else {
        accounts.insert(1, me);
    }
    let on = |k: &str| body.get(k).and_then(Value::as_bool).unwrap_or(true);
    Ok(Options { db, path_map, libraries: on("libraries"), watched: on("watched"), playlists: on("playlists"), markers: on("markers"), details: on("details"), accounts })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_plex_external_ids() {
        let v = |s: &[&str]| s.iter().map(|x| x.to_string()).collect::<Vec<_>>();
        let ext = plex_external_id;
        assert_eq!(ext("plex://movie/5d776", &v(&["imdb://tt0133093", "tmdb://603", "tvdb://169"])), Some(("tmdb", "603".into())));
        assert_eq!(ext("plex://movie/5d776", &v(&["imdb://tt0133093"])), Some(("imdb", "tt0133093".into())));
        assert_eq!(ext("com.plexapp.agents.imdb://tt0133093?lang=en", &[]), Some(("imdb", "tt0133093".into())));
        assert_eq!(ext("com.plexapp.agents.themoviedb://603?lang=de", &[]), Some(("tmdb", "603".into())));
        assert_eq!(ext("com.plexapp.agents.thetvdb://81189?lang=en", &[]), Some(("tvdb", "81189".into())));
        assert_eq!(ext("com.plexapp.agents.none://1", &[]), None);
        assert_eq!(ext("local://5", &v(&["imdb://xx"])), None);
    }
    #[test]
    fn paths() {
        let maps = vec![("D:\\Filme".to_string(), "/media/Filme".to_string()), ("D:\\".to_string(), "/media/other".to_string())];
        assert_eq!(map_path("D:\\Filme\\Alien (1979)\\Alien.mkv", &maps), "/media/Filme/Alien (1979)/Alien.mkv");
        assert_eq!(map_path("d:\\filme\\x.mkv", &maps), "/media/Filme/x.mkv");
        assert_eq!(map_path("D:\\Serien\\a.mkv", &maps), "/media/other/Serien/a.mkv");
        assert_eq!(map_path("/volume1/video/a.mkv", &maps), "/volume1/video/a.mkv");
        assert_eq!(map_path("/volume1/video/a.mkv", &[("/volume1/video".into(), "/media".into())]), "/media/a.mkv");
        assert_eq!(path_key("C:\\Movies\\A.mkv"), path_key("c:/movies/a.mkv"));
        assert_eq!(date_to_epoch("1970-01-02 00:00:10"), Some(86410));
        assert_eq!(db_date(86400 * 365), "1971-01-01");
    }
}

#[cfg(test)]
mod credits_index_tests {
    use super::*;
    #[test]
    fn finds_files_under_another_root() {
        let mut idx = CreditsIndex::default();
        let v = vec![2_648_297u64];
        let k = path_key("/volume1/Serien/Daisy Jones/Season 1/S01E01.mkv");
        for (n, t) in [&mut idx.tail3, &mut idx.tail2].into_iter().enumerate() {
            t.insert(path_tail(&k, 3 - n), Some(v.clone()));
        }
        idx.exact.insert(k, v.clone());
        assert_eq!(idx.get("/data/tv/Daisy Jones/Season 1/S01E01.mkv"), Some(&v));
        assert_eq!(idx.get(r"D:\Daisy Jones\Season 1\S01E01.mkv"), Some(&v));
        assert_eq!(idx.get("/data/tv/Other/Season 1/S01E02.mkv"), None);
    }
}

#[cfg(test)]
mod import_tests {
    use super::*;
    use crate::data::db::{Item, MediaFile};

    /// A tiny old-server database: two old movies with the same guid ("Movies" and "Movies 4K"),
    /// watched and rated by account 1, with two logged viewings.
    fn old_db(dir: &Path) -> PathBuf {
        std::fs::create_dir_all(dir).unwrap();
        let f = dir.join("plex.db");
        let c = Connection::open(&f).unwrap();
        c.execute_batch("create table metadata_items (id, parent_id, metadata_type, guid, title, original_title, summary, tagline, studio, content_rating, rating, year, originally_available_at, added_at);
            create table media_items (id, metadata_item_id); create table media_parts (id, media_item_id, file);
            create table metadata_item_settings (account_id, guid, rating, view_offset, view_count, last_viewed_at);
            create table metadata_item_views (account_id, guid, viewed_at); create table accounts (id, name);
            insert into accounts values (1, 'ste');
            insert into metadata_items values (100, null, 1, 'plex://movie/x', 'X', '', '', '', '', '', null, 2000, null, 0), (101, null, 1, 'plex://movie/x', 'X', '', '', '', '', '', null, 2000, null, 0);
            insert into media_items values (1, 100), (2, 101); insert into media_parts values (1, 1, '/m/a.mkv'), (2, 2, '/m4k/a.mkv');
            insert into metadata_item_settings values (1, 'plex://movie/x', 8, 0, 1, 1000);
            insert into metadata_item_views values (1, 'plex://movie/x', 500), (1, 'plex://movie/x', 900);").unwrap();
        f
    }

    fn app_with_two_copies(name: &str) -> (Arc<App>, PathBuf, Options) {
        let (app, root) = crate::server::tests::test_app(name);
        {
            let mut db = app.db.lock().unwrap();
            for (id, path) in [(10u64, "/m/a.mkv"), (11, "/m4k/a.mkv")] {
                let mut i: Item = serde_json::from_value(json!({"id": id, "section_id": 1, "kind": "movie", "title": "X", "sort_title": "x", "added_at": 0, "updated_at": 0})).unwrap();
                i.file = Some(MediaFile { path: path.into(), ..Default::default() });
                db.items.insert(id, i);
            }
        }
        let o = Options { db: old_db(&root.join("old")), path_map: vec![], libraries: false, watched: true, playlists: false, markers: false, details: false, accounts: HashMap::from([(1, 1)]) };
        (app, root, o)
    }

    #[test]
    fn the_same_movie_in_two_old_libraries_is_watched_in_both() {
        let (app, root, o) = app_with_two_copies("import-guid");
        run(&app, &o).unwrap();
        let db = app.db.lock().unwrap();
        let got: Vec<(u32, Option<f32>, usize)> = [10, 11].iter().map(|i| (db.view(1, *i).view_count, db.view(1, *i).rating, db.history.iter().filter(|h| h.item_id == *i).count())).collect();
        drop(db);
        let _ = std::fs::remove_dir_all(&root);
        assert_eq!(got, vec![(1, Some(8.0), 2), (1, Some(8.0), 2)], "(watched, rating, history entries) of each copy");
    }

    #[test]
    fn importing_twice_keeps_the_play_history_as_it_was() {
        let (app, root, o) = app_with_two_copies("import-twice");
        run(&app, &o).unwrap();
        let first = app.db.lock().unwrap().history.len();
        run(&app, &o).unwrap();
        let second = app.db.lock().unwrap().history.len();
        let _ = std::fs::remove_dir_all(&root);
        assert!(first > 0);
        assert_eq!(second, first, "the second import added the viewings again");
    }

    #[test]
    fn the_old_database_is_copied_again_only_when_it_changed() {
        let root = std::env::temp_dir().join(format!("lichtspiel-open-copy-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let src = old_db(&root);
        let work = root.join("work");
        drop(open_copy(&src, &work).unwrap());
        // Something only the copy has: a fresh copy would lose it.
        Connection::open(work.join("library.db")).unwrap().execute_batch("create table only_in_copy (x)").unwrap();
        let kept = open_copy(&src, &work).unwrap().prepare("select * from only_in_copy").is_ok();
        // The old server writes to its database: copied again.
        std::thread::sleep(std::time::Duration::from_millis(20));
        Connection::open(&src).unwrap().execute_batch("insert into accounts values (2, 'kid')").unwrap();
        let fresh = open_copy(&src, &work).unwrap().prepare("select * from only_in_copy").is_err();
        let _ = std::fs::remove_dir_all(&root);
        assert!(kept, "the unchanged old database was copied again");
        assert!(fresh, "a changed old database was not copied again");
    }
}

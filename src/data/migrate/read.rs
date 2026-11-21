//! Finding the old server's database and reading it from a copy: libraries, items, accounts, watch state, playlists and markers.

use super::*;

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
pub(super) fn date_to_epoch(s: &str) -> Option<i64> {
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
pub(super) fn open_copy(src: &Path, work: &Path) -> Result<Connection, String> {
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
    pub(super) exact: HashMap<String, Vec<u64>>,
    pub(super) tail3: HashMap<String, Option<Vec<u64>>>,
    pub(super) tail2: HashMap<String, Option<Vec<u64>>>,
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
pub(super) fn path_tail(k: &str, n: usize) -> String {
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

pub(super) fn db_date(t: i64) -> String {
    if t <= 0 {
        return String::new();
    }
    let (y, m, d) = crate::connect::rss::civil(t.div_euclid(86400));
    format!("{y:04}-{m:02}-{d:02}")
}

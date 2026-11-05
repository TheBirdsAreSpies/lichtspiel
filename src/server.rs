//! The HTTP server core: shared state, responses, the request loop,
//! background jobs and the JSON shapes. Library, hub, playlist and
//! transcode paths follow the names (/library/sections,
//! /library/metadata/{id}, /hubs/home/…, /playlists, /:/timeline,
//! /video/…/transcode); admin features live under /api/.
//! Routes are in routes.rs, the larger handlers in handlers.rs.

use std::net::IpAddr;
use crate::db::{self, now, Db, Invite, Item, ItemKind, Playlist, PlaylistKind, Section, User};
use crate::plugins::{self, Plugins};
use crate::query;
use crate::scanner;
use crate::store::{self, Changes, Store};
use crate::transcode::Method;
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::Child;
use std::sync::atomic::{AtomicBool, Ordering as AtomicOrdering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tiny_http::{Header, Response, StatusCode};

/// Raised when automatic matching learns something new (2: the movie's folder name
/// is tried too; 3: also above "CD1" or a folder named like the file, and first when
/// only the folder has a year), so titles that were not found before are tried once more.
pub const MATCH_VERSION: u64 = 3;

pub const VERSION: &str = env!("CARGO_PKG_VERSION");
/// The server's name (after the old German word for cinema, "Lichtspielhaus").
pub const APP_NAME: &str = "Lichtspiel";
/// The default port; it can be
/// changed in Settings (saved, used after a restart) or with --port /
/// LICHTSPIEL_PORT.
pub const DEFAULT_PORT: u16 = 8484;

/// How often library folders are looked at for new or changed files (seconds).
/// (The self-test makes it shorter.)
pub static WATCH_SECONDS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(15);

pub const WEB_APP: &str = include_str!("../web/index.html");
pub const WEB_JS: &str = include_str!("../web/app.js");
pub const WEB_PAGES_JS: &str = include_str!("../web/pages.js");
pub const WEB_DASHBOARD_JS: &str = include_str!("../web/dashboard.js");
pub const WEB_DOWNLOADS_JS: &str = include_str!("../web/downloads.js");
pub const WEB_I18N_JS: &str = include_str!("../web/i18n.js");
/// Language packs for the web app (web/lang/<code>.js); English needs none.
/// To add a language: put its file in web/lang, list it here and add a
/// <script> line for it in index.html.
pub const WEB_LANGS: &[(&str, &str)] = &[("de.js", include_str!("../web/lang/de.js"))];

pub type Resp = Response<Box<dyn Read + Send>>;

pub struct HlsSession {
    pub id: String,
    pub item_id: u64,
    pub user_id: u64,
    pub dir: PathBuf,
    pub child: Child,
    pub last_access: Instant,
    pub started_at: u64,
    pub method: Method,
    pub height: u32,
    /// The audio track asked for (its stream index), None = the default one.
    pub audio: Option<u32>,
    pub viewer: Viewer,
}

/// Where a video is played: the device and its address, for the dashboard.
#[derive(Clone, Debug, Default)]
pub struct Viewer {
    /// "Plex for Samsung — TV 2020", "Chrome on Windows" …
    pub device: String,
    pub ip: String,
    /// In this computer's local network (see `Ctx::is_local`).
    pub local: bool,
}

#[derive(Clone)]
pub struct Playing {
    pub user_id: u64,
    pub item_id: u64,
    pub offset_ms: u64,
    pub state: String,
    pub method: String,
    pub updated: Instant,
    pub client: String,
    /// Converted streams: the picture height sent (0 = as it is).
    pub height: u32,
    pub audio: Option<u32>,
    pub viewer: Viewer,
    /// Player reports: when this play started, and how long it has played so far
    /// (for the play statistics; 0 for streams).
    pub session_at: u64,
    pub watched_ms: u64,
}

impl Playing {
    /// The play so far, counting the time since a "playing" report (as much as
    /// a report adds at most, see `timeline`).
    pub fn session(&self) -> db::PlaySession {
        let since = if self.state == "playing" { (self.updated.elapsed().as_millis() as u64).min(30_000) } else { 0 };
        db::PlaySession { user_id: self.user_id, item_id: self.item_id, at: self.session_at, watched_ms: self.watched_ms + since }
    }
}

pub struct App {
    pub data_dir: PathBuf,
    /// The library file. Locked before `db` when both are needed (only `save` does).
    pub store: Mutex<Store>,
    pub db: Mutex<Db>,
    pub plugins: Mutex<Plugins>,
    pub hls: Mutex<HashMap<String, HlsSession>>,
    pub progressive: Mutex<HashMap<String, Playing>>,
    pub playing: Mutex<HashMap<String, Playing>>,
    /// Playbacks the owner stopped from the dashboard: (user, item) → (message, when).
    /// Their player is told on its next report, and new streams are refused for a short while.
    pub stops: Mutex<HashMap<(u64, u64), (String, Instant)>>,
    pub scanning: Mutex<HashSet<u64>>,
    pub started_at: u64,
    pub port: Mutex<u16>,
    /// Movies and shows waiting for online details: (item id, force).
    pub meta_queue: Mutex<std::collections::VecDeque<(u64, bool)>>,
    /// Item being looked up right now (0 = none).
    pub meta_busy: Mutex<Vec<u64>>,
    pub meta_progress: Mutex<MetaProgress>,
    /// Background jobs other than details (credits detection): (label, done, total).
    pub job: Mutex<Option<(String, usize, usize)>>,
    /// Set by "Cancel": the running job stops after the file it is working on.
    pub job_cancel: AtomicBool,
    /// Processor, memory and bandwidth samples and who was here lately (the dashboard).
    pub stats: crate::stats::Stats,
}

/// Counts finished details lookups since the server started, so the web app
/// can show progress and refresh pages when something was updated.
#[derive(Default)]
pub struct MetaProgress {
    pub done: u64,
    pub failed: u64,
    pub last: Value,
}

/// Bumped when the picture download changes; matched items with an older
/// number get their pictures fetched again on start (2 = actor photos saved).
pub const PICTURES_VERSION: u64 = 2;

/// Opens the library file in a data folder. A first start after an older
/// version reads its JSON file (returned third, to be renamed once saved).
/// A library file that can't be read stops the server instead of starting
/// it with an empty library.
fn open_library(data_dir: &Path) -> (Store, Db, Option<PathBuf>) {
    let file = data_dir.join(store::FILE_NAME);
    let stop = |e: String| -> ! {
        elog!("the library can't be opened: {e}");
        elog!("nothing was changed; move {} away to start with an empty library", file.display());
        std::process::exit(1);
    };
    let mut st = Store::open(&file).unwrap_or_else(|e| stop(e));
    match st.load() {
        Ok(Some(mut db)) => {
            db.after_load();
            (st, db, None)
        }
        Ok(None) => {
            let json = data_dir.join(store::OLD_JSON);
            let mut db = if json.exists() { Db::load_json(&json) } else { Db::default() };
            db.items.mark_all();
            db.views.mark_all();
            (st, db, json.exists().then_some(json))
        }
        Err(e) => stop(e),
    }
}

impl App {
    pub fn open(data_dir: &Path) -> Arc<App> {
        // A full path, so saved picture paths still work when the server is
        // started from a different folder.
        let abs = std::path::absolute(data_dir).unwrap_or_else(|_| data_dir.to_path_buf());
        let data_dir = abs.as_path();
        let _ = std::fs::create_dir_all(data_dir);
        for sub in ["plugins", "cache/thumbs", "transcode"] {
            let _ = std::fs::create_dir_all(data_dir.join(sub));
        }
        install_bundled_plugins(&data_dir.join("plugins"));
        let (store, mut db, from_json) = open_library(data_dir);
        // Age ratings and genres in one spelling each.
        let ids: Vec<u64> = db.items.values().filter(|it| {
            let mut c = (*it).clone();
            c.normalize();
            c.content_rating != it.content_rating || c.genres != it.genres
        }).map(|it| it.id).collect();
        for id in ids {
            if let Some(it) = db.items.get_mut(&id) { it.normalize(); }
        }
        // Older versions saved picture paths relative to the start folder.
        let relative = |p: &str| Path::new(p).is_relative() && Path::new(p).exists();
        let ids: Vec<u64> = db
            .items
            .values()
            .filter(|it| it.poster.as_deref().is_some_and(relative) || it.extra.get("art").and_then(|v| v.as_str()).is_some_and(relative))
            .map(|it| it.id)
            .collect();
        for id in ids {
            let Some(it) = db.items.get_mut(&id) else { continue };
            if let Some(p) = it.poster.as_ref().filter(|p| relative(p)) {
                it.poster = std::path::absolute(p).ok().map(|a| a.to_string_lossy().to_string()).or(it.poster.clone());
            }
            if let Some(p) = it.extra.get("art").and_then(|v| v.as_str()).filter(|p| relative(p)) {
                if let Ok(a) = std::path::absolute(p) {
                    it.extra.insert("art".into(), json!(a.to_string_lossy()));
                }
            }
        }
        let plugins = Plugins::load(&data_dir.join("plugins"));
        store.set_cache_mb(db.settings.db_cache_mb);
        let app = Arc::new(App {
            data_dir: data_dir.to_path_buf(),
            store: Mutex::new(store),
            db: Mutex::new(db),
            plugins: Mutex::new(plugins),
            hls: Mutex::new(HashMap::new()),
            progressive: Mutex::new(HashMap::new()),
            playing: Mutex::new(HashMap::new()),
            stops: Mutex::new(HashMap::new()),
            scanning: Mutex::new(HashSet::new()),
            started_at: now(),
            port: Mutex::new(DEFAULT_PORT),
            meta_queue: Mutex::new(Default::default()),
            meta_busy: Mutex::new(Vec::new()),
            meta_progress: Mutex::new(MetaProgress::default()),
            job: Mutex::new(None),
            job_cancel: AtomicBool::new(false),
            stats: Default::default(),
        });
        app.sync_plugin_states();
        match app.try_save() {
            Ok(()) => {
                // Taken over from an older version's file: keep it, but out of the way.
                if let Some(json) = from_json {
                    let kept = json.with_extension("json.migrated");
                    match std::fs::rename(&json, &kept) {
                        Ok(()) => elog!("the library was moved into {}; the old file is kept as {}", store::FILE_NAME, kept.display()),
                        Err(e) => elog!("the library was moved into {}, but {} could not be renamed: {e}", store::FILE_NAME, json.display()),
                    }
                }
            }
            Err(e) => {
                elog!("could not save the library: {e}");
                if from_json.is_some() {
                    elog!("the old library file is left as it is and will be read again on the next start");
                }
            }
        }
        // Items matched before actor photos were downloaded: fetch their pictures again.
        let stale: Vec<u64> = {
            let db = app.db.lock().unwrap();
            db.items
                .values()
                .filter(|i| i.extra.get("metadata_status").and_then(|v| v.as_str()) == Some("matched"))
                .filter(|i| i.extra.get("pictures_version").and_then(|v| v.as_u64()).unwrap_or(0) < PICTURES_VERSION)
                .map(|i| i.id)
                .collect()
        };
        app.queue_online_details(&stale, false);
        // Pictures that are not photos (an older version could save a film's
        // SVG logo as poster.jpg): delete them and fetch the details again.
        let broken = app.remove_broken_pictures();
        if !broken.is_empty() {
            elog!("{} items had a broken picture; getting their pictures again", broken.len());
            app.queue_online_details(&broken, true);
        }
        app
    }

    /// Where a person's shared photo is kept.
    pub fn person_photo(&self, key: &str) -> PathBuf {
        self.data_dir.join("metadata").join("people").join(format!("{key}.jpg"))
    }

    /// Deletes saved posters, backgrounds, stills and actor photos that aren't
    /// real photos. Returns the movies and shows they belonged to.
    pub fn remove_broken_pictures(&self) -> Vec<u64> {
        let bad = |p: &str| Path::new(p).is_file() && crate::metadata::file_image_type(Path::new(p)).is_none();
        let mut tops = HashSet::new();
        let mut db = self.db.lock().unwrap();
        let ids: Vec<u64> = db.items.keys().copied().collect();
        for id in ids {
            let top = Self::top_level(&db, id).unwrap_or(id);
            let Some(it) = db.items.get_mut(&id) else { continue };
            if let Some(p) = it.poster.clone().filter(|p| bad(p)) {
                let _ = std::fs::remove_file(&p);
                it.poster = None;
                tops.insert(top);
            }
            if let Some(p) = it.extra.get("art").and_then(|v| v.as_str()).map(String::from).filter(|p| bad(p)) {
                let _ = std::fs::remove_file(&p);
                it.extra.remove("art");
                tops.insert(top);
            }
        }
        drop(db);
        if let Ok(rd) = std::fs::read_dir(self.data_dir.join("metadata")) {
            for e in rd.flatten() {
                let Ok(files) = std::fs::read_dir(e.path().join("cast")) else { continue };
                for f in files.flatten() {
                    if bad(&f.path().to_string_lossy()) {
                        let _ = std::fs::remove_file(f.path());
                        if let Some(id) = e.file_name().to_str().and_then(|s| s.parse::<u64>().ok()) {
                            tops.insert(id);
                        }
                    }
                }
            }
        }
        let mut v: Vec<u64> = tops.into_iter().collect();
        v.sort();
        if !v.is_empty() {
            self.save();
            for id in &v {
                let _ = std::fs::remove_file(self.data_dir.join("cache").join("thumbs").join(format!("{id}.jpg")));
            }
        }
        v
    }

    /// Writes what changed since the last save. Callers must not hold `db`.
    pub fn save(&self) {
        if let Err(e) = self.try_save() {
            elog!("could not save the library: {e}");
        }
    }

    pub fn try_save(&self) -> Result<(), String> {
        // The file is locked first and kept until the write is done, so saves
        // reach it in the order their changes were taken; the library itself
        // is only locked while the changes are taken out.
        let mut st = self.store.lock().unwrap();
        let changes = Changes::take(&mut self.db.lock().unwrap());
        st.write(&changes).map_err(|e| {
            // Nothing of it was written: the next save writes everything.
            st.forget_written();
            let mut db = self.db.lock().unwrap();
            db.items.mark_all();
            db.views.mark_all();
            e.to_string()
        })
    }

    pub fn sync_plugin_states(&self) {
        let list: Vec<(String, bool)> = self
            .plugins
            .lock()
            .unwrap()
            .list
            .iter()
            .map(|p| (p.manifest.id.clone(), p.manifest.enabled_by_default))
            .collect();
        let mut db = self.db.lock().unwrap();
        for (id, on) in list {
            db.plugins.entry(id).or_insert(db::PluginState { enabled: on, settings: Default::default() });
        }
    }

    /// A folder chosen in the settings, else `<data>/<default>`.
    pub fn work_dir(&self, chosen: &str, default: &str) -> PathBuf {
        let c = chosen.trim();
        if c.is_empty() { self.data_dir.join(default) } else { PathBuf::from(c) }
    }
    pub fn transcode_dir(&self) -> PathBuf { self.work_dir(&self.settings().transcode_dir, "transcode") }
    pub fn downloads_dir(&self) -> PathBuf { self.work_dir(&self.settings().downloads_dir, "downloads") }

    pub fn settings(&self) -> db::Settings {
        self.db.lock().unwrap().settings.clone()
    }

    pub fn server_info(&self) -> Value {
        let port = *self.port.lock().unwrap();
        json!({"url": format!("{}://127.0.0.1:{port}", scheme()), "version": VERSION, "data_dir": self.data_dir.to_string_lossy()})
    }

    /// Plugins that listen to `event` and are switched on, with their settings.
    pub fn listeners(&self, event: &str) -> Vec<(plugins::Plugin, serde_json::Map<String, Value>)> {
        let p = self.plugins.lock().unwrap();
        let db = self.db.lock().unwrap();
        p.list
            .iter()
            .filter(|pl| pl.manifest.hooks.iter().any(|h| h == event || h == "*"))
            .filter_map(|pl| {
                let st = db.plugins.get(&pl.manifest.id)?;
                st.enabled.then(|| (pl.clone(), st.settings.clone()))
            })
            .collect()
    }

    /// What a plugin needs to know about an item to find it elsewhere: kind,
    /// title, year, the ids from the details download (imdb, tmdb, tvdb …) and,
    /// for an episode or season, the show with its ids and the numbers.
    pub fn item_brief(&self, id: u64) -> Value {
        let db = self.db.lock().unwrap();
        let Some(it) = db.items.get(&id) else { return Value::Null };
        let ids = |i: &Item| i.extra.get("ids").cloned().unwrap_or_else(|| json!({}));
        let show_of = |s: &Item| json!({"id": s.id, "title": s.title, "year": s.year, "ids": ids(s)});
        let mut o = json!({"id": it.id, "type": it.kind.as_str(), "title": it.title, "year": it.year, "duration_ms": it.duration_ms, "ids": ids(it)});
        let parent = it.parent_id.and_then(|p| db.items.get(&p));
        match it.kind {
            ItemKind::Episode => {
                o["episode"] = json!(it.index);
                if let Some(season) = parent {
                    o["season"] = json!(season.index);
                    if let Some(show) = season.parent_id.and_then(|g| db.items.get(&g)) { o["show"] = show_of(show); }
                }
            }
            ItemKind::Season => {
                o["season"] = json!(it.index);
                if let Some(show) = parent { o["show"] = show_of(show); }
            }
            _ => {}
        }
        o
    }

    /// Tells plugins about something, in the background.
    pub fn fire(self: &Arc<Self>, event: &str, data: Value) {
        let listeners = self.listeners(event);
        if listeners.is_empty() {
            return;
        }
        let app = self.clone();
        let event = event.to_string();
        let mut data = data;
        std::thread::spawn(move || {
            // Playback events carry what the item is, so a plugin can look it up
            // elsewhere (Trakt, Last.fm …) without asking the server.
            if event.starts_with("playback.") && data.get("item").is_none() {
                if let Some(id) = data["id"].as_u64() {
                    let item = app.item_brief(id);
                    let dur = item["duration_ms"].as_u64().unwrap_or(0);
                    if let (Some(t), true) = (data["time"].as_u64(), dur > 0) {
                        data["progress"] = json!(((t as f64 / dur as f64) * 1000.0).round().min(1000.0) / 10.0);
                    }
                    data["item"] = item;
                }
            }
            let server = app.server_info();
            for (pl, settings) in listeners {
                let r = plugins::invoke(&pl, &settings, &event, &data, &server);
                let mut p = app.plugins.lock().unwrap();
                match r {
                    Ok(v) => p.note(&pl.manifest.id, &event, true, short(&v.to_string())),
                    Err(e) => p.note(&pl.manifest.id, &event, false, e),
                }
            }
        });
    }

    /// Asks metadata plugins about new items and merges their answers.
    pub fn lookup_metadata(self: &Arc<Self>, ids: &[u64]) {
        let listeners = self.listeners("metadata.lookup");
        if listeners.is_empty() {
            return;
        }
        let server = self.server_info();
        for id in ids {
            let data = {
                let db = self.db.lock().unwrap();
                let Some(it) = db.items.get(id) else { continue };
                let mut v = serde_json::to_value(it).unwrap_or(Value::Null);
                if let Some(parent) = it.parent_id.and_then(|p| db.items.get(&p)) {
                    v["parent_title"] = json!(parent.title);
                    if let Some(gp) = parent.parent_id.and_then(|g| db.items.get(&g)) {
                        v["show_title"] = json!(gp.title);
                    }
                }
                v
            };
            for (pl, settings) in &listeners {
                match plugins::invoke(pl, settings, "metadata.lookup", &data, &server) {
                    Ok(answer) => {
                        if let Some(meta) = answer.get("metadata") {
                            let mut db = self.db.lock().unwrap();
                            if let Some(it) = db.items.get_mut(id) {
                                apply_metadata(it, meta, false);
                            }
                        }
                        self.plugins.lock().unwrap().note(&pl.manifest.id, "metadata.lookup", true, format!("item {id}"));
                    }
                    Err(e) => self.plugins.lock().unwrap().note(&pl.manifest.id, "metadata.lookup", false, e),
                }
            }
        }
    }

    /// Scans a library now, on this thread. None if it is already scanning.
    pub fn scan_now(self: &Arc<Self>, section_id: u64) -> Option<scanner::ScanReport> {
        self.scan_folder_now(section_id, None)
    }

    /// Scans a library, or with `only` just one folder of it. None if it is already scanning.
    pub fn scan_folder_now(self: &Arc<Self>, section_id: u64, only: Option<PathBuf>) -> Option<scanner::ScanReport> {
        {
            let mut s = self.scanning.lock().unwrap();
            if !s.insert(section_id) {
                return None;
            }
        }
        let section = self.db.lock().unwrap().sections.iter().find(|s| s.id == section_id).cloned();
        let report = section.map(|section| {
            let ffprobe = self.settings().ffprobe;
            let report = scanner::scan_section_in(&section, &ffprobe, only.as_deref(), |f| {
                let mut db = self.db.lock().unwrap();
                f(&mut db);
            });
            self.lookup_metadata(&report.added);
            self.queue_online_details(&report.added, false);
            self.save();
            for id in &report.added {
                let title = self.db.lock().unwrap().items.get(id).map(|i| i.title.clone()).unwrap_or_default();
                self.fire("library.item.added", json!({"id": id, "title": title, "section_id": section_id}));
            }
            for id in &report.removed {
                self.fire("library.item.removed", json!({"id": id, "section_id": section_id}));
            }
            self.fire(
                "library.scan.finished",
                json!({"section_id": section_id, "added": report.added.len(), "updated": report.updated.len(), "removed": report.removed.len(), "files": report.files_seen}),
            );
            report
        });
        self.scanning.lock().unwrap().remove(&section_id);
        report
    }

    pub fn scan_in_background(self: &Arc<Self>, section_id: u64) {
        self.scan_folder_in_background(section_id, None);
    }

    pub fn scan_folder_in_background(self: &Arc<Self>, section_id: u64, only: Option<PathBuf>) {
        let app = self.clone();
        std::thread::spawn(move || {
            app.scan_folder_now(section_id, only);
        });
    }

    /// An id tag ("{imdb-tt0068646}", "{tmdb-238}", "{tvdb-81189}") in the names
    /// below the library folder: for a movie its file name and folders, for a show
    /// its own folder (an episode's file name may carry the episode's id instead).
    pub fn id_tag_of(db: &Db, it: &Item) -> Option<(&'static str, String)> {
        let path = match it.kind {
            ItemKind::Movie => it.file.as_ref()?.path.clone(),
            ItemKind::Show => db.leaves(it.id).iter().filter_map(|l| db.items.get(l)?.file.as_ref().map(|f| f.path.clone())).next()?,
            _ => return None,
        };
        let path = Path::new(&path);
        let section = db.sections.iter().find(|s| s.id == it.section_id)?;
        let rel = section.paths.iter().find_map(|r| path.strip_prefix(r).ok()).unwrap_or(path);
        let names: Vec<String> = rel.components().map(|c| c.as_os_str().to_string_lossy().to_string()).collect();
        let names: Vec<&String> = match it.kind {
            ItemKind::Show if names.len() >= 2 => vec![&names[0]],
            ItemKind::Show => vec![],
            _ => names.iter().rev().collect(),
        };
        // Without a tag in the names, the match Plex made (kept by the import) decides.
        names.into_iter().find_map(|n| scanner::id_tag(n)).or_else(|| {
            let p = it.extra.get("plex_id")?;
            let src = match p["source"].as_str()? { "imdb" => "imdb", "tmdb" => "tmdb", "tvdb" => "tvdb", _ => return None };
            Some((src, p["id"].as_str()?.to_string()))
        })
    }

    /// Title and year from the folder a movie's file is in, when that folder holds
    /// only this movie (not the library folder itself, not a folder of many movies).
    pub fn folder_title(db: &Db, it: &Item) -> Option<(String, Option<u32>)> {
        if it.kind != ItemKind::Movie {
            return None;
        }
        let path = Path::new(&it.file.as_ref()?.path);
        let section = db.sections.iter().find(|s| s.id == it.section_id)?;
        let is_root = |d: &Path| section.paths.iter().any(|r| scanner_same_dir(Path::new(r), d));
        let mut dir = path.parent()?;
        if is_root(dir) {
            return None;
        }
        // Parts of the same movie ("… - pt1", "… - pt2") do not count as other movies.
        let base = |p: &Path| p.file_stem().map(|s| scanner::split_part(&s.to_string_lossy()).0.to_lowercase()).unwrap_or_default();
        let mine = base(path);
        let others = db.items.values().filter(|o| o.id != it.id && o.kind == ItemKind::Movie)
            .filter_map(|o| o.file.as_ref().map(|f| Path::new(&f.path)))
            .filter(|p| p.parent() == Some(dir) && base(p) != mine).count();
        if others > 0 {
            return None;
        }
        // A folder that says nothing about the movie ("CD1", "Disc 2", or named like the
        // file itself: "Der Pate (1972)/funxd/funxd.mkv"): its parent's name is the title.
        let file_title = scanner::parse_title_year(&mine).0.to_lowercase();
        for _ in 0..2 {
            let name = dir.file_name()?.to_string_lossy().to_string();
            let (t, _) = scanner::parse_title_year(&name);
            let generic = t.trim().is_empty() || t.to_lowercase() == file_title || is_disc_folder(&name);
            match dir.parent() {
                Some(up) if generic && !is_root(up) && up.file_name().is_some() => dir = up,
                _ => break,
            }
        }
        let (title, year) = scanner::parse_title_year(&dir.file_name()?.to_string_lossy());
        Some((title, year)).filter(|(t, _)| !t.trim().is_empty() && t.to_lowercase() != file_title)
    }

    /// The movie, show or other video an item belongs to (itself for movies and shows).
    pub fn top_level(db: &Db, id: u64) -> Option<u64> {
        let mut cur = db.items.get(&id)?;
        while let Some(p) = cur.parent_id.and_then(|p| db.items.get(&p)) {
            cur = p;
        }
        Some(cur.id)
    }

    /// Puts movies and shows (found from any of their items) in line for online details.
    pub fn queue_online_details(&self, ids: &[u64], force: bool) {
        let tops: Vec<u64> = {
            let db = self.db.lock().unwrap();
            if !db.settings.metadata_enabled && !force {
                return;
            }
            let mut t: Vec<u64> = ids.iter().filter_map(|i| Self::top_level(&db, *i)).collect();
            t.sort();
            t.dedup();
            t.retain(|i| db.items.get(i).is_some_and(|it| matches!(it.kind, ItemKind::Movie | ItemKind::Show)));
            t
        };
        let mut q = self.meta_queue.lock().unwrap();
        for id in tops {
            if !q.iter().any(|(i, _)| *i == id) {
                q.push_back((id, force));
            }
        }
    }

    /// "Detect credits": looks for end credits in the given items' files (on a
    /// background thread, shown as a job in the web app): chapters named like
    /// "Credits" first, else the picture of the last part of the video. It skips libraries with
    /// credits detection off and, unless `force`, files that are already up to
    /// date or failed more than 3 times. Returns how many videos it will check.
    pub fn detect_credits(self: &Arc<Self>, ids: Vec<u64>, force: bool) -> Result<usize, String> {
        let mut job = self.job.lock().unwrap();
        if job.is_some() {
            return Err("Another job (credits detection or ratings) is already running; try again in a moment".into());
        }
        let (files, ffmpeg, ffprobe, picture) = {
            let db = self.db.lock().unwrap();
            if db.settings.credits_markers_behaviour == "never" {
                return Err("Credits markers are switched off in Settings (Generate credits video markers: never)".into());
            }
            let mut leaves = vec![];
            for id in ids {
                match db.items.get(&id) {
                    Some(i) if i.kind.playable() => leaves.push(id),
                    Some(_) => leaves.extend(db.leaves(id)),
                    None => {}
                }
            }
            let files: Vec<(u64, String, u64)> = leaves
                .iter()
                .filter_map(|l| db.items.get(l))
                .filter(|i| db.credits_on(i))
                // A split movie's credits are in its last part.
                .filter_map(|i| i.last_file().map(|f| (i, f)))
                .filter(|(_, f)| force || (f.credits_checked < db::CREDITS_DETECTOR_VERSION && f.credits_errors <= db::CREDITS_MAX_ERRORS))
                .map(|(i, f)| (i.id, f.path.clone(), if i.parts.is_empty() { f.duration_ms.max(i.duration_ms) } else { f.duration_ms }))
                .collect();
            (files, db.settings.ffmpeg.clone(), db.settings.ffprobe.clone(), db.settings.credits_from_picture)
        };
        let total = files.len();
        if total == 0 {
            return Err(if force { "No videos to check (or credits detection is off for this library)".into() } else { "Everything is already up to date".into() });
        }
        *job = Some(("Detecting credits".into(), 0, total));
        self.job_cancel.store(false, AtomicOrdering::SeqCst);
        drop(job);
        let app = self.clone();
        std::thread::spawn(move || {
            let (mut found, mut failed) = (0, 0);
            for (n, (id, path, duration)) in files.into_iter().enumerate() {
                if app.job_cancelled() {
                    break;
                }
                let r = crate::scanner::find_credits(&ffmpeg, &ffprobe, &path, duration, picture);
                if let Some(f) = app.db.lock().unwrap().items.get_mut(&id).and_then(|i| i.last_file_mut()) {
                    match r {
                        Ok((c, how)) => {
                            found += usize::from(!c.is_empty());
                            f.credits_ms = c;
                            f.credits_source = how.into();
                            f.credits_checked = db::CREDITS_DETECTOR_VERSION;
                            f.credits_errors = 0;
                        }
                        Err(e) => {
                            failed += 1;
                            f.credits_errors += 1;
                            elog!("credits detection for item {id}: {e}");
                            if f.credits_errors > db::CREDITS_MAX_ERRORS {
                                elog!("credits detection for item {id} has failed too many times; it will not be retried automatically");
                            }
                        }
                    }
                }
                *app.job.lock().unwrap() = Some(("Detecting credits".into(), n + 1, total));
            }
            app.save();
            *app.job.lock().unwrap() = None;
            let cancelled = app.job_cancel.swap(false, AtomicOrdering::SeqCst);
            let mut p = app.meta_progress.lock().unwrap();
            p.done += 1;
            let failed_txt = if failed > 0 { format!(", {failed} could not be read") } else { String::new() };
            p.last = json!({"id": null, "title": "Credits detection", "ok": failed == 0,
                "message": format!("Credits detection {}: credits found in {found} of {total} videos{failed_txt}", if cancelled { "cancelled" } else { "finished" }), "at": now()});
        });
        Ok(total)
    }

    /// "Detect intros": compares the sound at the start of every episode of a
    /// season with the others (intro.rs),
    /// on a background thread shown as a job. `ids` may be
    /// shows, seasons or episodes. Unless `force`, seasons whose episodes are
    /// all up to date are skipped ("Already up to date"). Returns how many
    /// episodes it will listen to.
    /// Asks the running job (credits, intros, analysis, optimizing) to stop after the file it is on.
    pub fn cancel_job(&self) -> bool {
        let running = self.job.lock().unwrap().is_some();
        if running {
            self.job_cancel.store(true, AtomicOrdering::SeqCst);
        }
        // Details waiting for their turn are dropped too; the one being fetched right now finishes.
        let waiting = {
            let mut q = self.meta_queue.lock().unwrap();
            let n = q.len();
            q.clear();
            n > 0
        };
        running || waiting
    }

    pub fn job_cancelled(&self) -> bool {
        self.job_cancel.load(AtomicOrdering::SeqCst)
    }

    pub fn detect_intros(self: &Arc<Self>, ids: Vec<u64>, force: bool) -> Result<usize, String> {
        use crate::intro;
        let mut job = self.job.lock().unwrap();
        if job.is_some() {
            return Err("Another job (credits or intro detection, or ratings) is already running; try again in a moment".into());
        }
        type Episode = (u64, String, u64, u64);
        let (seasons, ffmpeg): (Vec<Vec<Episode>>, String) = {
            let db = self.db.lock().unwrap();
            if db.settings.intro_markers_behaviour == "never" {
                return Err("Intro markers are switched off in Settings (Generate intro video markers: never)".into());
            }
            let mut season_ids: Vec<u64> = vec![];
            for id in ids {
                let Some(it) = db.items.get(&id) else { continue };
                match it.kind {
                    ItemKind::Show => season_ids.extend(db.children(id).iter().filter(|c| c.kind == ItemKind::Season).map(|c| c.id)),
                    ItemKind::Season => season_ids.push(id),
                    ItemKind::Episode => season_ids.extend(it.parent_id),
                    _ => {}
                }
            }
            let mut seen = std::collections::HashSet::new();
            season_ids.retain(|s| seen.insert(*s));
            let mut seasons = vec![];
            for sid in season_ids {
                let eps: Vec<&Item> = db.children(sid).into_iter().filter(|e| e.kind == ItemKind::Episode && e.file.is_some() && db.intro_on(e)).collect();
                // One episode has nothing to be compared with.
                if eps.len() < 2 {
                    continue;
                }
                let stale = eps.iter().any(|e| e.file.as_ref().is_some_and(|f| intro::is_due(f.intro_checked, !f.intro_ms.is_empty()) && f.intro_errors <= db::CREDITS_MAX_ERRORS));
                if !force && !stale {
                    continue;
                }
                seasons.push(eps.iter().filter_map(|e| e.file.as_ref().map(|f| (e.id, f.path.clone(), f.size, f.modified))).collect());
            }
            (seasons, db.settings.ffmpeg.clone())
        };
        let total: usize = seasons.iter().map(|s| s.len()).sum();
        if total == 0 {
            return Err(if force { "No season with at least two episodes to compare (or intro detection is off for this library)".into() } else { "Everything is already up to date".into() });
        }
        *job = Some(("Detecting intros".into(), 0, total));
        self.job_cancel.store(false, AtomicOrdering::SeqCst);
        drop(job);
        let app = self.clone();
        let cache = self.data_dir.join("cache").join("intro");
        std::thread::spawn(move || {
            let (mut done, mut found, mut failed) = (0, 0, 0);
            'seasons: for eps in seasons {
                let (mut prints, mut errors) = (vec![], vec![]);
                for (_, path, size, modified) in &eps {
                    // A season is only judged when all its episodes were listened to.
                    if app.job_cancelled() {
                        break 'seasons;
                    }
                    let file = cache.join(intro::cache_name(path, *size, *modified));
                    let print = match intro::read_cached(&file) {
                        Some(p) => Ok(p),
                        None => intro::fingerprint_file(&ffmpeg, path).inspect(|p| intro::write_cached(&file, p)),
                    };
                    match print {
                        Ok(p) => {
                            prints.push(p);
                            errors.push(None);
                        }
                        Err(e) => {
                            prints.push(vec![]);
                            errors.push(Some(e));
                        }
                    }
                    done += 1;
                    *app.job.lock().unwrap() = Some(("Detecting intros".into(), done, total));
                }
                let intros = intro::detect_season(&prints);
                let mut db = app.db.lock().unwrap();
                for (((id, ..), found_intro), error) in eps.iter().zip(intros).zip(errors) {
                    let Some(f) = db.items.get_mut(id).and_then(|i| i.file.as_mut()) else { continue };
                    match error {
                        None => {
                            found += usize::from(found_intro.is_some());
                            f.intro_ms = found_intro.map(|(s, e)| vec![s, e]).unwrap_or_default();
                            f.intro_checked = intro::DETECTOR_VERSION;
                            f.intro_errors = 0;
                        }
                        Some(e) => {
                            failed += 1;
                            f.intro_errors += 1;
                            f.intro_ms.clear();
                            elog!("intro detection for item {id}: {e}");
                        }
                    }
                }
            }
            app.save();
            *app.job.lock().unwrap() = None;
            let cancelled = app.job_cancel.swap(false, AtomicOrdering::SeqCst);
            let mut p = app.meta_progress.lock().unwrap();
            p.done += 1;
            let failed_txt = if failed > 0 { format!(", {failed} could not be read") } else { String::new() };
            p.last = json!({"id": null, "title": "Intro detection", "ok": failed == 0,
                "message": format!("Intro detection {}: intros found in {found} of {total} episodes{failed_txt}", if cancelled { "cancelled" } else { "finished" }), "at": now()});
        });
        Ok(total)
    }

    /// Seasons still waiting for automatic intro detection (with at least two
    /// episodes, so there is something to compare): first those with an intro
    /// from an older detector, then those with an episode never looked at,
    /// newest first.
    pub fn intros_pending(&self) -> Vec<u64> {
        let db = self.db.lock().unwrap();
        if db.settings.intro_markers_behaviour == "never" {
            return vec![];
        }
        let mut due: Vec<(bool, u64, u64)> = db
            .items
            .values()
            .filter(|i| i.kind == ItemKind::Episode && db.intro_on(i))
            .filter_map(|i| {
                let f = i.file.as_ref()?;
                let wanted = crate::intro::is_due(f.intro_checked, !f.intro_ms.is_empty()) && f.intro_errors <= db::CREDITS_MAX_ERRORS;
                wanted.then_some((f.intro_checked == 0, i.added_at, i.parent_id?))
            })
            .collect();
        due.sort_by_key(|&(new, added, _)| (new, std::cmp::Reverse(added)));
        let mut seen = std::collections::HashSet::new();
        let mut v: Vec<u64> = due.into_iter().map(|d| d.2).filter(|s| seen.insert(*s)).collect();
        v.retain(|s| db.children(*s).iter().filter(|e| e.kind == ItemKind::Episode && e.file.is_some()).count() >= 2);
        v
    }

    /// Videos still waiting for automatic credits detection.
    pub fn credits_pending(&self) -> Vec<u64> {
        let db = self.db.lock().unwrap();
        if db.settings.credits_markers_behaviour == "never" {
            return vec![];
        }
        db.items
            .values()
            .filter(|i| db.credits_on(i))
            .filter(|i| i.last_file().is_some_and(|f| f.credits_checked < db::CREDITS_DETECTOR_VERSION && f.credits_errors <= db::CREDITS_MAX_ERRORS))
            .map(|i| i.id)
            .collect()
    }

    /// IMDb ratings from IMDb's own ratings file, for every movie and show with
    /// an IMDb id. The file is downloaded again when it is a week old.
    pub fn update_imdb_ratings(self: &Arc<Self>, force: bool) -> Result<String, String> {
        use crate::metadata;
        let file = self.data_dir.join("cache").join("imdb-ratings.tsv.gz");
        let age = std::fs::metadata(&file).and_then(|m| m.modified()).ok().and_then(|t| t.elapsed().ok());
        let stale = age.is_none_or(|a| a > Duration::from_secs(7 * 86400));
        let file_stamp = |f: &Path| std::fs::metadata(f).and_then(|m| m.modified()).ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map(|d| d.as_secs()).unwrap_or(0);
        let wanted: HashMap<String, Vec<u64>> = {
            let db = self.db.lock().unwrap();
            if !db.settings.imdb_ratings_file && !force {
                return Ok("IMDb ratings file is switched off".into());
            }
            let stamp = file_stamp(&file);
            let mut w: HashMap<String, Vec<u64>> = HashMap::new();
            for it in db.items.values().filter(|i| matches!(i.kind, ItemKind::Movie | ItemKind::Show)) {
                let Some(imdb) = it.extra.get("ids").and_then(|i| i["imdb"].as_str()).filter(|s| s.starts_with("tt")) else { continue };
                let done = it.extra.get("imdb_ratings_checked").and_then(|v| v.as_u64()) == Some(stamp);
                if force || stale || !done {
                    w.entry(imdb.to_string()).or_default().push(it.id);
                }
            }
            w
        };
        if wanted.is_empty() {
            return Ok("IMDb ratings are up to date".into());
        }
        {
            let mut job = self.job.lock().unwrap();
            if job.is_some() {
                return Err("Another job is running".into());
            }
            *job = Some(("Updating IMDb ratings".into(), 0, wanted.len()));
        }
        let result = (|| {
            if stale || force {
                if let Err(e) = metadata::download_imdb_ratings(&file) {
                    if !file.exists() {
                        return Err(e);
                    }
                }
            }
            let ids: std::collections::HashSet<String> = wanted.keys().cloned().collect();
            let found = metadata::read_imdb_ratings(&file, &ids)?;
            let stamp = file_stamp(&file);
            let mut db = self.db.lock().unwrap();
            for (imdb, items) in &wanted {
                for id in items {
                    let Some(it) = db.items.get_mut(id) else { continue };
                    if let Some((r, n)) = found.get(imdb) {
                        let mut ratings = it.extra.get("ratings").cloned().filter(|v| v.is_object()).unwrap_or(json!({}));
                        ratings["imdb"] = metadata::imdb_score(*r, *n, imdb);
                        it.extra.insert("ratings".into(), ratings);
                    }
                    it.extra.insert("imdb_ratings_checked".into(), json!(stamp));
                }
            }
            Ok(format!("IMDb ratings updated for {} of {} titles", found.len(), wanted.len()))
        })();
        self.save();
        *self.job.lock().unwrap() = None;
        let mut p = self.meta_progress.lock().unwrap();
        p.done += 1;
        p.last = json!({"id": null, "title": "IMDb ratings", "ok": result.is_ok(), "message": match &result { Ok(m) | Err(m) => m.clone() }, "at": now()});
        result
    }
    /// Queues every matched movie and show in the libraries `which` picks for a
    /// fresh download (after a language change). Returns how many were queued.
    pub fn refetch_details(&self, which: impl Fn(&Section) -> bool) -> usize {
        let ids: Vec<u64> = {
            let db = self.db.lock().unwrap();
            let secs: Vec<u64> = db.sections.iter().filter(|s| which(s)).map(|s| s.id).collect();
            db.items
                .values()
                .filter(|i| secs.contains(&i.section_id) && matches!(i.kind, ItemKind::Movie | ItemKind::Show))
                .filter(|i| i.extra.get("metadata_status").and_then(|v| v.as_str()) == Some("matched"))
                .map(|i| i.id)
                .collect()
        };
        self.queue_online_details(&ids, true);
        ids.len()
    }

    pub fn metadata_dir(&self, id: u64) -> PathBuf {
        self.data_dir.join("metadata").join(id.to_string())
    }

    /// Finds and applies online details for a movie or show, its seasons and
    /// episodes. `choice` = (source, id) from "Fix match"; otherwise the stored
    /// match is refreshed, or the best confident search result is taken.
    pub fn match_online(self: &Arc<Self>, id: u64, choice: Option<(String, String)>, force: bool) -> Result<String, String> {
        use crate::metadata;
        let mut settings = self.settings();
        let (kind, title, year, stored, seasons, status_recent, folder, tag, unmatched_by_hand) = {
            let db = self.db.lock().unwrap();
            let it = db.items.get(&id).ok_or("No such item")?;
            // The library's own language, else the server's.
            settings.metadata_language = db.language_for_section(it.section_id);
            // A language chosen for this title wins.
            if let Some(l) = it.extra.get("language_override").and_then(|v| v.as_str()).filter(|l| db::LANGUAGES.iter().any(|x| x.0 == *l)) {
                settings.metadata_language = l.to_string();
            }
            if !matches!(it.kind, ItemKind::Movie | ItemKind::Show) {
                return Err("Details are looked up for movies and shows".into());
            }
            let s = |k: &str| it.extra.get(k).and_then(|v| v.as_str()).map(String::from);
            let stored = s("metadata_source").zip(s("metadata_id"));
            let seasons: Vec<u32> = db.children(id).iter().filter_map(|c| c.index).collect();
            // A miss from before the folder name was tried counts as not checked yet.
            let recent = s("metadata_status").as_deref() == Some("unmatched")
                && it.extra.get("match_version").and_then(|v| v.as_u64()) == Some(MATCH_VERSION)
                && it.extra.get("metadata_checked_at").and_then(|v| v.as_u64()).is_some_and(|t| now().saturating_sub(t) < 7 * 86400);
            // Use the name from the file, not a name a previous wrong match gave it.
            let title = s("file_title").unwrap_or_else(|| it.title.clone());
            let folder = Self::folder_title(&db, it).filter(|(t, _)| !t.eq_ignore_ascii_case(&title));
            let tag = Self::id_tag_of(&db, it);
            let by_hand = it.extra.get("unmatched_by_hand").and_then(|v| v.as_bool()) == Some(true);
            (it.kind, title, it.year, stored, seasons, recent, folder, tag, by_hand)
        };
        // An id in the file or folder name ("{imdb-tt0068646}") decides the match,
        // unless one is picked by hand in "Fix match".
        let tagged = match (&choice, &tag) {
            (None, Some((src, tid))) => match metadata::resolve_id(&settings, kind, src, tid) {
                Ok(x) => Some(x),
                Err(e) => {
                    elog!("details for item {id}: {{{src}-{tid}}}: {e}");
                    None
                }
            },
            _ => None,
        };
        if choice.is_none() && unmatched_by_hand {
            // Only "Refresh details" asked for by hand looks for a match again.
            if !force { return Err("The match was removed by hand; use Match… to pick one".into()); }
            if let Some(it) = self.db.lock().unwrap().items.get_mut(&id) { it.extra.remove("unmatched_by_hand"); }
        }
        if choice.is_none() && tagged.is_none() && stored.is_none() && status_recent && !force {
            return Err("No match was found recently; use Fix match".into());
        }
        // A match made before a TMDB key was entered (Wikidata, TVmaze) is moved to TMDB when it finds the
        // title for sure: those matches often have no poster, and a refresh would keep them for ever.
        let stored = match stored {
            Some((ref src, _)) if choice.is_none() && tagged.is_none() && src != "tmdb" && !settings.tmdb_api_key.trim().is_empty() => {
                let better = metadata::search(&settings, kind, &title, year).ok()
                    .and_then(|c| c.into_iter().next()).filter(|c| c.source == "tmdb" && metadata::acceptable(c));
                better.map(|c| (c.source, c.id)).or(stored)
            }
            other => other,
        };
        let (source, mid) = match choice.or(tagged).or(stored) {
            Some(x) => x,
            None => {
                // Files are sometimes named after the release ("funxd.mkv"): then the
                // name of the movie's own folder ("Der Pate (1972)") is tried too, and
                // first when only the folder has a year.
                let pick = |r: &Result<Vec<metadata::Candidate>, String>| r.as_ref().ok().and_then(|c| c.first().cloned()).filter(metadata::acceptable);
                let folder_first = year.is_none() && folder.as_ref().is_some_and(|f| f.1.is_some());
                let by_folder = || folder.as_ref().map(|(ft, fy)| metadata::search(&settings, kind, ft, fy.or(year)));
                let mut found = None;
                let mut best = None;
                if folder_first {
                    best = by_folder().as_ref().and_then(pick);
                }
                if best.is_none() {
                    let r = metadata::search(&settings, kind, &title, year);
                    best = pick(&r);
                    found = Some(r);
                }
                if best.is_none() && !folder_first {
                    best = by_folder().as_ref().and_then(pick);
                }
                let found = found.unwrap_or_else(|| Ok(vec![]));
                let tried = match &folder {
                    Some((ft, fy)) => format!("\"{title}\" or \"{ft}{}\"", fy.map(|y| format!(" ({y})")).unwrap_or_default()),
                    None => format!("\"{title}\""),
                };
                match best {
                    Some(b) => (b.source, b.id),
                    None => {
                        let mut db = self.db.lock().unwrap();
                        if let Some(it) = db.items.get_mut(&id) {
                            it.extra.insert("metadata_status".into(), json!("unmatched"));
                            it.extra.insert("metadata_checked_at".into(), json!(now()));
                            it.extra.insert("match_version".into(), json!(MATCH_VERSION));
                        }
                        return Err(match found {
                            Err(e) => e,
                            Ok(c) if c.is_empty() => format!("Nothing found online for {tried}"),
                            Ok(c) => format!("No sure match for {tried} (closest: {} {})", c[0].title, c[0].year.map(|y| format!("({y})")).unwrap_or_default()),
                        });
                    }
                }
            }
        };
        let mut d = metadata::fetch(&settings, kind, &source, &mid, &seasons)?;
        // What the settings leave out (Settings → Metadata).
        if !settings.meta_cast {
            for k in ["cast", "directors", "writers", "producers", "creators"] { d.meta["extra"][k] = json!([]); }
        }
        if !settings.meta_artwork {
            d.poster_url = None;
            d.art_url = None;
            for s in d.seasons.iter_mut() { s.poster_url = None; }
            for e in d.episodes.iter_mut() { e.still_url = None; }
        }
        if !settings.meta_episodes {
            d.seasons.clear();
            d.episodes.clear();
        }
        if !settings.meta_ratings {
            if let Some(o) = d.meta.as_object_mut() { o.remove("rating"); o.remove("audienceRating"); }
            if let Some(o) = d.meta["extra"].as_object_mut() { o.remove("ratings"); }
        }
        // Pictures (downloaded without holding the library lock).
        let dir = self.metadata_dir(id);
        let got = |url: &Option<String>, name: &str, dir: &Path| -> Option<String> {
            let dest = dir.join(name);
            url.as_ref().filter(|u| metadata::download(u, &dest)).map(|_| dest.to_string_lossy().to_string())
        };
        let poster = got(&d.poster_url, "poster.jpg", &dir);
        let art = got(&d.art_url, "art.jpg", &dir);
        // Actor photos are kept on this server too (cast/0.jpg …), served at
        // /library/metadata/{id}/cast/{n}, so pages don't depend on other sites.
        // Actors are shared by the whole library: one photo per person
        // (metadata/people/<key>.jpg), downloaded once, whatever they play in.
        let mut cast = d.meta["extra"]["cast"].as_array().cloned().unwrap_or_default();
        let _ = std::fs::remove_dir_all(dir.join("cast"));
        for c in cast.iter_mut().take(settings.cast_photos_limit) {
            let key = person_key(c);
            c["key"] = json!(key);
            let dest = self.person_photo(&key);
            let have = metadata::file_image_type(&dest).is_some();
            let url = c["photo"].as_str().map(String::from);
            if have || url.is_some_and(|u| metadata::download(&u, &dest)) {
                c["thumb"] = json!(format!("/library/people/{key}/thumb"));
            }
        }
        for c in cast.iter_mut().skip(settings.cast_photos_limit) {
            c["key"] = json!(person_key(c));
        }
        let (season_items, episode_items) = {
            let db = self.db.lock().unwrap();
            let seasons: Vec<(u64, u32)> = db.children(id).iter().filter_map(|c| c.index.map(|n| (c.id, n))).collect();
            let mut eps = vec![];
            for (sid, n) in &seasons {
                for e in db.children(*sid) {
                    if let Some(i) = e.index {
                        eps.push((e.id, *n, i));
                    }
                }
            }
            (seasons, eps)
        };
        let mut season_pics = HashMap::new();
        for (sid, n) in &season_items {
            if let Some(s) = d.seasons.iter().find(|s| s.index == *n) {
                if let Some(p) = got(&s.poster_url, "poster.jpg", &self.metadata_dir(*sid)) {
                    season_pics.insert(*sid, p);
                }
            }
        }
        let mut episode_pics = HashMap::new();
        for (eid, sn, en) in &episode_items {
            if let Some(e) = d.episodes.iter().find(|e| e.season == *sn && e.episode == *en) {
                let dest = self.metadata_dir(*eid).join("still.jpg");
                if dest.exists() {
                    episode_pics.insert(*eid, dest.to_string_lossy().to_string());
                } else if let Some(p) = got(&e.still_url, "still.jpg", &self.metadata_dir(*eid)) {
                    episode_pics.insert(*eid, p);
                }
            }
        }
        let mut db = self.db.lock().unwrap();
        let Some(it) = db.items.get_mut(&id) else { return Err("The item went away".into()) };
        // People brought over from the old server stay when the online source has none.
        let kept: Vec<(&str, Value)> = if it.extra.contains_key("imported_from_old_server") {
            ["cast", "directors", "writers", "producers", "countries", "collections"].into_iter().filter(|k| d.meta["extra"][*k].as_array().is_none_or(|a| a.is_empty())).filter_map(|k| it.extra.get(k).cloned().map(|v| (k, v))).collect()
        } else {
            vec![]
        };
        // Forget what an earlier match said before applying this one.
        for k in ["cast", "directors", "writers", "producers", "countries", "creators", "ids", "art", "original_title", "runtime_minutes", "originally_available", "metadata_status", "metadata_checked_at", "ratings", "imdb_ratings_checked", "unmatched_by_hand", "trailers"] {
            if it.locked.iter().any(|l| l == k) { continue; }
            it.extra.remove(k);
        }
        if !it.extra.contains_key("file_title") {
            it.extra.insert("file_title".into(), json!(it.title));
        }
        let mut meta = d.meta.clone();
        if !cast.is_empty() {
            meta["extra"]["cast"] = json!(cast);
        }
        for (k, v) in kept {
            meta["extra"][k] = v;
        }
        if let Some(p) = &poster {
            meta["poster"] = json!(p);
        }
        meta["extra"]["metadata_status"] = json!("matched");
        meta["extra"]["pictures_version"] = json!(PICTURES_VERSION);
        meta["extra"]["matched_at"] = json!(now());
        if let Some(a) = &art {
            meta["extra"]["art"] = json!(a);
        }
        apply_metadata(it, &meta, false);
        use_original_title(it);
        let new_title = it.title.clone();
        let _ = std::fs::remove_file(self.data_dir.join("cache").join("thumbs").join(format!("{id}.jpg")));
        if art.is_some() {
            let _ = std::fs::remove_file(self.data_dir.join("cache").join("art").join(format!("{id}.jpg")));
        }
        let mut n_eps = 0;
        for (sid, n) in &season_items {
            if let (Some(s), Some(item)) = (d.seasons.iter().find(|s| s.index == *n), db.items.get_mut(sid)) {
                let mut m = json!({"summary": s.summary});
                if let Some(p) = season_pics.get(sid) {
                    m["poster"] = json!(p);
                }
                apply_metadata(item, &m, false);
            }
        }
        for (eid, sn, en) in &episode_items {
            let Some(e) = d.episodes.iter().find(|e| e.season == *sn && e.episode == *en) else { continue };
            let Some(item) = db.items.get_mut(eid) else { continue };
            if !item.extra.contains_key("file_title") {
                item.extra.insert("file_title".into(), json!(item.title));
            }
            let mut m = json!({"summary": e.summary, "extra": {"originally_available": e.aired}});
            if !e.title.is_empty() {
                m["title"] = json!(e.title);
            }
            if let Some(r) = e.rating {
                m["rating"] = json!(r);
            }
            if let Some(p) = episode_pics.get(eid) {
                m["poster"] = json!(p);
            }
            apply_metadata(item, &m, false);
            n_eps += 1;
        }
        drop(db);
        Ok(if kind == ItemKind::Show {
            format!("{new_title}: details from {source}, {n_eps} episodes")
        } else {
            format!("{new_title}: details from {source}")
        })
    }

    /// "Unmatch": forgets the online details of a movie or show (and of its
    /// seasons and episodes) and goes back to what the files say. Fields edited
    /// by hand stay. Automatic matching leaves it alone until "Match…" is used.
    pub fn unmatch(&self, id: u64) -> Result<String, String> {
        let mut db = self.db.lock().unwrap();
        let it = db.items.get(&id).ok_or("No such item")?;
        if !matches!(it.kind, ItemKind::Movie | ItemKind::Show) {
            return Err("Details are looked up for movies and shows".into());
        }
        let kind = it.kind;
        // Title and year as a scan of the files gives them.
        let leaf = if kind == ItemKind::Movie { Some(id) } else { db.leaves(id).first().copied() };
        let file = leaf.and_then(|l| db.items.get(&l)).and_then(|l| l.file.as_ref()).map(|f| PathBuf::from(&f.path));
        let section = db.sections.iter().find(|s| s.id == it.section_id).cloned();
        let root = match (&file, &section) {
            (Some(f), Some(sec)) => sec.paths.iter().map(PathBuf::from).filter(|r| f.starts_with(r)).max_by_key(|r| r.as_os_str().len()),
            _ => None,
        };
        let parsed = match (&file, &section, &root) {
            (Some(f), Some(sec), Some(r)) => Some(scanner::classify(sec, r, f)),
            _ => None,
        };
        let (scan_title, year) = match parsed {
            Some(p) if kind == ItemKind::Show => p.show.unwrap_or((String::new(), None)),
            Some(p) => (p.title, p.year),
            None => (String::new(), it.year),
        };
        let title = it.extra.get("file_title").and_then(|v| v.as_str()).map(String::from).filter(|t| !t.trim().is_empty())
            .or(Some(scan_title).filter(|t| !t.trim().is_empty()))
            .unwrap_or_else(|| it.title.clone());
        let local_poster = |db: &Db, i: u64| -> Option<String> {
            let f = PathBuf::from(&db.items.get(&i)?.file.as_ref()?.path);
            scanner::find_poster(f.parent()?, &f.file_stem()?.to_string_lossy())
        };
        let own_poster = if kind == ItemKind::Movie {
            local_poster(&db, id)
        } else {
            // A show's picture lies in the show's own folder.
            file.as_ref().zip(root.as_ref()).and_then(|(f, r)| {
                let first = f.strip_prefix(r).ok()?.components().next()?;
                scanner::find_poster(&r.join(first.as_os_str()), "__none__")
            })
        };
        let mut ids = vec![id];
        for s in db.children(id).iter().map(|c| c.id).collect::<Vec<_>>() {
            ids.push(s);
            ids.extend(db.children(s).iter().map(|e| e.id));
        }
        let posters: HashMap<u64, Option<String>> =
            ids.iter().map(|i| (*i, if *i == id { own_poster.clone() } else { local_poster(&db, *i) })).collect();
        for i in &ids {
            let dir = self.metadata_dir(*i);
            let Some(it) = db.items.get_mut(i) else { continue };
            let locked = it.locked.clone();
            let can = |f: &str| !locked.iter().any(|l| l == f);
            if *i == id {
                if can("title") {
                    it.title = title.clone();
                    it.sort_title = db::sort_title(&title);
                }
                if can("year") {
                    it.year = year;
                }
                if can("tagline") { it.tagline.clear(); }
                if can("content_rating") { it.content_rating.clear(); }
                if can("studio") { it.studio.clear(); }
                if can("genres") { it.genres.clear(); }
                for k in ["collections", "cast", "directors", "writers", "producers", "countries", "creators", "ids", "art", "original_title", "runtime_minutes",
                    "originally_available", "ratings", "imdb_ratings_checked", "trailers", "metadata_source", "metadata_id", "metadata_language", "matched_at",
                    "pictures_version", "metadata_checked_at", "match_version"] {
                    if it.locked.iter().any(|l| l == k) { continue; }
                    it.extra.remove(k);
                }
                it.extra.insert("metadata_status".into(), json!("unmatched"));
                it.extra.insert("unmatched_by_hand".into(), json!(true));
            } else if it.kind == ItemKind::Episode {
                if can("title") {
                    if let Some(t) = it.extra.get("file_title").and_then(|v| v.as_str()).map(String::from) {
                        it.sort_title = db::sort_title(&t);
                        it.title = t;
                    }
                }
                it.extra.remove("originally_available");
            }
            if can("summary") { it.summary.clear(); }
            if can("rating") { it.rating = None; }
            it.updated_at = now();
            // Pictures that came with the match go; ones next to the files come back.
            if it.poster.as_deref().is_none_or(|p| Path::new(p).starts_with(&dir)) {
                it.poster = posters.get(i).cloned().flatten();
            }
            let _ = std::fs::remove_dir_all(&dir);
            let _ = std::fs::remove_file(self.data_dir.join("cache").join("thumbs").join(format!("{i}.jpg")));
            let _ = std::fs::remove_file(self.data_dir.join("cache").join("art").join(format!("{i}.jpg")));
        }
        drop(db);
        Ok(format!("{title}: match removed"))
    }

    /// Works through the queue, one item at a time, gently.
    pub fn metadata_worker(self: &Arc<Self>) {
        loop {
            let next = self.meta_queue.lock().unwrap().pop_front();
            let Some((id, force)) = next else {
                std::thread::sleep(Duration::from_secs(2));
                continue;
            };
            if !self.settings().metadata_enabled && !force {
                continue;
            }
            self.meta_busy.lock().unwrap().push(id);
            let r = self.match_online(id, None, force);
            self.meta_busy.lock().unwrap().retain(|&b| b != id);
            {
                // What the web app shows while details are being updated.
                let title = self.db.lock().unwrap().items.get(&id).map(|i| i.title.clone()).unwrap_or_default();
                let mut p = self.meta_progress.lock().unwrap();
                p.done += 1;
                if r.is_err() {
                    p.failed += 1;
                }
                p.last = json!({"id": id, "title": title, "ok": r.is_ok(), "message": match &r { Ok(m) | Err(m) => m.clone() }, "at": now()});
            }
            if let Err(e) = &r {
                elog!("details for item {id}: {e}");
            }
            // Local information from plugins (e.g. .nfo files) wins over online details.
            let leaves = {
                let db = self.db.lock().unwrap();
                let mut v = vec![id];
                v.extend(db.children(id).iter().map(|c| c.id));
                v.extend(db.leaves(id));
                v
            };
            if r.is_ok() {
                self.lookup_metadata(&leaves);
                self.fire("library.item.updated", json!({"id": id}));
            }
            self.save();
            std::thread::sleep(Duration::from_millis(400));
        }
    }

    /// Videos being played or streamed right now (players that report progress,
    /// converted streams and HLS sessions; one person playing counts once).
    pub fn stream_count(&self) -> usize {
        let mut keys: HashSet<(u64, u64)> = HashSet::new();
        for p in self.playing.lock().unwrap().values() { keys.insert((p.user_id, p.item_id)); }
        for p in self.progressive.lock().unwrap().values() { keys.insert((p.user_id, p.item_id)); }
        for s in self.hls.lock().unwrap().values() { keys.insert((s.user_id, s.item_id)); }
        keys.len()
    }

    /// Stops what `user` is playing of `item` (from the dashboard): converted streams
    /// end at once, and the player hears `message` on its next report.
    pub fn stop_playback(&self, user: u64, item: u64, message: &str) -> usize {
        self.stops.lock().unwrap().insert((user, item), (message.trim().to_string(), Instant::now()));
        let mut n = 0;
        let ended: Vec<Playing> = {
            let mut map = self.playing.lock().unwrap();
            let keys: Vec<String> = map.iter().filter(|(_, p)| p.user_id == user && p.item_id == item).map(|(k, _)| k.clone()).collect();
            keys.iter().filter_map(|k| map.remove(k)).collect()
        };
        n += ended.len();
        self.close_sessions(ended);
        // Converted streams notice the stop themselves on their next read (see ChildStream).
        n += self.progressive.lock().unwrap().values().filter(|p| p.user_id == user && p.item_id == item).count();
        let ended: Vec<HlsSession> = {
            let mut map = self.hls.lock().unwrap();
            let ids: Vec<String> = map.values().filter(|s| s.user_id == user && s.item_id == item).map(|s| s.id.clone()).collect();
            ids.iter().filter_map(|id| map.remove(id)).collect()
        };
        n += ended.len();
        for mut s in ended {
            let _ = s.child.kill();
            let _ = s.child.wait();
            let _ = std::fs::remove_dir_all(&s.dir);
        }
        n
    }

    /// Plays that ended (stopped, or the player went away): into the statistics.
    pub fn close_sessions(&self, ended: Vec<Playing>) {
        let ended: Vec<db::PlaySession> = ended.iter().filter(|p| p.session_at > 0).map(|p| p.session()).collect();
        if ended.is_empty() { return; }
        let mut db = self.db.lock().unwrap();
        let mut any = false;
        for s in ended { any |= db.record_session(s); }
        drop(db);
        if any { self.save(); }
    }

    /// The owner's message when they stopped this playback lately (Some("") without one).
    /// People outside the home network may play only so many different videos at once.
    pub fn remote_plays_over_limit(&self, user: u64, item: u64, remote: bool) -> bool {
        let limit = self.settings().max_remote_plays as usize;
        if limit == 0 || !remote { return false; }
        let mut items: HashSet<u64> = HashSet::new();
        for p in self.playing.lock().unwrap().values().filter(|p| p.user_id == user && !p.viewer.local) { items.insert(p.item_id); }
        for p in self.progressive.lock().unwrap().values().filter(|p| p.user_id == user && !p.viewer.local) { items.insert(p.item_id); }
        for s in self.hls.lock().unwrap().values().filter(|s| s.user_id == user && !s.viewer.local) { items.insert(s.item_id); }
        items.remove(&item);
        items.len() >= limit
    }

    pub fn stopped(&self, user: u64, item: u64) -> Option<String> {
        let mut map = self.stops.lock().unwrap();
        map.retain(|_, (_, at)| at.elapsed() < STOP_HOLD);
        map.get(&(user, item)).map(|(m, _)| m.clone())
    }

    /// The player was told: it may start again right away.
    pub fn stop_seen(&self, user: u64, item: u64) -> Option<String> {
        let m = self.stopped(user, item);
        self.stops.lock().unwrap().remove(&(user, item));
        m
    }

    pub fn active_transcodes(&self) -> usize {
        self.hls.lock().unwrap().values().filter(|h| h.method == Method::Transcode).count()
            + self.progressive.lock().unwrap().values().filter(|p| p.method == "transcode").count()
    }
}

/// On first start, copies the plugins shipped with the server (switched off)
/// into the data folder, so they appear on the Plugins page.
fn install_bundled_plugins(dest: &Path) {
    // The marker lists the bundled plugins copied so far (one per line), so a
    // plugin bundled by a newer version arrives once and one deleted by hand
    // stays deleted. Older versions wrote one sentence instead: they had copied
    // the first two.
    let marker = dest.join(".bundled-installed");
    let mut done: Vec<String> = match std::fs::read_to_string(&marker) {
        Ok(t) if t.starts_with("bundled example plugins") => vec!["nfo-reader".into(), "webhook-notify".into()],
        Ok(t) => t.lines().map(|l| l.trim().to_string()).filter(|l| !l.is_empty()).collect(),
        Err(_) => vec![],
    };
    let mut sources: Vec<PathBuf> = vec![];
    if let Some(p) = crate::env("BUNDLED_PLUGINS") {
        sources.push(PathBuf::from(p));
    }
    if let Some(dir) = std::env::current_exe().ok().and_then(|e| e.parent().map(|p| p.to_path_buf())) {
        sources.push(dir.join("plugins"));
        sources.push(dir.join("..").join("..").join("plugins")); // cargo's target/debug next to the project
    }
    sources.push(PathBuf::from("/opt/lichtspiel/plugins"));
    sources.push(PathBuf::from("/opt/mediaserver/plugins"));
    let Some(src) = sources.into_iter().find(|s| s.is_dir() && s.canonicalize().ok() != dest.canonicalize().ok()) else { return };
    let Ok(rd) = std::fs::read_dir(&src) else { return };
    for e in rd.flatten() {
        let from = e.path();
        if !from.join("plugin.json").exists() {
            continue;
        }
        let name = e.file_name().to_string_lossy().to_string();
        let to = dest.join(e.file_name());
        let seen = done.contains(&name);
        if !seen { done.push(name); }
        if seen || to.exists() {
            continue;
        }
        let _ = std::fs::create_dir_all(&to);
        if let Ok(files) = std::fs::read_dir(&from) {
            for f in files.flatten().filter(|f| f.path().is_file()) {
                let _ = std::fs::copy(f.path(), to.join(f.file_name()));
            }
        }
    }
    done.sort();
    let _ = std::fs::write(marker, done.join("\n") + "\n");
}

pub fn short(s: &str) -> String {
    match s.char_indices().nth(300) {
        Some((i, _)) => format!("{}…", &s[..i]),
        None => s.to_string(),
    }
}

/// Copies fields from a plugin answer or an API edit into an item.
/// Fields edited by hand are locked; plugins don't overwrite them.
/// A short, safe name for a person: the details source's own person id when
/// there is one (tmdb-31, tvmaze-12, wd-Q2680), else one made from the name.
pub fn person_key(c: &Value) -> String {
    let id = c["person_id"].as_str().unwrap_or("");
    let ok = !id.is_empty() && !id.ends_with('-') && !id.ends_with("null") && id.len() <= 40 && id.chars().all(|ch| ch.is_ascii_alphanumeric() || ch == '-');
    if ok {
        return id.to_lowercase();
    }
    name_key(c["name"].as_str().unwrap_or(""))
}

/// A person key from a name alone (same name → same key).
pub fn name_key(name: &str) -> String {
    use sha2::{Digest, Sha256};
    let norm: String = name.to_lowercase().split_whitespace().collect::<Vec<_>>().join(" ");
    format!("n-{}", Sha256::digest(norm.as_bytes()).iter().take(8).map(|b| format!("{b:02x}")).collect::<String>())
}

/// "Use the original title" chosen for this title: the original title becomes the title.
pub fn use_original_title(it: &mut Item) {
    if it.extra.get("use_original_title").and_then(|v| v.as_str()) != Some("yes") {
        return;
    }
    if let Some(o) = it.extra.get("original_title").and_then(|v| v.as_str()).map(|s| s.trim().to_string()).filter(|s| !s.is_empty()) {
        it.sort_title = db::sort_title(&o);
        it.title = o;
    }
}

pub fn apply_metadata(it: &mut Item, meta: &Value, by_hand: bool) {
    let locked = it.locked.clone();
    let can = |f: &str| by_hand || !locked.iter().any(|l| l == f);
    let lock = |f: &str, it: &mut Item| {
        if by_hand && !it.locked.iter().any(|l| l == f) {
            it.locked.push(f.into());
        }
    };
    if let Some(v) = meta.get("title").and_then(|v| v.as_str()) {
        if can("title") && !v.trim().is_empty() {
            it.title = v.trim().into();
            it.sort_title = db::sort_title(v);
            lock("title", it);
        }
    }
    if let Some(v) = meta.get("sort_title").and_then(|v| v.as_str()) {
        if can("sort_title") {
            it.sort_title = v.to_lowercase();
            lock("sort_title", it);
        }
    }
    macro_rules! text {
        ($k:literal, $f:ident) => {
            if let Some(v) = meta.get($k).and_then(|v| v.as_str()) {
                if can($k) {
                    it.$f = v.into();
                    lock($k, it);
                }
            }
        };
    }
    text!("summary", summary);
    text!("tagline", tagline);
    text!("content_rating", content_rating);
    text!("studio", studio);
    if let Some(v) = meta.get("year") {
        if can("year") {
            it.year = v.as_u64().map(|y| y as u32).or_else(|| v.as_str().and_then(|s| s.parse().ok()));
            lock("year", it);
        }
    }
    if let Some(v) = meta.get("rating") {
        if can("rating") {
            it.rating = v.as_f64().map(|r| r as f32).or_else(|| v.as_str().and_then(|s| s.parse().ok()));
            lock("rating", it);
        }
    }
    if let Some(v) = meta.get("genres").and_then(|v| v.as_array()) {
        if can("genres") {
            it.genres = v.iter().filter_map(|g| g.as_str().map(|s| s.trim().to_string())).filter(|s| !s.is_empty()).collect();
            lock("genres", it);
        }
    }
    if let Some(v) = meta.get("poster").and_then(|v| v.as_str()) {
        if can("poster") && Path::new(v).exists() {
            it.poster = Some(v.into());
        }
    }
    if let Some(Value::Object(extra)) = meta.get("extra") {
        for (k, v) in extra {
            if !by_hand && locked.iter().any(|l| l == k) {
                continue;
            }
            it.extra.insert(k.clone(), v.clone());
        }
    }
    it.normalize();
    it.updated_at = now();
}

// ---------------------------------------------------------------- responses

pub fn header(k: &str, v: &str) -> Header {
    Header::from_bytes(k.as_bytes(), v.as_bytes()).unwrap_or_else(|_| Header::from_bytes(&b"X-Bad"[..], &b"1"[..]).unwrap())
}

pub fn base_headers() -> Vec<Header> {
    vec![
        header("Access-Control-Allow-Origin", "*"),
        header("Access-Control-Allow-Headers", "Content-Type, X-Token, Authorization, Range"),
        header("Access-Control-Allow-Methods", "GET, POST, PUT, PATCH, DELETE, OPTIONS"),
        header("Access-Control-Expose-Headers", "Content-Range, Accept-Ranges, Content-Length, X-Transcode-Method, X-Session"),
        header("X-Server", &format!("{APP_NAME}/{VERSION}")),
    ]
}

pub fn bytes(status: u16, ctype: &str, body: Vec<u8>) -> Resp {
    let len = body.len();
    let mut h = base_headers();
    h.push(header("Content-Type", ctype));
    Response::new(StatusCode(status), h, Box::new(std::io::Cursor::new(body)) as Box<dyn Read + Send>, Some(len), None)
}

pub fn js(status: u16, v: Value) -> Resp {
    // A reply that sets no cache rule of its own
    // gets "no-cache", so lists and details are never shown from a stale copy.
    let mut r = bytes(status, "application/json; charset=utf-8", v.to_string().into_bytes());
    r.add_header(header("Cache-Control", "no-cache"));
    r
}

/// The name the server shows: the
/// name set in Settings; when that is empty, the computer's description (Windows) or
/// its network name; and if none can be read, a fixed fallback.
pub fn display_name(settings: &crate::db::Settings) -> String {
    let set = settings.server_name.trim();
    if !set.is_empty() {
        return set.to_string();
    }
    computer_name().unwrap_or_else(|| APP_NAME.to_string())
}

fn computer_name() -> Option<String> {
    #[cfg(windows)]
    {
        let desc = std::process::Command::new("reg")
            .args(["query", r"HKLM\SYSTEM\CurrentControlSet\services\LanmanServer\Parameters", "/v", "srvcomment"])
            .output()
            .ok()
            .and_then(|o| String::from_utf8(o.stdout).ok())
            .and_then(|s| s.lines().find(|l| l.contains("srvcomment")).and_then(|l| l.split("REG_SZ").nth(1)).map(|v| v.trim().to_string()))
            .filter(|v| !v.is_empty());
        if desc.is_some() {
            return desc;
        }
    }
    std::env::var("COMPUTERNAME")
        .or_else(|_| std::env::var("HOSTNAME"))
        .ok()
        .or_else(|| std::fs::read_to_string("/etc/hostname").ok())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

pub fn ok(v: Value) -> Resp {
    js(200, v)
}

pub fn fail(status: u16, msg: &str) -> Resp {
    js(status, json!({"error": msg, "status": status}))
}

/// Lists are wrapped in a MediaContainer.
pub fn container(list: Vec<Value>, total: usize, offset: usize, extra: Value) -> Value {
    let mut mc = json!({"size": list.len(), "totalSize": total, "offset": offset, "Metadata": list});
    if let Value::Object(e) = extra {
        for (k, v) in e {
            mc[k] = v;
        }
    }
    json!({"MediaContainer": mc})
}

/// `ip` is in the list of addresses and ranges ("10.0.0.2, 172.16.0.0/12 ::1").
/// Checks a list of addresses and ranges (commas or spaces between them): every entry must be well-formed.
pub fn check_network_list(list: &str) -> Result<(), String> {
    for e in list.split(|c: char| c == ',' || c.is_whitespace()).filter(|e| !e.is_empty()) {
        let (addr, bits) = match e.split_once('/') { Some((a, b)) => (a, Some(b)), None => (e, None) };
        let ip: IpAddr = addr.parse().map_err(|_| format!("'{e}' is not an address or range"))?;
        if let Some(b) = bits {
            let max = if ip.is_ipv4() { 32 } else { 128 };
            if b.parse::<u32>().map_or(true, |b| b > max) { return Err(format!("'{e}' has no valid prefix length")); }
        }
    }
    Ok(())
}

/// Checks the "no sign-in" networks: every entry must be an address or range inside a private, loopback or
/// link-local space, because people from there get admin rights without a password.
pub fn check_open_networks(list: &str) -> Result<(), String> {
    for e in list.split(|c: char| c == ',' || c.is_whitespace()).filter(|e| !e.is_empty()) {
        let (addr, bits) = match e.split_once('/') { Some((a, b)) => (a, Some(b)), None => (e, None) };
        let ip: IpAddr = addr.parse().map_err(|_| format!("'{e}' is not an address or range"))?;
        let bits = match bits { Some(b) => Some(b.parse::<u32>().map_err(|_| format!("'{e}' has no valid prefix length"))?), None => None };
        let private = match ip {
            IpAddr::V4(a) => {
                if bits.is_some_and(|b| b > 32) { return Err(format!("'{e}' has no valid prefix length")); }
                let b = bits.unwrap_or(32);
                let o = a.octets();
                (o[0] == 10 && b >= 8) || (o[0] == 172 && (16..32).contains(&o[1]) && b >= 12) || (o[0] == 192 && o[1] == 168 && b >= 16)
                    || (o[0] == 127 && b >= 8) || (o[0] == 169 && o[1] == 254 && b >= 16)
            }
            IpAddr::V6(a) => {
                if bits.is_some_and(|b| b > 128) { return Err(format!("'{e}' has no valid prefix length")); }
                let b = bits.unwrap_or(128);
                let s0 = a.segments()[0];
                (a.is_loopback() && b >= 128) || ((s0 & 0xfe00) == 0xfc00 && b >= 7) || ((s0 & 0xffc0) == 0xfe80 && b >= 10)
            }
        };
        if !private { return Err(format!("'{e}' reaches beyond your home network; only private ranges such as 192.168.0.0/16 are allowed")); }
    }
    Ok(())
}

pub fn ip_in_list(list: &str, ip: IpAddr) -> bool {
    let ip = match ip { IpAddr::V6(a) => a.to_ipv4_mapped().map(IpAddr::V4).unwrap_or(IpAddr::V6(a)), v4 => v4 };
    list.split(|c: char| c == ',' || c.is_whitespace()).filter(|e| !e.is_empty()).any(|e| {
        let (addr, bits) = match e.split_once('/') { Some((a, b)) => (a, b.parse::<u32>().ok()), None => (e, None) };
        let Ok(net) = addr.parse::<IpAddr>() else { return false };
        match (net, ip) {
            (IpAddr::V4(n), IpAddr::V4(a)) => {
                let b = bits.unwrap_or(32).min(32);
                let mask = if b == 0 { 0 } else { u32::MAX << (32 - b) };
                (u32::from(n) & mask) == (u32::from(a) & mask)
            }
            (IpAddr::V6(n), IpAddr::V6(a)) => {
                let b = bits.unwrap_or(128).min(128);
                let mask = if b == 0 { 0 } else { u128::MAX << (128 - b) };
                (u128::from(n) & mask) == (u128::from(a) & mask)
            }
            _ => false,
        }
    })
}

/// One address from a forwarding header: "1.2.3.4", "1.2.3.4:5678", "[::1]:80", "\"[::1]\"".
fn parse_fwd(v: &str) -> Option<IpAddr> {
    let v = v.trim().trim_matches('"');
    if let Ok(ip) = v.parse() { return Some(ip); }
    if let Some(rest) = v.strip_prefix('[') { return rest.split(']').next()?.parse().ok(); }
    v.rsplit_once(':').and_then(|(a, _)| a.parse().ok())
}

/// The real caller: the connecting address, or, when that is a trusted reverse proxy, the
/// address it forwarded. The flag is false when a forwarding header came from someone who is
/// not trusted, so the address can't be relied on (it then never counts as local).
pub fn client_address(peer: Option<IpAddr>, xff: &[String], real_ip: Option<&str>, forwarded: &[String], trusted: &str) -> (Option<IpAddr>, bool) {
    if xff.is_empty() && real_ip.is_none() && forwarded.is_empty() {
        // A trusted proxy that says nothing about the caller: the caller is unknown, and the
        // proxy's own (private) address must not make everyone look like they are at home.
        let from_proxy = peer.is_some_and(|p| ip_in_list(trusted, p));
        return (peer, !from_proxy);
    }
    let Some(p) = peer else { return (None, false) };
    if !ip_in_list(trusted, p) { return (peer, false); }
    // X-Forwarded-For: proxies add on the right, so the first address from the right that is
    // not one of our proxies is the caller (anything left of it may be made up).
    let chain: Vec<&str> = xff.iter().flat_map(|h| h.split(',')).collect();
    if !chain.is_empty() {
        for part in chain.iter().rev() {
            match parse_fwd(part) {
                Some(ip) if ip_in_list(trusted, ip) => continue,
                Some(ip) => return (Some(ip), true),
                None => return (peer, false),
            }
        }
        return (parse_fwd(chain[0]), true);
    }
    if let Some(ip) = real_ip.and_then(parse_fwd) { return (Some(ip), true); }
    let fors: Vec<&str> = forwarded.iter().flat_map(|h| h.split(',')).filter_map(|el| {
        el.split(';').find_map(|kv| kv.trim().strip_prefix("for=").or_else(|| kv.trim().strip_prefix("For=")))
    }).collect();
    for f in fors.iter().rev() {
        match parse_fwd(f) {
            Some(ip) if ip_in_list(trusted, ip) => continue,
            Some(ip) => return (Some(ip), true),
            None => return (peer, false),
        }
    }
    (peer, false)
}

pub fn ip_is_local(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(a) => a.is_loopback() || a.is_private() || a.is_link_local(),
        IpAddr::V6(a) => {
            if let Some(v4) = a.to_ipv4_mapped() { return ip_is_local(IpAddr::V4(v4)); }
            let first = a.segments()[0];
            a.is_loopback() || (first & 0xfe00) == 0xfc00 || (first & 0xffc0) == 0xfe80
        }
    }
}

// ---------------------------------------------------------------- request context

/// How long a stopped playback can't start again unless its player heard why
/// (keeps players that reconnect by themselves from carrying on).
pub const STOP_HOLD: Duration = Duration::from_secs(30);

pub struct Ctx {
    pub method: String,
    pub path: String,
    pub params: Vec<(String, String)>,
    pub body: Value,
    pub user: Option<User>,
    pub host: String,
    pub range: Option<String>,
    pub token: Option<String>,
    /// The caller's IP address (behind a trusted reverse proxy: the one it forwarded).
    pub remote: Option<IpAddr>,
    /// `remote` can be relied on (no forwarding header from an untrusted machine).
    pub remote_trusted: bool,
    /// The caller's address is in the "home network" list of the settings.
    pub lan_extra: bool,
    /// The device asking: Plex apps name themselves (X-Plex-Product, X-Plex-Device-Name),
    /// browsers by their User-Agent ("Chrome on Windows").
    pub device: String,
}

impl Ctx {
    /// The request comes from this computer or the local network (private, link-local
    /// or loopback address). A request passed on by a reverse proxy counts only when the proxy
    /// is trusted in the settings, and then by the address it forwarded.
    pub fn is_local(&self) -> bool {
        self.remote_trusted && (self.remote.is_some_and(ip_is_local) || self.lan_extra)
    }

    pub fn viewer(&self) -> Viewer {
        Viewer { device: self.device.clone(), ip: self.remote.map(|a| a.to_string()).unwrap_or_default(), local: self.is_local() }
    }
    pub fn param(&self, k: &str) -> Option<&str> {
        self.params.iter().find(|(a, _)| a.eq_ignore_ascii_case(k)).map(|(_, v)| v.as_str())
    }
    pub fn num(&self, k: &str) -> Option<u64> {
        self.param(k).and_then(|v| v.parse().ok())
    }
    pub fn body_or_param(&self, k: &str) -> Option<String> {
        match self.body.get(k) {
            Some(Value::String(s)) => Some(s.clone()),
            Some(Value::Number(n)) => Some(n.to_string()),
            Some(Value::Bool(b)) => Some(b.to_string()),
            _ => self.param(k).map(String::from),
        }
    }
}

pub fn url_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'+' => out.push(b' '),
            b'%' if i + 2 < b.len() => {
                let h = std::str::from_utf8(&b[i + 1..i + 3]).ok().and_then(|h| u8::from_str_radix(h, 16).ok());
                match h {
                    Some(v) => {
                        out.push(v);
                        i += 2;
                    }
                    None => out.push(b'%'),
                }
            }
            c => out.push(c),
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).to_string()
}

pub fn parse_query(q: &str) -> Vec<(String, String)> {
    q.split('&')
        .filter(|p| !p.is_empty())
        .map(|p| match p.split_once('=') {
            Some((k, v)) => (url_decode(k), url_decode(v)),
            None => (url_decode(p), String::new()),
        })
        .collect()
}

// ---------------------------------------------------------------- server loop

/// Where the server listens ("address:port") and the certificate and key files it uses (empty = no HTTPS).
pub static LISTEN: Mutex<String> = Mutex::new(String::new());
pub static TLS_FILES: Mutex<(String, String)> = Mutex::new((String::new(), String::new()));
/// How often the certificate files are looked at for a renewal.
const TLS_CHECK_EVERY: Duration = Duration::from_secs(30);

fn tls_stamp() -> Vec<Option<std::time::SystemTime>> {
    let (c, k) = TLS_FILES.lock().unwrap().clone();
    [c, k].iter().map(|p| std::fs::metadata(p.trim()).and_then(|m| m.modified()).ok()).collect()
}

/// Listens again with the (renewed) certificate. The port is free for a moment only after the old
/// listener is gone, so this tries for a while.
fn relisten(pair: &(Vec<u8>, Vec<u8>)) -> Option<Arc<tiny_http::Server>> {
    let addr = LISTEN.lock().unwrap().clone();
    for _ in 0..40 {
        match tiny_http::Server::https(addr.as_str(), tiny_http::SslConfig { certificate: pair.0.clone(), private_key: pair.1.clone() }) {
            Ok(s) => return Some(Arc::new(s)),
            Err(_) => std::thread::sleep(Duration::from_millis(250)),
        }
    }
    None
}

pub fn serve(app: Arc<App>, server: Arc<tiny_http::Server>) {
    let mut server = Some(server);
    let mut stamp = tls_stamp();
    let mut checked = Instant::now();
    let mut good = {
        let (c, k) = TLS_FILES.lock().unwrap().clone();
        read_tls(&c, &k).ok().flatten()
    };
    loop {
        let Some(s) = server.as_ref() else { return };
        match s.recv_timeout(Duration::from_secs(5)) {
            Ok(Some(req)) => {
                let app = app.clone();
                std::thread::spawn(move || handle(app, req));
            }
            Ok(None) => {}
            Err(_) => return,
        }
        // A renewed certificate (certbot, acme.sh ...): listen again with it, no restart needed.
        if good.is_some() && checked.elapsed() >= TLS_CHECK_EVERY {
            checked = Instant::now();
            let now_stamp = tls_stamp();
            if now_stamp != stamp {
                stamp = now_stamp;
                let (c, k) = TLS_FILES.lock().unwrap().clone();
                match read_tls(&c, &k) {
                    Ok(Some(pair)) => {
                        drop(server.take());
                        server = relisten(&pair).or_else(|| good.as_ref().and_then(relisten));
                        if server.is_none() { crate::log::add("HTTPS: could not listen again after the certificate changed; restart the server".into()); return; }
                        good = Some(pair);
                        crate::log::add("HTTPS: the new certificate is in use".into());
                    }
                    Ok(None) => {}
                    Err(e) => crate::log::add(format!("HTTPS: the changed certificate files are not used ({e})")),
                }
            }
        }
    }
}

/// Answers every request on a plain http:// port with a redirect to the same page on https://.
pub fn redirect_to_https(bind: &str, port: u16, https_port: u16) -> Result<(), String> {
    let server = tiny_http::Server::http(format!("{bind}:{port}")).map_err(|e| e.to_string())?;
    std::thread::spawn(move || {
        for req in server.incoming_requests() {
            let host = req.headers().iter().find(|h| h.field.equiv("Host")).map(|h| h.value.as_str().to_string()).unwrap_or_default();
            let name = if let Some(rest) = host.strip_prefix('[') { format!("[{}", rest.split_once(']').map(|(a, _)| format!("{a}]")).unwrap_or_default()) } else { host.split(':').next().unwrap_or("").to_string() };
            let name = if name.is_empty() || name == "[" { "localhost".to_string() } else { name };
            let target = if https_port == 443 { format!("https://{name}{}", req.url()) } else { format!("https://{name}:{https_port}{}", req.url()) };
            let loc = Header::from_bytes(&b"Location"[..], target.as_bytes()).unwrap();
            let _ = req.respond(Response::empty(308).with_header(loc));
        }
    });
    Ok(())
}

pub fn start_background(app: &Arc<App>) {
    // Dashboard samples: processor, memory, bytes sent, streams.
    let a = app.clone();
    std::thread::spawn(move || {
        let mut last_t = crate::stats::times();
        let stats_file = a.data_dir.join("stats.json");
        a.stats.load(&stats_file);
        let mut last_bytes = a.stats.bytes_out.load(std::sync::atomic::Ordering::Relaxed);
        let mut last_remote = a.stats.bytes_remote.load(std::sync::atomic::Ordering::Relaxed);
        let mut taken: u64 = 0;
        let mut last_user: HashMap<u64, u64> = HashMap::new();
        let mut last_at = Instant::now();
        loop {
            std::thread::sleep(Duration::from_secs(crate::stats::SAMPLE_SECONDS));
            let secs = last_at.elapsed().as_secs_f64().max(0.001);
            last_at = Instant::now();
            let t = crate::stats::times();
            let cpu = match (last_t, t) { (Some(x), Some(y)) => crate::stats::cpu_between(x, y), _ => None };
            last_t = t;
            let mem = crate::stats::memory();
            let bytes = a.stats.bytes_out.load(std::sync::atomic::Ordering::Relaxed);
            let out_bps = (bytes.saturating_sub(last_bytes) as f64 / secs) as u64;
            last_bytes = bytes;
            let remote = a.stats.bytes_remote.load(std::sync::atomic::Ordering::Relaxed);
            let out_remote_bps = ((remote.saturating_sub(last_remote) as f64 / secs) as u64).min(out_bps);
            last_remote = remote;
            let mut per_user = HashMap::new();
            for (uid, c) in a.stats.user_bytes.lock().unwrap().iter() {
                let b = c.load(std::sync::atomic::Ordering::Relaxed);
                let before = last_user.insert(*uid, b).unwrap_or(b);
                per_user.insert(*uid, (b.saturating_sub(before) as f64 / secs) as u64);
            }
            *a.stats.user_bps.lock().unwrap() = per_user;
            a.stats.push(crate::stats::Sample {
                at: now(),
                cpu: cpu.map(|c| c.0),
                cpu_server: cpu.map(|c| c.1),
                mem_used: mem.as_ref().map(|m| m.used),
                mem_total: mem.as_ref().map(|m| m.total),
                mem_server: mem.as_ref().map(|m| m.process).filter(|p| *p > 0),
                out_bps,
                out_remote_bps,
                streams: a.stream_count(),
                transcodes: a.active_transcodes(),
            });
            // The minute and hour averages go to disk every five minutes.
            taken += 1;
            if taken % (300 / crate::stats::SAMPLE_SECONDS) == 0 {
                a.stats.save(&stats_file);
            }
        }
    });
    // Stop idle HLS transcodes and clean their folders.
    let a = app.clone();
    std::thread::spawn(move || loop {
        std::thread::sleep(Duration::from_secs(5));
        let mut stale = vec![];
        {
            let mut map = a.hls.lock().unwrap();
            let ids: Vec<String> = map.iter().filter(|(_, s)| s.last_access.elapsed() > Duration::from_secs(90)).map(|(k, _)| k.clone()).collect();
            for id in ids {
                if let Some(s) = map.remove(&id) {
                    stale.push(s);
                }
            }
        }
        for mut s in stale {
            let _ = s.child.kill();
            let _ = s.child.wait();
            let _ = std::fs::remove_dir_all(&s.dir);
        }
        // Players that stopped reporting: their play has ended.
        let gone: Vec<Playing> = {
            let mut map = a.playing.lock().unwrap();
            let keys: Vec<String> = map.iter().filter(|(_, p)| p.updated.elapsed() >= Duration::from_secs(120)).map(|(k, _)| k.clone()).collect();
            keys.iter().filter_map(|k| map.remove(k)).collect()
        };
        a.close_sessions(gone);
    });
    // Online details for new movies and shows.
    let a = app.clone();
    for _ in 0..3 { let a = a.clone(); std::thread::spawn(move || a.metadata_worker()); }
    // Converting downloads, one after the other.
    crate::downloads::start(app);
    // Watched state with a Plex server, both ways (when switched on).
    crate::plexsync::start(app);
    // Movies not found before the matcher learned more (MATCH_VERSION) get one more try.
    let retry: Vec<u64> = {
        let db = app.db.lock().unwrap();
        db.items.values()
            .filter(|i| i.kind == ItemKind::Movie && !i.extra.contains_key("metadata_source"))
            .filter(|i| i.extra.get("metadata_status").and_then(|v| v.as_str()) == Some("unmatched"))
            .filter(|i| i.extra.get("match_version").and_then(|v| v.as_u64()) != Some(MATCH_VERSION))
            .map(|i| i.id).collect()
    };
    if !retry.is_empty() {
        app.queue_online_details(&retry, false);
    }
    // Watch the library folders: when the files in a library change (added,
    // removed, renamed), wait until they stop changing (copying finished) and
    // scan that library. Only changed files are read again.
    let a = app.clone();
    std::thread::spawn(move || {
        let mut last: HashMap<u64, u64> = HashMap::new();
        let mut changed: HashSet<u64> = HashSet::new();
        loop {
            // (In 1 s steps, so a changed interval takes effect at once.)
            let mut waited = 0;
            while waited < WATCH_SECONDS.load(std::sync::atomic::Ordering::Relaxed) {
                std::thread::sleep(Duration::from_secs(1));
                waited += 1;
            }
            let (on, sections) = {
                let db = a.db.lock().unwrap();
                (db.settings.watch_folders, db.sections.clone())
            };
            if !on {
                last.clear();
                changed.clear();
                continue;
            }
            last.retain(|id, _| sections.iter().any(|s| s.id == *id));
            for s in sections {
                let sig = scanner::folder_signature(&s.paths, s.kind == db::SectionKind::Music);
                match last.insert(s.id, sig) {
                    // First look (also catches files added while the server was off).
                    None => {
                        changed.insert(s.id);
                    }
                    // Still changing: wait one more round.
                    Some(old) if old != sig => {
                        changed.insert(s.id);
                    }
                    Some(_) => {
                        if changed.contains(&s.id) && a.scan_now(s.id).is_some() {
                            changed.remove(&s.id);
                        }
                    }
                }
            }
        }
    });
    // Credits detection ("as soon as possible": within a minute of new files;
    // "scheduled": once a day) and IMDb ratings (weekly file).
    let a = app.clone();
    std::thread::spawn(move || {
        let mut last_scheduled = Instant::now();
        let mut last_intro = Instant::now();
        let mut imdb_retry = Instant::now();
        loop {
            std::thread::sleep(Duration::from_secs(60));
            let behaviour = a.settings().credits_markers_behaviour;
            let due = match behaviour.as_str() {
                "asap" => true,
                "scheduled" => last_scheduled.elapsed() >= Duration::from_secs(86400),
                _ => false,
            };
            let idle = a.meta_queue.lock().unwrap().is_empty() && a.job.lock().unwrap().is_none();
            if due && idle {
                let pending = a.credits_pending();
                if !pending.is_empty() {
                    last_scheduled = Instant::now();
                    let _ = a.detect_credits(pending, false);
                    continue;
                }
            }
            // Intro detection: the same choices ("Generate intro video markers").
            let intro_due = match a.settings().intro_markers_behaviour.as_str() {
                "asap" => true,
                "scheduled" => last_intro.elapsed() >= Duration::from_secs(86400),
                _ => false,
            };
            if intro_due && idle {
                let pending = a.intros_pending();
                if !pending.is_empty() {
                    last_intro = Instant::now();
                    let _ = a.detect_intros(pending, false);
                    continue;
                }
            }
            if idle && Instant::now() >= imdb_retry && a.settings().imdb_ratings_file {
                if let Err(e) = a.update_imdb_ratings(false) {
                    elog!("IMDb ratings: {e}");
                    imdb_retry = Instant::now() + Duration::from_secs(6 * 3600);
                }
            }
        }
    });
    // Scheduled maintenance: backup, optimize, clean up (inside the maintenance window).
    let a = app.clone();
    std::thread::spawn(move || loop {
        std::thread::sleep(Duration::from_secs(60));
        crate::maintenance::tick(&a);
    });
    // Plays that stay paused too long are ended.
    let a = app.clone();
    std::thread::spawn(move || loop {
        std::thread::sleep(Duration::from_secs(30));
        let minutes = a.settings().paused_stop_minutes;
        if minutes == 0 { continue; }
        let stale: Vec<(u64, u64)> = a.playing.lock().unwrap().values()
            .filter(|p| p.state == "paused" && p.updated.elapsed() >= Duration::from_secs(minutes as u64 * 60)).map(|p| (p.user_id, p.item_id)).collect();
        for (u, i) in stale { a.stop_playback(u, i, "Playback was ended because it stayed paused for too long"); }
    });
    // Scheduled library scans.
    let a = app.clone();
    std::thread::spawn(move || {
        let mut last: HashMap<u64, Instant> = HashMap::new();
        loop {
            std::thread::sleep(Duration::from_secs(30));
            let (enabled, global, ids) = {
                let db = a.db.lock().unwrap();
                (db.settings.scheduled_scans_enabled, db.settings.scan_interval_minutes, db.sections.iter().map(|s| (s.id, s.scan_interval_minutes)).collect::<Vec<_>>())
            };
            if !enabled {
                continue;
            }
            for (id, own) in ids {
                let interval = own.unwrap_or(global);
                if interval == 0 { continue; }
                let due = last.get(&id).is_none_or(|t| t.elapsed() >= Duration::from_secs(interval * 60));
                if due {
                    last.insert(id, Instant::now());
                    a.scan_now(id);
                }
            }
        }
    });
}

/// "Plex for Samsung — TV 2020" from the X-Plex headers (or the same names in the
/// address, as some Plex apps send them), else the browser ("Chrome on Windows").
pub fn device_label(product: &str, device: &str, params: &[(String, String)], agent: &str) -> String {
    let param = |k: &str| params.iter().find(|(n, _)| n.eq_ignore_ascii_case(k)).map(|(_, v)| v.trim().to_string()).unwrap_or_default();
    let product = if product.trim().is_empty() { param("X-Plex-Product") } else { product.trim().to_string() };
    let mut device = if device.trim().is_empty() { param("X-Plex-Device-Name") } else { device.trim().to_string() };
    if device.is_empty() { device = param("X-Plex-Device"); }
    match (product.is_empty(), device.is_empty() || device == product) {
        (false, false) => format!("{product} — {device}"),
        (false, true) => product,
        (true, false) => device,
        (true, true) => crate::stats::short_agent(agent),
    }
}

fn handle(app: Arc<App>, mut req: tiny_http::Request) {
    let method = req.method().as_str().to_uppercase();
    let url = req.url().to_string();
    let (path, query) = url.split_once('?').unwrap_or((&url, ""));
    let path = url_decode(path);
    let params = parse_query(query);
    let mut host = String::new();
    let mut token = None;
    let mut range = None;
    let mut agent = String::new();
    let (mut product, mut device_name) = (String::new(), String::new());
    let (mut xff, mut real_ip, mut forwarded) = (Vec::new(), None, Vec::new());
    for h in req.headers() {
        let f = h.field.as_str().as_str().to_ascii_lowercase();
        match f.as_str() {
            "host" => host = h.value.as_str().to_string(),
            "x-token" => token = Some(h.value.as_str().to_string()),
            "authorization" => {
                let v = h.value.as_str();
                token = Some(v.strip_prefix("Bearer ").unwrap_or(v).to_string());
            }
            "range" => range = Some(h.value.as_str().to_string()),
            "user-agent" => agent = h.value.as_str().to_string(),
            "x-plex-product" => product = h.value.as_str().to_string(),
            "x-plex-device-name" => device_name = h.value.as_str().to_string(),
            "x-plex-device" | "x-plex-model" if device_name.is_empty() => device_name = h.value.as_str().to_string(),
            "x-forwarded-for" => xff.push(h.value.as_str().to_string()),
            "x-real-ip" => real_ip = Some(h.value.as_str().to_string()),
            "forwarded" => forwarded.push(h.value.as_str().to_string()),
            _ => {}
        }
    }
    if token.is_none() {
        token = params
            .iter()
            .find(|(k, _)| k == "token")
            .map(|(_, v)| v.clone());
    }
    let mut body = Value::Null;
    if matches!(method.as_str(), "POST" | "PUT" | "PATCH" | "DELETE") {
        let mut buf = Vec::new();
        let _ = req.as_reader().take(4 * 1024 * 1024).read_to_end(&mut buf);
        if !buf.is_empty() {
            body = serde_json::from_slice(&buf).unwrap_or_else(|_| {
                // Accept form posts too.
                let s = String::from_utf8_lossy(&buf).to_string();
                Value::Object(parse_query(&s).into_iter().map(|(k, v)| (k, Value::String(v))).collect())
            });
        }
    }
    let device = device_label(&product, &device_name, &params, &agent);
    // A caller that came through the relay: the relay told us who it is (nobody else can).
    let relayed = req.remote_addr().and_then(|a| crate::relay::real_client(*a));
    let peer = relayed.map(|a| a.to_string()).or_else(|| req.remote_addr().map(|a| a.ip().to_string())).unwrap_or_default();
    let user = token.as_ref().and_then(|t| {
        let mut db = app.db.lock().unwrap();
        let at = now();
        let tk = db.tokens.iter_mut().find(|x| &x.token == t)?;
        // The list of devices says when each was used last (written at most once a minute).
        if at.saturating_sub(tk.last_used) > 60 { tk.last_used = at; tk.last_device = device.clone(); tk.last_ip = peer.clone(); }
        let uid = tk.user_id;
        db.user(uid).cloned()
    });
    let (trusted, no_auth, lan_list) = { let db = app.db.lock().unwrap(); (db.settings.trusted_proxies.clone(), db.settings.no_auth_networks.clone(), db.settings.lan_networks.clone()) };
    let (remote, remote_trusted) = match relayed {
        Some(ip) => (Some(ip), true),
        None => client_address(req.remote_addr().map(|a| a.ip()), &xff, real_ip.as_deref(), &forwarded, &trusted),
    };
    // Networks that need no sign-in act as the first admin (only with an address that is believed).
    let user = if user.is_none() && remote_trusted && !no_auth.trim().is_empty() && remote.is_some_and(|a| ip_in_list(&no_auth, a)) {
        app.db.lock().unwrap().users.iter().filter(|u| u.admin).min_by_key(|u| u.id).cloned()
    } else { user };
    let uid = user.as_ref().map(|u| u.id);
    if let Some(id) = uid {
        app.stats.saw(id, remote.map(|a| a.to_string()).unwrap_or_default(), &agent);
    }
    let lan_extra = !lan_list.trim().is_empty() && remote.is_some_and(|a| ip_in_list(&lan_list, a));
    let ctx = Ctx { method: method.clone(), path, params, body, user, host, range, token, remote, remote_trusted, lan_extra, device };
    let is_local = ctx.is_local();
    let resp = if method == "OPTIONS" { bytes(204, "text/plain", vec![]) } else { crate::routes::route(&app, &ctx) };
    // Count what is sent (bandwidth on the dashboard).
    let (status, headers, len) = (resp.status_code(), resp.headers().to_vec(), resp.data_length());
    let remote = if is_local { None } else { Some(app.stats.bytes_remote.clone()) };
    let body = crate::stats::Counting { inner: resp.into_reader(), all: app.stats.bytes_out.clone(), user: app.stats.counter_for(uid), remote };
    let resp: Resp = Response::new(status, headers, Box::new(body) as Box<dyn Read + Send>, len, None);
    let _ = req.respond(resp);
}

// ---------------------------------------------------------------- JSON views

pub fn item_json(db: &Db, user: &User, it: &Item, detail: bool) -> Value {
    let v = db.view(user.id, it.id);
    let mut o = json!({
        "ratingKey": it.id.to_string(),
        "id": it.id,
        "key": format!("/library/metadata/{}", it.id),
        "type": it.kind.as_str(),
        "metadataType": it.kind.code(),
        "title": it.title,
        "titleSort": it.sort_title,
        "librarySectionID": it.section_id,
        "summary": it.summary,
        "tagline": it.tagline,
        "genres": it.genres,
        "addedAt": it.added_at,
        "updatedAt": it.updated_at,
        "duration": it.duration_ms,
        // "?v=" changes with the item, so a browser that kept an old (or broken)
        // picture fetches the new one instead of showing it from its cache.
        "thumb": format!("/library/metadata/{}/thumb?v={}", it.id, it.updated_at),
        "favorite": v.favorite,
        "watchlist": v.watchlist_at > 0,
    });
    if v.watchlist_at > 0 { o["watchlistedAt"] = json!(v.watchlist_at); }
    if let Some(y) = it.year { o["year"] = json!(y); }
    if let Some(d) = it.extra.get("originally_available").and_then(|v| v.as_str()).filter(|d| !d.is_empty()) { o["originallyAvailableAt"] = json!(d); }
    if let Some(r) = it.extra.get("ratings").and_then(|r| r.as_object()) {
        let scores: serde_json::Map<String, Value> = r.iter().filter_map(|(k, v)| v.get("score").and_then(|s| s.as_str()).map(|s| (k.clone(), json!(s)))).collect();
        if !scores.is_empty() { o["scores"] = Value::Object(scores); }
    }
    if let Some(l) = it.extra.get("loudness_lufs").filter(|v| v.is_number()) { o["loudness"] = l.clone(); }
    if it.extra.contains_key(crate::scanner::MISSING) { o["missing"] = json!(true); }
    if let Some(r) = it.rating { o["rating"] = json!(r); }
    if let Some(r) = v.rating { o["userRating"] = json!(r); }
    if !it.content_rating.is_empty() { o["contentRating"] = json!(it.content_rating); }
    if !it.studio.is_empty() { o["studio"] = json!(it.studio); }
    if it.kind == ItemKind::Movie {
        // {edition-Theatrical} in the file name (or set by hand), shown under the poster.
        let e = it.extra.get("edition").and_then(|v| v.as_str()).map(String::from)
            .unwrap_or_else(|| it.file.as_ref().map(|f| crate::query::edition_of(&f.path)).unwrap_or_default());
        if !e.is_empty() { o["editionTitle"] = json!(e); }
    }
    if let Some(i) = it.index { o["index"] = json!(i); }
    for (k, name) in [("logo", "clearLogo"), ("square", "squareThumb")] {
        if it.extra.get(k).and_then(|v| v.as_str()).is_some() { o[name] = json!(format!("/library/metadata/{}/{k}?v={}", it.id, it.updated_at)); }
    }
    if it.kind.playable() {
        o["viewOffset"] = json!(v.view_offset_ms);
        o["viewCount"] = json!(v.view_count);
        if v.last_viewed_at > 0 { o["lastViewedAt"] = json!(v.last_viewed_at); }
    } else {
        let leaves = db.leaves(it.id);
        let seen = leaves.iter().filter(|l| db.view(user.id, **l).view_count > 0).count();
        o["leafCount"] = json!(leaves.len());
        o["viewedLeafCount"] = json!(seen);
        o["childCount"] = json!(db.children(it.id).len());
        if it.kind == ItemKind::Album { o["releaseType"] = it.extra.get("release").cloned().unwrap_or(json!("album")); }
    }
    if let Some(p) = it.parent_id.and_then(|p| db.items.get(&p)) {
        o["parentRatingKey"] = json!(p.id.to_string());
        o["parentTitle"] = json!(p.title);
        if let Some(i) = p.index { o["parentIndex"] = json!(i); }
        if let Some(g) = p.parent_id.and_then(|g| db.items.get(&g)) {
            o["grandparentRatingKey"] = json!(g.id.to_string());
            o["grandparentTitle"] = json!(g.title);
            o["grandparentThumb"] = json!(format!("/library/metadata/{}/thumb?v={}", g.id, g.updated_at));
        }
    }
    // Where the end credits start (ms), for the player's "Skip credits".
    if detail && it.kind.playable() {
        let credits = db.credits_markers(it);
        if !credits.is_empty() { o["credits"] = json!(credits); }
        // Where the intro runs (ms), for the player's "Skip intro".
        if let Some((s, e)) = db.intro_marker(it) { o["intro"] = json!({"start": s, "end": e}); }
    }
    // The file, then (if merged by hand) the other versions of it: 4K first.
    {
        let mut versions: Vec<&Item> = db.items.values().filter(|x| x.version_of() == Some(it.id)).collect();
        versions.sort_by_key(|x| (std::cmp::Reverse(x.file.as_ref().and_then(|f| f.video()).map(|v| v.height).unwrap_or(0)), x.id));
        let mut list: Vec<Value> = vec![];
        for m in std::iter::once(it).chain(versions) {
            let Some(f) = &m.file else { continue };
        let vid = f.video();
            let aud = f.audio();
            let mut media = json!({
                "id": m.id,
                "container": f.container,
                "formatName": f.format_name,
                "size": f.size,
                "bitrate": f.bitrate / 1000,
                "videoCodec": vid.map(|v| v.codec.clone()).unwrap_or_default(),
                "width": vid.map(|v| v.width).unwrap_or(0),
                "height": vid.map(|v| v.height).unwrap_or(0),
                "bitDepth": vid.map(|v| v.bit_depth).unwrap_or(0),
                "audioCodec": aud.map(|a| a.codec.clone()).unwrap_or_default(),
                "audioChannels": aud.map(|a| a.channels).unwrap_or(0),
                "probed": f.probed,
                "deepAnalysisVersion": if crate::deep::is_current(f) { f.deep_version } else { 0 },
                "requiredBandwidths": if crate::deep::is_current(f) { f.required_kbps.iter().map(|k| k.to_string()).collect::<Vec<_>>().join(",") } else { String::new() },
                // A split movie has a Part per file; they play one after the other.
                "Part": m.all_files().iter().enumerate().map(|(n, p)| json!({
                    "key": if n == 0 { format!("/library/parts/{}/file", m.id) } else { format!("/library/parts/{}/file?part={n}", m.id) },
                    "part": n, "size": p.size, "container": p.container, "duration": p.duration_ms,
                })).collect::<Vec<_>>(),
            });
            if detail {
                media["streams"] = json!(f.streams);
                let mut subs: Vec<Value> = f
                    .streams
                    .iter()
                    .filter(|s| s.kind == "subtitle")
                    .enumerate()
                    .map(|(n, s)| json!({"key": format!("/library/parts/{}/subtitles/{}", m.id, n), "language": s.language, "codec": s.codec, "title": s.title, "forced": s.forced, "sdh": s.sdh, "embedded": true,
                        "offsetMs": crate::handlers::subtitle_timing(m, &n.to_string()).map(|t| t.offset_ms)}))
                    .collect();
                for (n, p) in f.external_subtitles.iter().enumerate() {
                    subs.push(json!({"key": format!("/library/parts/{}/subtitles/x{}", m.id, n), "file": Path::new(p).file_name().map(|s| s.to_string_lossy().to_string()), "embedded": false,
                        "offsetMs": crate::handlers::subtitle_timing(m, &format!("x{n}")).map(|t| t.offset_ms)}));
                }
                media["subtitles"] = json!(subs);
                if user.admin {
                    media["file"] = json!(f.path);
                }
            }
            media["edition"] = json!(m.extra.get("edition").and_then(|v| v.as_str()).map(String::from).unwrap_or_else(|| crate::query::edition_of(&f.path)));
            list.push(media);
            }
        if !list.is_empty() { o["Media"] = json!(list); }
    }
    // Trailers, featurettes, deleted scenes … of a movie.
    if detail && it.kind == ItemKind::Movie {
        let mut extras: Vec<&Item> = db.items.values().filter(|x| x.parent_id == Some(it.id) && x.is_extra()).collect();
        extras.sort_by(|a, b| (a.extra["extra_type"].as_str() != Some("trailer"), &a.sort_title).cmp(&(b.extra["extra_type"].as_str() != Some("trailer"), &b.sort_title)));
        if !extras.is_empty() {
            o["Extras"] = json!(extras.iter().map(|x| json!({
                "ratingKey": x.id.to_string(), "id": x.id, "type": "clip", "title": x.title,
                "extraType": x.extra["extra_type"], "duration": x.duration_ms,
                "thumb": format!("/library/metadata/{}/thumb?v={}", x.id, x.updated_at),
            })).collect::<Vec<_>>());
        }
    }
    if !it.extra.is_empty() {
        o["extra"] = Value::Object(it.extra.clone());
        if !db.settings.trailers_online { o["extra"].as_object_mut().map(|m| m.remove("trailers")); }
        // Each person on a detail page carries
        // how many items of the same library they appear in.
        if detail {
            if let Some(Value::Array(cast)) = o["extra"].get_mut("cast") {
                // People are counted by name.
                let by_name = |c: &Value| name_key(c["name"].as_str().unwrap_or(""));
                let keys: Vec<String> = cast.iter().map(by_name).collect();
                let mut counts: HashMap<&str, u64> = HashMap::new();
                for other in db.items.values().filter(|i| i.section_id == it.section_id && i.parent_id.is_none()) {
                    if let Some(Value::Array(oc)) = other.extra.get("cast") {
                        let mut seen: Vec<String> = vec![];
                        for c in oc {
                            let k = by_name(c);
                            if let Some(mine) = keys.iter().find(|x| **x == k) {
                                if !seen.contains(&k) {
                                    *counts.entry(mine.as_str()).or_default() += 1;
                                    seen.push(k);
                                }
                            }
                        }
                    }
                }
                for (c, k) in cast.iter_mut().zip(keys.iter()) {
                    c["count"] = json!(counts.get(k.as_str()).copied().unwrap_or(1));
                }
            }
        }
    }
    o
}

pub fn section_json(db: &Db, s: &Section) -> Value {
    let top = db.items.values().filter(|i| i.section_id == s.id && i.parent_id.is_none() && i.version_of().is_none()).count();
    let playable = db.items.values().filter(|i| i.section_id == s.id && i.kind.playable() && !i.hidden()).count();
    let trash = db.items.values().filter(|i| i.section_id == s.id && i.extra.contains_key(crate::scanner::MISSING)).count();
    json!({
        "key": s.id.to_string(), "id": s.id, "title": s.name, "type": s.kind.as_str(),
        "Location": s.paths.iter().map(|p| json!({"path": p})).collect::<Vec<_>>(),
        "paths": s.paths, "createdAt": s.created_at, "scannedAt": s.scanned_at, "creditsDetection": s.credits_detection, "introDetection": s.intro_detection, "language": s.language, "scanIntervalMinutes": s.scan_interval_minutes,
        "count": top, "videoCount": playable, "trash": trash,
    })
}

pub fn playlist_json(db: &Db, user: &User, p: &Playlist) -> (Value, usize, u64) {
    let items = query::playlist_items(db, user, p);
    let duration: u64 = items.iter().filter_map(|i| db.items.get(i)).map(|i| i.duration_ms).sum();
    let thumb = items.first().map(|i| format!("/library/metadata/{i}/thumb"));
    // Up to four different pictures spread over the list, for the playlist's own picture.
    let mut thumbs: Vec<String> = vec![];
    let step = (items.len() / 4).max(1);
    for k in 0..items.len().min(4) {
        let top = items.get((k * step + (p.id as usize % step.max(1))).min(items.len() - 1)).and_then(|i| db.items.get(i));
        let top = top.map(|t| if t.kind == crate::db::ItemKind::Episode { t.parent_id.and_then(|s| db.items.get(&s)).and_then(|s| s.parent_id).and_then(|s| db.items.get(&s)).unwrap_or(t) } else { t });
        if let Some(t) = top { let u = format!("/library/metadata/{}/thumb?v={}", t.id, t.updated_at); if !thumbs.contains(&u) { thumbs.push(u); } }
    }
    let owner = if p.owner_id == 0 { "server".to_string() } else { db.user(p.owner_id).map(|u| u.username.clone()).unwrap_or_default() };
    let kind = if p.is_default() { "default" } else if p.kind == PlaylistKind::Smart { "smart" } else { "manual" };
    let v = json!({
        "ratingKey": p.id.to_string(), "id": p.id, "key": format!("/playlists/{}/items", p.id),
        "type": "playlist", "playlistType": if !items.is_empty() && items.iter().all(|i| db.items.get(i).is_some_and(|x| x.kind == crate::db::ItemKind::Track)) { "audio" } else { "video" }, "title": p.title, "summary": p.summary,
        "smart": p.kind == PlaylistKind::Smart, "kind": kind,
        "default": p.is_default(), "defaultKey": p.default_key, "query": p.query,
        "leafCount": items.len(), "duration": duration, "thumb": thumb, "composite": thumb, "thumbs": thumbs,
        "owner": owner, "ownerId": p.owner_id, "shared": p.shared, "pinned": p.pinned,
        "addedAt": p.created_at, "updatedAt": p.updated_at,
        "editable": p.owner_id == user.id || user.admin,
    });
    (v, items.len(), duration)
}

pub fn page<T: Clone>(ctx: &Ctx, list: &[T]) -> (Vec<T>, usize) {
    let off = ctx.num("offset").or(ctx.num("start")).unwrap_or(0) as usize;
    let lim = ctx.num("limit").or(ctx.num("size")).map(|l| l as usize).unwrap_or(usize::MAX);
    (list.iter().skip(off).take(lim).cloned().collect(), off)
}

pub fn user_json(u: &User) -> Value {
    json!({"id": u.id, "username": u.username, "email": u.email, "admin": u.admin, "created_at": u.created_at,
        "allowed_sections": u.allowed_sections, "allow_transcode": u.allow_transcode.unwrap_or(true), "allow_delete": u.allow_delete, "allow_download": u.admin || u.allow_download.unwrap_or(true), "invited_by": u.invited_by,
        "home": u.home, "has_pin": !u.pin_hash.is_empty(), "max_age": u.max_age, "block_unrated": u.block_unrated, "full_name": u.full_name, "two_factor": !u.totp_secret.is_empty()})
}

pub fn invite_json(db: &Db, i: &Invite, base: &str) -> Value {
    let status = if i.used_by.is_some() { "used" } else if i.expires_at < now() { "expired" } else { "open" };
    json!({"code": i.code, "url": format!("{base}/#/invite/{}", i.code), "email": i.email, "note": i.note, "created_at": i.created_at,
        "expires_at": i.expires_at, "allowed_sections": i.allowed_sections, "admin": i.admin, "status": status,
        "used_by": i.used_by.and_then(|u| db.user(u)).map(|u| u.username.clone()), "created_by": db.user(i.created_by).map(|u| u.username.clone())})
}

pub fn base_url(db: &Db, ctx: &Ctx) -> String {
    if !db.settings.public_url.trim().is_empty() {
        return db.settings.public_url.trim().trim_end_matches('/').to_string();
    }
    if ctx.host.is_empty() { format!("{}://localhost:{DEFAULT_PORT}", scheme()) } else { format!("{}://{}", scheme(), ctx.host) }
}

/// The server speaks HTTPS (a certificate is set).
pub static TLS_ON: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// "https" or "http", for the addresses the server hands out.
pub fn scheme() -> &'static str {
    if TLS_ON.load(std::sync::atomic::Ordering::Relaxed) { "https" } else { "http" }
}

/// Both PEM files read, or why not. (None, None) = no certificate asked for.
pub fn read_tls(cert: &str, key: &str) -> Result<Option<(Vec<u8>, Vec<u8>)>, String> {
    let (cert, key) = (cert.trim(), key.trim());
    if cert.is_empty() && key.is_empty() { return Ok(None); }
    if cert.is_empty() || key.is_empty() { return Err("HTTPS needs both the certificate file and the key file".into()); }
    let c = std::fs::read(cert).map_err(|e| format!("could not read the certificate {cert}: {e}"))?;
    let k = std::fs::read(key).map_err(|e| format!("could not read the key {key}: {e}"))?;
    if !String::from_utf8_lossy(&c).contains("-----BEGIN CERTIFICATE-----") { return Err(format!("{cert} is not a PEM certificate (it must contain -----BEGIN CERTIFICATE-----)")); }
    if !String::from_utf8_lossy(&k).contains("PRIVATE KEY-----") { return Err(format!("{key} is not a PEM private key")); }
    // The key must be the one of the certificate (else every browser fails the handshake, silently).
    use rustls::pki_types::{CertificateDer, PrivateKeyDer, pem::PemObject};
    let chain: Vec<CertificateDer<'static>> = CertificateDer::pem_slice_iter(&c).collect::<Result<_, _>>().map_err(|e| format!("{cert} is not a valid PEM certificate: {e}"))?;
    let parsed = PrivateKeyDer::from_pem_slice(&k).map_err(|e| format!("{key} is not a valid PEM private key (an encrypted key is not supported): {e}"))?;
    let signing = rustls::crypto::ring::sign::any_supported_type(&parsed).map_err(|e| format!("{key}: this kind of key is not supported ({e})"))?;
    if rustls::sign::CertifiedKey::new(chain, signing).keys_match().is_err() { return Err(format!("{key} is not the private key of the certificate {cert}")); }
    Ok(Some((c, k)))
}

pub fn visible_playlist(p: &Playlist, user: &User) -> bool {
    p.owner_id == 0 || p.owner_id == user.id || p.shared
}

pub fn paths_from(ctx: &Ctx) -> Vec<String> {
    match ctx.body.get("paths") {
        Some(Value::Array(a)) => a.iter().filter_map(|p| p.as_str()).map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect(),
        Some(Value::String(s)) => s.split(['\n', ';']).map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect(),
        _ => ctx.params.iter().filter(|(k, _)| k == "location" || k == "path").map(|(_, v)| v.clone()).collect(),
    }
}

pub fn ids_from(ctx: &Ctx) -> Vec<u64> {
    let mut out = vec![];
    if let Some(Value::Array(a)) = ctx.body.get("ids") {
        out.extend(a.iter().filter_map(|v| v.as_u64().or_else(|| v.as_str().and_then(|s| s.parse().ok()))));
    }
    if let Some(s) = ctx.param("ids") {
        out.extend(s.split(',').filter_map(|x| x.trim().parse::<u64>().ok()));
    }
    if let Some(uri) = ctx.param("uri") {
        // as a list: .../library/metadata/12,13
        if let Some(tail) = uri.rsplit("/library/metadata/").next() {
            out.extend(tail.split(',').filter_map(|x| x.trim().parse::<u64>().ok()));
        }
    }
    out
}

pub fn with_item(app: &Arc<App>, user: &User, iid: &str, f: impl FnOnce(&Db, &Item) -> Resp) -> Resp {
    let db = app.db.lock().unwrap();
    match iid.parse::<u64>().ok().and_then(|i| db.items.get(&i)) {
        Some(it) if db.can_see_item(user, it) => f(&db, it),
        Some(_) => fail(403, "You don't have access to this library"),
        None => fail(404, "Not found"),
    }
}

pub fn hub(db: &Db, user: &User, key: &str, title: &str, ids: Vec<u64>) -> Value {
    let list: Vec<Value> = ids.iter().filter_map(|i| db.items.get(i)).map(|i| item_json(db, user, i, false)).collect();
    json!({"hubIdentifier": key, "title": title, "size": list.len(), "Metadata": list})
}

pub fn recently_added(db: &Db, user: &User, kind: Option<ItemKind>, limit: usize) -> Vec<u64> {
    let mut v: Vec<&Item> = db
        .items
        .values()
        .filter(|i| db.can_see_item(user, i))
        .filter(|i| match kind {
            Some(k) => i.kind == k,
            None => i.kind.playable(),
        } && !i.hidden())
        .collect();
    v.sort_by(|a, b| b.added_at.cmp(&a.added_at).then(b.id.cmp(&a.id)));
    v.into_iter().take(limit).map(|i| i.id).collect()
}

/// The shows the given episodes belong to, each once, in the order of their newest episode.
pub fn shows_of(db: &Db, ids: &[u64], limit: usize) -> Vec<u64> {
    let mut out: Vec<u64> = vec![];
    for id in ids {
        let mut cur = db.items.get(id);
        for _ in 0..4 {
            match cur {
                Some(i) if i.kind == ItemKind::Show => { if !out.contains(&i.id) { out.push(i.id); } break; }
                Some(i) => cur = i.parent_id.and_then(|p| db.items.get(&p)),
                None => break,
            }
        }
        if out.len() >= limit { break; }
    }
    out
}

/// One week in seconds, for the Continue Watching window:
/// last viewed + weeks × one week >= now.
pub const WEEK_SECONDS: u64 = 604800;

/// Next unwatched episode of each show the user has been watching.
pub fn on_deck(db: &Db, user: &User) -> Vec<u64> {
    let window = db.settings.continue_watching_weeks * WEEK_SECONDS;
    let mut shows: Vec<(u64, u64)> = vec![];
    for show in db.items.values().filter(|i| i.kind == ItemKind::Show && db.can_see_item(user, i)) {
        let mut eps = db.leaves(show.id);
        query::sort_items(db, user.id, &mut eps, "episode_order");
        let Some(pos) = eps.iter().rposition(|e| db.view(user.id, *e).view_count > 0) else { continue };
        let when = db.view(user.id, eps[pos]).last_viewed_at;
        let Some(next) = eps.iter().skip(pos + 1).find(|e| db.view(user.id, **e).view_count == 0) else { continue };
        // Season premieres: a second
        // pass without the time window for the shows returns, i.e. shows
        // with an episode 1 of a season other than season 1 that was ADDED to the
        // library inside the window (index=1, parents.index<>1, added_at > cutoff).
        let cutoff = now().saturating_sub(window);
        let premiere = db.settings.continue_watching_include_premieres
            && eps.iter().filter_map(|e| db.items.get(e)).any(|e| {
                e.index == Some(1)
                    && e.added_at > cutoff
                    && e.parent_id.and_then(|s| db.items.get(&s)).and_then(|s| s.index) != Some(1)
            });
        if now().saturating_sub(when) > window && !premiere {
            continue;
        }
        shows.push((*next, when));
    }
    shows.sort_by(|a, b| b.1.cmp(&a.1));
    shows.into_iter().map(|s| s.0).collect()
}

/// Started-but-unfinished items plus the next episode of shows in progress,
/// newest first, limited by the window in weeks and a count.
pub fn continue_watching(db: &Db, user: &User) -> Vec<u64> {
    let window = db.settings.continue_watching_weeks * WEEK_SECONDS;
    let mut started: Vec<(u64, u64)> = db
        .items
        .values()
        .filter(|i| i.kind.playable() && !i.hidden() && db.can_see_item(user, i))
        .filter_map(|i| {
            let v = db.view(user.id, i.id);
            (v.view_offset_ms > 0 && now().saturating_sub(v.last_viewed_at) <= window).then_some((i.id, v.last_viewed_at))
        })
        .collect();
    for id in on_deck(db, user) {
        if !started.iter().any(|s| s.0 == id) {
            let when = db
                .items
                .get(&id)
                .and_then(|e| e.parent_id)
                .and_then(|season| db.items.get(&season))
                .and_then(|s| s.parent_id)
                .map(|show| db.leaves(show).iter().map(|l| db.view(user.id, *l).last_viewed_at).max().unwrap_or(0))
                .unwrap_or(0);
            started.push((id, when));
        }
    }
    // Taken off by hand: stays off until the person watches (the show or the title) again.
    started.retain(|(id, when)| { let h = db.view(user.id, *id).cw_hidden_at; h == 0 || *when > h });
    started.sort_by(|a, b| b.1.cmp(&a.1));
    started.truncate(db.settings.continue_watching_limit.max(1));
    started.into_iter().map(|s| s.0).collect()
}

pub const FAVICON: &str = "<svg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 32 32'><rect width='32' height='32' rx='7' fill='#14181c'/><path d='M11 8l14 8-14 8z' fill='#f2a516'/></svg>";

/// A gradient poster with the title, for items without a picture.
pub fn placeholder_svg(title: &str, kind: &str, id: u64) -> String {
    let hue = (id.wrapping_mul(47) % 360) as u32;
    let esc = |s: &str| s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;");
    let mut lines = vec![String::new()];
    for w in title.split_whitespace() {
        if lines.last().unwrap().len() + w.len() > 14 && !lines.last().unwrap().is_empty() {
            lines.push(String::new());
        }
        let l = lines.last_mut().unwrap();
        if !l.is_empty() {
            l.push(' ');
        }
        l.push_str(w);
    }
    lines.truncate(4);
    let text: String = lines
        .iter()
        .enumerate()
        .map(|(i, l)| format!("<text x='100' y='{}' text-anchor='middle' font-family='sans-serif' font-size='19' font-weight='bold' fill='#fff'>{}</text>", 130 + i as i32 * 24, esc(l)))
        .collect();
    format!(
        "<svg xmlns='http://www.w3.org/2000/svg' width='200' height='300' viewBox='0 0 200 300'><defs><linearGradient id='g' x1='0' y1='0' x2='1' y2='1'><stop offset='0' stop-color='hsl({hue},45%,38%)'/><stop offset='1' stop-color='hsl({},50%,14%)'/></linearGradient></defs><rect width='200' height='300' fill='url(#g)'/>{text}<text x='100' y='280' text-anchor='middle' font-family='sans-serif' font-size='12' fill='rgba(255,255,255,.6)'>{}</text></svg>",
        (hue + 40) % 360,
        kind.to_uppercase()
    )
}

/// "CD1", "Disc 2", "Part 3", "DVD1" … — a folder for one part of a movie.
fn is_disc_folder(name: &str) -> bool {
    let n = name.trim().to_lowercase();
    ["cd", "disc", "disk", "dvd", "part", "pt"].iter().any(|p| {
        n.strip_prefix(p).is_some_and(|rest| { let r = rest.trim_start_matches([' ', '.', '_', '-']); !r.is_empty() && r.chars().all(|c| c.is_ascii_digit()) })
    })
}

/// The same folder, however the path is written (separators, a trailing slash, letter case on Windows).
fn scanner_same_dir(a: &Path, b: &Path) -> bool {
    crate::migrate::path_key(&a.to_string_lossy()) == crate::migrate::path_key(&b.to_string_lossy())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::MediaFile;

    #[test]
    fn folder_title_only_for_a_movie_folder_of_its_own() {
        let mut db: Db = serde_json::from_value(json!({"version": 1, "machine_id": "x", "next_id": 9, "settings": crate::db::Settings::default(),
            "sections": [{"id": 1, "name": "Filme", "kind": "movie", "paths": ["/m"], "created_at": 0}],
            "items": {}, "users": [], "invites": [], "tokens": [], "playlists": [], "views": {}, "plugins": {}})).unwrap();
        let movie = |id: u64, path: &str| -> Item {
            let mut i: Item = serde_json::from_value(json!({"id": id, "section_id": 1, "kind": "movie", "title": "funxd", "sort_title": "funxd",
                "added_at": 0, "updated_at": 0})).unwrap();
            i.file = Some(MediaFile { path: path.into(), ..Default::default() });
            i
        };
        for i in [movie(1, "/m/Der Pate (1972)/funxd.mkv"), movie(2, "/m/loose.mkv"), movie(3, "/m/Sammlung/a.mkv"), movie(4, "/m/Sammlung/b.mkv")] {
            db.items.insert(i.id, i);
        }
        assert_eq!(App::folder_title(&db, &db.items[&1]), Some(("Der Pate".into(), Some(1972))));
        // Right in the library folder: no folder name to use.
        assert_eq!(App::folder_title(&db, &db.items[&2]), None);
        // A folder holding several movies is not a title.
        assert_eq!(App::folder_title(&db, &db.items[&3]), None);
    }

    #[test]
    fn folder_title_looks_past_disc_and_release_folders() {
        let mut db: Db = serde_json::from_value(json!({"version": 1, "machine_id": "x", "next_id": 9, "settings": crate::db::Settings::default(),
            "sections": [{"id": 1, "name": "Filme", "kind": "movie", "paths": ["/m/"], "created_at": 0}],
            "items": {}, "users": [], "invites": [], "tokens": [], "playlists": [], "views": {}, "plugins": {}})).unwrap();
        let movie = |id: u64, path: &str| -> Item {
            let mut i: Item = serde_json::from_value(json!({"id": id, "section_id": 1, "kind": "movie", "title": "funxd", "sort_title": "funxd",
                "added_at": 0, "updated_at": 0})).unwrap();
            i.file = Some(MediaFile { path: path.into(), ..Default::default() });
            i
        };
        for i in [movie(1, "/m/Der Pate (1972)/funxd/funxd.mkv"), movie(2, "/m/Heat (1995)/CD1/funxd.mkv"),
            movie(3, "/m/Der.Pate.1972.German.DL.1080p.BluRay.x264-FuNXD/funxd.mkv"), movie(4, "/m/funxd/funxd.mkv")] {
            db.items.insert(i.id, i);
        }
        assert_eq!(App::folder_title(&db, &db.items[&1]), Some(("Der Pate".into(), Some(1972))));
        assert_eq!(App::folder_title(&db, &db.items[&2]), Some(("Heat".into(), Some(1995))));
        assert_eq!(App::folder_title(&db, &db.items[&3]), Some(("Der Pate".into(), Some(1972))));
        // Only the library folder above: nothing better than the file name.
        assert_eq!(App::folder_title(&db, &db.items[&4]), None);
        assert!(is_disc_folder("CD1") && is_disc_folder("Disc 2") && !is_disc_folder("Cdrama"));
    }
}

#[cfg(test)]
mod proxy_tests {
    use super::*;
    fn ip(s: &str) -> Option<IpAddr> { s.parse().ok() }
    fn v(s: &[&str]) -> Vec<String> { s.iter().map(|x| x.to_string()).collect() }

    #[test]
    fn network_lists_must_be_well_formed() {
        assert!(check_network_list("").is_ok());
        assert!(check_network_list("100.64.0.0/10, 203.0.113.7 2001:db8::/32").is_ok());
        assert!(check_network_list("10.0.0.0/33").is_err());
        assert!(check_network_list("not-an-ip").is_err());
    }

    #[test]
    fn open_networks_must_be_private() {
        assert!(check_open_networks("").is_ok());
        // HTTPS settings: nothing = plain HTTP, half or wrong files = a clear error.
        assert_eq!(read_tls("", " "), Ok(None));
        assert!(read_tls("a.pem", "").unwrap_err().contains("both"));
        assert!(read_tls("/no/such/cert.pem", "/no/such/key.pem").unwrap_err().contains("could not read"));
        let dir = std::env::temp_dir().join(format!("lichtspiel-tls-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let (c, k) = (dir.join("c.pem"), dir.join("k.pem"));
        std::fs::write(&c, "hello").unwrap();
        std::fs::write(&k, "hello").unwrap();
        assert!(read_tls(&c.to_string_lossy(), &k.to_string_lossy()).unwrap_err().contains("not a PEM certificate"));
        std::fs::write(&c, "-----BEGIN CERTIFICATE-----
AAAA
-----END CERTIFICATE-----
").unwrap();
        assert!(read_tls(&c.to_string_lossy(), &k.to_string_lossy()).unwrap_err().contains("not a PEM private key"));
        let _ = std::fs::remove_dir_all(&dir);
        assert!(check_open_networks("192.168.1.0/24, 10.0.0.5 fd00::/8").is_ok());
        assert!(check_open_networks("0.0.0.0/0").is_err());
        assert!(check_open_networks("8.8.8.8").is_err());
        assert!(check_open_networks("192.168.0.0/8").is_err());
        assert!(check_open_networks("172.15.0.1").is_err());
        assert!(check_open_networks("nonsense").is_err());
    }

    #[test]
    fn lists() {
        assert!(ip_in_list("172.16.0.0/12", "172.18.0.3".parse().unwrap()));
        assert!(!ip_in_list("172.16.0.0/12", "172.32.0.1".parse().unwrap()));
        assert!(ip_in_list("10.0.0.1, ::1", "::1".parse().unwrap()));
        assert!(ip_in_list("127.0.0.1", "::ffff:127.0.0.1".parse().unwrap()));
        assert!(!ip_in_list("", "127.0.0.1".parse().unwrap()));
    }

    #[test]
    fn forwarding() {
        let proxy = ip("172.18.0.2");
        // No proxy in between: the connecting address.
        assert_eq!(client_address(ip("192.168.1.20"), &[], None, &[], ""), (ip("192.168.1.20"), true));
        // A forwarding header from a machine that isn't trusted can't be believed.
        assert_eq!(client_address(proxy, &v(&["192.168.1.20"]), None, &[], "").1, false);
        // Trusted proxy: the rightmost address that isn't a proxy; a made-up one on the left is ignored.
        assert_eq!(client_address(proxy, &v(&["192.168.1.20, 203.0.113.9"]), None, &[], "172.16.0.0/12"), (ip("203.0.113.9"), true));
        assert_eq!(client_address(proxy, &v(&["192.168.1.20"]), None, &[], "172.16.0.0/12"), (ip("192.168.1.20"), true));
        assert_eq!(client_address(proxy, &[], Some("203.0.113.9"), &[], "172.18.0.2"), (ip("203.0.113.9"), true));
        assert_eq!(client_address(proxy, &[], None, &v(&["for=\"[2001:db8::1]:4711\";proto=https"]), "172.18.0.2"), (ip("2001:db8::1"), true));
        assert_eq!(client_address(proxy, &v(&["garbage"]), None, &[], "172.18.0.2").1, false);
        // The proxy forgot the header: not trusted, so never "home network".
        assert_eq!(client_address(proxy, &[], None, &[], "172.16.0.0/12").1, false);
    }
}

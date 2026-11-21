//! The HTTP server core: the shared state (`App`), opening and saving the library, and
//! starting work. Library, hub, playlist and transcode paths
//! (/library/sections, /library/metadata/{id}, /hubs/home/…, /playlists, /:/timeline,
//! /video/…/transcode); admin features live under /api/.
//! Routes are in routes.rs, the larger handlers in handlers.rs. The rest of the server
//! lives next to this file, one topic each (all re-exported here as `server::…`):
//! serve (the request loop), http (building answers), net (who is calling),
//! views (JSON shapes), background, jobs (intros, credits), matching (online details)
//! and bundled (the plugins shipped with the server).

mod background;
mod bundled;
mod http;
mod jobs;
mod matching;
mod net;
mod serve;
mod views;

pub use background::*;
use bundled::install_bundled_plugins;
pub use http::*;
pub use matching::*;
pub use net::*;
pub use serve::*;
pub use views::*;

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
/// The commit this build is made from (a trailing "+" = with changes that were not committed) and when it was built.
pub const BUILD: &str = env!("LICHTSPIEL_BUILD");
pub const BUILT_AT: &str = env!("LICHTSPIEL_BUILT_AT");
/// The server's name (after the old German word for cinema, "Lichtspielhaus").
pub const APP_NAME: &str = "Lichtspiel";
/// The default port; it can be
/// changed in Settings (saved, used after a restart) or with --port /
/// LICHTSPIEL_PORT.
pub const DEFAULT_PORT: u16 = 8484;

/// How often library folders are looked at for new or changed files (seconds).
/// (The self-test makes it shorter.) Each look reads the size and time of every file
/// (about 0.3 s for 50,000 files), so not more often: a NAS's disks should get some rest.
pub static WATCH_SECONDS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(60);

pub const WEB_APP: &str = include_str!("../../web/index.html");
pub const WEB_JS: &str = include_str!("../../web/app.js");
pub const WEB_LIBRARY_JS: &str = include_str!("../../web/library.js");
pub const WEB_PLAYER_JS: &str = include_str!("../../web/player.js");
pub const WEB_PAGES_JS: &str = include_str!("../../web/pages.js");
pub const WEB_DASHBOARD_JS: &str = include_str!("../../web/dashboard.js");
pub const WEB_DOWNLOADS_JS: &str = include_str!("../../web/downloads.js");
pub const WEB_I18N_JS: &str = include_str!("../../web/i18n.js");
/// Language packs for the web app (web/lang/<code>.js); English needs none.
/// To add a language: put its file in web/lang, list it here and add a
/// <script> line for it in index.html.
pub const WEB_LANGS: &[(&str, &str)] = &[("de.js", include_str!("../../web/lang/de.js"))];

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
    /// The running credits or intro detection was started by the server itself (not by a button).
    pub job_auto: AtomicBool,
    /// An automatic credits or intro run was cancelled by hand: the server does not start one by itself
    /// again until a run is started by hand (or the server restarts).
    pub auto_paused: AtomicBool,
    /// It was stopped only to let the details go first: it ends quietly and starts again later.
    pub job_yield: AtomicBool,
    /// What the running job works through, in order (its label, then the names): for the activity panel.
    pub job_items: Mutex<(String, Vec<String>)>,
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
        let _ = crate::scanner::SUBTITLE_STORE.set(data_dir.join("subtitles"));
        let (store, mut db, from_json) = open_library(data_dir);
        db.upgrade();
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
            job_auto: AtomicBool::new(false),
            auto_paused: AtomicBool::new(false),
            job_yield: AtomicBool::new(false),
            job_items: Mutex::new((String::new(), vec![])),
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

    /// A file from the library may be sent: it really lies in a library folder (or, with `data`, in
    /// the data folder, where downloaded pictures are). Takes the library lock for a moment.
    pub fn may_send(&self, p: &Path, data: bool) -> bool {
        let mut folders: Vec<PathBuf> = self.db.lock().unwrap().sections.iter().flat_map(|s| s.paths.iter().map(PathBuf::from)).collect();
        // Subtitles that plugins saved for read-only media folders.
        folders.push(self.data_dir.join("subtitles"));
        if data { folders.push(self.data_dir.clone()); }
        inside_folders(&folders, p)
    }

    pub fn server_info(&self) -> Value {
        let port = *self.port.lock().unwrap();
        let settings = self.settings();
        // `public_url`: the address people reach the server at (for links in messages; empty when not set).
        json!({"url": format!("{}://127.0.0.1:{port}", scheme()), "version": VERSION, "data_dir": self.data_dir.to_string_lossy(),
               "name": display_name(&settings), "public_url": settings.public_url.trim().trim_end_matches('/')})
    }

    /// `server_info` for one plugin: one that reads the library also gets the
    /// address of the library listing, with its own key.
    pub fn server_info_for(&self, pl: &plugins::Plugin) -> Value {
        let mut v = self.server_info();
        if pl.manifest.reads_library {
            let id = &pl.manifest.id;
            v["library"] = json!(format!("{}/api/plugins/{id}/library?key={}", v["url"].as_str().unwrap_or(""), self.plugin_key(id)));
        }
        v
    }

    /// The key that lets a plugin read the library (from the server's secret, so it stays the same after a restart).
    pub fn plugin_key(&self, id: &str) -> String {
        let secret = self.db.lock().unwrap().ticket_key.clone();
        crate::auth::token_hash(&format!("plugin-library:{secret}:{id}"))[..32].to_string()
    }

    /// Every movie and episode (versions too, extras not) with its files, and
    /// the libraries: what `GET /api/plugins/{id}/library` answers.
    pub fn library_listing(&self) -> Value {
        let db = self.db.lock().unwrap();
        let sections: Vec<Value> = db.sections.iter().map(|s| json!({"id": s.id, "name": s.name, "kind": s.kind.as_str(), "paths": s.paths})).collect();
        let mut ids: Vec<u64> = db.items.values().filter(|i| matches!(i.kind, ItemKind::Movie | ItemKind::Episode) && !i.is_extra() && !i.extra.contains_key(crate::scanner::MISSING)).map(|i| i.id).collect();
        ids.sort_unstable();
        let items: Vec<Value> = ids.iter().map(|id| {
            let mut o = detail_in(&db, *id);
            if let Some(v) = db.items.get(id).and_then(|i| i.version_of()) { o["version_of"] = json!(v); }
            o
        }).collect();
        json!({"sections": sections, "items": items})
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
        brief_in(&self.db.lock().unwrap(), id)
    }

    /// `item_brief` plus the item's files: path, size, container, length, the
    /// streams (kind, codec, language, channels, size, forced, SDH) and the
    /// subtitle files next to it. For plugins that work with the files
    /// themselves (subtitles, library checks).
    pub fn item_detail(&self, id: u64) -> Value {
        detail_in(&self.db.lock().unwrap(), id)
    }

    /// Tells plugins about something, in the background.
    pub fn fire(self: &Arc<Self>, event: &str, data: Value) {
        crate::trakt::on_event(self, event, &data);
        if self.listeners(event).is_empty() {
            return;
        }
        let (app, event) = (self.clone(), event.to_string());
        std::thread::spawn(move || app.fire_now(&event, data));
    }

    /// Looks for an item's subtitle files again (next to the video and in the subtitle store).
    pub fn refresh_subtitles(&self, id: u64) {
        let store = self.data_dir.join("subtitles");
        let changed = {
            let mut db = self.db.lock().unwrap();
            let Some(it) = db.items.get_mut(&id) else { return };
            let mut changed = false;
            if let Some(f) = it.file.as_mut() { changed |= crate::scanner::refresh_subtitles(f, &store); }
            for f in it.parts.iter_mut() { changed |= crate::scanner::refresh_subtitles(f, &store); }
            changed
        };
        if changed { self.save(); }
    }

    /// Sends `schedule.daily` if it was not sent yet today (UTC). True if it was sent now.
    pub fn daily_tick(self: &Arc<Self>) -> bool {
        let file = self.data_dir.join("plugins").join(".daily-last");
        let today = crate::db::today();
        let last = std::fs::read_to_string(&file).unwrap_or_default();
        if last.trim() == today {
            return false;
        }
        let _ = std::fs::create_dir_all(file.parent().unwrap());
        let _ = std::fs::write(&file, &today);
        self.fire("schedule.daily", json!({"date": today, "last": last.trim()}));
        true
    }

    /// Tells plugins about something, on this thread.
    pub fn fire_now(self: &Arc<Self>, event: &str, mut data: Value) {
        let listeners = self.listeners(event);
        if listeners.is_empty() {
            return;
        }
        // Playback events carry what the item is, so a plugin can look it up
        // elsewhere (Trakt, Last.fm …) without asking the server.
        if event.starts_with("playback.") && data.get("item").is_none() {
            if let Some(id) = data["id"].as_u64() {
                let item = self.item_brief(id);
                let dur = item["duration_ms"].as_u64().unwrap_or(0);
                if let (Some(t), true) = (data["time"].as_u64(), dur > 0) {
                    data["progress"] = json!(((t as f64 / dur as f64) * 1000.0).round().min(1000.0) / 10.0);
                }
                data["item"] = item;
            }
        }
        // Events name people by user name ("user", "username" when someone joins);
        // "user_name" is how they are shown to others, for messages.
        if data.get("user_name").is_none() {
            if let Some(login) = data["user"].as_str().or(data["username"].as_str()).map(String::from) {
                let shown = self.db.lock().unwrap().users.iter().find(|u| u.username == login).map(|u| u.display_name());
                if let Some(n) = shown { data["user_name"] = json!(n); }
            }
        }
        for (pl, settings) in listeners {
            let r = plugins::invoke(&pl, &settings, event, &data, &self.server_info_for(&pl));
            // A plugin that saved subtitles for the item: they are looked for again at once.
            if let (Ok(v), Some(id)) = (&r, data["id"].as_u64()) {
                if v["refresh_subtitles"] == json!(true) { self.refresh_subtitles(id); }
            }
            let mut p = self.plugins.lock().unwrap();
            match r {
                Ok(v) => p.note(&pl.manifest.id, event, true, short(&v.to_string())),
                Err(e) => p.note(&pl.manifest.id, event, false, e),
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
            // One after the other on one thread: a first scan of 20,000 titles must not start 20,000 plugin programs.
            let (app, added, removed) = (self.clone(), report.added.clone(), report.removed.clone());
            let finished = json!({"section_id": section_id, "added": report.added.len(), "updated": report.updated.len(), "removed": report.removed.len(), "files": report.files_seen});
            std::thread::spawn(move || {
                for id in &added {
                    let title = app.db.lock().unwrap().items.get(id).map(|i| i.title.clone()).unwrap_or_default();
                    app.fire_now("library.item.added", json!({"id": id, "title": title, "section_id": section_id, "item": app.item_detail(*id)}));
                }
                for id in &removed {
                    app.fire_now("library.item.removed", json!({"id": id, "section_id": section_id}));
                }
                app.fire_now("library.scan.finished", finished);
            });
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

pub fn short(s: &str) -> String {
    match s.char_indices().nth(300) {
        Some((i, _)) => format!("{}…", &s[..i]),
        None => s.to_string(),
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::db::MediaFile;

    #[test]
    fn a_relayed_caller_claiming_a_home_address_is_not_local() {
        use std::io::Write;
        let root = std::env::temp_dir().join(format!("lichtspiel-relayed-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let app = App::open(&root);
        {
            let mut db = app.db.lock().unwrap();
            db.settings.no_auth_networks = "192.168.0.0/16".into();
            db.users.push(serde_json::from_value(json!({"id": 1, "username": "admin", "password_hash": "x", "salt": "x", "admin": true, "created_at": 0})).unwrap());
        }
        let server = tiny_http::Server::http("127.0.0.1:0").unwrap();
        let mut c = std::net::TcpStream::connect(server.server_addr().to_ip().unwrap()).unwrap();
        // The relay (or someone pretending to be it) says this caller is at home.
        crate::relay::RELAYED.lock().unwrap().get_or_insert_with(HashMap::new).insert(c.local_addr().unwrap(), "192.168.1.10".parse().unwrap());
        write!(c, "GET /api/settings HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n").unwrap();
        handle(app.clone(), server.recv().unwrap());
        let mut got = String::new();
        let _ = c.read_to_string(&mut got);
        crate::relay::RELAYED.lock().unwrap().as_mut().unwrap().remove(&c.local_addr().unwrap());
        assert!(got.starts_with("HTTP/1.1 401"), "a relayed caller got in without signing in: {}", got.lines().next().unwrap_or(""));
        let _ = std::fs::remove_dir_all(&root);
    }

    /// Sends one raw request through `handle` and returns the whole answer.
    pub(crate) fn ask(app: &Arc<App>, raw: &str) -> String {
        use std::io::Write;
        let server = tiny_http::Server::http("127.0.0.1:0").unwrap();
        let mut c = std::net::TcpStream::connect(server.server_addr().to_ip().unwrap()).unwrap();
        c.write_all(raw.as_bytes()).unwrap();
        handle(app.clone(), server.recv().unwrap());
        let mut got = String::new();
        let _ = c.read_to_string(&mut got);
        got
    }

    #[test]
    fn sign_in_tokens_are_stored_only_as_a_hash() {
        let (app, root) = test_app("token-hash");
        let (new, old) = {
            let mut db = app.db.lock().unwrap();
            // A token saved by an older version, as it was given out.
            let old = crate::db::random_hex(20);
            db.tokens.push(crate::db::Token { token: old.clone(), user_id: 2, created_at: 0, device: String::new(), last_used: 0, last_device: String::new(), last_ip: String::new() });
            (crate::handlers::new_token(&mut db, 1, "test"), old)
        };
        app.save();
        drop(app);
        let app = App::open(&root);
        app.save();
        let stored = serde_json::to_string(&app.db.lock().unwrap().tokens).unwrap();
        assert!(!stored.contains(&new) && !stored.contains(&old), "a sign-in token is stored as it is: {stored}");
        for (t, who) in [(&new, "admin"), (&old, "kid")] {
            let r = ask(&app, &format!("GET /api/me HTTP/1.1\r\nHost: x\r\nX-Token: {t}\r\nConnection: close\r\n\r\n"));
            assert!(r.contains(&format!("\"username\":\"{who}\"")), "the {who} token stopped working: {r}");
        }
        // The list of devices names the current one, and it can be signed out by its id.
        let list = ask(&app, &format!("GET /api/tokens HTTP/1.1\r\nHost: x\r\nX-Token: {new}\r\nConnection: close\r\n\r\n"));
        assert!(list.contains(&format!("\"id\":\"{}\"", crate::auth::token_id(&new))) && list.contains("\"current\":true"), "{list}");
        let _ = ask(&app, &format!("POST /api/logout HTTP/1.1\r\nHost: x\r\nX-Token: {old}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"));
        assert_eq!(app.db.lock().unwrap().tokens.len(), 1, "signing out did not end the old token");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn device_names_are_kept_short() {
        let long = "x".repeat(5000);
        assert!(device_label(&long, "", &[], "").chars().count() <= 100);
        assert!(device_label("", "", &[], &long).chars().count() <= 100);
        let (app, root) = test_app("device-name");
        let mut db = app.db.lock().unwrap();
        crate::handlers::new_token(&mut db, 1, &long);
        assert!(db.tokens[0].device.chars().count() <= 100, "a sign-in kept a {} letter device name", db.tokens[0].device.len());
        drop(db);
        let _ = std::fs::remove_dir_all(&root);
    }

    /// A data folder with a first admin (id 1) and a profile without PIN (id 2) on the sign-in screen.
    pub(crate) fn test_app(name: &str) -> (Arc<App>, PathBuf) {
        let root = std::env::temp_dir().join(format!("lichtspiel-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let app = App::open(&root);
        {
            let mut db = app.db.lock().unwrap();
            db.settings.home_profiles = true;
            db.users.push(serde_json::from_value(json!({"id": 1, "username": "admin", "password_hash": "x", "salt": "x", "admin": true, "created_at": 0})).unwrap());
            db.users.push(serde_json::from_value(json!({"id": 2, "username": "kid", "password_hash": "x", "salt": "x", "admin": false, "home": true, "created_at": 0})).unwrap());
        }
        (app, root)
    }

    #[test]
    fn the_web_app_page_cannot_be_framed_and_runs_only_its_own_scripts() {
        let (app, root) = test_app("headers");
        let got = ask(&app, "GET / HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n");
        let head = got.split("\r\n\r\n").next().unwrap().to_ascii_lowercase();
        for want in ["x-frame-options: deny", "x-content-type-options: nosniff", "referrer-policy: no-referrer", "frame-ancestors 'none'", "object-src 'none'"] {
            assert!(head.contains(want), "missing {want}: {head}");
        }
        let csp = head.lines().find(|l| l.starts_with("content-security-policy:")).unwrap();
        let script = csp.split(';').find(|d| d.trim().starts_with("script-src")).unwrap();
        assert!(!script.contains("unsafe-inline"), "any inline script may run: {script}");
        // The page's own inline script is allowed by its hash.
        let inline = WEB_APP.split("<script>").nth(1).unwrap().split("</script>").next().unwrap();
        assert!(script.contains(&inline_script_hash(inline).to_ascii_lowercase()), "{script}");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[cfg(unix)]
    #[test]
    fn files_outside_the_library_folders_are_never_sent() {
        let (app, root) = test_app("outside");
        let media = root.join("media");
        std::fs::create_dir_all(media.join("Film")).unwrap();
        std::fs::write(root.join("secret.txt"), "WEBVTT\n\ntop secret").unwrap();
        std::fs::write(media.join("Film/Film.mkv"), "video").unwrap();
        std::fs::write(media.join("Film/Film.de.vtt"), "WEBVTT\n\nhallo").unwrap();
        std::os::unix::fs::symlink(root.join("secret.txt"), media.join("Film/Film.en.vtt")).unwrap();
        std::os::unix::fs::symlink(root.join("secret.txt"), media.join("Film/Leak.mkv")).unwrap();
        let tok = {
            let mut db = app.db.lock().unwrap();
            db.sections.push(serde_json::from_value(json!({"id": 1, "name": "Filme", "kind": "movie", "paths": [media.to_string_lossy()], "created_at": 0})).unwrap());
            for (id, f) in [(1u64, "Film/Film.mkv"), (2, "Film/Leak.mkv")] {
                let mut i: Item = serde_json::from_value(json!({"id": id, "section_id": 1, "kind": "movie", "title": "x", "sort_title": "x", "added_at": 0, "updated_at": 0})).unwrap();
                let subs = vec![media.join("Film/Film.de.vtt").to_string_lossy().to_string(), media.join("Film/Film.en.vtt").to_string_lossy().to_string()];
                i.file = Some(MediaFile { path: media.join(f).to_string_lossy().to_string(), container: "matroska".into(), external_subtitles: subs, ..Default::default() });
                db.items.insert(id, i);
            }
            crate::handlers::new_token(&mut db, 2, "test")
        };
        let get = |path: &str| ask(&app, &format!("GET {path} HTTP/1.1\r\nHost: x\r\nX-Token: {tok}\r\nConnection: close\r\n\r\n"));
        assert!(get("/library/parts/1/subtitles/x0").contains("hallo"), "a subtitle in the library is not sent");
        assert!(get("/library/parts/1/file").ends_with("video"), "a video in the library is not sent");
        assert!(!get("/library/parts/1/subtitles/x1").contains("top secret"), "a subtitle linked to a file outside the library was sent");
        assert!(!get("/library/parts/2/file").contains("top secret"), "a video linked to a file outside the library was sent");
        let _ = std::fs::remove_dir_all(&root);
    }

    /// A tiny web server on this computer that counts its callers and answers with `answer`.
    pub(crate) fn local_web(answer: &'static [u8]) -> (String, Arc<std::sync::atomic::AtomicUsize>) {
        use std::io::Write;
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = l.local_addr().unwrap().to_string();
        let hits = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let h = hits.clone();
        std::thread::spawn(move || for mut s in l.incoming().flatten() {
            h.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            let mut buf = [0u8; 2048];
            let _ = s.read(&mut buf);
            let _ = s.write_all(answer);
        });
        (addr, hits)
    }

    #[test]
    fn a_picture_address_may_not_reach_this_computer_or_the_home_network() {
        let (app, root) = test_app("ssrf");
        let (addr, hits) = local_web(b"HTTP/1.1 200 OK\r\nContent-Length: 8\r\nConnection: close\r\n\r\n\x89PNGxxxx");
        {
            let mut db = app.db.lock().unwrap();
            let i: Item = serde_json::from_value(json!({"id": 1, "section_id": 1, "kind": "movie", "title": "x", "sort_title": "x", "added_at": 0, "updated_at": 0})).unwrap();
            db.items.insert(1, i);
        }
        let r = crate::handlers::set_image(&app, "1", "poster", &json!({"url": format!("http://{addr}/p.png")}));
        assert_ne!(r.status_code().0, 200);
        assert_eq!(hits.load(std::sync::atomic::Ordering::SeqCst), 0, "the server fetched a picture from this computer");
        for blocked in ["127.0.0.1:80", "10.1.2.3:80", "192.168.1.1:80", "172.16.0.1:80", "169.254.169.254:80", "100.64.0.1:80", "0.0.0.0:80", "[::1]:80", "[fe80::1]:80", "[fd00::1]:80", "[::ffff:192.168.1.1]:80"] {
            assert!(public_only(blocked).is_err(), "{blocked} is allowed");
        }
        assert!(public_only("8.8.8.8:443").is_ok());
        let _ = std::fs::remove_dir_all(&root);
    }

    /// test_app plus one movie (id 1) in library 1, which the profile "kid" may not see; tokens for admin and kid.
    fn app_with_hidden_movie(name: &str) -> (Arc<App>, PathBuf, String, String) {
        let (app, root) = test_app(name);
        let mut db = app.db.lock().unwrap();
        db.sections.push(serde_json::from_value(json!({"id": 1, "name": "Filme", "kind": "movie", "paths": [root.join("media").to_string_lossy()], "created_at": 0})).unwrap());
        let i: Item = serde_json::from_value(json!({"id": 1, "section_id": 1, "kind": "movie", "title": "x", "sort_title": "x", "added_at": 0, "updated_at": 0})).unwrap();
        db.items.insert(1, i);
        db.users[1].allowed_sections = Some(vec![]);
        let (a, k) = (crate::handlers::new_token(&mut db, 1, "test"), crate::handlers::new_token(&mut db, 2, "test"));
        drop(db);
        (app, root, a, k)
    }

    #[test]
    fn a_profile_cannot_mark_rate_or_favourite_what_it_may_not_see() {
        let (app, root, _, kid) = app_with_hidden_movie("can-see");
        for (m, path) in [("PUT", "/:/scrobble?key=1"), ("PUT", "/:/unscrobble?key=1"), ("PUT", "/:/rate?key=1&rating=8"), ("PUT", "/api/items/1/favorite"),
            ("POST", "/api/continue/remove?x=1")] {
            let body = if path.starts_with("/api/continue") { r#"{"ids":[1]}"# } else { "" };
            let got = ask(&app, &format!("{m} {path} HTTP/1.1\r\nHost: x\r\nX-Token: {kid}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()));
            assert!(got.starts_with("HTTP/1.1 404"), "{m} {path}: {}", got.lines().next().unwrap_or(""));
        }
        let db = app.db.lock().unwrap();
        assert!(db.views.get("2:1").is_none(), "the profile wrote watch state for a hidden movie");
        drop(db);
        let _ = std::fs::remove_dir_all(&root);
    }

    /// The routes touched by the security fixes need sign-in or admin: anonymous, a profile and the admin.
    #[test]
    fn routes_need_sign_in_or_admin() {
        let (app, root, admin, kid) = app_with_hidden_movie("access");
        app.db.lock().unwrap().users[1].allowed_sections = None;
        // (method, path, anonymous, profile, admin): the status each one gets.
        let table: &[(&str, &str, u16, u16, u16)] = &[
            ("GET", "/api/home", 200, 200, 200),
            ("PUT", "/:/scrobble?key=1", 401, 200, 200),
            ("PUT", "/:/unscrobble?key=1", 401, 200, 200),
            ("PUT", "/:/rate?key=1&rating=8", 401, 200, 200),
            ("PUT", "/api/items/1/favorite", 401, 200, 200),
            ("GET", "/library/parts/1/subtitles/x0", 401, 400, 400),
            ("GET", "/library/parts/1/file", 401, 400, 400),
            ("PUT", "/library/metadata/1/image/poster", 401, 403, 400),
            ("GET", "/api/settings", 401, 403, 200),
            ("GET", "/api/plex-sync", 401, 403, 200),
        ];
        for &(m, path, want_anon, want_kid, want_admin) in table {
            for (who, tok, want) in [("anonymous", "", want_anon), ("profile", kid.as_str(), want_kid), ("admin", admin.as_str(), want_admin)] {
                let got = ask(&app, &format!("{m} {path} HTTP/1.1\r\nHost: x\r\nX-Token: {tok}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"));
                assert!(got.starts_with(&format!("HTTP/1.1 {want}")), "{m} {path} as {who}: {}", got.lines().next().unwrap_or(""));
            }
        }
        let _ = std::fs::remove_dir_all(&root);
    }

    /// Every address in ROUTES needs sign-in unless it is on the short open list below, and every
    /// address described as "Admin:" refuses a signed-in person who is not an admin.
    #[test]
    fn every_route_needs_sign_in_and_every_admin_route_needs_an_admin() {
        const OPEN: &[(&str, &str)] = &[
            ("GET", "/api/status"), ("POST", "/api/setup"), ("POST", "/api/login"), ("POST", "/api/link"), ("GET", "/api/link/{id}"),
            ("GET", "/api/home"), ("POST", "/api/home/{id}/login"), ("GET", "/identity"),
            ("GET", "/api/invites/{code}"), ("POST", "/api/invites/{code}/accept"), ("GET", "/rss/watchlist/{key}.xml"),
            // A plugin's own key instead of a sign-in (it is refused without it, see the test of reads_library).
            ("GET", "/api/plugins/{id}/library"),
        ];
        for open in OPEN {
            assert!(crate::routes::ROUTES.iter().any(|r| (r.0, r.1) == *open), "{open:?} is on the open list but not in ROUTES");
        }
        let (app, root, _, kid) = app_with_hidden_movie("every-route");
        let fill = |path: &str| path.replace("{id}", "1").replace("{itemId}", "1").replace("{n}", "0").replace("{key}", "x").replace("{code}", "x").replace("{action}", "x");
        let call = |m: &str, path: &str, tok: &str| {
            let got = ask(&app, &format!("{m} {path} HTTP/1.1\r\nHost: x\r\nX-Token: {tok}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"));
            got.lines().next().unwrap_or("").to_string()
        };
        let mut wrong = Vec::new();
        for &(m, path, what) in crate::routes::ROUTES {
            let at = fill(path);
            if !OPEN.contains(&(m, path)) {
                let got = call(m, &at, "");
                if !got.starts_with("HTTP/1.1 401") { wrong.push(format!("{m} {path} without sign-in: {got}")); }
            }
            if what.starts_with("Admin:") {
                let got = call(m, &at, &kid);
                if !got.starts_with("HTTP/1.1 403") { wrong.push(format!("{m} {path} as a profile: {got}")); }
            }
        }
        let _ = std::fs::remove_dir_all(&root);
        assert!(wrong.is_empty(), "{} routes let the wrong person in:\n{}", wrong.len(), wrong.join("\n"));
    }

    /// Video, subtitle and picture addresses carry a short-lived ticket instead of the sign-in token.
    #[test]
    fn an_origin_is_ours_when_it_names_the_host_or_a_trusted_proxys_forwarded_host() {
        assert!(!origin_is_other_site("http://nas:8484", "nas:8484", &[]));
        assert!(origin_is_other_site("http://evil.com", "nas:8484", &[]));
        assert!(origin_is_other_site("null", "nas:8484", &[]));
        // Behind a proxy that rewrites Host, the address the browser used counts.
        assert!(!origin_is_other_site("http://films.home", "127.0.0.1:8484", &["films.home".to_string()]));
        assert!(origin_is_other_site("http://evil.com", "127.0.0.1:8484", &["films.home".to_string()]));
    }

    #[test]
    fn a_ticket_opens_only_its_own_item_while_the_sign_in_lasts() {
        let (app, root) = test_app("ticket");
        let media = root.join("media");
        std::fs::create_dir_all(media.join("Film")).unwrap();
        std::fs::write(media.join("Film/A.mkv"), "video-a").unwrap();
        std::fs::write(media.join("Film/B.mkv"), "video-b").unwrap();
        let (tok, kid) = {
            let mut db = app.db.lock().unwrap();
            db.sections.push(serde_json::from_value(json!({"id": 1, "name": "Filme", "kind": "movie", "paths": [media.to_string_lossy()], "created_at": 0})).unwrap());
            for (id, f) in [(1u64, "Film/A.mkv"), (2, "Film/B.mkv")] {
                let mut i: Item = serde_json::from_value(json!({"id": id, "section_id": 1, "kind": "movie", "title": "x", "sort_title": "x", "added_at": 0, "updated_at": 0})).unwrap();
                i.file = Some(MediaFile { path: media.join(f).to_string_lossy().to_string(), container: "matroska".into(), ..Default::default() });
                db.items.insert(id, i);
            }
            (crate::handlers::new_token(&mut db, 1, "test"), crate::handlers::new_token(&mut db, 2, "test"))
        };
        let get = |path: &str, extra: &str| ask(&app, &format!("GET {path} HTTP/1.1\r\nHost: x\r\n{extra}Connection: close\r\n\r\n"));
        let status = |r: &str| r.lines().next().unwrap_or("").to_string();
        let ticket_of = |r: &str| { let body = r.split("\r\n\r\n").nth(1).unwrap_or(""); serde_json::from_str::<Value>(body).ok().and_then(|v| v["ticket"].as_str().map(String::from)).unwrap_or_default() };
        let t1 = ticket_of(&get("/api/ticket?item=1", &format!("X-Token: {tok}\r\n")));
        assert!(!t1.is_empty() && !t1.contains(&tok), "no ticket for item 1: {t1}");
        // The ticket plays item 1, also in pieces (seeking)...
        assert!(get(&format!("/library/parts/1/file?ticket={t1}"), "").ends_with("video-a"));
        assert!(status(&get(&format!("/library/parts/1/file?ticket={t1}"), "Range: bytes=0-1\r\n")).contains("206"));
        // ...but nothing else.
        for p in ["/library/parts/2/file", "/video/transcode/2/stream.mp4", "/api/me", "/api/tokens"] {
            assert!(status(&get(&format!("{p}?ticket={t1}"), "")).contains("401"), "{p} opened with item 1's ticket");
        }
        // A changed ticket, an expired one and one for another item with item 1's signature are refused.
        let key = app.db.lock().unwrap().ticket_key.clone();
        let tampered = format!("{}{}", &t1[..t1.len() - 1], if t1.ends_with('0') { '1' } else { '0' });
        let (old, _) = crate::auth::ticket(&key, 1, "1", Some(&tok), now() - 7 * 3600);
        let swapped = t1.replacen(".1.", ".2.", 1);
        for t in [tampered, old, swapped.clone(), "1.2.3".into(), String::new()] {
            assert!(status(&get(&format!("/library/parts/1/file?ticket={t}"), "")).contains("401"), "ticket {t} was let in");
            assert!(status(&get(&format!("/library/parts/2/file?ticket={t}"), "")).contains("401"), "ticket {t} was let in");
        }
        // A ticket made without a token (a network that needs no sign-in) still opens its item.
        let (nt, _) = crate::auth::ticket(&key, 1, "1", None, now());
        assert!(get(&format!("/library/parts/1/file?ticket={nt}"), "").ends_with("video-a"), "a ticket without a sign-in was refused");
        // A profile that may not see the movie gets no ticket, and a made-up one does not show it either.
        app.db.lock().unwrap().users[1].allowed_sections = Some(vec![]);
        assert!(status(&get("/api/ticket?item=1", &format!("X-Token: {kid}\r\n"))).contains("404"));
        let (kt, _) = crate::auth::ticket(&key, 2, "1", Some(&kid), now());
        assert!(!get(&format!("/library/parts/1/file?ticket={kt}"), "").ends_with("video-a"), "a hidden movie was sent");
        // The picture ticket opens pictures, not videos; a ticket for any other address is not given.
        let tp = ticket_of(&get("/api/ticket", &format!("X-Token: {tok}\r\n")));
        assert!(!status(&get(&format!("/library/metadata/1/thumb?ticket={tp}"), "")).contains("401"));
        assert!(status(&get(&format!("/library/parts/1/file?ticket={tp}"), "")).contains("401"));
        assert!(status(&get("/api/ticket?path=/api/tokens", &format!("X-Token: {tok}\r\n"))).contains("400"));
        // The play decision hands out ticket addresses, not the sign-in token.
        let d = get("/library/metadata/1/decision", &format!("X-Token: {tok}\r\n"));
        assert!(d.contains("ticket=") && !d.contains(&tok), "{d}");
        // The token in the address is not taken, and signing out ends the tickets too.
        assert!(status(&get(&format!("/library/parts/1/file?token={tok}"), "")).contains("401"), "?token= in the address was taken");
        let _ = ask(&app, &format!("POST /api/logout HTTP/1.1\r\nHost: x\r\nX-Token: {tok}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"));
        assert!(status(&get(&format!("/library/parts/1/file?ticket={t1}"), "")).contains("401"), "a ticket outlived its sign-in");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn other_websites_cannot_read_the_answers() {
        let (app, root) = test_app("cors");
        let got = ask(&app, "GET /identity HTTP/1.1\r\nHost: x\r\nOrigin: https://evil.example\r\nConnection: close\r\n\r\n");
        assert!(got.starts_with("HTTP/1.1 200"), "{got}");
        assert!(!got.to_ascii_lowercase().contains("access-control-allow-origin"), "another website may read the answer: {got}");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn other_websites_cannot_pick_a_profile_or_use_the_no_sign_in_network() {
        let (app, root) = test_app("csrf");
        app.db.lock().unwrap().settings.no_auth_networks = "127.0.0.1".into();
        let login = |site: &str| ask(&app, &format!("POST /api/home/2/login HTTP/1.1\r\nHost: x\r\nSec-Fetch-Site: {site}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"));
        assert!(login("cross-site").starts_with("HTTP/1.1 403"), "a page of another website got a profile's token");
        assert!(login("same-origin").starts_with("HTTP/1.1 200"), "the web app itself can't pick a profile");
        // A no-sign-in network makes the web app an admin, but not a link or picture on another website.
        let settings = |site: &str| ask(&app, &format!("GET /api/settings HTTP/1.1\r\nHost: x\r\nSec-Fetch-Site: {site}\r\nConnection: close\r\n\r\n"));
        assert!(settings("same-origin").starts_with("HTTP/1.1 200"));
        assert!(settings("cross-site").starts_with("HTTP/1.1 401"), "another website acted as the admin");
        // Over plain http browsers send no Sec-Fetch-Site, but they do send Origin with a post.
        let post = |origin: &str| ask(&app, &format!("POST /api/home/2/login HTTP/1.1\r\nHost: x:8484\r\n{origin}Content-Length: 0\r\nConnection: close\r\n\r\n"));
        assert!(post("Origin: http://evil.example\r\n").starts_with("HTTP/1.1 403"), "a form on another website was let through over http");
        assert!(post("Origin: http://X:8484\r\n").starts_with("HTTP/1.1 200"), "the web app itself was refused over http");
        assert!(post("").starts_with("HTTP/1.1 200"), "an app that sends no Origin was refused");
        let _ = std::fs::remove_dir_all(&root);
    }
}

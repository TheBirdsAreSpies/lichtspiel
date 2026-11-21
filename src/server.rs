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

    /// Where a person's shared photo is kept.
    pub fn person_photo(&self, key: &str) -> PathBuf {
        self.data_dir.join("metadata").join("people").join(format!("{key}.jpg"))
    }

    /// Deletes saved posters, backgrounds, stills and actor photos that aren't
    /// real photos. Returns the movies and shows they belonged to.
    pub fn remove_broken_pictures(&self) -> Vec<u64> {
        // Only our own saved pictures: files in the user's media folders are never touched.
        let bad = |p: &str| Path::new(p).starts_with(&self.data_dir) && Path::new(p).is_file() && crate::metadata::file_image_type(Path::new(p)).is_none();
        let mut tops = HashSet::new();
        let mut db = self.db.lock().unwrap();
        let ids: Vec<u64> = db.items.values().filter(|it| it.poster.as_deref().is_some_and(bad) || it.extra.get("art").and_then(|v| v.as_str()).is_some_and(bad)).map(|it| it.id).collect();
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

    /// Asks metadata plugins about new items and merges their answers.
    pub fn lookup_metadata(self: &Arc<Self>, ids: &[u64]) {
        let listeners = self.listeners("metadata.lookup");
        if listeners.is_empty() {
            return;
        }
        for id in ids {
            let data = {
                let db = self.db.lock().unwrap();
                let Some(it) = db.items.get(id) else { continue };
                let mut v = serde_json::to_value(it).unwrap_or(Value::Null);
                // The library's name, so a plugin can be meant for some libraries only (e.g. "Anime").
                if let Some(s) = db.sections.iter().find(|s| s.id == it.section_id) {
                    v["section_name"] = json!(s.name);
                }
                if let Some(parent) = it.parent_id.and_then(|p| db.items.get(&p)) {
                    v["parent_title"] = json!(parent.title);
                    if let Some(gp) = parent.parent_id.and_then(|g| db.items.get(&g)) {
                        v["show_title"] = json!(gp.title);
                    }
                }
                v
            };
            for (pl, settings) in &listeners {
                match plugins::invoke(pl, settings, "metadata.lookup", &data, &self.server_info_for(pl)) {
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
        let before = q.len();
        for id in tops {
            if !q.iter().any(|(i, _)| *i == id) {
                q.push_back((id, force));
            }
        }
        let added = q.len() > before;
        drop(q);
        if added {
            self.yield_to_details();
        }
    }

    /// Details come before credits and intros: an automatic detection that is running steps aside after
    /// the file it is on and starts again once no details are waiting (the scheduler does that).
    pub fn yield_to_details(&self) {
        if !self.job_auto.load(AtomicOrdering::SeqCst) {
            return;
        }
        let detecting = self.job.lock().unwrap().as_ref().is_some_and(|j| j.0 == "Detecting credits" || j.0 == "Detecting intros");
        if detecting {
            self.job_yield.store(true, AtomicOrdering::SeqCst);
            self.job_cancel.store(true, AtomicOrdering::SeqCst);
        }
    }

    /// "Detect credits": looks for end credits in the given items' files (on a
    /// background thread, shown as a job in the web app): chapters named like
    /// "Credits" first, else the picture of the last part of the video. It skips libraries with
    /// credits detection off and, unless `force`, files that are already up to
    /// date or failed more than 3 times. Returns how many videos it will check.
    pub fn detect_credits(self: &Arc<Self>, ids: Vec<u64>, force: bool) -> Result<usize, String> {
        if force {
            self.auto_paused.store(false, AtomicOrdering::SeqCst);
        }
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
        self.job_yield.store(false, AtomicOrdering::SeqCst);
        self.job_auto.store(!force, AtomicOrdering::SeqCst);
        drop(job);
        self.set_job_items("Detecting credits", &files.iter().map(|f| f.0).collect::<Vec<_>>());
        let app = self.clone();
        std::thread::spawn(move || {
            let (found, failed) = (std::sync::atomic::AtomicUsize::new(0), std::sync::atomic::AtomicUsize::new(0));
            // The old Plex server's markers first (same file path): no need to search the picture for those.
            let old_db = crate::migrate::detect();
            if old_db.is_none() {
                elog!("credits detection: no old Plex database found (set OLD_SERVER), so every video is searched");
            }
            let from_plex = old_db
                .and_then(|p| crate::migrate::credits_by_path(&p, &app.data_dir.join("cache").join("old-markers")).map_err(|e| elog!("old server's credits markers: {e}")).ok())
                .unwrap_or_default();
            let taken = std::sync::atomic::AtomicUsize::new(0);
            elog!("credits detection: {} files with credits markers in the old server's database; {} videos to check", from_plex.len(), total);
            // Several files at once: each one is a separate ffmpeg that mostly waits for the disk.
            let (next, done) = (std::sync::atomic::AtomicUsize::new(0), std::sync::atomic::AtomicUsize::new(0));
            let workers = std::thread::available_parallelism().map_or(2, |n| n.get()).clamp(2, 4);
            std::thread::scope(|sc| {
                for _ in 0..workers {
                    sc.spawn(|| loop {
                        if app.job_cancelled() {
                            break;
                        }
                        let n = next.fetch_add(1, AtomicOrdering::SeqCst);
                        let Some((id, path, duration)) = files.get(n) else { break };
                        let id = *id;
                        let r = match from_plex.get(path) {
                            Some(c) if !force => {
                                let t = taken.fetch_add(1, AtomicOrdering::SeqCst) + 1;
                                if t % 1000 == 0 {
                                    elog!("credits detection: {t} videos took their markers from the old server so far");
                                }
                                Ok((c.clone(), "imported"))
                            }
                            _ => crate::scanner::find_credits(&ffmpeg, &ffprobe, path, *duration, picture),
                        };
                        if let Some(f) = app.db.lock().unwrap().items.get_mut(&id).and_then(|i| i.last_file_mut()) {
                            match r {
                                Ok((c, how)) => {
                                    found.fetch_add(usize::from(!c.is_empty()), AtomicOrdering::SeqCst);
                                    f.credits_ms = c;
                                    f.credits_source = how.into();
                                    f.credits_checked = db::CREDITS_DETECTOR_VERSION;
                                    f.credits_errors = 0;
                                }
                                Err(e) => {
                                    failed.fetch_add(1, AtomicOrdering::SeqCst);
                                    f.credits_errors += 1;
                                    elog!("credits detection for item {id}: {e}");
                                    if f.credits_errors > db::CREDITS_MAX_ERRORS {
                                        elog!("credits detection for item {id} has failed too many times; it will not be retried automatically");
                                    }
                                }
                            }
                        }
                        let d = done.fetch_add(1, AtomicOrdering::SeqCst) + 1;
                        *app.job.lock().unwrap() = Some(("Detecting credits".into(), d, total));
                        // Kept now and then, so a restart during a long first run loses little.
                        if d % 50 == 0 { app.save(); }
                    });
                }
            });
            let (found, failed) = (found.into_inner(), failed.into_inner());
            elog!("credits detection ended: {} of {total} videos took the old server's markers, the rest was searched in the picture", taken.into_inner());
            app.save();
            *app.job.lock().unwrap() = None;
            let cancelled = app.job_cancel.swap(false, AtomicOrdering::SeqCst);
            app.job_auto.store(false, AtomicOrdering::SeqCst);
            // Stopped for the details: no result to report, the scheduler takes it up again later.
            if app.job_yield.swap(false, AtomicOrdering::SeqCst) {
                return;
            }
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
            if self.job_auto.load(AtomicOrdering::SeqCst) {
                self.auto_paused.store(true, AtomicOrdering::SeqCst);
            }
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

    /// A readable name for an item in lists of work: "Show · S1E2 Title" for an episode, else its title.
    pub fn label_of(db: &Db, id: u64) -> String {
        let Some(it) = db.items.get(&id) else { return format!("#{id}") };
        match it.kind {
            ItemKind::Episode => {
                let season = it.parent_id.and_then(|p| db.items.get(&p));
                let show = season.and_then(|s| s.parent_id).and_then(|p| db.items.get(&p));
                format!("{} \u{b7} S{}E{} {}", show.map(|s| s.title.as_str()).unwrap_or(""), season.and_then(|s| s.index).unwrap_or(0), it.index.unwrap_or(0), it.title)
            }
            ItemKind::Season => {
                let show = it.parent_id.and_then(|p| db.items.get(&p));
                format!("{} \u{b7} {}", show.map(|s| s.title.as_str()).unwrap_or(""), it.title)
            }
            _ => it.title.clone(),
        }
    }

    /// Remembers what the job `label` is going to work through, for the activity panel.
    pub fn set_job_items(&self, label: &str, ids: &[u64]) {
        let names: Vec<String> = {
            let db = self.db.lock().unwrap();
            ids.iter().map(|i| Self::label_of(&db, *i)).collect()
        };
        *self.job_items.lock().unwrap() = (label.to_string(), names);
    }

    pub fn job_cancelled(&self) -> bool {
        self.job_cancel.load(AtomicOrdering::SeqCst)
    }

    pub fn detect_intros(self: &Arc<Self>, ids: Vec<u64>, force: bool) -> Result<usize, String> {
        if force {
            self.auto_paused.store(false, AtomicOrdering::SeqCst);
        }
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
        self.job_yield.store(false, AtomicOrdering::SeqCst);
        self.job_auto.store(!force, AtomicOrdering::SeqCst);
        drop(job);
        self.set_job_items("Detecting intros", &seasons.iter().flat_map(|s| s.iter().map(|e| e.0)).collect::<Vec<_>>());
        let app = self.clone();
        let cache = self.data_dir.join("cache").join("intro");
        std::thread::spawn(move || {
            let (mut done, mut found, mut failed) = (0, 0, 0);
            // The old Plex server's intro markers first (same file path): those episodes are not listened to.
            let from_plex = crate::migrate::detect()
                .and_then(|p| crate::migrate::intros_by_path(&p, &app.data_dir.join("cache").join("old-markers")).map_err(|e| elog!("old server's intro markers: {e}")).ok())
                .unwrap_or_default();
            elog!("intro detection: {} files with intro markers in the old server's database; {total} episodes to check", from_plex.len());
            let mut taken = 0;
            'seasons: for eps in seasons {
                let (mut known, mut eps, compare_only) = split_known(eps, |e| from_plex.get(&e.1).is_some_and(|m| m.len() == 2));
                {
                    let mut db = app.db.lock().unwrap();
                    for (id, path, ..) in &known {
                        let Some(f) = db.items.get_mut(id).and_then(|i| i.file.as_mut()) else { continue };
                        f.intro_ms = from_plex.get(path).cloned().unwrap_or_default();
                        f.intro_checked = intro::DETECTOR_VERSION;
                        f.intro_errors = 0;
                        taken += 1;
                        found += 1;
                    }
                    // A single episode left has nothing to be compared with.
                    if eps.len() < 2 {
                        for (id, ..) in &eps {
                            if let Some(f) = db.items.get_mut(id).and_then(|i| i.file.as_mut()) {
                                f.intro_ms.clear();
                                f.intro_checked = intro::DETECTOR_VERSION;
                                f.intro_errors = 0;
                            }
                        }
                    }
                }
                done += known.len() - compare_only;
                known.clear();
                *app.job.lock().unwrap() = Some(("Detecting intros".into(), done, total));
                if eps.len() < 2 {
                    done += eps.len();
                    eps.clear();
                    continue;
                }
                // Several episodes at once: each one is a separate ffmpeg that mostly waits for the disk or the network.
                let results: Vec<std::sync::Mutex<Option<Result<Vec<u32>, String>>>> = eps.iter().map(|_| std::sync::Mutex::new(None)).collect();
                let (next, finished) = (std::sync::atomic::AtomicUsize::new(0), std::sync::atomic::AtomicUsize::new(0));
                let workers = std::thread::available_parallelism().map_or(2, |n| n.get()).clamp(2, 4);
                std::thread::scope(|sc| {
                    for _ in 0..workers {
                        sc.spawn(|| loop {
                            // A season is only judged when all its episodes were listened to.
                            if app.job_cancelled() {
                                break;
                            }
                            let n = next.fetch_add(1, AtomicOrdering::SeqCst);
                            let Some((_, path, size, modified)) = eps.get(n) else { break };
                            let file = cache.join(intro::cache_name(path, *size, *modified));
                            let print = match intro::read_cached(&file) {
                                Some(p) => Ok(p),
                                None => intro::fingerprint_file(&ffmpeg, path).inspect(|p| intro::write_cached(&file, p)),
                            };
                            *results[n].lock().unwrap() = Some(print);
                            let d = finished.fetch_add(1, AtomicOrdering::SeqCst) + 1;
                            *app.job.lock().unwrap() = Some(("Detecting intros".into(), (done + d.saturating_sub(compare_only)).min(total), total));
                        });
                    }
                });
                if app.job_cancelled() {
                    break 'seasons;
                }
                done += eps.len() - compare_only;
                let (mut prints, mut errors) = (vec![], vec![]);
                for r in results {
                    match r.into_inner().unwrap().unwrap_or_else(|| Err("not read".into())) {
                        Ok(p) => {
                            prints.push(p);
                            errors.push(None);
                        }
                        Err(e) => {
                            prints.push(vec![]);
                            errors.push(Some(e));
                        }
                    }
                }
                let intros = intro::detect_season(&prints);
                let mut db = app.db.lock().unwrap();
                for (((id, ..), found_intro), error) in eps.iter().zip(intros).zip(errors).skip(compare_only) {
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
                drop(db);
                app.save();
            }
            elog!("intro detection ended: {taken} of {total} episodes took the old server's intro markers");
            app.save();
            *app.job.lock().unwrap() = None;
            let cancelled = app.job_cancel.swap(false, AtomicOrdering::SeqCst);
            app.job_auto.store(false, AtomicOrdering::SeqCst);
            if app.job_yield.swap(false, AtomicOrdering::SeqCst) {
                return;
            }
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
        self.imdb_update(force, crate::metadata::download_imdb_ratings)
    }

    fn imdb_update(self: &Arc<Self>, force: bool, download: fn(&Path) -> Result<(), String>) -> Result<String, String> {
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
        let mut failed = None;
        let result = (|| {
            if stale || force {
                if let Err(e) = download(&file) {
                    if !file.exists() {
                        return Err(e);
                    }
                    // The old file is used, but the update counts as failed, so it waits before trying again.
                    failed = Some(e);
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
        let result = match (result, failed) {
            (Ok(m), Some(e)) => Err(format!("{e} ({m} from the old file)")),
            (r, _) => r,
        };
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
        let (had_choice, had_tag, had_stored) = (choice.is_some(), tagged.is_some(), stored.is_some());
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
                // A search that got no answer (no internet) is not a miss.
                let mut folder_failed = false;
                let mut by_folder_pick = || { let r = by_folder(); folder_failed |= matches!(r, Some(Err(_))); r.as_ref().and_then(pick) };
                if folder_first {
                    best = by_folder_pick();
                }
                if best.is_none() {
                    let r = metadata::search(&settings, kind, &title, year);
                    best = pick(&r);
                    found = Some(r);
                }
                if best.is_none() && !folder_first {
                    best = by_folder_pick();
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
                        if let Some(it) = db.items.get_mut(&id).filter(|_| found.is_ok() && !folder_failed) {
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
        let mut d = match metadata::fetch(&settings, kind, &source, &mid, &seasons) {
            Ok(d) => d,
            // The online entry is not there (any more, or it is of the other kind).
            Err(e) if e.ends_with("HTTP 404") => {
                if !had_choice && !had_tag && had_stored {
                    // The match kept from before is stale: forget it and look the title up again.
                    elog!("details for item {id}: {source}-{mid} is gone online; looking the title up again");
                    if let Some(it) = self.db.lock().unwrap().items.get_mut(&id) {
                        it.extra.remove("metadata_source");
                        it.extra.remove("metadata_id");
                    }
                    return self.match_online(id, None, force);
                }
                return Err(format!("{source}-{mid} was not found online (HTTP 404)"));
            }
            Err(e) => return Err(e),
        };
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
            // Another worker is on this title right now (both would write the same poster): it waits its turn.
            {
                let mut busy = self.meta_busy.lock().unwrap();
                if busy.contains(&id) {
                    drop(busy);
                    self.meta_queue.lock().unwrap().push_back((id, force));
                    std::thread::sleep(Duration::from_millis(400));
                    continue;
                }
                busy.push(id);
            }
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
            } else {
                // Found nowhere online: a plugin may still know the title (e.g. an anime on AniList).
                self.lookup_metadata(&[id]);
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
    copy_bundled_plugins(&src, dest);
}

/// `a` is a higher dotted version number than `b` ("1.10" > "1.9").
fn newer_version(a: &str, b: &str) -> bool {
    let nums = |v: &str| v.trim().split('.').map(|n| n.parse::<u64>().unwrap_or(0)).collect::<Vec<_>>();
    let (a, b) = (nums(a), nums(b));
    (0..a.len().max(b.len())).map(|i| (a.get(i).copied().unwrap_or(0), b.get(i).copied().unwrap_or(0))).find(|(x, y)| x != y).is_some_and(|(x, y)| x > y)
}

/// The "version" in a plugin folder's plugin.json ("" when there is none).
fn plugin_version(dir: &Path) -> String {
    std::fs::read(dir.join("plugin.json")).ok().and_then(|b| serde_json::from_slice::<Value>(&b).ok())
        .and_then(|v| v["version"].as_str().map(String::from)).unwrap_or_default()
}

fn copy_bundled_plugins(src: &Path, dest: &Path) {
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
    let Ok(rd) = std::fs::read_dir(src) else { return };
    for e in rd.flatten() {
        let from = e.path();
        if !from.join("plugin.json").exists() {
            continue;
        }
        let name = e.file_name().to_string_lossy().to_string();
        let to = dest.join(e.file_name());
        let seen = done.contains(&name);
        if !seen { done.push(name.clone()); }
        // A newer bundled version replaces the plugin's own program files and plugin.json, never its
        // other files (its tokens and settings) or its tests; a plugin deleted by hand stays deleted.
        if to.join("plugin.json").is_file() && newer_version(&plugin_version(&from), &plugin_version(&to)) {
            for f in std::fs::read_dir(&from).into_iter().flatten().flatten() {
                let n = f.file_name().to_string_lossy().to_string();
                if n == "plugin.json" || (n.ends_with(".py") && !n.starts_with("test_")) {
                    let _ = std::fs::copy(f.path(), to.join(&n));
                }
            }
            crate::log::add(format!("Plugin {name} updated to version {}", plugin_version(&to)));
            continue;
        }
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
        // No Access-Control-Allow-Origin: pages of other websites must not read the answers
        // (the web app is on this server itself, the apps are no browsers).
        header("X-Server", &format!("{APP_NAME}/{VERSION}")),
        header("X-Content-Type-Options", "nosniff"),
        header("X-Frame-Options", "DENY"),
        // Links to other websites don't take the address along (it can hold a ?ticket= or the sign-in link's ?token=).
        header("Referrer-Policy", "no-referrer"),
    ]
}

/// An inline script's hash as the browser works it out: the browser reads the page with every line
/// break as a plain "\n", so a Windows checkout's "\r\n" (git with core.autocrlf) has to be hashed
/// the same way, or the browser refuses the scripts and the app never gets past "signed in".
fn inline_script_hash(script: &str) -> String {
    use sha2::Digest;
    let text = script.replace("\r\n", "\n").replace('\r', "\n");
    format!("'sha256-{}'", b64(&sha2::Sha256::digest(text.as_bytes())))
}

/// What the web app's page may load: its own scripts (the one inline script by its hash), pictures
/// from anywhere, YouTube trailers in a frame; no other website may show the page in a frame.
pub fn web_csp() -> &'static str {
    static CSP: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    CSP.get_or_init(|| {
        let hashes: Vec<String> = WEB_APP.split("<script>").skip(1).filter_map(|s| s.split("</script>").next())
            .map(inline_script_hash).collect();
        format!("default-src 'self'; script-src 'self' {}; style-src 'self' 'unsafe-inline'; img-src * data: blob:; media-src 'self' blob: data:; \
            frame-src https://www.youtube-nocookie.com; connect-src 'self'; object-src 'none'; base-uri 'none'; form-action 'self'; frame-ancestors 'none'", hashes.join(" "))
    })
}

/// A short fingerprint of the web app's scripts. The page asks for them as /web/app.js?v=<this>, so the
/// browser may keep them for good: a new version has new scripts, so a new fingerprint and new addresses.
pub fn web_version() -> &'static str {
    static V: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    V.get_or_init(|| {
        use sha2::Digest;
        let mut h = sha2::Sha256::new();
        for s in [WEB_JS, WEB_PAGES_JS, WEB_DASHBOARD_JS, WEB_DOWNLOADS_JS, WEB_I18N_JS] { h.update(s.as_bytes()); }
        for (name, s) in WEB_LANGS { h.update(name.as_bytes()); h.update(s.as_bytes()); }
        h.finalize().iter().take(6).map(|b| format!("{b:02x}")).collect()
    })
}

/// A script of the web app: kept for good when asked for with the current fingerprint (see `web_version`),
/// otherwise checked with the server each time.
pub fn web_script(body: &str, params: &[(String, String)]) -> Resp {
    let mut r = bytes(200, "text/javascript; charset=utf-8", body.as_bytes().to_vec());
    let current = params.iter().any(|(k, v)| k == "v" && v == web_version());
    r.add_header(header("Cache-Control", if current { "public, max-age=31536000, immutable" } else { "no-cache" }));
    r
}

/// Standard base64 (with padding).
pub fn b64(data: &[u8]) -> String {
    const ABC: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for c in data.chunks(3) {
        let n = (c[0] as u32) << 16 | (*c.get(1).unwrap_or(&0) as u32) << 8 | *c.get(2).unwrap_or(&0) as u32;
        for i in 0..4 { out.push(if i <= c.len() { ABC[(n >> (18 - 6 * i) & 63) as usize] as char } else { '=' }); }
    }
    out
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
/// What a plugin needs to know about an item (see `App::item_brief`).
fn brief_in(db: &Db, id: u64) -> Value {
    let Some(it) = db.items.get(&id) else { return Value::Null };
    let ids = |i: &Item| i.extra.get("ids").cloned().unwrap_or_else(|| json!({}));
    // The poster's file on this computer (a plugin can attach it to a message).
    let poster = |i: &Item| i.poster.as_ref().filter(|p| Path::new(p).is_file()).cloned();
    let show_of = |s: &Item| json!({"id": s.id, "title": s.title, "year": s.year, "ids": ids(s), "poster": poster(s), "genres": s.genres});
    let mut o = json!({"id": it.id, "type": it.kind.as_str(), "title": it.title, "year": it.year, "duration_ms": it.duration_ms, "ids": ids(it), "genres": it.genres, "poster": poster(it)});
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

/// `brief_in` plus the item's files (see `App::item_detail`).
fn detail_in(db: &Db, id: u64) -> Value {
    let mut o = brief_in(db, id);
    let Some(it) = db.items.get(&id) else { return Value::Null };
    let files: Vec<Value> = it.file.iter().chain(it.parts.iter()).map(|f| {
        let streams: Vec<Value> = f.streams.iter().map(|s| json!({"kind": s.kind, "codec": s.codec, "language": s.language, "channels": s.channels,
            "width": s.width, "height": s.height, "forced": s.forced, "sdh": s.sdh})).collect();
        json!({"path": f.path, "size": f.size, "container": f.container, "duration_ms": f.duration_ms, "streams": streams, "external_subtitles": f.external_subtitles})
    }).collect();
    o["section_id"] = json!(it.section_id);
    o["files"] = json!(files);
    o
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

/// The file really lies inside one of these folders, also after following links.
pub fn inside_folders<S: AsRef<Path>>(folders: &[S], p: &Path) -> bool {
    let Ok(real) = p.canonicalize() else { return false };
    folders.iter().any(|f| f.as_ref().canonicalize().is_ok_and(|f| real.starts_with(f)))
}

/// Looks up a host for a download a user asked for (a picture's address) and refuses this computer,
/// the home network, link-local (cloud metadata) and other special addresses. ureq asks this again
/// for every redirect, so a redirect can't lead there either.
pub fn public_only(netloc: &str) -> std::io::Result<Vec<std::net::SocketAddr>> {
    let all: Vec<std::net::SocketAddr> = std::net::ToSocketAddrs::to_socket_addrs(netloc)?.collect();
    let special = |ip: IpAddr| {
        let ip = match ip { IpAddr::V6(a) => a.to_ipv4_mapped().map_or(ip, IpAddr::V4), _ => ip };
        ip_is_local(ip) || ip.is_unspecified() || ip.is_multicast()
            || matches!(ip, IpAddr::V4(a) if a.is_broadcast() || a.octets()[0] == 0 || (a.octets()[0] == 100 && a.octets()[1] & 0xc0 == 64))
    };
    if all.is_empty() || all.iter().any(|a| special(a.ip())) {
        return Err(std::io::Error::new(std::io::ErrorKind::PermissionDenied, "addresses on this computer or the home network can't be used"));
    }
    Ok(all)
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

/// Splits a season into the episodes that take the old server's intro marker (`known`) and those to listen to.
/// Returns (known, listen, how many of the first in `listen` are known episodes only there to compare with).
pub fn split_known<E: Clone>(eps: Vec<E>, known: impl Fn(&E) -> bool) -> (Vec<E>, Vec<E>, usize) {
    let (known, mut listen): (Vec<E>, Vec<E>) = eps.into_iter().partition(|e| known(e));
    // A single new episode has nothing to be compared with but the known ones (their markers stay).
    if listen.len() != 1 || known.is_empty() {
        return (known, listen, 0);
    }
    let skip = known.len();
    listen.splice(0..0, known.iter().cloned());
    (known, listen, skip)
}

/// Movies and shows without details go back in line at start: the line is lost on a restart, and a title
/// missed recently is skipped by match_online until a week has passed (or the matcher learned more, MATCH_VERSION).
pub fn details_retry_list(db: &Db) -> Vec<u64> {
    db.items.values()
        .filter(|i| matches!(i.kind, ItemKind::Movie | ItemKind::Show) && !i.extra.contains_key("metadata_source"))
        .filter(|i| i.extra.get("unmatched_by_hand").and_then(|v| v.as_bool()) != Some(true))
        // (The same test as `recent` in match_online: those would only be turned away.)
        .filter(|i| !(i.extra.get("metadata_status").and_then(|v| v.as_str()) == Some("unmatched")
            && i.extra.get("match_version").and_then(|v| v.as_u64()) == Some(MATCH_VERSION)
            && i.extra.get("metadata_checked_at").and_then(|v| v.as_u64()).is_some_and(|t| now().saturating_sub(t) < 7 * 86400)))
        .map(|i| i.id).collect()
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
    crate::trakt::start(app);
    let retry = details_retry_list(&app.db.lock().unwrap());
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
    // Credits detection "as soon as possible" (within a minute of new files; "scheduled" runs
    // in maintenance::tick) and IMDb ratings (weekly file).
    let a = app.clone();
    std::thread::spawn(move || {
        let mut imdb_retry = Instant::now();
        loop {
            std::thread::sleep(Duration::from_secs(60));
            let due = a.settings().credits_markers_behaviour == "asap";
            // Credits and intros come last: only when no details are waiting or being fetched.
            let idle = a.meta_queue.lock().unwrap().is_empty() && a.meta_busy.lock().unwrap().is_empty() && a.job.lock().unwrap().is_none();
            let paused = a.auto_paused.load(AtomicOrdering::SeqCst);
            if due && idle && !paused {
                let pending = a.credits_pending();
                if !pending.is_empty() {
                    let _ = a.detect_credits(pending, false);
                    continue;
                }
            }
            // Intro detection: the same choices ("Generate intro video markers").
            let intro_due = a.settings().intro_markers_behaviour == "asap";
            if intro_due && idle && !paused {
                let pending = a.intros_pending();
                if !pending.is_empty() {
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
    // Once a day: schedule.daily for plugins. The last date goes in a file, so
    // a restart does not send it twice and a day missed while off is sent at start.
    let a = app.clone();
    std::thread::spawn(move || loop {
        a.daily_tick();
        std::thread::sleep(Duration::from_secs(60));
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
    let label = match (product.is_empty(), device.is_empty() || device == product) {
        (false, false) => format!("{product} — {device}"),
        (false, true) => product,
        (true, false) => device,
        (true, true) => crate::stats::short_agent(agent),
    };
    // The caller picks these headers; keep only what fits in the list of devices.
    label.chars().take(100).collect()
}

/// Whether an Origin header names another address than the one this server was reached at
/// (`host`, or one of `forwarded_hosts` from a trusted proxy).
fn origin_is_other_site(origin: &str, host: &str, forwarded_hosts: &[String]) -> bool {
    let Some((_, o)) = origin.split_once("://") else { return true };
    o != host.to_ascii_lowercase() && !forwarded_hosts.iter().any(|h| h == o)
}

/// Whether an Accept-Encoding header allows gzip (and does not turn it off with q=0).
fn accepts_gzip(v: &str) -> bool {
    v.split(',').any(|part| {
        let mut it = part.split(';').map(str::trim);
        let name = it.next().unwrap_or("").to_ascii_lowercase();
        let off = it.any(|p| p.strip_prefix("q=").and_then(|q| q.parse::<f32>().ok()).is_some_and(|q| q <= 0.0));
        (name == "gzip" || name == "*") && !off
    })
}

/// Text answers (the web app's page and scripts, JSON, XML) packed with gzip. Pictures, video and
/// downloads are packed already or are streamed, and small answers would not get smaller.
fn gzipped(resp: Resp) -> Resp {
    let has = |name: &'static str| resp.headers().iter().any(|h| h.field.equiv(name));
    let ctype = resp.headers().iter().find(|h| h.field.equiv("Content-Type")).map(|h| h.value.as_str().to_ascii_lowercase()).unwrap_or_default();
    let text = ["text/", "application/json", "application/xml", "application/javascript"].iter().any(|t| ctype.starts_with(t));
    let fits = resp.data_length().is_some_and(|n| (1024..=32 << 20).contains(&n));
    if !text || !fits || resp.status_code().0 != 200 || has("Content-Encoding") || has("Content-Range") { return resp; }
    let (status, mut headers) = (resp.status_code(), resp.headers().to_vec());
    let mut body = Vec::new();
    if resp.into_reader().read_to_end(&mut body).is_err() { return bytes(500, "text/plain", b"Could not read the answer".to_vec()); }
    let level = if body.len() > 1 << 20 { flate2::Compression::fast() } else { flate2::Compression::default() };
    let mut enc = flate2::write::GzEncoder::new(Vec::with_capacity(body.len() / 4), level);
    let packed = std::io::Write::write_all(&mut enc, &body).and_then(|_| enc.finish());
    let body = match packed { Ok(p) => { headers.push(header("Content-Encoding", "gzip")); p } Err(_) => body };
    headers.push(header("Vary", "Accept-Encoding"));
    let len = body.len();
    Response::new(status, headers, Box::new(std::io::Cursor::new(body)) as Box<dyn Read + Send>, Some(len), None)
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
    let (mut fetch_site, mut origin) = (String::new(), None);
    let mut fwd_host = Vec::new();
    let mut gzip_ok = false;
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
            "sec-fetch-site" => fetch_site = h.value.as_str().to_ascii_lowercase(),
            "origin" => origin = Some(h.value.as_str().to_ascii_lowercase()),
            "x-forwarded-host" => fwd_host.extend(h.value.as_str().split(',').map(|x| x.trim().to_ascii_lowercase())),
            "accept-encoding" => gzip_ok = accepts_gzip(h.value.as_str()),
            _ => {}
        }
    }
    // The token comes only in a header (X-Token or Authorization). Addresses end up in proxy logs and
    // browser histories, so ?token= is not taken; an address that has to work on its own uses ?ticket=.
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
        let hash = crate::auth::token_hash(t);
        let tk = db.tokens.iter_mut().find(|x| x.token == hash)?;
        // The list of devices says when each was used last (written at most once a minute).
        if at.saturating_sub(tk.last_used) > 60 { tk.last_used = at; tk.last_device = device.clone(); tk.last_ip = peer.clone(); }
        let uid = tk.user_id;
        db.user(uid).cloned()
    });
    // A ticket (?ticket=, from /api/ticket) opens only the addresses it was made for, while its sign-in lasts (one made without a token, in a network that needs no sign-in, has an empty id and is bound to its user and expiry only).
    let user = match params.iter().find(|(k, _)| k == "ticket") {
        Some((_, t)) if user.is_none() && matches!(method.as_str(), "GET" | "HEAD") => {
            let db = app.db.lock().unwrap();
            crate::auth::read_ticket(&db.ticket_key, t, now())
                .filter(|(uid, tid, scope)| crate::auth::ticket_scope(&path).as_ref() == Some(scope) && (tid.is_empty() || db.tokens.iter().any(|x| x.user_id == *uid && crate::auth::stored_token_id(&x.token) == tid)))
                .and_then(|(uid, ..)| db.user(uid).cloned())
        }
        _ => user,
    };
    let (trusted, no_auth, lan_list) = { let db = app.db.lock().unwrap(); (db.settings.trusted_proxies.clone(), db.settings.no_auth_networks.clone(), db.settings.lan_networks.clone()) };
    let (remote, remote_trusted) = match relayed {
        // The relay does not prove who it is, so a relayed caller never counts as local or as a no-sign-in network.
        Some(ip) => (Some(ip), false),
        None => client_address(req.remote_addr().map(|a| a.ip()), &xff, real_ip.as_deref(), &forwarded, &trusted),
    };
    // The browser says the request comes from a page of another website (a link, form or script there).
    // Over plain http there is no Sec-Fetch-Site, so then an Origin for another address counts too.
    // A trusted proxy that rewrites Host says the address the browser used in X-Forwarded-Host, which counts as ours as well.
    let from_trusted_proxy = req.remote_addr().is_some_and(|a| ip_in_list(&trusted, a.ip()));
    let other_site = if fetch_site.is_empty() { origin.as_deref().is_some_and(|o| origin_is_other_site(o, &host, if from_trusted_proxy { &fwd_host } else { &[] })) }
        else { matches!(fetch_site.as_str(), "cross-site" | "same-site") };
    // Networks that need no sign-in act as the first admin (only with an address that is believed,
    // and never for another website's page).
    let user = if user.is_none() && !other_site && remote_trusted && !no_auth.trim().is_empty() && remote.is_some_and(|a| ip_in_list(&no_auth, a)) {
        app.db.lock().unwrap().users.iter().filter(|u| u.admin).min_by_key(|u| u.id).cloned()
    } else { user };
    let uid = user.as_ref().map(|u| u.id);
    if let Some(id) = uid {
        app.stats.saw(id, remote.map(|a| a.to_string()).unwrap_or_default(), &agent);
    }
    let lan_extra = !lan_list.trim().is_empty() && remote.is_some_and(|a| ip_in_list(&lan_list, a));
    let ctx = Ctx { method: method.clone(), path, params, body, user, host, range, token, remote, remote_trusted, lan_extra, device };
    let is_local = ctx.is_local();
    let started = Instant::now();
    let mut resp = if method == "OPTIONS" { bytes(204, "text/plain", vec![]) }
        else if other_site && (!matches!(method.as_str(), "GET" | "HEAD") || ctx.path.starts_with("/api/home")) { fail(403, "Requests from other websites are not allowed") }
        else { crate::routes::route(&app, &ctx) };
    // How long the server needed to answer (not to send): in the browser's network tab as Server-Timing,
    // and in the server log when it was slow (playing and converting go on for long by design).
    let took = started.elapsed().as_millis();
    resp.add_header(header("Server-Timing", &format!("app;dur={took}")));
    if took >= 400 && !["/parts/", "/transcode", "/video/", "/hls", "/stream"].iter().any(|p| ctx.path.contains(p)) {
        crate::log::add(format!("Slow answer: {} {} took {} ms", method, ctx.path, took));
    }
    if gzip_ok { resp = gzipped(resp); }
    // Count what is sent (bandwidth on the dashboard).
    let (status, headers, len) = (resp.status_code(), resp.headers().to_vec(), resp.data_length());
    let remote = if is_local { None } else { Some(app.stats.bytes_remote.clone()) };
    let body = crate::stats::Counting { inner: resp.into_reader(), all: app.stats.bytes_out.clone(), user: app.stats.counter_for(uid), remote };
    let resp: Resp = Response::new(status, headers, Box::new(body) as Box<dyn Read + Send>, len, None);
    let _ = req.respond(resp);
}

// ---------------------------------------------------------------- JSON views

/// Who is whose child and which items are merged versions of which, found in one pass over the
/// library. A list of items builds it once; without it every item would scan the whole library.
pub struct Index {
    kids: HashMap<u64, Vec<u64>>,
    versions: HashMap<u64, Vec<u64>>,
}

impl Index {
    pub fn build(db: &Db) -> Index {
        let mut kids: HashMap<u64, Vec<u64>> = HashMap::new();
        let mut versions: HashMap<u64, Vec<u64>> = HashMap::new();
        for i in db.items.values() {
            if let Some(p) = i.parent_id.filter(|_| !db.is_version(i)) { kids.entry(p).or_default().push(i.id); }
            if let Some(v) = i.version_of() { versions.entry(v).or_default().push(i.id); }
        }
        Index { kids, versions }
    }

    /// Everything playable below an item (like `Db::leaves`).
    pub fn leaves(&self, db: &Db, id: u64) -> Vec<u64> {
        let mut out = vec![];
        self.collect_leaves(db, id, &mut out);
        out
    }

    fn collect_leaves(&self, db: &Db, id: u64, out: &mut Vec<u64>) {
        let Some(item) = db.items.get(&id) else { return };
        if item.kind.playable() { out.push(id); return; }
        for c in self.kids.get(&id).into_iter().flatten() { self.collect_leaves(db, *c, out); }
    }
}

pub fn item_json(db: &Db, user: &User, it: &Item, detail: bool) -> Value {
    item_json_ix(db, user, it, detail, None)
}

/// Like `item_json`; with an index (for lists) the children and versions are looked up, not searched for.
pub fn item_json_ix(db: &Db, user: &User, it: &Item, detail: bool, ix: Option<&Index>) -> Value {
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
        let leaves = match ix { Some(ix) => ix.leaves(db, it.id), None => db.leaves(it.id) };
        let seen = leaves.iter().filter(|l| db.view(user.id, **l).view_count > 0).count();
        o["leafCount"] = json!(leaves.len());
        o["viewedLeafCount"] = json!(seen);
        o["childCount"] = json!(match ix { Some(ix) => ix.kids.get(&it.id).map_or(0, |k| k.len()), None => db.children(it.id).len() });
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
        let mut versions: Vec<&Item> = match ix {
            Some(ix) => ix.versions.get(&it.id).into_iter().flatten().filter_map(|i| db.items.get(i)).collect(),
            None => db.items.values().filter(|x| x.version_of() == Some(it.id)).collect(),
        };
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
    playlist_json_with(db, user, p, query::playlist_items(db, user, p))
}

/// `playlist_json` for items that were worked out already (a smart playlist runs over the whole library).
pub fn playlist_json_with(db: &Db, user: &User, p: &Playlist, items: Vec<u64>) -> (Value, usize, u64) {
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

/// One row of the home page; `ix` is built once for all the rows.
pub fn hub_ix(db: &Db, user: &User, key: &str, title: &str, ids: Vec<u64>, ix: &Index) -> Value {
    let list: Vec<Value> = ids.iter().filter_map(|i| db.items.get(i)).map(|i| item_json_ix(db, user, i, false, Some(ix))).collect();
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
    // One pass over all items instead of one per show and season (db.leaves scans everything each time).
    let mut kids: HashMap<u64, Vec<u64>> = HashMap::new();
    for i in db.items.values() {
        if let Some(p) = i.parent_id { kids.entry(p).or_default().push(i.id); }
    }
    fn leaves_of(db: &Db, kids: &HashMap<u64, Vec<u64>>, id: u64, out: &mut Vec<u64>) {
        let Some(item) = db.items.get(&id) else { return };
        if item.kind.playable() { out.push(id); return; }
        for c in kids.get(&id).into_iter().flatten() { leaves_of(db, kids, *c, out); }
    }
    for show in db.items.values().filter(|i| i.kind == ItemKind::Show && db.can_see_item(user, i)) {
        let mut eps = vec![];
        leaves_of(db, &kids, show.id, &mut eps);
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
    // Only this person's watch states ("user:item" keys), not every item in the library.
    let t_now = now();
    let mut started: Vec<(u64, u64)> = db
        .views
        .range(format!("{}:", user.id)..format!("{};", user.id))
        .filter(|(_, v)| v.view_offset_ms > 0 && t_now.saturating_sub(v.last_viewed_at) <= window)
        .filter_map(|(k, v)| {
            let id = k.split_once(':')?.1.parse::<u64>().ok()?;
            let i = db.items.get(&id)?;
            (i.kind.playable() && !i.hidden() && db.can_see_item(user, i)).then_some((id, v.last_viewed_at))
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
pub(crate) mod tests {
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

    #[test]
    fn broken_pictures_cleanup_leaves_media_folders_and_unchanged_items_alone() {
        let root = std::env::temp_dir().join(format!("lichtspiel-broken-pictures-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let media = root.join("media");
        std::fs::create_dir_all(&media).unwrap();
        let app = App::open(&root.join("data"));
        std::fs::create_dir_all(app.data_dir.join("metadata/1")).unwrap();
        let ours = app.data_dir.join("metadata/1/poster.jpg");
        let theirs = media.join("poster.jpg");
        std::fs::write(&ours, b"<svg/>").unwrap();
        std::fs::write(&theirs, b"").unwrap();
        {
            let mut db = app.db.lock().unwrap();
            for (id, p) in [(1u64, Some(&ours)), (2, Some(&theirs)), (3, None)] {
                let mut i: Item = serde_json::from_value(json!({"id": id, "section_id": 1, "kind": "movie", "title": "x", "sort_title": "x", "added_at": 0, "updated_at": 0})).unwrap();
                i.poster = p.map(|p| p.to_string_lossy().to_string());
                db.items.insert(id, i);
            }
            let _ = db.items.take_changes();
        }
        let broken = app.remove_broken_pictures();
        assert_eq!(broken, vec![1]);
        assert!(!ours.exists());
        assert!(theirs.exists(), "a file in the user's media folder was deleted");
        assert_eq!(app.db.lock().unwrap().items[&2].poster.as_deref(), Some(&*theirs.to_string_lossy()));
        // Nothing broken is left: a second run changes no item (no row is rewritten).
        assert!(app.remove_broken_pictures().is_empty());
        let mut db = app.db.lock().unwrap();
        assert!(matches!(db.items.take_changes(), crate::store::Changed::Keys(k) if k.is_empty()));
        drop(db);
        let _ = std::fs::remove_dir_all(&root);
    }

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
    fn base64() {
        assert_eq!([b64(b""), b64(b"f"), b64(b"fo"), b64(b"foo"), b64(b"foob")], ["", "Zg==", "Zm8=", "Zm9v", "Zm9vYg=="]);
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

    #[test]
    fn an_inline_script_checked_out_with_windows_line_breaks_gets_the_hash_the_browser_works_out() {
        // What a browser hashes for "a\nb" (it reads "\r\n" and a lone "\r" as "\n").
        let browser = { use sha2::Digest; format!("'sha256-{}'", b64(&sha2::Sha256::digest(b"\na();\nb();\n"))) };
        assert_eq!(inline_script_hash("\na();\nb();\n"), browser);
        assert_eq!(inline_script_hash("\r\na();\r\nb();\r\n"), browser);
        assert_eq!(inline_script_hash("\ra();\rb();\r"), browser);
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
    fn a_newer_bundled_plugin_replaces_only_its_own_files() {
        let root = std::env::temp_dir().join(format!("lichtspiel-bundled-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let (src, dest) = (root.join("bundled"), root.join("plugins"));
        let plugin = |dir: &Path, version: &str, code: &str| {
            std::fs::create_dir_all(dir).unwrap();
            std::fs::write(dir.join("plugin.json"), format!(r#"{{"id": "x", "version": "{version}"}}"#)).unwrap();
            std::fs::write(dir.join("main.py"), code).unwrap();
            std::fs::write(dir.join("test_main.py"), code).unwrap();
        };
        plugin(&src.join("trakt"), "1.10", "new");
        plugin(&src.join("gone"), "2.0", "new");
        plugin(&dest.join("trakt"), "1.9", "old");
        std::fs::write(dest.join("trakt/tokens.json"), "mine").unwrap();
        std::fs::write(dest.join(".bundled-installed"), "trakt\ngone\n").unwrap();
        copy_bundled_plugins(&src, &dest);
        let read = |p: &str| std::fs::read_to_string(dest.join(p)).unwrap_or_default();
        assert_eq!(read("trakt/main.py"), "new", "the newer bundled version was not installed");
        assert!(read("trakt/plugin.json").contains("1.10"));
        assert_eq!(read("trakt/tokens.json"), "mine", "the plugin's own data was touched");
        assert_eq!(read("trakt/test_main.py"), "old", "a test file was copied over");
        assert!(!dest.join("gone").exists(), "a plugin deleted by hand came back");
        // The same or an older bundled version changes nothing.
        std::fs::write(dest.join("trakt/main.py"), "edited").unwrap();
        copy_bundled_plugins(&src, &dest);
        assert_eq!(read("trakt/main.py"), "edited");
        assert!(newer_version("1.10", "1.9") && newer_version("1.1", "1") && !newer_version("1.1", "1.1") && !newer_version("1.0", "1.1") && !newer_version("", "1.0"));
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

#[cfg(test)]
mod gzip_tests {
    use super::*;

    fn body(r: Resp) -> (Vec<String>, Vec<u8>) {
        let heads = r.headers().iter().map(|h| format!("{}: {}", h.field, h.value)).collect();
        let mut b = Vec::new();
        r.into_reader().read_to_end(&mut b).unwrap();
        (heads, b)
    }

    #[test]
    fn text_is_packed_only_for_browsers_that_take_gzip() {
        assert!(accepts_gzip("gzip, deflate, br") && accepts_gzip("br;q=1.0, gzip;q=0.8") && accepts_gzip("*"));
        assert!(!accepts_gzip("") && !accepts_gzip("br") && !accepts_gzip("gzip;q=0") && !accepts_gzip("identity"));
        let text = "Ein langer Text. ".repeat(500);
        let (heads, packed) = body(gzipped(bytes(200, "text/javascript; charset=utf-8", text.clone().into_bytes())));
        assert!(heads.iter().any(|h| h == "Content-Encoding: gzip") && packed.len() < text.len() / 10);
        let mut back = String::new();
        flate2::read::GzDecoder::new(&packed[..]).read_to_string(&mut back).unwrap();
        assert_eq!(back, text);
        // Pictures, small answers and errors stay as they are.
        for r in [bytes(200, "image/jpeg", vec![0; 5000]), bytes(200, "application/json", b"{}".to_vec()), bytes(404, "text/plain", vec![b'x'; 5000])] {
            assert!(!body(gzipped(r)).0.iter().any(|h| h.starts_with("Content-Encoding")));
        }
    }

    #[test]
    fn the_page_asks_for_scripts_that_may_be_kept() {
        let v = web_version().to_string();
        let kept = |p: Vec<(String, String)>| body(web_script("x", &p)).0.iter().any(|h| h.contains("immutable"));
        assert!(kept(vec![("v".into(), v)]));
        assert!(!kept(vec![]) && !kept(vec![("v".into(), "old".into())]));
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

#[cfg(test)]
mod background_tests {
    use super::*;
    use crate::server::tests::test_app;

    fn item(id: u64, kind: &str, extra: Value) -> Item {
        serde_json::from_value(json!({"id": id, "section_id": 1, "kind": kind, "title": "x", "sort_title": "x", "added_at": 0, "updated_at": 0, "extra": extra})).unwrap()
    }

    #[cfg(unix)]
    #[test]
    fn many_new_titles_do_not_start_a_plugin_for_each_at_once() {
        let (app, root) = test_app("plugin-storm");
        let dir = root.join("plugins").join("counter");
        std::fs::create_dir_all(dir.join("run")).unwrap();
        // Records how many copies of itself are running.
        std::fs::write(dir.join("plugin.json"), r#"{"id": "counter", "name": "counter", "hooks": ["library.item.added"], "command": ["sh", "-c", "touch run/$$; ls run | wc -l >> log; sleep 0.01; rm run/$$"]}"#).unwrap();
        app.plugins.lock().unwrap().reload();
        app.db.lock().unwrap().plugins.insert("counter".into(), serde_json::from_value(json!({"enabled": true})).unwrap());
        for id in 0..500 { app.fire("library.item.added", json!({"id": id})); }
        let log = dir.join("log");
        let lines = || std::fs::read_to_string(&log).unwrap_or_default().lines().count();
        for _ in 0..600 { if lines() >= 500 { break; } std::thread::sleep(Duration::from_millis(50)); }
        let most = std::fs::read_to_string(&log).unwrap_or_default().lines().filter_map(|l| l.trim().parse::<usize>().ok()).max().unwrap_or(0);
        let n = lines();
        let _ = std::fs::remove_dir_all(&root);
        assert_eq!(n, 500, "not every event reached the plugin");
        assert!(most <= crate::plugins::MAX_RUNNING, "{most} plugin programs ran at once");
    }

    #[test]
    fn a_new_title_tells_plugins_its_files_and_streams() {
        let (app, root) = test_app("item-detail");
        let mut m = item(7, "movie", json!({"ids": {"imdb": "tt0133093"}}));
        m.year = Some(1999);
        m.file = Some(serde_json::from_value(json!({"path": "/media/Matrix (1999).mkv", "size": 5, "modified": 0, "container": "mkv",
            "streams": [{"kind": "audio", "index": 1, "codec": "ac3", "language": "ger", "channels": 6}], "external_subtitles": ["/media/Matrix (1999).en.srt"]})).unwrap());
        app.db.lock().unwrap().items.insert(7, m);
        let d = app.item_detail(7);
        let _ = std::fs::remove_dir_all(&root);
        assert_eq!(d["type"], "movie");
        assert_eq!(d["ids"]["imdb"], "tt0133093");
        assert_eq!(d["files"][0]["path"], "/media/Matrix (1999).mkv");
        assert_eq!(d["files"][0]["streams"][0]["language"], "ger");
        assert_eq!(d["files"][0]["external_subtitles"][0], "/media/Matrix (1999).en.srt");
        assert!(app.item_detail(99).is_null());
    }

    #[test]
    fn subtitles_a_plugin_saved_in_the_data_folder_are_found_and_may_be_sent() {
        let (app, root) = test_app("substore");
        let media = root.join("media").join("Heat (1995)");
        std::fs::create_dir_all(&media).unwrap();
        let video = media.join("Heat (1995).mkv");
        std::fs::write(&video, b"x").unwrap();
        let mut m = item(9, "movie", json!({}));
        m.file = Some(serde_json::from_value(json!({"path": video.to_string_lossy(), "size": 1, "modified": 0, "container": "mkv"})).unwrap());
        app.db.lock().unwrap().items.insert(9, m);
        let dir = crate::scanner::store_dir(&app.data_dir.join("subtitles"), &media);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("Heat (1995).de.srt"), b"1").unwrap();
        app.refresh_subtitles(9);
        let subs = app.db.lock().unwrap().items.get(&9).unwrap().file.as_ref().unwrap().external_subtitles.clone();
        let sendable = subs.first().is_some_and(|p| app.may_send(Path::new(p), false));
        let elsewhere = app.may_send(&app.data_dir.join("library.db"), false);
        let _ = std::fs::remove_dir_all(&root);
        assert_eq!(subs, vec![dir.join("Heat (1995).de.srt").to_string_lossy().to_string()]);
        assert!(sendable && !elsewhere);
    }

    #[test]
    fn a_plugin_that_reads_the_library_gets_every_title_with_its_key_and_no_other_does() {
        let (app, root) = test_app("plugin-library");
        let mut m = item(7, "movie", json!({}));
        m.file = Some(serde_json::from_value(json!({"path": "/media/Heat (1995).mkv", "size": 5, "modified": 0, "container": "mkv"})).unwrap());
        let gone = item(8, "movie", json!({crate::scanner::MISSING: 1}));
        let show = item(9, "show", json!({}));
        for i in [m, gone, show] { app.db.lock().unwrap().items.insert(i.id, i); }
        let manifest = |id: &str, reads: bool, admins: bool| -> plugins::Manifest { serde_json::from_value(json!({"id": id, "name": id, "version": "1", "command": ["x"],
            "user_panel": true, "panel_admins_only": admins, "reads_library": reads})).unwrap() };
        {
            let mut p = app.plugins.lock().unwrap();
            p.list.push(plugins::Plugin { manifest: manifest("doctor", true, true), dir: root.clone() });
            p.list.push(plugins::Plugin { manifest: manifest("other", false, false), dir: root.clone() });
            let mut db = app.db.lock().unwrap();
            for id in ["doctor", "other"] { db.plugins.entry(id.into()).or_default().enabled = true; }
        }
        let (doctor, other) = { let p = app.plugins.lock().unwrap(); (p.get("doctor").cloned().unwrap(), p.get("other").cloned().unwrap()) };
        let address = app.server_info_for(&doctor)["library"].as_str().unwrap_or("").to_string();
        let key = address.split("key=").nth(1).unwrap_or("").to_string();
        let ask = |pid: &str, key: &str| {
            let ctx = Ctx { method: "GET".into(), path: format!("/api/plugins/{pid}/library"), params: vec![("key".into(), key.into())], body: Value::Null,
                user: None, host: String::new(), range: None, token: None, remote: Some([127, 0, 0, 1].into()), remote_trusted: true, lan_extra: false, device: "t".into() };
            crate::handlers::plugin_library(&app, &ctx, pid)
        };
        let listed = ask("doctor", &key);
        let codes = [ask("doctor", "wrong").status_code().0, ask("other", &key).status_code().0, ask("doctor", "").status_code().0];
        let (admin, kid) = { let db = app.db.lock().unwrap(); (db.users[0].clone(), db.users[1].clone()) };
        let panels = |u: &User| { let r = crate::handlers::plugin_panels(&app, u); let mut b = Vec::new(); r.into_reader().read_to_end(&mut b).unwrap(); serde_json::from_slice::<Value>(&b).unwrap()["panels"].as_array().map(|a| a.len()).unwrap_or(0) };
        let (for_admin, for_kid) = (panels(&admin), panels(&kid));
        let mut body = Vec::new();
        listed.into_reader().read_to_end(&mut body).unwrap();
        let v: Value = serde_json::from_slice(&body).unwrap_or(Value::Null);
        let _ = std::fs::remove_dir_all(&root);
        assert!(address.contains("/api/plugins/doctor/library?key=") && !key.is_empty());
        assert!(app.server_info_for(&other).get("library").is_none());
        assert_eq!(v["items"].as_array().map(|a| a.len()), Some(1));
        assert_eq!(v["items"][0]["files"][0]["path"], "/media/Heat (1995).mkv");
        assert_eq!(codes, [403, 403, 403]);
        assert_eq!((for_admin, for_kid), (2, 1));
    }

    #[test]
    fn schedule_daily_is_sent_once_a_day_and_after_a_restart_only_if_missed() {
        let (app, root) = test_app("daily");
        let first = app.daily_tick();
        let again = app.daily_tick();
        let stored = std::fs::read_to_string(root.join("plugins").join(".daily-last")).unwrap_or_default();
        std::fs::write(root.join("plugins").join(".daily-last"), "2000-01-01").unwrap();
        let missed = app.daily_tick();
        let _ = std::fs::remove_dir_all(&root);
        assert!(first && !again && missed);
        assert_eq!(stored, crate::db::today());
    }

    #[test]
    fn titles_without_details_are_queued_again_at_start() {
        let mut db = Db::default();
        let old = MATCH_VERSION - 1;
        for i in [item(1, "movie", json!({})), item(2, "show", json!({})), item(3, "movie", json!({"metadata_status": "unmatched", "match_version": old})),
            item(4, "movie", json!({"metadata_source": "tmdb", "metadata_id": "1"})), item(5, "movie", json!({"unmatched_by_hand": true})), item(6, "episode", json!({})),
            // Missed by this matcher: not again within a week.
            item(7, "movie", json!({"metadata_status": "unmatched", "match_version": MATCH_VERSION, "metadata_checked_at": now() - 86400})),
            item(8, "movie", json!({"metadata_status": "unmatched", "match_version": MATCH_VERSION, "metadata_checked_at": now() - 8 * 86400}))] {
            db.items.insert(i.id, i);
        }
        let mut got = details_retry_list(&db);
        got.sort();
        assert_eq!(got, vec![1, 2, 3, 8]);
    }

    #[test]
    fn a_title_a_worker_is_busy_with_waits_for_it() {
        let (app, root) = test_app("meta-busy");
        app.db.lock().unwrap().items.insert(7, item(7, "movie", json!({})));
        app.meta_busy.lock().unwrap().push(7);
        app.meta_queue.lock().unwrap().push_back((7, true));
        let a = app.clone();
        std::thread::spawn(move || a.metadata_worker());
        std::thread::sleep(Duration::from_millis(1500));
        let (queued, done) = (app.meta_queue.lock().unwrap().iter().any(|q| q.0 == 7), app.meta_progress.lock().unwrap().done);
        let _ = std::fs::remove_dir_all(&root);
        assert_eq!(done, 0, "a second worker looked the title up while another one was on it");
        assert!(queued, "the title was dropped instead of waiting");
    }

    #[test]
    fn a_new_episode_is_compared_with_the_episodes_plex_marked() {
        // 9 episodes with the old server's marker and 1 new one: the new one is listened to with them.
        let (known, listen, skip) = split_known((1..=10).collect::<Vec<u32>>(), |e| *e <= 9);
        assert_eq!((known.len(), listen.len(), skip), (9, 10, 9));
        // Two new ones are enough by themselves.
        let (known, listen, skip) = split_known((1..=5).collect::<Vec<u32>>(), |e| *e <= 3);
        assert_eq!((known.len(), listen, skip), (3, vec![4, 5], 0));
        // A single episode with nothing to compare with stays alone.
        let (_, listen, skip) = split_known(vec![1u32], |_| false);
        assert_eq!((listen, skip), (vec![1], 0));
    }

    #[test]
    fn a_failed_imdb_download_with_an_old_file_is_reported_so_it_waits() {
        let (app, root) = test_app("imdb-stale");
        let file = app.data_dir.join("cache").join("imdb-ratings.tsv.gz");
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        {
            use std::io::Write;
            let mut gz = flate2::write::GzEncoder::new(std::fs::File::create(&file).unwrap(), flate2::Compression::default());
            gz.write_all(b"tconst\taverageRating\tnumVotes\ntt0000001\t7.5\t100\n").unwrap();
        }
        std::fs::File::options().write(true).open(&file).unwrap().set_modified(std::time::SystemTime::now() - Duration::from_secs(30 * 86400)).unwrap();
        app.db.lock().unwrap().items.insert(1, item(1, "movie", json!({"ids": {"imdb": "tt0000001"}})));
        let r = app.imdb_update(true, |_| Err("offline".into()));
        let rating = app.db.lock().unwrap().items[&1].extra.get("ratings").cloned();
        let _ = std::fs::remove_dir_all(&root);
        assert!(r.is_err(), "the scheduler would try again in a minute: {r:?}");
        assert!(rating.is_some(), "the old file's ratings were not used");
    }
}

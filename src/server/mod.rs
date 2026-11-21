//! The HTTP server core: the shared state (`App`), opening and saving the library, and
//! starting work. Library, hub, playlist and transcode paths
//! (/library/sections, /library/metadata/{id}, /hubs/home/…, /playlists, /:/timeline,
//! /video/…/transcode); admin features live under /api/.
//! Routes are in api/routes.rs, the larger handlers in api/handlers.rs. The rest of the server
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
use crate::data::db::{self, now, Db, Invite, Item, ItemKind, Playlist, PlaylistKind, Section, User};
use crate::admin::plugins::{self, Plugins};
use crate::library::query;
use crate::library::scanner;
use crate::data::store::{self, Changes, Store};
use crate::playback::transcode::Method;
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
    pub stats: crate::admin::stats::Stats,
    /// "Coming soon": episodes and movies still to come (kept in upcoming.json).
    pub upcoming: Mutex<crate::connect::upcoming::Store>,
    /// The disks of the libraries and the data folder, as found by the last check (the dashboard).
    pub disks: Mutex<Vec<crate::admin::disk::Disk>>,
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
        let _ = crate::library::scanner::SUBTITLE_STORE.set(data_dir.join("subtitles"));
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
            upcoming: Mutex::new(crate::connect::upcoming::Store::load(data_dir)),
            disks: Mutex::new(vec![]),
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
            db.plugins.entry(id).or_insert(db::PluginState { enabled: on, ..Default::default() });
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
            v["history"] = json!(format!("{}/api/plugins/{id}/history?key={}", v["url"].as_str().unwrap_or(""), self.plugin_key(id)));
        }
        let s = self.settings();
        if pl.manifest.sends_mail && crate::accounts::mail::ready(&s) {
            v["smtp"] = json!({"host": s.smtp_host.trim(), "port": s.smtp_port, "security": s.smtp_security, "user": s.smtp_user.trim(), "password": s.smtp_password, "from": s.smtp_from.trim()});
        }
        v
    }

    /// The key that lets a plugin read the library (from the server's secret, so it stays the same after a restart).
    pub fn plugin_key(&self, id: &str) -> String {
        let secret = self.db.lock().unwrap().ticket_key.clone();
        crate::accounts::auth::token_hash(&format!("plugin-library:{secret}:{id}"))[..32].to_string()
    }

    /// Every movie and episode (versions too, extras not) with its files, and
    /// the libraries: what `GET /api/plugins/{id}/library` answers.
    pub fn library_listing(&self) -> Value {
        let db = self.db.lock().unwrap();
        let sections: Vec<Value> = db.sections.iter().map(|s| json!({"id": s.id, "name": s.name, "kind": s.kind.as_str(), "paths": s.paths})).collect();
        let mut ids: Vec<u64> = db.items.values().filter(|i| matches!(i.kind, ItemKind::Movie | ItemKind::Episode) && !i.is_extra() && !i.extra.contains_key(crate::library::scanner::MISSING)).map(|i| i.id).collect();
        ids.sort_unstable();
        // Who watched what, from the per-person states ("user:item"): for clean-up suggestions.
        let names: HashMap<u64, &str> = db.users.iter().map(|u| (u.id, u.username.as_str())).collect();
        let mut seen: HashMap<u64, (Vec<&str>, Vec<&str>, u64)> = HashMap::new();
        for (key, v) in db.views.iter() {
            let Some((uid, iid)) = key.split_once(':').and_then(|(u, i)| Some((u.parse::<u64>().ok()?, i.parse::<u64>().ok()?))) else { continue };
            let Some(name) = names.get(&uid) else { continue };
            let e = seen.entry(iid).or_default();
            if v.view_count > 0 { e.0.push(name); }
            if v.view_offset_ms > 0 { e.1.push(name); }
            e.2 = e.2.max(v.last_viewed_at);
        }
        let items: Vec<Value> = ids.iter().map(|id| {
            let mut o = detail_in(&db, *id);
            if let Some(it) = db.items.get(id) {
                if let Some(v) = it.version_of() { o["version_of"] = json!(v); }
                o["added_at"] = json!(it.added_at);
            }
            let (watched, started, last) = seen.remove(id).unwrap_or_default();
            o["watched_by"] = json!(watched);
            o["in_progress_by"] = json!(started);
            o["last_played"] = json!(last);
            o
        }).collect();
        let users: Vec<&str> = db.users.iter().map(|u| u.username.as_str()).collect();
        json!({"sections": sections, "users": users, "items": items})
    }

    /// One person's play history from `since` (unix seconds), oldest first: what `GET /api/plugins/{id}/history`
    /// answers. Every viewing of a movie or episode, here or brought in from Plex, with what the item is;
    /// `manual`: marked as watched (by hand, or by Plex without a time).
    pub fn play_history(&self, user: &str, since: u64) -> Option<Value> {
        let db = self.db.lock().unwrap();
        let uid = db.users.iter().find(|u| u.username.eq_ignore_ascii_case(user))?.id;
        let mut items: HashMap<u64, Value> = HashMap::new();
        let plays: Vec<Value> = db.history.iter().filter(|e| e.user_id == uid && e.at >= since)
            .filter(|e| db.items.get(&e.item_id).is_some_and(|i| matches!(i.kind, ItemKind::Movie | ItemKind::Episode)))
            .map(|e| json!({"at": e.at, "manual": e.manual, "item": items.entry(e.item_id).or_insert_with(|| brief_in(&db, e.item_id)).clone()}))
            .collect();
        Some(json!({"user": user, "plays": plays}))
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
        crate::connect::trakt::on_event(self, event, &data);
        if self.listeners(event).is_empty() {
            return;
        }
        let (app, event) = (self.clone(), event.to_string());
        std::thread::spawn(move || app.fire_now(&event, data));
    }

    /// Looks for an item's subtitle files again (next to the video and in the subtitle store).
    /// New ones are synchronized with the sound right away, in the background, when that is on.
    pub fn refresh_subtitles(self: &Arc<Self>, id: u64) {
        let store = self.data_dir.join("subtitles");
        let changed = {
            let mut db = self.db.lock().unwrap();
            let Some(it) = db.items.get_mut(&id) else { return };
            let mut changed = false;
            if let Some(f) = it.file.as_mut() {
                let old = f.external_subtitles.clone();
                if crate::library::scanner::refresh_subtitles(f, &store) {
                    changed = true;
                    let new = f.external_subtitles.clone();
                    if let Some(sync) = it.extra.get_mut("subsync") { crate::library::subsync::renumber_side_files(sync, &old, &new); }
                }
            }
            for f in it.parts.iter_mut() { changed |= crate::library::scanner::refresh_subtitles(f, &store); }
            changed
        };
        if changed {
            self.save();
            if self.settings().sync_subtitles {
                let app = self.clone();
                std::thread::spawn(move || { crate::admin::maintenance::sync_item_subtitles(&app, id); });
            }
        }
    }

    /// Sends `schedule.daily` if it was not sent yet today (UTC). True if it was sent now.
    pub fn daily_tick(self: &Arc<Self>) -> bool {
        let file = self.data_dir.join("plugins").join(".daily-last");
        let today = crate::data::db::today();
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
            // A plugin that keeps collections (smart collections): they are put in place.
            if let Ok(v) = &r { crate::api::handlers::set_plugin_collections(self, v); }
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
pub(crate) mod tests;

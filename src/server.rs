//! The HTTP server core: shared state, responses, the request loop,
//! background jobs and the JSON shapes. Library, hub, playlist and
//! transcode paths follow the names (/library/sections,
//! /library/metadata/{id}, /hubs/home/…, /playlists, /:/timeline,
//! /video/…/transcode); admin features live under /api/.
//! Routes are in routes.rs, the larger handlers in handlers.rs.

use crate::db::{self, now, Db, Invite, Item, ItemKind, Playlist, PlaylistKind, Section, User};
use crate::plugins::{self, Plugins};
use crate::query;
use crate::scanner;
use crate::transcode::Method;
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::Child;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tiny_http::{Header, Response, StatusCode};

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
}

pub struct App {
    pub data_dir: PathBuf,
    pub db_path: PathBuf,
    pub db: Mutex<Db>,
    pub plugins: Mutex<Plugins>,
    pub hls: Mutex<HashMap<String, HlsSession>>,
    pub progressive: Mutex<HashMap<String, Playing>>,
    pub playing: Mutex<HashMap<String, Playing>>,
    pub scanning: Mutex<HashSet<u64>>,
    pub started_at: u64,
    pub port: Mutex<u16>,
    /// Movies and shows waiting for online details: (item id, force).
    pub meta_queue: Mutex<std::collections::VecDeque<(u64, bool)>>,
    /// Item being looked up right now (0 = none).
    pub meta_busy: Mutex<u64>,
    pub meta_progress: Mutex<MetaProgress>,
    /// Background jobs other than details (credits detection): (label, done, total).
    pub job: Mutex<Option<(String, usize, usize)>>,
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
        let db_path = data_dir.join("library.json");
        let mut db = Db::load(&db_path);
        // Older versions saved picture paths relative to the start folder.
        for it in db.items.values_mut() {
            if let Some(p) = it.poster.as_ref().filter(|p| Path::new(p).is_relative() && Path::new(p).exists()) {
                it.poster = std::path::absolute(p).ok().map(|a| a.to_string_lossy().to_string()).or(it.poster.clone());
            }
            if let Some(p) = it.extra.get("art").and_then(|v| v.as_str()).filter(|p| Path::new(p).is_relative() && Path::new(p).exists()) {
                if let Ok(a) = std::path::absolute(p) {
                    it.extra.insert("art".into(), json!(a.to_string_lossy()));
                }
            }
        }
        let plugins = Plugins::load(&data_dir.join("plugins"));
        let app = Arc::new(App {
            data_dir: data_dir.to_path_buf(),
            db_path,
            db: Mutex::new(db),
            plugins: Mutex::new(plugins),
            hls: Mutex::new(HashMap::new()),
            progressive: Mutex::new(HashMap::new()),
            playing: Mutex::new(HashMap::new()),
            scanning: Mutex::new(HashSet::new()),
            started_at: now(),
            port: Mutex::new(DEFAULT_PORT),
            meta_queue: Mutex::new(Default::default()),
            meta_busy: Mutex::new(0),
            meta_progress: Mutex::new(MetaProgress::default()),
            job: Mutex::new(None),
        });
        app.sync_plugin_states();
        app.save();
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
            eprintln!("{} items had a broken picture; getting their pictures again", broken.len());
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

    pub fn save(&self) {
        let db = self.db.lock().unwrap();
        if let Err(e) = db.save(&self.db_path) {
            eprintln!("could not save the library: {e}");
        }
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

    pub fn settings(&self) -> db::Settings {
        self.db.lock().unwrap().settings.clone()
    }

    pub fn server_info(&self) -> Value {
        let port = *self.port.lock().unwrap();
        json!({"url": format!("http://127.0.0.1:{port}"), "version": VERSION, "data_dir": self.data_dir.to_string_lossy()})
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

    /// Tells plugins about something, in the background.
    pub fn fire(self: &Arc<Self>, event: &str, data: Value) {
        let listeners = self.listeners(event);
        if listeners.is_empty() {
            return;
        }
        let app = self.clone();
        let event = event.to_string();
        std::thread::spawn(move || {
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
        {
            let mut s = self.scanning.lock().unwrap();
            if !s.insert(section_id) {
                return None;
            }
        }
        let section = self.db.lock().unwrap().sections.iter().find(|s| s.id == section_id).cloned();
        let report = section.map(|section| {
            let ffprobe = self.settings().ffprobe;
            let report = scanner::scan_section(&section, &ffprobe, |f| {
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
        let app = self.clone();
        std::thread::spawn(move || {
            app.scan_now(section_id);
        });
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
                .filter(|i| db.sections.iter().find(|s| s.id == i.section_id).is_none_or(|s| s.credits_detection))
                .filter_map(|i| i.file.as_ref().map(|f| (i, f)))
                .filter(|(_, f)| force || (f.credits_checked < db::CREDITS_DETECTOR_VERSION && f.credits_errors <= db::CREDITS_MAX_ERRORS))
                .map(|(i, f)| (i.id, f.path.clone(), f.duration_ms.max(i.duration_ms)))
                .collect();
            (files, db.settings.ffmpeg.clone(), db.settings.ffprobe.clone(), db.settings.credits_from_picture)
        };
        let total = files.len();
        if total == 0 {
            return Err(if force { "No videos to check (or credits detection is off for this library)".into() } else { "Everything is already up to date".into() });
        }
        *job = Some(("Detecting credits".into(), 0, total));
        drop(job);
        let app = self.clone();
        std::thread::spawn(move || {
            let (mut found, mut failed) = (0, 0);
            for (n, (id, path, duration)) in files.into_iter().enumerate() {
                let r = crate::scanner::find_credits(&ffmpeg, &ffprobe, &path, duration, picture);
                if let Some(f) = app.db.lock().unwrap().items.get_mut(&id).and_then(|i| i.file.as_mut()) {
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
                            eprintln!("credits detection for item {id}: {e}");
                            if f.credits_errors > db::CREDITS_MAX_ERRORS {
                                eprintln!("credits detection for item {id} has failed too many times; it will not be retried automatically");
                            }
                        }
                    }
                }
                *app.job.lock().unwrap() = Some(("Detecting credits".into(), n + 1, total));
            }
            app.save();
            *app.job.lock().unwrap() = None;
            let mut p = app.meta_progress.lock().unwrap();
            p.done += 1;
            let failed_txt = if failed > 0 { format!(", {failed} could not be read") } else { String::new() };
            p.last = json!({"id": null, "title": "Credits detection", "ok": failed == 0,
                "message": format!("Credits detection finished: credits found in {found} of {total} videos{failed_txt}"), "at": now()});
        });
        Ok(total)
    }

    /// Videos still waiting for automatic credits detection.
    pub fn credits_pending(&self) -> Vec<u64> {
        let db = self.db.lock().unwrap();
        if db.settings.credits_markers_behaviour == "never" {
            return vec![];
        }
        db.items
            .values()
            .filter(|i| db.sections.iter().find(|s| s.id == i.section_id).is_none_or(|s| s.credits_detection))
            .filter(|i| i.file.as_ref().is_some_and(|f| f.credits_checked < db::CREDITS_DETECTOR_VERSION && f.credits_errors <= db::CREDITS_MAX_ERRORS))
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
        let (kind, title, year, stored, seasons, status_recent) = {
            let db = self.db.lock().unwrap();
            let it = db.items.get(&id).ok_or("No such item")?;
            // The library's own language, else the server's.
            settings.metadata_language = db.language_for_section(it.section_id);
            if !matches!(it.kind, ItemKind::Movie | ItemKind::Show) {
                return Err("Details are looked up for movies and shows".into());
            }
            let s = |k: &str| it.extra.get(k).and_then(|v| v.as_str()).map(String::from);
            let stored = s("metadata_source").zip(s("metadata_id"));
            let seasons: Vec<u32> = db.children(id).iter().filter_map(|c| c.index).collect();
            let recent = s("metadata_status").as_deref() == Some("unmatched")
                && it.extra.get("metadata_checked_at").and_then(|v| v.as_u64()).is_some_and(|t| now().saturating_sub(t) < 7 * 86400);
            // Use the name from the file, not a name a previous wrong match gave it.
            let title = s("file_title").unwrap_or_else(|| it.title.clone());
            (it.kind, title, it.year, stored, seasons, recent)
        };
        if choice.is_none() && stored.is_none() && status_recent && !force {
            return Err("No match was found recently; use Fix match".into());
        }
        let (source, mid) = match choice.or(stored) {
            Some(x) => x,
            None => {
                let found = metadata::search(&settings, kind, &title, year);
                let best = found.as_ref().ok().and_then(|c| c.first().cloned()).filter(metadata::acceptable);
                match best {
                    Some(b) => (b.source, b.id),
                    None => {
                        let mut db = self.db.lock().unwrap();
                        if let Some(it) = db.items.get_mut(&id) {
                            it.extra.insert("metadata_status".into(), json!("unmatched"));
                            it.extra.insert("metadata_checked_at".into(), json!(now()));
                        }
                        return Err(match found {
                            Err(e) => e,
                            Ok(c) if c.is_empty() => format!("Nothing found online for \"{title}\""),
                            Ok(c) => format!("No sure match for \"{title}\" (closest: {} {})", c[0].title, c[0].year.map(|y| format!("({y})")).unwrap_or_default()),
                        });
                    }
                }
            }
        };
        let d = metadata::fetch(&settings, kind, &source, &mid, &seasons)?;
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
        for k in ["cast", "directors", "writers", "producers", "countries", "creators", "ids", "art", "original_title", "runtime_minutes", "originally_available", "metadata_status", "metadata_checked_at", "ratings", "imdb_ratings_checked"] {
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
            *self.meta_busy.lock().unwrap() = id;
            let r = self.match_online(id, None, force);
            *self.meta_busy.lock().unwrap() = 0;
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
                eprintln!("details for item {id}: {e}");
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

    pub fn active_transcodes(&self) -> usize {
        self.hls.lock().unwrap().values().filter(|h| h.method == Method::Transcode).count()
            + self.progressive.lock().unwrap().values().filter(|p| p.method == "transcode").count()
    }
}

/// On first start, copies the plugins shipped with the server (switched off)
/// into the data folder, so they appear on the Plugins page.
fn install_bundled_plugins(dest: &Path) {
    let marker = dest.join(".bundled-installed");
    if marker.exists() {
        return;
    }
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
        let to = dest.join(e.file_name());
        if to.exists() {
            continue;
        }
        let _ = std::fs::create_dir_all(&to);
        if let Ok(files) = std::fs::read_dir(&from) {
            for f in files.flatten().filter(|f| f.path().is_file()) {
                let _ = std::fs::copy(f.path(), to.join(f.file_name()));
            }
        }
    }
    let _ = std::fs::write(marker, "bundled example plugins were copied here once\n");
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
        if Path::new(v).exists() {
            it.poster = Some(v.into());
        }
    }
    if let Some(Value::Object(extra)) = meta.get("extra") {
        for (k, v) in extra {
            it.extra.insert(k.clone(), v.clone());
        }
    }
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

// ---------------------------------------------------------------- request context

pub struct Ctx {
    pub method: String,
    pub path: String,
    pub params: Vec<(String, String)>,
    pub body: Value,
    pub user: Option<User>,
    pub host: String,
    pub range: Option<String>,
    pub token: Option<String>,
}

impl Ctx {
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

pub fn serve(app: Arc<App>, server: Arc<tiny_http::Server>) {
    for req in server.incoming_requests() {
        let app = app.clone();
        std::thread::spawn(move || handle(app, req));
    }
}

pub fn start_background(app: &Arc<App>) {
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
        a.playing.lock().unwrap().retain(|_, p| p.updated.elapsed() < Duration::from_secs(120));
    });
    // Online details for new movies and shows.
    let a = app.clone();
    std::thread::spawn(move || a.metadata_worker());
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
                let sig = scanner::folder_signature(&s.paths);
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
            if idle && Instant::now() >= imdb_retry && a.settings().imdb_ratings_file {
                if let Err(e) = a.update_imdb_ratings(false) {
                    eprintln!("IMDb ratings: {e}");
                    imdb_retry = Instant::now() + Duration::from_secs(6 * 3600);
                }
            }
        }
    });
    // Scheduled library scans.
    let a = app.clone();
    std::thread::spawn(move || {
        let mut last: HashMap<u64, Instant> = HashMap::new();
        loop {
            std::thread::sleep(Duration::from_secs(30));
            let (enabled, interval, ids) = {
                let db = a.db.lock().unwrap();
                (db.settings.scheduled_scans_enabled, db.settings.scan_interval_minutes, db.sections.iter().map(|s| s.id).collect::<Vec<_>>())
            };
            if !enabled || interval == 0 {
                continue;
            }
            for id in ids {
                let due = last.get(&id).is_none_or(|t| t.elapsed() >= Duration::from_secs(interval * 60));
                if due {
                    last.insert(id, Instant::now());
                    a.scan_now(id);
                }
            }
        }
    });
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
    let user = token.as_ref().and_then(|t| {
        let db = app.db.lock().unwrap();
        let uid = db.tokens.iter().find(|x| &x.token == t)?.user_id;
        db.user(uid).cloned()
    });
    let ctx = Ctx { method: method.clone(), path, params, body, user, host, range, token };
    let resp = if method == "OPTIONS" { bytes(204, "text/plain", vec![]) } else { crate::routes::route(&app, &ctx) };
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
    });
    if let Some(y) = it.year { o["year"] = json!(y); }
    if let Some(r) = it.rating { o["rating"] = json!(r); }
    if let Some(r) = v.rating { o["userRating"] = json!(r); }
    if !it.content_rating.is_empty() { o["contentRating"] = json!(it.content_rating); }
    if !it.studio.is_empty() { o["studio"] = json!(it.studio); }
    if let Some(i) = it.index { o["index"] = json!(i); }
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
    if let Some(f) = &it.file {
        let vid = f.video();
        let aud = f.audio();
        let mut media = json!({
            "id": it.id,
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
            "Part": [{"key": format!("/library/parts/{}/file", it.id), "size": f.size, "container": f.container}],
        });
        if detail {
            media["streams"] = json!(f.streams);
            let mut subs: Vec<Value> = f
                .streams
                .iter()
                .filter(|s| s.kind == "subtitle")
                .enumerate()
                .map(|(n, s)| json!({"key": format!("/library/parts/{}/subtitles/{}", it.id, n), "language": s.language, "codec": s.codec, "title": s.title, "embedded": true}))
                .collect();
            for (n, p) in f.external_subtitles.iter().enumerate() {
                subs.push(json!({"key": format!("/library/parts/{}/subtitles/x{}", it.id, n), "file": Path::new(p).file_name().map(|s| s.to_string_lossy().to_string()), "embedded": false}));
            }
            media["subtitles"] = json!(subs);
            if user.admin {
                media["file"] = json!(f.path);
            }
        }
        o["Media"] = json!([media]);
    }
    if !it.extra.is_empty() {
        o["extra"] = Value::Object(it.extra.clone());
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
    let top = db.items.values().filter(|i| i.section_id == s.id && i.parent_id.is_none()).count();
    let playable = db.items.values().filter(|i| i.section_id == s.id && i.kind.playable()).count();
    json!({
        "key": s.id.to_string(), "id": s.id, "title": s.name, "type": s.kind.as_str(),
        "Location": s.paths.iter().map(|p| json!({"path": p})).collect::<Vec<_>>(),
        "paths": s.paths, "createdAt": s.created_at, "scannedAt": s.scanned_at, "creditsDetection": s.credits_detection, "language": s.language,
        "count": top, "videoCount": playable,
    })
}

pub fn playlist_json(db: &Db, user: &User, p: &Playlist) -> (Value, usize, u64) {
    let items = query::playlist_items(db, user, p);
    let duration: u64 = items.iter().filter_map(|i| db.items.get(i)).map(|i| i.duration_ms).sum();
    let thumb = items.first().map(|i| format!("/library/metadata/{i}/thumb"));
    let owner = if p.owner_id == 0 { "server".to_string() } else { db.user(p.owner_id).map(|u| u.username.clone()).unwrap_or_default() };
    let kind = if p.is_default() { "default" } else if p.kind == PlaylistKind::Smart { "smart" } else { "manual" };
    let v = json!({
        "ratingKey": p.id.to_string(), "id": p.id, "key": format!("/playlists/{}/items", p.id),
        "type": "playlist", "playlistType": "video", "title": p.title, "summary": p.summary,
        "smart": p.kind == PlaylistKind::Smart, "kind": kind,
        "default": p.is_default(), "defaultKey": p.default_key, "query": p.query,
        "leafCount": items.len(), "duration": duration, "thumb": thumb, "composite": thumb,
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
        "allowed_sections": u.allowed_sections, "allow_transcode": u.allow_transcode.unwrap_or(true), "invited_by": u.invited_by})
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
    if ctx.host.is_empty() { format!("http://localhost:{DEFAULT_PORT}") } else { format!("http://{}", ctx.host) }
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
        Some(it) if db.can_see_section(user, it.section_id) => f(&db, it),
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
        .filter(|i| db.can_see_section(user, i.section_id))
        .filter(|i| match kind {
            Some(k) => i.kind == k,
            None => i.kind.playable(),
        })
        .collect();
    v.sort_by(|a, b| b.added_at.cmp(&a.added_at).then(b.id.cmp(&a.id)));
    v.into_iter().take(limit).map(|i| i.id).collect()
}

/// One week in seconds, for the Continue Watching window:
/// last viewed + weeks × one week >= now.
pub const WEEK_SECONDS: u64 = 604800;

/// Next unwatched episode of each show the user has been watching.
pub fn on_deck(db: &Db, user: &User) -> Vec<u64> {
    let window = db.settings.continue_watching_weeks * WEEK_SECONDS;
    let mut shows: Vec<(u64, u64)> = vec![];
    for show in db.items.values().filter(|i| i.kind == ItemKind::Show && db.can_see_section(user, i.section_id)) {
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
        .filter(|i| i.kind.playable() && db.can_see_section(user, i.section_id))
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

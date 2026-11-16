//! Trakt (trakt.tv), built into the server.
//!
//! Everyone connects their own Trakt account with Trakt's device login (a code to type in at
//! trakt.tv/activate). Then:
//!  - while a movie or episode plays, Trakt shows "watching now" (paused when paused), and once it
//!    is watched it goes into the Trakt history (the scrobble API);
//!  - titles marked as watched by hand go into the history too;
//!  - an automatic sync (and "Sync now") brings watched state, ratings and the watchlist together in
//!    both directions. Nothing is ever removed on either side: what is watched, rated or on the
//!    watchlist on one side is added to the other. When a rating differs, the side that changed it
//!    since the last sync wins (Trakt's, if both did).
//!
//! An admin creates a Trakt app (trakt.tv/oauth/applications/new, redirect URI
//! urn:ietf:wg:oauth:2.0:oob) and puts its Client ID and Client Secret into the settings. Everything
//! is kept in data/trakt.json (the logins too), so it survives updates of a Docker image.
//! Titles are matched by their IMDb / TMDB / TVDB ids (the details download has to be on).

use crate::db::{now, Db, Item, ItemKind, User};
use crate::server::App;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::{Arc, Mutex};
use std::time::Duration;

const API: &str = "https://api.trakt.tv";
const REDIRECT: &str = "urn:ietf:wg:oauth:2.0:oob";
const CONFIG: &str = "trakt.json";
/// A finished scrobble is not sent again by a later stop/watched of the same item.
const SCROBBLED_FOR: u64 = 6 * 3600;
/// Entries per request when pushing to Trakt.
const CHUNK: usize = 100;

static FILE: Mutex<()> = Mutex::new(());
/// Device logins waiting for the code to be typed in: user → (device code, expires, interval seconds).
static PENDING: Mutex<Option<HashMap<u64, (String, u64, u64)>>> = Mutex::new(None);
static SYNCING: Mutex<Option<HashSet<u64>>> = Mutex::new(None);
static SCROBBLED: Mutex<Option<HashMap<(u64, u64), u64>>> = Mutex::new(None);

fn yes() -> bool { true }
fn hour() -> u64 { 60 }

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct Account {
    pub access_token: String,
    pub refresh_token: String,
    pub expires_at: u64,
    /// The Trakt user name (only shown).
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub last_sync: u64,
    /// What the last sync did (shown in the settings).
    #[serde(default)]
    pub report: Value,
    /// What was last sent while playing, as text ("watching Heat").
    #[serde(default)]
    pub last_sent: String,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Config {
    #[serde(default)]
    pub client_id: String,
    #[serde(default)]
    pub client_secret: String,
    #[serde(default = "yes")]
    pub scrobble: bool,
    #[serde(default = "yes")]
    pub send_marked_watched: bool,
    #[serde(default)]
    pub sync_enabled: bool,
    #[serde(default = "yes")]
    pub sync_watched: bool,
    #[serde(default = "yes")]
    pub sync_ratings: bool,
    #[serde(default = "yes")]
    pub sync_watchlist: bool,
    #[serde(default = "hour")]
    pub interval_minutes: u64,
    /// By Lichtspiel user id.
    #[serde(default)]
    pub accounts: BTreeMap<String, Account>,
}

impl Default for Config {
    fn default() -> Self {
        serde_json::from_str("{}").unwrap()
    }
}

impl Config {
    fn configured(&self) -> bool {
        !self.client_id.trim().is_empty() && !self.client_secret.trim().is_empty()
    }
}

// ---------------------------------------------------------------- the settings file

fn path(app: &App) -> std::path::PathBuf {
    app.data_dir.join(CONFIG)
}

pub fn load(app: &App) -> Config {
    std::fs::read(path(app)).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
}

fn write(app: &App, c: &Config) -> Result<(), String> {
    let p = path(app);
    let tmp = p.with_extension("tmp");
    std::fs::write(&tmp, serde_json::to_vec_pretty(c).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, &p).map_err(|e| e.to_string())
}

/// Changes the file under a lock, so two threads never overwrite each other.
fn edit<R>(app: &App, f: impl FnOnce(&mut Config) -> R) -> Result<R, String> {
    let _g = FILE.lock().unwrap();
    let mut c = load(app);
    let r = f(&mut c);
    write(app, &c)?;
    Ok(r)
}

fn hint(s: &str) -> String {
    if s.is_empty() { String::new() } else { format!("…{}", &s[s.len().saturating_sub(4)..]) }
}

/// What the settings page shows for the caller.
pub fn public(app: &App, user: &User) -> Value {
    let c = load(app);
    let acc = c.accounts.get(&user.id.to_string());
    let mut o = json!({
        "configured": c.configured(),
        "connected": acc.is_some(),
        "name": acc.map(|a| a.name.clone()),
        "last_sync": acc.map(|a| a.last_sync).unwrap_or(0),
        "report": acc.map(|a| a.report.clone()).unwrap_or(Value::Null),
        "last_sent": acc.map(|a| a.last_sent.clone()).unwrap_or_default(),
        "syncing": SYNCING.lock().unwrap().as_ref().is_some_and(|s| s.contains(&user.id)),
        "scrobble": c.scrobble, "send_marked_watched": c.send_marked_watched,
        "sync_enabled": c.sync_enabled, "sync_watched": c.sync_watched, "sync_ratings": c.sync_ratings, "sync_watchlist": c.sync_watchlist,
        "interval_minutes": c.interval_minutes,
        "app_url": "https://trakt.tv/oauth/applications/new", "redirect_uri": REDIRECT,
    });
    if user.admin {
        o["client_id"] = json!(c.client_id);
        o["client_secret_hint"] = json!(hint(&c.client_secret));
        o["accounts"] = json!(c.accounts.iter().map(|(id, a)| json!({"user_id": id, "name": a.name, "last_sync": a.last_sync})).collect::<Vec<_>>());
    }
    o
}

/// Admin: change the Trakt app and what is sent.
pub fn update(app: &App, body: &Value) -> Result<(), String> {
    edit(app, |c| {
        if let Some(s) = body["client_id"].as_str() { c.client_id = s.trim().to_string(); }
        if let Some(s) = body["client_secret"].as_str().filter(|s| !s.trim().is_empty()) { c.client_secret = s.trim().to_string(); }
        for (key, slot) in [("scrobble", &mut c.scrobble), ("send_marked_watched", &mut c.send_marked_watched), ("sync_enabled", &mut c.sync_enabled),
            ("sync_watched", &mut c.sync_watched), ("sync_ratings", &mut c.sync_ratings), ("sync_watchlist", &mut c.sync_watchlist)] {
            if let Some(b) = body[key].as_bool() { *slot = b; }
        }
        if let Some(n) = body["interval_minutes"].as_u64() { c.interval_minutes = n.clamp(10, 24 * 60); }
    })
}

// ---------------------------------------------------------------- Trakt's API

#[derive(Debug)]
struct TraktError {
    code: u16,
    text: String,
}

impl std::fmt::Display for TraktError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.code {
            0 => write!(f, "Trakt is not reachable: {}", self.text),
            401 => write!(f, "Trakt did not accept the login (HTTP 401); connect again"),
            c => write!(f, "Trakt answered HTTP {c}"),
        }
    }
}

fn call(c: &Config, method: &str, path: &str, body: Option<&Value>, token: Option<&str>) -> Result<Value, TraktError> {
    let agent = ureq::AgentBuilder::new().timeout(Duration::from_secs(60)).build();
    let mut req = agent.request(method, &format!("{API}{path}"))
        .set("trakt-api-version", "2")
        .set("trakt-api-key", &c.client_id)
        .set("User-Agent", &format!("Lichtspiel/{}", crate::server::VERSION))
        .set("Content-Type", "application/json");
    if let Some(t) = token { req = req.set("Authorization", &format!("Bearer {t}")); }
    let r = match body { Some(b) => req.send_string(&b.to_string()), None => req.call() };
    match r {
        Ok(resp) => {
            let text = resp.into_string().map_err(|e| TraktError { code: 0, text: e.to_string() })?;
            Ok(if text.trim().is_empty() { Value::Null } else { serde_json::from_str(&text).unwrap_or(Value::Null) })
        }
        Err(ureq::Error::Status(code, r)) => Err(TraktError { code, text: r.into_string().unwrap_or_default().chars().take(300).collect() }),
        Err(ureq::Error::Transport(t)) => Err(TraktError { code: 0, text: t.to_string() }),
    }
}

/// The login of one person, refreshed when it is nearly out of date. None: not connected.
fn token_for(app: &App, uid: u64) -> Result<Option<String>, String> {
    let c = load(app);
    let Some(a) = c.accounts.get(&uid.to_string()).cloned() else { return Ok(None) };
    // Trakt's tokens now live only a day or a week and each refresh replaces the refresh token, so refresh only at the very end.
    if a.expires_at > now() + 600 || a.refresh_token.is_empty() { return Ok(Some(a.access_token)); }
    let body = json!({"refresh_token": a.refresh_token, "client_id": c.client_id, "client_secret": c.client_secret, "redirect_uri": REDIRECT, "grant_type": "refresh_token"});
    match call(&c, "POST", "/oauth/token", Some(&body), None) {
        Ok(t) => {
            let access = t["access_token"].as_str().unwrap_or("").to_string();
            if access.is_empty() { return Err("Trakt sent no token".into()); }
            let refresh = t["refresh_token"].as_str().unwrap_or(&a.refresh_token).to_string();
            let expires = t["created_at"].as_u64().unwrap_or_else(now) + t["expires_in"].as_u64().unwrap_or(7_776_000);
            edit(app, |c| if let Some(x) = c.accounts.get_mut(&uid.to_string()) { x.access_token = access.clone(); x.refresh_token = refresh; x.expires_at = expires; })?;
            Ok(Some(access))
        }
        // Only a refresh token Trakt no longer knows means the login is gone; a wrong Client Secret
        // (invalid_client) or a hiccup must not throw away everyone's login.
        Err(e) if (e.code == 400 || e.code == 401) && e.text.contains("invalid_grant") => {
            edit(app, |c| { c.accounts.remove(&uid.to_string()); })?;
            Ok(None)
        }
        Err(e) => Err(e.to_string()),
    }
}

// ---------------------------------------------------------------- connecting

/// Starts the device login: the answer has the code to type in at the link.
pub fn connect(app: &App, user: &User) -> Result<Value, String> {
    let c = load(app);
    if !c.configured() { return Err("Trakt is not set up yet: an admin has to enter the Client ID and Client Secret first".into()); }
    let r = call(&c, "POST", "/oauth/device/code", Some(&json!({"client_id": c.client_id})), None).map_err(|e| e.to_string())?;
    let (Some(dev), Some(code)) = (r["device_code"].as_str(), r["user_code"].as_str()) else { return Err("Trakt sent no code".into()) };
    let expires = r["expires_in"].as_u64().unwrap_or(600);
    let interval = r["interval"].as_u64().unwrap_or(5).max(2);
    PENDING.lock().unwrap().get_or_insert_with(Default::default).insert(user.id, (dev.to_string(), now() + expires, interval));
    Ok(json!({"code": code, "url": r["verification_url"].as_str().unwrap_or("https://trakt.tv/activate"), "expires_in": expires, "interval": interval}))
}

/// Asks whether the code was typed in: {"status": "waiting"|"connected"|"expired"|"denied"}.
pub fn poll(app: &App, user: &User) -> Result<Value, String> {
    let (dev, expires, _) = PENDING.lock().unwrap().get_or_insert_with(Default::default).get(&user.id).cloned().ok_or("Start the connection first")?;
    if expires <= now() {
        PENDING.lock().unwrap().get_or_insert_with(Default::default).remove(&user.id);
        return Ok(json!({"status": "expired"}));
    }
    let c = load(app);
    match call(&c, "POST", "/oauth/device/token", Some(&json!({"code": dev, "client_id": c.client_id, "client_secret": c.client_secret})), None) {
        Ok(t) => {
            PENDING.lock().unwrap().get_or_insert_with(Default::default).remove(&user.id);
            let access = t["access_token"].as_str().unwrap_or("").to_string();
            if access.is_empty() { return Err("Trakt sent no token".into()); }
            let name = call(&c, "GET", "/users/settings", None, Some(&access)).ok().and_then(|s| s["user"]["username"].as_str().map(String::from)).unwrap_or_default();
            let acc = Account {
                access_token: access, refresh_token: t["refresh_token"].as_str().unwrap_or("").to_string(),
                expires_at: t["created_at"].as_u64().unwrap_or_else(now) + t["expires_in"].as_u64().unwrap_or(7_776_000), name, ..Default::default()
            };
            edit(app, |c| { c.accounts.insert(user.id.to_string(), acc); })?;
            Ok(json!({"status": "connected"}))
        }
        Err(e) => match e.code {
            400 | 429 => Ok(json!({"status": "waiting"})), // not entered yet / asked too fast
            404 | 409 | 410 => { PENDING.lock().unwrap().get_or_insert_with(Default::default).remove(&user.id); Ok(json!({"status": "expired"})) }
            418 => { PENDING.lock().unwrap().get_or_insert_with(Default::default).remove(&user.id); Ok(json!({"status": "denied"})) }
            _ => Err(e.to_string()),
        },
    }
}

pub fn disconnect(app: &App, user: &User) -> Result<(), String> {
    let c = load(app);
    if let Some(a) = c.accounts.get(&user.id.to_string()) {
        let _ = call(&c, "POST", "/oauth/revoke", Some(&json!({"token": a.access_token, "client_id": c.client_id, "client_secret": c.client_secret})), None);
    }
    edit(app, |c| { c.accounts.remove(&user.id.to_string()); })
}

// ---------------------------------------------------------------- ids and items

/// The ids Trakt knows (imdb text; tmdb, tvdb numbers). Empty when there are none.
fn trakt_ids(ids: &Value) -> Value {
    let mut o = serde_json::Map::new();
    for k in ["imdb", "tmdb", "tvdb", "trakt", "slug"] {
        match &ids[k] {
            Value::String(s) if !s.trim().is_empty() => {
                if matches!(k, "tmdb" | "tvdb" | "trakt") { if let Ok(n) = s.trim().parse::<u64>() { o.insert(k.into(), json!(n)); } } else { o.insert(k.into(), json!(s.trim())); }
            }
            Value::Number(n) if n.as_u64().unwrap_or(0) > 0 => { o.insert(k.into(), Value::Number(n.clone())); }
            _ => {}
        }
    }
    Value::Object(o)
}

fn has_ids(ids: &Value) -> bool {
    ids.as_object().is_some_and(|o| !o.is_empty())
}

/// "imdb:tt123" … for each id a title has; two titles match when they share one.
fn keys(ids: &Value) -> Vec<String> {
    let Value::Object(m) = ids else { return vec![] };
    m.iter().filter_map(|(k, v)| {
        if !matches!(k.as_str(), "imdb" | "tmdb" | "tvdb") { return None; }
        let s = match v { Value::String(s) => s.trim().to_ascii_lowercase(), Value::Number(n) => n.to_string(), _ => return None };
        (!s.is_empty() && s != "0").then(|| format!("{k}:{s}"))
    }).collect()
}

fn local_ids(it: &Item) -> Value {
    trakt_ids(it.extra.get("ids").unwrap_or(&Value::Null))
}

fn title_obj(title: &str, year: &Value, ids: &Value) -> Value {
    let mut o = json!({"title": title, "ids": trakt_ids(ids)});
    if year.as_u64().is_some() { o["year"] = year.clone(); }
    o
}

/// What the scrobble API wants for a movie or an episode (None for anything else).
fn scrobble_body(item: &Value) -> Option<Value> {
    match item["type"].as_str()? {
        "movie" => Some(json!({"movie": title_obj(item["title"].as_str().unwrap_or(""), &item["year"], &item["ids"])})),
        "episode" if item["show"].is_object() && item["season"].is_number() && item["episode"].is_number() => {
            let s = &item["show"];
            Some(json!({"show": title_obj(s["title"].as_str().unwrap_or(""), &s["year"], &s["ids"]), "episode": {"season": item["season"], "number": item["episode"]}}))
        }
        _ => None,
    }
}

/// /sync/history for a movie, episode, season or whole show.
fn history_body(item: &Value, at: &str) -> Option<Value> {
    let kind = item["type"].as_str()?;
    if kind == "movie" {
        let mut m = scrobble_body(item)?["movie"].clone();
        m["watched_at"] = json!(at);
        return Some(json!({"movies": [m]}));
    }
    let show = match kind { "episode" | "season" => &item["show"], "show" => item, _ => return None };
    if !show.is_object() { return None; }
    let mut s = title_obj(show["title"].as_str().unwrap_or(""), &show["year"], &show["ids"]);
    match kind {
        "show" => s["watched_at"] = json!(at),
        "season" => s["seasons"] = json!([{"number": item["season"], "watched_at": at}]),
        _ => s["seasons"] = json!([{"number": item["season"], "episodes": [{"number": item["episode"], "watched_at": at}]}]),
    }
    Some(json!({"shows": [s]}))
}

fn label(item: &Value) -> String {
    if item["type"] == "episode" && item["show"].is_object() {
        format!("{} S{:02}E{:02}", item["show"]["title"].as_str().unwrap_or("?"), item["season"].as_u64().unwrap_or(0), item["episode"].as_u64().unwrap_or(0))
    } else {
        item["title"].as_str().unwrap_or("?").to_string()
    }
}

// ---------------------------------------------------------------- dates

/// 2024-05-01T20:15:00.000Z → unix seconds.
fn unix(s: &str) -> Option<u64> {
    let b = s.get(..19)?;
    let n = |r: std::ops::Range<usize>| b.get(r)?.parse::<i64>().ok();
    let days = crate::query::days_from_civil(n(0..4)?, n(5..7)? as u32, n(8..10)? as u32);
    let t = days * 86400 + n(11..13)? * 3600 + n(14..16)? * 60 + n(17..19)?;
    u64::try_from(t).ok()
}

fn iso(t: u64) -> String {
    let (y, m, d) = crate::rss::civil((t / 86400) as i64);
    let r = t % 86400;
    format!("{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}.000Z", r / 3600, r % 3600 / 60, r % 60)
}

// ---------------------------------------------------------------- playing

/// Called for every event the server fires; only playback matters here.
pub fn on_event(app: &Arc<App>, event: &str, data: &Value) {
    if !event.starts_with("playback.") { return; }
    let Some(name) = data["user"].as_str() else { return };
    let (Some(iid), true) = (data["id"].as_u64(), data.get("by_owner").is_none()) else { return };
    let c = load(app);
    if !c.scrobble || !c.configured() || c.accounts.is_empty() { return; }
    let Some(uid) = app.db.lock().unwrap().users.iter().find(|u| u.username == name).map(|u| u.id) else { return };
    if !c.accounts.contains_key(&uid.to_string()) { return; }
    let (app, event, data) = (app.clone(), event.to_string(), data.clone());
    std::thread::spawn(move || {
        if let Err(e) = scrobble(&app, uid, iid, &event, &data) { elog!("Trakt: {e}"); }
    });
}

fn scrobble(app: &Arc<App>, uid: u64, iid: u64, event: &str, data: &Value) -> Result<(), String> {
    let c = load(app);
    let Some(token) = token_for(app, uid)? else { return Ok(()) };
    let item = app.item_brief(iid);
    let dur = item["duration_ms"].as_u64().unwrap_or(0);
    let progress = match (data["time"].as_u64(), dur) { (Some(t), d) if d > 0 => (t as f64 / d as f64 * 100.0).clamp(0.0, 100.0), _ => 0.0 };
    let recently = SCROBBLED.lock().unwrap().get_or_insert_with(Default::default).get(&(uid, iid)).is_some_and(|t| now() < t + SCROBBLED_FOR);

    let note = |what: &str| { let l = format!("{what}: {}", label(&item)); let _ = edit(app, |c| if let Some(a) = c.accounts.get_mut(&uid.to_string()) { a.last_sent = l; }); };

    if event == "playback.watched" && data["manual"] == json!(true) {
        if !c.send_marked_watched { return Ok(()); }
        let Some(body) = history_body(&item, &iso(now())) else { return Ok(()) };
        call(&c, "POST", "/sync/history", Some(&body), Some(&token)).map_err(|e| e.to_string())?;
        note("history");
        return Ok(());
    }
    let Some(mut body) = scrobble_body(&item) else { return Ok(()) };
    let (action, progress) = match event {
        "playback.watched" if recently => return Ok(()),
        // Watched here (threshold or credits) counts as watched on Trakt too.
        "playback.watched" => ("stop", progress.max(80.0)),
        "playback.stopped" if recently => return Ok(()),
        "playback.stopped" => ("stop", progress),
        _ if data["state"] == "paused" => ("pause", progress),
        _ => ("start", progress),
    };
    body["progress"] = json!((progress * 100.0).round() / 100.0);
    match call(&c, "POST", &format!("/scrobble/{action}"), Some(&body), Some(&token)) {
        Ok(_) => {}
        // Trakt already has this scrobble.
        Err(e) if e.code == 409 => { SCROBBLED.lock().unwrap().get_or_insert_with(Default::default).insert((uid, iid), now()); return Ok(()); }
        Err(e) => return Err(e.to_string()),
    }
    let mut g = SCROBBLED.lock().unwrap();
    let m = g.get_or_insert_with(Default::default);
    if action == "stop" && progress >= 80.0 { m.insert((uid, iid), now()); }
    else if action == "start" && recently && progress < 10.0 { m.remove(&(uid, iid)); } // watching it again from the start
    m.retain(|_, t| now() < *t + SCROBBLED_FOR);
    drop(g);
    note(action);
    Ok(())
}

// ---------------------------------------------------------------- sync

/// A title of the library for a Trakt entry: the movies and shows by any of their ids.
struct Index {
    movies: HashMap<String, u64>,
    shows: HashMap<String, u64>,
    /// (show, season, episode) → episode.
    episodes: HashMap<(u64, u32, u32), u64>,
}

fn index(db: &Db, user: &User) -> Index {
    let mut ix = Index { movies: HashMap::new(), shows: HashMap::new(), episodes: HashMap::new() };
    for it in db.items.values() {
        if !matches!(it.kind, ItemKind::Movie | ItemKind::Show) || it.hidden() || !db.can_see_item(user, it) { continue; }
        let map = if it.kind == ItemKind::Movie { &mut ix.movies } else { &mut ix.shows };
        for k in keys(it.extra.get("ids").unwrap_or(&Value::Null)) { map.entry(k).or_insert(it.id); }
        if it.kind == ItemKind::Show {
            for leaf in db.leaves(it.id) {
                let ep = db.items.get(&leaf);
                let season = ep.and_then(|e| e.parent_id).and_then(|p| db.items.get(&p));
                if let (Some(e), Some(s)) = (ep.and_then(|e| e.index), season.and_then(|s| s.index)) { ix.episodes.insert((it.id, s, e), leaf); }
            }
        }
    }
    ix
}

fn find(map: &HashMap<String, u64>, ids: &Value) -> Option<u64> {
    keys(ids).iter().find_map(|k| map.get(k)).copied()
}

fn get(c: &Config, token: &str, path: &str) -> Result<Vec<Value>, String> {
    match call(c, "GET", path, None, Some(token)).map_err(|e| e.to_string())? {
        Value::Array(a) => Ok(a),
        _ => Ok(vec![]),
    }
}

fn post_chunks(c: &Config, token: &str, path: &str, field: &str, list: Vec<Value>) -> Result<usize, String> {
    let n = list.len();
    for chunk in list.chunks(if field == "shows" { 25 } else { CHUNK }) {
        call(c, "POST", path, Some(&json!({ field: chunk })), Some(token)).map_err(|e| e.to_string())?;
    }
    Ok(n)
}

/// One person's sync, both ways. Returns the report.
fn sync_user(app: &Arc<App>, uid: u64) -> Result<Value, String> {
    let started = now();
    let c = load(app);
    let Some(token) = token_for(app, uid)? else { return Err("Not connected to Trakt".into()) };
    let Some(user) = app.db.lock().unwrap().user(uid).cloned() else { return Err("No such user".into()) };
    let last = c.accounts.get(&uid.to_string()).map(|a| a.last_sync).unwrap_or(0);
    let mut report = json!({"pulled_watched": 0, "pushed_watched": 0, "pulled_ratings": 0, "pushed_ratings": 0, "pulled_watchlist": 0, "pushed_watchlist": 0, "no_ids": 0});
    let bump = |r: &mut Value, k: &str, n: usize| { r[k] = json!(r[k].as_u64().unwrap_or(0) + n as u64); };

    // ---- watched
    let mut remote_movies: HashSet<String> = HashSet::new();
    let mut remote_eps: HashSet<(String, u64, u64)> = HashSet::new();
    if c.sync_watched {
        let mut marks: Vec<Value> = vec![];
        for m in get(&c, &token, "/sync/watched/movies")? {
            for k in keys(&m["movie"]["ids"]) { remote_movies.insert(k); }
            marks.push(json!({"type": "movie", "ids": m["movie"]["ids"], "plays": m["plays"], "watched_at": m["last_watched_at"].as_str().and_then(unix)}));
        }
        for s in get(&c, &token, "/sync/watched/shows")? {
            let ks = keys(&s["show"]["ids"]);
            for season in s["seasons"].as_array().into_iter().flatten() {
                for ep in season["episodes"].as_array().into_iter().flatten() {
                    for k in &ks { remote_eps.insert((k.clone(), season["number"].as_u64().unwrap_or(0), ep["number"].as_u64().unwrap_or(0))); }
                    marks.push(json!({"type": "episode", "show": {"ids": s["show"]["ids"]}, "season": season["number"], "episode": ep["number"], "plays": ep["plays"], "watched_at": ep["last_watched_at"].as_str().and_then(unix)}));
                }
            }
        }
        let (_, marked) = crate::handlers::mark_watched_from_plugin(app, &user, &marks);
        bump(&mut report, "pulled_watched", marked);

        // What is watched here and not on Trakt goes there.
        let (movies, shows) = {
            let db = app.db.lock().unwrap();
            let mut movies: Vec<Value> = vec![];
            let mut shows: BTreeMap<u64, Vec<(u32, u32, u64)>> = BTreeMap::new();
            let mut no_ids = 0;
            for it in db.items.values().filter(|i| !i.hidden() && db.can_see_item(&user, i)) {
                let v = db.view(uid, it.id);
                if v.view_count == 0 { continue; }
                let at = if v.last_viewed_at > 0 { v.last_viewed_at } else { started };
                match it.kind {
                    ItemKind::Movie => {
                        let ids = local_ids(it);
                        if !has_ids(&ids) { no_ids += 1; }
                        else if !keys(&ids).iter().any(|k| remote_movies.contains(k)) { movies.push(json!({"ids": ids, "watched_at": iso(at)})); }
                    }
                    ItemKind::Episode => {
                        let season = it.parent_id.and_then(|p| db.items.get(&p));
                        let show = season.and_then(|s| s.parent_id).and_then(|p| db.items.get(&p));
                        if let (Some(e), Some(s), Some(show)) = (it.index, season.and_then(|s| s.index), show) {
                            let ks = keys(show.extra.get("ids").unwrap_or(&Value::Null));
                            if ks.is_empty() { no_ids += 1; }
                            else if !ks.iter().any(|k| remote_eps.contains(&(k.clone(), s as u64, e as u64))) { shows.entry(show.id).or_default().push((s, e, at)); }
                        }
                    }
                    _ => {}
                }
            }
            bump(&mut report, "no_ids", no_ids);
            let shows: Vec<Value> = shows.into_iter().filter_map(|(sid, eps)| {
                let show = db.items.get(&sid)?;
                let mut by_season: BTreeMap<u32, Vec<Value>> = BTreeMap::new();
                for (s, e, at) in eps { by_season.entry(s).or_default().push(json!({"number": e, "watched_at": iso(at)})); }
                Some(json!({"ids": local_ids(show), "seasons": by_season.into_iter().map(|(n, e)| json!({"number": n, "episodes": e})).collect::<Vec<_>>()}))
            }).collect();
            (movies, shows)
        };
        let n = post_chunks(&c, &token, "/sync/history", "movies", movies)? + post_chunks(&c, &token, "/sync/history", "shows", shows)?;
        bump(&mut report, "pushed_watched", n);
    }

    // ---- ratings
    if c.sync_ratings {
        let (movies, shows, eps) = (get(&c, &token, "/sync/ratings/movies")?, get(&c, &token, "/sync/ratings/shows")?, get(&c, &token, "/sync/ratings/episodes")?);
        let ix = { let db = app.db.lock().unwrap(); index(&db, &user) };
        let mut remote: HashMap<u64, (f32, u64)> = HashMap::new();
        let rated = |v: &Value| (v["rating"].as_f64().unwrap_or(0.0) as f32, v["rated_at"].as_str().and_then(unix).unwrap_or(0));
        for m in &movies { if let Some(i) = find(&ix.movies, &m["movie"]["ids"]) { remote.insert(i, rated(m)); } }
        for s in &shows { if let Some(i) = find(&ix.shows, &s["show"]["ids"]) { remote.insert(i, rated(s)); } }
        for e in &eps {
            let Some(show) = find(&ix.shows, &e["show"]["ids"]) else { continue };
            if let Some(i) = ix.episodes.get(&(show, e["episode"]["season"].as_u64().unwrap_or(0) as u32, e["episode"]["number"].as_u64().unwrap_or(0) as u32)) { remote.insert(*i, rated(e)); }
        }
        let mut pull = 0;
        let (mut push_m, mut push_s, mut push_e): (Vec<Value>, Vec<Value>, Vec<Value>) = (vec![], vec![], vec![]);
        {
            let mut db = app.db.lock().unwrap();
            let ids: Vec<u64> = db.items.values().filter(|i| matches!(i.kind, ItemKind::Movie | ItemKind::Show | ItemKind::Episode) && !i.hidden() && db.can_see_item(&user, i)).map(|i| i.id).collect();
            for id in ids {
                let local = db.view(uid, id).rating.filter(|r| *r > 0.0).map(|r| r.round().clamp(1.0, 10.0));
                let rem = remote.get(&id).copied();
                let take_remote = match (local, rem) { (None, Some(_)) => true, (Some(l), Some((r, at))) => l != r && at > last, _ => false };
                if take_remote {
                    db.view_mut(uid, id).rating = rem.map(|r| r.0);
                    pull += 1;
                    continue;
                }
                let Some(l) = local else { continue };
                if rem.is_some_and(|(r, _)| r == l) { continue; }
                let Some(it) = db.items.get(&id) else { continue };
                let at = iso(db.view(uid, id).last_viewed_at.max(1));
                match it.kind {
                    ItemKind::Movie if has_ids(&local_ids(it)) => push_m.push(json!({"ids": local_ids(it), "rating": l, "rated_at": at})),
                    ItemKind::Show if has_ids(&local_ids(it)) => push_s.push(json!({"ids": local_ids(it), "rating": l, "rated_at": at})),
                    ItemKind::Episode => {
                        let season = it.parent_id.and_then(|p| db.items.get(&p));
                        let show = season.and_then(|s| s.parent_id).and_then(|p| db.items.get(&p));
                        if let (Some(e), Some(s), Some(show)) = (it.index, season.and_then(|s| s.index), show) {
                            if has_ids(&local_ids(show)) { push_e.push(json!({"ids": local_ids(show), "seasons": [{"number": s, "episodes": [{"number": e, "rating": l, "rated_at": at}]}]})); }
                        }
                    }
                    _ => {}
                }
            }
        }
        if pull > 0 { app.save(); }
        bump(&mut report, "pulled_ratings", pull);
        let n = post_chunks(&c, &token, "/sync/ratings", "movies", push_m)? + post_chunks(&c, &token, "/sync/ratings", "shows", push_s)? + post_chunks(&c, &token, "/sync/ratings", "shows", push_e)?;
        bump(&mut report, "pushed_ratings", n);
    }

    // ---- watchlist
    if c.sync_watchlist {
        let (movies, shows) = (get(&c, &token, "/sync/watchlist/movies")?, get(&c, &token, "/sync/watchlist/shows")?);
        let ix = { let db = app.db.lock().unwrap(); index(&db, &user) };
        let mut remote: HashSet<u64> = HashSet::new();
        for m in &movies { if let Some(i) = find(&ix.movies, &m["movie"]["ids"]) { remote.insert(i); } }
        for s in &shows { if let Some(i) = find(&ix.shows, &s["show"]["ids"]) { remote.insert(i); } }
        let (mut pull, mut push_m, mut push_s) = (0, vec![], vec![]);
        {
            let mut db = app.db.lock().unwrap();
            for id in remote.iter() { let v = db.view_mut(uid, *id); if v.watchlist_at == 0 { v.watchlist_at = started; pull += 1; } }
            let on: Vec<u64> = db.items.values().filter(|i| matches!(i.kind, ItemKind::Movie | ItemKind::Show) && !i.hidden() && db.can_see_item(&user, i) && db.view(uid, i.id).watchlist_at > 0 && !remote.contains(&i.id)).map(|i| i.id).collect();
            for id in on {
                let Some(it) = db.items.get(&id) else { continue };
                let ids = local_ids(it);
                if !has_ids(&ids) { continue; }
                if it.kind == ItemKind::Movie { push_m.push(json!({"ids": ids})); } else { push_s.push(json!({"ids": ids})); }
            }
        }
        if pull > 0 { app.save(); }
        bump(&mut report, "pulled_watchlist", pull);
        let n = post_chunks(&c, &token, "/sync/watchlist", "movies", push_m)? + post_chunks(&c, &token, "/sync/watchlist", "shows", push_s)?;
        bump(&mut report, "pushed_watchlist", n);
    }
    report["at"] = json!(started);
    Ok(report)
}

/// Runs one person's sync in the background (false: one is already running).
pub fn sync_now(app: &Arc<App>, uid: u64) -> bool {
    if !SYNCING.lock().unwrap().get_or_insert_with(Default::default).insert(uid) { return false; }
    let app = app.clone();
    std::thread::spawn(move || {
        let r = sync_user(&app, uid);
        let report = match &r { Ok(v) => v.clone(), Err(e) => { elog!("Trakt sync: {e}"); json!({"error": e, "at": now()}) } };
        let _ = edit(&app, |c| if let Some(a) = c.accounts.get_mut(&uid.to_string()) { a.last_sync = now(); a.report = report; });
        SYNCING.lock().unwrap().get_or_insert_with(Default::default).remove(&uid);
    });
    true
}

/// Syncs everyone who is connected, every few minutes, when switched on.
pub fn start(app: &Arc<App>) {
    let app = app.clone();
    std::thread::spawn(move || loop {
        std::thread::sleep(Duration::from_secs(60));
        let c = load(&app);
        if !c.sync_enabled || !c.configured() { continue; }
        for (id, a) in &c.accounts {
            if now() < a.last_sync + c.interval_minutes * 60 { continue; }
            if let Ok(uid) = id.parse::<u64>() { sync_now(&app, uid); }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dates_round_trip() {
        assert_eq!(unix("2024-05-01T20:15:00.000Z"), Some(1714594500));
        assert_eq!(iso(1714594500), "2024-05-01T20:15:00.000Z");
        assert_eq!(unix("nonsense"), None);
    }

    #[test]
    fn bodies_for_trakt() {
        let movie = json!({"type": "movie", "title": "Heat", "year": 1995, "ids": {"imdb": "tt0113277", "tmdb": "949", "tvdb": ""}});
        let b = scrobble_body(&movie).unwrap();
        assert_eq!(b["movie"]["ids"], json!({"imdb": "tt0113277", "tmdb": 949}));
        assert_eq!(b["movie"]["year"], 1995);
        let ep = json!({"type": "episode", "season": 2, "episode": 5, "show": {"title": "Dark", "ids": {"tvdb": 334824}}});
        let b = scrobble_body(&ep).unwrap();
        assert_eq!(b["episode"], json!({"season": 2, "number": 5}));
        assert_eq!(label(&ep), "Dark S02E05");
        assert!(scrobble_body(&json!({"type": "track"})).is_none());
        let h = history_body(&ep, "2024-01-01T00:00:00.000Z").unwrap();
        assert_eq!(h["shows"][0]["seasons"][0]["episodes"][0]["number"], 5);
        assert_eq!(history_body(&json!({"type": "show", "title": "Dark", "ids": {"tvdb": 1}}), "x").unwrap()["shows"][0]["watched_at"], "x");
    }

    #[test]
    fn titles_match_by_any_shared_id() {
        let a = keys(&json!({"imdb": "TT1", "tmdb": 5}));
        let b = keys(&json!({"tmdb": "5", "tvdb": 9}));
        assert!(a.iter().any(|k| b.contains(k)));
        assert!(keys(&json!({"tmdb": 0, "imdb": ""})).is_empty());
    }
}

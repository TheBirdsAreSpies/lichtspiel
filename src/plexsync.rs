//! Keeps watched state, resume points and ratings in step with a running
//! Plex Media Server (both ways, or one way only: the master side wins), through Plex's own HTTP API.
//!
//! Each Plex account (its X-Plex-Token) belongs to one person here. Movies and
//! episodes are matched by their file path (folder paths can be translated,
//! like for the import). Every run remembers what both sides agreed on; on the
//! next run a field that changed on one side only goes to the other side, and
//! when both changed, the side watched more recently wins. On the very first
//! run nothing is lost: watched on either side stays watched, a rating set on
//! either side is kept, and the newer resume point wins.
//!
//! The settings live in data/plex-sync.json, what was agreed in
//! data/plex-sync-state.json and the last run's report in
//! data/cache/plex-sync-last.json.

use crate::db::{now, ItemKind};
use crate::migrate::{map_path, path_key};
use crate::server::App;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

const CONFIG: &str = "plex-sync.json";
const STATE: &str = "plex-sync-state.json";
const IDENT: &str = "com.plexapp.plugins.library";
/// Resume points this close together count as the same.
const OFFSET_SLACK_MS: u64 = 5000;
/// Items asked from Plex per request.
const PAGE: usize = 1000;

static RUNNING: AtomicBool = AtomicBool::new(false);

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct Account {
    pub id: String,
    pub token: String,
    pub user_id: u64,
    /// The Plex account's name, as plex.tv tells it (only shown).
    #[serde(default)]
    pub plex_name: String,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct PathRule {
    pub from: String,
    pub to: String,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Config {
    #[serde(default)]
    pub enabled: bool,
    /// e.g. http://192.168.1.10:32400
    #[serde(default)]
    pub url: String,
    #[serde(default = "interval")]
    pub interval_minutes: u64,
    #[serde(default = "yes")]
    pub watched: bool,
    #[serde(default = "yes")]
    pub resume: bool,
    #[serde(default = "yes")]
    pub ratings: bool,
    /// "both", "from_plex" (Plex is the master: this server copies it) or "to_plex" (this server is the master).
    #[serde(default = "both")]
    pub direction: String,
    /// Plex's folder → the same folder here.
    #[serde(default)]
    pub path_map: Vec<PathRule>,
    #[serde(default)]
    pub accounts: Vec<Account>,
    /// A random id Plex sees as the client's.
    #[serde(default)]
    pub client_id: String,
}

fn interval() -> u64 {
    15
}
fn yes() -> bool {
    true
}
fn both() -> String {
    "both".into()
}

impl Default for Config {
    fn default() -> Self {
        Config { enabled: false, url: String::new(), interval_minutes: interval(), watched: true, resume: true, ratings: true, direction: both(), path_map: vec![], accounts: vec![], client_id: String::new() }
    }
}

/// What both sides agreed on for one person and one item.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
struct Snap {
    #[serde(default)]
    w: bool,
    #[serde(default)]
    o: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    r: Option<f32>,
}

#[derive(Serialize, Deserialize, Default)]
struct State {
    /// The server the state belongs to; another address starts over.
    #[serde(default)]
    url: String,
    /// "user id:item id"
    #[serde(default)]
    items: BTreeMap<String, Snap>,
}

fn config_path(app: &App) -> PathBuf {
    app.data_dir.join(CONFIG)
}
fn state_path(app: &App) -> PathBuf {
    app.data_dir.join(STATE)
}
fn last_path(app: &App) -> PathBuf {
    app.data_dir.join("cache").join("plex-sync-last.json")
}

pub fn load(app: &App) -> Config {
    let mut c: Config = std::fs::read(config_path(app)).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default();
    if c.client_id.is_empty() {
        c.client_id = format!("lichtspiel-{}", crate::db::random_hex(8));
    }
    c
}

fn write_json<T: Serialize>(path: &PathBuf, v: &T) -> Result<(), String> {
    if let Some(d) = path.parent() {
        let _ = std::fs::create_dir_all(d);
    }
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, serde_json::to_vec_pretty(v).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, path).map_err(|e| e.to_string())
}

fn base(url: &str) -> String {
    let u = url.trim().trim_end_matches('/');
    if u.starts_with("http://") || u.starts_with("https://") { u.to_string() } else { format!("http://{u}") }
}

fn hint(token: &str) -> String {
    let n = token.chars().count();
    if n <= 4 { "…".into() } else { format!("…{}", token.chars().skip(n - 4).collect::<String>()) }
}

/// The settings as the web app sees them (tokens only as their last letters).
pub fn public(app: &App) -> Value {
    let c = load(app);
    let db = app.db.lock().unwrap();
    let last = std::fs::read(last_path(app)).ok().and_then(|b| serde_json::from_slice::<Value>(&b).ok());
    json!({
        "enabled": c.enabled, "url": c.url, "interval_minutes": c.interval_minutes,
        "watched": c.watched, "resume": c.resume, "ratings": c.ratings, "direction": c.direction,
        "path_map": c.path_map,
        "accounts": c.accounts.iter().map(|a| json!({"id": a.id, "token_hint": hint(&a.token), "user_id": a.user_id, "plex_name": a.plex_name,
            "username": db.user(a.user_id).map(|u| u.username.clone())})).collect::<Vec<_>>(),
        "running": RUNNING.load(Ordering::SeqCst),
        "last": last,
    })
}

/// Saves new settings: {enabled, url, interval_minutes, watched, resume, ratings,
/// path_map: [{from, to}], accounts: [{id?, token?, user_id}]} (a known id
/// without a token keeps its token).
pub fn update(app: &App, body: &Value) -> Result<Value, String> {
    let mut c = load(app);
    if let Some(v) = body.get("enabled").and_then(Value::as_bool) { c.enabled = v; }
    if let Some(v) = body.get("url").and_then(Value::as_str) { c.url = v.trim().to_string(); }
    if let Some(v) = body.get("interval_minutes").and_then(Value::as_u64) { c.interval_minutes = v.clamp(1, 24 * 60); }
    for (k, f) in [("watched", &mut c.watched), ("resume", &mut c.resume), ("ratings", &mut c.ratings)] {
        if let Some(v) = body.get(k).and_then(Value::as_bool) { *f = v; }
    }
    if let Some(v) = body.get("direction").and_then(Value::as_str) {
        if !["both", "from_plex", "to_plex"].contains(&v) { return Err("Direction must be both, from_plex or to_plex".into()); }
        c.direction = v.into();
    }
    if let Some(list) = body.get("path_map").and_then(Value::as_array) {
        c.path_map = list.iter().map(|r| PathRule { from: r["from"].as_str().unwrap_or("").trim().into(), to: r["to"].as_str().unwrap_or("").trim().into() })
            .filter(|r| !r.from.is_empty() && !r.to.is_empty()).collect();
    }
    if let Some(list) = body.get("accounts").and_then(Value::as_array) {
        let users: Vec<u64> = app.db.lock().unwrap().users.iter().map(|u| u.id).collect();
        let mut out: Vec<Account> = vec![];
        for a in list {
            let user_id = a["user_id"].as_u64().ok_or("Pick a person for every Plex account")?;
            if !users.contains(&user_id) { return Err("That person does not exist".into()); }
            if out.iter().any(|x| x.user_id == user_id) { return Err("Each person can have only one Plex account".into()); }
            let old = a["id"].as_str().and_then(|id| c.accounts.iter().find(|x| x.id == id));
            let token = a["token"].as_str().map(str::trim).filter(|t| !t.is_empty()).map(String::from).or(old.map(|o| o.token.clone()))
                .ok_or("Enter the Plex token for every new account")?;
            let plex_name = if old.is_some_and(|o| o.token == token) { old.unwrap().plex_name.clone() } else { plex_account_name(&c.client_id, &token).unwrap_or_default() };
            out.push(Account { id: old.map(|o| o.id.clone()).unwrap_or_else(|| crate::db::random_hex(6)), token, user_id, plex_name });
        }
        c.accounts = out;
    }
    if c.enabled && c.url.is_empty() { return Err("Enter the Plex server's address".into()); }
    write_json(&config_path(app), &c)?;
    Ok(public(app))
}

// ------------------------------------------------------------------ Plex API

fn agent() -> ureq::Agent {
    ureq::AgentBuilder::new().timeout(Duration::from_secs(60)).build()
}

fn plex_err(url: &str, e: ureq::Error) -> String {
    match e {
        ureq::Error::Status(401, _) => "Plex did not accept the token (HTTP 401)".into(),
        ureq::Error::Status(code, _) => format!("Plex answered HTTP {code} for {}", url.split('?').next().unwrap_or(url)),
        ureq::Error::Transport(t) => format!("Plex is not reachable: {t}"),
    }
}

fn request(method: &str, url: &str, client: &str, token: &str) -> Result<Value, String> {
    let r = agent().request(method, url)
        .set("X-Plex-Token", token)
        .set("X-Plex-Client-Identifier", client)
        .set("X-Plex-Product", crate::server::APP_NAME)
        .set("Accept", "application/json")
        .call()
        .map_err(|e| plex_err(url, e))?;
    let text = r.into_string().map_err(|e| e.to_string())?;
    if text.trim().is_empty() { return Ok(Value::Null); }
    Ok(serde_json::from_str(&text).unwrap_or(Value::Null))
}

fn plex_account_name(client: &str, token: &str) -> Option<String> {
    let v = request("GET", "https://plex.tv/api/v2/user", client, token).ok()?;
    v["username"].as_str().or(v["title"].as_str()).map(String::from).filter(|s| !s.is_empty())
}

struct PlexItem {
    rk: String,
    files: Vec<String>,
    count: u32,
    offset: u64,
    last: u64,
    rating: Option<f32>,
}

struct PlexSection {
    key: String,
    kind: String,
    title: String,
    paths: Vec<String>,
}

fn sections(url: &str, client: &str, token: &str) -> Result<Vec<PlexSection>, String> {
    let v = request("GET", &format!("{url}/library/sections"), client, token)?;
    Ok(v["MediaContainer"]["Directory"].as_array().cloned().unwrap_or_default().iter().map(|d| PlexSection {
        key: d["key"].as_str().map(String::from).unwrap_or_else(|| d["key"].to_string()),
        kind: d["type"].as_str().unwrap_or("").into(),
        title: d["title"].as_str().unwrap_or("").into(),
        paths: d["Location"].as_array().map(|l| l.iter().filter_map(|x| x["path"].as_str().map(String::from)).collect()).unwrap_or_default(),
    }).collect())
}

fn num(v: &Value) -> Option<f64> {
    v.as_f64().or_else(|| v.as_str().and_then(|s| s.parse().ok()))
}

fn items(url: &str, client: &str, token: &str, s: &PlexSection) -> Result<Vec<PlexItem>, String> {
    let kind = match s.kind.as_str() { "movie" => 1, "show" => 4, _ => return Ok(vec![]) };
    let mut out = vec![];
    let mut start = 0;
    loop {
        let v = request("GET", &format!("{url}/library/sections/{}/all?type={kind}&X-Plex-Container-Start={start}&X-Plex-Container-Size={PAGE}", s.key), client, token)?;
        let mc = &v["MediaContainer"];
        let list = mc["Metadata"].as_array().cloned().unwrap_or_default();
        let n = list.len();
        for m in list {
            let rk = m["ratingKey"].as_str().map(String::from).unwrap_or_else(|| m["ratingKey"].to_string());
            let files = m["Media"].as_array().into_iter().flatten()
                .flat_map(|md| md["Part"].as_array().cloned().unwrap_or_default())
                .filter_map(|p| p["file"].as_str().map(String::from)).collect();
            out.push(PlexItem {
                rk,
                files,
                count: num(&m["viewCount"]).unwrap_or(0.0) as u32,
                offset: num(&m["viewOffset"]).unwrap_or(0.0) as u64,
                last: num(&m["lastViewedAt"]).unwrap_or(0.0) as u64,
                rating: num(&m["userRating"]).map(|r| r as f32).filter(|r| *r > 0.0),
            });
        }
        let total = num(&mc["totalSize"]).map(|t| t as usize).unwrap_or(0);
        start += n;
        if n == 0 || n < PAGE || start >= total { break; }
    }
    Ok(out)
}

/// Checks the address and a token: the server's name and its movie and show
/// libraries, each folder with the folder here it seems to be.
pub fn test(app: &App, body: &Value) -> Result<Value, String> {
    let c = load(app);
    let url = base(body["url"].as_str().filter(|s| !s.trim().is_empty()).unwrap_or(&c.url));
    if url == "http://" { return Err("Enter the Plex server's address".into()); }
    let token = body["token"].as_str().map(str::trim).filter(|t| !t.is_empty()).map(String::from)
        .or_else(|| body["account_id"].as_str().and_then(|id| c.accounts.iter().find(|a| a.id == id)).map(|a| a.token.clone()))
        .or_else(|| c.accounts.first().map(|a| a.token.clone()))
        .ok_or("Enter a Plex token")?;
    let root = request("GET", &format!("{url}/"), &c.client_id, &token)?;
    let secs = sections(&url, &c.client_id, &token)?;
    let maps: Vec<(String, String)> = c.path_map.iter().map(|r| (r.from.clone(), r.to.clone())).collect();
    let ours: Vec<String> = app.db.lock().unwrap().sections.iter().flat_map(|s| s.paths.clone()).collect();
    Ok(json!({
        "server": root["MediaContainer"]["friendlyName"].as_str().unwrap_or(""),
        "version": root["MediaContainer"]["version"].as_str().unwrap_or(""),
        "plex_name": plex_account_name(&c.client_id, &token),
        "libraries": secs.iter().filter(|s| s.kind == "movie" || s.kind == "show").map(|s| json!({
            "title": s.title, "type": s.kind,
            "folders": s.paths.iter().map(|p| {
                let mapped = map_path(p, &maps);
                let known = ours.iter().any(|o| { let (a, b) = (path_key(o), path_key(&mapped)); a == b || b.starts_with(&format!("{a}/")) || a.starts_with(&format!("{b}/")) });
                json!({"plex": p, "here": mapped, "in_a_library": known, "suggested": if known { String::new() } else { crate::migrate::suggest(p, &ours) }})
            }).collect::<Vec<_>>(),
        })).collect::<Vec<_>>(),
    }))
}

// ------------------------------------------------------------------ sync

enum Push {
    Scrobble,
    Unscrobble,
    Progress(u64),
    Rate(Option<f32>),
}

#[derive(Default)]
struct Counts {
    watched: usize,
    unwatched: usize,
    resume: usize,
    ratings: usize,
}

impl Counts {
    fn json(&self) -> Value {
        json!({"watched": self.watched, "unwatched": self.unwatched, "resume": self.resume, "ratings": self.ratings})
    }
}

fn same_offset(a: u64, b: u64) -> bool {
    a.abs_diff(b) <= OFFSET_SLACK_MS
}

/// Picks one side's value: changed on one side → that side; on both → the
/// newer one; nothing agreed yet → `first`.
fn pick<T: Clone>(agreed: Option<T>, here: T, plex: T, plex_newer: bool, eq: impl Fn(&T, &T) -> bool, first: impl FnOnce(T, T) -> T) -> T {
    match agreed {
        None => first(here, plex),
        Some(s) => {
            let here_changed = !eq(&here, &s);
            let plex_changed = !eq(&plex, &s);
            match (here_changed, plex_changed) {
                (false, true) => plex,
                (true, false) => here,
                (true, true) if plex_newer => plex,
                _ => here,
            }
        }
    }
}

/// One-way sync: the master side's value always wins ("from_plex": Plex, "to_plex": here).
fn directed<T>(dir: &str, picked: T, here: T, plex: T) -> T {
    match dir {
        "from_plex" => plex,
        "to_plex" => here,
        _ => picked,
    }
}

/// Starts a run on a background thread unless one is running.
pub fn start_now(app: &Arc<App>) -> bool {
    if RUNNING.swap(true, Ordering::SeqCst) { return false; }
    let a = app.clone();
    std::thread::spawn(move || {
        let r = run(&a);
        RUNNING.store(false, Ordering::SeqCst);
        if let Err(e) = &r { elog!("Plex sync: {e}"); }
    });
    true
}

/// Runs every few minutes when switched on.
pub fn start(app: &Arc<App>) {
    let a = app.clone();
    std::thread::spawn(move || {
        let mut last: Option<Instant> = None;
        loop {
            std::thread::sleep(Duration::from_secs(30));
            let c = load(&a);
            if !c.enabled || c.url.is_empty() || c.accounts.is_empty() { continue; }
            if last.is_some_and(|t| t.elapsed() < Duration::from_secs(c.interval_minutes.max(1) * 60)) { continue; }
            last = Some(Instant::now());
            start_now(&a);
        }
    });
}

fn run(app: &Arc<App>) -> Result<Value, String> {
    let c = load(app);
    let started = now();
    let shown = {
        let mut job = app.job.lock().unwrap();
        if job.is_none() {
            *job = Some(("Syncing with Plex".into(), 0, c.accounts.len().max(1)));
            app.job_cancel.store(false, std::sync::atomic::Ordering::SeqCst);
            true
        } else {
            false
        }
    };
    let r = run_inner(app, &c);
    if shown {
        *app.job.lock().unwrap() = None;
        app.job_cancel.store(false, std::sync::atomic::Ordering::SeqCst);
    }
    let report = match &r {
        Ok(accounts) => json!({"at": started, "finished_at": now(), "ok": accounts.iter().all(|a| a["error"].is_null()), "accounts": accounts}),
        Err(e) => json!({"at": started, "finished_at": now(), "ok": false, "error": e, "accounts": []}),
    };
    let _ = write_json(&last_path(app), &report);
    r.map(|_| report)
}

fn run_inner(app: &Arc<App>, c: &Config) -> Result<Vec<Value>, String> {
    if c.url.is_empty() { return Err("No Plex server address set".into()); }
    if c.accounts.is_empty() { return Err("No Plex account added".into()); }
    let url = base(&c.url);
    let mut state: State = std::fs::read(state_path(app)).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default();
    if state.url != url {
        state = State { url: url.clone(), items: BTreeMap::new() };
    }
    let maps: Vec<(String, String)> = c.path_map.iter().map(|r| (r.from.clone(), r.to.clone())).collect();
    // Files here → movie or episode.
    let by_path: HashMap<String, u64> = {
        let db = app.db.lock().unwrap();
        db.items.values().filter(|i| matches!(i.kind, ItemKind::Movie | ItemKind::Episode) && !i.is_extra())
            .flat_map(|i| i.all_files().into_iter().map(move |f| (path_key(&f.path), i.id))).collect()
    };
    let mut reports = vec![];
    for (n, acc) in c.accounts.iter().enumerate() {
        if let Some(j) = app.job.lock().unwrap().as_mut().filter(|j| j.0 == "Syncing with Plex") { j.1 = n; }
        let mut rep = json!({"user_id": acc.user_id, "plex_name": acc.plex_name});
        match sync_account(app, c, &url, acc, &maps, &by_path, &mut state) {
            Ok(v) => { if let Value::Object(m) = v { for (k, x) in m { rep[k] = x; } } }
            Err(e) => rep["error"] = json!(e),
        }
        reports.push(rep);
        if app.job_cancelled() { break; }
    }
    write_json(&state_path(app), &state)?;
    Ok(reports)
}

fn sync_account(app: &Arc<App>, c: &Config, url: &str, acc: &Account, maps: &[(String, String)], by_path: &HashMap<String, u64>, state: &mut State) -> Result<Value, String> {
    let u = acc.user_id;
    if app.db.lock().unwrap().user(u).is_none() { return Err("The person for this Plex account no longer exists".into()); }
    let mut plex: Vec<PlexItem> = vec![];
    for s in sections(url, &c.client_id, &acc.token)? {
        plex.extend(items(url, &c.client_id, &acc.token, &s)?);
        if app.job_cancelled() { return Err("Cancelled".into()); }
    }
    let mut matched: Vec<(PlexItem, u64)> = vec![];
    let mut missing: Vec<String> = vec![];
    for p in plex {
        match p.files.iter().find_map(|f| by_path.get(&path_key(&map_path(f, maps))).copied()) {
            Some(id) => matched.push((p, id)),
            None => missing.push(p.files.first().cloned().unwrap_or_default()),
        }
    }
    let plex_items = matched.len() + missing.len();
    let (mut here, mut there) = (Counts::default(), Counts::default());
    let mut pushes: Vec<(String, String, Push)> = vec![];
    // Two Plex items can point at the same file here (a movie in two Plex libraries); the first counts.
    let mut done: std::collections::HashSet<u64> = Default::default();
    {
        let mut db = app.db.lock().unwrap();
        for (p, id) in &matched {
            if !done.insert(*id) { continue; }
            let key = format!("{u}:{id}");
            let lv = db.view(u, *id);
            let agreed = state.items.get(&key).cloned();
            let plex_newer = p.last > lv.last_viewed_at;
            let mut res = agreed.clone().unwrap_or_default();
            let mut scrobbled = false;
            if c.watched {
                let w = pick(agreed.as_ref().map(|s| s.w), lv.view_count > 0, p.count > 0, plex_newer, |a, b| a == b, |a, b| a || b);
                let w = directed(&c.direction, w, lv.view_count > 0, p.count > 0);
                res.w = w;
                if w != (lv.view_count > 0) {
                    let v = db.view_mut(u, *id);
                    if w {
                        v.view_count = p.count.max(1);
                        v.last_viewed_at = v.last_viewed_at.max(p.last);
                        v.finished = false;
                        let at = if p.last > 0 { p.last } else { now() };
                        db.record_play_at(u, *id, true, at);
                        here.watched += 1;
                    } else {
                        v.view_count = 0;
                        v.finished = false;
                        here.unwatched += 1;
                    }
                }
                if w != (p.count > 0) {
                    pushes.push((key.clone(), p.rk.clone(), if w { Push::Scrobble } else { Push::Unscrobble }));
                    scrobbled = w;
                    if w { there.watched += 1 } else { there.unwatched += 1 }
                }
            }
            if c.resume {
                let o = pick(agreed.as_ref().map(|s| s.o), lv.view_offset_ms, p.offset, plex_newer, |a, b| same_offset(*a, *b), |a, b| if plex_newer { b } else { a });
                let o = directed(&c.direction, o, lv.view_offset_ms, p.offset);
                res.o = o;
                if !same_offset(o, lv.view_offset_ms) {
                    let v = db.view_mut(u, *id);
                    v.view_offset_ms = o;
                    v.last_viewed_at = v.last_viewed_at.max(p.last);
                    here.resume += 1;
                }
                // Plex clears the resume point itself when something is marked watched.
                if !same_offset(o, p.offset) && !(scrobbled && o == 0) {
                    pushes.push((key.clone(), p.rk.clone(), Push::Progress(o)));
                    there.resume += 1;
                }
            }
            if c.ratings {
                let eq = |a: &Option<f32>, b: &Option<f32>| match (a, b) { (Some(x), Some(y)) => (x - y).abs() < 0.05, (None, None) => true, _ => false };
                let r = pick(agreed.as_ref().map(|s| s.r), lv.rating, p.rating, plex_newer, eq, |a, b| a.or(b));
                let r = directed(&c.direction, r, lv.rating, p.rating);
                res.r = r;
                if !eq(&r, &lv.rating) {
                    db.view_mut(u, *id).rating = r;
                    here.ratings += 1;
                }
                if !eq(&r, &p.rating) {
                    pushes.push((key.clone(), p.rk.clone(), Push::Rate(r)));
                    there.ratings += 1;
                }
            }
            state.items.insert(key, res);
        }
    }
    app.save();
    // Then tell Plex. What could not be sent is agreed again next time.
    let mut errors: Vec<String> = vec![];
    let mut pushes = pushes.into_iter();
    while let Some((key, rk, push)) = pushes.next() {
        // Cancelled: what was not sent yet is agreed again next time.
        if app.job_cancelled() {
            state.items.remove(&key);
            for (k, ..) in pushes { state.items.remove(&k); }
            errors.push("Cancelled".into());
            break;
        }
        let q = format!("key={rk}&identifier={IDENT}");
        let r = match push {
            Push::Scrobble => request("GET", &format!("{url}/:/scrobble?{q}"), &c.client_id, &acc.token),
            Push::Unscrobble => request("GET", &format!("{url}/:/unscrobble?{q}"), &c.client_id, &acc.token),
            Push::Progress(ms) => request("GET", &format!("{url}/:/progress?{q}&time={ms}&state=stopped"), &c.client_id, &acc.token),
            Push::Rate(r) => request("PUT", &format!("{url}/:/rate?{q}&rating={}", r.map(|x| format!("{x}")).unwrap_or_else(|| "-1".into())), &c.client_id, &acc.token),
        };
        if let Err(e) = r {
            state.items.remove(&key);
            if errors.len() < 5 { errors.push(e); }
        }
    }
    Ok(json!({
        "plex_items": plex_items, "matched": matched.len(), "not_found": missing.len(),
        "not_found_examples": missing.iter().take(8).collect::<Vec<_>>(),
        "to_here": here.json(), "to_plex": there.json(),
        "error": if errors.is_empty() { Value::Null } else { json!(format!("Not everything reached Plex: {}", errors.join("; "))) },
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_way_follows_the_master() {
        assert!(!directed("from_plex", true, true, false));
        assert!(directed("to_plex", false, true, false));
        assert!(directed("both", true, false, false));
    }

    #[test]
    fn picks_the_side_that_changed() {
        let eq = |a: &bool, b: &bool| a == b;
        // First run: watched anywhere stays watched.
        assert!(pick(None, false, true, false, eq, |a, b| a || b));
        assert!(pick(None, true, false, true, eq, |a, b| a || b));
        // Unwatched on Plex only → unwatched here.
        assert!(!pick(Some(true), true, false, false, eq, |a, b| a || b));
        // Unwatched here only → stays unwatched.
        assert!(!pick(Some(true), false, true, true, eq, |a, b| a || b));
        // Both changed: the newer side.
        assert_eq!(pick(Some(10_000u64), 50_000, 90_000, true, |a, b| same_offset(*a, *b), |a, _| a), 90_000);
        assert_eq!(pick(Some(10_000u64), 50_000, 90_000, false, |a, b| same_offset(*a, *b), |a, _| a), 50_000);
        // A few seconds apart is the same place.
        assert_eq!(pick(Some(10_000u64), 12_000, 10_000, true, |a, b| same_offset(*a, *b), |a, _| a), 12_000);
    }
}

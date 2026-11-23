//! The transcode helper: another computer (`lichtspiel --transcode-helper`) converts video for
//! playback with its own ffmpeg and graphics chip, so a small NAS does not have to.
//!
//! The server builds the ffmpeg command line as always (with the helper's hardware), but the input
//! is an address on this server (`/helper/source/<token>`, a one-time key per stream, with Range
//! requests for seeking) and the HLS folder is `{dir}`, which the helper fills in. So the helper needs
//! no shared folders and no paths of the NAS. The server passes the helper's HLS files and its MP4
//! stream on to the player; when the helper can't be reached, the server converts itself.
//!
//! Only conversions go to the helper: copying the picture (remux) costs no work, so it stays here.

use crate::data::db::{self, Settings};
use crate::playback::transcode;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

pub const DEFAULT_PORT: u16 = 8486;
/// Stands for the job's folder in an HLS command line; the helper puts its own folder there.
pub const DIR: &str = "{dir}";
const SECRET_HEADER: &str = "X-Helper-Secret";

// ---------------------------------------------------------------- on the server

/// Files the helper may read now: token → path. A token ends with its stream.
static SOURCES: Mutex<Option<HashMap<String, (String, Instant)>>> = Mutex::new(None);

fn add_source(path: &str) -> String {
    let token = db::random_hex(24);
    let mut map = SOURCES.lock().unwrap_or_else(|e| e.into_inner());
    let map = map.get_or_insert_with(HashMap::new);
    // A stream that ended without saying so (a crash): its key goes after a day at the latest.
    map.retain(|_, (_, at)| at.elapsed() < Duration::from_secs(24 * 3600));
    map.insert(token.clone(), (path.to_string(), Instant::now()));
    token
}

fn drop_source(token: &str) {
    if let Some(map) = SOURCES.lock().unwrap_or_else(|e| e.into_inner()).as_mut() { map.remove(token); }
}

/// The file behind a token of `/helper/source/<token>`.
pub fn source_path(token: &str) -> Option<String> {
    SOURCES.lock().unwrap_or_else(|e| e.into_inner()).as_ref()?.get(token).map(|(p, _)| p.clone())
}

/// Streams the helper converts right now: (user, item) → how many, for the dashboard.
static SERVING: Mutex<Option<HashMap<(u64, u64), usize>>> = Mutex::new(None);

fn serving_add(key: (u64, u64), n: isize) {
    let mut map = SERVING.lock().unwrap_or_else(|e| e.into_inner());
    let map = map.get_or_insert_with(HashMap::new);
    let v = map.entry(key).or_insert(0);
    *v = (*v as isize + n).max(0) as usize;
    if *v == 0 { map.remove(&key); }
}

/// This person's stream of this item is converted on the helper.
pub fn serving(user: u64, item: u64) -> bool {
    SERVING.lock().unwrap_or_else(|e| e.into_inner()).as_ref().is_some_and(|m| m.contains_key(&(user, item)))
}

/// What the helper said about itself.
#[derive(Clone, Debug)]
pub struct Helper {
    pub base: String,
    secret: String,
    /// The hardware it converts with ("none", "vaapi", "qsv", "nvenc").
    pub hw: String,
    pub device: String,
    pub tonemap: bool,
    /// This server's address as the helper sees it.
    pub you: String,
    pub version: String,
    pub ffmpeg_version: String,
}

/// The helper's address as typed in the settings, without a / at the end; http:// when none is given.
fn base(settings: &Settings) -> String {
    let b = settings.transcode_helper.trim().trim_end_matches('/');
    if b.is_empty() || b.contains("://") { b.to_string() } else { format!("http://{b}") }
}

fn agent(read: Duration) -> ureq::Agent {
    ureq::AgentBuilder::new().timeout_connect(Duration::from_secs(4)).timeout_read(read).timeout_write(Duration::from_secs(20)).redirects(0).build()
}

fn call_err(e: ureq::Error) -> String {
    match e {
        ureq::Error::Status(401, _) => "the helper did not accept the secret".into(),
        ureq::Error::Status(code, r) => format!("the helper answered {code}: {}", r.into_string().unwrap_or_default().chars().take(200).collect::<String>()),
        ureq::Error::Transport(t) => format!("the helper can't be reached ({t})"),
    }
}

type Cached = (String, Instant, Result<Helper, String>);
static STATUS: Mutex<Option<Cached>> = Mutex::new(None);

/// The helper, or why it can't be used; None when no helper is set. Asked again after 30 s
/// (10 s after a failure), or now with `fresh`.
pub fn status(settings: &Settings, fresh: bool) -> Option<Result<Helper, String>> {
    let base = base(settings);
    if base.is_empty() { return None; }
    let key = format!("{base}|{}", settings.transcode_helper_secret);
    if !fresh {
        if let Some((k, at, r)) = STATUS.lock().unwrap_or_else(|e| e.into_inner()).as_ref() {
            let keep = if r.is_ok() { 30 } else { 10 };
            if *k == key && at.elapsed() < Duration::from_secs(keep) { return Some(r.clone()); }
        }
    }
    let r = ask_status(&base, &settings.transcode_helper_secret);
    let before = STATUS.lock().unwrap_or_else(|e| e.into_inner()).replace((key, Instant::now(), r.clone()));
    // Say it in the log when the helper goes away or comes back (not on every look).
    let was_ok = before.as_ref().map(|(_, _, r)| r.is_ok());
    match (&r, was_ok) {
        (Err(e), Some(true) | None) => elog!("transcode helper {base}: {e}; this server converts until it is back"),
        (Ok(h), Some(false) | None) => crate::admin::log::add(format!("transcode helper {base} is there, converting with {}", if h.hw == "none" { "the processor" } else { &h.hw })),
        _ => {}
    }
    Some(r)
}

fn ask_status(base: &str, secret: &str) -> Result<Helper, String> {
    if secret.len() < 8 { return Err("the helper's secret is missing (at least 8 characters)".into()); }
    let v = agent(Duration::from_secs(20)).get(&format!("{base}/status")).set(SECRET_HEADER, secret).call().map_err(call_err)?;
    let v: Value = read_json(v)?;
    if v["ffmpeg_found"].as_bool() != Some(true) { return Err("the helper has no ffmpeg".into()); }
    let s = |k: &str| v[k].as_str().unwrap_or_default().to_string();
    Ok(Helper { base: base.to_string(), secret: secret.to_string(), hw: s("hw_in_use"), device: s("hw_device"), tonemap: v["tonemap"].as_bool().unwrap_or(false),
        you: s("you"), version: s("version"), ffmpeg_version: s("ffmpeg_version") })
}

fn read_json(r: ureq::Response) -> Result<Value, String> {
    let text = r.into_string().map_err(|e| format!("the helper's answer could not be read ({e})"))?;
    serde_json::from_str(&text).map_err(|e| format!("the helper's answer could not be read ({e})"))
}

/// The helper as it answered last (no new question: for pages drawn while the library is locked).
pub fn last_known() -> Option<Helper> {
    STATUS.lock().unwrap_or_else(|e| e.into_inner()).as_ref().and_then(|(_, _, r)| r.as_ref().ok().cloned())
}

/// For the transcoder settings: whether the helper answers, what it converts with, or what is wrong.
pub fn report(settings: &Settings) -> Value {
    match status(settings, true) {
        None => Value::Null,
        Some(Ok(h)) => json!({"address": h.base, "ok": true, "hw": h.hw, "version": h.version, "ffmpeg_version": h.ffmpeg_version,
            "same_version": h.version == crate::server::VERSION, "tonemap": h.tonemap}),
        Some(Err(e)) => json!({"address": base(settings), "ok": false, "error": e}),
    }
}

/// The helper to convert with now, when one is set and answers.
pub fn usable(settings: &Settings) -> Option<Helper> {
    status(settings, false)?.ok()
}

/// The settings a command line for the helper is made with: its hardware and what its ffmpeg can.
pub fn settings_for(settings: &Settings, h: &Helper) -> Settings {
    let mut st = settings.clone();
    st.hw_accel = if h.hw.is_empty() { "none".into() } else { h.hw.clone() };
    st.hw_device = h.device.clone();
    st.hdr_tone_mapping = settings.hdr_tone_mapping && h.tonemap;
    st.on_helper = true;
    st
}

/// The command line converts the picture (not only copies it).
pub fn converts(args: &[String]) -> bool {
    args.windows(2).any(|w| w[0] == "-c:v" && w[1] != "copy")
}

fn source_url(settings: &Settings, h: &Helper, port: u16, token: &str) -> String {
    let own = settings.transcode_helper_source.trim().trim_end_matches('/');
    if !own.is_empty() {
        let own = if own.contains("://") { own.to_string() } else { format!("http://{own}") };
        return format!("{own}/helper/source/{token}");
    }
    let host = if h.you.contains(':') { format!("[{}]", h.you) } else { h.you.clone() };
    format!("{}://{host}:{port}/helper/source/{token}", crate::server::scheme())
}

/// The input file becomes an address on this server; ffmpeg reconnects when the network hiccups.
fn swap_input(args: &[String], path: &str, url: &str) -> Vec<String> {
    let mut out = Vec::with_capacity(args.len() + 2);
    let mut i = 0;
    while i < args.len() {
        if args[i] == "-i" && args.get(i + 1).is_some_and(|p| p == path) {
            out.extend(["-reconnect".to_string(), "1".to_string(), "-i".to_string(), url.to_string()]);
            i += 2;
            continue;
        }
        out.push(args[i].clone());
        i += 1;
    }
    out
}

/// A conversion running on the helper. Ending it (or dropping it) stops ffmpeg there and closes the file.
pub struct Job {
    base: String,
    secret: String,
    /// HLS jobs have an id on the helper; an MP4 stream ends when its connection closes.
    id: Option<String>,
    token: String,
    key: (u64, u64),
    done: bool,
}

impl Job {
    fn new(h: &Helper, token: String, key: (u64, u64)) -> Job {
        serving_add(key, 1);
        Job { base: h.base.clone(), secret: h.secret.clone(), id: None, token, key, done: false }
    }

    pub fn stop(&mut self) {
        if self.done { return; }
        self.done = true;
        drop_source(&self.token);
        serving_add(self.key, -1);
        if let Some(id) = &self.id {
            let _ = agent(Duration::from_secs(5)).delete(&format!("{}/hls/{id}", self.base)).set(SECRET_HEADER, &self.secret).call();
        }
    }

    /// One file of an HLS job (index.m3u8, segNNNNN.ts), waiting up to `wait` for the helper to write it:
    /// (status, content type, body).
    pub fn fetch(&self, file: &str, wait: Duration) -> (u16, String, Vec<u8>) {
        let Some(id) = &self.id else { return (404, "text/plain".into(), vec![]) };
        let url = format!("{}/hls/{id}/{file}?wait={}", self.base, wait.as_secs());
        let r = agent(wait + Duration::from_secs(20)).get(&url).set(SECRET_HEADER, &self.secret).call();
        let (code, resp) = match r {
            Ok(r) => (200, r),
            Err(ureq::Error::Status(c, r)) => (c, r),
            Err(e) => return (502, "text/plain".into(), call_err(e).into_bytes()),
        };
        let ctype = resp.content_type().to_string();
        let mut body = vec![];
        if resp.into_reader().take(256 << 20).read_to_end(&mut body).is_err() { return (502, "text/plain".into(), b"the helper stopped sending".to_vec()); }
        (code, ctype, body)
    }

    /// A copy that can ask the helper for files (the session keeps the job itself).
    pub fn handle(&self) -> JobHandle {
        JobHandle { base: self.base.clone(), secret: self.secret.clone(), id: self.id.clone() }
    }
}

impl Drop for Job {
    fn drop(&mut self) { self.stop(); }
}

/// Reads an HLS job's files without owning the job.
pub struct JobHandle { base: String, secret: String, id: Option<String> }

impl JobHandle {
    pub fn fetch(&self, file: &str, wait: Duration) -> (u16, String, Vec<u8>) {
        // Not a Job: dropping it must not stop the conversion.
        let j = std::mem::ManuallyDrop::new(Job { base: self.base.clone(), secret: self.secret.clone(), id: self.id.clone(), token: String::new(), key: (0, 0), done: true });
        j.fetch(file, wait)
    }
}

/// Starts an HLS conversion on the helper (`args` made by `transcode::hls_args` with the folder `DIR`).
pub fn start_hls(settings: &Settings, h: &Helper, port: u16, path: &str, args: &[String], key: (u64, u64)) -> Result<Job, String> {
    let token = add_source(path);
    let args = swap_input(args, path, &source_url(settings, h, port, &token));
    let mut job = Job::new(h, token, key);
    let v = agent(Duration::from_secs(20)).post(&format!("{}/hls", h.base)).set(SECRET_HEADER, &h.secret).set("Content-Type", "application/json").send_string(&json!({"args": args}).to_string()).map_err(call_err)?;
    let v = read_json(v)?;
    let id = v["id"].as_str().filter(|i| !i.is_empty() && i.chars().all(|c| c.is_ascii_hexdigit())).ok_or("the helper did not start the conversion")?;
    job.id = Some(id.to_string());
    Ok(job)
}

/// Starts an MP4 stream on the helper (`args` from `transcode::progressive_args`): the job and what it sends.
pub fn start_pipe(settings: &Settings, h: &Helper, port: u16, path: &str, args: &[String], key: (u64, u64)) -> Result<(Job, Box<dyn Read + Send>), String> {
    let token = add_source(path);
    let args = swap_input(args, path, &source_url(settings, h, port, &token));
    let job = Job::new(h, token, key);
    // No limit on the whole stream (a movie takes hours), only on a pause in what arrives.
    let r = agent(Duration::from_secs(90)).post(&format!("{}/pipe", h.base)).set(SECRET_HEADER, &h.secret).set("Content-Type", "application/json").send_string(&json!({"args": args}).to_string()).map_err(call_err)?;
    Ok((job, Box::new(r.into_reader())))
}

/// An ffmpeg run here or on the helper.
pub enum Runner {
    Local(Child),
    Helper(Job),
}

impl Runner {
    pub fn kill(&mut self) -> std::io::Result<()> {
        match self { Runner::Local(c) => c.kill(), Runner::Helper(j) => { j.stop(); Ok(()) } }
    }

    pub fn wait(&mut self) -> std::io::Result<()> {
        match self { Runner::Local(c) => c.wait().map(|_| ()), Runner::Helper(_) => Ok(()) }
    }

    /// Some(()) when ffmpeg has ended (the helper says that itself when asked for a file).
    pub fn try_wait(&mut self) -> std::io::Result<Option<()>> {
        match self { Runner::Local(c) => c.try_wait().map(|s| s.map(|_| ())), Runner::Helper(_) => Ok(None) }
    }

    pub fn helper(&self) -> Option<JobHandle> {
        match self { Runner::Helper(j) => Some(j.handle()), Runner::Local(_) => None }
    }
}

// ---------------------------------------------------------------- on the helper

pub struct HelperOptions {
    pub listen: String,
    pub secret: String,
    pub ffmpeg: String,
    pub hw: String,
    pub hw_device: String,
    /// Where HLS jobs write their files.
    pub dir: PathBuf,
}

struct HelperJob {
    child: Child,
    dir: PathBuf,
    last: Instant,
}

struct State {
    o: HelperOptions,
    /// What /status says (looked at once: the hardware tests take a few seconds).
    info: Value,
    jobs: Mutex<HashMap<String, HelperJob>>,
}

/// `lichtspiel --transcode-helper`: waits for conversions from a Lichtspiel server.
pub fn run(o: HelperOptions) -> Result<(), String> {
    if o.secret.len() < 8 { return Err("give a secret of at least 8 characters with --helper-secret (or LICHTSPIEL_HELPER_SECRET); the same goes into the server's settings".into()); }
    if !["auto", "none", "vaapi", "qsv", "nvenc"].contains(&o.hw.as_str()) { return Err("--hw must be auto, none, vaapi, qsv or nvenc".into()); }
    // Jobs of an earlier run are over.
    let _ = std::fs::remove_dir_all(&o.dir);
    std::fs::create_dir_all(&o.dir).map_err(|e| format!("could not make the work folder {} ({e})", o.dir.display()))?;
    let st = Settings { ffmpeg: o.ffmpeg.clone(), hw_accel: o.hw.clone(), hw_device: o.hw_device.clone(), ..Settings::default() };
    println!("Looking at ffmpeg and the graphics chip …");
    let mut info = transcode::detect(&st);
    if info["ffmpeg_found"].as_bool() != Some(true) {
        return Err(format!("ffmpeg was not found ({}); install it (e.g. winget install ffmpeg) or give its path with --ffmpeg", o.ffmpeg));
    }
    info["tonemap"] = json!(transcode::can_tonemap(&o.ffmpeg));
    info["hw_device"] = json!(o.hw_device);
    info["version"] = json!(crate::server::VERSION);
    let http = tiny_http::Server::http(&o.listen).map_err(|e| format!("could not listen on {} ({e}); is another program using that port?", o.listen))?;
    let hw = info["hw_in_use"].as_str().unwrap_or("none").to_string();
    println!("{} transcode helper {} is running on {}.", crate::server::APP_NAME, crate::server::VERSION, o.listen);
    println!("  ffmpeg: {}", info["ffmpeg_version"].as_str().unwrap_or(""));
    println!("  Converting with: {}", if hw == "none" { "the processor (software)" } else { &hw });
    println!("  In the server's settings (Transcoder → Transcode helper), enter http://{}:{} and the same secret.", crate::lan_address(), http.server_addr().to_ip().map(|a| a.port()).unwrap_or(DEFAULT_PORT));
    let state = Arc::new(State { o, info, jobs: Mutex::new(HashMap::new()) });
    // A server that went away without ending its jobs (restart, crash): they stop after 2 minutes.
    let s = state.clone();
    std::thread::spawn(move || loop {
        std::thread::sleep(Duration::from_secs(10));
        let stale: Vec<HelperJob> = {
            let mut jobs = s.jobs.lock().unwrap();
            let ids: Vec<String> = jobs.iter().filter(|(_, j)| j.last.elapsed() > Duration::from_secs(120)).map(|(k, _)| k.clone()).collect();
            ids.iter().filter_map(|i| jobs.remove(i)).collect()
        };
        for j in stale { end(j); }
    });
    for req in http.incoming_requests() {
        let s = state.clone();
        std::thread::spawn(move || answer(&s, req));
    }
    Ok(())
}

fn end(mut j: HelperJob) {
    let _ = j.child.kill();
    let _ = j.child.wait();
    let _ = std::fs::remove_dir_all(&j.dir);
}

type Out = tiny_http::Response<Box<dyn Read + Send>>;

fn reply(code: u16, ctype: &str, body: Vec<u8>) -> Out {
    let len = body.len();
    let h = vec![tiny_http::Header::from_bytes("Content-Type", ctype).unwrap()];
    tiny_http::Response::new(tiny_http::StatusCode(code), h, Box::new(std::io::Cursor::new(body)) as Box<dyn Read + Send>, Some(len), None)
}

fn reply_json(code: u16, v: Value) -> Out { reply(code, "application/json", v.to_string().into_bytes()) }

fn same(a: &str, b: &str) -> bool {
    a.len() == b.len() && a.bytes().zip(b.bytes()).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

/// The command line from the server, with the job's folder put in.
fn job_args(body: &Value, dir: &Path) -> Option<Vec<String>> {
    let d = dir.to_string_lossy().replace('\\', "/");
    let args: Vec<String> = body["args"].as_array()?.iter().map(|a| a.as_str().map(|s| s.replace(DIR, &d))).collect::<Option<_>>()?;
    (!args.is_empty()).then_some(args)
}

fn answer(s: &State, mut req: tiny_http::Request) {
    let ok = req.headers().iter().find(|h| h.field.equiv(SECRET_HEADER)).is_some_and(|h| same(h.value.as_str(), &s.o.secret));
    if !ok {
        let _ = req.respond(reply_json(401, json!({"error": "wrong secret"})));
        return;
    }
    let you = req.remote_addr().map(|a| a.ip().to_string()).unwrap_or_default();
    let url = req.url().to_string();
    let (path, query) = url.split_once('?').unwrap_or((&url, ""));
    let segs: Vec<&str> = path.split('/').filter(|x| !x.is_empty()).collect();
    let method = req.method().as_str().to_string();
    let mut body = String::new();
    if method == "POST" && req.as_reader().take(1 << 20).read_to_string(&mut body).is_err() {
        let _ = req.respond(reply_json(400, json!({"error": "could not read the request"})));
        return;
    }
    let body: Value = serde_json::from_str(&body).unwrap_or(Value::Null);
    let out = match (method.as_str(), segs.as_slice()) {
        ("GET", ["status"]) => {
            let mut v = s.info.clone();
            v["you"] = json!(you);
            v["jobs"] = json!(s.jobs.lock().unwrap().len());
            reply_json(200, v)
        }
        ("POST", ["hls"]) => {
            let id = db::random_hex(12);
            let dir = s.o.dir.join(&id);
            let Some(args) = job_args(&body, &dir) else { let _ = req.respond(reply_json(400, json!({"error": "no command line"}))); return };
            if let Err(e) = std::fs::create_dir_all(&dir) { let _ = req.respond(reply_json(500, json!({"error": format!("could not make the folder ({e})")}))); return; }
            let log = std::fs::File::create(dir.join("ffmpeg.log")).map(Stdio::from).unwrap_or_else(|_| Stdio::null());
            match Command::new(&s.o.ffmpeg).args(&args).stdin(Stdio::null()).stdout(Stdio::null()).stderr(log).spawn() {
                Ok(child) => {
                    s.jobs.lock().unwrap().insert(id.clone(), HelperJob { child, dir, last: Instant::now() });
                    reply_json(200, json!({"id": id}))
                }
                Err(e) => { let _ = std::fs::remove_dir_all(&dir); reply_json(500, json!({"error": format!("ffmpeg could not be started ({e})")})) }
            }
        }
        ("GET", ["hls", id, file]) => {
            let wait = query.split('&').find_map(|kv| kv.strip_prefix("wait=")).and_then(|w| w.parse::<u64>().ok()).unwrap_or(20).clamp(5, 120);
            hls_file(s, id, file, Duration::from_secs(wait))
        }
        ("DELETE", ["hls", id]) => {
            let j = s.jobs.lock().unwrap().remove(*id);
            if let Some(j) = j { end(j); }
            reply_json(200, json!({"ok": true}))
        }
        ("POST", ["pipe"]) => {
            let Some(args) = job_args(&body, &s.o.dir) else { let _ = req.respond(reply_json(400, json!({"error": "no command line"}))); return };
            match Command::new(&s.o.ffmpeg).args(&args).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::null()).spawn() {
                Ok(mut child) => {
                    let out = child.stdout.take().unwrap();
                    let h = vec![tiny_http::Header::from_bytes("Content-Type", "video/mp4").unwrap()];
                    // No length: sent in chunks while ffmpeg works; when the server hangs up, ffmpeg is stopped.
                    tiny_http::Response::new(tiny_http::StatusCode(200), h, Box::new(Piped { child, out }) as Box<dyn Read + Send>, None, None)
                }
                Err(e) => reply_json(500, json!({"error": format!("ffmpeg could not be started ({e})")})),
            }
        }
        _ => reply_json(404, json!({"error": "unknown address"})),
    };
    let _ = req.respond(out);
}

struct Piped { child: Child, out: std::process::ChildStdout }

impl Read for Piped {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> { self.out.read(buf) }
}

impl Drop for Piped {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// Waits until ffmpeg has written the file (as the server does for its own HLS streams).
fn hls_file(s: &State, id: &str, file: &str, wait: Duration) -> Out {
    let ok_name = file == "index.m3u8" || (file.starts_with("seg") && file.ends_with(".ts") && file.chars().all(|c| c.is_ascii_alphanumeric() || c == '.'));
    if !ok_name || !id.chars().all(|c| c.is_ascii_hexdigit()) { return reply_json(404, json!({"error": "No such file"})); }
    let start = Instant::now();
    loop {
        let (dir, finished) = {
            let mut jobs = s.jobs.lock().unwrap();
            let Some(j) = jobs.get_mut(id) else { return reply_json(404, json!({"error": "This stream has ended"})) };
            j.last = Instant::now();
            (j.dir.clone(), matches!(j.child.try_wait(), Ok(Some(_))))
        };
        let index = std::fs::read_to_string(dir.join("index.m3u8")).unwrap_or_default();
        let ready = if file == "index.m3u8" { index.contains(".ts") } else { index.lines().any(|l| l.trim() == file) };
        if ready || (finished && dir.join(file).exists()) {
            if file == "index.m3u8" { return reply(200, "application/vnd.apple.mpegurl", index.into_bytes()); }
            return match std::fs::read(dir.join(file)) {
                Ok(b) => reply(200, "video/mp2t", b),
                Err(_) => reply_json(404, json!({"error": "No such file"})),
            };
        }
        if finished || start.elapsed() > wait {
            let log = std::fs::read_to_string(dir.join("ffmpeg.log")).unwrap_or_default();
            let why = log.lines().rev().find(|l| !l.trim().is_empty()).unwrap_or("").to_string();
            return reply_json(if finished { 404 } else { 504 }, json!({"error": "The transcoder did not write this part in time", "ffmpeg": why}));
        }
        std::thread::sleep(Duration::from_millis(150));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_input_becomes_the_address_and_the_rest_stays() {
        let a: Vec<String> = ["-ss", "10", "-i", "/m/x.mkv", "-map", "0:v:0?", "-c:v", "h264_nvenc", "{dir}/index.m3u8"].map(String::from).to_vec();
        let b = swap_input(&a, "/m/x.mkv", "http://h/helper/source/t");
        assert_eq!(b, ["-ss", "10", "-reconnect", "1", "-i", "http://h/helper/source/t", "-map", "0:v:0?", "-c:v", "h264_nvenc", "{dir}/index.m3u8"]);
        assert!(converts(&b));
        assert!(!converts(&["-c:v".to_string(), "copy".to_string()]));
        let dir = Path::new("C:\\work\\ab");
        assert_eq!(job_args(&json!({"args": b}), dir).unwrap().last().unwrap(), "C:/work/ab/index.m3u8");
    }

    #[test]
    fn a_helper_converts_with_its_own_hardware() {
        let h = Helper { base: "http://h:8486".into(), secret: "s".repeat(8), hw: "nvenc".into(), device: "1".into(), tonemap: false, you: "192.168.1.10".into(), version: String::new(), ffmpeg_version: String::new() };
        let st = settings_for(&Settings { hw_accel: "vaapi".into(), ..Settings::default() }, &h);
        assert_eq!((st.hw_accel.as_str(), st.hw_device.as_str(), st.hdr_tone_mapping, st.on_helper), ("nvenc", "1", false, true));
        assert_eq!(transcode::effective_hw(&st), "nvenc");
        assert_eq!(source_url(&st, &h, 8484, "t"), format!("{}://192.168.1.10:8484/helper/source/t", crate::server::scheme()));
        let own = Settings { transcode_helper_source: "nas.local:8484/".into(), ..st };
        assert_eq!(source_url(&own, &h, 8484, "t"), "http://nas.local:8484/helper/source/t");
    }

    #[test]
    fn a_source_key_opens_its_file_only_while_the_stream_runs() {
        let h = Helper { base: "http://127.0.0.1:9".into(), secret: "s".repeat(8), hw: "none".into(), device: String::new(), tonemap: true, you: String::new(), version: String::new(), ffmpeg_version: String::new() };
        let token = add_source("/m/x.mkv");
        assert_eq!(source_path(&token).as_deref(), Some("/m/x.mkv"));
        assert!(source_path("nope").is_none());
        let job = Job::new(&h, token.clone(), (7, 8));
        assert!(serving(7, 8));
        drop(job);
        assert!(source_path(&token).is_none() && !serving(7, 8));
    }
}

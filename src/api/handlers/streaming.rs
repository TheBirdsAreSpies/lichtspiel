//! Playback: progress and sessions, direct play with seeking, transcoding (progressive MP4 and HLS) and subtitles.

use super::*;

/// Progress from a player. Passing the watched point (by default the earlier of
/// 90 % and the first credits marker) counts one viewing and
/// clears the resume point.
pub fn timeline(app: &Arc<App>, user: &User, iid: u64, time_ms: u64, state: &str, client: String, viewer: Viewer) -> Resp {
    let mut watched_now = false;
    // Stopped from the dashboard: the player hears it now (and may start again).
    let stopped_by_owner = app.stop_seen(user.id, iid).filter(|_| state != "stopped");
    {
        let mut db = app.db.lock().unwrap();
        let Some(it) = db.items.get(&iid) else { return fail(404, "Not found") };
        if !db.can_see_item(user, it) { return fail(403, "You don't have access to this library"); }
        let duration = it.duration_ms;
        let watched_at = db::watched_at_ms(db.settings.played_at_behaviour, db.settings.watched_threshold_percent, duration, &db.credits_markers(it));
        let v = db.view_mut(user.id, iid);
        v.last_viewed_at = now();
        if duration > 0 && time_ms >= watched_at {
            if !v.finished {
                v.view_count += 1;
                v.finished = true;
                watched_now = true;
            }
            v.view_offset_ms = 0;
        } else {
            v.finished = false;
            v.view_offset_ms = time_ms;
        }
        if watched_now { db.record_play(user.id, iid, false); }
    }
    // Which player it is, so a plugin can act per device (dim only the living room).
    let player = json!({"device": viewer.device, "ip": viewer.ip, "local": viewer.local, "client": client});
    let key = format!("{}:{}", user.id, iid);
    // The play so far (for the statistics): carried from report to report,
    // counting the time between two while it was playing.
    let (before, entry) = {
        let map = app.playing.lock().unwrap();
        let before = map.get(&key).cloned();
        let (session_at, mut watched_ms) = before.as_ref().map(|b| (b.session_at, b.watched_ms)).unwrap_or((now(), 0));
        if let Some(b) = before.as_ref().filter(|b| b.state == "playing") {
            watched_ms += (b.updated.elapsed().as_millis() as u64).min(30_000);
        }
        (before.map(|b| b.state), Playing { user_id: user.id, item_id: iid, offset_ms: time_ms, state: state.into(), method: "player".into(), updated: Instant::now(), client, height: 0, audio: None, viewer, session_at, watched_ms })
    };
    if let Some(message) = stopped_by_owner {
        let ended = app.playing.lock().unwrap().remove(&key);
        app.close_sessions(ended.into_iter().chain([entry]).collect());
        app.save();
        app.fire("playback.stopped", json!({"id": iid, "user": user.username, "time": time_ms, "by_owner": true, "player": player}));
        return ok(json!({"ok": false, "stopped": true, "message": message, "watched": watched_now}));
    }
    if state == "stopped" {
        app.playing.lock().unwrap().remove(&key);
        app.close_sessions(vec![entry]);
    } else {
        app.playing.lock().unwrap().insert(key, entry);
        match before {
            None => app.fire("playback.started", json!({"id": iid, "user": user.username, "time": time_ms, "state": state, "player": player})),
            // Paused or playing again (not every 10-second report).
            Some(b) if b != state => app.fire("playback.progress", json!({"id": iid, "user": user.username, "time": time_ms, "state": state, "player": player})),
            _ => {}
        }
    }
    app.save();
    if state == "stopped" { app.fire("playback.stopped", json!({"id": iid, "user": user.username, "time": time_ms, "player": player})); }
    if watched_now { app.fire("playback.watched", json!({"id": iid, "user": user.username, "time": time_ms, "player": player})); }
    ok(json!({"ok": true, "watched": watched_now}))
}

pub fn sessions(app: &Arc<App>, user: &User) -> Resp {
    let db = app.db.lock().unwrap();
    let mut all: Vec<(Playing, &str)> = vec![];
    for p in app.playing.lock().unwrap().values() { all.push((p.clone(), "player")); }
    for p in app.progressive.lock().unwrap().values() { all.push((p.clone(), "progressive")); }
    for s in app.hls.lock().unwrap().values() {
        all.push((Playing { user_id: s.user_id, item_id: s.item_id, offset_ms: 0, state: "playing".into(), method: format!("{:?}", s.method).to_lowercase(), updated: s.last_access, client: format!("hls {}", s.id), height: s.height, audio: s.audio, viewer: s.viewer.clone(), session_at: 0, watched_ms: 0 }, "hls"));
    }
    let list: Vec<Value> = all.iter().filter(|(p, _)| user.admin || p.user_id == user.id).map(|(p, kind)| {
        let it = db.items.get(&p.item_id);
        json!({"user": db.user(p.user_id).map(|u| u.display_name()), "item": p.item_id, "title": it.map(|i| i.title.clone()),
            "type": it.map(|i| i.kind.as_str()), "offset": p.offset_ms, "duration": it.map(|i| i.duration_ms), "state": p.state,
            "method": p.method, "stream": kind, "client": p.client})
    }).collect();
    ok(json!({"MediaContainer": {"size": list.len(), "Metadata": list, "transcodes": app.active_transcodes()}}))
}


/// The player's request, held to the bitrate limit for people outside the home network.
/// (Takes the limit instead of the app: callers may already hold the library lock.)
pub fn transcode_request(limit: u32, boost_dialogue: bool, ctx: &Ctx) -> transcode::Request {
    let mut req = transcode_request_plain(ctx);
    req.boost_dialogue = boost_dialogue;
    if limit > 0 && !ctx.is_local() {
        let cap = limit.saturating_mul(1000);
        if req.max_kbps == 0 || req.max_kbps > cap { req.max_kbps = cap; }
    }
    req
}

fn transcode_request_plain(ctx: &Ctx) -> transcode::Request {
    let (mut h, mut kb) = transcode::quality(ctx.param("quality").unwrap_or("original"));
    if let Some(x) = ctx.num("maxVideoHeight").or(ctx.num("height")) { h = x as u32; }
    if let Some(x) = ctx.num("maxVideoBitrate").or(ctx.num("bitrate")) { kb = x as u32; }
    let list = |k: &str| ctx.param(k).map(|s| s.split(',').map(|x| x.trim().to_lowercase()).filter(|x| !x.is_empty()).collect()).unwrap_or_default();
    transcode::Request {
        max_height: h,
        max_kbps: kb,
        force_transcode: ctx.param("transcode") == Some("1") || ctx.num("subtitle").is_some(),
        audio_stream: ctx.num("audio").map(|a| a as u32),
        extra_video: list("videoCodecs"),
        extra_containers: list("containers"),
        video_quality: ctx.num("videoQuality").map(|q| q.min(99) as u32),
        h264_level: ctx.param("level").and_then(transcode::parse_level),
        burn_subtitle: ctx.num("subtitle").map(|n| n as u32),
        boost_dialogue: false,
        native: ctx.param("native") == Some("1"),
    }
}

/// The item and one of its files: `part` 0 is the file, 1 … the further parts
/// of a split movie (?part= on the streaming addresses).
pub(super) fn playable_file(app: &Arc<App>, user: &User, iid: &str, part: usize) -> Result<(Item, db::MediaFile), Resp> {
    let db = app.db.lock().unwrap();
    let it = iid.parse::<u64>().ok().and_then(|i| db.items.get(&i)).ok_or_else(|| fail(404, "Not found"))?;
    if !db.can_see_item(user, it) { return Err(fail(403, "You don't have access to this library")); }
    if it.file.is_none() { return Err(fail(400, "This entry has no video file")); }
    let f = it.part(part).cloned().ok_or_else(|| fail(404, "No such part"))?;
    let it = it.clone();
    drop(db);
    if !app.may_send(Path::new(&f.path), false) { return Err(fail(403, "This file is outside the library folders")); }
    Ok((it, f))
}

fn part_of(ctx: &Ctx) -> usize {
    ctx.num("part").map(|n| n as usize).unwrap_or(0)
}

/// Sends the original file, honouring Range requests so players can seek.
/// The owner stopped this playback a moment ago: no new stream for it yet.
fn remote_refusal(app: &Arc<App>, ctx: &Ctx, user: &User, item: u64) -> Option<Resp> {
    if user.admin || !app.remote_plays_over_limit(user.id, item, !ctx.is_local()) { return None; }
    Some(js(403, json!({"error": "You are already playing as many videos outside the home network as is allowed", "status": 403})))
}

fn stop_refusal(app: &Arc<App>, user: &User, item: u64) -> Option<Resp> {
    let m = app.stopped(user.id, item)?;
    Some(js(403, json!({"error": if m.is_empty() { "The owner stopped this playback".to_string() } else { m.clone() }, "status": 403, "stopped": true, "message": m})))
}

pub fn direct_file(app: &Arc<App>, ctx: &Ctx, user: &User, iid: &str) -> Resp {
    let (it, f) = match playable_file(app, user, iid, part_of(ctx)) { Ok(x) => x, Err(r) => return r };
    if ctx.param("download") != Some("1") {
        if let Some(r) = stop_refusal(app, user, it.id) { return r; }
        if let Some(r) = remote_refusal(app, ctx, user, it.id) { return r; }
    }
    if ctx.param("download") == Some("1") && !crate::playback::downloads::may_download(user) { return decision_fail(403, 2011); }
    if ctx.param("optimized") == Some("1") {
        let Some(p) = crate::api::actions::optimized_file(app, &it) else { return fail(404, "There is no optimized version") };
        return serve_file(&p, "video/mp4", ctx.range.as_deref(), ctx.param("download") == Some("1"));
    }
    serve_file(Path::new(&f.path), transcode::content_type(&f.container), ctx.range.as_deref(), ctx.param("download") == Some("1"))
}

pub fn serve_file(path: &Path, ctype: &str, range: Option<&str>, download: bool) -> Resp {
    let name = download.then(|| path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default());
    serve_file_as(path, ctype, range, name.as_deref())
}

/// "attachment; filename=…" for any name: a plain fallback plus the UTF-8 one.
pub fn attachment(name: &str) -> String {
    let plain: String = name.chars().map(|c| if c.is_ascii() && !c.is_ascii_control() && c != '"' && c != '\\' { c } else { '_' }).collect();
    let mut enc = String::new();
    for b in name.bytes() {
        if b.is_ascii_alphanumeric() || b"-._~".contains(&b) { enc.push(b as char) } else { enc.push_str(&format!("%{b:02X}")) }
    }
    format!("attachment; filename=\"{plain}\"; filename*=UTF-8''{enc}")
}

/// Sends a file; with `download` the browser saves it under that name.
pub fn serve_file_as(path: &Path, ctype: &str, range: Option<&str>, download: Option<&str>) -> Resp {
    let Ok(mut file) = std::fs::File::open(path) else { return decision_fail(404, 2012) };
    let size = file.metadata().map(|m| m.len()).unwrap_or(0);
    let mut h = base_headers();
    h.push(header("Content-Type", ctype));
    h.push(header("Accept-Ranges", "bytes"));
    if let Some(name) = download {
        h.push(header("Content-Disposition", &attachment(name)));
    }
    if let Some(r) = range {
        match parse_range(r, size) {
            Some((start, end)) if start < size && end >= start => {
                let _ = file.seek(SeekFrom::Start(start));
                let len = end - start + 1;
                h.push(header("Content-Range", &format!("bytes {start}-{end}/{size}")));
                return Response::new(StatusCode(206), h, Box::new(file.take(len)) as Box<dyn Read + Send>, Some(len as usize), None);
            }
            _ => {
                let mut h = base_headers();
                h.push(header("Content-Range", &format!("bytes */{size}")));
                return Response::new(StatusCode(416), h, Box::new(std::io::empty()) as Box<dyn Read + Send>, Some(0), None);
            }
        }
    }
    Response::new(StatusCode(200), h, Box::new(file) as Box<dyn Read + Send>, Some(size as usize), None)
}

pub fn parse_range(r: &str, size: u64) -> Option<(u64, u64)> {
    let r = r.trim().strip_prefix("bytes=")?;
    let first = r.split(',').next()?.trim();
    let (a, b) = first.split_once('-')?;
    if size == 0 {
        return None;
    }
    if a.is_empty() {
        let n: u64 = b.parse().ok()?;
        return Some((size.saturating_sub(n), size - 1));
    }
    let start: u64 = a.parse().ok()?;
    let end = if b.is_empty() { size - 1 } else { b.parse::<u64>().ok()?.min(size - 1) };
    Some((start, end))
}

/// Kills ffmpeg when the player goes away.
struct ChildStream {
    child: Child,
    out: ChildStdout,
    app: Arc<App>,
    key: String,
    user: u64,
    item: u64,
    reads: u32,
}

impl Read for ChildStream {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        // Stopped from the dashboard? (Looked at every 64 reads, not on each one.)
        self.reads = self.reads.wrapping_add(1);
        if self.reads % 64 == 0 && self.app.stopped(self.user, self.item).is_some() {
            return Err(std::io::Error::new(std::io::ErrorKind::ConnectionAborted, "stopped by the owner"));
        }
        self.out.read(buf)
    }
}

impl Drop for ChildStream {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        self.app.progressive.lock().unwrap().remove(&self.key);
    }
}

/// An error carrying a playback decision code and its message.
pub fn decision_fail(status: u16, code: u32) -> Resp {
    js(status, json!({"error": transcode::decision_text(code), "status": status, "decisionCode": code}))
}

/// Held from the limit check until the new stream is counted, so streams started at the same moment
/// can't all pass the check before any of them is running.
static STARTING: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn check_transcode_allowed(app: &Arc<App>, user: &User, method: &Method) -> Option<Resp> {
    if *method != Method::Transcode {
        return None;
    }
    if user.allow_transcode == Some(false) || !app.settings().transcoding_enabled {
        // Direct play isn't possible and this account may not convert.
        return Some(decision_fail(403, 2000));
    }
    let settings = app.settings();
    let mut max = settings.max_transcodes;
    // Separate limits for the graphics chip and the processor (every conversion uses the same one).
    if settings.max_gpu_transcodes > 0 || settings.max_cpu_transcodes > 0 {
        let own = if transcode::effective_hw(&settings) == "none" { settings.max_cpu_transcodes } else { settings.max_gpu_transcodes };
        if own > 0 && (max == 0 || own < max) { max = own; }
    }
    if max > 0 && app.active_transcodes() >= max {
        return Some(decision_fail(503, 4010));
    }
    None
}

/// Any container in, fragmented MP4 (H.264 + AAC) out, streamed while ffmpeg works.
pub fn progressive(app: &Arc<App>, ctx: &Ctx, user: &User, iid: &str) -> Resp {
    let (it, f) = match playable_file(app, user, iid, part_of(ctx)) { Ok(x) => x, Err(r) => return r };
    let req = { let st = app.settings(); transcode_request(st.remote_max_mbps, st.boost_dialogue, ctx) };
    let mut d = transcode::decide(&f, &req);
    if d.method == Method::Direct {
        d.method = Method::Remux;
    }
    if let Some(r) = stop_refusal(app, user, it.id) { return r; }
    if let Some(r) = remote_refusal(app, ctx, user, it.id) { return r; }
    let _turn = STARTING.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(r) = check_transcode_allowed(app, user, &d.method) {
        return r;
    }
    let settings = app.settings();
    let offset = ctx.param("offset").and_then(|o| o.parse::<f64>().ok()).unwrap_or(0.0).max(0.0);
    let args = transcode::progressive_args(&settings, &f, &d, &req, offset);
    let child = Command::new(&settings.ffmpeg).args(&args).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::null()).spawn();
    let mut child = match child {
        Ok(c) => c,
        Err(e) => return fail(500, &format!("ffmpeg could not be started ({e}). Install ffmpeg or set its path in the settings.")),
    };
    let out = child.stdout.take().unwrap();
    let key = db::random_hex(8);
    let method = format!("{:?}", d.method).to_lowercase();
    app.progressive.lock().unwrap().insert(key.clone(), Playing { user_id: user.id, item_id: it.id, offset_ms: (offset * 1000.0) as u64, state: "playing".into(), method: method.clone(), updated: Instant::now(), client: "browser".into(), height: d.height, audio: req.audio_stream, viewer: ctx.viewer(), session_at: 0, watched_ms: 0 });
    let mut h = base_headers();
    h.push(header("Content-Type", "video/mp4"));
    h.push(header("X-Transcode-Method", &method));
    h.push(header("Cache-Control", "no-store"));
    let stream = ChildStream { child, out, app: app.clone(), key, user: user.id, item: it.id, reads: 0 };
    Response::new(StatusCode(200), h, Box::new(stream) as Box<dyn Read + Send>, None, None)
}

/// HLS streams one person can have running at once (on all their devices).
pub const HLS_PER_USER: usize = 4;

pub fn hls_start(app: &Arc<App>, ctx: &Ctx, user: &User, iid: &str) -> Resp {
    let (it, f) = match playable_file(app, user, iid, part_of(ctx)) { Ok(x) => x, Err(r) => return r };
    let req = { let st = app.settings(); transcode_request(st.remote_max_mbps, st.boost_dialogue, ctx) };
    let mut d = transcode::decide(&f, &req);
    if d.method == Method::Direct {
        d.method = Method::Remux;
    }
    if let Some(r) = stop_refusal(app, user, it.id) { return r; }
    if let Some(r) = remote_refusal(app, ctx, user, it.id) { return r; }
    let _turn = STARTING.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(r) = check_transcode_allowed(app, user, &d.method) {
        return r;
    }
    // A player that retries, or quick switching between episodes, leaves streams running for 90 s:
    // one person keeps at most a few, and the one used longest ago makes room. A stream fetched in the
    // last 20 s is still being watched (one account on several devices), so then the start just goes ahead.
    let oldest = {
        let mut map = app.hls.lock().unwrap();
        let mine: Vec<(Instant, String)> = map.values().filter(|s| s.user_id == user.id).map(|s| (s.last_access, s.id.clone())).collect();
        if mine.len() >= HLS_PER_USER { mine.into_iter().filter(|(t, _)| t.elapsed() > Duration::from_secs(20)).min().and_then(|(_, k)| map.remove(&k)) } else { None }
    };
    // Not while other starts wait: deleting a long stream's folder can take a while on a NAS.
    if let Some(mut s) = oldest {
        std::thread::spawn(move || {
            let _ = s.child.kill();
            let _ = s.child.wait();
            let _ = std::fs::remove_dir_all(&s.dir);
        });
    }
    let settings = app.settings();
    let sid = db::random_hex(16);
    let dir = app.transcode_dir().join(&sid);
    if let Err(e) = std::fs::create_dir_all(&dir) {
        return fail(500, &format!("Could not make the transcode folder: {e}"));
    }
    let offset = ctx.param("offset").and_then(|o| o.parse::<f64>().ok()).unwrap_or(0.0).max(0.0);
    let args = transcode::hls_args(&settings, &f, &d, &req, offset, &dir.to_string_lossy().replace('\\', "/"));
    let child = match Command::new(&settings.ffmpeg).args(&args).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).spawn() {
        Ok(c) => c,
        Err(e) => return fail(500, &format!("ffmpeg could not be started ({e}). Install ffmpeg or set its path in the settings.")),
    };
    app.hls.lock().unwrap().insert(sid.clone(), HlsSession { id: sid.clone(), item_id: it.id, user_id: user.id, dir, child, last_access: Instant::now(), started_at: now(), method: d.method.clone(), height: d.height, audio: req.audio_stream, viewer: ctx.viewer() });
    let v = ctx.viewer();
    app.fire("playback.started", json!({"id": it.id, "user": user.username, "stream": "hls", "player": {"device": v.device, "ip": v.ip, "local": v.local, "client": "hls"}}));
    let playlist = format!("/video/transcode/hls/{sid}/index.m3u8");
    let mut h = base_headers();
    h.push(header("Location", &playlist));
    h.push(header("X-Session", &sid));
    h.push(header("Content-Type", "application/json"));
    let body = json!({"session": sid, "playlist": playlist, "method": d.method}).to_string();
    let len = body.len();
    let status = if ctx.param("redirect") == Some("0") { 200 } else { 302 };
    Response::new(StatusCode(status), h, Box::new(std::io::Cursor::new(body.into_bytes())) as Box<dyn Read + Send>, Some(len), None)
}

/// Serves the HLS playlist and segments, waiting for ffmpeg to write them.
pub fn hls_file(app: &Arc<App>, sid: &str, file: &str) -> Resp {
    let ok_name = file == "index.m3u8" || (file.starts_with("seg") && file.ends_with(".ts") && file.chars().all(|c| c.is_ascii_alphanumeric() || c == '.'));
    if !ok_name || !sid.chars().all(|c| c.is_ascii_hexdigit()) {
        return fail(404, "No such file");
    }
    let timeout = Duration::from_secs(app.settings().segmented_transcoder_timeout_seconds.max(5));
    let start = Instant::now();
    loop {
        let (dir, finished) = {
            let mut map = app.hls.lock().unwrap();
            let Some(s) = map.get_mut(sid) else { return fail(404, "This stream has ended") };
            s.last_access = Instant::now();
            let finished = matches!(s.child.try_wait(), Ok(Some(_)));
            (s.dir.clone(), finished)
        };
        let index = std::fs::read_to_string(dir.join("index.m3u8")).unwrap_or_default();
        let ready = if file == "index.m3u8" { index.contains(".ts") } else { index.lines().any(|l| l.trim() == file) };
        if ready || (finished && dir.join(file).exists()) {
            if file == "index.m3u8" {
                return bytes(200, "application/vnd.apple.mpegurl", index.into_bytes());
            }
            return serve_file(&dir.join(file), "video/mp2t", None, false);
        }
        if finished || start.elapsed() > timeout {
            return fail(if finished { 404 } else { 504 }, "The transcoder did not write this part in time");
        }
        std::thread::sleep(Duration::from_millis(150));
    }
}

/// A subtitle track as WebVTT, as it is in the file (n: "2" for the third embedded track, "x0" for the first side file).
pub fn subtitle_vtt(app: &Arc<App>, f: &db::MediaFile, n: &str) -> Result<Vec<u8>, Resp> {
    let (source, embedded) = if let Some(x) = n.strip_prefix('x') {
        let Some(p) = x.parse::<usize>().ok().and_then(|i| f.external_subtitles.get(i)) else { return Err(fail(404, "No such subtitle")) };
        if !app.may_send(Path::new(p), false) { return Err(fail(403, "This file is outside the library folders")); }
        if p.to_lowercase().ends_with(".vtt") {
            return std::fs::read(p).map_err(|_| fail(404, "No such subtitle"));
        }
        (p.clone(), None)
    } else {
        let Some(i) = n.parse::<u32>().ok() else { return Err(fail(404, "No such subtitle")) };
        (f.path.clone(), Some(i))
    };
    let ffmpeg = app.settings().ffmpeg;
    match Command::new(&ffmpeg).args(transcode::subtitle_args(&source, embedded)).stdin(Stdio::null()).stderr(Stdio::null()).output() {
        Ok(o) if o.status.success() && !o.stdout.is_empty() => Ok(o.stdout),
        Ok(_) => Err(fail(415, "This subtitle can't be turned into text (picture-based subtitles need burning in)")),
        Err(e) => Err(fail(500, &format!("ffmpeg could not be started: {e}"))),
    }
}

/// The timing set for a subtitle track (by synchronizing or by hand), if any.
pub fn subtitle_timing(it: &Item, n: &str) -> Option<crate::library::subsync::Timing> {
    it.extra.get("subsync").and_then(|s| s.get(n)).and_then(crate::library::subsync::Timing::from_json).filter(|t| !t.is_none())
}

/// A subtitle track as WebVTT, with its timing set by hand or by listening. `?from=<seconds>`: for a
/// converted stream that starts there (a Chromecast can't move the lines itself), the lines are moved
/// back by that much. Any page may read it (a Chromecast's player is a page of Google's): the address
/// only opens with a ticket, never with a cookie.
pub fn subtitles(app: &Arc<App>, ctx: &Ctx, user: &User, iid: &str, n: &str) -> Resp {
    let (it, f) = match playable_file(app, user, iid, 0) { Ok(x) => x, Err(r) => return r };
    let n = n.trim_end_matches(".vtt");
    let vtt = match subtitle_vtt(app, &f, n) { Ok(v) => v, Err(r) => return r };
    let from_ms = ctx.param("from").and_then(|s| s.parse::<f64>().ok()).filter(|s| s.is_finite() && *s > 0.0).map(|s| (s * 1000.0).round() as i64).unwrap_or(0);
    let timing = subtitle_timing(&it, n).unwrap_or(crate::library::subsync::Timing { offset_ms: 0, scale: 1.0 });
    let timing = crate::library::subsync::Timing { offset_ms: timing.offset_ms - from_ms, ..timing };
    let body = if timing.is_none() { vtt } else { crate::library::subsync::shift_vtt(&String::from_utf8_lossy(&vtt), timing).into_bytes() };
    let mut r = bytes(200, "text/vtt; charset=utf-8", body);
    r.add_header(header("Access-Control-Allow-Origin", "*"));
    r
}

/// Listens to the sound of the video and moves the subtitle track so its lines fall on the speech.
/// Slow (the whole audio is read): the answer says what was found, and it is only kept when it is certain.
pub fn subtitle_sync(app: &Arc<App>, user: &User, iid: &str, n: &str) -> Resp {
    let (it, f) = match playable_file(app, user, iid, 0) { Ok(x) => x, Err(r) => return r };
    let vtt = match subtitle_vtt(app, &f, n) { Ok(v) => v, Err(r) => return r };
    let cues = crate::library::subsync::cues(&String::from_utf8_lossy(&vtt));
    if cues.len() < 20 { return fail(422, "This subtitle has too few lines to synchronize"); }
    if !f.streams.iter().any(|s| s.kind == "audio") { return fail(422, "The video has no sound to listen to"); }
    let ffmpeg = app.settings().ffmpeg;
    let curve = match crate::library::subsync::loudness_curve(&ffmpeg, &f.path) { Ok(c) => c, Err(e) => return fail(502, &e) };
    let signal = crate::library::subsync::speech_signal(&curve);
    let Some((timing, sure)) = crate::library::subsync::find_timing(&cues, &signal) else {
        return js(200, json!({"ok": true, "applied": false, "message": "The subtitle could not be matched to the speech with certainty; nothing was changed"}));
    };
    {
        let mut db = app.db.lock().unwrap();
        let Some(item) = db.items.get_mut(&it.id) else { return fail(404, "Not found") };
        let sync = item.extra.entry("subsync").or_insert_with(|| json!({}));
        if let Some(o) = sync.as_object_mut() { o.insert(n.to_string(), timing.to_json()); }
    }
    app.save();
    js(200, json!({"ok": true, "applied": true, "offsetMs": timing.offset_ms, "scale": timing.scale, "confidence": (sure * 10.0).round() / 10.0}))
}

/// Sets the timing of a subtitle track by hand: `offset_ms` (and `scale`), `delta_ms` to move it further,
/// or `reset` to go back to the file's own timing.
pub fn subtitle_set_timing(app: &Arc<App>, user: &User, iid: &str, n: &str, body: &Value) -> Resp {
    let (it, f) = match playable_file(app, user, iid, 0) { Ok(x) => x, Err(r) => return r };
    let known = if let Some(x) = n.strip_prefix('x') { x.parse::<usize>().is_ok_and(|i| i < f.external_subtitles.len()) } else { n.parse::<usize>().is_ok() };
    if !known { return fail(404, "No such subtitle"); }
    let cur = subtitle_timing(&it, n).unwrap_or(crate::library::subsync::Timing { offset_ms: 0, scale: 1.0 });
    let next = if body["reset"].as_bool() == Some(true) {
        crate::library::subsync::Timing { offset_ms: 0, scale: 1.0 }
    } else {
        let offset = match (body["offset_ms"].as_i64(), body["delta_ms"].as_i64()) { (Some(o), _) => o, (None, Some(d)) => cur.offset_ms + d, _ => return fail(400, "Give offset_ms, delta_ms or reset") };
        let scale = body["scale"].as_f64().unwrap_or(cur.scale);
        if offset.abs() > 3_600_000 || !(0.5..=2.0).contains(&scale) { return fail(400, "That timing is out of range"); }
        crate::library::subsync::Timing { offset_ms: offset, scale }
    };
    {
        let mut db = app.db.lock().unwrap();
        let Some(item) = db.items.get_mut(&it.id) else { return fail(404, "Not found") };
        let sync = item.extra.entry("subsync").or_insert_with(|| json!({}));
        if let Some(o) = sync.as_object_mut() { if next.is_none() { o.remove(n); } else { o.insert(n.to_string(), next.to_json()); } }
    }
    app.save();
    js(200, json!({"ok": true, "offsetMs": next.offset_ms, "scale": next.scale}))
}



#[cfg(all(test, unix))]
mod session_tests {
    use super::*;

    /// An app with one movie in a library, a stand-in "ffmpeg" that just keeps running, and these settings.
    fn app(name: &str, max_transcodes: usize) -> (Arc<App>, PathBuf, User) {
        let root = std::env::temp_dir().join(format!("lichtspiel-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("movies")).unwrap();
        std::fs::write(root.join("movies/x.mp4"), b"x").unwrap();
        let ff = root.join("ffmpeg.sh");
        // Hardware checks (a lavfi test picture) just fail; a stream keeps going.
        std::fs::write(&ff, "#!/bin/sh\ncase \"$*\" in *lavfi*) exit 1;; esac\nsleep 30\n").unwrap();
        std::fs::set_permissions(&ff, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
        let app = App::open(&root.join("data"));
        {
            let mut db = app.db.lock().unwrap();
            db.settings.ffmpeg = ff.to_string_lossy().to_string();
            db.settings.max_transcodes = max_transcodes;
            db.sections.push(serde_json::from_value(json!({"id": 1, "name": "Filme", "kind": "movie", "paths": [root.join("movies").to_string_lossy()], "created_at": 0})).unwrap());
            let mut i: Item = serde_json::from_value(json!({"id": 1, "section_id": 1, "kind": "movie", "title": "x", "sort_title": "x", "added_at": 0, "updated_at": 0})).unwrap();
            i.file = Some(db::MediaFile { path: root.join("movies/x.mp4").to_string_lossy().to_string(), container: "mp4".into(), duration_ms: 1000, ..Default::default() });
            db.items.insert(1, i);
        }
        let user: User = serde_json::from_value(json!({"id": 1, "username": "o", "admin": true, "password_hash": "", "salt": "", "created_at": 0})).unwrap();
        (app, root, user)
    }

    fn ctx(params: &[(&str, &str)]) -> Ctx {
        Ctx { method: "GET".into(), path: "/video/start.m3u8".into(), params: params.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect(), body: Value::Null,
            user: None, host: String::new(), range: None, token: None, remote: Some([127, 0, 0, 1].into()), remote_trusted: true, lan_extra: false, device: "test".into() }
    }

    fn end_all(app: &App, root: &Path) {
        for (_, mut s) in std::mem::take(&mut *app.hls.lock().unwrap()) { let _ = s.child.kill(); let _ = s.child.wait(); }
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn starts_at_the_same_moment_keep_to_the_transcode_limit() {
        let (app, root, user) = app("transcode-race", 1);
        let go = Arc::new(std::sync::Barrier::new(8));
        let starts: Vec<_> = (0..8).map(|_| {
            let (app, user, go) = (app.clone(), user.clone(), go.clone());
            std::thread::spawn(move || { go.wait(); hls_start(&app, &ctx(&[("transcode", "1"), ("redirect", "0")]), &user, "1").status_code().0 })
        }).collect();
        let codes: Vec<u16> = starts.into_iter().map(|s| s.join().unwrap()).collect();
        let running = app.active_transcodes();
        end_all(&app, &root);
        assert_eq!(running, 1, "{running} conversions run with a limit of one (answers {codes:?})");
    }

    #[test]
    fn a_crash_while_starting_or_making_a_picture_does_not_block_the_next_ones() {
        let (app, root, user) = app("poisoned-locks", 0);
        // A thread that panics while it holds the locks.
        let _ = std::thread::spawn(|| { let _a = STARTING.lock(); let _b = PICTURES.0.lock(); panic!("boom") }).join();
        assert!(STARTING.is_poisoned() && PICTURES.0.is_poisoned());
        let code = std::panic::catch_unwind(|| hls_start(&app, &ctx(&[("redirect", "0")]), &user, "1").status_code().0);
        let pic = std::panic::catch_unwind(|| make_picture("false", &root.join("p.jpg"), |_| vec![]));
        end_all(&app, &root);
        assert_eq!(code.ok(), Some(200), "a video could not start after an earlier crash");
        assert_eq!(pic.ok(), Some(false), "pictures could not be made after an earlier crash");
    }

    #[test]
    fn one_person_cannot_pile_up_remux_streams() {
        let (app, root, user) = app("remux-pile", 0);
        for _ in 0..6 {
            assert_eq!(hls_start(&app, &ctx(&[("redirect", "0")]), &user, "1").status_code().0, 200);
            // Left behind by a player that moved on half a minute ago.
            for s in app.hls.lock().unwrap().values_mut() { s.last_access = Instant::now() - Duration::from_secs(30); }
        }
        let mine = app.hls.lock().unwrap().values().filter(|s| s.user_id == user.id).count();
        end_all(&app, &root);
        assert!(mine <= HLS_PER_USER, "{mine} streams kept running for one person");
    }

    #[test]
    fn the_seek_bar_picture_of_a_part_outside_the_libraries_is_refused() {
        let (app, root, user) = app("frame-outside", 0);
        std::fs::write(root.join("secret.mp4"), b"x").unwrap();
        // A stand-in "ffmpeg" that writes a picture to the last name it is given.
        let ff = root.join("picture.sh");
        std::fs::write(&ff, "#!/bin/sh\nfor a; do last=$a; done\necho x > \"$last\"\n").unwrap();
        std::fs::set_permissions(&ff, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
        {
            let mut db = app.db.lock().unwrap();
            db.settings.ffmpeg = ff.to_string_lossy().to_string();
            let path = root.join("secret.mp4").to_string_lossy().to_string();
            db.items.get_mut(&1).unwrap().parts.push(db::MediaFile { path, container: "mp4".into(), duration_ms: 1000, ..Default::default() });
        }
        let at = |t: &str| frame(&app, &ctx(&[("t", t), ("exact", "1")]), &user, "1").status_code().0;
        let (inside, outside) = (at("0"), at("1"));
        end_all(&app, &root);
        assert_eq!(inside, 200, "the picture of the first part was refused");
        assert_eq!(outside, 403, "a picture of a file outside the libraries was made");
    }

    #[test]
    fn a_stream_that_is_still_watched_is_not_stopped_for_another_device() {
        let (app, root, user) = app("shared-account", 0);
        for _ in 0..HLS_PER_USER + 1 {
            assert_eq!(hls_start(&app, &ctx(&[("redirect", "0")]), &user, "1").status_code().0, 200);
        }
        let running = app.hls.lock().unwrap().len();
        end_all(&app, &root);
        assert_eq!(running, HLS_PER_USER + 1, "a device that is playing was stopped");
    }
}

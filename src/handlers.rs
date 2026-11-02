//! The larger request handlers: accounts and invites, playlists, playback
//! progress, direct streaming with seeking, transcoding (progressive MP4
//! and HLS), subtitles, posters, folder browsing and plugins.

use crate::auth;
use crate::db::{self, now, Db, Item, ItemKind, Playlist, PlaylistKind, SmartQuery, User};
use crate::plugins;
use crate::scanner;
use crate::server::*;
use crate::transcode::{self, Method};
use serde_json::{json, Value};
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdout, Command, Stdio};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tiny_http::{Response, StatusCode};

// ---------------------------------------------------------------- accounts

pub fn new_token(db: &mut Db, user_id: u64, device: &str) -> String {
    let t = db::random_hex(20);
    db.tokens.push(db::Token { token: t.clone(), user_id, created_at: now(), device: device.into() });
    t
}

pub fn setup(app: &Arc<App>, ctx: &Ctx) -> Resp {
    let username = ctx.body_or_param("username").unwrap_or_default().trim().to_string();
    let password = ctx.body_or_param("password").unwrap_or_default();
    if !auth::valid_username(&username) { return fail(400, "Pick a user name of 2-40 letters or numbers"); }
    if password.chars().count() < 6 { return fail(400, "Use at least 6 characters for the password"); }
    let (h, s) = auth::hash_password(&password);
    let mut db = app.db.lock().unwrap();
    if !db.users.is_empty() { return fail(409, "This server is already set up; sign in instead"); }
    let id = db.new_id();
    db.users.push(User { id, username, email: ctx.body_or_param("email").unwrap_or_default(), password_hash: h, salt: s, admin: true, created_at: now(), allowed_sections: None, invited_by: None, allow_transcode: None, allow_delete: false, sidebar: serde_json::Value::Null });
    if let Some(name) = ctx.body_or_param("server_name").filter(|n| !n.trim().is_empty()) {
        db.settings.server_name = name;
    }
    let tok = new_token(&mut db, id, "setup");
    let u = user_json(db.user(id).unwrap());
    drop(db);
    app.save();
    js(201, json!({"token": tok, "user": u}))
}

pub fn login(app: &Arc<App>, ctx: &Ctx) -> Resp {
    let username = ctx.body_or_param("username").unwrap_or_default();
    let password = ctx.body_or_param("password").unwrap_or_default();
    let found = {
        let db = app.db.lock().unwrap();
        db.users.iter().find(|u| u.username.eq_ignore_ascii_case(username.trim())).cloned()
    };
    match found {
        Some(u) if auth::verify(&password, &u.salt, &u.password_hash) => {
            let mut db = app.db.lock().unwrap();
            let tok = new_token(&mut db, u.id, &ctx.body_or_param("device").unwrap_or_default());
            drop(db);
            app.save();
            ok(json!({"token": tok, "user": user_json(&u)}))
        }
        _ => {
            std::thread::sleep(Duration::from_millis(400));
            fail(401, "User name or password is wrong")
        }
    }
}

pub fn invite_info(app: &Arc<App>, code: &str) -> Resp {
    let db = app.db.lock().unwrap();
    match db.invites.iter().find(|i| i.code == code) {
        Some(i) if i.used_by.is_none() && i.expires_at >= now() => ok(json!({"valid": true, "server": crate::server::display_name(&db.settings), "email": i.email, "note": i.note,
            "invited_by": db.user(i.created_by).map(|u| u.username.clone()), "expires_at": i.expires_at})),
        Some(i) if i.used_by.is_some() => fail(410, "This invite was already used"),
        Some(_) => fail(410, "This invite has expired"),
        None => fail(404, "No such invite"),
    }
}

pub fn invite_accept(app: &Arc<App>, ctx: &Ctx, code: &str) -> Resp {
    let username = ctx.body_or_param("username").unwrap_or_default().trim().to_string();
    let password = ctx.body_or_param("password").unwrap_or_default();
    if !auth::valid_username(&username) { return fail(400, "Pick a user name of 2-40 letters or numbers"); }
    if password.chars().count() < 6 { return fail(400, "Use at least 6 characters for the password"); }
    let (h, s) = auth::hash_password(&password);
    let mut db = app.db.lock().unwrap();
    if db.users.iter().any(|u| u.username.eq_ignore_ascii_case(&username)) { return fail(409, "That user name is taken"); }
    let Some(inv) = db.invites.iter().find(|i| i.code == code).cloned() else { return fail(404, "No such invite") };
    if inv.used_by.is_some() { return fail(410, "This invite was already used"); }
    if inv.expires_at < now() { return fail(410, "This invite has expired"); }
    let id = db.new_id();
    let email = ctx.body_or_param("email").filter(|e| !e.is_empty()).unwrap_or(inv.email.clone());
    db.users.push(User { id, username: username.clone(), email, password_hash: h, salt: s, admin: inv.admin, created_at: now(), allowed_sections: inv.allowed_sections.clone(), invited_by: Some(inv.created_by), allow_transcode: None, allow_delete: false, sidebar: serde_json::Value::Null });
    if let Some(i) = db.invites.iter_mut().find(|i| i.code == code) {
        i.used_by = Some(id);
        i.used_at = Some(now());
    }
    let tok = new_token(&mut db, id, "invite");
    let u = user_json(db.user(id).unwrap());
    drop(db);
    app.save();
    app.fire("user.joined", json!({"username": username, "invite": code}));
    js(201, json!({"token": tok, "user": u}))
}

// ---------------------------------------------------------------- playlists

pub fn create_playlist(app: &Arc<App>, ctx: &Ctx, user: &User) -> Resp {
    let title = ctx.body_or_param("title").unwrap_or_default().trim().to_string();
    if title.is_empty() { return fail(400, "Give the playlist a title"); }
    let smart = ctx.body_or_param("kind").map(|k| k == "smart").unwrap_or(false)
        || ctx.body.get("smart").and_then(|v| v.as_bool()).unwrap_or(false)
        || ctx.param("smart") == Some("1")
        || ctx.body.get("query").is_some_and(|q| !q.is_null());
    let query = if smart {
        match serde_json::from_value::<SmartQuery>(ctx.body.get("query").cloned().unwrap_or(json!({}))) {
            Ok(q) => Some(q),
            Err(e) => return fail(400, &format!("The rules could not be read: {e}")),
        }
    } else {
        None
    };
    let ids = ids_from(ctx);
    let mut db = app.db.lock().unwrap();
    let mut items: Vec<u64> = vec![];
    for i in ids.iter().flat_map(|i| db.leaves(*i)) {
        if db.items.get(&i).is_some_and(|it| db.can_see_section(user, it.section_id)) && !items.contains(&i) {
            items.push(i);
        }
    }
    let id = db.new_id();
    let t = now();
    let p = Playlist {
        id,
        owner_id: user.id,
        title,
        summary: ctx.body_or_param("summary").unwrap_or_default(),
        kind: if smart { PlaylistKind::Smart } else { PlaylistKind::Manual },
        items: if smart { vec![] } else { items },
        query,
        default_key: None,
        shared: ctx.body.get("shared").and_then(|v| v.as_bool()).unwrap_or(false),
        pinned: ctx.body.get("pinned").and_then(|v| v.as_bool()).unwrap_or(false),
        created_at: t,
        updated_at: t,
    };
    let (v, _, _) = playlist_json(&db, user, &p);
    db.playlists.push(p);
    drop(db);
    app.save();
    app.fire("playlist.created", json!({"id": id, "title": v["title"], "smart": smart, "user": user.username}));
    js(201, container(vec![v], 1, 0, json!({})))
}

pub fn update_playlist(app: &Arc<App>, ctx: &Ctx, user: &User, pid: &str) -> Resp {
    let Some(pid) = pid.parse::<u64>().ok() else { return fail(404, "No such playlist") };
    let mut db = app.db.lock().unwrap();
    let Some(p) = db.playlists.iter_mut().find(|p| p.id == pid) else { return fail(404, "No such playlist") };
    if p.owner_id != user.id && !user.admin {
        return fail(403, "This playlist belongs to someone else");
    }
    if let Some(t) = ctx.body_or_param("title").filter(|t| !t.trim().is_empty()) { p.title = t; }
    if let Some(s) = ctx.body_or_param("summary") { p.summary = s; }
    if let Some(v) = ctx.body.get("shared").and_then(|v| v.as_bool()) { p.shared = v || p.is_default(); }
    if let Some(v) = ctx.body.get("pinned").and_then(|v| v.as_bool()) { p.pinned = v; }
    if let Some(q) = ctx.body.get("query").filter(|q| !q.is_null()) {
        if p.kind != PlaylistKind::Smart { return fail(400, "Only smart playlists have rules"); }
        match serde_json::from_value::<SmartQuery>(q.clone()) {
            Ok(q) => p.query = Some(q),
            Err(e) => return fail(400, &format!("The rules could not be read: {e}")),
        }
    }
    p.updated_at = now();
    let p = p.clone();
    let (v, _, _) = playlist_json(&db, user, &p);
    drop(db);
    app.save();
    app.fire("playlist.updated", json!({"id": pid}));
    ok(container(vec![v], 1, 0, json!({})))
}

// ---------------------------------------------------------------- playback progress

/// Progress from a player. Passing the watched point (by default the earlier of
/// 90 % and the first credits marker) counts one viewing and
/// clears the resume point.
pub fn timeline(app: &Arc<App>, user: &User, iid: u64, time_ms: u64, state: &str, client: String) -> Resp {
    let mut watched_now = false;
    {
        let mut db = app.db.lock().unwrap();
        let Some(it) = db.items.get(&iid) else { return fail(404, "Not found") };
        if !db.can_see_section(user, it.section_id) { return fail(403, "You don't have access to this library"); }
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
    let key = format!("{}:{}", user.id, iid);
    if state == "stopped" {
        app.playing.lock().unwrap().remove(&key);
    } else {
        let mut map = app.playing.lock().unwrap();
        let first = !map.contains_key(&key);
        map.insert(key, Playing { user_id: user.id, item_id: iid, offset_ms: time_ms, state: state.into(), method: "player".into(), updated: Instant::now(), client });
        drop(map);
        if first { app.fire("playback.started", json!({"id": iid, "user": user.username})); }
    }
    app.save();
    if state == "stopped" { app.fire("playback.stopped", json!({"id": iid, "user": user.username, "time": time_ms})); }
    if watched_now { app.fire("playback.watched", json!({"id": iid, "user": user.username})); }
    ok(json!({"ok": true, "watched": watched_now}))
}

pub fn sessions(app: &Arc<App>, user: &User) -> Resp {
    let db = app.db.lock().unwrap();
    let mut all: Vec<(Playing, &str)> = vec![];
    for p in app.playing.lock().unwrap().values() { all.push((p.clone(), "player")); }
    for p in app.progressive.lock().unwrap().values() { all.push((p.clone(), "progressive")); }
    for s in app.hls.lock().unwrap().values() {
        all.push((Playing { user_id: s.user_id, item_id: s.item_id, offset_ms: 0, state: "playing".into(), method: format!("{:?}", s.method).to_lowercase(), updated: s.last_access, client: format!("hls {}", s.id) }, "hls"));
    }
    let list: Vec<Value> = all.iter().filter(|(p, _)| user.admin || p.user_id == user.id).map(|(p, kind)| {
        let it = db.items.get(&p.item_id);
        json!({"user": db.user(p.user_id).map(|u| u.username.clone()), "item": p.item_id, "title": it.map(|i| i.title.clone()),
            "type": it.map(|i| i.kind.as_str()), "offset": p.offset_ms, "duration": it.map(|i| i.duration_ms), "state": p.state,
            "method": p.method, "stream": kind, "client": p.client})
    }).collect();
    ok(json!({"MediaContainer": {"size": list.len(), "Metadata": list, "transcodes": app.active_transcodes()}}))
}

// ---------------------------------------------------------------- streaming

pub fn transcode_request(ctx: &Ctx) -> transcode::Request {
    let (mut h, mut kb) = transcode::quality(ctx.param("quality").unwrap_or("original"));
    if let Some(x) = ctx.num("maxVideoHeight").or(ctx.num("height")) { h = x as u32; }
    if let Some(x) = ctx.num("maxVideoBitrate").or(ctx.num("bitrate")) { kb = x as u32; }
    let list = |k: &str| ctx.param(k).map(|s| s.split(',').map(|x| x.trim().to_lowercase()).filter(|x| !x.is_empty()).collect()).unwrap_or_default();
    transcode::Request {
        max_height: h,
        max_kbps: kb,
        force_transcode: ctx.param("transcode") == Some("1"),
        audio_stream: ctx.num("audio").map(|a| a as u32),
        extra_video: list("videoCodecs"),
        extra_containers: list("containers"),
        video_quality: ctx.num("videoQuality").map(|q| q.min(99) as u32),
        h264_level: ctx.param("level").and_then(transcode::parse_level),
    }
}

/// The item and one of its files: `part` 0 is the file, 1 … the further parts
/// of a split movie (?part= on the streaming addresses).
fn playable_file(app: &Arc<App>, user: &User, iid: &str, part: usize) -> Result<(Item, db::MediaFile), Resp> {
    let db = app.db.lock().unwrap();
    let it = iid.parse::<u64>().ok().and_then(|i| db.items.get(&i)).ok_or_else(|| fail(404, "Not found"))?;
    if !db.can_see_section(user, it.section_id) { return Err(fail(403, "You don't have access to this library")); }
    if it.file.is_none() { return Err(fail(400, "This entry has no video file")); }
    let f = it.part(part).cloned().ok_or_else(|| fail(404, "No such part"))?;
    Ok((it.clone(), f))
}

fn part_of(ctx: &Ctx) -> usize {
    ctx.num("part").map(|n| n as usize).unwrap_or(0)
}

/// Sends the original file, honouring Range requests so players can seek.
pub fn direct_file(app: &Arc<App>, ctx: &Ctx, user: &User, iid: &str) -> Resp {
    let (it, f) = match playable_file(app, user, iid, part_of(ctx)) { Ok(x) => x, Err(r) => return r };
    if ctx.param("optimized") == Some("1") {
        let Some(p) = crate::actions::optimized_file(app, &it) else { return fail(404, "There is no optimized version") };
        return serve_file(&p, "video/mp4", ctx.range.as_deref(), ctx.param("download") == Some("1"));
    }
    serve_file(Path::new(&f.path), transcode::content_type(&f.container), ctx.range.as_deref(), ctx.param("download") == Some("1"))
}

pub fn serve_file(path: &Path, ctype: &str, range: Option<&str>, download: bool) -> Resp {
    let Ok(mut file) = std::fs::File::open(path) else { return decision_fail(404, 2012) };
    let size = file.metadata().map(|m| m.len()).unwrap_or(0);
    let mut h = base_headers();
    h.push(header("Content-Type", ctype));
    h.push(header("Accept-Ranges", "bytes"));
    if download {
        let name = path.file_name().map(|n| n.to_string_lossy().replace('"', "")).unwrap_or_default();
        h.push(header("Content-Disposition", &format!("attachment; filename=\"{name}\"")));
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
}

impl Read for ChildStream {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
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

fn check_transcode_allowed(app: &Arc<App>, user: &User, method: &Method) -> Option<Resp> {
    if *method != Method::Transcode {
        return None;
    }
    if user.allow_transcode == Some(false) {
        // Direct play isn't possible and this account may not convert.
        return Some(decision_fail(403, 2000));
    }
    let max = app.settings().max_transcodes;
    if max > 0 && app.active_transcodes() >= max {
        return Some(decision_fail(503, 4010));
    }
    None
}

/// Any container in, fragmented MP4 (H.264 + AAC) out, streamed while ffmpeg works.
pub fn progressive(app: &Arc<App>, ctx: &Ctx, user: &User, iid: &str) -> Resp {
    let (it, f) = match playable_file(app, user, iid, part_of(ctx)) { Ok(x) => x, Err(r) => return r };
    let req = transcode_request(ctx);
    let mut d = transcode::decide(&f, &req);
    if d.method == Method::Direct {
        d.method = Method::Remux;
    }
    if let Some(r) = check_transcode_allowed(app, user, &d.method) {
        return r;
    }
    let settings = app.settings();
    let offset = ctx.param("offset").and_then(|o| o.parse::<f64>().ok()).unwrap_or(0.0).max(0.0);
    let args = transcode::progressive_args(&settings, &it, &f, &d, &req, offset);
    let child = Command::new(&settings.ffmpeg).args(&args).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::null()).spawn();
    let mut child = match child {
        Ok(c) => c,
        Err(e) => return fail(500, &format!("ffmpeg could not be started ({e}). Install ffmpeg or set its path in the settings.")),
    };
    let out = child.stdout.take().unwrap();
    let key = db::random_hex(8);
    let method = format!("{:?}", d.method).to_lowercase();
    app.progressive.lock().unwrap().insert(key.clone(), Playing { user_id: user.id, item_id: it.id, offset_ms: (offset * 1000.0) as u64, state: "playing".into(), method: method.clone(), updated: Instant::now(), client: "browser".into() });
    let mut h = base_headers();
    h.push(header("Content-Type", "video/mp4"));
    h.push(header("X-Transcode-Method", &method));
    h.push(header("Cache-Control", "no-store"));
    let stream = ChildStream { child, out, app: app.clone(), key };
    Response::new(StatusCode(200), h, Box::new(stream) as Box<dyn Read + Send>, None, None)
}

pub fn hls_start(app: &Arc<App>, ctx: &Ctx, user: &User, iid: &str) -> Resp {
    let (it, f) = match playable_file(app, user, iid, part_of(ctx)) { Ok(x) => x, Err(r) => return r };
    let req = transcode_request(ctx);
    let mut d = transcode::decide(&f, &req);
    if d.method == Method::Direct {
        d.method = Method::Remux;
    }
    if let Some(r) = check_transcode_allowed(app, user, &d.method) {
        return r;
    }
    let settings = app.settings();
    let sid = db::random_hex(16);
    let dir = app.data_dir.join("transcode").join(&sid);
    if let Err(e) = std::fs::create_dir_all(&dir) {
        return fail(500, &format!("Could not make the transcode folder: {e}"));
    }
    let offset = ctx.param("offset").and_then(|o| o.parse::<f64>().ok()).unwrap_or(0.0).max(0.0);
    let args = transcode::hls_args(&settings, &f, &d, &req, offset, &dir.to_string_lossy().replace('\\', "/"));
    let child = match Command::new(&settings.ffmpeg).args(&args).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).spawn() {
        Ok(c) => c,
        Err(e) => return fail(500, &format!("ffmpeg could not be started ({e}). Install ffmpeg or set its path in the settings.")),
    };
    app.hls.lock().unwrap().insert(sid.clone(), HlsSession { id: sid.clone(), item_id: it.id, user_id: user.id, dir, child, last_access: Instant::now(), started_at: now(), method: d.method.clone(), height: d.height });
    app.fire("playback.started", json!({"id": it.id, "user": user.username, "stream": "hls"}));
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

pub fn subtitles(app: &Arc<App>, user: &User, iid: &str, n: &str) -> Resp {
    let (_, f) = match playable_file(app, user, iid, 0) { Ok(x) => x, Err(r) => return r };
    let n = n.trim_end_matches(".vtt");
    let (source, embedded) = if let Some(x) = n.strip_prefix('x') {
        let Some(p) = x.parse::<usize>().ok().and_then(|i| f.external_subtitles.get(i)) else { return fail(404, "No such subtitle") };
        if p.to_lowercase().ends_with(".vtt") {
            return serve_file(Path::new(p), "text/vtt; charset=utf-8", None, false);
        }
        (p.clone(), None)
    } else {
        let Some(i) = n.parse::<u32>().ok() else { return fail(404, "No such subtitle") };
        (f.path.clone(), Some(i))
    };
    let ffmpeg = app.settings().ffmpeg;
    match Command::new(&ffmpeg).args(transcode::subtitle_args(&source, embedded)).stdin(Stdio::null()).stderr(Stdio::null()).output() {
        Ok(o) if o.status.success() && !o.stdout.is_empty() => bytes(200, "text/vtt; charset=utf-8", o.stdout),
        Ok(_) => fail(415, "This subtitle can't be turned into text (picture-based subtitles need burning in)"),
        Err(e) => fail(500, &format!("ffmpeg could not be started: {e}")),
    }
}

// ---------------------------------------------------------------- posters

/// The wide background picture from the online details (the show's for seasons and episodes).
pub fn art(app: &Arc<App>, user: &User, iid: &str) -> Resp {
    let path = {
        let db = app.db.lock().unwrap();
        let Some(it) = iid.parse::<u64>().ok().and_then(|i| db.items.get(&i)) else { return fail(404, "Not found") };
        if !db.can_see_section(user, it.section_id) {
            return fail(403, "No access");
        }
        let top_id = App::top_level(&db, it.id).unwrap_or(it.id);
        let top = db.items.get(&top_id);
        let online = it.extra.get("art").or_else(|| top.and_then(|t| t.extra.get("art"))).and_then(|v| v.as_str()).map(String::from);
        // No background online: a picture from the video itself (the movie's file,
        // or a show's first episode), a third of the way in, past opening titles.
        let video = top.and_then(|t| t.file.as_ref().map(|f| (f.path.clone(), f.duration_ms)))
            .or_else(|| db.leaves(top_id).first().and_then(|l| db.items.get(l)).and_then(|l| l.file.as_ref().map(|f| (f.path.clone(), l.duration_ms))));
        (online, video, top_id)
    };
    let (online, video, top_id) = path;
    // (Only real photos: a drawing saved by an older version is skipped.)
    if let Some((p, mime)) = online.and_then(|p| crate::metadata::file_image_type(Path::new(&p)).map(|m| (p, m))) {
        return with_cache(serve_file(Path::new(&p), mime, None, false));
    }
    let cache = app.data_dir.join("cache").join("art").join(format!("{top_id}.jpg"));
    if !std::fs::metadata(&cache).is_ok_and(|m| m.len() > 0) {
        let Some((src, dur)) = video else { return fail(404, "No background picture") };
        let _ = std::fs::create_dir_all(cache.parent().unwrap_or(Path::new(".")));
        let at = if dur > 0 { dur as f64 / 1000.0 / 3.0 } else { 60.0 };
        let ran = Command::new(app.settings().ffmpeg)
            .args(transcode::art_args(&src, at, &cache.to_string_lossy()))
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok_and(|s| s.success());
        if !ran || !cache.exists() {
            return fail(404, "No background picture");
        }
    }
    with_cache(serve_file(&cache, "image/jpeg", None, false))
}

/// An actor photo saved with the online details (metadata/{id}/cast/{n}.jpg).
pub fn cast_photo(app: &Arc<App>, user: &User, iid: &str, n: &str) -> Resp {
    let top = {
        let db = app.db.lock().unwrap();
        let Some(it) = iid.parse::<u64>().ok().and_then(|i| db.items.get(&i)) else { return fail(404, "Not found") };
        if !db.can_see_section(user, it.section_id) {
            return fail(403, "No access");
        }
        App::top_level(&db, it.id).unwrap_or(it.id)
    };
    let Ok(n) = n.parse::<u32>() else { return fail(404, "Not found") };
    let p = app.metadata_dir(top).join("cast").join(format!("{n}.jpg"));
    let Some(mime) = crate::metadata::file_image_type(&p) else { return fail(404, "No photo") };
    with_cache(serve_file(&p, mime, None, false))
}

pub fn thumb(app: &Arc<App>, user: &User, iid: &str) -> Resp {
    let (poster, file, duration, title, kind, first_leaf, id) = {
        let db = app.db.lock().unwrap();
        let Some(it) = iid.parse::<u64>().ok().and_then(|i| db.items.get(&i)) else { return fail(404, "Not found") };
        if !db.can_see_section(user, it.section_id) {
            return fail(403, "No access");
        }
        let mut poster = it.poster.clone();
        if poster.is_none() && it.kind == ItemKind::Season {
            poster = it.parent_id.and_then(|p| db.items.get(&p)).and_then(|s| s.poster.clone());
        }
        let first_leaf = if it.kind.playable() { None } else { db.leaves(it.id).first().copied() };
        (poster, it.file.as_ref().map(|f| f.path.clone()), it.file.as_ref().map_or(it.duration_ms, |f| f.duration_ms), it.title.clone(), it.kind, first_leaf, it.id)
    };
    // The type is read from the file itself (a .jpg may really be a PNG);
    // something that is not a photo at all is skipped.
    if let Some((p, mime)) = poster.and_then(|p| crate::metadata::file_image_type(Path::new(&p)).map(|m| (p, m))) {
        return with_cache(serve_file(Path::new(&p), mime, None, false));
    }
    let cache: PathBuf = app.data_dir.join("cache").join("thumbs").join(format!("{id}.jpg"));
    if std::fs::metadata(&cache).map(|m| m.len() > 0).unwrap_or(false) {
        return with_cache(serve_file(&cache, "image/jpeg", None, false));
    }
    let source = match (file, first_leaf) {
        (Some(f), _) => Some((f, duration)),
        (None, Some(leaf)) => {
            let db = app.db.lock().unwrap();
            db.items.get(&leaf).and_then(|l| l.file.as_ref().map(|f| (f.path.clone(), l.duration_ms)))
        }
        _ => None,
    };
    if let Some((src, dur)) = source {
        let at = if dur > 0 { (dur as f64 / 1000.0 * 0.1).min(300.0) } else { 5.0 };
        let ffmpeg = app.settings().ffmpeg;
        let ran = Command::new(&ffmpeg)
            .args(transcode::thumb_args(&src, at, &cache.to_string_lossy()))
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false);
        if ran && cache.exists() {
            return with_cache(serve_file(&cache, "image/jpeg", None, false));
        }
    }
    bytes(200, "image/svg+xml", placeholder_svg(&title, kind.as_str(), id).into_bytes())
}

pub fn with_cache(mut r: Resp) -> Resp {
    r.add_header(header("Cache-Control", "private, max-age=3600"));
    r
}

// ---------------------------------------------------------------- folders

pub fn browse(path: &str) -> Resp {
    let path = path.trim();
    if path.is_empty() {
        if cfg!(windows) {
            let drives: Vec<Value> = (b'C'..=b'Z')
                .map(|c| format!("{}:\\", c as char))
                .filter(|d| Path::new(d).exists())
                .map(|d| json!({"name": d, "path": d}))
                .collect();
            return ok(json!({"path": "", "parent": null, "dirs": drives, "videos_here": 0}));
        }
        return list_dir(Path::new("/"));
    }
    list_dir(Path::new(path))
}

fn list_dir(p: &Path) -> Resp {
    let Ok(rd) = std::fs::read_dir(p) else { return fail(404, "That folder can't be opened") };
    let mut dirs: Vec<(String, String)> = vec![];
    let mut videos = 0;
    for e in rd.flatten() {
        let path = e.path();
        let name = e.file_name().to_string_lossy().to_string();
        if path.is_dir() {
            if !name.starts_with('.') && !name.starts_with('@') && !name.starts_with('#') && name != "$RECYCLE.BIN" && name != "System Volume Information" {
                dirs.push((name, path.to_string_lossy().to_string()));
            }
        } else if scanner::is_video(&path) {
            videos += 1;
        }
    }
    dirs.sort_by_key(|d| d.0.to_lowercase());
    let parent = p.parent().map(|x| x.to_string_lossy().to_string()).filter(|s| !s.is_empty());
    let parent = if cfg!(windows) && parent.is_none() { Some(String::new()) } else { parent };
    ok(json!({"path": p.to_string_lossy(), "parent": parent,
        "dirs": dirs.iter().map(|(n, pth)| json!({"name": n, "path": pth})).collect::<Vec<_>>(), "videos_here": videos}))
}

// ---------------------------------------------------------------- plugins

pub fn plugin_list(app: &Arc<App>) -> Resp {
    let p = app.plugins.lock().unwrap();
    let db = app.db.lock().unwrap();
    let list: Vec<Value> = p.list.iter().map(|pl| {
        let st = db.plugins.get(&pl.manifest.id).cloned().unwrap_or_default();
        let mut settings = pl.manifest.settings.clone();
        for (k, v) in &st.settings {
            settings.insert(k.clone(), v.clone());
        }
        json!({"id": pl.manifest.id, "name": pl.manifest.name, "version": pl.manifest.version, "description": pl.manifest.description,
            "author": pl.manifest.author, "hooks": pl.manifest.hooks, "command": pl.manifest.command, "enabled": st.enabled,
            "settings": settings, "folder": pl.dir.to_string_lossy()})
    }).collect();
    ok(json!({"folder": p.dir.to_string_lossy(), "plugins": list, "errors": p.errors, "events": plugins::EVENTS}))
}

pub fn plugin_call(app: &Arc<App>, ctx: &Ctx, user: &User, pid: &str, action: &str) -> Resp {
    let pl = app.plugins.lock().unwrap().get(pid).cloned();
    let Some(pl) = pl else { return fail(404, "No such plugin") };
    let st = app.db.lock().unwrap().plugins.get(pid).cloned().unwrap_or_default();
    if !st.enabled {
        return fail(409, "This plugin is switched off");
    }
    let data = json!({"action": action, "body": ctx.body, "params": ctx.params.iter().map(|(k, v)| json!([k, v])).collect::<Vec<_>>(), "user": user.username, "admin": user.admin});
    match plugins::invoke(&pl, &st.settings, "api.call", &data, &app.server_info()) {
        Ok(v) => {
            app.plugins.lock().unwrap().note(pid, "api.call", true, action.to_string());
            ok(v)
        }
        Err(e) => {
            app.plugins.lock().unwrap().note(pid, "api.call", false, e.clone());
            fail(502, &format!("The plugin failed: {e}"))
        }
    }
}

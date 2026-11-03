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
    db.users.push(User { id, username, email: ctx.body_or_param("email").unwrap_or_default(), password_hash: h, salt: s, admin: true, created_at: now(), allowed_sections: None, invited_by: None, allow_transcode: None, allow_delete: false, allow_download: None, sidebar: serde_json::Value::Null, home: false, pin_hash: String::new(), pin_salt: String::new(), max_age: None, block_unrated: false });
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

// ---------------------------------------------------------------- home profiles

/// A home profile may be picked on the sign-in screen: it is marked as one, and an admin
/// only with a PIN (else anyone in the house could take over the server).
fn pickable(u: &User) -> bool {
    u.home && (!u.admin || !u.pin_hash.is_empty())
}

/// Set or remove (empty) a user's PIN.
pub fn set_pin(u: &mut User, pin: &str) -> Result<(), &'static str> {
    if pin.is_empty() {
        u.pin_hash.clear();
        u.pin_salt.clear();
        return Ok(());
    }
    if !auth::valid_pin(pin) { return Err("A PIN is 4 to 8 digits"); }
    let (h, s) = auth::hash_password(pin);
    u.pin_hash = h;
    u.pin_salt = s;
    Ok(())
}

/// Wrong PINs per user: (count, locked until). Five wrong ones lock the profile for five minutes.
static PIN_FAILS: std::sync::Mutex<Option<std::collections::HashMap<u64, (u32, u64)>>> = std::sync::Mutex::new(None);
const PIN_TRIES: u32 = 5;
const PIN_LOCK_SECS: u64 = 300;

pub fn home_profiles(app: &Arc<App>, ctx: &Ctx) -> Resp {
    let db = app.db.lock().unwrap();
    if !db.settings.home_profiles || !ctx.is_local() { return ok(json!({"local": ctx.is_local(), "profiles": []})); }
    let list: Vec<Value> = db.users.iter().filter(|u| pickable(u))
        .map(|u| json!({"id": u.id, "username": u.username, "has_pin": !u.pin_hash.is_empty()})).collect();
    ok(json!({"local": true, "profiles": list}))
}

pub fn home_login(app: &Arc<App>, ctx: &Ctx, uid: &str) -> Resp {
    if !app.db.lock().unwrap().settings.home_profiles { return fail(403, "Profiles on the sign-in screen are switched off"); }
    if !ctx.is_local() { return fail(403, "Profiles can only be picked in the local network; sign in with your password"); }
    let Some(uid) = uid.parse::<u64>().ok() else { return fail(404, "No such profile") };
    let Some(u) = app.db.lock().unwrap().user(uid).cloned().filter(pickable) else { return fail(404, "No such profile") };
    if !u.pin_hash.is_empty() {
        {
            let mut g = PIN_FAILS.lock().unwrap();
            if let Some((n, until)) = g.get_or_insert_with(Default::default).get(&uid).copied() {
                if n >= PIN_TRIES && until > now() { return fail(429, "Too many wrong PINs; try again in a few minutes"); }
            }
        }
        let pin = ctx.body_or_param("pin").unwrap_or_default();
        if !auth::verify(&pin, &u.pin_salt, &u.pin_hash) {
            std::thread::sleep(Duration::from_millis(400));
            let mut g = PIN_FAILS.lock().unwrap();
            let e = g.get_or_insert_with(Default::default).entry(uid).or_insert((0, 0));
            if e.1 != 0 && e.1 <= now() { *e = (0, 0); }
            e.0 += 1;
            if e.0 >= PIN_TRIES { e.1 = now() + PIN_LOCK_SECS; }
            return fail(401, "Wrong PIN");
        }
        PIN_FAILS.lock().unwrap().get_or_insert_with(Default::default).remove(&uid);
    }
    let mut db = app.db.lock().unwrap();
    let tok = new_token(&mut db, u.id, &ctx.body_or_param("device").unwrap_or_else(|| "profile".into()));
    drop(db);
    app.save();
    ok(json!({"token": tok, "user": user_json(&u)}))
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
    db.users.push(User { id, username: username.clone(), email, password_hash: h, salt: s, admin: inv.admin, created_at: now(), allowed_sections: inv.allowed_sections.clone(), invited_by: Some(inv.created_by), allow_transcode: None, allow_delete: false, allow_download: None, sidebar: serde_json::Value::Null, home: false, pin_hash: String::new(), pin_salt: String::new(), max_age: None, block_unrated: false });
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
        if db.items.get(&i).is_some_and(|it| db.can_see_item(user, it)) && !items.contains(&i) {
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
        app.fire("playback.stopped", json!({"id": iid, "user": user.username, "time": time_ms, "by_owner": true}));
        return ok(json!({"ok": false, "stopped": true, "message": message, "watched": watched_now}));
    }
    if state == "stopped" {
        app.playing.lock().unwrap().remove(&key);
        app.close_sessions(vec![entry]);
    } else {
        app.playing.lock().unwrap().insert(key, entry);
        match before {
            None => app.fire("playback.started", json!({"id": iid, "user": user.username, "time": time_ms, "state": state})),
            // Paused or playing again (not every 10-second report).
            Some(b) if b != state => app.fire("playback.progress", json!({"id": iid, "user": user.username, "time": time_ms, "state": state})),
            _ => {}
        }
    }
    app.save();
    if state == "stopped" { app.fire("playback.stopped", json!({"id": iid, "user": user.username, "time": time_ms})); }
    if watched_now { app.fire("playback.watched", json!({"id": iid, "user": user.username, "time": time_ms})); }
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
        json!({"user": db.user(p.user_id).map(|u| u.username.clone()), "item": p.item_id, "title": it.map(|i| i.title.clone()),
            "type": it.map(|i| i.kind.as_str()), "offset": p.offset_ms, "duration": it.map(|i| i.duration_ms), "state": p.state,
            "method": p.method, "stream": kind, "client": p.client})
    }).collect();
    ok(json!({"MediaContainer": {"size": list.len(), "Metadata": list, "transcodes": app.active_transcodes()}}))
}

// ---------------------------------------------------------------- streaming

/// The player's request, held to the bitrate limit for people outside the home network.
/// (Takes the limit instead of the app: callers may already hold the library lock.)
pub fn transcode_request(limit: u32, ctx: &Ctx) -> transcode::Request {
    let mut req = transcode_request_plain(ctx);
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
    if !db.can_see_item(user, it) { return Err(fail(403, "You don't have access to this library")); }
    if it.file.is_none() { return Err(fail(400, "This entry has no video file")); }
    let f = it.part(part).cloned().ok_or_else(|| fail(404, "No such part"))?;
    Ok((it.clone(), f))
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
    if ctx.param("download") == Some("1") && !crate::downloads::may_download(user) { return decision_fail(403, 2011); }
    if ctx.param("optimized") == Some("1") {
        let Some(p) = crate::actions::optimized_file(app, &it) else { return fail(404, "There is no optimized version") };
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

fn check_transcode_allowed(app: &Arc<App>, user: &User, method: &Method) -> Option<Resp> {
    if *method != Method::Transcode {
        return None;
    }
    if user.allow_transcode == Some(false) || !app.settings().transcoding_enabled {
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
    let req = transcode_request(app.settings().remote_max_mbps, ctx);
    let mut d = transcode::decide(&f, &req);
    if d.method == Method::Direct {
        d.method = Method::Remux;
    }
    if let Some(r) = stop_refusal(app, user, it.id) { return r; }
    if let Some(r) = remote_refusal(app, ctx, user, it.id) { return r; }
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
    app.progressive.lock().unwrap().insert(key.clone(), Playing { user_id: user.id, item_id: it.id, offset_ms: (offset * 1000.0) as u64, state: "playing".into(), method: method.clone(), updated: Instant::now(), client: "browser".into(), height: d.height, audio: req.audio_stream, viewer: ctx.viewer(), session_at: 0, watched_ms: 0 });
    let mut h = base_headers();
    h.push(header("Content-Type", "video/mp4"));
    h.push(header("X-Transcode-Method", &method));
    h.push(header("Cache-Control", "no-store"));
    let stream = ChildStream { child, out, app: app.clone(), key, user: user.id, item: it.id, reads: 0 };
    Response::new(StatusCode(200), h, Box::new(stream) as Box<dyn Read + Send>, None, None)
}

pub fn hls_start(app: &Arc<App>, ctx: &Ctx, user: &User, iid: &str) -> Resp {
    let (it, f) = match playable_file(app, user, iid, part_of(ctx)) { Ok(x) => x, Err(r) => return r };
    let req = transcode_request(app.settings().remote_max_mbps, ctx);
    let mut d = transcode::decide(&f, &req);
    if d.method == Method::Direct {
        d.method = Method::Remux;
    }
    if let Some(r) = stop_refusal(app, user, it.id) { return r; }
    if let Some(r) = remote_refusal(app, ctx, user, it.id) { return r; }
    if let Some(r) = check_transcode_allowed(app, user, &d.method) {
        return r;
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
        if !db.can_see_item(user, it) {
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
        if !db.can_see_item(user, it) {
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
        if !db.can_see_item(user, it) {
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

/// A small still at a moment of a movie or episode, for the player's seek bar
/// preview: ?t= seconds into the whole video (a split movie counts as one).
/// Rounded to 10 seconds and kept in cache/frames.
pub fn frame(app: &Arc<App>, ctx: &Ctx, user: &User, iid: &str) -> Resp {
    let want = ctx.param("t").and_then(|t| t.parse::<f64>().ok()).unwrap_or(0.0).max(0.0);
    let step = (want / 10.0).floor() as u64 * 10;
    let (it, _) = match playable_file(app, user, iid, 0) { Ok(x) => x, Err(r) => return r };
    // Which file the moment is in, and the time inside that file.
    let mut at = step as f64;
    let mut src = None;
    for f in it.all_files() {
        let d = f.duration_ms as f64 / 1000.0;
        if d <= 0.0 || at < d { src = Some(f.path.clone()); break; }
        at -= d;
    }
    let Some(src) = src.or_else(|| it.last_file().map(|f| f.path.clone())) else { return fail(404, "No video file") };
    let dir = app.data_dir.join("cache").join("frames");
    let _ = std::fs::create_dir_all(&dir);
    let out = dir.join(format!("{}-{step}.jpg", it.id));
    if !std::fs::metadata(&out).map(|m| m.len() > 0).unwrap_or(false) {
        let ffmpeg = app.settings().ffmpeg;
        let args: Vec<String> = vec![
            "-hide_banner".into(), "-loglevel".into(), "error".into(), "-nostdin".into(), "-y".into(),
            "-ss".into(), format!("{at:.2}"), "-i".into(), src, "-frames:v".into(), "1".into(),
            "-vf".into(), "scale=240:-2".into(), "-q:v".into(), "6".into(), out.to_string_lossy().to_string(),
        ];
        let ran = Command::new(&ffmpeg).args(&args).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).status().map(|s| s.success()).unwrap_or(false);
        if !ran || !out.exists() { return fail(404, "No picture at that moment"); }
    }
    let mut r = serve_file(&out, "image/jpeg", None, false);
    r.add_header(header("Cache-Control", "private, max-age=86400"));
    r
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
            "settings": settings, "folder": pl.dir.to_string_lossy(), "user_panel": pl.manifest.user_panel})
    }).collect();
    ok(json!({"folder": p.dir.to_string_lossy(), "plugins": list, "errors": p.errors, "events": plugins::EVENTS}))
}

/// The switched-on plugins that offer each person a panel of their own in
/// Settings (e.g. "Connect Trakt"); the plugin draws it through api.call "panel".
pub fn plugin_panels(app: &Arc<App>) -> Resp {
    let p = app.plugins.lock().unwrap();
    let db = app.db.lock().unwrap();
    let list: Vec<Value> = p.list.iter()
        .filter(|pl| pl.manifest.user_panel && db.plugins.get(&pl.manifest.id).is_some_and(|s| s.enabled))
        .map(|pl| json!({"id": pl.manifest.id, "name": pl.manifest.name, "description": pl.manifest.description}))
        .collect();
    ok(json!({"panels": list}))
}

/// Same id in two spellings (Trakt sends numbers, the details download may keep strings).
fn id_key(source: &str, v: &Value) -> Option<String> {
    let s = match v { Value::String(s) => s.trim().to_string(), Value::Number(n) => n.to_string(), _ => return None };
    (!s.is_empty() && s != "0").then(|| format!("{source}:{}", s.to_ascii_lowercase()))
}

/// Marks what a plugin found watched elsewhere. Each entry is {"id": n}, or
/// {"type": "movie"|"show", "ids": {imdb, tmdb, tvdb …}} (a show: every
/// episode), or {"type": "episode", "show": {"ids": {…}}, "season": s,
/// "episode": e}; "watched_at" (unix seconds) and "plays" are optional.
/// Returns (entries found, items newly watched). No playback.watched events:
/// the plugin would only send them back where they came from.
pub fn mark_watched_from_plugin(app: &Arc<App>, user: &User, list: &[Value]) -> (usize, usize) {
    let mut db = app.db.lock().unwrap();
    let mut by_id: std::collections::HashMap<String, Vec<u64>> = Default::default();
    for it in db.items.values() {
        if !matches!(it.kind, ItemKind::Movie | ItemKind::Show) || !db.can_see_item(user, it) { continue; }
        if let Some(Value::Object(ids)) = it.extra.get("ids") {
            for (k, v) in ids {
                if let Some(key) = id_key(k, v) { by_id.entry(key).or_default().push(it.id); }
            }
        }
    }
    let find = |ids: &Value, kind: ItemKind, db: &Db| -> Option<u64> {
        let Value::Object(m) = ids else { return None };
        m.iter().filter_map(|(k, v)| id_key(k, v)).filter_map(|k| by_id.get(&k)).flatten().copied()
            .find(|i| db.items.get(i).is_some_and(|x| x.kind == kind))
    };
    let (mut found, mut marked) = (0, 0);
    for e in list {
        let leaves: Vec<u64> = if let Some(id) = e["id"].as_u64() {
            match db.items.get(&id) {
                Some(it) if db.can_see_item(user, it) => db.leaves(id),
                _ => vec![],
            }
        } else {
            match e["type"].as_str().unwrap_or("") {
                "movie" => find(&e["ids"], ItemKind::Movie, &db).into_iter().collect(),
                "show" => find(&e["ids"], ItemKind::Show, &db).map(|s| db.leaves(s)).unwrap_or_default(),
                "episode" => {
                    let show = find(&e["show"]["ids"], ItemKind::Show, &db);
                    let (s, n) = (e["season"].as_u64(), e["episode"].as_u64());
                    show.map(|show| db.leaves(show).into_iter().filter(|l| {
                        let ep = db.items.get(l);
                        let season = ep.and_then(|x| x.parent_id).and_then(|p| db.items.get(&p));
                        ep.and_then(|x| x.index).map(u64::from) == n && season.and_then(|x| x.index).map(u64::from) == s
                    }).collect()).unwrap_or_default()
                }
                _ => vec![],
            }
        };
        if leaves.is_empty() { continue; }
        found += 1;
        let at = e["watched_at"].as_u64().filter(|t| *t > 0 && *t <= now()).unwrap_or_else(now);
        let plays = e["plays"].as_u64().unwrap_or(1).clamp(1, 1000) as u32;
        for leaf in leaves {
            let v = db.view_mut(user.id, leaf);
            if v.view_count > 0 { continue; }
            v.view_count = plays;
            v.last_viewed_at = v.last_viewed_at.max(at);
            v.view_offset_ms = 0;
            v.finished = false;
            marked += 1;
            db.record_play_at(user.id, leaf, true, at);
        }
    }
    drop(db);
    if marked > 0 { app.save(); }
    (found, marked)
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
        Ok(mut v) => {
            // {"mark_watched": [...]}: the plugin found these watched elsewhere
            // (Trakt's history …); they become watched for whoever called.
            let mut detail = action.to_string();
            if let Some(list) = v.get("mark_watched").and_then(|l| l.as_array()).cloned() {
                let (found, marked) = mark_watched_from_plugin(app, user, &list);
                if let Some(o) = v.as_object_mut() {
                    o.remove("mark_watched");
                    o.insert("matched".into(), json!(found));
                    o.insert("marked".into(), json!(marked));
                    o.insert("not_found".into(), json!(list.len().saturating_sub(found)));
                }
                detail = format!("{action}: {found} of {} found, {marked} newly watched", list.len());
            }
            app.plugins.lock().unwrap().note(pid, "api.call", true, detail);
            ok(v)
        }
        Err(e) => {
            app.plugins.lock().unwrap().note(pid, "api.call", false, e.clone());
            fail(502, &format!("The plugin failed: {e}"))
        }
    }
}

// ---------------------------------------------------------------- pictures chosen by hand

fn b64_decode(s: &str) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(s.len() * 3 / 4);
    let (mut acc, mut bits) = (0u32, 0);
    for c in s.bytes().filter(|c| !c.is_ascii_whitespace() && *c != b'=') {
        let v = match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' | b'-' => 62,
            b'/' | b'_' => 63,
            _ => return None,
        };
        acc = (acc << 6) | v as u32;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
            acc &= (1 << bits) - 1;
        }
    }
    Some(out)
}

pub const IMAGE_SLOTS: [&str; 4] = ["poster", "art", "logo", "square"];

/// Saves a picture for a poster, background, logo or square poster. Body: {data: base64} or {url}.
/// Pictures set by hand are locked: refreshing the online details keeps them.
pub fn set_image(app: &Arc<App>, iid: &str, slot: &str, body: &Value) -> Resp {
    let Some(id) = iid.parse::<u64>().ok() else { return fail(404, "Not found") };
    if !IMAGE_SLOTS.contains(&slot) {
        return fail(404, "No such picture");
    }
    let bytes = if let Some(d) = body["data"].as_str() {
        let d = d.split_once(',').map_or(d, |(_, rest)| rest);
        match b64_decode(d) {
            Some(b) => b,
            None => return fail(400, "The picture data is not valid"),
        }
    } else if let Some(u) = body["url"].as_str().filter(|u| u.starts_with("http://") || u.starts_with("https://")) {
        let agent = ureq::AgentBuilder::new().timeout(Duration::from_secs(20)).build();
        let mut buf = Vec::new();
        match agent.get(u).call() {
            Ok(r) => { let _ = r.into_reader().take(8 * 1024 * 1024).read_to_end(&mut buf); }
            Err(e) => return fail(502, &format!("The picture could not be downloaded: {e}")),
        }
        buf
    } else {
        return fail(400, "Send the picture as {data} (base64) or {url}");
    };
    if crate::metadata::image_type(&bytes).is_none() {
        return fail(415, "That is not a JPEG, PNG, WebP or GIF picture");
    }
    let dir = app.data_dir.join("custom").join(id.to_string());
    let _ = std::fs::create_dir_all(&dir);
    let path = dir.join(slot);
    if let Err(e) = std::fs::write(&path, &bytes) {
        return fail(500, &format!("The picture could not be saved: {e}"));
    }
    {
        let mut db = app.db.lock().unwrap();
        let Some(it) = db.items.get_mut(&id) else { return fail(404, "Not found") };
        let p = path.to_string_lossy().to_string();
        if slot == "poster" { it.poster = Some(p); } else { it.extra.insert(slot.into(), json!(p)); }
        if !it.locked.iter().any(|l| l == slot) { it.locked.push(slot.into()); }
        it.updated_at = now();
    }
    drop_picture_caches(app, id);
    app.save();
    ok(json!({"ok": true}))
}

/// Takes the hand-chosen picture away again (a refresh brings the online one back).
pub fn clear_image(app: &Arc<App>, iid: &str, slot: &str) -> Resp {
    let Some(id) = iid.parse::<u64>().ok() else { return fail(404, "Not found") };
    if !IMAGE_SLOTS.contains(&slot) {
        return fail(404, "No such picture");
    }
    {
        let mut db = app.db.lock().unwrap();
        let Some(it) = db.items.get_mut(&id) else { return fail(404, "Not found") };
        if slot == "poster" { it.poster = None; } else { it.extra.remove(slot); }
        it.locked.retain(|l| l != slot);
        it.updated_at = now();
    }
    let _ = std::fs::remove_file(app.data_dir.join("custom").join(id.to_string()).join(slot));
    drop_picture_caches(app, id);
    app.save();
    ok(json!({"ok": true}))
}

fn drop_picture_caches(app: &Arc<App>, id: u64) {
    let _ = std::fs::remove_file(app.data_dir.join("cache").join("thumbs").join(format!("{id}.jpg")));
    let _ = std::fs::remove_file(app.data_dir.join("cache").join("art").join(format!("{id}.jpg")));
}

/// The logo or square poster chosen for an item.
pub fn custom_image(app: &Arc<App>, user: &User, iid: &str, slot: &str) -> Resp {
    let path = {
        let db = app.db.lock().unwrap();
        let Some(it) = iid.parse::<u64>().ok().and_then(|i| db.items.get(&i)) else { return fail(404, "Not found") };
        if !db.can_see_item(user, it) {
            return fail(403, "No access");
        }
        it.extra.get(slot).and_then(|v| v.as_str()).map(String::from)
    };
    let Some(p) = path else { return fail(404, "No picture") };
    let Some(mime) = crate::metadata::file_image_type(Path::new(&p)) else { return fail(404, "No picture") };
    with_cache(serve_file(Path::new(&p), mime, None, false))
}

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
    db.tokens.push(db::Token { token: auth::token_hash(&t), user_id, created_at: now(), device: device.chars().take(100).collect(), last_used: 0, last_device: String::new(), last_ip: String::new() });
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
    db.users.push(User { id, username, email: ctx.body_or_param("email").unwrap_or_default(), password_hash: h, salt: s, admin: true, created_at: now(), allowed_sections: None, invited_by: None, allow_transcode: None, allow_delete: false, allow_download: None, sidebar: serde_json::Value::Null, home: false, pin_hash: String::new(), pin_salt: String::new(), max_age: None, block_unrated: false, rss_key: String::new(), full_name: String::new(), totp_secret: String::new(), totp_pending: String::new(), prefs: Default::default() });
    if let Some(name) = ctx.body_or_param("server_name").filter(|n| !n.trim().is_empty()) {
        db.settings.server_name = name;
    }
    let tok = new_token(&mut db, id, "setup");
    let u = user_json(db.user(id).unwrap());
    drop(db);
    app.save();
    js(201, json!({"token": tok, "user": u}))
}

/// Wrong sign-ins per user name and per address: (count, locked until). Five wrong ones for a
/// user name (30 for an address, which may be a whole household) lock it for five minutes.
static LOGIN_FAILS: std::sync::Mutex<Option<std::collections::HashMap<String, (u32, u64)>>> = std::sync::Mutex::new(None);
/// The last code step accepted per user: a code works once.
static USED_STEPS: std::sync::Mutex<Option<std::collections::HashMap<u64, u64>>> = std::sync::Mutex::new(None);

/// Takes a code step for a user; false if it (or a later one) was used already.
fn use_step(user: u64, step: u64) -> bool {
    let mut g = USED_STEPS.lock().unwrap_or_else(|e| e.into_inner());
    let last = g.get_or_insert_with(Default::default).entry(user).or_insert(0);
    if step <= *last { return false; }
    *last = step;
    true
}

fn login_keys(ctx: &Ctx, name: &str) -> [(String, u32); 2] {
    [(format!("user:{}", name.trim().to_lowercase()), 5), (format!("ip:{}", ctx.remote.map(|a| a.to_string()).unwrap_or_default()), 30)]
}

/// Minutes left of a lock, if one of the keys is locked.
fn login_locked(keys: &[(String, u32)]) -> Option<u64> {
    let g = LOGIN_FAILS.lock().unwrap();
    let map = g.as_ref()?;
    keys.iter().filter_map(|(k, limit)| map.get(k).filter(|(n, until)| n >= limit && *until > now()).map(|(_, until)| (until - now()).div_ceil(60))).max()
}

fn login_failed(keys: &[(String, u32)]) {
    let mut g = LOGIN_FAILS.lock().unwrap();
    let map = g.get_or_insert_with(Default::default);
    if map.len() > 10_000 { map.retain(|_, (_, until)| *until > now()); }
    for (k, limit) in keys {
        let e = map.entry(k.clone()).or_insert((0, 0));
        if e.1 != 0 && e.1 <= now() { *e = (0, 0); }
        e.0 += 1;
        if e.0 >= *limit { e.1 = now() + PIN_LOCK_SECS; }
    }
}

pub fn login(app: &Arc<App>, ctx: &Ctx) -> Resp {
    let username = ctx.body_or_param("username").unwrap_or_default();
    let password = ctx.body_or_param("password").unwrap_or_default();
    let keys = login_keys(ctx, &username);
    if let Some(min) = login_locked(&keys) {
        return fail(429, &format!("Too many wrong attempts; try again in {min} minute(s)"));
    }
    let found = {
        let db = app.db.lock().unwrap();
        db.users.iter().find(|u| u.username.eq_ignore_ascii_case(username.trim())).cloned()
    };
    match found {
        Some(u) if auth::verify(&password, &u.salt, &u.password_hash) => {
            if !u.totp_secret.is_empty() {
                let code = ctx.body_or_param("code").unwrap_or_default();
                if code.trim().is_empty() {
                    return js(401, json!({"error": "Enter the code from your authenticator app", "status": 401, "needs_code": true}));
                }
                let step = crate::totp::matching_step(&u.totp_secret, &code, now());
                if !step.is_some_and(|s| use_step(u.id, s)) {
                    std::thread::sleep(Duration::from_millis(400));
                    login_failed(&keys);
                    return js(401, json!({"error": if step.is_some() { "That code was already used; wait for the next one" } else { "That code is wrong" }, "status": 401, "needs_code": true}));
                }
            }
            if let Some(m) = LOGIN_FAILS.lock().unwrap().as_mut() { m.remove(&keys[0].0); }
            let mut db = app.db.lock().unwrap();
            let tok = new_token(&mut db, u.id, &ctx.body_or_param("device").unwrap_or_default());
            drop(db);
            app.save();
            ok(json!({"token": tok, "user": user_json(&u)}))
        }
        _ => {
            std::thread::sleep(Duration::from_millis(400));
            login_failed(&keys);
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
        .map(|u| json!({"id": u.id, "username": u.username, "display_name": u.display_name(), "has_pin": !u.pin_hash.is_empty()})).collect();
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
            "invited_by": db.user(i.created_by).map(|u| u.display_name()), "expires_at": i.expires_at})),
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
    db.users.push(User { id, username: username.clone(), email, password_hash: h, salt: s, admin: inv.admin, created_at: now(), allowed_sections: inv.allowed_sections.clone(), invited_by: Some(inv.created_by), allow_transcode: None, allow_delete: false, allow_download: None, sidebar: serde_json::Value::Null, home: false, pin_hash: String::new(), pin_salt: String::new(), max_age: None, block_unrated: false, rss_key: String::new(), full_name: String::new(), totp_secret: String::new(), totp_pending: String::new(), prefs: Default::default() });
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

// ---------------------------------------------------------------- streaming

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
fn playable_file(app: &Arc<App>, user: &User, iid: &str, part: usize) -> Result<(Item, db::MediaFile), Resp> {
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
pub fn subtitle_timing(it: &Item, n: &str) -> Option<crate::subsync::Timing> {
    it.extra.get("subsync").and_then(|s| s.get(n)).and_then(crate::subsync::Timing::from_json).filter(|t| !t.is_none())
}

pub fn subtitles(app: &Arc<App>, user: &User, iid: &str, n: &str) -> Resp {
    let (it, f) = match playable_file(app, user, iid, 0) { Ok(x) => x, Err(r) => return r };
    let n = n.trim_end_matches(".vtt");
    let vtt = match subtitle_vtt(app, &f, n) { Ok(v) => v, Err(r) => return r };
    match subtitle_timing(&it, n) {
        Some(t) => bytes(200, "text/vtt; charset=utf-8", crate::subsync::shift_vtt(&String::from_utf8_lossy(&vtt), t).into_bytes()),
        None => bytes(200, "text/vtt; charset=utf-8", vtt),
    }
}

/// Listens to the sound of the video and moves the subtitle track so its lines fall on the speech.
/// Slow (the whole audio is read): the answer says what was found, and it is only kept when it is certain.
pub fn subtitle_sync(app: &Arc<App>, user: &User, iid: &str, n: &str) -> Resp {
    let (it, f) = match playable_file(app, user, iid, 0) { Ok(x) => x, Err(r) => return r };
    let vtt = match subtitle_vtt(app, &f, n) { Ok(v) => v, Err(r) => return r };
    let cues = crate::subsync::cues(&String::from_utf8_lossy(&vtt));
    if cues.len() < 20 { return fail(422, "This subtitle has too few lines to synchronize"); }
    if !f.streams.iter().any(|s| s.kind == "audio") { return fail(422, "The video has no sound to listen to"); }
    let ffmpeg = app.settings().ffmpeg;
    let curve = match crate::subsync::loudness_curve(&ffmpeg, &f.path) { Ok(c) => c, Err(e) => return fail(502, &e) };
    let signal = crate::subsync::speech_signal(&curve);
    let Some((timing, sure)) = crate::subsync::find_timing(&cues, &signal) else {
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
    let cur = subtitle_timing(&it, n).unwrap_or(crate::subsync::Timing { offset_ms: 0, scale: 1.0 });
    let next = if body["reset"].as_bool() == Some(true) {
        crate::subsync::Timing { offset_ms: 0, scale: 1.0 }
    } else {
        let offset = match (body["offset_ms"].as_i64(), body["delta_ms"].as_i64()) { (Some(o), _) => o, (None, Some(d)) => cur.offset_ms + d, _ => return fail(400, "Give offset_ms, delta_ms or reset") };
        let scale = body["scale"].as_f64().unwrap_or(cur.scale);
        if offset.abs() > 3_600_000 || !(0.5..=2.0).contains(&scale) { return fail(400, "That timing is out of range"); }
        crate::subsync::Timing { offset_ms: offset, scale }
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
    let video = video.filter(|(p, _)| app.may_send(Path::new(p), false));
    // (Only real photos: a drawing saved by an older version is skipped.)
    if let Some((p, mime)) = online.filter(|p| app.may_send(Path::new(p), true)).and_then(|p| crate::metadata::file_image_type(Path::new(&p)).map(|m| (p, m))) {
        return with_cache(serve_file(Path::new(&p), mime, None, false));
    }
    let cache = app.data_dir.join("cache").join("art").join(format!("{top_id}.jpg"));
    if !std::fs::metadata(&cache).is_ok_and(|m| m.len() > 0) {
        let Some((src, dur)) = video else { return fail(404, "No background picture") };
        let _ = std::fs::create_dir_all(cache.parent().unwrap_or(Path::new(".")));
        let at = if dur > 0 { dur as f64 / 1000.0 / 3.0 } else { 60.0 };
        if !make_picture(&app.settings().ffmpeg, &cache, |tmp| transcode::art_args(&src, at, tmp)) {
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

/// Copies being made right now (at most two at a time, so a page of fresh posters does not start a
/// hundred ffmpeg processes at once and slow the whole server down).
static RESIZING: std::sync::Mutex<std::collections::BTreeSet<PathBuf>> = std::sync::Mutex::new(std::collections::BTreeSet::new());

/// A smaller copy of a poster (`width` px wide, never enlarged) from cache/thumbs. When it is not there
/// yet, ffmpeg makes it in the background and None is returned, so this request gets the original; the
/// next one finds the small copy.
fn scaled_poster(app: &Arc<App>, id: u64, src: &Path, width: u32) -> Option<PathBuf> {
    let out = app.data_dir.join("cache").join("thumbs").join(format!("{id}_w{width}.jpg"));
    let modified = |p: &Path| std::fs::metadata(p).and_then(|m| m.modified()).ok();
    if std::fs::metadata(&out).is_ok_and(|m| m.len() > 0) && modified(&out) >= modified(src) { return Some(out); }
    {
        let mut busy = RESIZING.lock().unwrap();
        if busy.len() >= 2 || !busy.insert(out.clone()) { return None; }
    }
    let (ffmpeg, src, key) = (app.settings().ffmpeg, src.to_path_buf(), out.clone());
    std::thread::spawn(move || {
        let _ = std::fs::create_dir_all(out.parent().unwrap_or(Path::new(".")));
        // Written under a name of its own and then renamed, so nobody reads a half-written picture.
        let tmp = out.with_file_name(format!("{id}_w{width}.tmp.jpg"));
        let ran = Command::new(ffmpeg)
            .args(["-hide_banner", "-loglevel", "error", "-nostdin", "-y", "-i"]).arg(&src)
            .args(["-frames:v", "1", "-vf", &format!("scale='min({width},iw)':-2"), "-q:v", "4"]).arg(&tmp)
            .stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null())
            .status().is_ok_and(|s| s.success());
        if !(ran && std::fs::rename(&tmp, &out).is_ok()) { let _ = std::fs::remove_file(&tmp); }
        RESIZING.lock().unwrap().remove(&key);
    });
    None
}

pub fn thumb(app: &Arc<App>, user: &User, iid: &str, width: Option<u32>) -> Resp {
    let (poster, file, duration, title, kind, id) = {
        let db = app.db.lock().unwrap();
        let Some(it) = iid.parse::<u64>().ok().and_then(|i| db.items.get(&i)) else { return fail(404, "Not found") };
        if !db.can_see_item(user, it) {
            return fail(403, "No access");
        }
        let mut poster = it.poster.clone();
        if poster.is_none() && it.kind == ItemKind::Season {
            poster = it.parent_id.and_then(|p| db.items.get(&p)).and_then(|s| s.poster.clone());
        }
        // A song without its own picture shows its album's.
        if poster.is_none() && it.kind == ItemKind::Track {
            poster = it.parent_id.and_then(|p| db.items.get(&p)).and_then(|a| a.poster.clone());
        }
        (poster, it.file.as_ref().map(|f| f.path.clone()), it.file.as_ref().map_or(it.duration_ms, |f| f.duration_ms), it.title.clone(), it.kind, it.id)
    };
    // The type is read from the file itself (a .jpg may really be a PNG);
    // something that is not a photo at all is skipped.
    if let Some((p, mime)) = poster.filter(|p| app.may_send(Path::new(p), true)).and_then(|p| crate::metadata::file_image_type(Path::new(&p)).map(|m| (p, m))) {
        if let Some(small) = width.filter(|w| *w >= 64).and_then(|w| scaled_poster(app, id, Path::new(&p), w)) {
            return with_cache(serve_file(&small, "image/jpeg", None, false));
        }
        // A small copy was asked for but is still being made: the original now, kept only briefly,
        // so the browser asks again soon and then gets the small one.
        if width.is_some_and(|w| w >= 64) {
            let mut r = serve_file(Path::new(&p), mime, None, false);
            r.add_header(header("Cache-Control", "private, max-age=60"));
            return r;
        }
        return with_cache(serve_file(Path::new(&p), mime, None, false));
    }
    let cache: PathBuf = app.data_dir.join("cache").join("thumbs").join(format!("{id}.jpg"));
    if std::fs::metadata(&cache).map(|m| m.len() > 0).unwrap_or(false) {
        return with_cache(serve_file(&cache, "image/jpeg", None, false));
    }
    // The first episode of a show or season is looked for only now: searching the library for it on every
    // request (also for the many that have a poster) held up every other request.
    let source = match file {
        Some(f) => Some((f, duration)),
        None if !kind.playable() => {
            let db = app.db.lock().unwrap();
            db.leaves(id).first().and_then(|leaf| db.items.get(leaf)).and_then(|l| l.file.as_ref().map(|f| (f.path.clone(), l.duration_ms)))
        }
        None => None,
    };
    if let Some((src, dur)) = source.filter(|(p, _)| app.may_send(Path::new(p), false)) {
        let at = if dur > 0 { (dur as f64 / 1000.0 * 0.1).min(300.0) } else { 5.0 };
        let ffmpeg = app.settings().ffmpeg;
        // Music: the cover art inside the file; a still of the "picture" would be nothing.
        let music = matches!(kind, ItemKind::Artist | ItemKind::Album | ItemKind::Track);
        let ran = make_picture(&ffmpeg, &cache, |tmp| if music { transcode::cover_args(&src, tmp) } else { transcode::thumb_args(&src, at, tmp) });
        if ran {
            return with_cache(serve_file(&cache, "image/jpeg", None, false));
        }
    }
    bytes(200, "image/svg+xml", placeholder_svg(&title, kind.as_str(), id).into_bytes())
}

/// Posters, backgrounds and seek-bar pictures made with ffmpeg at the same time: a page of new titles
/// waits its turn instead of starting a hundred ffmpeg processes at once.
pub const PICTURES_AT_ONCE: usize = 3;
static PICTURES: (std::sync::Mutex<usize>, std::sync::Condvar) = (std::sync::Mutex::new(0), std::sync::Condvar::new());

/// Runs ffmpeg with `args(tmp)` to make the picture `out`. It is written under a name of its own and
/// renamed when done, so nobody reads a half-written file. False when ffmpeg failed or no turn came within 20 s.
pub fn make_picture(ffmpeg: &str, out: &Path, args: impl FnOnce(&str) -> Vec<String>) -> bool {
    let (count, freed) = &PICTURES;
    {
        let n = count.lock().unwrap_or_else(|e| e.into_inner());
        let (mut n, waited) = freed.wait_timeout_while(n, Duration::from_secs(20), |n| *n >= PICTURES_AT_ONCE).unwrap_or_else(|e| e.into_inner());
        if waited.timed_out() { return false; }
        *n += 1;
    }
    let tmp = out.with_extension(format!("{}.tmp.jpg", db::random_hex(4)));
    let ran = Command::new(ffmpeg).args(args(&tmp.to_string_lossy())).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).status().is_ok_and(|s| s.success());
    *count.lock().unwrap_or_else(|e| e.into_inner()) -= 1;
    freed.notify_one();
    let done = ran && std::fs::metadata(&tmp).is_ok_and(|m| m.len() > 0) && std::fs::rename(&tmp, out).is_ok();
    if !done { let _ = std::fs::remove_file(&tmp); }
    done
}

/// A small still at a moment of a movie or episode, for the player's seek bar
/// preview: ?t= seconds into the whole video (a split movie counts as one).
/// Rounded to 10 seconds and kept in cache/frames.
/// Takes one small picture at `at` seconds into `src` and saves it as `out` (already there = done).
pub fn make_frame(ffmpeg: &str, src: &str, at: f64, out: &std::path::Path) -> bool {
    if std::fs::metadata(out).map(|m| m.len() > 0).unwrap_or(false) { return true; }
    make_picture(ffmpeg, out, |tmp| vec![
        "-hide_banner".into(), "-loglevel".into(), "error".into(), "-nostdin".into(), "-y".into(),
        "-ss".into(), format!("{at:.2}"), "-i".into(), src.to_string(), "-frames:v".into(), "1".into(),
        "-vf".into(), "scale=240:-2".into(), "-q:v".into(), "6".into(), tmp.to_string(),
    ])
}

/// The second whose picture stands for a chapter: a little after its start (chapters often begin black).
pub fn chapter_picture_at(start_ms: u64, end_ms: u64) -> u64 {
    (start_ms as f64 / 1000.0 + ((end_ms as f64 - start_ms as f64) / 1000.0).min(6.0) / 2.0).floor() as u64
}

pub fn frame(app: &Arc<App>, ctx: &Ctx, user: &User, iid: &str) -> Resp {
    let want = ctx.param("t").and_then(|t| t.parse::<f64>().ok()).unwrap_or(0.0).max(0.0);
    // ?exact=1 (chapter pictures) keeps the second; the seek bar's pictures are rounded to 10 s.
    let step = if ctx.param("exact") == Some("1") { want.floor() as u64 } else { (want / 10.0).floor() as u64 * 10 };
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
    if !app.may_send(Path::new(&src), false) { return fail(403, "This file is outside the library folders"); }
    let dir = app.data_dir.join("cache").join("frames");
    let _ = std::fs::create_dir_all(&dir);
    let out = dir.join(format!("{}-{step}.jpg", it.id));
    if !make_frame(&app.settings().ffmpeg, &src, at, &out) { return fail(404, "No picture at that moment"); }
    let mut r = serve_file(&out, "image/jpeg", None, false);
    r.add_header(header("Cache-Control", "private, max-age=86400"));
    r
}

/// Pictures may be kept for a day: a changed poster or background has a new address (?v= the item's
/// change time), so a kept copy is never an outdated one.
pub fn with_cache(mut r: Resp) -> Resp {
    r.add_header(header("Cache-Control", "private, max-age=86400"));
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
            "settings": settings, "folder": pl.dir.to_string_lossy(), "user_panel": pl.manifest.user_panel, "reads_library": pl.manifest.reads_library})
    }).collect();
    ok(json!({"folder": p.dir.to_string_lossy(), "plugins": list, "errors": p.errors, "events": plugins::EVENTS}))
}

/// The switched-on plugins that offer each person a panel of their own in
/// Settings (e.g. "Connect Trakt"); the plugin draws it through api.call "panel".
pub fn plugin_panels(app: &Arc<App>, user: &User) -> Resp {
    let p = app.plugins.lock().unwrap();
    let db = app.db.lock().unwrap();
    let list: Vec<Value> = p.list.iter()
        .filter(|pl| pl.manifest.user_panel && db.plugins.get(&pl.manifest.id).is_some_and(|s| s.enabled))
        .filter(|pl| user.admin || !pl.manifest.panel_admins_only)
        .map(|pl| json!({"id": pl.manifest.id, "name": pl.manifest.name, "description": pl.manifest.description}))
        .collect();
    ok(json!({"panels": list}))
}

/// The whole library for a plugin that reads it (`reads_library`), with the
/// key the server gave it in `server.library`; no sign-in needed.
pub fn plugin_library(app: &Arc<App>, ctx: &Ctx, pid: &str) -> Resp {
    let pl = app.plugins.lock().unwrap().get(pid).cloned();
    let enabled = app.db.lock().unwrap().plugins.get(pid).is_some_and(|s| s.enabled);
    let given = ctx.param("key").unwrap_or("");
    match pl {
        Some(pl) if pl.manifest.reads_library && enabled && !given.is_empty() && given == app.plugin_key(pid) => ok(app.library_listing()),
        _ => fail(403, "This plugin may not read the library"),
    }
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

/// Deletes the titles a plugin named (`delete_media`), each as if the caller
/// had pressed "Delete" on it; gives how many went and why others did not.
pub fn delete_media_from_plugin(app: &Arc<App>, user: &User, list: &[Value]) -> (usize, Vec<String>) {
    let (mut deleted, mut errors) = (0, vec![]);
    for id in list.iter().take(500).filter_map(|v| v.as_u64()) {
        let r = crate::actions::delete_media(app, user, id);
        if r.status_code().0 == 200 { deleted += 1; continue; }
        let mut b = Vec::new();
        let _ = r.into_reader().read_to_end(&mut b);
        let why = serde_json::from_slice::<Value>(&b).ok().and_then(|v| v["error"].as_str().map(String::from)).unwrap_or_else(|| "could not be deleted".into());
        if !errors.contains(&why) { errors.push(why); }
    }
    (deleted, errors)
}

pub fn plugin_call(app: &Arc<App>, ctx: &Ctx, user: &User, pid: &str, action: &str) -> Resp {
    let pl = app.plugins.lock().unwrap().get(pid).cloned();
    let Some(pl) = pl else { return fail(404, "No such plugin") };
    let st = app.db.lock().unwrap().plugins.get(pid).cloned().unwrap_or_default();
    if !st.enabled {
        return fail(409, "This plugin is switched off");
    }
    let data = json!({"action": action, "body": ctx.body, "params": ctx.params.iter().map(|(k, v)| json!([k, v])).collect::<Vec<_>>(), "user": user.username, "user_name": user.display_name(), "admin": user.admin,
        "may_delete": crate::actions::may_delete(&app.db.lock().unwrap(), user)});
    match plugins::invoke(&pl, &st.settings, "api.call", &data, &app.server_info_for(&pl)) {
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
            // {"delete_media": [ids]}: the files go, with the caller's own right to
            // delete (and only while media deletion is allowed in the settings).
            if let Some(list) = v.get("delete_media").and_then(|l| l.as_array()).cloned() {
                let (deleted, errors) = delete_media_from_plugin(app, user, &list);
                if let Some(o) = v.as_object_mut() {
                    o.remove("delete_media");
                    o.insert("deleted".into(), json!(deleted));
                    if !errors.is_empty() {
                        o.remove("message");
                        o.insert("error".into(), json!(errors.join("; ")));
                    }
                }
                detail = format!("{action}: {deleted} of {} deleted", list.len());
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
        let agent = ureq::AgentBuilder::new().timeout(Duration::from_secs(20)).resolver(crate::server::public_only).build();
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
    let Some(p) = path.filter(|p| app.may_send(Path::new(p), true)) else { return fail(404, "No picture") };
    let Some(mime) = crate::metadata::file_image_type(Path::new(&p)) else { return fail(404, "No picture") };
    with_cache(serve_file(Path::new(&p), mime, None, false))
}

// ---------------------------------------------------------------- movie pre-roll

/// The video files named in the settings that exist.
fn preroll_files(app: &Arc<App>) -> Vec<String> {
    let list = app.settings().preroll_paths;
    list.split(|c| c == ';' || c == '\n').map(|p| p.trim().trim_matches('"').to_string()).filter(|p| !p.is_empty() && Path::new(p).is_file()).collect()
}

/// GET /api/preroll → {url, urls}: what to play before a movie, by the setting "preroll_mode"
/// (one at random, all in the order listed, or always the first); {url: null} when there is none.
pub fn preroll(app: &Arc<App>, ctx: &Ctx) -> Resp {
    let files = preroll_files(app);
    if files.is_empty() { return ok(json!({"url": null, "urls": []})); }
    let tok = match &ctx.user { Some(u) => format!("?ticket={}", crate::auth::ticket(&app.db.lock().unwrap().ticket_key, u.id, "p", ctx.token.as_deref(), now()).0), None => String::new() };
    let url = |n: usize| format!("/api/preroll/{n}/file{tok}");
    let picked: Vec<usize> = match app.settings().preroll_mode.as_str() {
        "all" => (0..files.len()).collect(),
        "first" => vec![0],
        _ => vec![(std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.subsec_nanos()).unwrap_or(0) as usize) % files.len()],
    };
    let urls: Vec<String> = picked.into_iter().map(url).collect();
    ok(json!({"url": urls.first(), "urls": urls, "count": files.len()}))
}

/// GET /api/preroll/{n}/file: the n-th pre-roll video, with seeking.
pub fn preroll_file(app: &Arc<App>, ctx: &Ctx, n: &str) -> Resp {
    let files = preroll_files(app);
    let Some(path) = n.parse::<usize>().ok().and_then(|i| files.get(i)) else { return fail(404, "No such pre-roll video") };
    let ext = Path::new(path).extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
    serve_file_as(Path::new(path), transcode::content_type(&ext), ctx.range.as_deref(), None)
}

// ---------------------------------------------------------------- theme music

const THEME_EXTS: [&str; 6] = ["mp3", "m4a", "flac", "ogg", "wav", "aac"];

/// A theme file's name without its ending, "theme-?[0-9]?": theme, theme1, theme-1.
fn is_theme_name(stem: &str) -> bool {
    let s = stem.to_lowercase();
    let Some(rest) = s.strip_prefix("theme") else { return false };
    let rest = rest.strip_prefix('-').unwrap_or(rest);
    rest.is_empty() || (rest.len() == 1 && rest.as_bytes()[0].is_ascii_digit())
}

/// The first theme file in a folder, by name.
fn theme_in(dir: &Path) -> Option<PathBuf> {
    let mut found: Vec<PathBuf> = std::fs::read_dir(dir)
        .ok()?
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.file_stem().is_some_and(|s| is_theme_name(&s.to_string_lossy())))
        .filter(|p| p.extension().is_some_and(|e| THEME_EXTS.contains(&e.to_string_lossy().to_lowercase().as_str())))
        .filter(|p| p.is_file())
        .collect();
    found.sort();
    found.into_iter().next()
}

/// A theme file (theme.mp3, theme-1.mp3 …, also .m4a, .flac, .ogg, .wav, .aac) next to the first
/// file of a movie or show, or one folder above it (a show's season folder).
pub(crate) fn theme_path(db: &Db, item: &Item) -> Option<PathBuf> {
    let first = db.leaves(item.id).first().and_then(|id| db.items.get(id))?;
    let file = first.file.as_ref().map(|f| f.path.clone()).or_else(|| first.all_files().first().map(|f| f.path.clone()))?;
    let dir = Path::new(&file).parent()?;
    let mut dirs = vec![dir];
    if first.kind != ItemKind::Movie {
        if let Some(up) = dir.parent() { dirs.push(up); }
    }
    dirs.into_iter().find_map(theme_in)
}

/// GET /library/metadata/{id}/theme: the theme music of a movie or show, with seeking: a theme file in
/// its folder, else for a show the one cut from its intro during maintenance.
pub fn theme(app: &Arc<App>, ctx: &Ctx, user: &User, iid: &str) -> Resp {
    let (local, show) = {
        let db = app.db.lock().unwrap();
        let Some(it) = iid.parse::<u64>().ok().and_then(|i| db.items.get(&i)).filter(|i| db.can_see_item(user, i)) else { return fail(404, "Not found") };
        if !matches!(it.kind, ItemKind::Movie | ItemKind::Show) { return fail(404, "No theme music"); }
        (theme_path(&db, it), (it.kind == ItemKind::Show && db.settings.theme_from_intros).then_some(it.id))
    };
    let cut = show.map(|id| app.data_dir.join(crate::maintenance::THEME_DIR).join(format!("{id}.m4a"))).filter(|p| p.is_file());
    let Some(path) = local.filter(|p| app.may_send(p, false)).or(cut) else { return fail(404, "No theme music") };
    let ext = path.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
    let ctype = match ext.as_str() { "mp3" => "audio/mpeg", "m4a" | "aac" => "audio/mp4", "flac" => "audio/flac", "ogg" => "audio/ogg", _ => "audio/wav" };
    serve_file_as(&path, ctype, ctx.range.as_deref(), None)
}

// ---------------------------------------------------------------- chapters

/// GET /library/metadata/{id}/chapters: [{index, title, start, end, thumb}] (ms), over all parts of a split movie.
pub fn chapters(app: &Arc<App>, user: &User, iid: &str) -> Resp {
    let (it, _) = match playable_file(app, user, iid, 0) { Ok(x) => x, Err(r) => return r };
    let ffprobe = app.settings().ffprobe;
    let (mut list, mut offset) = (vec![], 0u64);
    for f in it.all_files() {
        for (s, e, title) in scanner::read_chapters(&ffprobe, &f.path).unwrap_or_default() {
            list.push((offset + s, offset + e, title));
        }
        offset += f.duration_ms;
    }
    let out: Vec<Value> = list.iter().enumerate().map(|(i, (s, e, title))| {
        // The picture is taken a little after the start (a chapter often begins on a black frame).
        let at = chapter_picture_at(*s, *e);
        json!({"index": i + 1, "title": title, "start": s, "end": e, "thumb": format!("/library/parts/{}/frame?exact=1&t={at}", it.id)})
    }).collect();
    ok(json!({"MediaContainer": {"size": out.len(), "Chapter": out}}))
}

/// GET /library/metadata/{id}/lyrics: {synced, lines: [{t (ms, only when synced), text}]}, 404 without words.
pub fn lyrics(app: &Arc<App>, user: &User, iid: &str) -> Resp {
    let (it, f) = match playable_file(app, user, iid, 0) { Ok(x) => x, Err(r) => return r };
    if it.kind != ItemKind::Track { return fail(404, "Only songs have lyrics"); }
    let ffprobe = app.settings().ffprobe;
    let found = crate::music::lyrics_of(Path::new(&f.path), || {
        let out = Command::new(&ffprobe).args(["-v", "quiet", "-print_format", "json", "-show_format", "-show_streams"]).arg(&f.path).output().ok()?;
        serde_json::from_slice::<Value>(&out.stdout).ok().map(|v| crate::music::tags_of(&v))
    });
    match found {
        Some(l) => ok(json!({"synced": l.synced, "lines": l.lines.iter().map(|(t, w)| match t { Some(t) => json!({"t": t, "text": w}), None => json!({"text": w}) }).collect::<Vec<_>>()})),
        None => fail(404, "No lyrics"),
    }
}

#[cfg(test)]
mod step_tests {
    /// Parallel sign-ins with the same code: only one may use it.
    #[test]
    fn a_code_step_is_taken_once_even_in_parallel() {
        for user in 1_000_000..1_000_200u64 {
            let gate = std::sync::Arc::new(std::sync::Barrier::new(8));
            let won: usize = (0..8).map(|_| { let g = gate.clone(); std::thread::spawn(move || { g.wait(); super::use_step(user, 7) }) })
                .collect::<Vec<_>>().into_iter().map(|t| t.join().unwrap() as usize).sum();
            assert_eq!(won, 1, "user {user}: the same code worked {won} times");
        }
        assert!(!super::use_step(1_000_000, 6) && super::use_step(1_000_000, 8));
    }
}

#[cfg(test)]
mod theme_tests {
    use super::is_theme_name;

    #[test]
    fn theme_file_names() {
        for n in ["theme", "Theme", "theme1", "theme-2"] { assert!(is_theme_name(n), "{n}"); }
        for n in ["themes", "theme12", "theme-song", "my theme"] { assert!(!is_theme_name(n), "{n}"); }
    }
}

// ---------------------------------------------------------------- code sign-in

/// A TV or phone asked for a code; someone signed in elsewhere may confirm it.
struct LinkRequest {
    id: String,
    code: String,
    device: String,
    expires: u64,
    /// Set by the person who entered the code.
    approved_by: Option<u64>,
}

static LINKS: std::sync::Mutex<Vec<LinkRequest>> = std::sync::Mutex::new(Vec::new());
/// Wrong codes entered per signed-in user: (count, window start).
static LINK_FAILS: std::sync::Mutex<Option<std::collections::HashMap<u64, (u32, u64)>>> = std::sync::Mutex::new(None);
const LINK_TTL_SECS: u64 = 600;
const LINK_MAX_PENDING: usize = 200;
const LINK_TRIES: u32 = 8;
const LINK_WINDOW_SECS: u64 = 300;
/// No 0/O or 1/I: the code is read from a TV and typed on another device.
const LINK_ALPHABET: &[u8] = b"ABCDEFGHJKLMNPQRSTUVWXYZ23456789";

fn link_code() -> String {
    let mut b = [0u8; 4];
    let _ = getrandom::getrandom(&mut b);
    b.iter().map(|x| LINK_ALPHABET[*x as usize % LINK_ALPHABET.len()] as char).collect()
}

fn link_normalize(code: &str) -> String {
    code.chars().filter(|c| c.is_ascii_alphanumeric()).collect::<String>().to_ascii_uppercase()
}

/// The device asks for a code to show on the screen (no sign-in needed).
pub fn link_start(ctx: &Ctx) -> Resp {
    let mut g = LINKS.lock().unwrap();
    g.retain(|l| l.expires > now());
    if g.len() >= LINK_MAX_PENDING { return fail(429, "Too many sign-in codes are waiting; try again in a few minutes"); }
    let mut code = link_code();
    while g.iter().any(|l| l.code == code) { code = link_code(); }
    let id = db::random_hex(16);
    let device = ctx.body_or_param("device").filter(|d| !d.trim().is_empty()).unwrap_or_else(|| ctx.device.clone());
    g.push(LinkRequest { id: id.clone(), code: code.clone(), device, expires: now() + LINK_TTL_SECS, approved_by: None });
    ok(json!({"id": id, "code": code, "expires_in": LINK_TTL_SECS}))
}

/// The device asks whether its code was entered yet. The token is handed over once.
pub fn link_poll(app: &Arc<App>, id: &str) -> Resp {
    let taken = {
        let mut g = LINKS.lock().unwrap();
        g.retain(|l| l.expires > now());
        let Some(pos) = g.iter().position(|l| l.id == id) else { return fail(410, "This code has expired; ask for a new one") };
        if g[pos].approved_by.is_none() { return ok(json!({"status": "pending"})); }
        g.remove(pos)
    };
    let uid = taken.approved_by.unwrap_or(0);
    let mut db = app.db.lock().unwrap();
    let Some(u) = db.user(uid).cloned() else { return fail(410, "That account no longer exists") };
    let tok = new_token(&mut db, uid, &taken.device);
    drop(db);
    app.save();
    ok(json!({"status": "ok", "token": tok, "user": user_json(&u)}))
}

/// A signed-in person types the code shown on the device and signs it in as themselves.
pub fn link_confirm(ctx: &Ctx, user: &User) -> Resp {
    let code = link_normalize(&ctx.body_or_param("code").unwrap_or_default());
    {
        let mut g = LINK_FAILS.lock().unwrap();
        let e = g.get_or_insert_with(Default::default).entry(user.id).or_insert((0, now()));
        if e.1 + LINK_WINDOW_SECS <= now() { *e = (0, now()); }
        if e.0 >= LINK_TRIES { return fail(429, "Too many wrong codes; try again in a few minutes"); }
    }
    let mut g = LINKS.lock().unwrap();
    g.retain(|l| l.expires > now());
    match g.iter_mut().find(|l| l.code == code && l.approved_by.is_none()) {
        Some(l) => {
            l.approved_by = Some(user.id);
            ok(json!({"ok": true, "device": l.device}))
        }
        None => {
            drop(g);
            if let Some(e) = LINK_FAILS.lock().unwrap().get_or_insert_with(Default::default).get_mut(&user.id) { e.0 += 1; }
            fail(404, "That code is wrong or has expired")
        }
    }
}

#[cfg(all(test, unix))]
mod picture_tests {
    use super::*;

    /// A stand-in "ffmpeg" (a shell script) in a fresh folder; `$out` is its last argument (the picture).
    fn fake_ffmpeg(name: &str, script: &str) -> (PathBuf, String) {
        let root = std::env::temp_dir().join(format!("lichtspiel-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let ff = root.join("ffmpeg.sh");
        std::fs::write(&ff, format!("#!/bin/sh\nfor out; do :; done\n{script}\n")).unwrap();
        std::fs::set_permissions(&ff, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
        (root, ff.to_string_lossy().to_string())
    }

    #[test]
    fn a_picture_is_never_seen_half_written() {
        let (root, ff) = fake_ffmpeg("half-picture", "printf half > \"$out\"; sleep 0.5; printf rest >> \"$out\"");
        let out = root.join("1-0.jpg");
        let (ff2, out2) = (ff.clone(), out.clone());
        let maker = std::thread::spawn(move || make_frame(&ff2, "x.mkv", 1.0, &out2));
        let start = Instant::now();
        while start.elapsed() < Duration::from_millis(400) {
            if let Ok(b) = std::fs::read(&out) {
                assert_ne!(b, b"half", "another request could read the picture while ffmpeg was still writing it");
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(maker.join().unwrap());
        assert_eq!(std::fs::read(&out).unwrap(), b"halfrest");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn pictures_are_made_a_few_at_a_time() {
        let (root, ff) = fake_ffmpeg("many-pictures", "d=$(dirname \"$0\")/running; mkdir -p \"$d\"; touch \"$d/$$\"; ls \"$d\" | wc -l >> \"$d.log\"; sleep 0.3; rm \"$d/$$\"; printf x > \"$out\"");
        let makers: Vec<_> = (0..9).map(|n| {
            let (ff, out) = (ff.clone(), root.join(format!("1-{n}.jpg")));
            std::thread::spawn(move || make_frame(&ff, "x.mkv", n as f64, &out))
        }).collect();
        for m in makers { assert!(m.join().unwrap()); }
        let most = std::fs::read_to_string(root.join("running.log")).unwrap().lines().filter_map(|l| l.trim().parse::<usize>().ok()).max().unwrap();
        assert!(most <= PICTURES_AT_ONCE, "{most} ffmpeg processes ran at once");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn no_picture_is_taken_from_a_video_outside_the_libraries() {
        let (root, ff) = fake_ffmpeg("outside-picture", "touch \"$(dirname \"$0\")/ran\"; printf x > \"$out\"");
        let app = App::open(&root.join("data"));
        let user: User = serde_json::from_value(json!({"id": 1, "username": "o", "admin": true, "password_hash": "", "salt": "", "created_at": 0})).unwrap();
        {
            let mut db = app.db.lock().unwrap();
            db.settings.ffmpeg = ff;
            // The movie's library folder is elsewhere (its file is not in any library).
            db.sections.push(serde_json::from_value(json!({"id": 1, "name": "Filme", "kind": "movie", "paths": [root.join("movies").to_string_lossy()], "created_at": 0})).unwrap());
            let mut i: Item = serde_json::from_value(json!({"id": 1, "section_id": 1, "kind": "movie", "title": "x", "sort_title": "x", "added_at": 0, "updated_at": 0})).unwrap();
            i.file = Some(db::MediaFile { path: "/etc/hosts".into(), duration_ms: 1000, ..Default::default() });
            db.items.insert(1, i);
        }
        assert_eq!(art(&app, &user, "1").status_code().0, 404);
        assert!(!root.join("ran").exists(), "ffmpeg read a file outside the libraries for a background");
        let _ = thumb(&app, &user, "1", None);
        assert!(!root.join("ran").exists(), "ffmpeg read a file outside the libraries for a poster");
        let _ = std::fs::remove_dir_all(&root);
    }
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

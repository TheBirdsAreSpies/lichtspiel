//! The watchlist (per person) and the owner's dashboard: what is playing,
//! bandwidth, processor and memory, who is active, and the play history
//! counted per day.

use crate::db::{now, Db, ItemKind, PlaySession, User, SESSION_MIN_MS};
use crate::server::*;
use serde_json::{json, Value};
use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

// ---------------------------------------------------------------- watchlist

/// Movies, shows, seasons and episodes on a person's watchlist, newest first
/// unless `sort` says otherwise (added, title, year, rating, added_at = when
/// the item came into the library).
pub fn watchlist_ids(db: &Db, user: &User, sort: &str, kind: Option<ItemKind>) -> Vec<u64> {
    let prefix = format!("{}:", user.id);
    let mut list: Vec<(u64, u64)> = db
        .views
        .iter()
        .filter(|(k, v)| v.watchlist_at > 0 && k.starts_with(&prefix))
        .filter_map(|(k, v)| Some((k[prefix.len()..].parse::<u64>().ok()?, v.watchlist_at)))
        .filter(|(id, _)| db.items.get(id).is_some_and(|i| db.can_see_item(user, i) && kind.is_none_or(|k| i.kind == k)))
        .collect();
    let it = |id: &u64| &db.items[id];
    match sort {
        "title" => list.sort_by(|a, b| it(&a.0).sort_title.cmp(&it(&b.0).sort_title)),
        "year" | "year:desc" => list.sort_by(|a, b| it(&b.0).year.cmp(&it(&a.0).year)),
        "rating" | "rating:desc" => list.sort_by(|a, b| it(&b.0).rating.unwrap_or(0.0).total_cmp(&it(&a.0).rating.unwrap_or(0.0))),
        "added_at" | "added_at:desc" => list.sort_by(|a, b| it(&b.0).added_at.cmp(&it(&a.0).added_at)),
        "oldest" => list.sort_by_key(|x| (x.1, x.0)),
        _ => list.sort_by(|a, b| b.1.cmp(&a.1).then(b.0.cmp(&a.0))),
    }
    list.into_iter().map(|x| x.0).collect()
}

/// Puts items on the watchlist or takes them off; returns how many changed.
pub fn set_watchlist(db: &mut Db, user: &User, ids: &[u64], on: bool) -> Result<usize, Resp> {
    for id in ids {
        let Some(it) = db.items.get(id) else { return Err(fail(404, "Not found")) };
        if !db.can_see_item(user, it) {
            return Err(fail(403, "You don't have access to this library"));
        }
    }
    let mut changed = 0;
    let t = now();
    for id in ids {
        let was = db.view(user.id, *id).watchlist_at;
        if on == (was > 0) {
            continue;
        }
        db.view_mut(user.id, *id).watchlist_at = if on { t } else { 0 };
        changed += 1;
    }
    Ok(changed)
}

// ---------------------------------------------------------------- dashboard

/// Seconds since the last request after which a person no longer counts as active.
pub const ACTIVE_SECONDS: u64 = 300;

fn sample_json(s: &crate::stats::Sample) -> Value {
    json!({"at": s.at, "cpu": s.cpu, "cpu_server": s.cpu_server, "mem_used": s.mem_used, "mem_total": s.mem_total,
        "mem_server": s.mem_server, "out_bps": s.out_bps, "out_remote_bps": s.out_remote_bps, "streams": s.streams, "transcodes": s.transcodes})
}

/// Everything playing right now, one entry per person and video: the player's
/// own reports (position, paused) joined with the stream the server sends.
fn now_playing(app: &Arc<App>, db: &Db) -> Vec<Value> {
    struct Entry {
        user: u64,
        item: u64,
        offset: u64,
        state: String,
        method: String,
        stream: &'static str,
        client: String,
        height: u32,
        audio: Option<u32>,
        viewer: Viewer,
        idle: u64,
        /// Seconds since the stream (not the player) last did something.
        stream_idle: Option<u64>,
    }
    let mut by: BTreeMap<(u64, u64), Entry> = BTreeMap::new();
    let mut put = |e: Entry| {
        let k = (e.user, e.item);
        match by.get_mut(&k) {
            None => {
                by.insert(k, e);
            }
            Some(o) => {
                // The player knows where it is; the stream knows how it is sent.
                if e.stream == "player" {
                    o.offset = e.offset;
                    o.state = e.state;
                    o.client = e.client;
                    o.idle = o.idle.min(e.idle);
                    if o.viewer.device.is_empty() { o.viewer = e.viewer; }
                } else {
                    o.method = e.method;
                    o.stream = e.stream;
                    o.height = o.height.max(e.height);
                    o.audio = o.audio.or(e.audio);
                    o.stream_idle = Some(o.stream_idle.unwrap_or(u64::MAX).min(e.idle));
                    // A stream's device and address are the ones that receive the picture.
                    if !e.viewer.device.is_empty() || !e.viewer.ip.is_empty() { o.viewer = e.viewer; }
                }
            }
        }
    };
    for p in app.progressive.lock().unwrap().values() {
        put(Entry { user: p.user_id, item: p.item_id, offset: p.offset_ms, state: p.state.clone(), method: p.method.clone(), stream: "progressive", client: p.client.clone(), height: p.height, audio: p.audio, viewer: p.viewer.clone(), idle: p.updated.elapsed().as_secs(), stream_idle: Some(p.updated.elapsed().as_secs()) });
    }
    for s in app.hls.lock().unwrap().values() {
        put(Entry { user: s.user_id, item: s.item_id, offset: 0, state: "playing".into(), method: format!("{:?}", s.method).to_lowercase(), stream: "hls", client: String::new(), height: s.height, audio: s.audio, viewer: s.viewer.clone(), idle: s.last_access.elapsed().as_secs(), stream_idle: Some(s.last_access.elapsed().as_secs()) });
    }
    let mut reported: HashMap<(u64, u64), u64> = HashMap::new();
    for p in app.playing.lock().unwrap().values() {
        let idle = p.updated.elapsed().as_secs();
        reported.insert((p.user_id, p.item_id), idle);
        put(Entry { user: p.user_id, item: p.item_id, offset: p.offset_ms, state: p.state.clone(), method: "direct".into(), stream: "player", client: p.client.clone(), height: 0, audio: None, viewer: p.viewer.clone(), idle, stream_idle: None });
    }
    // A converted stream started after the player's last "paused" (it jumped to
    // another point and plays again): it is playing.
    for (k, e) in by.iter_mut() {
        if e.state == "paused" && e.stream == "progressive" && e.stream_idle.zip(reported.get(k)).is_some_and(|(si, pi)| si < *pi) {
            e.state = "playing".into();
        }
    }
    let bps = app.stats.user_bps.lock().unwrap().clone();
    let seen = app.stats.seen.lock().unwrap().clone();
    // Converted pictures are made by the hardware when it works (see transcode::effective_hw).
    let hw = by.values().any(|e| e.method == "transcode") && matches!(crate::transcode::effective_hw(&db.settings).as_str(), "vaapi" | "qsv" | "nvenc");
    by.into_values()
        .map(|e| {
            let it = db.items.get(&e.item);
            let mut o = crate::actions::brief(it, e.item, db);
            let m = it.and_then(|i| i.file.as_ref());
            let who = seen.get(&e.user);
            o["thumb"] = json!(format!("/library/metadata/{}/thumb?v={}", e.item, it.map(|i| i.updated_at).unwrap_or(0)));
            o["year"] = json!(it.and_then(|i| i.year));
            o["duration"] = json!(it.map(|i| i.duration_ms));
            o["user"] = json!(db.user(e.user).map(|u| u.username.clone()));
            o["user_id"] = json!(e.user);
            o["offset"] = json!(e.offset);
            o["state"] = json!(e.state);
            o["method"] = json!(e.method);
            o["stream"] = json!(e.stream);
            let device = if !e.viewer.device.is_empty() { e.viewer.device.clone() } else if e.client.is_empty() || e.client.starts_with("hls ") || e.client == "web" || e.client == "browser" { who.map(|w| w.agent.clone()).unwrap_or_default() } else { e.client.clone() };
            o["client"] = json!(device);
            let ip = if e.viewer.ip.is_empty() { who.map(|w| w.ip.clone()) } else { Some(e.viewer.ip.clone()) };
            let local = if e.viewer.ip.is_empty() { ip.as_deref().and_then(|a| a.parse().ok()).is_some_and(crate::server::ip_is_local) } else { e.viewer.local };
            o["ip"] = json!(ip);
            o["local"] = json!(local);
            o["height"] = json!(if e.height > 0 { Some(e.height) } else { None });
            o["source"] = json!(m.map(|f| json!({"container": f.container, "height": f.video().map(|v| v.height), "video_codec": f.video().map(|v| v.codec.clone()), "bitrate": f.bitrate})));
            // Picture and sound: what the file has and what is sent (as the transcoder does it).
            let transcode = e.method == "transcode";
            let direct = e.method == "direct";
            o["video"] = json!(m.and_then(|f| f.video()).map(|v| json!({
                "height": v.height, "codec": v.codec, "profile": v.profile,
                "hdr": !v.color_transfer.is_empty() && v.color_transfer != "bt709",
                "action": if transcode { "transcode" } else { "copy" },
                "out_codec": if transcode { "h264".to_string() } else { v.codec.clone() },
                "out_height": if transcode && e.height > 0 { e.height.min(v.height.max(1)) } else { v.height },
                "hw": transcode && hw,
            })));
            o["audio"] = json!(m.and_then(|f| {
                let tracks: Vec<_> = f.streams.iter().filter(|s| s.kind == "audio").collect();
                let pick = e.audio.and_then(|i| tracks.iter().position(|s| s.index == i)).or_else(|| tracks.iter().position(|s| s.default)).unwrap_or(0);
                tracks.get(pick).map(|a| json!({
                    "language": a.language, "title": a.title, "codec": a.codec, "profile": a.profile, "channels": a.channels,
                    // Converted and repackaged streams always carry stereo AAC.
                    "action": if direct { "copy" } else { "transcode" },
                    "out_codec": if direct { a.codec.clone() } else { "aac".to_string() },
                    "out_channels": if direct { a.channels } else { crate::transcode::TRANSCODE_AUDIO_CHANNELS },
                }))
            }));
            o["out_bps"] = json!(bps.get(&e.user).copied().unwrap_or(0));
            o["idle"] = json!(e.idle);
            o
        })
        .collect()
}

/// GET /api/dashboard: the owner's view of the server right now and over the last hour.
pub fn dashboard(app: &Arc<App>) -> Resp {
    let db = app.db.lock().unwrap();
    let playing = now_playing(app, &db);
    let samples: Vec<Value> = app.stats.samples.lock().unwrap().iter().map(sample_json).collect();
    let current = app.stats.last().map(|s| sample_json(&s)).unwrap_or(Value::Null);
    let seen = app.stats.seen.lock().unwrap().clone();
    let bps = app.stats.user_bps.lock().unwrap().clone();
    let t = now();
    let mut users: Vec<Value> = db
        .users
        .iter()
        .map(|u| {
            let s = seen.get(&u.id);
            let last_play = db.history.iter().rev().find(|e| e.user_id == u.id).map(|e| e.at);
            json!({"id": u.id, "username": u.username, "admin": u.admin,
                "last_seen": s.map(|s| s.at), "active": s.is_some_and(|s| t.saturating_sub(s.at) <= ACTIVE_SECONDS),
                "ip": s.map(|s| s.ip.clone()), "client": s.map(|s| s.agent.clone()),
                "playing": playing.iter().filter(|p| p["user_id"] == json!(u.id)).count(),
                "out_bps": bps.get(&u.id).copied().unwrap_or(0), "last_play": last_play})
        })
        .collect();
    users.sort_by_key(|u| (!u["active"].as_bool().unwrap_or(false), std::cmp::Reverse(u["last_seen"].as_u64().unwrap_or(0))));
    let sent = app.stats.bytes_out.load(std::sync::atomic::Ordering::Relaxed);
    ok(json!({
        "now": t,
        "started_at": app.started_at,
        "version": VERSION,
        "cores": std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1),
        "sample_seconds": crate::stats::SAMPLE_SECONDS,
        "active_seconds": ACTIVE_SECONDS,
        "bytes_sent": sent,
        "max_transcodes": db.settings.max_transcodes,
        "current": current,
        "samples": samples,
        "playing": playing,
        "users": users,
    }))
}

fn kind_name(db: &Db, id: u64) -> &'static str {
    match db.items.get(&id).map(|i| i.kind) {
        Some(ItemKind::Movie) => "movie",
        Some(ItemKind::Episode) => "episode",
        _ => "other",
    }
}

/// The movie, or for an episode its show, so "most played" groups a show's episodes.
fn top_of(db: &Db, id: u64) -> u64 {
    let mut at = id;
    while let Some(p) = db.items.get(&at).and_then(|i| i.parent_id) {
        at = p;
    }
    at
}

/// GET /api/dashboard/history?days=30&user=: plays over the last days, as
/// a compact list the browser counts per day in its own time zone
/// [[at, user id, kind, minutes], …], plus who and what was played most and
/// the latest plays.
///
/// A play is each time someone played a movie or episode for at least a minute,
/// with the time it really ran (`open`: the ones still going). Marked watched
/// by hand (or imported) counts as a play of the whole length; viewings from
/// before plays were recorded count as before.
pub fn history(db: &Db, open: &[PlaySession], days: u64, only_user: Option<u64>) -> Resp {
    struct Play {
        at: u64,
        user: u64,
        item: u64,
        minutes: f64,
        manual: bool,
        live: bool,
    }
    let days = days.clamp(1, 366);
    let from = now().saturating_sub(days * 86400);
    let length = |id: u64| db.items.get(&id).map(|i| i.duration_ms as f64 / 60000.0).unwrap_or(0.0);
    let since = db.sessions.first().map(|s| s.at).into_iter().chain(open.iter().map(|s| s.at)).min().unwrap_or(u64::MAX);
    let wanted = |at: u64, user: u64| at >= from && only_user.is_none_or(|u| user == u);
    let mut events: Vec<Play> = vec![];
    for (s, live) in db.sessions.iter().map(|s| (s, false)).chain(open.iter().filter(|s| s.watched_ms >= SESSION_MIN_MS).map(|s| (s, true))) {
        if wanted(s.at, s.user_id) {
            events.push(Play { at: s.at, user: s.user_id, item: s.item_id, minutes: s.watched_ms as f64 / 60000.0, manual: false, live });
        }
    }
    for e in &db.history {
        // Watched to the end while plays were recorded: that play is counted already.
        if (e.manual || e.at < since) && wanted(e.at, e.user_id) {
            events.push(Play { at: e.at, user: e.user_id, item: e.item_id, minutes: length(e.item_id), manual: e.manual, live: false });
        }
    }
    events.sort_by_key(|e| e.at);
    let round = |m: f64| (m * 10.0).round() / 10.0;
    let plays: Vec<Value> = events.iter().map(|e| json!([e.at, e.user, kind_name(db, e.item), round(e.minutes)])).collect();
    let mut by_user: HashMap<u64, (u64, f64)> = HashMap::new();
    let mut by_title: HashMap<u64, (u64, f64)> = HashMap::new();
    for e in &events {
        let u = by_user.entry(e.user).or_default();
        u.0 += 1;
        u.1 += e.minutes;
        let t = by_title.entry(top_of(db, e.item)).or_default();
        t.0 += 1;
        t.1 += e.minutes;
    }
    let mut users: Vec<(u64, u64, f64)> = by_user.into_iter().map(|(id, (n, m))| (id, n, m)).collect();
    users.sort_by(|a, b| b.1.cmp(&a.1).then(b.2.total_cmp(&a.2)));
    let users: Vec<Value> = users
        .into_iter()
        .map(|(id, n, m)| json!({"id": id, "username": db.user(id).map(|u| u.username.clone()), "plays": n, "minutes": round(m)}))
        .collect();
    let mut titles: Vec<(u64, u64, f64)> = by_title.into_iter().map(|(id, (n, m))| (id, n, m)).collect();
    titles.sort_by(|a, b| b.1.cmp(&a.1).then(b.2.total_cmp(&a.2)));
    let titles: Vec<Value> = titles
        .into_iter()
        .take(10)
        .map(|(id, n, m)| {
            let it = db.items.get(&id);
            json!({"id": id, "title": it.map(|i| i.title.clone()), "type": it.map(|i| i.kind.as_str()), "year": it.and_then(|i| i.year),
                "thumb": format!("/library/metadata/{}/thumb?v={}", id, it.map(|i| i.updated_at).unwrap_or(0)), "plays": n, "minutes": round(m)})
        })
        .collect();
    let recent: Vec<Value> = events
        .iter()
        .rev()
        .take(25)
        .map(|e| json!({"at": e.at, "manual": e.manual, "live": e.live, "minutes": round(e.minutes),
            "user": db.user(e.user).map(|u| u.username.clone()), "item": crate::actions::brief(db.items.get(&e.item), e.item, db)}))
        .collect();
    let total_minutes = events.iter().fold(0.0, |n, e| n + e.minutes);
    ok(json!({"days": days, "from": from, "size": plays.len(), "minutes": round(total_minutes),
        "plays": plays, "users": users, "titles": titles, "recent": recent}))
}

/// GET /api/dashboard/stats?range=realtime|12h|24h|7d|30d|90d|365d|all: bandwidth
/// (with the part sent outside the local network), processor, memory and streams
/// on an even grid; a time without values is only {"at"}.
pub fn stats_range(app: &Arc<App>, range: &str) -> Resp {
    let range = match range { "12h" | "24h" | "7d" | "30d" | "90d" | "365d" | "all" => range, _ => "realtime" };
    let (step, values) = app.stats.range(range, now());
    let samples: Vec<Value> = values.iter().map(|(at, s)| s.as_ref().map(sample_json).unwrap_or_else(|| json!({"at": at}))).collect();
    ok(json!({"range": range, "step": step, "samples": samples}))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{Item, ItemKind};

    fn item(id: u64, kind: ItemKind, parent: Option<u64>) -> Item {
        let mut it: Item = serde_json::from_value(json!({"id": id, "section_id": 1, "kind": kind.as_str(), "title": format!("t{id}"), "added_at": 1, "updated_at": 1})).unwrap();
        it.parent_id = parent;
        it
    }

    fn setup() -> (Db, User) {
        let mut db = Db::default();
        for it in [item(10, ItemKind::Movie, None), item(20, ItemKind::Show, None), item(21, ItemKind::Season, Some(20)), item(22, ItemKind::Episode, Some(21)), item(23, ItemKind::Episode, Some(21))] {
            db.items.insert(it.id, it);
        }
        let user: User = serde_json::from_value(json!({"id": 1, "username": "a", "password_hash": "", "salt": "", "admin": true, "created_at": 0})).unwrap();
        db.users.push(user.clone());
        (db, user)
    }

    #[test]
    fn plays_count_with_the_time_really_watched() {
        let (mut db, _) = setup();
        db.items.get_mut(&10).unwrap().duration_ms = 90 * 60000;
        let t = now();
        // Before plays were recorded: a viewing counts with the whole length.
        db.history.push(crate::db::PlayEvent { user_id: 1, item_id: 10, at: t - 5000, manual: false });
        assert!(db.record_session(PlaySession { user_id: 1, item_id: 10, at: t - 4000, watched_ms: 20 * 60000 }));
        // Too short: not counted.
        assert!(!db.record_session(PlaySession { user_id: 1, item_id: 22, at: t - 3000, watched_ms: 30_000 }));
        // Watched to the end during a recorded play: counted once (by the play).
        db.history.push(crate::db::PlayEvent { user_id: 1, item_id: 10, at: t - 3500, manual: false });
        // Marked watched by hand: counts.
        db.history.push(crate::db::PlayEvent { user_id: 1, item_id: 23, at: t - 2000, manual: true });
        let open = [PlaySession { user_id: 1, item_id: 22, at: t - 600, watched_ms: 5 * 60000 }];
        let r = history(&db, &open, 30, None);
        let v: Value = serde_json::from_slice(&{ let mut b = vec![]; std::io::Read::read_to_end(&mut r.into_reader(), &mut b).unwrap(); b }).unwrap();
        assert_eq!(v["size"], 4, "{v}");
        assert_eq!(v["minutes"].as_f64().unwrap(), 90.0 + 20.0 + 0.0 + 5.0);
        assert_eq!(v["recent"][0]["live"], true);
        assert_eq!(v["recent"][0]["minutes"].as_f64().unwrap(), 5.0);
    }

    #[test]
    fn watching_takes_things_off_the_watchlist() {
        let (mut db, user) = setup();
        assert_eq!(set_watchlist(&mut db, &user, &[10, 20], true).ok(), Some(2));
        assert_eq!(set_watchlist(&mut db, &user, &[10], true).ok(), Some(0));
        assert_eq!(watchlist_ids(&db, &user, "", None).len(), 2);
        db.view_mut(1, 10).view_count = 1;
        db.record_play(1, 10, false);
        // The show stays until every episode is watched.
        db.view_mut(1, 22).view_count = 1;
        db.record_play(1, 22, false);
        assert_eq!(watchlist_ids(&db, &user, "", None), vec![20]);
        db.view_mut(1, 23).view_count = 1;
        db.record_play(1, 23, false);
        assert!(watchlist_ids(&db, &user, "", None).is_empty());
    }
}

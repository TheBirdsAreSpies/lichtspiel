//! Downloads: people pick movies, episodes, seasons or shows and a quality;
//! the original file is handed out as it is, a converted copy is made in the
//! background (one at a time) and kept in <data>/downloads until it is saved,
//! removed, or `download_keep_days` have passed. The Downloads page lists
//! them with progress and a save button each.

use crate::db::{self, now, Db, Download, Item, User};
use crate::handlers::{decision_fail, serve_file_as};
use crate::server::*;
use crate::transcode;
use serde_json::{json, Value};
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

/// May this person download? Admins always; others unless switched off for them.
pub fn may_download(user: &User) -> bool {
    user.admin || user.allow_download.unwrap_or(true)
}

/// Where a converted copy is kept: <data>/downloads/<download id>.mp4.
pub fn copy_path(app: &App, id: u64) -> PathBuf {
    app.data_dir.join("downloads").join(format!("{id}.mp4"))
}

/// The conversion running now: (download id, percent done) and its ffmpeg.
struct Running {
    id: u64,
    pct: u32,
    child: Option<Child>,
    cancelled: bool,
}
static RUNNING: Mutex<Option<Running>> = Mutex::new(None);
/// Wakes the worker when something was queued.
static WAKE: (Mutex<bool>, Condvar) = (Mutex::new(false), Condvar::new());

fn wake() {
    *WAKE.0.lock().unwrap() = true;
    WAKE.1.notify_one();
}

/// Stops the conversion of a download if it is the one running.
fn cancel(id: u64) {
    if let Some(r) = RUNNING.lock().unwrap().as_mut().filter(|r| r.id == id) {
        r.cancelled = true;
        if let Some(c) = r.child.as_mut() { let _ = c.kill(); }
    }
}

/// Live progress of a download that is being converted.
fn progress(id: u64) -> Option<u32> {
    RUNNING.lock().unwrap().as_ref().filter(|r| r.id == id).map(|r| r.pct)
}

// ---------------------------------------------------------------- names

/// "Movie (2019) - 720p.mp4", "Show - S01E02 - Title.mkv".
pub fn file_name(db: &Db, it: &Item, quality: &str, ext: &str) -> String {
    let season = it.parent_id.and_then(|p| db.items.get(&p));
    let show = season.and_then(|s| s.parent_id).and_then(|g| db.items.get(&g));
    let mut name = match (it.kind, show) {
        (db::ItemKind::Episode, Some(show)) => format!("{} - S{:02}E{:02} - {}", show.title,
            season.and_then(|s| s.index).unwrap_or(0), it.index.unwrap_or(0), it.title),
        _ => match it.year { Some(y) => format!("{} ({y})", it.title), None => it.title.clone() },
    };
    if quality != "original" {
        name.push_str(" - ");
        name.push_str(quality.split('-').next().unwrap_or(quality));
    }
    // Characters Windows and macOS don't allow in file names.
    let clean: String = name.chars().map(|c| if "<>:\"/\\|?*".contains(c) || c.is_control() { '_' } else { c }).collect();
    format!("{}.{ext}", clean.trim().trim_end_matches('.'))
}

// ---------------------------------------------------------------- the list

fn json_of(db: &Db, d: &Download, keep_days: u64) -> Value {
    let it = db.items.get(&d.item_id);
    let mut v = crate::actions::brief(it, d.item_id, db);
    // Episodes show their show's poster.
    let pic = it.and_then(|i| if i.kind == db::ItemKind::Episode { i.parent_id.and_then(|p| db.items.get(&p)).and_then(|s| s.parent_id) } else { None }).unwrap_or(d.item_id);
    let pic_v = db.items.get(&pic).map(|i| i.updated_at).unwrap_or(0);
    let parts = it.map(|i| i.all_files().len()).unwrap_or(0);
    let expires = if !d.original && d.state == "ready" && keep_days > 0 { d.finished_at + keep_days * 86400 } else { 0 };
    json!({"id": d.id, "item": v.take(), "thumb": format!("/library/metadata/{pic}/thumb?v={pic_v}"),
        "duration": it.map(|i| i.duration_ms).unwrap_or(0), "quality": d.quality, "state": d.state,
        "progress": if d.state == "ready" { 100 } else { progress(d.id).unwrap_or(0) },
        "original": d.original, "size": d.size, "error": d.error, "created_at": d.created_at, "finished_at": d.finished_at,
        "expires_at": expires, "parts": if d.original { parts.max(1) } else { 1 },
        "user": db.user(d.user_id).map(|u| u.username.clone()), "user_id": d.user_id,
        "url": format!("/api/downloads/{}/file", d.id)})
}

/// Your downloads, newest first (an admin with ?all=1: everyone's).
pub fn list(app: &Arc<App>, ctx: &Ctx, user: &User) -> Resp {
    let db = app.db.lock().unwrap();
    let all = user.admin && ctx.param("all") == Some("1");
    let keep = db.settings.download_keep_days;
    let list: Vec<Value> = db.downloads.iter().rev().filter(|d| all || d.user_id == user.id).map(|d| json_of(&db, d, keep)).collect();
    let count = |s: &str| list.iter().filter(|d| d["state"] == s).count();
    ok(json!({"size": list.len(), "allowed": may_download(user), "may_convert": user.admin || user.allow_transcode != Some(false),
        "keep_days": keep, "queued": count("queued"), "converting": count("converting"), "ready": count("ready"), "failed": count("failed"),
        "qualities": transcode::QUALITIES.iter().map(|q| json!({"name": q.0, "height": q.1, "kbps": q.2})).collect::<Vec<_>>(),
        "downloads": list}))
}

/// Adds movies and episodes (a season or show: every episode) in a quality.
/// The original file, or one that already fits, is ready at once; the others
/// are converted one after the other.
pub fn add(app: &Arc<App>, user: &User, ids: &[u64], quality: &str) -> Resp {
    if !may_download(user) { return decision_fail(403, 2011); }
    if !transcode::QUALITIES.iter().any(|q| q.0 == quality) { return fail(400, "Unknown quality"); }
    let mut db = app.db.lock().unwrap();
    let mut leaves = vec![];
    for &id in ids {
        let Some(it) = db.items.get(&id) else { return fail(404, "Not found") };
        if !db.can_see_section(user, it.section_id) { return fail(404, "Not found"); }
        for l in db.leaves(id) {
            let ok = db.items.get(&l).is_some_and(|x| x.file.is_some() && !x.extra.contains_key(crate::scanner::MISSING));
            if ok && !leaves.contains(&l) { leaves.push(l); }
        }
    }
    if leaves.is_empty() { return fail(400, "There is no video file to download"); }
    let may_convert = user.admin || user.allow_transcode != Some(false);
    let (mut added, mut ready, mut queued, mut already, mut refused) = (0, 0, 0, 0, 0);
    for l in leaves {
        if let Some(d) = db.downloads.iter_mut().find(|d| d.user_id == user.id && d.item_id == l && d.quality == quality) {
            // Asked again after it failed: one more try.
            if d.state == "failed" { d.state = "queued".into(); d.error.clear(); queued += 1; } else { already += 1; }
            continue;
        }
        let Some(it) = db.items.get(&l) else { continue };
        let files = it.all_files();
        let plan = if files.len() > 1 && quality != "original" {
            // A movie in parts: joined while converting.
            let (h, kb) = transcode::quality(quality);
            Some((h, kb))
        } else {
            files.first().and_then(|f| transcode::download_plan(f, quality))
        };
        if plan.is_some() && !may_convert { refused += 1; continue; }
        let size: u64 = files.iter().map(|f| f.size).sum();
        let id = db.downloads.iter().map(|d| d.id).max().unwrap_or(0) + 1;
        let t = now();
        let d = match plan {
            None => { ready += 1; Download { id, user_id: user.id, item_id: l, quality: quality.into(), state: "ready".into(), original: true, size, error: String::new(), created_at: t, finished_at: t } }
            Some(_) => { queued += 1; Download { id, user_id: user.id, item_id: l, quality: quality.into(), state: "queued".into(), original: false, size: 0, error: String::new(), created_at: t, finished_at: 0 } }
        };
        db.downloads.push(d);
        added += 1;
    }
    drop(db);
    app.save();
    if queued > 0 { wake(); }
    if added == 0 && queued == 0 && refused > 0 {
        return fail(403, "You may not have videos converted; pick Original quality");
    }
    js(201, json!({"ok": true, "added": added, "ready": ready, "queued": queued, "already": already, "refused": refused}))
}

fn find_own<'a>(db: &'a Db, user: &User, id: u64) -> Option<&'a Download> {
    db.downloads.iter().find(|d| d.id == id && (d.user_id == user.id || user.admin))
}

/// Takes a download off the list (stopping its conversion) and deletes its copy.
pub fn remove(app: &Arc<App>, user: &User, id: u64) -> Resp {
    let mut db = app.db.lock().unwrap();
    if find_own(&db, user, id).is_none() { return fail(404, "No such download"); }
    db.downloads.retain(|d| d.id != id);
    drop(db);
    cancel(id);
    let _ = std::fs::remove_file(copy_path(app, id));
    app.save();
    ok(json!({"ok": true}))
}

/// Several at once: ?state=ready|failed|all (your own; an admin's ?all=1 everyone's).
pub fn remove_many(app: &Arc<App>, ctx: &Ctx, user: &User) -> Resp {
    let state = ctx.param("state").unwrap_or("finished").to_string();
    let everyone = user.admin && ctx.param("all") == Some("1");
    let mut db = app.db.lock().unwrap();
    let gone: Vec<u64> = db.downloads.iter()
        .filter(|d| everyone || d.user_id == user.id)
        .filter(|d| match state.as_str() { "all" => true, "ready" => d.state == "ready", "failed" => d.state == "failed", _ => d.state == "ready" || d.state == "failed" })
        .map(|d| d.id).collect();
    db.downloads.retain(|d| !gone.contains(&d.id));
    drop(db);
    for id in &gone {
        cancel(*id);
        let _ = std::fs::remove_file(copy_path(app, *id));
    }
    app.save();
    ok(json!({"ok": true, "removed": gone.len()}))
}

/// A failed download is tried again.
pub fn retry(app: &Arc<App>, user: &User, id: u64) -> Resp {
    let mut db = app.db.lock().unwrap();
    if find_own(&db, user, id).is_none() { return fail(404, "No such download"); }
    let Some(d) = db.downloads.iter_mut().find(|d| d.id == id) else { return fail(404, "No such download") };
    if d.state != "failed" { return fail(409, "Only a failed download can be tried again"); }
    d.state = "queued".into();
    d.error.clear();
    drop(db);
    app.save();
    wake();
    ok(json!({"ok": true}))
}

/// The file of a ready download (?part=n for the parts of a movie in parts).
pub fn file(app: &Arc<App>, ctx: &Ctx, user: &User, id: u64) -> Resp {
    if !may_download(user) { return decision_fail(403, 2011); }
    let db = app.db.lock().unwrap();
    let Some(d) = find_own(&db, user, id) else { return fail(404, "No such download") };
    if d.state != "ready" { return fail(409, "This download is not ready yet"); }
    let Some(it) = db.items.get(&d.item_id) else { return fail(404, "The movie or episode is no longer in the library") };
    if !db.can_see_section(user, it.section_id) { return fail(403, "You don't have access to this library"); }
    if d.original {
        let n = ctx.num("part").unwrap_or(0) as usize;
        let Some(f) = it.part(n) else { return fail(404, "No such part") };
        let ext = Path::new(&f.path).extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_else(|| f.container.clone());
        let mut name = file_name(&db, it, "original", &ext);
        if it.all_files().len() > 1 { name = name.replacen(&format!(".{ext}"), &format!(" - part{}.{ext}", n + 1), 1); }
        let (path, ctype) = (PathBuf::from(&f.path), transcode::content_type(&f.container));
        drop(db);
        return serve_file_as(&path, ctype, ctx.range.as_deref(), Some(&name));
    }
    let name = file_name(&db, it, &d.quality, "mp4");
    drop(db);
    serve_file_as(&copy_path(app, id), "video/mp4", ctx.range.as_deref(), Some(&name))
}

// ---------------------------------------------------------------- the worker

/// Starts the converter. Conversions cut short by a stop start over.
pub fn start(app: &Arc<App>) {
    let dir = app.data_dir.join("downloads");
    if let Ok(rd) = std::fs::read_dir(&dir) {
        for e in rd.flatten() {
            let n = e.file_name().to_string_lossy().to_string();
            if n.ends_with(".part.mp4") || n.ends_with(".txt") { let _ = std::fs::remove_file(e.path()); }
        }
    }
    {
        let mut db = app.db.lock().unwrap();
        for d in db.downloads.iter_mut().filter(|d| d.state == "converting") { d.state = "queued".into(); }
    }
    let app = app.clone();
    std::thread::spawn(move || loop {
        tidy(&app);
        let next = {
            let db = app.db.lock().unwrap();
            db.downloads.iter().find(|d| d.state == "queued").map(|d| d.id)
        };
        match next {
            Some(id) => convert(&app, id),
            None => {
                let mut woken = WAKE.0.lock().unwrap();
                if !*woken { woken = WAKE.1.wait_timeout(woken, Duration::from_secs(600)).unwrap().0; }
                *woken = false;
            }
        }
    });
}

/// Drops downloads whose movie, episode or person is gone, and converted
/// copies older than `download_keep_days`.
fn tidy(app: &Arc<App>) {
    let mut db = app.db.lock().unwrap();
    let keep = db.settings.download_keep_days;
    let t = now();
    let gone: Vec<u64> = db.downloads.iter().filter(|d| {
        !db.items.contains_key(&d.item_id) || db.user(d.user_id).is_none()
            || (keep > 0 && !d.original && d.state == "ready" && d.finished_at + keep * 86400 < t)
    }).map(|d| d.id).collect();
    if gone.is_empty() { return; }
    db.downloads.retain(|d| !gone.contains(&d.id));
    drop(db);
    for id in gone {
        cancel(id);
        let _ = std::fs::remove_file(copy_path(app, id));
    }
    app.save();
}

/// One conversion, with progress from ffmpeg's -progress lines.
fn convert(app: &Arc<App>, id: u64) {
    // False when the download was removed in the meantime.
    let set_state = |state: &str, size: u64, error: &str| -> bool {
        let found = match app.db.lock().unwrap().downloads.iter_mut().find(|d| d.id == id) {
            Some(d) => {
                d.state = state.into();
                d.size = size;
                d.error = error.into();
                if state == "ready" || state == "failed" { d.finished_at = now(); }
                true
            }
            None => false,
        };
        app.save();
        found
    };
    let (files, settings, quality) = {
        let mut db = app.db.lock().unwrap();
        let Some(d) = db.downloads.iter().find(|d| d.id == id).cloned() else { return };
        let files: Vec<db::MediaFile> = db.items.get(&d.item_id).map(|i| i.all_files().into_iter().cloned().collect()).unwrap_or_default();
        if let Some(d) = db.downloads.iter_mut().find(|d| d.id == id) { d.state = "converting".into(); }
        (files, db.settings.clone(), d.quality)
    };
    let Some(first) = files.first() else { set_state("failed", 0, "The movie or episode has no video file"); return };
    let dir = app.data_dir.join("downloads");
    if let Err(e) = std::fs::create_dir_all(&dir) { set_state("failed", 0, &format!("{}: {e}", dir.display())); return; }
    let out = copy_path(app, id);
    let tmp = out.with_extension("part.mp4");
    let (h, kb) = transcode::download_plan(first, &quality).unwrap_or_else(|| transcode::quality(&quality));
    let duration_ms: u64 = files.iter().map(|f| f.duration_ms).sum();
    // Several parts: ffmpeg's concat list joins them into one file.
    let list = dir.join(format!("{id}.txt"));
    let input: Vec<String> = if files.len() > 1 {
        let body: String = files.iter().map(|f| format!("file '{}'\n", f.path.replace('\'', "'\\''"))).collect();
        if let Err(e) = std::fs::write(&list, body) { set_state("failed", 0, &e.to_string()); return; }
        ["-f", "concat", "-safe", "0", "-i"].iter().map(|s| s.to_string()).chain([list.to_string_lossy().to_string()]).collect()
    } else {
        vec!["-i".into(), first.path.clone()]
    };
    let args = transcode::file_args(&settings, first, &input, h, kb, &tmp.to_string_lossy());
    *RUNNING.lock().unwrap() = Some(Running { id, pct: 0, child: None, cancelled: false });
    app.save();
    let child = Command::new(&settings.ffmpeg).args(&args).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn();
    let result = match child {
        Err(e) => Err(format!("ffmpeg could not be started: {e}")),
        Ok(mut c) => {
            let so = c.stdout.take();
            let se = c.stderr.take();
            // stderr is read on the side so a chatty ffmpeg never blocks.
            let err = std::thread::spawn(move || { let mut s = String::new(); if let Some(mut se) = se { let _ = std::io::Read::read_to_string(&mut se, &mut s); } s });
            let mut cancelled = false;
            if let Some(r) = RUNNING.lock().unwrap().as_mut() { cancelled = r.cancelled; r.child = Some(c); }
            if cancelled { if let Some(c) = RUNNING.lock().unwrap().as_mut().and_then(|r| r.child.as_mut()) { let _ = c.kill(); } }
            if let Some(so) = so {
                for line in BufReader::new(so).lines().map_while(Result::ok) {
                    if let Some(us) = line.strip_prefix("out_time_us=").and_then(|v| v.trim().parse::<u64>().ok()) {
                        let pct = if duration_ms > 0 { (us / 1000 * 100 / duration_ms).min(99) as u32 } else { 0 };
                        if let Some(r) = RUNNING.lock().unwrap().as_mut() { r.pct = pct; }
                    }
                }
            }
            let (child, cancelled) = RUNNING.lock().unwrap().as_mut().map(|r| (r.child.take(), r.cancelled)).unwrap_or((None, true));
            let status = child.map(|mut c| c.wait().map(|s| s.success()).unwrap_or(false)).unwrap_or(false);
            let err = err.join().unwrap_or_default();
            if cancelled { Err(String::new()) } else if status { Ok(()) } else {
                eprintln!("download {id}: converting failed: {}", err.trim());
                Err(err.lines().rev().find(|l| !l.trim().is_empty()).unwrap_or("The conversion failed").trim().chars().take(300).collect())
            }
        }
    };
    *RUNNING.lock().unwrap() = None;
    let _ = std::fs::remove_file(&list);
    match result {
        Ok(()) if std::fs::rename(&tmp, &out).is_ok() => {
            let size = std::fs::metadata(&out).map(|m| m.len()).unwrap_or(0);
            if !set_state("ready", size, "") { let _ = std::fs::remove_file(&out); return; }
            let db = app.db.lock().unwrap();
            let who = db.downloads.iter().find(|d| d.id == id).and_then(|d| db.user(d.user_id)).map(|u| u.username.clone());
            drop(db);
            app.fire("download.ready", json!({"id": id, "user": who, "quality": quality, "size": size}));
        }
        Ok(()) => { let _ = std::fs::remove_file(&tmp); set_state("failed", 0, "The converted file could not be saved"); }
        // Removed while converting: nothing to keep.
        Err(e) if e.is_empty() => { let _ = std::fs::remove_file(&tmp); }
        Err(e) => { let _ = std::fs::remove_file(&tmp); set_state("failed", 0, &e); }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn names() {
        let mut db: Db = serde_json::from_value(json!({"version": 1, "machine_id": "x", "next_id": 9, "settings": db::Settings::default(),
            "sections": [], "items": {}, "users": [], "invites": [], "tokens": [], "playlists": [], "views": {}, "plugins": {}})).unwrap();
        let item = |v: Value| -> Item { serde_json::from_value(v).unwrap() };
        db.items.insert(1, item(json!({"id": 1, "section_id": 1, "kind": "show", "title": "Dark", "added_at": 0, "updated_at": 0})));
        db.items.insert(2, item(json!({"id": 2, "section_id": 1, "kind": "season", "title": "Season 1", "index": 1, "parent_id": 1, "added_at": 0, "updated_at": 0})));
        let ep = item(json!({"id": 3, "section_id": 1, "kind": "episode", "title": "Secrets?", "index": 4, "parent_id": 2, "added_at": 0, "updated_at": 0}));
        assert_eq!(file_name(&db, &ep, "720p-4", "mp4"), "Dark - S01E04 - Secrets_ - 720p.mp4");
        let mv = item(json!({"id": 4, "section_id": 1, "kind": "movie", "title": "Amélie", "year": 2001, "added_at": 0, "updated_at": 0}));
        assert_eq!(file_name(&db, &mv, "original", "mkv"), "Amélie (2001).mkv");
        assert_eq!(crate::handlers::attachment("Amélie (2001).mkv"), "attachment; filename=\"Am_lie (2001).mkv\"; filename*=UTF-8''Am%C3%A9lie%20%282001%29.mkv");
    }
}

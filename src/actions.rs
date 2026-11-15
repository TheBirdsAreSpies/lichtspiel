//! What the "…" menu on a poster does beyond the basics: play history,
//! problem reports, analysing files again, optimized versions,
//! "Watch together" rooms, emptying a library's trash and deleting files.

use std::sync::atomic::Ordering as AtomicOrdering;
use crate::db::{self, now, Db, Item, User};
use crate::server::*;
use crate::transcode;
use serde_json::{json, Value};
use std::collections::{BTreeMap, HashMap};
use std::io::{BufRead, BufReader};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Instant;

/// The item and every episode under it (a show, a season), or just the item.
fn with_leaves(db: &Db, id: u64) -> Vec<u64> {
    let mut ids = vec![id];
    ids.extend(db.leaves(id).into_iter().filter(|l| *l != id));
    ids
}

/// A short description of an item for lists (history, reports).
pub fn brief(it: Option<&Item>, id: u64, db: &Db) -> Value {
    let Some(it) = it else { return json!({"id": id, "title": null}) };
    let parent = it.parent_id.and_then(|p| db.items.get(&p));
    let show = parent.and_then(|p| p.parent_id).and_then(|g| db.items.get(&g));
    json!({"id": it.id, "title": it.title, "type": it.kind.as_str(), "index": it.index,
        "parentIndex": parent.and_then(|p| p.index), "grandparentTitle": show.map(|s| s.title.clone())})
}

// ---------------------------------------------------------------- play history

/// Viewings of an item (and its episodes), newest first. The owner sees
/// everyone's, other users only their own.
pub fn history(app: &Arc<App>, user: &User, iid: u64) -> Resp {
    let db = app.db.lock().unwrap();
    let Some(it) = db.items.get(&iid) else { return fail(404, "Not found") };
    if !db.can_see_item(user, it) { return fail(403, "You don't have access to this library"); }
    let ids: std::collections::HashSet<u64> = with_leaves(&db, iid).into_iter().collect();
    let list: Vec<Value> = db.history.iter().rev()
        .filter(|e| ids.contains(&e.item_id) && (user.admin || e.user_id == user.id))
        .take(500)
        .map(|e| json!({"at": e.at, "manual": e.manual, "user": db.user(e.user_id).map(|u| u.username.clone()),
            "item": brief(db.items.get(&e.item_id), e.item_id, &db)}))
        .collect();
    ok(json!({"size": list.len(), "history": list}))
}

// ---------------------------------------------------------------- problem reports

pub const PROBLEM_KINDS: &[&str] = &["playback", "video", "audio", "subtitles", "metadata", "other"];

pub fn report_problem(app: &Arc<App>, ctx: &Ctx, user: &User, iid: u64) -> Resp {
    let kind = ctx.body_or_param("kind").unwrap_or_else(|| "other".into());
    if !PROBLEM_KINDS.contains(&kind.as_str()) { return fail(400, "Unknown kind of problem"); }
    let note: String = ctx.body_or_param("note").unwrap_or_default().trim().chars().take(2000).collect();
    let mut db = app.db.lock().unwrap();
    let Some(it) = db.items.get(&iid) else { return fail(404, "Not found") };
    if !db.can_see_item(user, it) { return fail(403, "You don't have access to this library"); }
    let title = it.title.clone();
    let id = db.reports.iter().map(|r| r.id).max().unwrap_or(0) + 1;
    db.reports.push(db::ProblemReport { id, user_id: user.id, item_id: iid, kind: kind.clone(), note: note.clone(), at: now(), resolved: false });
    drop(db);
    app.save();
    app.fire("item.problem", json!({"id": iid, "title": title, "user": user.username, "kind": kind, "note": note}));
    js(201, json!({"ok": true, "id": id}))
}

pub fn reports(app: &Arc<App>, ctx: &Ctx) -> Resp {
    let db = app.db.lock().unwrap();
    let open_only = ctx.param("open") == Some("1");
    let list: Vec<Value> = db.reports.iter().rev().filter(|r| !open_only || !r.resolved).map(|r| json!({
        "id": r.id, "kind": r.kind, "note": r.note, "at": r.at, "resolved": r.resolved,
        "user": db.user(r.user_id).map(|u| u.username.clone()), "item": brief(db.items.get(&r.item_id), r.item_id, &db)})).collect();
    let open = db.reports.iter().filter(|r| !r.resolved).count();
    ok(json!({"size": list.len(), "open": open, "reports": list}))
}

pub fn update_report(app: &Arc<App>, ctx: &Ctx, rid: u64, delete: bool) -> Resp {
    let mut db = app.db.lock().unwrap();
    let Some(pos) = db.reports.iter().position(|r| r.id == rid) else { return fail(404, "No such report") };
    if delete {
        db.reports.remove(pos);
    } else if let Some(b) = ctx.body.get("resolved").and_then(|v| v.as_bool()) {
        db.reports[pos].resolved = b;
    }
    drop(db);
    app.save();
    ok(json!({"ok": true}))
}

// ---------------------------------------------------------------- analyze

/// Reads every file of the item (and its episodes) again with ffprobe, on a
/// background thread shown as a job. Returns how many videos it will read.
pub fn analyze(app: &Arc<App>, iid: u64) -> Result<usize, String> {
    if !app.db.lock().unwrap().items.contains_key(&iid) { return Err("Not found".into()); }
    analyze_where(app, iid, |db| with_leaves(db, iid))
}

/// "Analyze" for a whole library: every video in it.
pub fn analyze_section(app: &Arc<App>, sid: u64) -> Result<usize, String> {
    if !app.db.lock().unwrap().sections.iter().any(|s| s.id == sid) { return Err("No such library".into()); }
    analyze_where(app, sid, |db| db.items.values().filter(|i| i.section_id == sid && i.kind.playable()).map(|i| i.id).collect())
}

fn analyze_where(app: &Arc<App>, iid: u64, pick: impl FnOnce(&Db) -> Vec<u64>) -> Result<usize, String> {
    let mut job = app.job.lock().unwrap();
    if job.is_some() { return Err("Another job is already running; try again in a moment".into()); }
    let (ids, ffprobe) = {
        let db = app.db.lock().unwrap();
        // Unavailable files (in the trash) are left out.
        let ids: Vec<u64> = pick(&db).into_iter()
            .filter(|i| db.items.get(i).is_some_and(|x| x.file.is_some() && !x.extra.contains_key(crate::scanner::MISSING))).collect();
        (ids, db.settings.ffprobe.clone())
    };
    let total = ids.len();
    if total == 0 { return Err("There is no video file to analyze".into()); }
    *job = Some(("Analyzing".into(), 0, total));
    app.job_cancel.store(false, AtomicOrdering::SeqCst);
    drop(job);
    app.set_job_items("Analyzing", &ids);
    let app = app.clone();
    std::thread::spawn(move || {
        let mut failed = 0;
        for (n, id) in ids.into_iter().enumerate() {
            if app.job_cancelled() { break; }
            let files: Vec<db::MediaFile> = app.db.lock().unwrap().items.get(&id).map(|i| i.all_files().into_iter().cloned().collect()).unwrap_or_default();
            let mut read = vec![];
            for mut f in files {
                if let Ok(m) = std::fs::metadata(&f.path) { f.size = m.len(); }
                if !crate::scanner::probe(&ffprobe, &mut f) { failed += 1; }
                read.push(f);
            }
            if let Some(it) = app.db.lock().unwrap().items.get_mut(&id) {
                let mut read = read.into_iter();
                if let Some(f) = read.next() { it.file = Some(f); }
                it.parts = read.collect();
                let total_ms: u64 = it.all_files().iter().map(|f| f.duration_ms).sum();
                if total_ms > 0 { it.duration_ms = total_ms; }
                it.updated_at = now();
            }
            *app.job.lock().unwrap() = Some(("Analyzing".into(), n + 1, total));
        }
        app.save();
        *app.job.lock().unwrap() = None;
        let cancelled = app.job_cancel.swap(false, AtomicOrdering::SeqCst);
        let mut p = app.meta_progress.lock().unwrap();
        p.done += 1;
        p.last = json!({"id": iid, "title": "Analyze", "ok": failed == 0, "at": now(),
            "message": if cancelled { "Analysis cancelled".to_string() } else if failed == 0 { format!("Analyzed {total} video(s)") } else { format!("Analyzed {total} video(s); {failed} file(s) could not be read") }});
    });
    Ok(total)
}

/// "Deep analysis" of an item (and its episodes): every packet of its files is
/// read to work out the connection speed they really need (deep.rs).
pub fn deep_analyze(app: &Arc<App>, iid: u64) -> Result<usize, String> {
    if !app.db.lock().unwrap().items.contains_key(&iid) { return Err("Not found".into()); }
    deep_where(app, iid, |db| with_leaves(db, iid))
}

/// "Deep analysis" for a whole library.
pub fn deep_analyze_section(app: &Arc<App>, sid: u64) -> Result<usize, String> {
    if !app.db.lock().unwrap().sections.iter().any(|s| s.id == sid) { return Err("No such library".into()); }
    deep_where(app, sid, |db| db.items.values().filter(|i| i.section_id == sid && i.kind.playable() && i.kind != crate::db::ItemKind::Track).map(|i| i.id).collect())
}

fn deep_where(app: &Arc<App>, iid: u64, pick: impl FnOnce(&Db) -> Vec<u64>) -> Result<usize, String> {
    let mut job = app.job.lock().unwrap();
    if job.is_some() { return Err("Another job is already running; try again in a moment".into()); }
    let ids: Vec<u64> = {
        let db = app.db.lock().unwrap();
        pick(&db).into_iter().filter(|i| db.items.get(i).is_some_and(|x| x.file.is_some() && !x.extra.contains_key(crate::scanner::MISSING))).collect()
    };
    let total = ids.len();
    if total == 0 { return Err("There is no video file to analyze".into()); }
    *job = Some(("Deep analysis".into(), 0, total));
    app.job_cancel.store(false, AtomicOrdering::SeqCst);
    drop(job);
    app.set_job_items("Deep analysis", &ids);
    let app = app.clone();
    std::thread::spawn(move || {
        let mut failed = 0;
        for (n, id) in ids.into_iter().enumerate() {
            if app.job_cancelled() { break; }
            failed += deep_read(&app, id);
            *app.job.lock().unwrap() = Some(("Deep analysis".into(), n + 1, total));
        }
        app.save();
        *app.job.lock().unwrap() = None;
        let cancelled = app.job_cancel.swap(false, AtomicOrdering::SeqCst);
        let mut p = app.meta_progress.lock().unwrap();
        p.done += 1;
        p.last = json!({"id": iid, "title": "Deep analysis", "ok": failed == 0, "at": now(),
            "message": if cancelled { "Deep analysis cancelled".to_string() } else if failed == 0 { format!("Analyzed {total} video(s)") } else { format!("Analyzed {total} video(s); {failed} file(s) could not be read") }});
    });
    Ok(total)
}

/// Runs deep analysis on the files of one item, without holding the library
/// while a file is read. Returns how many files could not be read.
pub fn deep_read(app: &App, id: u64) -> usize {
    let ffprobe = app.settings().ffprobe;
    let paths: Vec<String> = app.db.lock().unwrap().items.get(&id).map(|i| i.all_files().into_iter().map(|f| f.path.clone()).collect()).unwrap_or_default();
    let mut failed = 0;
    for path in paths {
        let result = crate::deep::analyze_file(&ffprobe, &path);
        let mut db = app.db.lock().unwrap();
        let Some(it) = db.items.get_mut(&id) else { continue };
        let Some(f) = it.file.iter_mut().chain(it.parts.iter_mut()).find(|f| f.path == path) else { continue };
        match result {
            Ok(r) => crate::deep::apply(f, r),
            Err(e) => {
                failed += 1;
                f.deep_errors += 1;
                elog!("deep analysis of item {id}: {e}");
            }
        }
        let total_ms: u64 = it.all_files().iter().map(|f| f.duration_ms).sum();
        if total_ms > 0 { it.duration_ms = total_ms; }
    }
    failed
}

// ---------------------------------------------------------------- optimize

/// Where optimized versions are kept: <data>/optimized/<item id>.mp4.
pub fn optimized_path(app: &App, iid: u64) -> PathBuf {
    app.data_dir.join("optimized").join(format!("{iid}.mp4"))
}

/// The optimized version of an item, when there is one and its file is still there.
pub fn optimized_file(app: &App, it: &Item) -> Option<PathBuf> {
    it.extra.get("optimized")?;
    let p = optimized_path(app, it.id);
    p.is_file().then_some(p)
}

/// Makes an optimized version (MP4, H.264 + AAC) of a movie or of every
/// episode under a show or season, one after the other on a background thread.
pub fn optimize(app: &Arc<App>, iid: u64, preset: &str) -> Result<usize, String> {
    if !transcode::OPTIMIZE.iter().any(|p| p.0 == preset) { return Err("Unknown quality".into()); }
    let mut job = app.job.lock().unwrap();
    if job.is_some() { return Err("Another job is already running; try again in a moment".into()); }
    let (todo, settings) = {
        let db = app.db.lock().unwrap();
        if !db.items.contains_key(&iid) { return Err("Not found".into()); }
        // Movies split over several files are left out (they would need joining).
        let todo: Vec<(u64, db::MediaFile)> = with_leaves(&db, iid).into_iter()
            .filter_map(|i| db.items.get(&i)).filter(|i| i.parts.is_empty())
            .filter_map(|i| i.file.clone().map(|f| (i.id, f))).collect();
        (todo, db.settings.clone())
    };
    let total = todo.len();
    if total == 0 { return Err("There is no video file to optimize".into()); }
    let dir = app.data_dir.join("optimized");
    std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    *job = Some(("Optimizing".into(), 0, total * 100));
    app.job_cancel.store(false, AtomicOrdering::SeqCst);
    drop(job);
    app.set_job_items("Optimizing", &todo.iter().map(|t| t.0).collect::<Vec<_>>());
    let app = app.clone();
    let preset = preset.to_string();
    std::thread::spawn(move || {
        let mut failed = 0;
        for (n, (id, file)) in todo.into_iter().enumerate() {
            if app.job_cancelled() { break; }
            let out = optimized_path(&app, id);
            let tmp = out.with_extension("part.mp4");
            let Some(args) = transcode::optimize_args(&settings, &file, &preset, &tmp.to_string_lossy()) else { continue };
            let child = Command::new(&settings.ffmpeg).args(&args).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn();
            let ok = match child {
                Ok(mut c) => {
                    // stderr is read on the side so a chatty ffmpeg never blocks.
                    let se = c.stderr.take();
                    let err = std::thread::spawn(move || { let mut s = String::new(); if let Some(mut se) = se { let _ = std::io::Read::read_to_string(&mut se, &mut s); } s });
                    // -progress prints out_time_us=… lines: shown as a percentage of this file.
                    if let Some(so) = c.stdout.take() {
                        for line in BufReader::new(so).lines().map_while(Result::ok) {
                            if app.job_cancelled() { let _ = c.kill(); }
                            if let Some(us) = line.strip_prefix("out_time_us=").and_then(|v| v.trim().parse::<u64>().ok()) {
                                let pct = if file.duration_ms > 0 { (us / 1000 * 100 / file.duration_ms).min(99) as usize } else { 0 };
                                *app.job.lock().unwrap() = Some(("Optimizing".into(), n * 100 + pct, total * 100));
                            }
                        }
                    }
                    let st = c.wait().map(|s| s.success()).unwrap_or(false);
                    let err = err.join().unwrap_or_default();
                    if !st { elog!("optimizing item {id} failed: {}", err.trim()); }
                    st
                }
                Err(e) => { elog!("optimizing item {id}: ffmpeg could not be started: {e}"); false }
            };
            if ok && std::fs::rename(&tmp, &out).is_ok() {
                let size = std::fs::metadata(&out).map(|m| m.len()).unwrap_or(0);
                if let Some(it) = app.db.lock().unwrap().items.get_mut(&id) {
                    it.extra.insert("optimized".into(), json!({"quality": preset, "size": size, "at": now()}));
                }
            } else {
                failed += 1;
                let _ = std::fs::remove_file(&tmp);
            }
            *app.job.lock().unwrap() = Some(("Optimizing".into(), (n + 1) * 100, total * 100));
        }
        app.save();
        *app.job.lock().unwrap() = None;
        let cancelled = app.job_cancel.swap(false, AtomicOrdering::SeqCst);
        let mut p = app.meta_progress.lock().unwrap();
        p.done += 1;
        p.last = json!({"id": iid, "title": "Optimize", "ok": failed == 0, "at": now(),
            "message": if cancelled { "Optimizing cancelled".to_string() } else if failed == 0 { format!("Optimized {total} video(s)") } else { format!("Optimized {} of {total} video(s); the others failed (see the server log)", total - failed) }});
    });
    Ok(total)
}

/// Deletes the optimized versions of an item and its episodes.
pub fn remove_optimized(app: &Arc<App>, iid: u64) -> usize {
    let mut db = app.db.lock().unwrap();
    let mut n = 0;
    for id in with_leaves(&db, iid) {
        if let Some(it) = db.items.get_mut(&id) {
            if it.extra.remove("optimized").is_some() { n += 1; }
        }
        let _ = std::fs::remove_file(optimized_path(app, id));
    }
    drop(db);
    app.save();
    n
}

/// Every optimized version: [{id, title, quality, size}], biggest first.
pub fn optimized_list(app: &Arc<App>) -> Value {
    let db = app.db.lock().unwrap();
    let mut rows: Vec<(u64, Value)> = db.items.values().filter_map(|it| {
        let o = it.extra.get("optimized")?;
        let size = o.get("size").and_then(|v| v.as_u64()).unwrap_or(0);
        let show = it.parent_id.and_then(|p| db.items.get(&p)).and_then(|p| p.parent_id.and_then(|g| db.items.get(&g))).map(|g| g.title.clone());
        let title = match show { Some(s) => format!("{s} – {}", it.title), None => it.title.clone() };
        Some((size, json!({"id": it.id, "title": title, "quality": o.get("quality").cloned().unwrap_or(Value::Null), "size": size})))
    }).collect();
    rows.sort_by(|a, b| b.0.cmp(&a.0));
    json!({"items": rows.into_iter().map(|r| r.1).collect::<Vec<_>>()})
}

/// Delete every optimized version at once → how many.
pub fn remove_all_optimized(app: &Arc<App>) -> usize {
    let ids: Vec<u64> = { let db = app.db.lock().unwrap(); db.items.values().filter(|i| i.extra.contains_key("optimized")).map(|i| i.id).collect() };
    ids.into_iter().map(|id| remove_optimized(app, id)).sum()
}

// ---------------------------------------------------------------- trash

/// "Empty trash" of a library: unavailable items leave it for good.
pub fn empty_trash(app: &Arc<App>, sid: u64) -> Result<usize, String> {
    if app.scanning.lock().unwrap().contains(&sid) { return Err("That library is being scanned; try again when it is done".into()); }
    let mut db = app.db.lock().unwrap();
    if !db.sections.iter().any(|s| s.id == sid) { return Err("No such library".into()); }
    let gone = crate::scanner::empty_trash(&mut db, sid);
    drop(db);
    app.save();
    for id in &gone { app.fire("library.item.removed", json!({"id": id, "section_id": sid})); }
    Ok(gone.len())
}

// ---------------------------------------------------------------- deleting files

/// May this person delete files from disk? Only when the server allows media
/// deletion, and then the owner and admins, and people given the right.
pub fn may_delete(db: &Db, user: &User) -> bool {
    db.settings.allow_media_deletion && (user.admin || user.allow_delete)
}

/// Deletes the files of a movie or episode (every part), or of every episode of
/// a season or show, and takes them out of the library. Optimized copies go too.
pub fn delete_media(app: &Arc<App>, user: &User, iid: u64) -> Resp {
    let mut db = app.db.lock().unwrap();
    if !db.settings.allow_media_deletion { return fail(403, "Deleting media is switched off in the settings"); }
    if !may_delete(&db, user) { return fail(403, "You may not delete files"); }
    let Some(it) = db.items.get(&iid) else { return fail(404, "Not found") };
    if !db.can_see_item(user, it) { return fail(404, "Not found"); }
    let sid = it.section_id;
    if app.scanning.lock().unwrap().contains(&sid) { return fail(409, "That library is being scanned; try again when it is done"); }
    let roots: Vec<PathBuf> = db.sections.iter().find(|s| s.id == sid).map(|s| s.paths.iter().map(PathBuf::from).collect()).unwrap_or_default();
    let mut removed = vec![];
    let mut files = 0;
    let mut failed: Vec<String> = vec![];
    for id in with_leaves(&db, iid) {
        let Some(leaf) = db.items.get(&id) else { continue };
        if !leaf.kind.playable() { continue; }
        let mut all = true;
        for f in leaf.all_files() {
            let path = std::path::Path::new(&f.path);
            // Only files inside the library's own folders are touched.
            if !roots.iter().any(|r| path.starts_with(r)) { failed.push(format!("{}: not inside the library's folders", f.path)); all = false; continue; }
            match std::fs::remove_file(path) {
                Ok(()) => files += 1,
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => { failed.push(format!("{}: {e}", f.path)); all = false; }
            }
        }
        if all {
            let _ = std::fs::remove_file(optimized_path(app, id));
            removed.push(id);
        }
    }
    for id in &removed { db.items.remove(id); }
    let pruned = crate::scanner::prune_empty(&mut db, sid);
    drop(db);
    app.save();
    for id in removed.iter().chain(pruned.iter()) { app.fire("library.item.removed", json!({"id": id, "section_id": sid, "deleted_by": user.username})); }
    if !failed.is_empty() {
        elog!("deleting item {iid}: {}", failed.join("; "));
        return js(500, json!({"error": format!("{} file(s) could not be deleted: {}", failed.len(), failed.join("; ")), "deleted": files, "failed": failed}));
    }
    ok(json!({"ok": true, "deleted": files, "removed": removed.len() + pruned.len()}))
}

// ---------------------------------------------------------------- watch together

/// One "Watch together" room: everyone in it plays the same video at the
/// same place. Whoever plays, pauses or seeks moves everyone (kept in memory).
struct Room {
    item_id: u64,
    host: u64,
    /// user id → (name, last seen)
    members: BTreeMap<u64, (String, Instant)>,
    position_ms: u64,
    paused: bool,
    since: Instant,
    /// Goes up with every change, so players know when to follow.
    seq: u64,
    by: String,
}

fn rooms() -> &'static Mutex<HashMap<String, Room>> {
    static R: OnceLock<Mutex<HashMap<String, Room>>> = OnceLock::new();
    R.get_or_init(|| Mutex::new(HashMap::new()))
}

impl Room {
    fn position(&self) -> u64 {
        if self.paused { self.position_ms } else { self.position_ms + self.since.elapsed().as_millis() as u64 }
    }
    fn json(&self, code: &str, db: &Db) -> Value {
        let alive: Vec<Value> = self.members.iter().filter(|(_, (_, seen))| seen.elapsed().as_secs() < 20)
            .map(|(id, (name, _))| json!({"id": id, "name": name, "host": *id == self.host})).collect();
        json!({"code": code, "item": self.item_id, "title": db.items.get(&self.item_id).map(|i| i.title.clone()),
            "position": self.position(), "paused": self.paused, "seq": self.seq, "by": self.by, "members": alive})
    }
}

fn forget_old_rooms(map: &mut HashMap<String, Room>) {
    map.retain(|_, r| r.members.values().any(|(_, seen)| seen.elapsed().as_secs() < 3600));
}

pub fn together_create(app: &Arc<App>, ctx: &Ctx, user: &User) -> Resp {
    let Some(iid) = ctx.body_or_param("item").and_then(|v| v.parse::<u64>().ok()) else { return fail(400, "item is needed") };
    let db = app.db.lock().unwrap();
    match db.items.get(&iid) {
        Some(it) if !db.can_see_item(user, it) => return fail(403, "You don't have access to this library"),
        Some(it) if it.file.is_none() => return fail(400, "This entry has no video file"),
        None => return fail(404, "Not found"),
        _ => {}
    }
    let position_ms = ctx.body_or_param("position").and_then(|v| v.parse::<f64>().ok()).unwrap_or(0.0).max(0.0) as u64;
    let mut map = rooms().lock().unwrap();
    forget_old_rooms(&mut map);
    let code = loop {
        let c = db::random_hex(4);
        if !map.contains_key(&c) { break c; }
    };
    let mut members = BTreeMap::new();
    members.insert(user.id, (user.username.clone(), Instant::now()));
    let room = Room { item_id: iid, host: user.id, members, position_ms, paused: true, since: Instant::now(), seq: 1, by: user.username.clone() };
    let v = room.json(&code, &db);
    map.insert(code, room);
    js(201, v)
}

/// The room's state; asking also keeps you in it (players ask every few seconds).
pub fn together_get(app: &Arc<App>, user: &User, code: &str) -> Resp {
    let db = app.db.lock().unwrap();
    let mut map = rooms().lock().unwrap();
    let Some(room) = map.get_mut(code) else { return fail(404, "This watch-together link has ended") };
    if db.items.get(&room.item_id).is_none_or(|it| !db.can_see_item(user, it)) {
        return fail(403, "You don't have access to this library");
    }
    room.members.insert(user.id, (user.username.clone(), Instant::now()));
    ok(room.json(code, &db))
}

/// Play, pause, seek or another video: {position (ms), paused, item}.
pub fn together_update(app: &Arc<App>, ctx: &Ctx, user: &User, code: &str) -> Resp {
    let db = app.db.lock().unwrap();
    let mut map = rooms().lock().unwrap();
    let Some(room) = map.get_mut(code) else { return fail(404, "This watch-together link has ended") };
    if !room.members.contains_key(&user.id) { return fail(403, "Open the link to join first"); }
    if let Some(iid) = ctx.body.get("item").and_then(|v| v.as_u64().or_else(|| v.as_str().and_then(|s| s.parse().ok()))) {
        if db.items.get(&iid).is_none_or(|it| !db.can_see_item(user, it) || it.file.is_none()) { return fail(404, "Not found"); }
        room.item_id = iid;
    }
    if let Some(p) = ctx.body.get("position").and_then(|v| v.as_f64()) { room.position_ms = p.max(0.0) as u64; } else { room.position_ms = room.position(); }
    if let Some(b) = ctx.body.get("paused").and_then(|v| v.as_bool()) { room.paused = b; }
    room.since = Instant::now();
    room.seq += 1;
    room.by = user.username.clone();
    room.members.insert(user.id, (user.username.clone(), Instant::now()));
    ok(room.json(code, &db))
}

pub fn together_leave(user: &User, code: &str) -> Resp {
    let mut map = rooms().lock().unwrap();
    if let Some(room) = map.get_mut(code) {
        room.members.remove(&user.id);
        if room.members.is_empty() { map.remove(code); }
    }
    ok(json!({"ok": true}))
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    /// An app with one movie and a stand-in "ffmpeg" (a shell script) in its settings.
    fn app_with_fake_ffmpeg(name: &str, script: &str) -> (Arc<App>, PathBuf) {
        let root = std::env::temp_dir().join(format!("lichtspiel-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let ff = root.join("ffmpeg.sh");
        // Hardware checks (a lavfi test picture) just fail; everything else runs the script.
        std::fs::write(&ff, format!("#!/bin/sh\ncase \"$*\" in *lavfi*) exit 1;; esac\n{script}\n")).unwrap();
        std::fs::set_permissions(&ff, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
        let app = App::open(&root.join("data"));
        {
            let mut db = app.db.lock().unwrap();
            db.settings.ffmpeg = ff.to_string_lossy().to_string();
            let mut i: Item = serde_json::from_value(json!({"id": 1, "section_id": 1, "kind": "movie", "title": "x", "sort_title": "x", "added_at": 0, "updated_at": 0})).unwrap();
            i.file = Some(db::MediaFile { path: root.join("x.mkv").to_string_lossy().to_string(), duration_ms: 1000, ..Default::default() });
            db.items.insert(1, i);
        }
        (app, root)
    }

    fn job_ends_within(app: &App, secs: u64) -> bool {
        let start = Instant::now();
        while start.elapsed().as_secs() < secs {
            if app.job.lock().unwrap().is_none() { return true; }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        false
    }

    #[test]
    fn optimize_does_not_hang_when_ffmpeg_prints_many_errors() {
        // More than a pipe holds (about 64 KB) on stderr before anything on stdout.
        let (app, root) = app_with_fake_ffmpeg("optimize-chatty", "yes 'broken frame' | head -c 300000 >&2\necho out_time_us=1000");
        optimize(&app, 1, transcode::OPTIMIZE[0].0).unwrap();
        assert!(job_ends_within(&app, 10), "the optimize job hung");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn cancel_stops_a_running_optimize() {
        let (app, root) = app_with_fake_ffmpeg("optimize-cancel", "while true; do echo out_time_us=1000; sleep 0.1; done");
        optimize(&app, 1, transcode::OPTIMIZE[0].0).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(300));
        assert!(app.cancel_job());
        assert!(job_ends_within(&app, 10), "Cancel did not stop ffmpeg");
        let _ = std::fs::remove_dir_all(&root);
    }
}

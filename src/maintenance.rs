//! Troubleshooting for the owner: optimize the library file, clean up leftovers,
//! download a copy of the library file and a report of how the server is set up.

use crate::handlers::serve_file_as;
use crate::server::*;
use serde_json::json;
use std::path::Path;

fn dir_size(dir: &Path) -> u64 {
    let Ok(rd) = std::fs::read_dir(dir) else { return 0 };
    rd.flatten().map(|e| match e.metadata() {
        Ok(m) if m.is_dir() => dir_size(&e.path()),
        Ok(m) => m.len(),
        Err(_) => 0,
    }).sum()
}

fn file_size(p: &Path) -> u64 {
    std::fs::metadata(p).map(|m| m.len()).unwrap_or(0)
}

/// The library file with its journal files.
fn library_bytes(app: &App) -> u64 {
    let f = app.data_dir.join(crate::store::FILE_NAME);
    ["", "-wal", "-shm"].iter().map(|e| file_size(Path::new(&format!("{}{e}", f.display())))).sum()
}

/// Writes what is pending, then compacts the library file.
pub fn optimize(app: &App) -> Resp {
    app.save();
    let before = library_bytes(app);
    let r = app.store.lock().unwrap().optimize();
    match r {
        Ok(()) => ok(json!({"ok": true, "before": before, "after": library_bytes(app)})),
        Err(e) => fail(500, &format!("Could not optimize the library file: {e}")),
    }
}

/// Removes converted leftovers nobody is watching and cache files older than a week.
pub fn cleanup(app: &App) -> Resp {
    let before = dir_size(&app.transcode_dir()) + dir_size(&app.data_dir.join("cache"));
    let idle = app.hls.lock().unwrap().is_empty() && app.progressive.lock().unwrap().is_empty();
    if idle {
        if let Ok(rd) = std::fs::read_dir(app.transcode_dir()) {
            for e in rd.flatten() {
                let p = e.path();
                let _ = if p.is_dir() { std::fs::remove_dir_all(&p) } else { std::fs::remove_file(&p) };
            }
        }
    }
    let week = std::time::Duration::from_secs(7 * 86400);
    fn old_files(dir: &Path, week: std::time::Duration) {
        let Ok(rd) = std::fs::read_dir(dir) else { return };
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() {
                old_files(&p, week);
                continue;
            }
            let old = e.metadata().and_then(|m| m.modified()).ok().and_then(|t| t.elapsed().ok()).is_some_and(|a| a > week);
            if old { let _ = std::fs::remove_file(&p); }
        }
    }
    old_files(&app.data_dir.join("cache"), week);
    let after = dir_size(&app.transcode_dir()) + dir_size(&app.data_dir.join("cache"));
    ok(json!({"ok": true, "freed": before.saturating_sub(after), "transcodes_kept": !idle}))
}

/// A consistent copy of the library file, sent as a download.
pub fn database(app: &App) -> Resp {
    app.save();
    let dir = app.data_dir.join("cache").join("export");
    let _ = std::fs::remove_dir_all(&dir);
    if std::fs::create_dir_all(&dir).is_err() { return fail(500, "Could not make the export folder"); }
    let path = dir.join("library.db");
    if let Err(e) = app.store.lock().unwrap().copy_to(&path) { return fail(500, &format!("Could not copy the library file: {e}")); }
    serve_file_as(&path, "application/octet-stream", None, Some("lichtspiel-library.db"))
}

/// A text report: version, system, settings (keys left out), libraries, tools and plugin messages.
pub fn report(app: &App) -> Resp {
    let mut out = String::new();
    let mut line = |s: String| { out.push_str(&s); out.push('\n'); };
    line(format!("{} {}", APP_NAME, VERSION));
    line(format!("system: {} {}", std::env::consts::OS, std::env::consts::ARCH));
    line(format!("data folder: {}", app.data_dir.display()));
    line(format!("library file: {} bytes; cache: {} bytes; transcode: {} bytes", library_bytes(app), dir_size(&app.data_dir.join("cache")), dir_size(&app.transcode_dir())));
    line(format!("running since: {} (unix seconds)", app.started_at));
    {
        let db = app.db.lock().unwrap();
        line(format!("users: {}, libraries: {}, items: {}", db.users.len(), db.sections.len(), db.items.len()));
        for s in &db.sections { line(format!("  library {} \"{}\" ({:?}): {}", s.id, s.name, s.kind, s.paths.join("; "))); }
        let mut st = serde_json::to_value(&db.settings).unwrap_or_default();
        if let Some(o) = st.as_object_mut() {
            for (k, v) in o.iter_mut() { if k.contains("key") || k.contains("token") || k.contains("password") { if v.as_str().is_some_and(|s| !s.is_empty()) { *v = json!("(set)"); } } }
        }
        line(format!("settings: {}", serde_json::to_string_pretty(&st).unwrap_or_default()));
        line(format!("transcoder: {}", crate::transcode::detect(&db.settings)));
    }
    {
        let pl = app.plugins.lock().unwrap();
        for e in &pl.errors { line(format!("plugin error: {e}")); }
        for e in pl.log.iter() { line(format!("plugin log: {e}")); }
    }
    let mut h = base_headers();
    h.push(header("Content-Type", "text/plain; charset=utf-8"));
    h.push(header("Content-Disposition", &crate::handlers::attachment("lichtspiel-report.txt")));
    let n = out.len();
    tiny_http::Response::new(tiny_http::StatusCode(200), h, Box::new(std::io::Cursor::new(out.into_bytes())) as Box<dyn std::io::Read + Send>, Some(n), None)
}

/// Is the local hour inside the window (it may wrap past midnight; equal hours = always)?
pub fn in_window(now_unix: u64, offset_min: i64, start: u32, end: u32) -> bool {
    let hour = (((now_unix as i64 + offset_min * 60) / 3600).rem_euclid(24)) as u32;
    if start == end { return true; }
    if start < end { hour >= start && hour < end } else { hour >= start || hour < end }
}

/// Saves a copy of the library file into the backup folder and keeps the newest few.
pub fn backup(app: &App) -> Result<String, String> {
    app.save();
    let (dir, keep) = {
        let db = app.db.lock().unwrap();
        let d = db.settings.backup_dir.trim();
        (if d.is_empty() { app.data_dir.join("backups") } else { std::path::PathBuf::from(d) }, db.settings.backups_keep.max(1) as usize)
    };
    std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let name = format!("library-{}.db", crate::db::now());
    let path = dir.join(&name);
    app.store.lock().unwrap().copy_to(&path)?;
    let mut old: Vec<_> = std::fs::read_dir(&dir).map(|rd| rd.flatten().map(|e| e.file_name().to_string_lossy().to_string()).filter(|n| n.starts_with("library-") && n.ends_with(".db")).collect()).unwrap_or_default();
    old.sort();
    while old.len() > keep { let _ = std::fs::remove_file(dir.join(old.remove(0))); }
    Ok(path.display().to_string())
}

/// The backups in the backup folder, newest first: (file name, bytes, modified as unix seconds).
pub fn backups(app: &App) -> Vec<(String, u64, u64)> {
    let dir = backup_dir(app);
    let mut v: Vec<(String, u64, u64)> = std::fs::read_dir(&dir).map(|rd| rd.flatten().filter_map(|e| {
        let n = e.file_name().to_string_lossy().to_string();
        if !(n.starts_with("library-") && n.ends_with(".db")) { return None; }
        let m = e.metadata().ok()?;
        let t = m.modified().ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map(|d| d.as_secs()).unwrap_or(0);
        Some((n, m.len(), t))
    }).collect()).unwrap_or_default();
    v.sort_by(|a, b| b.0.cmp(&a.0));
    v
}

fn backup_dir(app: &App) -> std::path::PathBuf {
    let db = app.db.lock().unwrap();
    let d = db.settings.backup_dir.trim();
    if d.is_empty() { app.data_dir.join("backups") } else { std::path::PathBuf::from(d) }
}

/// Puts a backup in place of the current library: everything (libraries, users, settings, watch
/// history) goes back to how it was then. The current state is backed up first.
pub fn restore(app: &App, name: &str) -> Result<(), String> {
    if !(name.starts_with("library-") && name.ends_with(".db")) || name.contains(['/', '\\']) { return Err("That is not a backup file".into()); }
    let src = backup_dir(app).join(name);
    if !src.is_file() { return Err("That backup does not exist any more".into()); }
    // Read from a copy: opening a file can write next to it.
    let tmp = app.data_dir.join("restore-check.db");
    let _ = std::fs::remove_file(&tmp);
    std::fs::copy(&src, &tmp).map_err(|e| e.to_string())?;
    let loaded = (|| {
        let mut st = crate::store::Store::open(&tmp)?;
        st.load()?.ok_or_else(|| "That backup file holds no library".to_string())
    })();
    for ext in ["", "-wal", "-shm"] { let _ = std::fs::remove_file(app.data_dir.join(format!("restore-check.db{ext}"))); }
    let mut loaded = loaded?;
    backup(app)?;
    loaded.items.mark_all();
    loaded.views.mark_all();
    *app.db.lock().unwrap() = loaded;
    app.store.lock().unwrap().forget_written();
    app.save();
    elog!("restored the library from {name}");
    Ok(())
}

/// The integrated loudness (LUFS) from the summary ffmpeg's ebur128 filter prints at the end.
pub fn parse_loudness(log: &str) -> Option<f64> {
    log.lines().rev().find_map(|l| {
        let l = l.trim();
        let rest = l.strip_prefix("I:")?;
        rest.trim().strip_suffix("LUFS")?.trim().parse::<f64>().ok().filter(|v| v.is_finite() && *v > -120.0)
    })
}

/// Measures the loudness of the first audio track (the first 20 minutes are enough).
pub fn measure_loudness(ffmpeg: &str, path: &str) -> Result<f64, String> {
    use std::process::{Command, Stdio};
    let out = Command::new(ffmpeg)
        .args(["-nostdin", "-hide_banner", "-t", "1200", "-i"]).arg(path)
        .args(["-map", "0:a:0", "-vn", "-sn", "-dn", "-af", "ebur128", "-f", "null", "-"])
        .stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::piped())
        .output().map_err(|e| format!("ffmpeg could not be started: {e}"))?;
    parse_loudness(&String::from_utf8_lossy(&out.stderr)).ok_or_else(|| "no loudness in the output of ffmpeg".to_string())
}

/// Measures up to `max` videos that have not been measured yet; returns how many were done.
pub fn analyze_loudness(app: &App, max: usize) -> usize {
    let ffmpeg = app.settings().ffmpeg;
    let todo: Vec<(u64, String)> = {
        let db = app.db.lock().unwrap();
        db.items.values()
            .filter(|i| i.kind.playable() && !i.extra.contains_key("loudness_lufs") && !i.extra.contains_key("loudness_failed"))
            .filter_map(|i| i.file.as_ref().filter(|f| i.parts.is_empty() && std::path::Path::new(&f.path).is_file()).map(|f| (i.id, f.path.clone())))
            .take(max).collect()
    };
    for (id, path) in &todo {
        let r = measure_loudness(&ffmpeg, path);
        let mut db = app.db.lock().unwrap();
        if let Some(it) = db.items.get_mut(id) {
            match r {
                Ok(v) => { it.extra.insert("loudness_lufs".into(), json!((v * 10.0).round() / 10.0)); }
                Err(_) => { it.extra.insert("loudness_failed".into(), json!(crate::db::now())); }
            }
        }
    }
    if !todo.is_empty() { app.save(); }
    todo.len()
}

/// Deep analysis of up to `max` videos that need it, the latest changed first
/// (a new or changed file, or one from an older analysis). Returns how many were done.
pub fn deep_analysis(app: &App, max: usize) -> usize {
    let mut todo: Vec<(u64, u64)> = {
        let db = app.db.lock().unwrap();
        db.items.values()
            .filter(|i| i.kind.playable() && i.kind != crate::db::ItemKind::Track && !i.extra.contains_key(crate::scanner::MISSING))
            .filter(|i| i.all_files().iter().any(|f| !crate::deep::is_current(f) && f.deep_errors <= crate::db::CREDITS_MAX_ERRORS && Path::new(&f.path).is_file()))
            .map(|i| (i.updated_at, i.id)).collect()
    };
    todo.sort_by_key(|t| std::cmp::Reverse(*t));
    todo.truncate(max);
    for (_, id) in &todo { crate::actions::deep_read(app, *id); }
    if !todo.is_empty() { app.save(); }
    todo.len()
}

/// Makes the seek-bar pictures (one every 10 s) and the chapter pictures of single-file videos ahead
/// of time, so they show at once. Returns how many videos were done.
pub fn make_thumbnails(app: &App, max: usize) -> usize {
    let (ffmpeg, ffprobe) = { let s = app.settings(); (s.ffmpeg, s.ffprobe) };
    let dir = app.data_dir.join("cache").join("frames");
    let _ = std::fs::create_dir_all(&dir);
    let todo: Vec<(u64, String)> = {
        let db = app.db.lock().unwrap();
        db.items.values()
            .filter(|i| i.kind.playable() && i.kind != crate::db::ItemKind::Track && !i.extra.contains_key("thumbnails_done"))
            .filter_map(|i| i.file.as_ref().filter(|f| i.parts.is_empty() && Path::new(&f.path).is_file()).map(|f| (i.id, f.path.clone())))
            .take(max).collect()
    };
    for (id, path) in &todo {
        let tmp = format!("tmp{id}-");
        let args: Vec<String> = ["-nostdin", "-hide_banner", "-loglevel", "error", "-y", "-i", path, "-an", "-sn", "-dn", "-vf", "fps=1/10,scale=240:-2", "-q:v", "6"]
            .iter().map(|x| x.to_string()).chain(std::iter::once(dir.join(format!("{tmp}%05d.jpg")).to_string_lossy().to_string())).collect();
        let ok = std::process::Command::new(&ffmpeg).args(&args).stdin(std::process::Stdio::null()).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null()).status().map(|s| s.success()).unwrap_or(false);
        // The n-th picture (counting from 1) is the one at (n-1)*10 seconds.
        for n in 1..100000u64 {
            let from = dir.join(format!("{tmp}{n:05}.jpg"));
            if !from.exists() { break; }
            let to = dir.join(format!("{id}-{}.jpg", (n - 1) * 10));
            if to.exists() { let _ = std::fs::remove_file(&from); } else { let _ = std::fs::rename(&from, &to); }
        }
        for (s, e, _) in crate::scanner::read_chapters(&ffprobe, path).unwrap_or_default() {
            let at = crate::handlers::chapter_picture_at(s, e);
            crate::handlers::make_frame(&ffmpeg, path, at as f64, &dir.join(format!("{id}-{at}.jpg")));
        }
        let mut db = app.db.lock().unwrap();
        if let Some(it) = db.items.get_mut(id) { it.extra.insert("thumbnails_done".into(), json!(if ok { crate::db::now() } else { 0 })); }
    }
    if !todo.is_empty() { app.save(); }
    todo.len()
}

/// Runs once a minute: starts the tasks that are due, but only inside the maintenance window.
pub fn tick(app: &std::sync::Arc<App>) {
    let (s, now) = (app.settings(), crate::db::now());
    if !in_window(now, s.utc_offset_minutes, s.maintenance_start_hour, s.maintenance_end_hour) { return; }
    let due = |key: &str, days: u64| now.saturating_sub(s.maintenance_last.get(key).copied().unwrap_or(0)) >= days * 86400 - 3600;
    let mut ran: Vec<&str> = vec![];
    if s.backup_enabled && due("backup", 3) {
        match backup(app) { Ok(_) => {} Err(e) => elog!("backup: {e}") }
        ran.push("backup");
    }
    if s.optimize_weekly && due("optimize", 7) { let _ = optimize(app); ran.push("optimize"); }
    if s.cleanup_weekly && due("cleanup", 7) { let _ = cleanup(app); ran.push("cleanup"); }
    if s.refresh_metadata && due("refresh", 3) {
        let ids: Vec<u64> = app.db.lock().unwrap().sections.iter().map(|x| x.id).collect();
        for id in ids { app.scan_in_background(id); }
        ran.push("refresh");
    }
    if s.analyze_loudness { analyze_loudness(app, 5); }
    if s.deep_analysis { deep_analysis(app, 1); }
    if s.generate_thumbnails { make_thumbnails(app, 2); }
    if !ran.is_empty() {
        { let mut db = app.db.lock().unwrap(); for k in ran { db.settings.maintenance_last.insert(k.to_string(), now); } }
        app.save();
    }
}

#[cfg(test)]
mod tests {
    use super::{in_window, parse_loudness};
    #[test]
    fn loudness_of_a_real_file() {
        let dir = std::env::temp_dir().join("lichtspiel-loudness-test");
        let _ = std::fs::create_dir_all(&dir);
        let mk = |name: &str, db: &str| {
            let f = dir.join(name);
            let ok = std::process::Command::new("ffmpeg").args(["-nostdin", "-loglevel", "error", "-y", "-f", "lavfi", "-i", "sine=frequency=440:duration=6", "-af", &format!("volume={db}dB")]).arg(&f).status().map(|s| s.success()).unwrap_or(false);
            ok.then(|| super::measure_loudness("ffmpeg", &f.to_string_lossy()))
        };
        let (Some(quiet), Some(loud)) = (mk("quiet.wav", "-20"), mk("loud.wav", "-6")) else { return }; // no ffmpeg here
        let (q, l) = (quiet.unwrap(), loud.unwrap());
        assert!((l - q - 14.0).abs() < 1.0, "quiet {q}, loud {l}");
        let _ = std::fs::remove_dir_all(&dir);
    }
    #[test]
    fn loudness_is_read_from_the_summary() {
        let log = "[Parsed_ebur128_0 @ 0x1] Summary:

  Integrated loudness:
    I:         -23.4 LUFS
    Threshold: -33.4 LUFS

  Loudness range:
    LRA:         5.2 LU
";
        assert_eq!(parse_loudness(log), Some(-23.4));
        assert_eq!(parse_loudness("nothing"), None);
    }
    #[test]
    fn window_follows_local_hours() {
        // 03:00 UTC with +60 min offset = 04:00 local
        assert!(in_window(3 * 3600, 60, 2, 5));
        assert!(!in_window(6 * 3600, 0, 2, 5));
        // wraps past midnight: 22:00-04:00
        assert!(in_window(23 * 3600, 0, 22, 4) && in_window(3 * 3600, 0, 22, 4) && !in_window(12 * 3600, 0, 22, 4));
        assert!(in_window(12 * 3600, 0, 0, 0));
    }
}

#[cfg(test)]
mod thumb_tests {
    #[test]
    fn chapter_picture_is_taken_after_the_start() {
        assert_eq!(crate::handlers::chapter_picture_at(0, 4000), 2);
        assert_eq!(crate::handlers::chapter_picture_at(60000, 600000), 63);
    }
}

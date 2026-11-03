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
    let before = dir_size(&app.data_dir.join("transcode")) + dir_size(&app.data_dir.join("cache"));
    let idle = app.hls.lock().unwrap().is_empty() && app.progressive.lock().unwrap().is_empty();
    if idle {
        if let Ok(rd) = std::fs::read_dir(app.data_dir.join("transcode")) {
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
    let after = dir_size(&app.data_dir.join("transcode")) + dir_size(&app.data_dir.join("cache"));
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
    line(format!("library file: {} bytes; cache: {} bytes; transcode: {} bytes", library_bytes(app), dir_size(&app.data_dir.join("cache")), dir_size(&app.data_dir.join("transcode"))));
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

/// Runs once a minute: starts the tasks that are due, but only inside the maintenance window.
pub fn tick(app: &App) {
    let (s, now) = (app.settings(), crate::db::now());
    if !in_window(now, s.utc_offset_minutes, s.maintenance_start_hour, s.maintenance_end_hour) { return; }
    let due = |key: &str, days: u64| now.saturating_sub(s.maintenance_last.get(key).copied().unwrap_or(0)) >= days * 86400 - 3600;
    let mut ran: Vec<&str> = vec![];
    if s.backup_enabled && due("backup", 3) {
        match backup(app) { Ok(_) => {} Err(e) => eprintln!("backup: {e}") }
        ran.push("backup");
    }
    if s.optimize_weekly && due("optimize", 7) { let _ = optimize(app); ran.push("optimize"); }
    if s.cleanup_weekly && due("cleanup", 7) { let _ = cleanup(app); ran.push("cleanup"); }
    if !ran.is_empty() {
        { let mut db = app.db.lock().unwrap(); for k in ran { db.settings.maintenance_last.insert(k.to_string(), now); } }
        app.save();
    }
}

#[cfg(test)]
mod tests {
    use super::in_window;
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

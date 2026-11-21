//! Import from the old server, IMDb ratings, Plex sync, Trakt, and what the server is busy with.

use super::*;

pub(super) fn routes(app: &Arc<App>, ctx: &Ctx, user: &db::User, m: &str, segs: &[&str]) -> Option<Resp> {
    let admin = user.admin;
    handled(|hit| match (m, segs) {
        ("POST", ["api", "imdb-ratings", "refresh"]) => {
            if !admin { return need_admin(); }
            if app.job.lock().unwrap().is_some() { return fail(409, "Another job is running; try again when it has finished"); }
            let a = app.clone();
            std::thread::spawn(move || {
                if let Err(e) = a.update_imdb_ratings(true) { elog!("IMDb ratings: {e}"); }
            });
            js(202, json!({"ok": true, "started": true}))
        }
        ("GET", ["api", "import", "old-server"]) => {
            if !admin { return need_admin(); }
            let last = std::fs::read_to_string(app.data_dir.join("cache").join("old-server").join("last-import.json")).ok().and_then(|s| serde_json::from_str::<Value>(&s).ok());
            let running = app.job.lock().unwrap().as_ref().is_some_and(|j| j.0.starts_with("Importing"));
            ok(json!({"detected": crate::data::migrate::detect().map(|p| p.to_string_lossy().to_string()), "running": running, "last": last,
                "looked_in": crate::data::migrate::candidates().iter().map(|p| p.to_string_lossy().to_string()).collect::<Vec<_>>()}))
        }
        ("GET", ["api", "import", "old-server", "preview"]) => {
            if !admin { return need_admin(); }
            let src = match ctx.param("db").map(str::trim).filter(|s| !s.is_empty()) {
                Some(p) => crate::data::migrate::find_db(Path::new(p)),
                None => crate::data::migrate::detect(),
            };
            let Some(src) = src else { return fail(404, &format!("No {} found there. Enter the old server's data folder or the file itself.", crate::data::migrate::DB_NAME)) };
            match crate::data::migrate::preview(app, &src) {
                Ok(v) => ok(v),
                Err(e) => fail(400, &e),
            }
        }
        ("POST", ["api", "import", "old-server"]) => {
            if !admin { return need_admin(); }
            let o = match crate::data::migrate::options_from(&ctx.body, user.id) {
                Ok(o) => o,
                Err(e) => return fail(400, &e),
            };
            {
                let mut job = app.job.lock().unwrap();
                if job.is_some() { return fail(409, "Another job is running; try again when it has finished"); }
                *job = Some(("Importing: starting".into(), 0, 6));
                app.job_cancel.store(false, std::sync::atomic::Ordering::SeqCst);
            }
            let a = app.clone();
            std::thread::spawn(move || {
                let r = crate::data::migrate::run(&a, &o);
                *a.job.lock().unwrap() = None;
                a.job_cancel.store(false, std::sync::atomic::Ordering::SeqCst);
                if let Err(e) = &r { elog!("import: {e}"); }
                let mut p = a.meta_progress.lock().unwrap();
                p.done += 1;
                p.last = json!({"id": null, "title": "Import", "ok": r.is_ok(), "message": match &r { Ok(v) => crate::data::migrate::summary(v), Err(e) if e == crate::data::migrate::CANCELLED => "Import cancelled (what was done so far is kept; import again to continue)".to_string(), Err(e) => format!("Import failed: {e}") }, "at": now()});
            });
            js(202, json!({"ok": true, "started": true}))
        }
        ("GET", ["api", "plex-sync"]) => {
            if !admin { return need_admin(); }
            ok(crate::connect::plexsync::public(app))
        }
        ("PUT", ["api", "plex-sync"]) => {
            if !admin { return need_admin(); }
            match crate::connect::plexsync::update(app, &ctx.body) { Ok(v) => ok(v), Err(e) => fail(400, &e) }
        }
        ("POST", ["api", "plex-sync", "test"]) => {
            if !admin { return need_admin(); }
            match crate::connect::plexsync::test(app, &ctx.body) { Ok(v) => ok(v), Err(e) => fail(400, &e) }
        }
        ("POST", ["api", "plex-sync", "run"]) => {
            if !admin { return need_admin(); }
            match crate::connect::plexsync::start_now(app) { Ok(()) => js(202, json!({"ok": true, "started": true})), Err(e) => fail(409, e) }
        }
        ("GET", ["api", "trakt"]) => ok(crate::connect::trakt::public(app, &user)),
        ("PUT", ["api", "trakt"]) => {
            if !admin { return need_admin(); }
            match crate::connect::trakt::update(app, &ctx.body) { Ok(()) => ok(crate::connect::trakt::public(app, &user)), Err(e) => fail(400, &e) }
        }
        ("POST", ["api", "trakt", "connect"]) => match crate::connect::trakt::connect(app, &user) { Ok(v) => ok(v), Err(e) => fail(400, &e) },
        ("POST", ["api", "trakt", "poll"]) => match crate::connect::trakt::poll(app, &user) { Ok(v) => ok(v), Err(e) => fail(400, &e) },
        ("POST", ["api", "trakt", "disconnect"]) => match crate::connect::trakt::disconnect(app, &user) { Ok(()) => ok(json!({"ok": true})), Err(e) => fail(400, &e) },
        ("POST", ["api", "trakt", "sync"]) => {
            if crate::connect::trakt::sync_now(app, user.id) { js(202, json!({"ok": true, "started": true})) } else { fail(409, "A sync with Trakt is already running") }
        }
        ("GET", ["api", "activity"]) => {
            if !user.admin { return fail(403, "Only an admin can do this"); }
            const SHOWN: usize = 25;
            let job = app.job.lock().unwrap().clone();
            let (jl, names) = app.job_items.lock().unwrap().clone();
            let job_json = job.map(|(label, done, total)| {
                // Optimizing counts in hundredths of a video.
                let (done_n, total_n) = if label == "Optimizing" { (done / 100, total / 100) } else { (done, total) };
                let rest: Vec<&String> = if jl == label { names.iter().skip(done_n).collect() } else { vec![] };
                json!({"label": label, "done": done, "total": total, "items": rest.iter().take(SHOWN).collect::<Vec<_>>(), "more": rest.len().saturating_sub(SHOWN),
                    "step": done_n, "steps": total_n})
            });
            let queue: Vec<u64> = app.meta_queue.lock().unwrap().iter().map(|q| q.0).collect();
            let working: Vec<u64> = app.meta_busy.lock().unwrap().clone();
            let scanning_ids = app.scanning.lock().unwrap().clone();
            let credits = app.credits_pending();
            let intros = app.intros_pending();
            let db = app.db.lock().unwrap();
            let name_of = |ids: &[u64]| -> Vec<String> { ids.iter().take(SHOWN).map(|i| App::label_of(&db, *i)).collect() };
            let examples = |ids: &[u64]| -> Vec<String> { ids.iter().take(8).map(|i| App::label_of(&db, *i)).collect() };
            ok(json!({
                "job": job_json,
                "scanning": db.sections.iter().filter(|s| scanning_ids.contains(&s.id)).map(|s| s.name.clone()).collect::<Vec<_>>(),
                "details": {"working": name_of(&working), "queued": name_of(&queue), "more": queue.len().saturating_sub(SHOWN), "total": queue.len(),
                    "enabled": db.settings.metadata_enabled},
                "next": {
                    "credits": {"n": credits.len(), "mode": db.settings.credits_markers_behaviour, "examples": examples(&credits)},
                    "intros": {"n": intros.len(), "mode": db.settings.intro_markers_behaviour, "examples": examples(&intros)},
                },
                "last": app.meta_progress.lock().unwrap().last.clone(),
            }))
        }
        ("POST", ["api", "job", "cancel"]) => {
            if !user.admin { return fail(403, "Only an admin can do this"); }
            if !app.cancel_job() { return fail(409, "Nothing is running that can be cancelled"); }
            ok(json!({"ok": true}))
        }
        ("GET", ["api", "metadata", "status"]) => {
            let q = app.meta_queue.lock().unwrap().len();
            let busy_all: Vec<u64> = app.meta_busy.lock().unwrap().clone();
            let busy = busy_all.first().copied().unwrap_or(0);
            let busy_title = if busy == 0 { None } else { app.db.lock().unwrap().items.get(&busy).map(|i| i.title.clone()) };
            let p = app.meta_progress.lock().unwrap();
            let job = app.job.lock().unwrap().clone();
            // Libraries being scanned right now (by name), so the web app can show it.
            let scanning_ids = app.scanning.lock().unwrap().clone();
            let scanning: Vec<String> = if scanning_ids.is_empty() { vec![] } else {
                let db = app.db.lock().unwrap();
                db.sections.iter().filter(|s| scanning_ids.contains(&s.id) && db.can_see_section(&user, s.id)).map(|s| s.name.clone()).collect()
            };
            // How far each scan is: what it is doing, how many files of how many, and the file it is on.
            let scan_progress: Vec<Value> = if scanning_ids.is_empty() { vec![] } else {
                let db = app.db.lock().unwrap();
                db.sections.iter().filter(|s| scanning_ids.contains(&s.id) && db.can_see_section(&user, s.id))
                    .filter_map(|s| crate::library::scanner::progress_of(s.id).map(|p| json!({"name": s.name, "phase": p.phase, "done": p.done, "total": p.total, "current": p.current})))
                    .collect()
            };
            ok(json!({"queued": q, "scanning": scanning, "scan_progress": scan_progress, "working_on": if busy == 0 { Value::Null } else { json!(busy) }, "working_title": busy_title, "working_ids": busy_all,
                "done": p.done, "failed": p.failed, "last": p.last,
                "job": job.map(|(label, done, total)| json!({"label": label, "done": done, "total": total})),
                // For the owner's "playing now" button in the top bar.
                "playing": if user.admin { json!(app.stream_count()) } else { Value::Null }}))
        }
        _ => { *hit = false; fail(404, "") }
    })
}

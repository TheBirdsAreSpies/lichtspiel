//! Watched state, resume points, ratings, favourites, the watchlist and history.

use super::*;

pub(super) fn routes(app: &Arc<App>, ctx: &Ctx, user: &db::User, m: &str, segs: &[&str]) -> Option<Resp> {
    let admin = user.admin;
    handled(|hit| match (m, segs) {
        (_, [":", "timeline"]) => {
            let key = ctx.body_or_param("ratingKey").or(ctx.body_or_param("key"));
            let Some(iid) = key.and_then(|k| k.rsplit('/').next().and_then(|x| x.parse::<u64>().ok())) else { return fail(400, "ratingKey is needed") };
            // A merged version counts for the item it was merged into.
            let iid = app.db.lock().unwrap().items.get(&iid).and_then(|i| i.version_of()).unwrap_or(iid);
            let time = ctx.body_or_param("time").and_then(|t| t.parse::<f64>().ok()).unwrap_or(0.0).max(0.0) as u64;
            let state = ctx.body_or_param("state").unwrap_or_else(|| "playing".into());
            timeline(app, &user, iid, time, &state, ctx.body_or_param("client").unwrap_or_default(), ctx.viewer())
        }
        (_, [":", "scrobble"]) | (_, [":", "unscrobble"]) => {
            // One item, or several at once: key=1,2,3 (the library's multiple selection).
            let key = ctx.body_or_param("key").or(ctx.body_or_param("ratingKey")).unwrap_or_default();
            let mut ids: Vec<u64> = key.split(',').filter_map(|k| k.trim().parse::<u64>().ok()).collect();
            if ids.is_empty() { return fail(400, "key is needed") }
            let watched = segs[1] == "scrobble";
            let mut db = app.db.lock().unwrap();
            // A merged version counts for the item it was merged into.
            for i in ids.iter_mut() { if let Some(p) = db.items.get(i).and_then(|x| x.version_of()) { *i = p; } }
            if ids.iter().any(|i| !db.items.get(i).is_some_and(|it| db.can_see_item(&user, it))) { return fail(404, "Not found"); }
            for leaf in ids.iter().flat_map(|i| db.leaves(*i)).collect::<Vec<_>>() {
                let v = db.view_mut(user.id, leaf);
                let was = v.view_count;
                if watched {
                    v.view_count = v.view_count.max(1);
                    v.last_viewed_at = now();
                } else {
                    v.view_count = 0;
                }
                v.view_offset_ms = 0;
                v.finished = false;
                // Marked by hand: in the history once, not again for something already watched.
                if watched && was == 0 { db.record_play(user.id, leaf, true); }
            }
            drop(db);
            app.save();
            if watched { for iid in ids { app.fire("playback.watched", json!({"id": iid, "user": user.username, "manual": true})); } }
            ok(json!({"ok": true}))
        }
        (_, [":", "rate"]) => {
            let Some(iid) = ctx.body_or_param("key").and_then(|k| k.parse::<u64>().ok()) else { return fail(400, "key is needed") };
            let rating = ctx.body_or_param("rating").and_then(|r| r.parse::<f32>().ok()).filter(|r| *r >= 0.0);
            let mut db = app.db.lock().unwrap();
            if !db.items.get(&iid).is_some_and(|it| db.can_see_item(&user, it)) { return fail(404, "Not found"); }
            db.view_mut(user.id, iid).rating = rating.map(|r| r.min(10.0));
            drop(db);
            app.save();
            ok(json!({"ok": true}))
        }
        ("PUT", ["api", "items", iid, "favorite"]) | ("POST", ["api", "items", iid, "favorite"]) => {
            let Some(iid) = id(iid) else { return fail(404, "Not found") };
            let fav = ctx.body.get("favorite").and_then(|v| v.as_bool()).or(ctx.param("favorite").map(|v| v == "1" || v == "true")).unwrap_or(true);
            let mut db = app.db.lock().unwrap();
            if !db.items.get(&iid).is_some_and(|it| db.can_see_item(&user, it)) { return fail(404, "Not found"); }
            db.view_mut(user.id, iid).favorite = fav;
            drop(db);
            app.save();
            ok(json!({"ok": true, "favorite": fav}))
        }
        ("PUT", ["api", "items", iid, "watchlist"]) | ("POST", ["api", "items", iid, "watchlist"]) => {
            let Some(iid) = id(iid) else { return fail(404, "Not found") };
            let on = ctx.body.get("watchlist").and_then(|v| v.as_bool()).or(ctx.param("watchlist").map(|v| v == "1" || v == "true")).unwrap_or(true);
            let mut db = app.db.lock().unwrap();
            if let Err(r) = crate::api::dashboard::set_watchlist(&mut db, &user, &[iid], on) { return r; }
            drop(db);
            app.save();
            ok(json!({"ok": true, "watchlist": on}))
        }
        ("POST", ["api", "continue", "remove"]) => {
            // Take titles off Continue Watching without marking them watched {ids: [..]}.
            let ids: Vec<u64> = ctx.body.get("ids").and_then(|v| v.as_array()).map(|a| a.iter().filter_map(|x| x.as_u64().or(x.as_str().and_then(|s| s.parse().ok()))).collect()).unwrap_or_default();
            if ids.is_empty() { return fail(400, "ids are needed") }
            let mut db = app.db.lock().unwrap();
            if ids.iter().any(|i| !db.items.get(i).is_some_and(|it| db.can_see_item(&user, it))) { return fail(404, "Not found"); }
            for leaf in ids.iter().flat_map(|i| db.leaves(*i)).collect::<Vec<_>>() {
                db.view_mut(user.id, leaf).cw_hidden_at = now();
            }
            drop(db);
            app.save();
            ok(json!({"ok": true}))
        }
        ("GET", ["library", "metadata", iid, "theme"]) => crate::api::handlers::theme(app, ctx, &user, iid),
        ("POST", ["library", "metadata", iid, "theme"]) => {
            if !admin { return need_admin(); }
            let Some(iid) = id(iid) else { return fail(404, "Not found") };
            match crate::admin::maintenance::cut_show_theme(app, iid) {
                Ok(()) => {
                    // A theme file in the show's folder is played instead of the one cut.
                    let db = app.db.lock().unwrap();
                    let folder = db.items.get(&iid).and_then(|it| crate::api::handlers::theme_path(&db, it)).map(|p| p.to_string_lossy().to_string());
                    ok(json!({"ok": true, "folder_theme": folder}))
                }
                Err(e) => fail(400, &e),
            }
        }
        ("GET", ["api", "preroll"]) => crate::api::handlers::preroll(app, ctx),
        ("GET", ["api", "preroll", n, "file"]) => crate::api::handlers::preroll_file(app, ctx, n),
        ("GET", ["api", "rss"]) | ("POST", ["api", "rss"]) => crate::connect::rss::manage(app, ctx, user.id),
        ("GET", ["api", "watchlist"]) => {
            let db = app.db.lock().unwrap();
            let ids = crate::api::dashboard::watchlist_ids(&db, &user, ctx.param("sort").unwrap_or(""), ctx.param("type").and_then(ItemKind::parse));
            list_resp(&db, &user, &narrow(&db, &user, ids, ctx), "watchlist")
        }
        // The filters of the library menu, with the values found on the watchlist.
        ("GET", ["api", "watchlist", "filters"]) => {
            let db = app.db.lock().unwrap();
            let kind = ctx.param("type").and_then(ItemKind::parse);
            let ids = crate::api::dashboard::watchlist_ids(&db, &user, "", kind);
            let items: Vec<&db::Item> = ids.iter().filter_map(|i| db.items.get(i)).collect();
            let list = query::section_filters(&db, user.id, &items, kind.unwrap_or(ItemKind::Movie), user.admin, matches!(ctx.param("includeAdvanced"), Some("1") | Some("true")));
            ok(json!({"MediaContainer": {"size": list.len(), "Filter": list}}))
        }
        ("POST", ["api", "watchlist"]) => {
            let ids: Vec<u64> = ctx.body.get("ids").and_then(|v| v.as_array()).map(|a| a.iter().filter_map(|x| x.as_u64().or(x.as_str().and_then(|s| s.parse().ok()))).collect()).unwrap_or_default();
            if ids.is_empty() { return fail(400, "ids are needed"); }
            let on = ctx.body.get("watchlist").and_then(|v| v.as_bool()).unwrap_or(true);
            let mut db = app.db.lock().unwrap();
            let changed = match crate::api::dashboard::set_watchlist(&mut db, &user, &ids, on) { Ok(n) => n, Err(r) => return r };
            drop(db);
            app.save();
            ok(json!({"ok": true, "changed": changed}))
        }
        ("GET", ["api", "downloads"]) => crate::playback::downloads::list(app, ctx, &user),
        ("POST", ["api", "downloads"]) => {
            let ids: Vec<u64> = ctx.body.get("ids").and_then(|v| v.as_array()).map(|a| a.iter().filter_map(|x| x.as_u64().or(x.as_str().and_then(|s| s.parse().ok()))).collect()).unwrap_or_default();
            if ids.is_empty() { return fail(400, "ids are needed"); }
            let q = ctx.body_or_param("quality").unwrap_or_else(|| "original".into());
            crate::playback::downloads::add(app, &user, &ids, &q)
        }
        ("DELETE", ["api", "downloads"]) => crate::playback::downloads::remove_many(app, ctx, &user),
        ("GET", ["api", "downloads", did, "file"]) | ("GET", ["api", "downloads", did, "file", _]) => match id(did) { Some(d) => crate::playback::downloads::file(app, ctx, &user, d), None => fail(404, "No such download") },
        ("POST", ["api", "downloads", did, "retry"]) => match id(did) { Some(d) => crate::playback::downloads::retry(app, &user, d), None => fail(404, "No such download") },
        ("DELETE", ["api", "downloads", did]) => match id(did) { Some(d) => crate::playback::downloads::remove(app, &user, d), None => fail(404, "No such download") },
        ("GET", ["api", "log"]) => {
            if !admin { return need_admin(); }
            ok(json!({"lines": crate::admin::log::lines().into_iter().map(|(t, m)| json!({"t": t, "msg": m})).collect::<Vec<_>>()}))
        }
        ("GET", ["api", "log", "download"]) => {
            if !admin { return need_admin(); }
            let text = crate::admin::log::export_text().into_bytes();
            let gz = ctx.param("gz") == Some("1");
            let (body, ctype, name) = if gz {
                use std::io::Write;
                let mut e = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
                let _ = e.write_all(&text);
                (e.finish().unwrap_or_default(), "application/gzip", "lichtspiel-server-log.txt.gz")
            } else { (text, "text/plain; charset=utf-8", "lichtspiel-server-log.txt") };
            let mut r = crate::server::bytes(200, ctype, body);
            r.add_header(crate::server::header("Content-Disposition", &crate::api::handlers::attachment(name)));
            r
        }
        ("GET", ["api", "maintenance", "backups"]) => { if !admin { return need_admin(); } ok(json!({"backups": crate::admin::maintenance::backups(app).into_iter().map(|(n, s, t)| json!({"name": n, "size": s, "time": t})).collect::<Vec<_>>()})) }
        ("POST", ["api", "maintenance", "restore"]) => {
            if !admin { return need_admin(); }
            let name = ctx.body_or_param("name").unwrap_or_default();
            match crate::admin::maintenance::restore(app, &name) { Ok(()) => ok(json!({"ok": true})), Err(e) => fail(400, &e) }
        }
        ("GET", ["api", "optimized"]) => { if !admin { return need_admin(); } ok(crate::api::actions::optimized_list(app)) }
        ("DELETE", ["api", "optimized"]) => { if !admin { return need_admin(); } ok(json!({"ok": true, "removed": crate::api::actions::remove_all_optimized(app)})) }
        ("POST", ["api", "maintenance", "optimize"]) => { if !admin { return need_admin(); } crate::admin::maintenance::optimize(app) }
        ("POST", ["api", "maintenance", "cleanup"]) => { if !admin { return need_admin(); } crate::admin::maintenance::cleanup(app) }
        ("POST", ["api", "maintenance", "themes"]) => {
            if !admin { return need_admin(); }
            match crate::admin::maintenance::start_intro_themes(app) { Ok(n) => js(if n > 0 { 202 } else { 200 }, json!({"ok": true, "started": n > 0, "shows": n})), Err(e) => fail(409, &e) }
        }
        ("POST", ["api", "maintenance", "backup"]) => { if !admin { return need_admin(); } match crate::admin::maintenance::backup(app) { Ok(p) => ok(json!({"ok": true, "path": p})), Err(e) => fail(500, &e) } }
        ("GET", ["api", "maintenance", "database"]) => { if !admin { return need_admin(); } crate::admin::maintenance::database(app) }
        ("GET", ["api", "maintenance", "report"]) => { if !admin { return need_admin(); } crate::admin::maintenance::report(app) }
        ("GET", ["api", "dashboard"]) => {
            if !user.admin { return fail(403, "Only an admin can do this"); }
            crate::api::dashboard::dashboard(app)
        }
        ("GET", ["api", "dashboard", "stats"]) => {
            if !user.admin { return fail(403, "Only an admin can do this"); }
            crate::api::dashboard::stats_range(app, ctx.param("range").unwrap_or("realtime"))
        }
        ("POST", ["api", "dashboard", "stop"]) => {
            if !admin { return need_admin(); }
            let (Some(uid), Some(iid)) = (ctx.body["user"].as_u64(), ctx.body["item"].as_u64()) else { return fail(400, "user and item are needed") };
            let message: String = ctx.body["message"].as_str().unwrap_or("").chars().take(500).collect();
            let n = app.stop_playback(uid, iid, &message);
            ok(json!({"ok": true, "stopped": n}))
        }
        ("GET", ["api", "dashboard", "history"]) => {
            if !user.admin { return fail(403, "Only an admin can do this"); }
            // Plays still going count too (with the time so far).
            let open: Vec<crate::data::db::PlaySession> = app.playing.lock().unwrap().values().filter(|p| p.session_at > 0).map(|p| p.session()).collect();
            let db = app.db.lock().unwrap();
            crate::api::dashboard::history(&db, &open, ctx.num("days").unwrap_or(30), ctx.num("user"))
        }
        _ => { *hit = false; fail(404, "") }
    })
}

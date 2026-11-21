//! One movie, show or episode: details, editing, pictures, matching, markers, merging, reports and watching together.

use super::*;

pub(super) fn routes(app: &Arc<App>, ctx: &Ctx, user: &db::User, m: &str, segs: &[&str]) -> Option<Resp> {
    let admin = user.admin;
    handled(|hit| match (m, segs) {
        ("GET", ["library", "metadata", iid]) => with_item(app, &user, iid, |db, it| ok(container(vec![item_json(db, &user, it, true)], 1, 0, json!({})))),
        ("PUT", ["library", "metadata", iid]) | ("PATCH", ["library", "metadata", iid]) => {
            if !admin { return need_admin(); }
            let Some(iid) = id(iid) else { return fail(404, "Not found") };
            let mut body = if ctx.body.is_object() { ctx.body.clone() } else { Value::Object(ctx.params.iter().map(|(k, v)| (k.clone(), json!(v))).collect()) };
            if let Some(g) = body.get("genres").and_then(|g| g.as_str()).map(String::from) {
                body["genres"] = json!(g.split(',').map(|s| s.trim()).filter(|s| !s.is_empty()).collect::<Vec<_>>());
            }
            // Labels, collections and edition are kept with the other details.
            for k in ["labels", "collections", "edition", "directors", "writers", "producers", "countries", "original_title", "originally_available", "language_override", "credits_detection", "use_original_title"] {
                let Some(v) = body.get(k).cloned() else { continue };
                let v = match (k, v) {
                    ("edition" | "original_title" | "originally_available", v) => json!(v.as_str().unwrap_or("").trim()),
                    ("language_override", v) => {
                        let l = v.as_str().unwrap_or("").trim().to_string();
                        if !l.is_empty() && !db::LANGUAGES.iter().any(|x| x.0 == l) { return fail(400, "Unknown language"); }
                        json!(l)
                    }
                    ("use_original_title", v) => json!(match v.as_str().unwrap_or("") { "yes" => "yes", "no" => "no", _ => "" }),
                    ("credits_detection", v) => json!(match v.as_str().unwrap_or("") { "on" => "on", "off" => "off", _ => "" }),
                    (_, Value::String(t)) => json!(t.split(',').map(|s| s.trim()).filter(|s| !s.is_empty()).collect::<Vec<_>>()),
                    (_, v) => v,
                };
                if !body["extra"].is_object() { body["extra"] = json!({}); }
                body["extra"][k] = v;
            }
            let mut db = app.db.lock().unwrap();
            let Some(it) = db.items.get_mut(&iid) else { return fail(404, "Not found") };
            if body.get("unlock").and_then(|v| v.as_bool()) == Some(true) { it.locked.clear(); }
            let old_title = it.title.clone();
            // What was changed by hand is locked: a refresh from online keeps it.
            let changed: Vec<String> = ["original_title", "originally_available", "directors", "writers", "producers", "countries"].into_iter()
                .filter(|k| body["extra"].get(*k).is_some_and(|v| {
                    let old = it.extra.get(*k).cloned().unwrap_or(Value::Null);
                    old != *v && !(old.is_null() && (v == &json!("") || v == &json!([])))
                }))
                .map(String::from).collect();
            apply_metadata(it, &body, true);
            for k in changed { if !it.locked.contains(&k) { it.locked.push(k); } }
            if body["extra"].get("use_original_title").is_some() && it.title == old_title { use_original_title(it); }
            drop(db);
            app.save();
            with_item(app, &user, &iid.to_string(), |db, it| ok(item_json(db, &user, it, true)))
        }
        ("GET", ["library", "metadata", iid, "children"]) => with_item(app, &user, iid, |db, it| {
            let ix = Index::build(db);
            let list: Vec<Value> = db.children(it.id).iter().map(|c| item_json_ix(db, &user, c, false, Some(&ix))).collect();
            let n = list.len();
            ok(container(list, n, 0, json!({"parentTitle": it.title, "parentRatingKey": it.id.to_string()})))
        }),
        ("GET", ["library", "metadata", iid, "allLeaves"]) => with_item(app, &user, iid, |db, it| {
            let mut ids = db.leaves(it.id);
            query::sort_items(db, user.id, &mut ids, ctx.param("sort").unwrap_or("episode_order"));
            let ix = Index::build(db);
            let list: Vec<Value> = ids.iter().filter_map(|i| db.items.get(i)).map(|c| item_json_ix(db, &user, c, false, Some(&ix))).collect();
            let n = list.len();
            ok(container(list, n, 0, json!({})))
        }),
        ("GET", ["library", "metadata", iid, "thumb"]) | ("GET", ["library", "metadata", iid, "thumb", _]) => thumb(app, &user, iid, ctx.num("w").map(|w| w.min(4000) as u32)),
        ("GET", ["library", "metadata", iid, "cast", n]) => cast_photo(app, &user, iid, n),
        ("GET", ["library", "metadata", iid, slot @ ("logo" | "square")]) => custom_image(app, &user, iid, slot),
        ("PUT", ["library", "metadata", iid, "image", slot]) => { if !admin { return need_admin(); } set_image(app, iid, slot, &ctx.body) }
        ("DELETE", ["library", "metadata", iid, "image", slot]) => { if !admin { return need_admin(); } clear_image(app, iid, slot) }
        ("GET", ["library", "metadata", iid, "art"]) | ("GET", ["library", "metadata", iid, "art", _]) => art(app, &user, iid),
        ("GET", ["library", "metadata", iid, "matches"]) => {
            if !admin { return need_admin(); }
            let Some(iid) = id(iid) else { return fail(404, "Not found") };
            let (kind, title, year) = {
                let db = app.db.lock().unwrap();
                let Some(it) = db.items.get(&iid) else { return fail(404, "Not found") };
                let file_title = it.extra.get("file_title").and_then(|v| v.as_str()).unwrap_or(&it.title).to_string();
                (it.kind, file_title, it.year)
            };
            if !matches!(kind, ItemKind::Movie | ItemKind::Show) { return fail(400, "Details are looked up for movies and shows"); }
            // A title that is still a file name ("Apocalypse.Now.1979.Final.Cut.1080p… {edition-Final Cut}",
            // also the stored one of an item scanned before) is read like the scanner does:
            // "Apocalypse Now", 1979. Any other typed title is used as it is.
            let typed = ctx.param("title").filter(|t| !t.trim().is_empty()).map(String::from);
            let (title, parsed_year) = match typed {
                Some(t) if !t.contains(['.', '{', '[']) => (t, None),
                other => crate::library::scanner::parse_title_year(&other.unwrap_or(title)),
            };
            // An empty year means "no year" (but the year read from a file name is kept); no parameter means the item's own.
            let year = match ctx.param("year").map(str::trim) { Some("") => parsed_year, Some(y) => y.parse().ok(), None => parsed_year.or(year) };
            match crate::library::metadata::search(&app.settings(), kind, &title, year) {
                Ok(c) => ok(json!({"title": title, "year": year, "matches": c})),
                Err(e) => fail(502, &e),
            }
        }
        ("PUT", ["library", "metadata", iid, "match"]) | ("POST", ["library", "metadata", iid, "match"]) => {
            if !admin { return need_admin(); }
            let Some(iid) = id(iid) else { return fail(404, "Not found") };
            let source = ctx.body["source"].as_str().or(ctx.param("source")).unwrap_or("").to_string();
            let mid = ctx.body["id"].as_str().map(String::from).or_else(|| ctx.body["id"].as_u64().map(|n| n.to_string())).or(ctx.param("id").map(String::from)).unwrap_or_default();
            if source.is_empty() || mid.is_empty() { return fail(400, "Send {source, id} from /matches"); }
            match app.match_online(iid, Some((source, mid)), true) {
                Ok(msg) => {
                    app.save();
                    with_item(app, &user, &iid.to_string(), |db, it| ok(json!({"ok": true, "message": msg, "item": item_json(db, &user, it, true)})))
                }
                Err(e) => fail(502, &e),
            }
        }
        ("DELETE", ["library", "metadata", iid, "match"]) | ("POST", ["library", "metadata", iid, "unmatch"]) | ("PUT", ["library", "metadata", iid, "unmatch"]) => {
            if !admin { return need_admin(); }
            let Some(iid) = id(iid) else { return fail(404, "Not found") };
            match app.unmatch(iid) {
                Ok(msg) => {
                    app.save();
                    with_item(app, &user, &iid.to_string(), |db, it| ok(json!({"ok": true, "message": msg, "item": item_json(db, &user, it, true)})))
                }
                Err(e) => fail(400, &e),
            }
        }
        ("GET", ["library", "metadata", iid, "markers"]) => with_item(app, &user, iid, |db, it| {
            let manual = it.extra.contains_key("credits_ms");
            let credits = db.credits_markers(it);
            let at = db::watched_at_ms(db.settings.played_at_behaviour, db.settings.watched_threshold_percent, it.duration_ms, &credits);
            let f = it.last_file();
            let how = f.map(|f| f.credits_source.as_str()).filter(|s| !s.is_empty()).unwrap_or("chapters");
            let checked = f.is_some_and(|f| f.credits_checked >= db::CREDITS_DETECTOR_VERSION);
            let intro = db.intro_marker(it);
            let intro_source = if it.extra.contains_key("intro_ms") { "manual" } else if intro.is_some() { "sound" }
                else if it.file.as_ref().is_some_and(|f| f.intro_checked > 0) { "none" } else { "pending" };
            ok(json!({"credits": credits, "source": if manual { "manual" } else if !credits.is_empty() { how } else if checked { "none" } else { "pending" },
                "watchedAt": at, "duration": it.duration_ms, "errors": f.map(|f| f.credits_errors).unwrap_or(0),
                "intro": intro.map(|(s, e)| json!([s, e])), "introSource": intro_source,
                "introDetection": it.kind == ItemKind::Episode && db.intro_on(it)}))
        }),
        ("PUT", ["library", "metadata", iid, "markers"]) => {
            if !admin { return need_admin(); }
            let Some(iid) = id(iid) else { return fail(404, "Not found") };
            // {"credits": [ms, …]} sets them; {"credits": null} goes back to the chapters.
            // {"intro": [start ms, end ms]} sets the intro; {"intro": null} goes back to the found one.
            let list: Option<Option<Vec<u64>>> = match ctx.body.get("credits") {
                Some(Value::Array(a)) => Some(Some(a.iter().filter_map(|v| v.as_u64()).collect())),
                Some(Value::Null) => Some(None),
                None if ctx.body.get("intro").is_some() => None,
                _ => return fail(400, "Send {\"credits\": [start in ms, …]} or {\"credits\": null}, and/or {\"intro\": [start ms, end ms]} or {\"intro\": null}"),
            };
            let intro: Option<Option<Vec<u64>>> = match ctx.body.get("intro") {
                Some(Value::Array(a)) => {
                    let v: Vec<u64> = a.iter().filter_map(|v| v.as_u64()).collect();
                    if v.len() != 2 || v[1] <= v[0] { return fail(400, "intro must be [start ms, end ms] with the end after the start"); }
                    Some(Some(v))
                }
                Some(Value::Null) => Some(None),
                None => None,
                _ => return fail(400, "intro must be [start ms, end ms] or null"),
            };
            {
                let mut db = app.db.lock().unwrap();
                let Some(it) = db.items.get_mut(&iid) else { return fail(404, "Not found") };
                if !it.kind.playable() { return fail(400, "Only movies and episodes have credits"); }
                match list {
                    Some(Some(l)) => { it.extra.insert("credits_ms".into(), json!(l)); }
                    Some(None) => { it.extra.remove("credits_ms"); }
                    None => {}
                }
                match intro {
                    Some(Some(v)) => { it.extra.insert("intro_ms".into(), json!(v)); }
                    Some(None) => { it.extra.remove("intro_ms"); }
                    None => {}
                }
            }
            app.save();
            with_item(app, &user, &iid.to_string(), |db, it| ok(json!({"ok": true, "credits": db.credits_markers(it), "intro": db.intro_marker(it).map(|(s, e)| json!([s, e]))})))
        }
        ("POST", ["library", "metadata", iid, "refresh"]) | ("PUT", ["library", "metadata", iid, "refresh"]) => {
            if !admin { return need_admin(); }
            let Some(iid) = id(iid) else { return fail(404, "Not found") };
            let top = { let db = app.db.lock().unwrap(); App::top_level(&db, iid) };
            let Some(top) = top else { return fail(404, "Not found") };
            match app.match_online(top, None, true) {
                Ok(msg) => {
                    app.save();
                    with_item(app, &user, &iid.to_string(), |db, it| ok(json!({"ok": true, "message": msg, "item": item_json(db, &user, it, true)})))
                }
                Err(e) => fail(502, &e),
            }
        }
        ("GET", ["library", "metadata", iid, "history"]) => match id(iid) { Some(i) => crate::api::actions::history(app, &user, i), None => fail(404, "Not found") },
        ("POST", ["library", "metadata", iid, "report"]) => match id(iid) { Some(i) => crate::api::actions::report_problem(app, ctx, &user, i), None => fail(404, "Not found") },
        ("POST", ["api", "merge"]) => {
            if !admin { return need_admin(); }
            let ids: Vec<u64> = ctx.body["ids"].as_array().map(|a| a.iter().filter_map(|v| v.as_u64().or_else(|| v.as_str().and_then(|s| s.parse().ok()))).collect()).unwrap_or_default();
            let mut db = app.db.lock().unwrap();
            let Some(&main) = ids.first() else { return fail(400, "ids is needed") };
            if ids.len() < 2 { return fail(400, "Pick at least two to merge"); }
            let Some(m) = db.items.get(&main) else { return fail(404, "Not found") };
            if m.version_of().is_some() || m.is_extra() || !matches!(m.kind, ItemKind::Movie | ItemKind::Episode) || m.file.is_none() { return fail(400, "That can't be merged into"); }
            let kind = m.kind;
            let mut others = vec![];
            for i in &ids[1..] {
                if *i == main { continue; }
                let Some(o) = db.items.get(i) else { return fail(404, "Not found") };
                if o.kind != kind || o.is_extra() || o.file.is_none() || o.version_of().is_some() { return fail(400, "Only movies with movies and episodes with episodes can be merged"); }
                others.push(*i);
            }
            // What the others had as versions moves along.
            let moved: Vec<u64> = db.items.values().filter(|x| x.version_of().is_some_and(|p| others.contains(&p))).map(|x| x.id).collect();
            for i in others.iter().chain(moved.iter()) {
                if let Some(o) = db.items.get_mut(i) { o.extra.insert("version_of".into(), json!(main)); }
            }
            drop(db);
            app.save();
            ok(json!({"merged": others.len() + moved.len(), "id": main}))
        }
        ("POST", ["library", "metadata", iid, "split"]) => {
            if !admin { return need_admin(); }
            let Some(iid) = id(iid) else { return fail(404, "Not found") };
            let only = ctx.num("version");
            let mut db = app.db.lock().unwrap();
            let vs: Vec<u64> = db.items.values().filter(|x| x.version_of() == Some(iid) && only.is_none_or(|o| o == x.id)).map(|x| x.id).collect();
            // "no_merge": the next scan must not put them back together.
            for v in &vs { if let Some(o) = db.items.get_mut(v) { o.extra.remove("version_of"); o.extra.insert("no_merge".into(), json!(true)); } }
            drop(db);
            app.save();
            ok(json!({"split": vs.len()}))
        }
        ("DELETE", ["library", "metadata", iid]) => match id(iid) { Some(i) => crate::api::actions::delete_media(app, &user, i), None => fail(404, "Not found") },
        ("POST", ["library", "metadata", iid, "deepanalyze"]) => {
            if !admin { return need_admin(); }
            let Some(iid) = id(iid) else { return fail(404, "Not found") };
            match crate::api::actions::deep_analyze(app, iid) { Ok(n) => ok(json!({"ok": true, "videos": n})), Err(e) => fail(409, &e) }
        }
        ("POST", ["library", "metadata", iid, "analyze"]) => {
            if !admin { return need_admin(); }
            let Some(iid) = id(iid) else { return fail(404, "Not found") };
            match crate::api::actions::analyze(app, iid) { Ok(n) => ok(json!({"ok": true, "videos": n})), Err(e) => fail(409, &e) }
        }
        ("POST", ["library", "metadata", iid, "optimize"]) => {
            if !admin { return need_admin(); }
            let Some(iid) = id(iid) else { return fail(404, "Not found") };
            let q = ctx.body_or_param("quality").unwrap_or_else(|| "tv".into());
            match crate::api::actions::optimize(app, iid, &q) { Ok(n) => ok(json!({"ok": true, "videos": n})), Err(e) => fail(409, &e) }
        }
        ("DELETE", ["library", "metadata", iid, "optimize"]) => {
            if !admin { return need_admin(); }
            let Some(iid) = id(iid) else { return fail(404, "Not found") };
            ok(json!({"ok": true, "removed": crate::api::actions::remove_optimized(app, iid)}))
        }
        ("GET", ["api", "reports"]) => if admin { crate::api::actions::reports(app, ctx) } else { need_admin() },
        ("PUT", ["api", "reports", rid]) | ("PATCH", ["api", "reports", rid]) | ("DELETE", ["api", "reports", rid]) => {
            if !admin { return need_admin(); }
            match id(rid) { Some(r) => crate::api::actions::update_report(app, ctx, r, m == "DELETE"), None => fail(404, "No such report") }
        }
        ("POST", ["api", "together"]) => crate::api::actions::together_create(app, ctx, &user),
        ("GET", ["api", "together", "invites"]) => crate::api::actions::together_invites(app, &user),
        ("GET", ["api", "together", code]) => crate::api::actions::together_get(app, &user, code),
        ("GET", ["api", "together", code, "people"]) => crate::api::actions::together_people(app, &user, code),
        ("POST", ["api", "together", code, "invite"]) => crate::api::actions::together_invite(app, ctx, &user, code),
        ("DELETE", ["api", "together", code, "invite"]) => crate::api::actions::together_decline(&user, code),
        ("POST", ["api", "together", code]) | ("PUT", ["api", "together", code]) => crate::api::actions::together_update(app, ctx, &user, code),
        ("DELETE", ["api", "together", code]) => crate::api::actions::together_leave(&user, code),
        _ => { *hit = false; fail(404, "") }
    })
}

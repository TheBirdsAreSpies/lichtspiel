//! Libraries: adding, changing, removing and scanning them, and the people (actors are shared by the whole library).

use super::*;

pub(super) fn routes(app: &Arc<App>, ctx: &Ctx, user: &db::User, m: &str, segs: &[&str]) -> Option<Resp> {
    let admin = user.admin;
    handled(|hit| match (m, segs) {
        ("GET", ["library", "sections"]) => {
            let db = app.db.lock().unwrap();
            let scanning = app.scanning.lock().unwrap().clone();
            let list: Vec<Value> = db.sections.iter().filter(|s| db.can_see_section(&user, s.id)).map(|s| {
                let mut v = section_json(&db, s);
                v["scanning"] = json!(scanning.contains(&s.id));
                v
            }).collect();
            ok(json!({"MediaContainer": {"size": list.len(), "Directory": list}}))
        }
        ("POST", ["library", "sections"]) => {
            if !admin { return need_admin(); }
            let name = ctx.body_or_param("name").unwrap_or_default();
            let kind = ctx.body_or_param("type").or(ctx.body_or_param("kind")).and_then(|k| SectionKind::parse(&k));
            let Some(kind) = kind else { return fail(400, "type must be movie, show, music or other") };
            let paths = paths_from(ctx);
            if paths.is_empty() { return fail(400, "Give at least one folder in paths"); }
            if let Some(missing) = paths.iter().find(|p| !Path::new(p).is_dir()) { return fail(400, &format!("Folder not found: {missing}")); }
            let mut db = app.db.lock().unwrap();
            let sid = db.new_id();
            let name = if name.trim().is_empty() {
                match kind { SectionKind::Movie => "Movies", SectionKind::Show => "TV Shows", SectionKind::Other => "Videos", SectionKind::Music => "Music" }.to_string()
            } else { name };
            let s = Section { id: sid, name, kind, paths, created_at: now(), scanned_at: 0, credits_detection: true, intro_detection: true, language: ctx.body_or_param("language").unwrap_or_default(), scan_interval_minutes: None };
            let v = section_json(&db, &s);
            db.sections.push(s);
            drop(db);
            app.save();
            if ctx.param("scan") == Some("wait") {
                app.scan_now(sid);
            } else if ctx.param("scan") != Some("0") {
                app.scan_in_background(sid);
            }
            js(201, v)
        }
        ("GET", ["library", "sections", sid]) => {
            let db = app.db.lock().unwrap();
            match id(sid).and_then(|s| db.sections.iter().find(|x| x.id == s)) {
                Some(s) if db.can_see_section(&user, s.id) => {
                    let mut v = section_json(&db, s);
                    v["scanning"] = json!(app.scanning.lock().unwrap().contains(&s.id));
                    ok(v)
                }
                _ => fail(404, "No such library"),
            }
        }
        ("PUT", ["library", "sections", sid]) | ("PATCH", ["library", "sections", sid]) => {
            if !admin { return need_admin(); }
            let paths = paths_from(ctx);
            if let Some(missing) = paths.iter().find(|p| !Path::new(p).is_dir()) { return fail(400, &format!("Folder not found: {missing}")); }
            let mut db = app.db.lock().unwrap();
            let Some(s) = id(sid).and_then(|s| db.sections.iter_mut().find(|x| x.id == s)) else { return fail(404, "No such library") };
            if let Some(n) = ctx.body_or_param("name").filter(|n| !n.trim().is_empty()) { s.name = n; }
            let new_paths = !paths.is_empty();
            if new_paths { s.paths = paths; }
            if let Some(b) = ctx.body.get("credits_detection").and_then(|v| v.as_bool()) { s.credits_detection = b; }
            if let Some(b) = ctx.body.get("intro_detection").and_then(|v| v.as_bool()) { s.intro_detection = b; }
            let interval_set = ctx.body.get("scan_interval_minutes").is_some();
            if let Some(v) = ctx.body.get("scan_interval_minutes") {
                // null = the server's setting, 0 = never, else minutes (at least 5).
                s.scan_interval_minutes = match v.as_u64() { Some(m) if m == 0 || (5..=10080).contains(&m) => Some(m), Some(_) => return fail(400, "scan_interval_minutes must be 0 or 5-10080"), None if v.is_null() => None, None => return fail(400, "scan_interval_minutes must be a number or null") };
            }
            let mut language_changed = false;
            if let Some(l) = ctx.body.get("language").and_then(|v| v.as_str()) {
                let l = l.trim().to_lowercase();
                if !l.is_empty() && !db::LANGUAGES.iter().any(|x| x.0 == l) { return fail(400, "Unknown language"); }
                language_changed = l != s.language;
                s.language = l;
            }
            if language_changed {
                // New language: download this library's details again in it.
                let sid = s.id;
                drop(db);
                app.save();
                if new_paths { app.scan_in_background(sid); }
                let n = app.refetch_details(|sec| sec.id == sid);
                return ok(json!({"ok": true, "id": sid, "refetching": n}));
            }
            if !new_paths && (interval_set || ctx.body.get("credits_detection").is_some() || ctx.body.get("intro_detection").is_some()) {
                // Only a switch changed: no rescan needed.
                let sid = s.id;
                drop(db);
                app.save();
                return ok(json!({"ok": true, "id": sid}));
            }
            let sid = s.id;
            drop(db);
            app.save();
            app.scan_in_background(sid);
            ok(json!({"ok": true, "id": sid}))
        }
        ("DELETE", ["library", "sections", sid]) => {
            if !admin { return need_admin(); }
            let Some(sid) = id(sid) else { return fail(404, "No such library") };
            let mut db = app.db.lock().unwrap();
            db.sections.retain(|s| s.id != sid);
            db.items.retain(|_, i| i.section_id != sid);
            drop(db);
            app.save();
            ok(json!({"ok": true}))
        }
        ("POST", ["library", "sections", sid, "refresh"]) | ("GET", ["library", "sections", sid, "refresh"]) => {
            if !admin { return need_admin(); }
            let Some(sid) = id(sid) else { return fail(404, "No such library") };
            let only = match ctx.param("path").filter(|p| !p.trim().is_empty()) {
                None => None,
                Some(p) => {
                    let p = PathBuf::from(p);
                    let inside = app.db.lock().unwrap().sections.iter().find(|s| s.id == sid)
                        .is_some_and(|s| s.paths.iter().any(|r| p.starts_with(r)) && !p.components().any(|c| matches!(c, std::path::Component::ParentDir)));
                    if !inside { return fail(400, "That folder is not inside the library's folders"); }
                    if !p.is_dir() { return fail(400, "Folder not found"); }
                    Some(p)
                }
            };
            if ctx.param("wait") == Some("1") {
                return match app.scan_folder_now(sid, only) {
                    Some(r) => ok(json!({"added": r.added.len(), "updated": r.updated.len(), "removed": r.removed.len(), "files": r.files_seen})),
                    None => fail(409, "That library is already being scanned"),
                };
            }
            app.scan_folder_in_background(sid, only);
            ok(json!({"ok": true, "scanning": true}))
        }
        ("POST", ["library", "sections", sid, "deepanalyze"]) => {
            if !admin { return need_admin(); }
            let Some(sid) = id(sid) else { return fail(404, "No such library") };
            match crate::api::actions::deep_analyze_section(app, sid) { Ok(n) => ok(json!({"ok": true, "videos": n})), Err(e) => fail(409, &e) }
        }
        ("POST", ["library", "sections", sid, "analyze"]) => {
            if !admin { return need_admin(); }
            let Some(sid) = id(sid) else { return fail(404, "No such library") };
            match crate::api::actions::analyze_section(app, sid) { Ok(n) => ok(json!({"ok": true, "videos": n})), Err(e) => fail(409, &e) }
        }
        ("POST", ["library", "sections", sid, "emptyTrash"]) => {
            if !admin { return need_admin(); }
            let Some(sid) = id(sid) else { return fail(404, "No such library") };
            match crate::api::actions::empty_trash(app, sid) { Ok(n) => ok(json!({"ok": true, "removed": n})), Err(e) => fail(409, &e) }
        }
        ("POST", ["library", "sections", sid, "refreshMetadata"]) => {
            if !admin { return need_admin(); }
            let Some(sid) = id(sid) else { return fail(404, "No such library") };
            let ids: Vec<u64> = app.db.lock().unwrap().items.values().filter(|i| i.section_id == sid && i.parent_id.is_none()).map(|i| i.id).collect();
            app.queue_online_details(&ids, true);
            ok(json!({"ok": true, "queued": ids.len()}))
        }
        ("POST", ["library", "sections", sid, "credits"]) | ("POST", ["library", "metadata", sid, "credits"]) => {
            if !admin { return need_admin(); }
            let Some(n) = id(sid) else { return fail(404, "Not found") };
            let ids: Vec<u64> = if ctx.path.starts_with("/library/sections/") {
                app.db.lock().unwrap().items.values().filter(|i| i.section_id == n && i.parent_id.is_none()).map(|i| i.id).collect()
            } else {
                vec![n]
            };
            match app.detect_credits(ids, true) {
                Ok(total) => ok(json!({"ok": true, "videos": total})),
                Err(e) => fail(409, &e),
            }
        }
        ("POST", ["library", "sections", sid, "intros"]) | ("POST", ["library", "metadata", sid, "intros"]) => {
            if !admin { return need_admin(); }
            let Some(n) = id(sid) else { return fail(404, "Not found") };
            let ids: Vec<u64> = if ctx.path.starts_with("/library/sections/") {
                app.db.lock().unwrap().items.values().filter(|i| i.section_id == n && i.parent_id.is_none()).map(|i| i.id).collect()
            } else {
                vec![n]
            };
            match app.detect_intros(ids, true) {
                Ok(total) => ok(json!({"ok": true, "episodes": total})),
                Err(e) => fail(409, &e),
            }
        }
        // people (actors are shared by the whole library)
        ("GET", ["library", "people", key, "thumb"]) => {
            if key.is_empty() || key.len() > 40 || !key.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') { return fail(404, "Not found"); }
            let p = app.person_photo(key);
            match crate::library::metadata::file_image_type(&p) {
                Some(mime) => with_cache(serve_file(&p, mime, None, false)),
                None => fail(404, "No photo"),
            }
        }
        ("GET", ["library", "people", key, "details"]) => {
            if key.is_empty() || key.len() > 40 || !key.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') { return fail(404, "Not found"); }
            if !app.db.lock().unwrap().settings.meta_person_details { return fail(403, "Details about people are switched off in the settings"); }
            let cache = app.data_dir.join("metadata").join("people").join(format!("{key}.json"));
            let fresh = std::fs::metadata(&cache).and_then(|m| m.modified()).ok().and_then(|t| t.elapsed().ok()).is_some_and(|e| e < std::time::Duration::from_secs(14 * 86400));
            let mut det = None;
            if fresh && ctx.param("refresh") != Some("1") {
                det = std::fs::read_to_string(&cache).ok().and_then(|s| serde_json::from_str::<Value>(&s).ok());
            }
            if det.is_none() {
                let settings = app.db.lock().unwrap().settings.clone();
                match crate::library::metadata::person_details(&settings, key, ctx.param("name").unwrap_or("")) {
                    Ok(d) => {
                        let _ = std::fs::create_dir_all(cache.parent().unwrap());
                        let _ = std::fs::write(&cache, d.to_string());
                        det = Some(d);
                    }
                    Err(e) => match std::fs::read_to_string(&cache).ok().and_then(|s| serde_json::from_str::<Value>(&s).ok()) {
                        Some(old) => det = Some(old),
                        None => return fail(502, &e),
                    },
                }
            }
            let mut det = det.unwrap();
            // Which of the filmography is in your libraries (by TMDB id, else by title and year).
            let db = app.db.lock().unwrap();
            let items: Vec<&db::Item> = db.items.values().filter(|i| matches!(i.kind, ItemKind::Movie | ItemKind::Show) && db.can_see_item(&user, i)).collect();
            if let Some(list) = det["credits"].as_array_mut() {
                for c in list.iter_mut() {
                    let show = c["type"] == "show";
                    let tid = c["tmdb_id"].as_u64().unwrap_or(0).to_string();
                    let title = c["title"].as_str().unwrap_or("").to_lowercase();
                    let year = c["year"].as_u64().map(|y| y as u32);
                    let hit = items.iter().find(|i| (i.kind == ItemKind::Show) == show && i.extra.get("metadata_source").and_then(|v| v.as_str()) == Some("tmdb") && i.extra.get("metadata_id").and_then(|v| v.as_str()) == Some(tid.as_str()))
                        .or_else(|| items.iter().find(|i| (i.kind == ItemKind::Show) == show && !title.is_empty() && i.title.to_lowercase() == title && (year.is_none() || i.year == year)));
                    c["item"] = hit.map(|i| item_json(&db, &user, i, false)).unwrap_or(Value::Null);
                }
            }
            ok(det)
        }
        // A person's list is also at /library/people/{id}/media.
        ("GET", ["library", "people", key]) | ("GET", ["library", "people", key, "media"]) => {
            let db = app.db.lock().unwrap();
            let mut name = ctx.param("name").unwrap_or("").to_string();
            let mut photo = Value::Null;
            // Everything this person is in, matched by their id or, across
            // different details sources, by their name.
            let cast_of = |it: &db::Item| it.extra.get("cast").and_then(|c| c.as_array()).cloned().unwrap_or_default();
            if name.is_empty() {
                for it in db.items.values() {
                    if let Some(c) = cast_of(it).into_iter().find(|c| c["key"].as_str() == Some(*key) || crate::server::person_key(c) == *key) {
                        name = c["name"].as_str().unwrap_or("").to_string();
                        break;
                    }
                }
            }
            let lname = name.to_lowercase();
            let mut found: Vec<(Option<u32>, Value)> = vec![];
            for it in db.items.values().filter(|i| matches!(i.kind, ItemKind::Movie | ItemKind::Show) && db.can_see_item(&user, i)) {
                let cast = cast_of(it);
                let Some(c) = cast.iter().find(|c| c["key"].as_str() == Some(*key) || crate::server::person_key(c) == *key || (!lname.is_empty() && c["name"].as_str().is_some_and(|n| n.to_lowercase() == lname))) else { continue };
                if photo.is_null() && c["photo"].is_string() { photo = c["photo"].clone(); }
                let mut v = item_json(&db, &user, it, false);
                v["role"] = c["role"].clone();
                found.push((it.year, v));
            }
            found.sort_by(|a, b| b.0.cmp(&a.0));
            let has_photo = crate::library::metadata::file_image_type(&app.person_photo(key)).is_some();
            ok(json!({"key": key, "type": "person", "name": name, "media": format!("/library/people/{key}/media"), "thumb": if has_photo { json!(format!("/library/people/{key}/thumb")) } else { Value::Null }, "photo": photo,
                "size": found.len(), "Metadata": found.into_iter().map(|x| x.1).collect::<Vec<_>>()}))
        }
        _ => { *hit = false; fail(404, "") }
    })
}

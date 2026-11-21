//! Plugin endpoints: the list, panels, library and history access, and actions plugins run on the server.

use super::*;

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
            "settings": settings, "folder": pl.dir.to_string_lossy(), "user_panel": pl.manifest.user_panel, "reads_library": pl.manifest.reads_library,
            "placement": plugins::placement(&pl.manifest, &st.placement), "suggested_placement": plugins::placement(&pl.manifest, "")})
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
        .map(|pl| {
            let place = plugins::placement(&pl.manifest, db.plugins.get(&pl.manifest.id).map(|s| s.placement.as_str()).unwrap_or(""));
            json!({"id": pl.manifest.id, "name": pl.manifest.name, "description": pl.manifest.description,
                "settings": place != "sidebar", "sidebar": place != "settings", "admins_only": pl.manifest.panel_admins_only})
        })
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

/// One person's play history for a plugin that reads the library (`?key=…&user=name&since=unix seconds`).
pub fn plugin_history(app: &Arc<App>, ctx: &Ctx, pid: &str) -> Resp {
    let pl = app.plugins.lock().unwrap().get(pid).cloned();
    let enabled = app.db.lock().unwrap().plugins.get(pid).is_some_and(|s| s.enabled);
    let given = ctx.param("key").unwrap_or("");
    if !matches!(pl, Some(pl) if pl.manifest.reads_library && enabled && !given.is_empty() && auth::same(given, &app.plugin_key(pid))) {
        return fail(403, "This plugin may not read the library");
    }
    let since = ctx.param("since").and_then(|s| s.parse::<u64>().ok()).unwrap_or(0);
    match app.play_history(ctx.param("user").unwrap_or(""), since) {
        Some(v) => ok(v),
        None => fail(404, "No such user"),
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

/// A plugin's `{"collections": {"IMDb Top 250": [ids], …}}`: each named collection holds exactly
/// those titles afterwards — added where missing, taken off titles the plugin put in it before
/// (`auto_collections`) that are no longer listed. A collection given by hand stays.
/// Gives (added, removed), or None when the answer has no collections.
pub fn set_plugin_collections(app: &Arc<App>, v: &Value) -> Option<(usize, usize)> {
    let map = v.get("collections")?.as_object()?;
    let want: Vec<(String, std::collections::HashSet<u64>)> = map.iter().map(|(n, ids)| (n.trim().to_string(), ids)).filter(|(n, _)| !n.is_empty() && n.len() <= 200)
        .map(|(n, ids)| (n, ids.as_array().map(|a| a.iter().filter_map(|x| x.as_u64()).collect()).unwrap_or_default())).collect();
    let names = |v: Option<&Value>| -> Vec<String> { v.and_then(|v| v.as_array()).map(|a| a.iter().filter_map(|s| s.as_str().map(String::from)).collect()).unwrap_or_default() };
    let (mut added, mut removed) = (0, 0);
    let mut db = app.db.lock().unwrap();
    let ids: Vec<u64> = db.items.keys().copied().collect();
    for id in ids {
        let Some(it) = db.items.get(&id) else { continue };
        let (mut have, mut auto) = (names(it.extra.get("collections")), names(it.extra.get("auto_collections")));
        let mut changed = false;
        for (name, members) in &want {
            let is_in = members.contains(&id);
            if is_in && !auto.contains(name) && !have.contains(name) {
                // (A title already in it by hand stays the owner's, so it is never taken off.)
                auto.push(name.clone());
                have.push(name.clone());
                added += 1;
                changed = true;
            } else if !is_in && auto.contains(name) {
                auto.retain(|a| a != name);
                have.retain(|a| a != name);
                removed += 1;
                changed = true;
            }
        }
        if !changed { continue; }
        let Some(it) = db.items.get_mut(&id) else { continue };
        it.extra.insert("collections".into(), json!(have));
        if auto.is_empty() { it.extra.remove("auto_collections"); } else { it.extra.insert("auto_collections".into(), json!(auto)); }
    }
    drop(db);
    if added + removed > 0 { app.save(); }
    Some((added, removed))
}

/// What a plugin brings in for whoever pressed its button (an import from elsewhere):
/// `set_ratings: [{"id", "rating" 0-10}]` (only where the person has no rating yet),
/// `add_to_watchlist: [ids]` and `save_playlist: {"title", "summary", "ids"}` (their own
/// playlist of that title is filled anew, else a new one is made). Titles the person cannot
/// see are left out. Each is replaced in the answer by its count (`rated`, `watchlisted`,
/// `playlist_id` and `playlist_items`).
pub fn apply_plugin_user_changes(app: &Arc<App>, user: &User, v: &mut Value) -> Option<String> {
    let o = v.as_object_mut()?;
    let mut done = vec![];
    let mut changed = false;
    let mut db = app.db.lock().unwrap();
    let visible = |db: &Db, id: u64| db.items.get(&id).is_some_and(|it| db.can_see_item(user, it));
    if let Some(list) = o.remove("set_ratings") {
        let mut rated = 0;
        for e in list.as_array().into_iter().flatten().take(20_000) {
            let (Some(id), Some(r)) = (e["id"].as_u64(), e["rating"].as_f64()) else { continue };
            if !(0.0..=10.0).contains(&r) || !visible(&db, id) || db.view(user.id, id).rating.is_some() { continue; }
            db.view_mut(user.id, id).rating = Some(r as f32);
            rated += 1;
        }
        o.insert("rated".into(), json!(rated));
        done.push(format!("{rated} rated"));
        changed |= rated > 0;
    }
    if let Some(list) = o.remove("add_to_watchlist") {
        let ids: Vec<u64> = list.as_array().into_iter().flatten().filter_map(|x| x.as_u64()).filter(|id| visible(&db, *id)).take(20_000).collect();
        let n = crate::api::dashboard::set_watchlist(&mut db, user, &ids, true).unwrap_or(0);
        o.insert("watchlisted".into(), json!(n));
        done.push(format!("{n} on the watchlist"));
        changed |= n > 0;
    }
    let mut fire = None;
    if let Some(pl) = o.remove("save_playlist") {
        let title: String = pl["title"].as_str().unwrap_or("").trim().chars().take(200).collect();
        if !title.is_empty() {
            let mut items: Vec<u64> = vec![];
            for i in pl["ids"].as_array().into_iter().flatten().filter_map(|x| x.as_u64()).take(20_000).flat_map(|i| db.leaves(i)) {
                if visible(&db, i) && !items.contains(&i) { items.push(i); }
            }
            let summary = pl["summary"].as_str().unwrap_or("").to_string();
            let t = now();
            let n = items.len();
            let id = if let Some(p) = db.playlists.iter_mut().find(|p| p.owner_id == user.id && p.kind == PlaylistKind::Manual && p.title == title) {
                p.items = items;
                if !summary.is_empty() { p.summary = summary; }
                p.updated_at = t;
                fire = Some(("playlist.updated", json!({"id": p.id})));
                p.id
            } else {
                let id = db.new_id();
                db.playlists.push(Playlist { id, owner_id: user.id, title: title.clone(), summary, kind: PlaylistKind::Manual, items, query: None,
                    default_key: None, shared: false, pinned: false, created_at: t, updated_at: t });
                fire = Some(("playlist.created", json!({"id": id, "title": title, "smart": false, "user": user.username})));
                id
            };
            o.insert("playlist_id".into(), json!(id));
            o.insert("playlist_items".into(), json!(n));
            done.push(format!("playlist {id} with {n}"));
            changed = true;
        }
    }
    drop(db);
    if changed { app.save(); }
    if let Some((ev, data)) = fire { app.fire(ev, data); }
    (!done.is_empty()).then(|| done.join(", "))
}

/// Deletes the titles a plugin named (`delete_media`), each as if the caller
/// had pressed "Delete" on it; gives how many went and why others did not.
pub fn delete_media_from_plugin(app: &Arc<App>, user: &User, list: &[Value]) -> (usize, Vec<String>) {
    let (mut deleted, mut errors) = (0, vec![]);
    for id in list.iter().take(500).filter_map(|v| v.as_u64()) {
        let r = crate::api::actions::delete_media(app, user, id);
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
        "may_delete": crate::api::actions::may_delete(&app.db.lock().unwrap(), user)});
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
            if let Some(d) = apply_plugin_user_changes(app, user, &mut v) {
                detail = format!("{action}: {d}");
            }
            if let Some((added, removed)) = set_plugin_collections(app, &v) {
                if let Some(o) = v.as_object_mut() {
                    o.remove("collections");
                    o.insert("collections_added".into(), json!(added));
                    o.insert("collections_removed".into(), json!(removed));
                }
                detail = format!("{action}: collections +{added} −{removed}");
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

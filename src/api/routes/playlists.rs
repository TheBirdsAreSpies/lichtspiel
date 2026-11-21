//! Playlists.

use super::*;

pub(super) fn routes(app: &Arc<App>, ctx: &Ctx, user: &db::User, m: &str, segs: &[&str]) -> Option<Resp> {
    let admin = user.admin;
    handled(|hit| match (m, segs) {
        ("GET", ["playlists"]) | ("GET", ["playlists", "all"]) => {
            let db = app.db.lock().unwrap();
            let mut rows: Vec<(Playlist, usize, u64)> = vec![];
            let mut jsons: HashMap<u64, Value> = HashMap::new();
            for p in db.playlists.iter().filter(|p| visible_playlist(p, &user)) {
                // Worked out once: a smart playlist runs over the whole library.
                let items = query::playlist_items(&db, &user, p);
                // ?section=<id>: only playlists with something from that library (its Playlists page).
                let in_section = ctx.param("section").and_then(|s| s.parse::<u64>().ok()).is_none_or(|sid| {
                    items.iter().any(|i| db.items.get(i).is_some_and(|x| x.section_id == sid))
                });
                let (v, n, d) = playlist_json_with(&db, &user, p, items);
                if in_section && query::playlist_matches(p, &user, &ctx.params, n) {
                    jsons.insert(p.id, v);
                    rows.push((p.clone(), n, d));
                }
            }
            query::sort_playlists(&mut rows, ctx.param("sort").unwrap_or(""));
            let all: Vec<Value> = rows.iter().filter_map(|r| jsons.remove(&r.0.id)).collect();
            let (pg, off) = page(ctx, &all);
            ok(container(pg, all.len(), off, json!({})))
        }
        ("POST", ["playlists"]) => create_playlist(app, ctx, &user),
        ("POST", ["playlists", "preview"]) => {
            let q: SmartQuery = match serde_json::from_value(ctx.body.get("query").cloned().unwrap_or(ctx.body.clone())) {
                Ok(q) => q,
                Err(e) => return fail(400, &format!("The rules could not be read: {e}")),
            };
            let db = app.db.lock().unwrap();
            let ids = query::run_smart(&db, &user, &q);
            let ix = Index::build(&db);
            let list: Vec<Value> = ids.iter().take(200).filter_map(|i| db.items.get(i)).map(|i| item_json_ix(&db, &user, i, false, Some(&ix))).collect();
            ok(container(list, ids.len(), 0, json!({})))
        }
        ("GET", ["playlists", pid]) => {
            let db = app.db.lock().unwrap();
            match id(pid).and_then(|p| db.playlists.iter().find(|x| x.id == p)).filter(|p| visible_playlist(p, &user)) {
                Some(p) => ok(container(vec![playlist_json(&db, &user, p).0], 1, 0, json!({}))),
                None => fail(404, "No such playlist"),
            }
        }
        ("PUT", ["playlists", pid]) | ("PATCH", ["playlists", pid]) => update_playlist(app, ctx, &user, pid),
        ("DELETE", ["playlists", pid]) => {
            let Some(pid) = id(pid) else { return fail(404, "No such playlist") };
            let mut db = app.db.lock().unwrap();
            let Some(p) = db.playlists.iter().find(|p| p.id == pid) else { return fail(404, "No such playlist") };
            if p.is_default() { return fail(400, "Default playlists can't be deleted; unpin them instead (PUT pinned=false)"); }
            if p.owner_id != user.id && !admin { return fail(403, "This playlist belongs to someone else"); }
            db.playlists.retain(|p| p.id != pid);
            drop(db);
            app.save();
            ok(json!({"ok": true}))
        }
        ("GET", ["playlists", pid, "items"]) => {
            let db = app.db.lock().unwrap();
            let Some(p) = id(pid).and_then(|p| db.playlists.iter().find(|x| x.id == p)).filter(|p| visible_playlist(p, &user)) else { return fail(404, "No such playlist") };
            let mut ids = query::playlist_items(&db, &user, p);
            let rules = query::rules_from_params(&ctx.params);
            let any = ctx.param("match") == Some("any");
            let text = ctx.param("q").or(ctx.param("query")).map(|s| s.to_lowercase());
            let ix = query::index_for(&db, &rules);
            ids.retain(|i| db.items.get(i).is_some_and(|it| query::matches_all(&db, ix.as_ref(), user.id, it, &rules, !any) && text.as_ref().is_none_or(|t| it.title.to_lowercase().contains(t))));
            if let Some(s) = ctx.param("sort").filter(|s| *s != "playlist" && !s.is_empty()) {
                query::sort_items(&db, user.id, &mut ids, s);
            }
            let (pg, off) = page(ctx, &ids);
            let ix = Index::build(&db);
            let list: Vec<Value> = pg.iter().filter_map(|i| db.items.get(i)).map(|i| {
                let mut v = item_json_ix(&db, &user, i, false, Some(&ix));
                v["playlistItemID"] = json!(i);
                v
            }).collect();
            ok(container(list, ids.len(), off, json!({"playlistID": p.id, "title": p.title, "smart": p.kind == PlaylistKind::Smart})))
        }
        ("POST", ["playlists", pid, "items"]) | ("PUT", ["playlists", pid, "items"]) => {
            let ids = ids_from(ctx);
            let mut db = app.db.lock().unwrap();
            let leaves: Vec<u64> = ids.iter().flat_map(|i| db.leaves(*i)).filter(|i| db.items.get(i).is_some_and(|it| db.can_see_item(&user, it))).collect();
            let Some(p) = id(pid).and_then(|p| db.playlists.iter_mut().find(|x| x.id == p)) else { return fail(404, "No such playlist") };
            if p.kind == PlaylistKind::Smart { return fail(400, "Smart playlists fill themselves; change their rules instead"); }
            if p.owner_id != user.id && !admin { return fail(403, "This playlist belongs to someone else"); }
            let before = p.items.len();
            for l in leaves {
                if !p.items.contains(&l) { p.items.push(l); }
            }
            p.updated_at = now();
            let added = p.items.len() - before;
            let pid = p.id;
            drop(db);
            app.save();
            app.fire("playlist.updated", json!({"id": pid, "added": added}));
            ok(json!({"ok": true, "added": added}))
        }
        ("DELETE", ["playlists", pid, "items", ..]) | ("POST", ["playlists", pid, "items", "remove"]) => {
            // One entry (/items/{itemId}), several (?ids=1,2 or {ids: [..]}), or
            // all of them (?all=1). A show or season removes all its episodes.
            let one = segs.get(3).filter(|s| **s != "remove").and_then(|s| id(s));
            let wanted = match one { Some(i) => vec![i], None => ids_from(ctx) };
            let all = ctx.param("all") == Some("1") || ctx.body.get("all").and_then(Value::as_bool) == Some(true);
            if segs.len() > 4 || (one.is_none() && segs.len() == 4 && segs[3] != "remove") {
                return fail(404, "Not found");
            }
            if wanted.is_empty() && !all {
                return fail(400, "Say which entries to remove: /playlists/{id}/items/{itemId}, ?ids=1,2, {ids: [..]} or ?all=1");
            }
            let mut db = app.db.lock().unwrap();
            let mut gone: Vec<u64> = wanted.clone();
            for w in &wanted {
                gone.extend(db.leaves(*w));
            }
            let Some(p) = id(pid).and_then(|p| db.playlists.iter_mut().find(|x| x.id == p)) else { return fail(404, "No such playlist") };
            if p.kind == PlaylistKind::Smart { return fail(400, "Smart playlists fill themselves; change their rules instead"); }
            if p.owner_id != user.id && !admin { return fail(403, "This playlist belongs to someone else"); }
            let before = p.items.len();
            if all { p.items.clear(); } else { p.items.retain(|i| !gone.contains(i)); }
            let removed = before - p.items.len();
            if removed == 0 { return fail(404, "Not in this playlist"); }
            p.updated_at = now();
            let (pid, left) = (p.id, p.items.len());
            drop(db);
            app.save();
            app.fire("playlist.updated", json!({"id": pid, "removed": removed}));
            ok(json!({"ok": true, "removed": removed, "left": left}))
        }
        ("PUT", ["playlists", pid, "items", iid, "move"]) => {
            let after = ctx.num("after").unwrap_or(0);
            let mut db = app.db.lock().unwrap();
            let Some(p) = id(pid).and_then(|p| db.playlists.iter_mut().find(|x| x.id == p)) else { return fail(404, "No such playlist") };
            if p.owner_id != user.id && !admin { return fail(403, "This playlist belongs to someone else"); }
            let Some(iid) = id(iid) else { return fail(404, "Not in this playlist") };
            let Some(pos) = p.items.iter().position(|i| *i == iid) else { return fail(404, "Not in this playlist") };
            p.items.remove(pos);
            let at = if after == 0 { 0 } else { p.items.iter().position(|i| *i == after).map(|x| x + 1).unwrap_or(p.items.len()) };
            p.items.insert(at, iid);
            p.updated_at = now();
            drop(db);
            app.save();
            ok(json!({"ok": true}))
        }
        ("POST", ["playlists", pid, "copy"]) => {
            let mut db = app.db.lock().unwrap();
            let Some(p) = id(pid).and_then(|p| db.playlists.iter().find(|x| x.id == p)).filter(|p| visible_playlist(p, &user)).cloned() else { return fail(404, "No such playlist") };
            let items = query::playlist_items(&db, &user, &p);
            let nid = db.new_id();
            let t = now();
            let title = ctx.body_or_param("title").unwrap_or_else(|| format!("{} (copy)", p.title));
            db.playlists.push(Playlist { id: nid, owner_id: user.id, title, summary: p.summary.clone(), kind: PlaylistKind::Manual, items, query: None, default_key: None, shared: false, pinned: false, created_at: t, updated_at: t });
            drop(db);
            app.save();
            js(201, json!({"ok": true, "id": nid}))
        }
        _ => { *hit = false; fail(404, "") }
    })
}

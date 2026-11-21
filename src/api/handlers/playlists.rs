//! Creating and changing playlists (manual and smart).

use super::*;

pub fn create_playlist(app: &Arc<App>, ctx: &Ctx, user: &User) -> Resp {
    let title = ctx.body_or_param("title").unwrap_or_default().trim().to_string();
    if title.is_empty() { return fail(400, "Give the playlist a title"); }
    let smart = ctx.body_or_param("kind").map(|k| k == "smart").unwrap_or(false)
        || ctx.body.get("smart").and_then(|v| v.as_bool()).unwrap_or(false)
        || ctx.param("smart") == Some("1")
        || ctx.body.get("query").is_some_and(|q| !q.is_null());
    let query = if smart {
        match serde_json::from_value::<SmartQuery>(ctx.body.get("query").cloned().unwrap_or(json!({}))) {
            Ok(q) => Some(q),
            Err(e) => return fail(400, &format!("The rules could not be read: {e}")),
        }
    } else {
        None
    };
    let ids = ids_from(ctx);
    let mut db = app.db.lock().unwrap();
    let mut items: Vec<u64> = vec![];
    for i in ids.iter().flat_map(|i| db.leaves(*i)) {
        if db.items.get(&i).is_some_and(|it| db.can_see_item(user, it)) && !items.contains(&i) {
            items.push(i);
        }
    }
    let id = db.new_id();
    let t = now();
    let p = Playlist {
        id,
        owner_id: user.id,
        title,
        summary: ctx.body_or_param("summary").unwrap_or_default(),
        kind: if smart { PlaylistKind::Smart } else { PlaylistKind::Manual },
        items: if smart { vec![] } else { items },
        query,
        default_key: None,
        shared: ctx.body.get("shared").and_then(|v| v.as_bool()).unwrap_or(false),
        pinned: ctx.body.get("pinned").and_then(|v| v.as_bool()).unwrap_or(false),
        created_at: t,
        updated_at: t,
    };
    let (v, _, _) = playlist_json(&db, user, &p);
    db.playlists.push(p);
    drop(db);
    app.save();
    app.fire("playlist.created", json!({"id": id, "title": v["title"], "smart": smart, "user": user.username}));
    js(201, container(vec![v], 1, 0, json!({})))
}

pub fn update_playlist(app: &Arc<App>, ctx: &Ctx, user: &User, pid: &str) -> Resp {
    let Some(pid) = pid.parse::<u64>().ok() else { return fail(404, "No such playlist") };
    let mut db = app.db.lock().unwrap();
    let Some(p) = db.playlists.iter_mut().find(|p| p.id == pid) else { return fail(404, "No such playlist") };
    if p.owner_id != user.id && !user.admin {
        return fail(403, "This playlist belongs to someone else");
    }
    if let Some(t) = ctx.body_or_param("title").filter(|t| !t.trim().is_empty()) { p.title = t; }
    if let Some(s) = ctx.body_or_param("summary") { p.summary = s; }
    if let Some(v) = ctx.body.get("shared").and_then(|v| v.as_bool()) { p.shared = v || p.is_default(); }
    if let Some(v) = ctx.body.get("pinned").and_then(|v| v.as_bool()) { p.pinned = v; }
    if let Some(q) = ctx.body.get("query").filter(|q| !q.is_null()) {
        if p.kind != PlaylistKind::Smart { return fail(400, "Only smart playlists have rules"); }
        match serde_json::from_value::<SmartQuery>(q.clone()) {
            Ok(q) => p.query = Some(q),
            Err(e) => return fail(400, &format!("The rules could not be read: {e}")),
        }
    }
    p.updated_at = now();
    let p = p.clone();
    let (v, _, _) = playlist_json(&db, user, &p);
    drop(db);
    app.save();
    app.fire("playlist.updated", json!({"id": pid}));
    ok(container(vec![v], 1, 0, json!({})))
}

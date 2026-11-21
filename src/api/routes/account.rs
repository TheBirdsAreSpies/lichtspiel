//! Your own account: sign out, profile, preferences, two-step sign-in, sidebar, password; also the API docs and filter lists.

use super::*;

pub(super) fn routes(app: &Arc<App>, ctx: &Ctx, user: &db::User, m: &str, segs: &[&str]) -> Option<Resp> {
    handled(|hit| match (m, segs) {
        ("POST", ["api", "link", "confirm"]) => return link_confirm(ctx, &user),
        ("POST", ["mcp"]) => return crate::api::mcp::handle(app, ctx),
        // No event stream and no sessions: everything is answered on the POST.
        ("GET", ["mcp"]) | ("DELETE", ["mcp"]) => return fail(405, "POST JSON-RPC messages to /mcp"),
        ("POST", ["api", "logout"]) => {
            let mut db = app.db.lock().unwrap();
            match &ctx.token {
                Some(t) => { let h = auth::token_hash(t); db.tokens.retain(|x| x.token != h) }
                None => db.tokens.retain(|x| x.user_id != user.id),
            }
            drop(db);
            app.save();
            ok(json!({"ok": true}))
        }
        ("GET", ["api", "upcoming"]) => {
            let days = ctx.param("days").and_then(|d| d.parse::<i64>().ok()).unwrap_or(30);
            let db = app.db.lock().unwrap();
            let store = app.upcoming.lock().unwrap();
            ok(crate::connect::upcoming::answer(&db, &store, &user, days))
        }
        ("GET", ["api", "me"]) => {
            let mut v = user_json(&user);
            v["can_delete"] = json!(crate::api::actions::may_delete(&app.db.lock().unwrap(), &user));
            v["can_download"] = json!(crate::playback::downloads::may_download(&user));
            v["sidebar"] = user.sidebar.clone();
            v["prefs"] = json!(user.prefs);
            ok(v)
        }
        ("POST", ["api", "me", "profile"]) => {
            let email = ctx.body.get("email").and_then(|v| v.as_str()).map(|e| e.trim().to_string());
            if let Some(e) = &email { if !e.is_empty() && !crate::accounts::mail::valid_address(e) { return fail(400, "That is not an e-mail address"); } }
            let mut db = app.db.lock().unwrap();
            let Some(u) = db.users.iter_mut().find(|u| u.id == user.id) else { return fail(404, "No such user") };
            if let Some(n) = ctx.body.get("full_name").and_then(|v| v.as_str()) { u.full_name = n.trim().chars().take(80).collect(); }
            if let Some(e) = email { u.email = e; }
            let v = json!({"ok": true, "full_name": u.full_name, "email": u.email});
            drop(db);
            app.save();
            ok(v)
        }
        ("POST", ["api", "me", "prefs"]) => {
            // {key: text, …}: merged into this person's settings; null/"" removes a key.
            let Some(obj) = ctx.body.as_object() else { return fail(400, "Send an object") };
            let mut db = app.db.lock().unwrap();
            if let Some(u) = db.users.iter_mut().find(|u| u.id == user.id) {
                for (k, v) in obj {
                    if k.is_empty() || k.len() > 40 || !k.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') { continue; }
                    match v.as_str() {
                        Some(s) => { if u.prefs.len() < 64 || u.prefs.contains_key(k) { u.prefs.insert(k.clone(), s.chars().take(2000).collect()); } }
                        None => { u.prefs.remove(k); }
                    }
                }
            }
            drop(db);
            app.save();
            ok(json!({"ok": true}))
        }
        ("POST", ["api", "me", "2fa", "setup"]) => {
            let secret = crate::accounts::totp::new_secret();
            let mut db = app.db.lock().unwrap();
            let issuer = display_name(&db.settings);
            if let Some(u) = db.users.iter_mut().find(|u| u.id == user.id) { u.totp_pending = secret.clone(); }
            drop(db);
            app.save();
            let uri = crate::accounts::totp::uri(&issuer, &user.username, &secret);
            ok(json!({"secret": secret, "qr": crate::accounts::totp::qr_svg(&uri), "uri": uri}))
        }
        ("POST", ["api", "me", "2fa", "enable"]) => {
            let code = ctx.body_or_param("code").unwrap_or_default();
            let mut db = app.db.lock().unwrap();
            let Some(u) = db.users.iter_mut().find(|u| u.id == user.id) else { return fail(404, "No such user") };
            if u.totp_pending.is_empty() { return fail(400, "Start the setup first"); }
            if !crate::accounts::totp::verify(&u.totp_pending, &code, now()) { return fail(400, "That code is wrong"); }
            u.totp_secret = std::mem::take(&mut u.totp_pending);
            drop(db);
            app.save();
            ok(json!({"ok": true}))
        }
        ("POST", ["api", "me", "2fa", "disable"]) => {
            let password = ctx.body_or_param("password").unwrap_or_default();
            let code = ctx.body_or_param("code").unwrap_or_default();
            if !auth::verify(&password, &user.salt, &user.password_hash) { return fail(403, "The password is wrong"); }
            let mut db = app.db.lock().unwrap();
            let Some(u) = db.users.iter_mut().find(|u| u.id == user.id) else { return fail(404, "No such user") };
            if !u.totp_secret.is_empty() && !crate::accounts::totp::verify(&u.totp_secret, &code, now()) { return fail(403, "That code is wrong"); }
            u.totp_secret.clear();
            u.totp_pending.clear();
            drop(db);
            app.save();
            ok(json!({"ok": true}))
        }
        ("PUT", ["api", "me", "sidebar"]) => {
            // {order: [keys], hidden: [keys]}; a key is "s<library id>", "playlists" or "p:<plugin id>".
            let keys = |k: &str| -> Vec<Value> {
                ctx.body.get(k).and_then(|v| v.as_array()).map(|a| a.iter().filter_map(|x| x.as_str()).filter(|x| x.len() <= 40).take(500).map(|x| json!(x)).collect()).unwrap_or_default()
            };
            let v = json!({"order": keys("order"), "hidden": keys("hidden")});
            let mut db = app.db.lock().unwrap();
            if let Some(u) = db.users.iter_mut().find(|u| u.id == user.id) { u.sidebar = v.clone(); }
            drop(db);
            app.save();
            ok(json!({"ok": true, "sidebar": v}))
        }
        ("POST", ["api", "me", "password"]) => {
            let old = ctx.body_or_param("old").unwrap_or_default();
            let new = ctx.body_or_param("new").unwrap_or_default();
            if !auth::verify(&old, &user.salt, &user.password_hash) {
                return fail(403, "The old password is wrong");
            }
            if new.chars().count() < 6 {
                return fail(400, "Use at least 6 characters");
            }
            let (h, s) = auth::hash_password(&new);
            let mut db = app.db.lock().unwrap();
            if let Some(u) = db.users.iter_mut().find(|u| u.id == user.id) {
                u.password_hash = h;
                u.salt = s;
            }
            drop(db);
            app.save();
            ok(json!({"ok": true}))
        }
        ("GET", ["api", "docs"]) => ok(json!({
            "routes": ROUTES.iter().map(|r| json!({"method": r.0, "path": r.1, "description": r.2, "group": route_group(r.1)})).collect::<Vec<_>>(),
            "groups": GROUPS.iter().map(|g| json!({"name": g, "routes": ROUTES.iter().filter(|r| route_group(r.1) == *g)
                .map(|r| json!({"method": r.0, "path": r.1, "description": r.2})).collect::<Vec<_>>()})).collect::<Vec<_>>(),
            "auth": "Send X-Token: <token> (or Authorization: Bearer <token>)",
        })),
        ("GET", ["api", "filters"]) => ok(json!({
            "fields": query::ITEM_FIELDS.iter().map(|f| json!({"field": f.0, "type": f.1, "label": f.2})).collect::<Vec<_>>(),
            "operators": query::OPERATORS.iter().map(|o| json!({"op": o.0, "label": o.1})).collect::<Vec<_>>(),
            "url_shortcuts": {"field=value": "is", "field!=value": "is not", "field>=value": "at least", "field<=value": "at most", "field~=text": "contains", "field.op=value": "any operator", "a,b": "one of"},
            "playlist_sorts": ["title", "created_at", "updated_at", "item_count", "duration", "kind", "pinned", "owner"],
            "playlist_filters": ["kind=default|smart|manual", "smart=0|1", "owner=me|others|server|<id>", "title=<text>", "shared=0|1", "pinned=0|1", "min_items", "max_items"],
            "qualities": transcode::QUALITIES.iter().map(|q| q.0).collect::<Vec<_>>(),
            "plugin_events": plugins::EVENTS,
        })),
        _ => { *hit = false; fail(404, "") }
    })
}

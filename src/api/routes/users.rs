//! Sign-in tokens and devices, users and invites.

use super::*;

pub(super) fn routes(app: &Arc<App>, ctx: &Ctx, user: &db::User, m: &str, segs: &[&str]) -> Option<Resp> {
    let admin = user.admin;
    handled(|hit| match (m, segs) {
        ("GET", ["api", "ticket"]) => {
            let db = app.db.lock().unwrap();
            let scope = match (ctx.num("item"), ctx.param("path")) {
                (Some(i), _) => { if !db.items.get(&i).is_some_and(|it| db.can_see_item(&user, it)) { return fail(404, "Not found"); } i.to_string() }
                (None, Some(p)) => match auth::ticket_scope(p) { Some(s) if s == p => s, _ => return fail(400, "No ticket is made for this address") },
                (None, None) => "p".into(),
            };
            let (t, expires) = auth::ticket(&db.ticket_key, user.id, &scope, ctx.token.as_deref(), now());
            ok(json!({"ticket": t, "expires": expires}))
        }
        ("GET", ["api", "tokens"]) => {
            let db = app.db.lock().unwrap();
            let mine = ctx.token.as_deref().map(auth::token_hash);
            let list: Vec<Value> = db.tokens.iter().filter(|t| t.user_id == user.id).map(|t| json!({
                "id": auth::stored_token_id(&t.token), "device": t.device, "created_at": t.created_at, "last_used": t.last_used, "last_device": t.last_device, "last_ip": t.last_ip,
                "current": mine.as_ref() == Some(&t.token),
            })).collect();
            ok(json!({"tokens": list}))
        }
        ("POST", ["api", "tokens"]) => {
            // An extra token, e.g. for a script; it shows once.
            let device = ctx.body_or_param("device").filter(|d| !d.trim().is_empty()).unwrap_or_else(|| "API token".into());
            let mut db = app.db.lock().unwrap();
            let t = new_token(&mut db, user.id, &device);
            drop(db);
            app.save();
            js(201, json!({"ok": true, "token": t, "id": auth::token_id(&t), "device": device}))
        }
        ("POST", ["api", "tokens", "rotate"]) => {
            // New token for this sign-in; the old one stops working at once.
            // {all: true} also signs out every other device and script.
            let all = ctx.body.get("all").and_then(Value::as_bool).unwrap_or(false) || ctx.param("all") == Some("1");
            let mut db = app.db.lock().unwrap();
            let mine = ctx.token.as_deref().map(auth::token_hash);
            let device = db.tokens.iter().find(|t| mine.as_ref() == Some(&t.token)).map(|t| t.device.clone()).unwrap_or_default();
            let before = db.tokens.len();
            db.tokens.retain(|t| t.user_id != user.id || (!all && mine.as_ref() != Some(&t.token)));
            let revoked = before - db.tokens.len();
            let t = new_token(&mut db, user.id, &device);
            drop(db);
            app.save();
            ok(json!({"ok": true, "token": t, "id": auth::token_id(&t), "revoked": revoked}))
        }
        ("DELETE", ["api", "tokens", tid]) => {
            let mut db = app.db.lock().unwrap();
            let before = db.tokens.len();
            db.tokens.retain(|t| t.user_id != user.id || auth::stored_token_id(&t.token) != *tid);
            if db.tokens.len() == before { return fail(404, "No such token"); }
            drop(db);
            app.save();
            ok(json!({"ok": true}))
        }
        ("DELETE", ["api", "users", uid, "tokens"]) => {
            if !admin { return need_admin(); }
            let Some(uid) = id(uid) else { return fail(404, "No such user") };
            let mut db = app.db.lock().unwrap();
            let before = db.tokens.len();
            db.tokens.retain(|t| t.user_id != uid);
            let revoked = before - db.tokens.len();
            drop(db);
            app.save();
            ok(json!({"ok": true, "revoked": revoked}))
        }

        // users and invites
        ("GET", ["api", "users"]) => {
            if !admin { return need_admin(); }
            let db = app.db.lock().unwrap();
            ok(json!({"users": db.users.iter().map(user_json).collect::<Vec<_>>()}))
        }
        ("POST", ["api", "users"]) => {
            // A local account made by an admin: no invite link, they sign in with what the admin set.
            if !admin { return need_admin(); }
            let username = ctx.body_or_param("username").unwrap_or_default().trim().to_string();
            let password = ctx.body_or_param("password").unwrap_or_default();
            let home = ctx.body.get("home").and_then(|v| v.as_bool()).unwrap_or(false);
            let pin = ctx.body_or_param("pin").unwrap_or_default();
            let as_admin = ctx.body.get("admin").and_then(|v| v.as_bool()).unwrap_or(false);
            if !auth::valid_username(&username) { return fail(400, "Pick a user name of 2-40 letters or numbers"); }
            // A home profile may go without a password: it is then only reachable from the sign-in screen.
            if !(home && password.is_empty()) && password.chars().count() < 6 { return fail(400, "Use at least 6 characters for the password"); }
            if !pin.is_empty() && !auth::valid_pin(&pin) { return fail(400, "A PIN is 4 to 8 digits"); }
            if home && as_admin && pin.is_empty() { return fail(400, "An admin's home profile needs a PIN"); }
            let (h, s) = if password.is_empty() { (db::random_hex(32), db::random_hex(16)) } else { auth::hash_password(&password) };
            let sections = match ctx.body.get("allowed_sections") {
                Some(Value::Array(a)) => Some(a.iter().filter_map(|x| x.as_u64()).collect()),
                _ => None,
            };
            let mut db = app.db.lock().unwrap();
            if db.users.iter().any(|u| u.username.eq_ignore_ascii_case(&username)) { return fail(409, "That user name is taken"); }
            let id = db.new_id();
            db.users.push(db::User { id, username: username.clone(), email: ctx.body_or_param("email").unwrap_or_default(), password_hash: h, salt: s,
                admin: as_admin, created_at: now(), allowed_sections: sections, invited_by: Some(user.id),
                allow_transcode: None, allow_delete: false, allow_download: None, sidebar: Value::Null, home, pin_hash: String::new(), pin_salt: String::new(), max_age: None, block_unrated: false, rss_key: String::new(), full_name: String::new(), totp_secret: String::new(), totp_pending: String::new(), prefs: Default::default() });
            let _ = set_pin(db.users.last_mut().unwrap(), &pin);
            let v = user_json(db.user(id).unwrap());
            drop(db);
            app.save();
            app.fire("user.joined", json!({"username": username, "added_by": user.username}));
            js(201, v)
        }
        ("PATCH", ["api", "users", uid]) | ("PUT", ["api", "users", uid]) => {
            if !admin { return need_admin(); }
            let Some(uid) = id(uid) else { return fail(404, "No such user") };
            let mut db = app.db.lock().unwrap();
            let admins = db.users.iter().filter(|u| u.admin).count();
            let Some(u) = db.users.iter_mut().find(|u| u.id == uid) else { return fail(404, "No such user") };
            let new_pin = ctx.body.get("pin").and_then(|v| v.as_str());
            if let Some(p) = new_pin { if !p.is_empty() && !auth::valid_pin(p) { return fail(400, "A PIN is 4 to 8 digits"); } }
            let will_admin = ctx.body.get("admin").and_then(|v| v.as_bool()).unwrap_or(u.admin);
            let will_home = ctx.body.get("home").and_then(|v| v.as_bool()).unwrap_or(u.home);
            let will_pin = new_pin.map(|p| !p.is_empty()).unwrap_or(!u.pin_hash.is_empty());
            if will_home && will_admin && !will_pin { return fail(400, "An admin's home profile needs a PIN"); }
            if let Some(a) = ctx.body.get("admin").and_then(|v| v.as_bool()) {
                if !a && u.admin && admins <= 1 { return fail(400, "The last admin can't be removed"); }
                u.admin = a;
            }
            if let Some(v) = ctx.body.get("allowed_sections") {
                u.allowed_sections = if v.is_null() { None } else { v.as_array().map(|a| a.iter().filter_map(|x| x.as_u64()).collect()) };
            }
            if let Some(v) = ctx.body.get("allow_transcode").and_then(|v| v.as_bool()) { u.allow_transcode = Some(v); }
            if let Some(v) = ctx.body.get("allow_delete").and_then(|v| v.as_bool()) { u.allow_delete = v; }
            if let Some(v) = ctx.body.get("allow_download").and_then(|v| v.as_bool()) { u.allow_download = Some(v); }
            if ctx.body.get("reset_2fa").and_then(|v| v.as_bool()) == Some(true) { u.totp_secret.clear(); u.totp_pending.clear(); }
            if let Some(v) = ctx.body.get("max_age") { u.max_age = v.as_u64().map(|n| n.min(99) as u32); }
            if let Some(v) = ctx.body.get("block_unrated").and_then(|v| v.as_bool()) { u.block_unrated = v; }
            if let Some(e) = ctx.body.get("email").and_then(|v| v.as_str()) { u.email = e.into(); }
            if let Some(p) = new_pin { let _ = set_pin(u, p); }
            u.home = will_home;
            if let Some(pw) = ctx.body.get("password").and_then(|v| v.as_str()) {
                if pw.chars().count() < 6 { return fail(400, "Use at least 6 characters"); }
                let (h, s) = auth::hash_password(pw);
                u.password_hash = h;
                u.salt = s;
            }
            let v = user_json(u);
            drop(db);
            app.save();
            ok(v)
        }
        ("DELETE", ["api", "users", uid]) => {
            if !admin { return need_admin(); }
            let Some(uid) = id(uid) else { return fail(404, "No such user") };
            if uid == user.id { return fail(400, "You can't remove yourself"); }
            let mut db = app.db.lock().unwrap();
            db.users.retain(|u| u.id != uid);
            db.tokens.retain(|t| t.user_id != uid);
            db.playlists.retain(|p| p.owner_id != uid);
            let prefix = format!("{uid}:");
            db.views.retain(|k, _| !k.starts_with(&prefix));
            drop(db);
            app.save();
            ok(json!({"ok": true}))
        }
        ("GET", ["api", "invites"]) => {
            if !admin { return need_admin(); }
            let db = app.db.lock().unwrap();
            let base = base_url(&db, ctx);
            ok(json!({"invites": db.invites.iter().rev().map(|i| invite_json(&db, i, &base)).collect::<Vec<_>>()}))
        }
        ("POST", ["api", "invites"]) => {
            if !admin { return need_admin(); }
            let mut db = app.db.lock().unwrap();
            let days = ctx.body_or_param("days").and_then(|d| d.parse::<u64>().ok()).unwrap_or(db.settings.invite_default_days).clamp(1, 365);
            let sections = match ctx.body.get("allowed_sections") {
                Some(Value::Array(a)) => Some(a.iter().filter_map(|x| x.as_u64()).collect()),
                _ => None,
            };
            let inv = Invite {
                code: db::random_hex(12),
                email: ctx.body_or_param("email").unwrap_or_default(),
                note: ctx.body_or_param("note").unwrap_or_default(),
                created_by: user.id,
                created_at: now(),
                expires_at: now() + days * 86400,
                allowed_sections: sections,
                admin: ctx.body.get("admin").and_then(|v| v.as_bool()).unwrap_or(false),
                used_by: None,
                used_at: None,
            };
            let send = ctx.body.get("send_mail").and_then(|v| v.as_bool()).unwrap_or(false);
            if send && !crate::accounts::mail::valid_address(&inv.email) { return fail(400, "Enter their e-mail address to send the invite"); }
            let base = base_url(&db, ctx);
            let mut v = invite_json(&db, &inv, &base);
            let settings = db.settings.clone();
            db.invites.push(inv);
            drop(db);
            app.save();
            app.fire("user.invited", json!({"email": v["email"], "url": v["url"], "note": v["note"], "expires_at": v["expires_at"], "by": user.username}));
            if send {
                // The link is made either way; a mail that does not go out is said, and the link can still be copied.
                let lang = ctx.body_or_param("lang").unwrap_or_else(|| settings.metadata_language.clone());
                let (to, note, url) = (v["email"].as_str().unwrap_or("").to_string(), v["note"].as_str().unwrap_or("").to_string(), v["url"].as_str().unwrap_or("").to_string());
                match invite_mail(&settings, &lang, &user.display_name(), &to, &note, &url, days) {
                    Ok(()) => v["mailed"] = json!(true),
                    Err(e) => v["mail_error"] = json!(e),
                }
            }
            js(201, v)
        }
        ("DELETE", ["api", "invites", code]) => {
            if !admin { return need_admin(); }
            let mut db = app.db.lock().unwrap();
            db.invites.retain(|i| i.code != *code);
            drop(db);
            app.save();
            ok(json!({"ok": true}))
        }
        _ => { *hit = false; fail(404, "") }
    })
}

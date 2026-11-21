//! Plugins: the list, settings, uploads, panels and calls.

use super::*;

pub(super) fn routes(app: &Arc<App>, ctx: &Ctx, user: &db::User, m: &str, segs: &[&str]) -> Option<Resp> {
    let admin = user.admin;
    handled(|hit| match (m, segs) {
        ("GET", ["api", "plugins"]) => {
            if !admin { return need_admin(); }
            plugin_list(app)
        }
        ("POST", ["api", "plugins", "reload"]) => {
            if !admin { return need_admin(); }
            app.plugins.lock().unwrap().reload();
            app.sync_plugin_states();
            app.save();
            let p = app.plugins.lock().unwrap();
            ok(json!({"ok": true, "count": p.list.len(), "errors": p.errors}))
        }
        ("POST", ["api", "plugins", "upload"]) => {
            if !admin { return need_admin(); }
            let Some(zip) = ctx.body["data"].as_str().and_then(|d| b64_decode(d.split_once(',').map_or(d, |(_, rest)| rest))) else {
                return fail(400, "Send the zip as {data} (base64)");
            };
            let replace = ctx.body["replace"].as_bool().unwrap_or(false);
            let got = app.plugins.lock().unwrap().install_zip(&zip, replace);
            match got {
                Ok(i) => {
                    app.sync_plugin_states();
                    app.save();
                    let enabled = app.db.lock().unwrap().plugins.get(&i.id).is_some_and(|s| s.enabled);
                    let p = app.plugins.lock().unwrap();
                    ok(json!({"ok": true, "id": i.id, "name": i.name, "version": i.version, "replaced": i.replaced, "enabled": enabled, "files": i.files, "count": p.list.len(), "errors": p.errors}))
                }
                Err(plugins::InstallError::Exists { id, name, version }) => js(409, json!({"error": "A plugin with this id is installed already", "status": 409, "exists": true, "id": id, "name": name, "version": version})),
                Err(plugins::InstallError::Bad(e)) => fail(400, &e),
            }
        }
        ("GET", ["api", "plugins", "log"]) => {
            if !admin { return need_admin(); }
            let p = app.plugins.lock().unwrap();
            ok(json!({"log": p.log.iter().rev().cloned().collect::<Vec<_>>()}))
        }
        ("POST", ["api", "plugins", pid, onoff]) if *onoff == "enable" || *onoff == "disable" => {
            if !admin { return need_admin(); }
            if app.plugins.lock().unwrap().get(pid).is_none() { return fail(404, "No such plugin"); }
            let mut db = app.db.lock().unwrap();
            db.plugins.entry(pid.to_string()).or_default().enabled = *onoff == "enable";
            drop(db);
            app.save();
            ok(json!({"ok": true, "enabled": *onoff == "enable"}))
        }
        ("PUT", ["api", "plugins", pid, "settings"]) => {
            if !admin { return need_admin(); }
            if app.plugins.lock().unwrap().get(pid).is_none() { return fail(404, "No such plugin"); }
            let Value::Object(map) = ctx.body.clone() else { return fail(400, "Send the settings as a JSON object") };
            let mut db = app.db.lock().unwrap();
            let st = db.plugins.entry(pid.to_string()).or_default();
            for (k, v) in map { st.settings.insert(k, v); }
            drop(db);
            app.save();
            ok(json!({"ok": true}))
        }
        ("PUT", ["api", "plugins", pid, "placement"]) => {
            if !admin { return need_admin(); }
            if app.plugins.lock().unwrap().get(pid).is_none() { return fail(404, "No such plugin"); }
            let place = ctx.body_or_param("placement").unwrap_or_default();
            if !place.is_empty() && !plugins::PLACEMENTS.contains(&place.as_str()) { return fail(400, "placement must be settings, sidebar or both"); }
            app.db.lock().unwrap().plugins.entry(pid.to_string()).or_default().placement = place;
            app.save();
            ok(json!({"ok": true}))
        }
        ("GET", ["api", "plugins", "panels"]) => plugin_panels(app, &user),
        ("POST", ["api", "plugins", pid, "call", action]) => plugin_call(app, ctx, &user, pid, action),
        _ => { *hit = false; fail(404, "") }
    })
}

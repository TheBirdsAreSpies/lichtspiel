//! Addresses open to everyone: the web app and its scripts, server identity and status, signing in,
//! invites, password reset, HLS segments, the RSS feed and what plugins read with their own key.

use super::*;

pub(super) fn routes(app: &Arc<App>, ctx: &Ctx, m: &str, segs: &[&str]) -> Option<Resp> {
    handled(|hit| match (m, segs) {
        ("GET", ["web", "app.js"]) => return web_script(WEB_JS, &ctx.params),
        ("GET", ["web", "library.js"]) => return web_script(WEB_LIBRARY_JS, &ctx.params),
        ("GET", ["web", "player.js"]) => return web_script(WEB_PLAYER_JS, &ctx.params),
        ("GET", ["web", "pages.js"]) => return web_script(WEB_PAGES_JS, &ctx.params),
        ("GET", ["web", "dashboard.js"]) => return web_script(WEB_DASHBOARD_JS, &ctx.params),
        ("GET", ["web", "downloads.js"]) => return web_script(WEB_DOWNLOADS_JS, &ctx.params),
        ("GET", ["web", "cast.js"]) => return web_script(WEB_CAST_JS, &ctx.params),
        ("GET", ["web", "i18n.js"]) => return web_script(WEB_I18N_JS, &ctx.params),
        ("GET", ["web", "lang", file]) => {
            return match WEB_LANGS.iter().find(|l| l.0 == *file) {
                Some(l) => web_script(l.1, &ctx.params),
                None => fail(404, "Not found"),
            };
        }
        ("GET", []) | ("GET", ["web", ..]) | ("GET", ["index.html"]) => {
            // The server's own name goes in right away, so a reload does not show
            // "Lichtspiel" first and swap it for the name a moment later.
            let name = crate::server::display_name(&app.db.lock().unwrap().settings);
            let esc = name.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;");
            let tab = if name == crate::server::APP_NAME { esc.clone() } else { format!("{esc} · {}", crate::server::APP_NAME) };
            let page = WEB_APP
                .replacen("<title>Lichtspiel</title>", &format!("<title>{tab}</title>"), 1)
                .replacen("<span id=\"sname\">Lichtspiel</span>", &format!("<span id=\"sname\">{esc}</span>"), 1)
                // The scripts with the version's fingerprint, so the browser keeps them until the next update.
                .replace(".js\"></script>", &format!(".js?v={}\"></script>", web_version()));
            let mut r = bytes(200, "text/html; charset=utf-8", page.into_bytes());
            r.add_header(crate::server::header("Content-Security-Policy", crate::server::web_csp()));
            r.add_header(crate::server::header("Cache-Control", "no-cache"));
            return r;
        }
        ("GET", ["identity"]) => {
            let db = app.db.lock().unwrap();
            return ok(json!({"MediaContainer": {"machineIdentifier": db.machine_id, "version": VERSION, "friendlyName": crate::server::display_name(&db.settings)}}));
        }
        ("GET", ["api", "status"]) => {
            let db = app.db.lock().unwrap();
            return ok(json!({"name": crate::server::display_name(&db.settings), "version": VERSION, "build": BUILD, "built_at": BUILT_AT.parse::<u64>().unwrap_or(0), "setup_needed": db.users.is_empty(), "machine_id": db.machine_id, "mail": crate::accounts::mail::ready(&db.settings),
                "started_at": app.started_at, "signed_in": ctx.user.as_ref().map(|u| u.username.clone()),
                // For a Chromecast, when the page was opened as localhost: the address it reaches the server at.
                "lan_url": ctx.user.as_ref().map(|_| format!("{}://{}:{}", crate::server::scheme(), crate::lan_address(), *app.port.lock().unwrap()))}));
        }
        ("POST", ["api", "setup"]) => return setup(app, ctx),
        ("POST", ["api", "login"]) => return login(app, ctx),
        ("POST", ["api", "link"]) => return link_start(ctx),
        ("GET", ["api", "link", lid]) => return link_poll(app, lid),
        ("GET", ["api", "home"]) => return home_profiles(app, ctx),
        ("POST", ["api", "home", uid, "login"]) => return home_login(app, ctx, uid),
        ("GET", ["api", "invites", code]) => return invite_info(app, code),
        ("POST", ["api", "invites", code, "accept"]) => return invite_accept(app, ctx, code),
        ("POST", ["api", "password", "forgot"]) => return password_forgot(app, ctx),
        ("GET", ["api", "password", "reset", code]) => return password_reset_info(app, code),
        ("POST", ["api", "password", "reset", code]) => return password_reset(app, ctx, code),
        ("GET", ["video", "transcode", "hls", sid, file]) => return hls_file(app, sid, file),
        ("GET", ["rss", "watchlist", key]) => return crate::connect::rss::feed(app, ctx, key),
        ("GET", ["api", "plugins", pid, "library"]) => return plugin_library(app, ctx, pid),
        ("GET", ["api", "plugins", pid, "history"]) => return plugin_history(app, ctx, pid),
        ("GET", ["favicon.ico"]) => return bytes(200, "image/svg+xml", FAVICON.as_bytes().to_vec()),
        _ => { *hit = false; fail(404, "") }
    })
}

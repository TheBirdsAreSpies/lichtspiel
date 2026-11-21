//! Server settings, the transcoder and folder browsing.

use super::*;

pub(super) fn routes(app: &Arc<App>, ctx: &Ctx, user: &db::User, m: &str, segs: &[&str]) -> Option<Resp> {
    let admin = user.admin;
    handled(|hit| match (m, segs) {
        ("GET", ["api", "settings"]) => {
            if !admin { return need_admin(); }
            let db = app.db.lock().unwrap();
            ok(json!({"settings": db.settings, "lan_url": format!("{}://{}:{}", crate::server::scheme(), crate::lan_address(), *app.port.lock().unwrap()), "scan_intervals": db::SCAN_INTERVALS.iter().map(|s| json!({"minutes": s.0, "label": s.1})).collect::<Vec<_>>(),
                "data_dir": app.data_dir.to_string_lossy(), "transcoder_qualities": ["automatic", "speed", "quality", "max"], "hw_accel": ["auto", "none", "vaapi", "qsv", "nvenc"],
                "played_thresholds": db::PLAYED_THRESHOLDS, "h264_presets": transcode::X264_PRESETS,
                "played_at_choices": db::PLAYED_AT_CHOICES.iter().map(|c| json!({"value": c.0, "label": c.1})).collect::<Vec<_>>(),
                "credits_marker_choices": db::CREDITS_MARKER_CHOICES.iter().map(|c| json!({"value": c.0, "label": c.1})).collect::<Vec<_>>(),
                "languages": db::LANGUAGES.iter().map(|l| json!({"code": l.0, "name": l.1})).collect::<Vec<_>>(),
                "port_in_use": *app.port.lock().unwrap(),
                "your_address": ctx.remote.map(|a| a.to_string()), "your_address_trusted": ctx.remote_trusted, "you_are_local": ctx.is_local()}))
        }
        ("PUT", ["api", "settings"]) | ("PATCH", ["api", "settings"]) => {
            if !admin { return need_admin(); }
            let mut db = app.db.lock().unwrap();
            let mut cur = serde_json::to_value(&db.settings).unwrap_or(json!({}));
            if let (Value::Object(c), Value::Object(b)) = (&mut cur, &ctx.body) {
                for (k, v) in b {
                    if c.contains_key(k) { c.insert(k.clone(), v.clone()); }
                }
            }
            match serde_json::from_value::<db::Settings>(cur) {
                Ok(s) => {
                    if !["auto", "none", "vaapi", "qsv", "nvenc"].contains(&s.hw_accel.as_str()) { return fail(400, "hw_accel must be auto, none, vaapi, qsv or nvenc"); }
                    if !transcode::X264_PRESETS.contains(&s.h264_preset.as_str()) { return fail(400, "h264_preset must be one of the x264 presets"); }
                    if !(1..=51).contains(&s.h264_minimum_crf) { return fail(400, "h264_minimum_crf must be 1-51"); }
                    if s.played_at_behaviour > 3 { return fail(400, "played_at_behaviour must be 0, 1, 2 or 3"); }
                    if !db::CREDITS_MARKER_CHOICES.iter().any(|c| c.0 == s.credits_markers_behaviour) { return fail(400, "credits_markers_behaviour must be never, scheduled or asap"); }
                    if !db::CREDITS_MARKER_CHOICES.iter().any(|c| c.0 == s.intro_markers_behaviour) { return fail(400, "intro_markers_behaviour must be never, scheduled or asap"); }
                    if !["automatic", "speed", "quality", "max"].contains(&s.transcoder_quality.as_str()) { return fail(400, "transcoder_quality must be automatic, speed, quality or max"); }
                    if s.port == 0 { return fail(400, "port must be 1-65535"); }
                    if let Err(e) = crate::server::check_network_list(&s.lan_networks) { return fail(400, &e); }
                    if !s.bind_address.trim().is_empty() && s.bind_address.trim().parse::<std::net::IpAddr>().is_err() { return fail(400, "network card address must be empty or an IP address such as 192.168.1.10"); }
                    if let Err(e) = crate::server::read_tls(&s.tls_cert, &s.tls_key) { return fail(400, &e); }
                    if (!s.relay_server.trim().is_empty() || !s.relay_secret.is_empty()) && (!s.relay_server.trim().contains(':') || s.relay_secret.len() < 8) {
                        return fail(400, "the relay needs an address like name.example.com:8485 and a secret of at least 8 characters");
                    }
                    if s.http_redirect_port != 0 && s.http_redirect_port == s.port { return fail(400, "the port that passes http:// on must differ from the server port"); }
                    let dev = s.hw_device.trim();
                    if !(dev.is_empty() || dev.chars().all(|c| c.is_ascii_digit()) || (dev.starts_with("/dev/") && !dev.contains(' ') && !dev.contains(".."))) {
                        return fail(400, "graphics device must be empty, a card number or a path like /dev/dri/renderD128");
                    }
                    if let Err(e) = crate::server::check_open_networks(&s.no_auth_networks) { return fail(400, &e); }
                    if let Err(e) = crate::accounts::mail::check_settings(&s) { return fail(400, &e); }
                    let port_changed = s.port != db.settings.port;
                    let cache_mb = s.db_cache_mb;
                    let language_changed = s.metadata_language != db.settings.metadata_language;
                    let imdb_plots = !s.omdb_api_key.trim().is_empty() && s.omdb_api_key.trim() != db.settings.omdb_api_key.trim();
                    if !db::LANGUAGES.iter().any(|x| x.0 == s.metadata_language) { return fail(400, "Unknown language"); }
                    let articles_changed = s.sort_ignore_articles != db.settings.sort_ignore_articles || s.sort_articles != db.settings.sort_articles;
                    db.settings = s;
                    if articles_changed {
                        db::set_articles(db.settings.sort_ignore_articles, &db.settings.sort_articles);
                        let ids: Vec<u64> = db.items.keys().copied().collect();
                        for id in ids { if let Some(it) = db.items.get_mut(&id) { it.sort_title = db::sort_title(&it.title); } }
                    }
                    let mut v = json!({"ok": true, "settings": db.settings});
                    if port_changed || db.settings.port != *app.port.lock().unwrap() {
                        v["restart_needed"] = json!(format!("The server keeps using port {} until it is restarted (a --port option or LICHTSPIEL_PORT still wins over this setting).", *app.port.lock().unwrap()));
                    }
                    drop(db);
                    app.store.lock().unwrap().set_cache_mb(cache_mb);
                    app.save();
                    if language_changed {
                        // Libraries that follow the server's language get their details again.
                        v["refetching"] = json!(app.refetch_details(|sec| sec.language.trim().is_empty()));
                    } else if imdb_plots {
                        // A new OMDb key: everything gets its plot from IMDb now.
                        v["refetching"] = json!(app.refetch_details(|_| true));
                    }
                    ok(v)
                }
                Err(e) => fail(400, &format!("Those settings could not be read: {e}")),
            }
        }
        ("POST", ["api", "settings", "mail-test"]) => {
            if !admin { return need_admin(); }
            mail_test(app, ctx, &user)
        }
        ("GET", ["api", "transcoder"]) => {
            if !admin { return need_admin(); }
            let mut v = transcode::detect(&app.settings());
            v["active_transcodes"] = json!(app.active_transcodes());
            ok(v)
        }
        ("GET", ["api", "browse"]) => {
            if !admin { return need_admin(); }
            browse(ctx.param("path").unwrap_or(""))
        }
        _ => { *hit = false; fail(404, "") }
    })
}

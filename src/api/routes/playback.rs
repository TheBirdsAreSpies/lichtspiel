//! Playback: the decision how to play, files, subtitles, frames and streams.

use super::*;

pub(super) fn routes(app: &Arc<App>, ctx: &Ctx, user: &db::User, m: &str, segs: &[&str]) -> Option<Resp> {
    let admin = user.admin;
    handled(|hit| match (m, segs) {
        ("GET", ["library", "metadata", iid, "decision"]) => {
            let Some(iid) = id(iid) else { return fail(404, "Not found") };
            let db = app.db.lock().unwrap();
            let Some(it) = db.items.get(&iid).filter(|i| db.can_see_item(&user, i)) else { return fail(404, "Not found") };
            if it.file.is_none() { return fail(400, "This entry has no video file") }
            // ?part=n: one file of a split movie (0 = the first).
            let part = ctx.num("part").map(|n| n as usize).unwrap_or(0);
            let Some(f) = it.part(part) else { return fail(404, "No such part") };
            let req = transcode_request(db.settings.remote_max_mbps, db.settings.boost_dialogue, ctx);
            let d = transcode::decide(f, &req);
            let allowed = user.allow_transcode.unwrap_or(true) && db.settings.transcoding_enabled;
            // The addresses carry a ticket for this item, never the sign-in token.
            let tok = format!("&ticket={}", auth::ticket(&db.ticket_key, user.id, &iid.to_string(), ctx.token.as_deref(), now()).0);
            let tok = if part > 0 { format!("&part={part}{tok}") } else { tok };
            let q = ctx.param("quality").unwrap_or("original");
            // A chosen audio track travels with the stream addresses.
            let tok = match ctx.num("audio") { Some(a) => format!("&audio={a}{tok}"), None => tok };
            let url = match d.method {
                Method::Direct => format!("/library/parts/{iid}/file?x=1{tok}"),
                _ => format!("/video/transcode/{iid}/stream.mp4?quality={q}{tok}"),
            };
            // An optimized version plays as it is instead of converting the original.
            let original_quality = req.max_height == 0 && req.max_kbps == 0 && req.audio_stream.is_none() && !req.force_transcode;
            if d.method != Method::Direct && part == 0 && original_quality && crate::api::actions::optimized_file(app, it).is_some() {
                let url = format!("/library/parts/{iid}/file?optimized=1{tok}");
                return ok(json!({"method": "direct", "optimized": true, "reasons": ["Plays the optimized version"], "height": d.height, "bitrateKbps": d.bitrate_kbps,
                    "url": url, "decisionCode": 1000, "decisionText": transcode::decision_text(1000), "direct": url, "allowed": true,
                    "hls": format!("/video/transcode/{iid}/start.m3u8?quality={q}{tok}"), "resume": db.view(user.id, iid).view_offset_ms, "duration": it.duration_ms}));
            }
            let code = if !allowed && d.method == Method::Transcode { 2000 } else { transcode::decision_code(&d.method) };
            ok(json!({"method": d.method, "reasons": d.reasons, "height": d.height, "bitrateKbps": d.bitrate_kbps, "url": url,
                "decisionCode": code, "decisionText": transcode::decision_text(code),
                "hls": format!("/video/transcode/{iid}/start.m3u8?quality={q}{tok}"), "direct": format!("/library/parts/{iid}/file?x=1{tok}"),
                "allowed": allowed || d.method == Method::Direct, "resume": db.view(user.id, iid).view_offset_ms, "duration": it.duration_ms}))
        }
        ("GET", ["library", "parts", iid, "file"]) | ("GET", ["library", "parts", iid, _, "file"]) | ("GET", ["library", "parts", iid, "file", _]) => direct_file(app, ctx, &user, iid),
        ("GET", ["library", "parts", iid, "subtitles", n]) => subtitles(app, ctx, &user, iid, n),
        ("POST", ["library", "parts", iid, "subtitles", n, "sync"]) => {
            if !admin { return need_admin(); }
            crate::api::handlers::subtitle_sync(app, &user, iid, n)
        }
        ("PUT", ["library", "parts", iid, "subtitles", n, "timing"]) => {
            if !admin { return need_admin(); }
            crate::api::handlers::subtitle_set_timing(app, &user, iid, n, &ctx.body)
        }
        ("GET", ["library", "metadata", iid, "chapters"]) => crate::api::handlers::chapters(app, &user, iid),
        ("GET", ["library", "metadata", iid, "lyrics"]) => crate::api::handlers::lyrics(app, &user, iid),
        ("GET", ["library", "parts", iid, "frame"]) => frame(app, ctx, &user, iid),

        // streaming
        ("GET", ["video", "transcode", iid, "stream.mp4"]) => progressive(app, ctx, &user, iid),
        ("GET", ["video", "transcode", iid, "start.m3u8"]) => hls_start(app, ctx, &user, iid),
        ("GET", ["status", "sessions"]) => sessions(app, &user),
        ("GET", ["video", "transcode", "sessions"]) => {
            if !admin { return need_admin(); }
            let map = app.hls.lock().unwrap();
            ok(json!({"sessions": map.values().map(|s| json!({"id": s.id, "item": s.item_id, "user": s.user_id, "method": s.method, "height": s.height, "started_at": s.started_at})).collect::<Vec<_>>()}))
        }
        ("DELETE", ["video", "transcode", "sessions", sid]) => {
            let mut map = app.hls.lock().unwrap();
            let allowed = map.get(*sid).is_some_and(|s| admin || s.user_id == user.id);
            if !allowed { return fail(404, "No such session"); }
            let mut s = map.remove(*sid).unwrap();
            drop(map);
            let _ = s.child.kill();
            let _ = s.child.wait();
            let _ = std::fs::remove_dir_all(&s.dir);
            ok(json!({"ok": true}))
        }
        _ => { *hit = false; fail(404, "") }
    })
}

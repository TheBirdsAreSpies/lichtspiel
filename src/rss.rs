//! The watchlist as an RSS feed. Each person has a private key in the address,
//! so a feed reader needs no sign-in; a new key (or switching it off) ends the old address.

use crate::server::*;
use serde_json::json;

fn esc(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

/// Unix seconds as an RFC 822 date ("Tue, 03 Nov 2026 10:00:00 +0000").
pub fn rfc822(t: u64) -> String {
    let days = (t / 86400) as i64;
    let secs = t % 86400;
    let (y, m, d) = civil(days);
    let wd = ["Thu", "Fri", "Sat", "Sun", "Mon", "Tue", "Wed"][(days.rem_euclid(7)) as usize];
    let mon = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"][(m - 1) as usize];
    format!("{wd}, {d:02} {mon} {y} {:02}:{:02}:{:02} +0000", secs / 3600, secs % 3600 / 60, secs % 60)
}

/// Days since 1970-01-01 → (year, month, day).
fn civil(z: i64) -> (i64, u32, u32) {
    let z = z + 719468;
    let era = z.div_euclid(146097);
    let doe = z.rem_euclid(146097);
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    ((yoe + era * 400) + if m <= 2 { 1 } else { 0 }, m, d)
}

/// The public feed: /rss/watchlist/<key>.xml
pub fn feed(app: &App, ctx: &Ctx, key: &str) -> Resp {
    let key = key.trim_end_matches(".xml");
    let db = app.db.lock().unwrap();
    let Some(user) = db.users.iter().find(|u| !u.rss_key.is_empty() && u.rss_key == key) else { return fail(404, "No such feed") };
    let base = base_url(&db, ctx);
    let ids = crate::dashboard::watchlist_ids(&db, user, "added", None);
    let mut out = String::from("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<rss version=\"2.0\"><channel>\n");
    out.push_str(&format!("<title>{}</title>\n<link>{}</link>\n<description>{}</description>\n", esc(&format!("{} - watchlist of {}", display_name(&db.settings), user.username)), esc(&base), esc("Movies and shows on the watchlist")));
    for id in ids {
        let Some(it) = db.items.get(&id) else { continue };
        if !db.can_see_item(user, it) { continue; }
        let title = match it.year { Some(y) => format!("{} ({y})", it.title), None => it.title.clone() };
        let link = format!("{base}/#/item/{id}");
        out.push_str(&format!("<item><title>{}</title><link>{}</link><guid isPermaLink=\"false\">lichtspiel-{}-{id}</guid><pubDate>{}</pubDate><description>{}</description></item>\n",
            esc(&title), esc(&link), esc(&db.machine_id), rfc822(db.view(user.id, id).watchlist_at), esc(&it.summary)));
    }
    out.push_str("</channel></rss>\n");
    bytes(200, "application/rss+xml; charset=utf-8", out.into_bytes())
}

/// GET/POST /api/rss: the feed address of the signed-in person.
/// POST {enabled: true|false, regenerate: true}
pub fn manage(app: &App, ctx: &Ctx, user_id: u64) -> Resp {
    if ctx.method == "POST" {
        let on = ctx.body.get("enabled").and_then(|v| v.as_bool());
        let regen = ctx.body.get("regenerate").and_then(|v| v.as_bool()).unwrap_or(false);
        {
            let mut db = app.db.lock().unwrap();
            let Some(u) = db.users.iter_mut().find(|u| u.id == user_id) else { return fail(404, "No such user") };
            match on {
                Some(false) => u.rss_key.clear(),
                Some(true) if u.rss_key.is_empty() => u.rss_key = crate::db::random_hex(16),
                _ => {}
            }
            if regen && !u.rss_key.is_empty() { u.rss_key = crate::db::random_hex(16); }
        }
        app.save();
    }
    let db = app.db.lock().unwrap();
    let key = db.user(user_id).map(|u| u.rss_key.clone()).unwrap_or_default();
    let url = if key.is_empty() { String::new() } else { format!("{}/rss/watchlist/{key}.xml", base_url(&db, ctx)) };
    ok(json!({"enabled": !key.is_empty(), "url": url}))
}

#[cfg(test)]
mod tests {
    #[test]
    fn dates() {
        assert_eq!(super::rfc822(0), "Thu, 01 Jan 1970 00:00:00 +0000");
        assert_eq!(super::rfc822(1_793_686_026), "Tue, 03 Nov 2026 06:07:06 +0000");
    }
}

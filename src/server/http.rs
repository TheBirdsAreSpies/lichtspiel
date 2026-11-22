//! Building answers: headers, the web app's scripts and security policy, JSON and error replies.

use super::*;

pub fn header(k: &str, v: &str) -> Header {
    Header::from_bytes(k.as_bytes(), v.as_bytes()).unwrap_or_else(|_| Header::from_bytes(&b"X-Bad"[..], &b"1"[..]).unwrap())
}


pub fn base_headers() -> Vec<Header> {
    vec![
        // No Access-Control-Allow-Origin: pages of other websites must not read the answers
        // (the web app is on this server itself, the apps are no browsers).
        header("X-Server", &format!("{APP_NAME}/{VERSION}")),
        header("X-Content-Type-Options", "nosniff"),
        header("X-Frame-Options", "DENY"),
        // Links to other websites don't take the address along (it can hold a ?ticket= or the sign-in link's ?token=).
        header("Referrer-Policy", "no-referrer"),
    ]
}


/// An inline script's hash as the browser works it out: the browser reads the page with every line
/// break as a plain "\n", so a Windows checkout's "\r\n" (git with core.autocrlf) has to be hashed
/// the same way, or the browser refuses the scripts and the app never gets past "signed in".
pub(super) fn inline_script_hash(script: &str) -> String {
    use sha2::Digest;
    let text = script.replace("\r\n", "\n").replace('\r', "\n");
    format!("'sha256-{}'", b64(&sha2::Sha256::digest(text.as_bytes())))
}


/// What the web app's page may load: its own scripts (the one inline script by its hash), pictures
/// from anywhere, YouTube trailers in a frame; no other website may show the page in a frame.
pub fn web_csp() -> &'static str {
    static CSP: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    CSP.get_or_init(|| {
        let hashes: Vec<String> = WEB_APP.split("<script>").skip(1).filter_map(|s| s.split("</script>").next())
            .map(inline_script_hash).collect();
        // gstatic: Google's Cast library, loaded only in Chrome and Edge to send a video to a Chromecast.
        format!("default-src 'self'; script-src 'self' https://www.gstatic.com {}; style-src 'self' 'unsafe-inline'; img-src * data: blob:; media-src 'self' blob: data:; \
            frame-src https://www.youtube-nocookie.com; connect-src 'self'; object-src 'none'; base-uri 'none'; form-action 'self'; frame-ancestors 'none'", hashes.join(" "))
    })
}


/// A short fingerprint of the web app's scripts. The page asks for them as /web/app.js?v=<this>, so the
/// browser may keep them for good: a new version has new scripts, so a new fingerprint and new addresses.
pub fn web_version() -> &'static str {
    static V: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    V.get_or_init(|| {
        use sha2::Digest;
        let mut h = sha2::Sha256::new();
        for s in [WEB_JS, WEB_LIBRARY_JS, WEB_PLAYER_JS, WEB_PAGES_JS, WEB_DASHBOARD_JS, WEB_DOWNLOADS_JS, WEB_CAST_JS, WEB_I18N_JS] { h.update(s.as_bytes()); }
        for (name, s) in WEB_LANGS { h.update(name.as_bytes()); h.update(s.as_bytes()); }
        h.finalize().iter().take(6).map(|b| format!("{b:02x}")).collect()
    })
}


/// A script of the web app: kept for good when asked for with the current fingerprint (see `web_version`),
/// otherwise checked with the server each time.
pub fn web_script(body: &str, params: &[(String, String)]) -> Resp {
    let mut r = bytes(200, "text/javascript; charset=utf-8", body.as_bytes().to_vec());
    let current = params.iter().any(|(k, v)| k == "v" && v == web_version());
    r.add_header(header("Cache-Control", if current { "public, max-age=31536000, immutable" } else { "no-cache" }));
    r
}


/// A file of the Vue web app (/vue/…, see VUE_FILES). Its page for the address itself and index.html: checked with
/// the server each time, as it names the current scripts. Those (assets/, with a fingerprint in the name)
/// are kept for good. Without a build, the page says how to make one.
pub fn vue_file(rest: &[&str]) -> Resp {
    let path = rest.join("/");
    let page = path.is_empty() || path == "index.html";
    let Some((_, body)) = VUE_FILES.iter().find(|f| f.0 == if page { "index.html" } else { path.as_str() }) else {
        if !page { return bytes(404, "text/plain; charset=utf-8", b"Not found".to_vec()); }
        let note = "<!doctype html><meta charset=utf-8><title>Lichtspiel</title><body style=\"font-family:sans-serif;padding:2em\">\
            <h1>The Vue web app is not built</h1><p>Run <code>npm ci</code> and <code>npm run build</code> in <code>web-vue/</code>, then build the server again. \
            The usual web app is at <a href=\"/\">/</a>.</p>";
        return bytes(200, "text/html; charset=utf-8", note.as_bytes().to_vec());
    };
    let ext = path.rsplit('.').next().unwrap_or("");
    let ctype = if page { "text/html; charset=utf-8" } else {
        match ext {
            "js" | "mjs" => "text/javascript; charset=utf-8",
            "css" => "text/css; charset=utf-8",
            "svg" => "image/svg+xml",
            "png" => "image/png",
            "ico" => "image/x-icon",
            "woff2" => "font/woff2",
            "json" | "map" => "application/json",
            "webmanifest" => "application/manifest+json",
            _ => "application/octet-stream",
        }
    };
    let mut r = bytes(200, ctype, body.to_vec());
    if page {
        // Like the usual web app's (web_csp), without inline scripts or YouTube: the build has none.
        // gstatic: Google's Cast library, loaded only in Chrome and Edge to send a video to a Chromecast.
        r.add_header(header("Content-Security-Policy", "default-src 'self'; script-src 'self' https://www.gstatic.com; style-src 'self' 'unsafe-inline'; img-src * data: blob:; \
            media-src 'self' blob: data:; connect-src 'self'; object-src 'none'; base-uri 'none'; form-action 'self'; frame-ancestors 'none'"));
        r.add_header(header("Cache-Control", "no-cache"));
    } else {
        r.add_header(header("Cache-Control", if path.starts_with("assets/") { "public, max-age=31536000, immutable" } else { "no-cache" }));
    }
    r
}


/// Standard base64 (with padding).
pub fn b64(data: &[u8]) -> String {
    const ABC: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for c in data.chunks(3) {
        let n = (c[0] as u32) << 16 | (*c.get(1).unwrap_or(&0) as u32) << 8 | *c.get(2).unwrap_or(&0) as u32;
        for i in 0..4 { out.push(if i <= c.len() { ABC[(n >> (18 - 6 * i) & 63) as usize] as char } else { '=' }); }
    }
    out
}


pub fn bytes(status: u16, ctype: &str, body: Vec<u8>) -> Resp {
    let len = body.len();
    let mut h = base_headers();
    h.push(header("Content-Type", ctype));
    Response::new(StatusCode(status), h, Box::new(std::io::Cursor::new(body)) as Box<dyn Read + Send>, Some(len), None)
}


pub fn js(status: u16, v: Value) -> Resp {
    // A reply that sets no cache rule of its own
    // gets "no-cache", so lists and details are never shown from a stale copy.
    let mut r = bytes(status, "application/json; charset=utf-8", v.to_string().into_bytes());
    r.add_header(header("Cache-Control", "no-cache"));
    r
}

/// What a plugin needs to know about an item (see `App::item_brief`).
pub(super) fn brief_in(db: &Db, id: u64) -> Value {
    let Some(it) = db.items.get(&id) else { return Value::Null };
    let ids = |i: &Item| i.extra.get("ids").cloned().unwrap_or_else(|| json!({}));
    // The poster's file on this computer (a plugin can attach it to a message).
    let poster = |i: &Item| i.poster.as_ref().filter(|p| Path::new(p).is_file()).cloned();
    let show_of = |s: &Item| json!({"id": s.id, "title": s.title, "year": s.year, "ids": ids(s), "poster": poster(s), "genres": s.genres});
    let mut o = json!({"id": it.id, "type": it.kind.as_str(), "title": it.title, "year": it.year, "duration_ms": it.duration_ms, "ids": ids(it), "genres": it.genres, "poster": poster(it)});
    let parent = it.parent_id.and_then(|p| db.items.get(&p));
    match it.kind {
        ItemKind::Episode => {
            o["episode"] = json!(it.index);
            if let Some(season) = parent {
                o["season"] = json!(season.index);
                if let Some(show) = season.parent_id.and_then(|g| db.items.get(&g)) { o["show"] = show_of(show); }
            }
        }
        ItemKind::Season => {
            o["season"] = json!(it.index);
            if let Some(show) = parent { o["show"] = show_of(show); }
        }
        _ => {}
    }
    o
}


/// `brief_in` plus the item's files (see `App::item_detail`).
pub(super) fn detail_in(db: &Db, id: u64) -> Value {
    let mut o = brief_in(db, id);
    let Some(it) = db.items.get(&id) else { return Value::Null };
    let files: Vec<Value> = it.file.iter().chain(it.parts.iter()).map(|f| {
        let streams: Vec<Value> = f.streams.iter().map(|s| json!({"kind": s.kind, "codec": s.codec, "language": s.language, "channels": s.channels,
            "width": s.width, "height": s.height, "forced": s.forced, "sdh": s.sdh})).collect();
        json!({"path": f.path, "size": f.size, "container": f.container, "duration_ms": f.duration_ms, "streams": streams, "external_subtitles": f.external_subtitles})
    }).collect();
    o["section_id"] = json!(it.section_id);
    o["files"] = json!(files);
    o
}



/// The name the server shows: the
/// name set in Settings; when that is empty, the computer's description (Windows) or
/// its network name; and if none can be read, a fixed fallback.
pub fn display_name(settings: &crate::data::db::Settings) -> String {
    let set = settings.server_name.trim();
    if !set.is_empty() {
        return set.to_string();
    }
    computer_name().unwrap_or_else(|| APP_NAME.to_string())
}


fn computer_name() -> Option<String> {
    #[cfg(windows)]
    {
        let desc = std::process::Command::new("reg")
            .args(["query", r"HKLM\SYSTEM\CurrentControlSet\services\LanmanServer\Parameters", "/v", "srvcomment"])
            .output()
            .ok()
            .and_then(|o| String::from_utf8(o.stdout).ok())
            .and_then(|s| s.lines().find(|l| l.contains("srvcomment")).and_then(|l| l.split("REG_SZ").nth(1)).map(|v| v.trim().to_string()))
            .filter(|v| !v.is_empty());
        if desc.is_some() {
            return desc;
        }
    }
    std::env::var("COMPUTERNAME")
        .or_else(|_| std::env::var("HOSTNAME"))
        .ok()
        .or_else(|| std::fs::read_to_string("/etc/hostname").ok())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}


pub fn ok(v: Value) -> Resp {
    js(200, v)
}


pub fn fail(status: u16, msg: &str) -> Resp {
    js(status, json!({"error": msg, "status": status}))
}


/// Lists are wrapped in a MediaContainer.
pub fn container(list: Vec<Value>, total: usize, offset: usize, extra: Value) -> Value {
    let mut mc = json!({"size": list.len(), "totalSize": total, "offset": offset, "Metadata": list});
    if let Value::Object(e) = extra {
        for (k, v) in e {
            mc[k] = v;
        }
    }
    json!({"MediaContainer": mc})
}


/// A projector lamp, its beam and the screen it falls on.
pub const FAVICON: &str = "<svg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 32 32'><defs><linearGradient id='b'><stop offset='0' stop-color='#ff5e7e' stop-opacity='.95'/><stop offset='1' stop-color='#ff5e7e' stop-opacity='.15'/></linearGradient></defs><rect width='32' height='32' rx='7' fill='#14181c'/><path d='M7 16 24 6.5v19z' fill='url(#b)'/><circle cx='7' cy='16' r='3.2' fill='#ff5e7e'/><rect x='24.5' y='5.5' width='2.8' height='21' rx='1.4' fill='#ff5e7e'/></svg>";


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64() {
        assert_eq!([b64(b""), b64(b"f"), b64(b"fo"), b64(b"foo"), b64(b"foob")], ["", "Zg==", "Zm8=", "Zm9v", "Zm9vYg=="]);
    }


    #[test]
    fn an_inline_script_checked_out_with_windows_line_breaks_gets_the_hash_the_browser_works_out() {
        // What a browser hashes for "a\nb" (it reads "\r\n" and a lone "\r" as "\n").
        let browser = { use sha2::Digest; format!("'sha256-{}'", b64(&sha2::Sha256::digest(b"\na();\nb();\n"))) };
        assert_eq!(inline_script_hash("\na();\nb();\n"), browser);
        assert_eq!(inline_script_hash("\r\na();\r\nb();\r\n"), browser);
        assert_eq!(inline_script_hash("\ra();\rb();\r"), browser);
    }

}

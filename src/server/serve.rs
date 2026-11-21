//! The request loop: listening (with or without TLS), the request context, and handing each request to
//! the routes (with gzip for browsers that take it).

use super::*;

/// How long a stopped playback can't start again unless its player heard why
/// (keeps players that reconnect by themselves from carrying on).
pub const STOP_HOLD: Duration = Duration::from_secs(30);


pub struct Ctx {
    pub method: String,
    pub path: String,
    pub params: Vec<(String, String)>,
    pub body: Value,
    pub user: Option<User>,
    pub host: String,
    pub range: Option<String>,
    pub token: Option<String>,
    /// The caller's IP address (behind a trusted reverse proxy: the one it forwarded).
    pub remote: Option<IpAddr>,
    /// `remote` can be relied on (no forwarding header from an untrusted machine).
    pub remote_trusted: bool,
    /// The caller's address is in the "home network" list of the settings.
    pub lan_extra: bool,
    /// The device asking: Plex apps name themselves (X-Plex-Product, X-Plex-Device-Name),
    /// browsers by their User-Agent ("Chrome on Windows").
    pub device: String,
}


impl Ctx {
    /// The request comes from this computer or the local network (private, link-local
    /// or loopback address). A request passed on by a reverse proxy counts only when the proxy
    /// is trusted in the settings, and then by the address it forwarded.
    pub fn is_local(&self) -> bool {
        self.remote_trusted && (self.remote.is_some_and(ip_is_local) || self.lan_extra)
    }

    pub fn viewer(&self) -> Viewer {
        Viewer { device: self.device.clone(), ip: self.remote.map(|a| a.to_string()).unwrap_or_default(), local: self.is_local() }
    }
    pub fn param(&self, k: &str) -> Option<&str> {
        self.params.iter().find(|(a, _)| a.eq_ignore_ascii_case(k)).map(|(_, v)| v.as_str())
    }
    pub fn num(&self, k: &str) -> Option<u64> {
        self.param(k).and_then(|v| v.parse().ok())
    }
    pub fn body_or_param(&self, k: &str) -> Option<String> {
        match self.body.get(k) {
            Some(Value::String(s)) => Some(s.clone()),
            Some(Value::Number(n)) => Some(n.to_string()),
            Some(Value::Bool(b)) => Some(b.to_string()),
            _ => self.param(k).map(String::from),
        }
    }
}


pub fn url_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'+' => out.push(b' '),
            b'%' if i + 2 < b.len() => {
                let h = std::str::from_utf8(&b[i + 1..i + 3]).ok().and_then(|h| u8::from_str_radix(h, 16).ok());
                match h {
                    Some(v) => {
                        out.push(v);
                        i += 2;
                    }
                    None => out.push(b'%'),
                }
            }
            c => out.push(c),
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).to_string()
}


pub fn parse_query(q: &str) -> Vec<(String, String)> {
    q.split('&')
        .filter(|p| !p.is_empty())
        .map(|p| match p.split_once('=') {
            Some((k, v)) => (url_decode(k), url_decode(v)),
            None => (url_decode(p), String::new()),
        })
        .collect()
}


/// Where the server listens ("address:port") and the certificate and key files it uses (empty = no HTTPS).
pub static LISTEN: Mutex<String> = Mutex::new(String::new());

pub static TLS_FILES: Mutex<(String, String)> = Mutex::new((String::new(), String::new()));

/// How often the certificate files are looked at for a renewal.
const TLS_CHECK_EVERY: Duration = Duration::from_secs(30);


fn tls_stamp() -> Vec<Option<std::time::SystemTime>> {
    let (c, k) = TLS_FILES.lock().unwrap().clone();
    [c, k].iter().map(|p| std::fs::metadata(p.trim()).and_then(|m| m.modified()).ok()).collect()
}


/// Listens again with the (renewed) certificate. The port is free for a moment only after the old
/// listener is gone, so this tries for a while.
fn relisten(pair: &(Vec<u8>, Vec<u8>)) -> Option<Arc<tiny_http::Server>> {
    let addr = LISTEN.lock().unwrap().clone();
    for _ in 0..40 {
        match tiny_http::Server::https(addr.as_str(), tiny_http::SslConfig { certificate: pair.0.clone(), private_key: pair.1.clone() }) {
            Ok(s) => return Some(Arc::new(s)),
            Err(_) => std::thread::sleep(Duration::from_millis(250)),
        }
    }
    None
}


pub fn serve(app: Arc<App>, server: Arc<tiny_http::Server>) {
    let mut server = Some(server);
    let mut stamp = tls_stamp();
    let mut checked = Instant::now();
    let mut good = {
        let (c, k) = TLS_FILES.lock().unwrap().clone();
        read_tls(&c, &k).ok().flatten()
    };
    loop {
        let Some(s) = server.as_ref() else { return };
        match s.recv_timeout(Duration::from_secs(5)) {
            Ok(Some(req)) => {
                let app = app.clone();
                std::thread::spawn(move || handle(app, req));
            }
            Ok(None) => {}
            Err(_) => return,
        }
        // A renewed certificate (certbot, acme.sh ...): listen again with it, no restart needed.
        if good.is_some() && checked.elapsed() >= TLS_CHECK_EVERY {
            checked = Instant::now();
            let now_stamp = tls_stamp();
            if now_stamp != stamp {
                stamp = now_stamp;
                let (c, k) = TLS_FILES.lock().unwrap().clone();
                match read_tls(&c, &k) {
                    Ok(Some(pair)) => {
                        drop(server.take());
                        server = relisten(&pair).or_else(|| good.as_ref().and_then(relisten));
                        if server.is_none() { crate::admin::log::add("HTTPS: could not listen again after the certificate changed; restart the server".into()); return; }
                        good = Some(pair);
                        crate::admin::log::add("HTTPS: the new certificate is in use".into());
                    }
                    Ok(None) => {}
                    Err(e) => crate::admin::log::add(format!("HTTPS: the changed certificate files are not used ({e})")),
                }
            }
        }
    }
}


/// Answers every request on a plain http:// port with a redirect to the same page on https://.
pub fn redirect_to_https(bind: &str, port: u16, https_port: u16) -> Result<(), String> {
    let server = tiny_http::Server::http(format!("{bind}:{port}")).map_err(|e| e.to_string())?;
    std::thread::spawn(move || {
        for req in server.incoming_requests() {
            let host = req.headers().iter().find(|h| h.field.equiv("Host")).map(|h| h.value.as_str().to_string()).unwrap_or_default();
            let name = if let Some(rest) = host.strip_prefix('[') { format!("[{}", rest.split_once(']').map(|(a, _)| format!("{a}]")).unwrap_or_default()) } else { host.split(':').next().unwrap_or("").to_string() };
            let name = if name.is_empty() || name == "[" { "localhost".to_string() } else { name };
            let target = if https_port == 443 { format!("https://{name}{}", req.url()) } else { format!("https://{name}:{https_port}{}", req.url()) };
            let loc = Header::from_bytes(&b"Location"[..], target.as_bytes()).unwrap();
            let _ = req.respond(Response::empty(308).with_header(loc));
        }
    });
    Ok(())
}


/// "Plex for Samsung — TV 2020" from the X-Plex headers (or the same names in the
/// address, as some Plex apps send them), else the browser ("Chrome on Windows").
pub fn device_label(product: &str, device: &str, params: &[(String, String)], agent: &str) -> String {
    let param = |k: &str| params.iter().find(|(n, _)| n.eq_ignore_ascii_case(k)).map(|(_, v)| v.trim().to_string()).unwrap_or_default();
    let product = if product.trim().is_empty() { param("X-Plex-Product") } else { product.trim().to_string() };
    let mut device = if device.trim().is_empty() { param("X-Plex-Device-Name") } else { device.trim().to_string() };
    if device.is_empty() { device = param("X-Plex-Device"); }
    let label = match (product.is_empty(), device.is_empty() || device == product) {
        (false, false) => format!("{product} — {device}"),
        (false, true) => product,
        (true, false) => device,
        (true, true) => crate::admin::stats::short_agent(agent),
    };
    // The caller picks these headers; keep only what fits in the list of devices.
    label.chars().take(100).collect()
}


/// Whether an Origin header names another address than the one this server was reached at
/// (`host`, or one of `forwarded_hosts` from a trusted proxy).
pub(super) fn origin_is_other_site(origin: &str, host: &str, forwarded_hosts: &[String]) -> bool {
    let Some((_, o)) = origin.split_once("://") else { return true };
    o != host.to_ascii_lowercase() && !forwarded_hosts.iter().any(|h| h == o)
}


/// Whether an Accept-Encoding header allows gzip (and does not turn it off with q=0).
fn accepts_gzip(v: &str) -> bool {
    v.split(',').any(|part| {
        let mut it = part.split(';').map(str::trim);
        let name = it.next().unwrap_or("").to_ascii_lowercase();
        let off = it.any(|p| p.strip_prefix("q=").and_then(|q| q.parse::<f32>().ok()).is_some_and(|q| q <= 0.0));
        (name == "gzip" || name == "*") && !off
    })
}


/// Text answers (the web app's page and scripts, JSON, XML) packed with gzip. Pictures, video and
/// downloads are packed already or are streamed, and small answers would not get smaller.
fn gzipped(resp: Resp) -> Resp {
    let has = |name: &'static str| resp.headers().iter().any(|h| h.field.equiv(name));
    let ctype = resp.headers().iter().find(|h| h.field.equiv("Content-Type")).map(|h| h.value.as_str().to_ascii_lowercase()).unwrap_or_default();
    let text = ["text/", "application/json", "application/xml", "application/javascript"].iter().any(|t| ctype.starts_with(t));
    let fits = resp.data_length().is_some_and(|n| (1024..=32 << 20).contains(&n));
    if !text || !fits || resp.status_code().0 != 200 || has("Content-Encoding") || has("Content-Range") { return resp; }
    let (status, mut headers) = (resp.status_code(), resp.headers().to_vec());
    let mut body = Vec::new();
    if resp.into_reader().read_to_end(&mut body).is_err() { return bytes(500, "text/plain", b"Could not read the answer".to_vec()); }
    let level = if body.len() > 1 << 20 { flate2::Compression::fast() } else { flate2::Compression::default() };
    let mut enc = flate2::write::GzEncoder::new(Vec::with_capacity(body.len() / 4), level);
    let packed = std::io::Write::write_all(&mut enc, &body).and_then(|_| enc.finish());
    let body = match packed { Ok(p) => { headers.push(header("Content-Encoding", "gzip")); p } Err(_) => body };
    headers.push(header("Vary", "Accept-Encoding"));
    let len = body.len();
    Response::new(status, headers, Box::new(std::io::Cursor::new(body)) as Box<dyn Read + Send>, Some(len), None)
}


pub(super) fn handle(app: Arc<App>, mut req: tiny_http::Request) {
    let method = req.method().as_str().to_uppercase();
    let url = req.url().to_string();
    let (path, query) = url.split_once('?').unwrap_or((&url, ""));
    let path = url_decode(path);
    let params = parse_query(query);
    let mut host = String::new();
    let mut token = None;
    let mut range = None;
    let mut agent = String::new();
    let (mut product, mut device_name) = (String::new(), String::new());
    let (mut xff, mut real_ip, mut forwarded) = (Vec::new(), None, Vec::new());
    let (mut fetch_site, mut origin) = (String::new(), None);
    let mut fwd_host = Vec::new();
    let mut gzip_ok = false;
    for h in req.headers() {
        let f = h.field.as_str().as_str().to_ascii_lowercase();
        match f.as_str() {
            "host" => host = h.value.as_str().to_string(),
            "x-token" => token = Some(h.value.as_str().to_string()),
            "authorization" => {
                let v = h.value.as_str();
                token = Some(v.strip_prefix("Bearer ").unwrap_or(v).to_string());
            }
            "range" => range = Some(h.value.as_str().to_string()),
            "user-agent" => agent = h.value.as_str().to_string(),
            "x-plex-product" => product = h.value.as_str().to_string(),
            "x-plex-device-name" => device_name = h.value.as_str().to_string(),
            "x-plex-device" | "x-plex-model" if device_name.is_empty() => device_name = h.value.as_str().to_string(),
            "x-forwarded-for" => xff.push(h.value.as_str().to_string()),
            "x-real-ip" => real_ip = Some(h.value.as_str().to_string()),
            "forwarded" => forwarded.push(h.value.as_str().to_string()),
            "sec-fetch-site" => fetch_site = h.value.as_str().to_ascii_lowercase(),
            "origin" => origin = Some(h.value.as_str().to_ascii_lowercase()),
            "x-forwarded-host" => fwd_host.extend(h.value.as_str().split(',').map(|x| x.trim().to_ascii_lowercase())),
            "accept-encoding" => gzip_ok = accepts_gzip(h.value.as_str()),
            _ => {}
        }
    }
    // The token comes only in a header (X-Token or Authorization). Addresses end up in proxy logs and
    // browser histories, so ?token= is not taken; an address that has to work on its own uses ?ticket=.
    let mut body = Value::Null;
    if matches!(method.as_str(), "POST" | "PUT" | "PATCH" | "DELETE") {
        let mut buf = Vec::new();
        // A plugin uploaded as a zip (base64) may be bigger than anything else that is sent.
        let limit = if path == "/api/plugins/upload" { 48 } else { 4 };
        let _ = req.as_reader().take(limit * 1024 * 1024).read_to_end(&mut buf);
        if !buf.is_empty() {
            body = serde_json::from_slice(&buf).unwrap_or_else(|_| {
                // Accept form posts too.
                let s = String::from_utf8_lossy(&buf).to_string();
                Value::Object(parse_query(&s).into_iter().map(|(k, v)| (k, Value::String(v))).collect())
            });
        }
    }
    let device = device_label(&product, &device_name, &params, &agent);
    // A caller that came through the relay: the relay told us who it is (nobody else can).
    let relayed = req.remote_addr().and_then(|a| crate::connect::relay::real_client(*a));
    let peer = relayed.map(|a| a.to_string()).or_else(|| req.remote_addr().map(|a| a.ip().to_string())).unwrap_or_default();
    let user = token.as_ref().and_then(|t| {
        let mut db = app.db.lock().unwrap();
        let at = now();
        let hash = crate::accounts::auth::token_hash(t);
        let tk = db.tokens.iter_mut().find(|x| x.token == hash)?;
        // The list of devices says when each was used last (written at most once a minute).
        if at.saturating_sub(tk.last_used) > 60 { tk.last_used = at; tk.last_device = device.clone(); tk.last_ip = peer.clone(); }
        let uid = tk.user_id;
        db.user(uid).cloned()
    });
    // A ticket (?ticket=, from /api/ticket) opens only the addresses it was made for, while its sign-in lasts (one made without a token, in a network that needs no sign-in, has an empty id and is bound to its user and expiry only).
    let user = match params.iter().find(|(k, _)| k == "ticket") {
        Some((_, t)) if user.is_none() && matches!(method.as_str(), "GET" | "HEAD") => {
            let db = app.db.lock().unwrap();
            crate::accounts::auth::read_ticket(&db.ticket_key, t, now())
                .filter(|(uid, tid, scope)| crate::accounts::auth::ticket_scope(&path).as_ref() == Some(scope) && (tid.is_empty() || db.tokens.iter().any(|x| x.user_id == *uid && crate::accounts::auth::stored_token_id(&x.token) == tid)))
                .and_then(|(uid, ..)| db.user(uid).cloned())
        }
        _ => user,
    };
    let (trusted, no_auth, lan_list) = { let db = app.db.lock().unwrap(); (db.settings.trusted_proxies.clone(), db.settings.no_auth_networks.clone(), db.settings.lan_networks.clone()) };
    let (remote, remote_trusted) = match relayed {
        // The relay does not prove who it is, so a relayed caller never counts as local or as a no-sign-in network.
        Some(ip) => (Some(ip), false),
        None => client_address(req.remote_addr().map(|a| a.ip()), &xff, real_ip.as_deref(), &forwarded, &trusted),
    };
    // The browser says the request comes from a page of another website (a link, form or script there).
    // Over plain http there is no Sec-Fetch-Site, so then an Origin for another address counts too.
    // A trusted proxy that rewrites Host says the address the browser used in X-Forwarded-Host, which counts as ours as well.
    let from_trusted_proxy = req.remote_addr().is_some_and(|a| ip_in_list(&trusted, a.ip()));
    let other_site = if fetch_site.is_empty() { origin.as_deref().is_some_and(|o| origin_is_other_site(o, &host, if from_trusted_proxy { &fwd_host } else { &[] })) }
        else { matches!(fetch_site.as_str(), "cross-site" | "same-site") };
    // Networks that need no sign-in act as the first admin (only with an address that is believed,
    // and never for another website's page).
    let user = if user.is_none() && !other_site && remote_trusted && !no_auth.trim().is_empty() && remote.is_some_and(|a| ip_in_list(&no_auth, a)) {
        app.db.lock().unwrap().users.iter().filter(|u| u.admin).min_by_key(|u| u.id).cloned()
    } else { user };
    let uid = user.as_ref().map(|u| u.id);
    if let Some(id) = uid {
        app.stats.saw(id, remote.map(|a| a.to_string()).unwrap_or_default(), &agent);
    }
    let lan_extra = !lan_list.trim().is_empty() && remote.is_some_and(|a| ip_in_list(&lan_list, a));
    let ctx = Ctx { method: method.clone(), path, params, body, user, host, range, token, remote, remote_trusted, lan_extra, device };
    let is_local = ctx.is_local();
    let started = Instant::now();
    let mut resp = if method == "OPTIONS" { bytes(204, "text/plain", vec![]) }
        else if other_site && (!matches!(method.as_str(), "GET" | "HEAD") || ctx.path.starts_with("/api/home")) { fail(403, "Requests from other websites are not allowed") }
        else { crate::api::routes::route(&app, &ctx) };
    // How long the server needed to answer (not to send): in the browser's network tab as Server-Timing,
    // and in the server log when it was slow (playing and converting go on for long by design).
    let took = started.elapsed().as_millis();
    resp.add_header(header("Server-Timing", &format!("app;dur={took}")));
    if took >= 400 && !["/parts/", "/transcode", "/video/", "/hls", "/stream"].iter().any(|p| ctx.path.contains(p)) {
        crate::admin::log::add(format!("Slow answer: {} {} took {} ms", method, ctx.path, took));
    }
    if gzip_ok { resp = gzipped(resp); }
    // Count what is sent (bandwidth on the dashboard).
    let (status, headers, len) = (resp.status_code(), resp.headers().to_vec(), resp.data_length());
    let remote = if is_local { None } else { Some(app.stats.bytes_remote.clone()) };
    let body = crate::admin::stats::Counting { inner: resp.into_reader(), all: app.stats.bytes_out.clone(), user: app.stats.counter_for(uid), remote };
    let resp: Resp = Response::new(status, headers, Box::new(body) as Box<dyn Read + Send>, len, None);
    let _ = req.respond(resp);
}


pub fn base_url(db: &Db, ctx: &Ctx) -> String {
    if !db.settings.public_url.trim().is_empty() {
        return db.settings.public_url.trim().trim_end_matches('/').to_string();
    }
    if ctx.host.is_empty() { format!("{}://localhost:{DEFAULT_PORT}", scheme()) } else { format!("{}://{}", scheme(), ctx.host) }
}


/// The server speaks HTTPS (a certificate is set).
pub static TLS_ON: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);


/// "https" or "http", for the addresses the server hands out.
pub fn scheme() -> &'static str {
    if TLS_ON.load(std::sync::atomic::Ordering::Relaxed) { "https" } else { "http" }
}


/// Both PEM files read, or why not. (None, None) = no certificate asked for.
pub fn read_tls(cert: &str, key: &str) -> Result<Option<(Vec<u8>, Vec<u8>)>, String> {
    let (cert, key) = (cert.trim(), key.trim());
    if cert.is_empty() && key.is_empty() { return Ok(None); }
    if cert.is_empty() || key.is_empty() { return Err("HTTPS needs both the certificate file and the key file".into()); }
    let c = std::fs::read(cert).map_err(|e| format!("could not read the certificate {cert}: {e}"))?;
    let k = std::fs::read(key).map_err(|e| format!("could not read the key {key}: {e}"))?;
    if !String::from_utf8_lossy(&c).contains("-----BEGIN CERTIFICATE-----") { return Err(format!("{cert} is not a PEM certificate (it must contain -----BEGIN CERTIFICATE-----)")); }
    if !String::from_utf8_lossy(&k).contains("PRIVATE KEY-----") { return Err(format!("{key} is not a PEM private key")); }
    // The key must be the one of the certificate (else every browser fails the handshake, silently).
    use rustls::pki_types::{CertificateDer, PrivateKeyDer, pem::PemObject};
    let chain: Vec<CertificateDer<'static>> = CertificateDer::pem_slice_iter(&c).collect::<Result<_, _>>().map_err(|e| format!("{cert} is not a valid PEM certificate: {e}"))?;
    let parsed = PrivateKeyDer::from_pem_slice(&k).map_err(|e| format!("{key} is not a valid PEM private key (an encrypted key is not supported): {e}"))?;
    let signing = rustls::crypto::ring::sign::any_supported_type(&parsed).map_err(|e| format!("{key}: this kind of key is not supported ({e})"))?;
    if rustls::sign::CertifiedKey::new(chain, signing).keys_match().is_err() { return Err(format!("{key} is not the private key of the certificate {cert}")); }
    Ok(Some((c, k)))
}


#[cfg(test)]
mod tests {
    use super::*;

    fn body(r: Resp) -> (Vec<String>, Vec<u8>) {
        let heads = r.headers().iter().map(|h| format!("{}: {}", h.field, h.value)).collect();
        let mut b = Vec::new();
        r.into_reader().read_to_end(&mut b).unwrap();
        (heads, b)
    }

    #[test]
    fn text_is_packed_only_for_browsers_that_take_gzip() {
        assert!(accepts_gzip("gzip, deflate, br") && accepts_gzip("br;q=1.0, gzip;q=0.8") && accepts_gzip("*"));
        assert!(!accepts_gzip("") && !accepts_gzip("br") && !accepts_gzip("gzip;q=0") && !accepts_gzip("identity"));
        let text = "Ein langer Text. ".repeat(500);
        let (heads, packed) = body(gzipped(bytes(200, "text/javascript; charset=utf-8", text.clone().into_bytes())));
        assert!(heads.iter().any(|h| h == "Content-Encoding: gzip") && packed.len() < text.len() / 10);
        let mut back = String::new();
        flate2::read::GzDecoder::new(&packed[..]).read_to_string(&mut back).unwrap();
        assert_eq!(back, text);
        // Pictures, small answers and errors stay as they are.
        for r in [bytes(200, "image/jpeg", vec![0; 5000]), bytes(200, "application/json", b"{}".to_vec()), bytes(404, "text/plain", vec![b'x'; 5000])] {
            assert!(!body(gzipped(r)).0.iter().any(|h| h.starts_with("Content-Encoding")));
        }
    }

    #[test]
    fn the_page_asks_for_scripts_that_may_be_kept() {
        let v = web_version().to_string();
        let kept = |p: Vec<(String, String)>| body(web_script("x", &p)).0.iter().any(|h| h.contains("immutable"));
        assert!(kept(vec![("v".into(), v)]));
        assert!(!kept(vec![]) && !kept(vec![("v".into(), "old".into())]));
    }
}

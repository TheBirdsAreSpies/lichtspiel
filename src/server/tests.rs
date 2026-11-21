//! Tests for the server, and helpers other modules' tests use too
//! (`crate::server::tests::test_app`, `ask`, `local_web`).

use super::*;
use crate::data::db::MediaFile;

#[test]
fn a_relayed_caller_claiming_a_home_address_is_not_local() {
    use std::io::Write;
    let root = std::env::temp_dir().join(format!("lichtspiel-relayed-test-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let app = App::open(&root);
    {
        let mut db = app.db.lock().unwrap();
        db.settings.no_auth_networks = "192.168.0.0/16".into();
        db.users.push(serde_json::from_value(json!({"id": 1, "username": "admin", "password_hash": "x", "salt": "x", "admin": true, "created_at": 0})).unwrap());
    }
    let server = tiny_http::Server::http("127.0.0.1:0").unwrap();
    let mut c = std::net::TcpStream::connect(server.server_addr().to_ip().unwrap()).unwrap();
    // The relay (or someone pretending to be it) says this caller is at home.
    crate::connect::relay::RELAYED.lock().unwrap().get_or_insert_with(HashMap::new).insert(c.local_addr().unwrap(), "192.168.1.10".parse().unwrap());
    write!(c, "GET /api/settings HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n").unwrap();
    handle(app.clone(), server.recv().unwrap());
    let mut got = String::new();
    let _ = c.read_to_string(&mut got);
    crate::connect::relay::RELAYED.lock().unwrap().as_mut().unwrap().remove(&c.local_addr().unwrap());
    assert!(got.starts_with("HTTP/1.1 401"), "a relayed caller got in without signing in: {}", got.lines().next().unwrap_or(""));
    let _ = std::fs::remove_dir_all(&root);
}

/// Sends one raw request through `handle` and returns the whole answer.
pub(crate) fn ask(app: &Arc<App>, raw: &str) -> String {
    use std::io::Write;
    let server = tiny_http::Server::http("127.0.0.1:0").unwrap();
    let mut c = std::net::TcpStream::connect(server.server_addr().to_ip().unwrap()).unwrap();
    c.write_all(raw.as_bytes()).unwrap();
    handle(app.clone(), server.recv().unwrap());
    let mut got = String::new();
    let _ = c.read_to_string(&mut got);
    got
}

#[test]
fn sign_in_tokens_are_stored_only_as_a_hash() {
    let (app, root) = test_app("token-hash");
    let (new, old) = {
        let mut db = app.db.lock().unwrap();
        // A token saved by an older version, as it was given out.
        let old = crate::data::db::random_hex(20);
        db.tokens.push(crate::data::db::Token { token: old.clone(), user_id: 2, created_at: 0, device: String::new(), last_used: 0, last_device: String::new(), last_ip: String::new() });
        (crate::api::handlers::new_token(&mut db, 1, "test"), old)
    };
    app.save();
    drop(app);
    let app = App::open(&root);
    app.save();
    let stored = serde_json::to_string(&app.db.lock().unwrap().tokens).unwrap();
    assert!(!stored.contains(&new) && !stored.contains(&old), "a sign-in token is stored as it is: {stored}");
    for (t, who) in [(&new, "admin"), (&old, "kid")] {
        let r = ask(&app, &format!("GET /api/me HTTP/1.1\r\nHost: x\r\nX-Token: {t}\r\nConnection: close\r\n\r\n"));
        assert!(r.contains(&format!("\"username\":\"{who}\"")), "the {who} token stopped working: {r}");
    }
    // The list of devices names the current one, and it can be signed out by its id.
    let list = ask(&app, &format!("GET /api/tokens HTTP/1.1\r\nHost: x\r\nX-Token: {new}\r\nConnection: close\r\n\r\n"));
    assert!(list.contains(&format!("\"id\":\"{}\"", crate::accounts::auth::token_id(&new))) && list.contains("\"current\":true"), "{list}");
    let _ = ask(&app, &format!("POST /api/logout HTTP/1.1\r\nHost: x\r\nX-Token: {old}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"));
    assert_eq!(app.db.lock().unwrap().tokens.len(), 1, "signing out did not end the old token");
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn device_names_are_kept_short() {
    let long = "x".repeat(5000);
    assert!(device_label(&long, "", &[], "").chars().count() <= 100);
    assert!(device_label("", "", &[], &long).chars().count() <= 100);
    let (app, root) = test_app("device-name");
    let mut db = app.db.lock().unwrap();
    crate::api::handlers::new_token(&mut db, 1, &long);
    assert!(db.tokens[0].device.chars().count() <= 100, "a sign-in kept a {} letter device name", db.tokens[0].device.len());
    drop(db);
    let _ = std::fs::remove_dir_all(&root);
}

/// A data folder with a first admin (id 1) and a profile without PIN (id 2) on the sign-in screen.
pub(crate) fn test_app(name: &str) -> (Arc<App>, PathBuf) {
    let root = std::env::temp_dir().join(format!("lichtspiel-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let app = App::open(&root);
    {
        let mut db = app.db.lock().unwrap();
        db.settings.home_profiles = true;
        db.users.push(serde_json::from_value(json!({"id": 1, "username": "admin", "password_hash": "x", "salt": "x", "admin": true, "created_at": 0})).unwrap());
        db.users.push(serde_json::from_value(json!({"id": 2, "username": "kid", "password_hash": "x", "salt": "x", "admin": false, "home": true, "created_at": 0})).unwrap());
    }
    (app, root)
}

#[test]
fn the_web_app_page_cannot_be_framed_and_runs_only_its_own_scripts() {
    let (app, root) = test_app("headers");
    let got = ask(&app, "GET / HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n");
    let head = got.split("\r\n\r\n").next().unwrap().to_ascii_lowercase();
    for want in ["x-frame-options: deny", "x-content-type-options: nosniff", "referrer-policy: no-referrer", "frame-ancestors 'none'", "object-src 'none'"] {
        assert!(head.contains(want), "missing {want}: {head}");
    }
    let csp = head.lines().find(|l| l.starts_with("content-security-policy:")).unwrap();
    let script = csp.split(';').find(|d| d.trim().starts_with("script-src")).unwrap();
    assert!(!script.contains("unsafe-inline"), "any inline script may run: {script}");
    // The page's own inline script is allowed by its hash.
    let inline = WEB_APP.split("<script>").nth(1).unwrap().split("</script>").next().unwrap();
    assert!(script.contains(&inline_script_hash(inline).to_ascii_lowercase()), "{script}");
    let _ = std::fs::remove_dir_all(&root);
}

/// For a Chromecast: the lines of a converted stream that starts later are moved back, and Google's
/// player page may read them.
#[test]
fn subtitles_for_a_stream_that_starts_later() {
    let (app, root) = test_app("cast-subs");
    let media = root.join("media");
    std::fs::create_dir_all(&media).unwrap();
    std::fs::write(media.join("Film.mkv"), "video").unwrap();
    std::fs::write(media.join("Film.de.vtt"), "WEBVTT\n\n00:00:10.000 --> 00:00:12.000\nhallo\n").unwrap();
    let tok = {
        let mut db = app.db.lock().unwrap();
        db.sections.push(serde_json::from_value(json!({"id": 1, "name": "Filme", "kind": "movie", "paths": [media.to_string_lossy()], "created_at": 0})).unwrap());
        let mut i: Item = serde_json::from_value(json!({"id": 1, "section_id": 1, "kind": "movie", "title": "x", "sort_title": "x", "added_at": 0, "updated_at": 0})).unwrap();
        i.file = Some(MediaFile { path: media.join("Film.mkv").to_string_lossy().to_string(), container: "matroska".into(),
            external_subtitles: vec![media.join("Film.de.vtt").to_string_lossy().to_string()], ..Default::default() });
        db.items.insert(1, i);
        crate::api::handlers::new_token(&mut db, 2, "test")
    };
    let get = |path: &str| ask(&app, &format!("GET {path} HTTP/1.1\r\nHost: x\r\nX-Token: {tok}\r\nConnection: close\r\n\r\n"));
    assert!(get("/library/parts/1/subtitles/x0").contains("00:00:10.000 --> 00:00:12.000"));
    let moved = get("/library/parts/1/subtitles/x0?from=4.5");
    assert!(moved.contains("00:00:05.500 --> 00:00:07.500\nhallo"), "{moved}");
    assert!(moved.to_ascii_lowercase().contains("access-control-allow-origin: *"), "{moved}");
    let _ = std::fs::remove_dir_all(&root);
}

#[cfg(unix)]
#[test]
fn files_outside_the_library_folders_are_never_sent() {
    let (app, root) = test_app("outside");
    let media = root.join("media");
    std::fs::create_dir_all(media.join("Film")).unwrap();
    std::fs::write(root.join("secret.txt"), "WEBVTT\n\ntop secret").unwrap();
    std::fs::write(media.join("Film/Film.mkv"), "video").unwrap();
    std::fs::write(media.join("Film/Film.de.vtt"), "WEBVTT\n\nhallo").unwrap();
    std::os::unix::fs::symlink(root.join("secret.txt"), media.join("Film/Film.en.vtt")).unwrap();
    std::os::unix::fs::symlink(root.join("secret.txt"), media.join("Film/Leak.mkv")).unwrap();
    let tok = {
        let mut db = app.db.lock().unwrap();
        db.sections.push(serde_json::from_value(json!({"id": 1, "name": "Filme", "kind": "movie", "paths": [media.to_string_lossy()], "created_at": 0})).unwrap());
        for (id, f) in [(1u64, "Film/Film.mkv"), (2, "Film/Leak.mkv")] {
            let mut i: Item = serde_json::from_value(json!({"id": id, "section_id": 1, "kind": "movie", "title": "x", "sort_title": "x", "added_at": 0, "updated_at": 0})).unwrap();
            let subs = vec![media.join("Film/Film.de.vtt").to_string_lossy().to_string(), media.join("Film/Film.en.vtt").to_string_lossy().to_string()];
            i.file = Some(MediaFile { path: media.join(f).to_string_lossy().to_string(), container: "matroska".into(), external_subtitles: subs, ..Default::default() });
            db.items.insert(id, i);
        }
        crate::api::handlers::new_token(&mut db, 2, "test")
    };
    let get = |path: &str| ask(&app, &format!("GET {path} HTTP/1.1\r\nHost: x\r\nX-Token: {tok}\r\nConnection: close\r\n\r\n"));
    assert!(get("/library/parts/1/subtitles/x0").contains("hallo"), "a subtitle in the library is not sent");
    assert!(get("/library/parts/1/file").ends_with("video"), "a video in the library is not sent");
    assert!(!get("/library/parts/1/subtitles/x1").contains("top secret"), "a subtitle linked to a file outside the library was sent");
    assert!(!get("/library/parts/2/file").contains("top secret"), "a video linked to a file outside the library was sent");
    let _ = std::fs::remove_dir_all(&root);
}

/// A tiny web server on this computer that counts its callers and answers with `answer`.
pub(crate) fn local_web(answer: &'static [u8]) -> (String, Arc<std::sync::atomic::AtomicUsize>) {
    use std::io::Write;
    let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = l.local_addr().unwrap().to_string();
    let hits = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let h = hits.clone();
    std::thread::spawn(move || for mut s in l.incoming().flatten() {
        h.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let mut buf = [0u8; 2048];
        let _ = s.read(&mut buf);
        let _ = s.write_all(answer);
    });
    (addr, hits)
}

#[test]
fn a_picture_address_may_not_reach_this_computer_or_the_home_network() {
    let (app, root) = test_app("ssrf");
    let (addr, hits) = local_web(b"HTTP/1.1 200 OK\r\nContent-Length: 8\r\nConnection: close\r\n\r\n\x89PNGxxxx");
    {
        let mut db = app.db.lock().unwrap();
        let i: Item = serde_json::from_value(json!({"id": 1, "section_id": 1, "kind": "movie", "title": "x", "sort_title": "x", "added_at": 0, "updated_at": 0})).unwrap();
        db.items.insert(1, i);
    }
    let r = crate::api::handlers::set_image(&app, "1", "poster", &json!({"url": format!("http://{addr}/p.png")}));
    assert_ne!(r.status_code().0, 200);
    assert_eq!(hits.load(std::sync::atomic::Ordering::SeqCst), 0, "the server fetched a picture from this computer");
    for blocked in ["127.0.0.1:80", "10.1.2.3:80", "192.168.1.1:80", "172.16.0.1:80", "169.254.169.254:80", "100.64.0.1:80", "0.0.0.0:80", "[::1]:80", "[fe80::1]:80", "[fd00::1]:80", "[::ffff:192.168.1.1]:80"] {
        assert!(public_only(blocked).is_err(), "{blocked} is allowed");
    }
    assert!(public_only("8.8.8.8:443").is_ok());
    let _ = std::fs::remove_dir_all(&root);
}

/// test_app plus one movie (id 1) in library 1, which the profile "kid" may not see; tokens for admin and kid.
fn app_with_hidden_movie(name: &str) -> (Arc<App>, PathBuf, String, String) {
    let (app, root) = test_app(name);
    let mut db = app.db.lock().unwrap();
    db.sections.push(serde_json::from_value(json!({"id": 1, "name": "Filme", "kind": "movie", "paths": [root.join("media").to_string_lossy()], "created_at": 0})).unwrap());
    let i: Item = serde_json::from_value(json!({"id": 1, "section_id": 1, "kind": "movie", "title": "x", "sort_title": "x", "added_at": 0, "updated_at": 0})).unwrap();
    db.items.insert(1, i);
    db.users[1].allowed_sections = Some(vec![]);
    let (a, k) = (crate::api::handlers::new_token(&mut db, 1, "test"), crate::api::handlers::new_token(&mut db, 2, "test"));
    drop(db);
    (app, root, a, k)
}

/// "Forgot password": the link from the mail sets a new password once, signs out every device,
/// and the request answers the same for a name that does not exist.
#[test]
fn a_reset_link_sets_the_password_once_and_signs_out_everywhere() {
    let (app, root, _, kid) = app_with_hidden_movie("reset");
    let post = |path: &str, body: &str, tok: &str| ask(&app, &format!("POST {path} HTTP/1.1\r\nHost: evil.example\r\nX-Token: {tok}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()));
    // Without a mail server nothing can be sent, and the server says so.
    assert!(post("/api/password/forgot", r#"{"name":"x"}"#, "").starts_with("HTTP/1.1 503"));
    {
        let mut db = app.db.lock().unwrap();
        db.settings.smtp_host = "127.0.0.1".into();
        db.settings.smtp_port = 9; // nobody answers: the mail fails in the background
        db.settings.smtp_from = "kino@example.com".into();
        db.users[1].email = "kid@example.com".into();
    }
    let known = post("/api/password/forgot", r#"{"name":"KID@example.com"}"#, "");
    let unknown = post("/api/password/forgot", r#"{"name":"nobody"}"#, "");
    assert!(known.starts_with("HTTP/1.1 200") && unknown.starts_with("HTTP/1.1 200"), "{known}\n{unknown}");
    assert_eq!(known.rsplit("\r\n\r\n").next(), unknown.rsplit("\r\n\r\n").next(), "the answer tells nothing");
    let (uid, code) = {
        let db = app.db.lock().unwrap();
        let u = &db.users[1];
        (u.id, crate::accounts::auth::reset_code(&db.ticket_key, u.id, &u.password_hash, crate::data::db::now()))
    };
    assert!(ask(&app, &format!("GET /api/password/reset/{code} HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n")).starts_with("HTTP/1.1 200"));
    assert!(post(&format!("/api/password/reset/{code}"), r#"{"password":"123"}"#, "").starts_with("HTTP/1.1 400"), "too short");
    assert!(post(&format!("/api/password/reset/{code}x"), r#"{"password":"neues-passwort"}"#, "").starts_with("HTTP/1.1 410"));
    let got = post(&format!("/api/password/reset/{code}"), r#"{"password":"neues-passwort"}"#, "");
    assert!(got.starts_with("HTTP/1.1 200"), "{got}");
    assert!(post(&format!("/api/password/reset/{code}"), r#"{"password":"noch-eins"}"#, "").starts_with("HTTP/1.1 410"), "the link works once");
    assert!(ask(&app, &format!("GET /api/me HTTP/1.1\r\nHost: x\r\nX-Token: {kid}\r\nConnection: close\r\n\r\n")).starts_with("HTTP/1.1 401"), "old sign-ins end");
    let name = app.db.lock().unwrap().user(uid).unwrap().username.clone();
    assert!(post("/api/login", &format!(r#"{{"username":"{name}","password":"neues-passwort"}}"#), "").starts_with("HTTP/1.1 200"));
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn a_profile_cannot_mark_rate_or_favourite_what_it_may_not_see() {
    let (app, root, _, kid) = app_with_hidden_movie("can-see");
    for (m, path) in [("PUT", "/:/scrobble?key=1"), ("PUT", "/:/unscrobble?key=1"), ("PUT", "/:/rate?key=1&rating=8"), ("PUT", "/api/items/1/favorite"),
        ("POST", "/api/continue/remove?x=1")] {
        let body = if path.starts_with("/api/continue") { r#"{"ids":[1]}"# } else { "" };
        let got = ask(&app, &format!("{m} {path} HTTP/1.1\r\nHost: x\r\nX-Token: {kid}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()));
        assert!(got.starts_with("HTTP/1.1 404"), "{m} {path}: {}", got.lines().next().unwrap_or(""));
    }
    let db = app.db.lock().unwrap();
    assert!(db.views.get("2:1").is_none(), "the profile wrote watch state for a hidden movie");
    drop(db);
    let _ = std::fs::remove_dir_all(&root);
}

/// The routes touched by the security fixes need sign-in or admin: anonymous, a profile and the admin.
#[test]
fn routes_need_sign_in_or_admin() {
    let (app, root, admin, kid) = app_with_hidden_movie("access");
    app.db.lock().unwrap().users[1].allowed_sections = None;
    // (method, path, anonymous, profile, admin): the status each one gets.
    let table: &[(&str, &str, u16, u16, u16)] = &[
        ("GET", "/api/home", 200, 200, 200),
        ("PUT", "/:/scrobble?key=1", 401, 200, 200),
        ("PUT", "/:/unscrobble?key=1", 401, 200, 200),
        ("PUT", "/:/rate?key=1&rating=8", 401, 200, 200),
        ("PUT", "/api/items/1/favorite", 401, 200, 200),
        ("GET", "/library/parts/1/subtitles/x0", 401, 400, 400),
        ("GET", "/library/parts/1/file", 401, 400, 400),
        ("PUT", "/library/metadata/1/image/poster", 401, 403, 400),
        ("GET", "/api/settings", 401, 403, 200),
        ("GET", "/api/plex-sync", 401, 403, 200),
    ];
    for &(m, path, want_anon, want_kid, want_admin) in table {
        for (who, tok, want) in [("anonymous", "", want_anon), ("profile", kid.as_str(), want_kid), ("admin", admin.as_str(), want_admin)] {
            let got = ask(&app, &format!("{m} {path} HTTP/1.1\r\nHost: x\r\nX-Token: {tok}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"));
            assert!(got.starts_with(&format!("HTTP/1.1 {want}")), "{m} {path} as {who}: {}", got.lines().next().unwrap_or(""));
        }
    }
    let _ = std::fs::remove_dir_all(&root);
}

/// Every address in ROUTES needs sign-in unless it is on the short open list below, and every
/// address described as "Admin:" refuses a signed-in person who is not an admin.
#[test]
fn every_route_needs_sign_in_and_every_admin_route_needs_an_admin() {
    const OPEN: &[(&str, &str)] = &[
        ("GET", "/api/status"), ("POST", "/api/setup"), ("POST", "/api/login"), ("POST", "/api/link"), ("GET", "/api/link/{id}"),
        ("GET", "/api/home"), ("POST", "/api/home/{id}/login"), ("GET", "/identity"),
        ("GET", "/api/invites/{code}"), ("POST", "/api/invites/{code}/accept"), ("GET", "/rss/watchlist/{key}.xml"),
        // "Forgot password": the link in the mail is the proof (see auth::reset_code).
        ("POST", "/api/password/forgot"), ("GET", "/api/password/reset/{code}"), ("POST", "/api/password/reset/{code}"),
        // A plugin's own key instead of a sign-in (it is refused without it, see the test of reads_library).
        ("GET", "/api/plugins/{id}/library"), ("GET", "/api/plugins/{id}/history"),
    ];
    for open in OPEN {
        assert!(crate::api::routes::ROUTES.iter().any(|r| (r.0, r.1) == *open), "{open:?} is on the open list but not in ROUTES");
    }
    let (app, root, _, kid) = app_with_hidden_movie("every-route");
    let fill = |path: &str| path.replace("{id}", "1").replace("{itemId}", "1").replace("{n}", "0").replace("{key}", "x").replace("{code}", "x").replace("{action}", "x");
    let call = |m: &str, path: &str, tok: &str| {
        let got = ask(&app, &format!("{m} {path} HTTP/1.1\r\nHost: x\r\nX-Token: {tok}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"));
        got.lines().next().unwrap_or("").to_string()
    };
    let mut wrong = Vec::new();
    for &(m, path, what) in crate::api::routes::ROUTES {
        let at = fill(path);
        if !OPEN.contains(&(m, path)) {
            let got = call(m, &at, "");
            if !got.starts_with("HTTP/1.1 401") { wrong.push(format!("{m} {path} without sign-in: {got}")); }
        }
        if what.starts_with("Admin:") {
            let got = call(m, &at, &kid);
            if !got.starts_with("HTTP/1.1 403") { wrong.push(format!("{m} {path} as a profile: {got}")); }
        }
    }
    let _ = std::fs::remove_dir_all(&root);
    assert!(wrong.is_empty(), "{} routes let the wrong person in:\n{}", wrong.len(), wrong.join("\n"));
}

/// Video, subtitle and picture addresses carry a short-lived ticket instead of the sign-in token.
#[test]
fn an_origin_is_ours_when_it_names_the_host_or_a_trusted_proxys_forwarded_host() {
    assert!(!origin_is_other_site("http://nas:8484", "nas:8484", &[]));
    assert!(origin_is_other_site("http://evil.com", "nas:8484", &[]));
    assert!(origin_is_other_site("null", "nas:8484", &[]));
    // Behind a proxy that rewrites Host, the address the browser used counts.
    assert!(!origin_is_other_site("http://films.home", "127.0.0.1:8484", &["films.home".to_string()]));
    assert!(origin_is_other_site("http://evil.com", "127.0.0.1:8484", &["films.home".to_string()]));
}

#[test]
fn a_ticket_opens_only_its_own_item_while_the_sign_in_lasts() {
    let (app, root) = test_app("ticket");
    let media = root.join("media");
    std::fs::create_dir_all(media.join("Film")).unwrap();
    std::fs::write(media.join("Film/A.mkv"), "video-a").unwrap();
    std::fs::write(media.join("Film/B.mkv"), "video-b").unwrap();
    let (tok, kid) = {
        let mut db = app.db.lock().unwrap();
        db.sections.push(serde_json::from_value(json!({"id": 1, "name": "Filme", "kind": "movie", "paths": [media.to_string_lossy()], "created_at": 0})).unwrap());
        for (id, f) in [(1u64, "Film/A.mkv"), (2, "Film/B.mkv")] {
            let mut i: Item = serde_json::from_value(json!({"id": id, "section_id": 1, "kind": "movie", "title": "x", "sort_title": "x", "added_at": 0, "updated_at": 0})).unwrap();
            i.file = Some(MediaFile { path: media.join(f).to_string_lossy().to_string(), container: "matroska".into(), ..Default::default() });
            db.items.insert(id, i);
        }
        (crate::api::handlers::new_token(&mut db, 1, "test"), crate::api::handlers::new_token(&mut db, 2, "test"))
    };
    let get = |path: &str, extra: &str| ask(&app, &format!("GET {path} HTTP/1.1\r\nHost: x\r\n{extra}Connection: close\r\n\r\n"));
    let status = |r: &str| r.lines().next().unwrap_or("").to_string();
    let ticket_of = |r: &str| { let body = r.split("\r\n\r\n").nth(1).unwrap_or(""); serde_json::from_str::<Value>(body).ok().and_then(|v| v["ticket"].as_str().map(String::from)).unwrap_or_default() };
    let t1 = ticket_of(&get("/api/ticket?item=1", &format!("X-Token: {tok}\r\n")));
    assert!(!t1.is_empty() && !t1.contains(&tok), "no ticket for item 1: {t1}");
    // The ticket plays item 1, also in pieces (seeking)...
    assert!(get(&format!("/library/parts/1/file?ticket={t1}"), "").ends_with("video-a"));
    assert!(status(&get(&format!("/library/parts/1/file?ticket={t1}"), "Range: bytes=0-1\r\n")).contains("206"));
    // ...but nothing else.
    for p in ["/library/parts/2/file", "/video/transcode/2/stream.mp4", "/api/me", "/api/tokens"] {
        assert!(status(&get(&format!("{p}?ticket={t1}"), "")).contains("401"), "{p} opened with item 1's ticket");
    }
    // A changed ticket, an expired one and one for another item with item 1's signature are refused.
    let key = app.db.lock().unwrap().ticket_key.clone();
    let tampered = format!("{}{}", &t1[..t1.len() - 1], if t1.ends_with('0') { '1' } else { '0' });
    let (old, _) = crate::accounts::auth::ticket(&key, 1, "1", Some(&tok), now() - 7 * 3600);
    let swapped = t1.replacen(".1.", ".2.", 1);
    for t in [tampered, old, swapped.clone(), "1.2.3".into(), String::new()] {
        assert!(status(&get(&format!("/library/parts/1/file?ticket={t}"), "")).contains("401"), "ticket {t} was let in");
        assert!(status(&get(&format!("/library/parts/2/file?ticket={t}"), "")).contains("401"), "ticket {t} was let in");
    }
    // A ticket made without a token (a network that needs no sign-in) still opens its item.
    let (nt, _) = crate::accounts::auth::ticket(&key, 1, "1", None, now());
    assert!(get(&format!("/library/parts/1/file?ticket={nt}"), "").ends_with("video-a"), "a ticket without a sign-in was refused");
    // A profile that may not see the movie gets no ticket, and a made-up one does not show it either.
    app.db.lock().unwrap().users[1].allowed_sections = Some(vec![]);
    assert!(status(&get("/api/ticket?item=1", &format!("X-Token: {kid}\r\n"))).contains("404"));
    let (kt, _) = crate::accounts::auth::ticket(&key, 2, "1", Some(&kid), now());
    assert!(!get(&format!("/library/parts/1/file?ticket={kt}"), "").ends_with("video-a"), "a hidden movie was sent");
    // The picture ticket opens pictures, not videos; a ticket for any other address is not given.
    let tp = ticket_of(&get("/api/ticket", &format!("X-Token: {tok}\r\n")));
    assert!(!status(&get(&format!("/library/metadata/1/thumb?ticket={tp}"), "")).contains("401"));
    assert!(status(&get(&format!("/library/parts/1/file?ticket={tp}"), "")).contains("401"));
    assert!(status(&get("/api/ticket?path=/api/tokens", &format!("X-Token: {tok}\r\n"))).contains("400"));
    // The play decision hands out ticket addresses, not the sign-in token.
    let d = get("/library/metadata/1/decision", &format!("X-Token: {tok}\r\n"));
    assert!(d.contains("ticket=") && !d.contains(&tok), "{d}");
    // The token in the address is not taken, and signing out ends the tickets too.
    assert!(status(&get(&format!("/library/parts/1/file?token={tok}"), "")).contains("401"), "?token= in the address was taken");
    let _ = ask(&app, &format!("POST /api/logout HTTP/1.1\r\nHost: x\r\nX-Token: {tok}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"));
    assert!(status(&get(&format!("/library/parts/1/file?ticket={t1}"), "")).contains("401"), "a ticket outlived its sign-in");
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn other_websites_cannot_read_the_answers() {
    let (app, root) = test_app("cors");
    let got = ask(&app, "GET /identity HTTP/1.1\r\nHost: x\r\nOrigin: https://evil.example\r\nConnection: close\r\n\r\n");
    assert!(got.starts_with("HTTP/1.1 200"), "{got}");
    assert!(!got.to_ascii_lowercase().contains("access-control-allow-origin"), "another website may read the answer: {got}");
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn other_websites_cannot_pick_a_profile_or_use_the_no_sign_in_network() {
    let (app, root) = test_app("csrf");
    app.db.lock().unwrap().settings.no_auth_networks = "127.0.0.1".into();
    let login = |site: &str| ask(&app, &format!("POST /api/home/2/login HTTP/1.1\r\nHost: x\r\nSec-Fetch-Site: {site}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"));
    assert!(login("cross-site").starts_with("HTTP/1.1 403"), "a page of another website got a profile's token");
    assert!(login("same-origin").starts_with("HTTP/1.1 200"), "the web app itself can't pick a profile");
    // A no-sign-in network makes the web app an admin, but not a link or picture on another website.
    let settings = |site: &str| ask(&app, &format!("GET /api/settings HTTP/1.1\r\nHost: x\r\nSec-Fetch-Site: {site}\r\nConnection: close\r\n\r\n"));
    assert!(settings("same-origin").starts_with("HTTP/1.1 200"));
    assert!(settings("cross-site").starts_with("HTTP/1.1 401"), "another website acted as the admin");
    // Over plain http browsers send no Sec-Fetch-Site, but they do send Origin with a post.
    let post = |origin: &str| ask(&app, &format!("POST /api/home/2/login HTTP/1.1\r\nHost: x:8484\r\n{origin}Content-Length: 0\r\nConnection: close\r\n\r\n"));
    assert!(post("Origin: http://evil.example\r\n").starts_with("HTTP/1.1 403"), "a form on another website was let through over http");
    assert!(post("Origin: http://X:8484\r\n").starts_with("HTTP/1.1 200"), "the web app itself was refused over http");
    assert!(post("").starts_with("HTTP/1.1 200"), "an app that sends no Origin was refused");
    let _ = std::fs::remove_dir_all(&root);
}

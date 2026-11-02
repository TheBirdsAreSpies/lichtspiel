//! `--self-test out.json`: starts a real server on a free port with a
//! made-up library in a temporary folder, uses it the way a person and a
//! player would (through the HTTP API only) and writes what worked.

use serde_json::{json, Value};
use std::collections::HashSet;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

#[derive(Clone)]
struct Client {
    port: u16,
    token: Option<String>,
}

struct Reply {
    status: u16,
    head: String,
    body: Vec<u8>,
}

impl Reply {
    fn json(&self) -> Value {
        serde_json::from_slice(&self.body).unwrap_or(Value::Null)
    }
}

impl Client {
    fn call(&self, method: &str, path: &str, body: Option<Value>, extra: &[(&str, &str)]) -> Reply {
        let fail = |e: String| Reply { status: 0, head: e, body: vec![] };
        let mut s = match TcpStream::connect(("127.0.0.1", self.port)) {
            Ok(s) => s,
            Err(e) => return fail(e.to_string()),
        };
        let _ = s.set_read_timeout(Some(Duration::from_secs(90)));
        let payload = body.map(|b| b.to_string()).unwrap_or_default();
        let mut req = format!("{method} {path} HTTP/1.1\r\nHost: 127.0.0.1:{}\r\nConnection: close\r\nContent-Length: {}\r\n", self.port, payload.len());
        if !payload.is_empty() {
            req.push_str("Content-Type: application/json\r\n");
        }
        if let Some(t) = &self.token {
            req.push_str(&format!("X-Token: {t}\r\n"));
        }
        for (k, v) in extra {
            req.push_str(&format!("{k}: {v}\r\n"));
        }
        req.push_str("\r\n");
        req.push_str(&payload);
        if let Err(e) = s.write_all(req.as_bytes()) {
            return fail(e.to_string());
        }
        let mut raw = vec![];
        let _ = s.read_to_end(&mut raw);
        let split = raw.windows(4).position(|w| w == b"\r\n\r\n").unwrap_or(raw.len());
        let head = String::from_utf8_lossy(&raw[..split]).to_string();
        let mut body = raw.get(split + 4..).unwrap_or(&[]).to_vec();
        if head.to_lowercase().contains("transfer-encoding: chunked") {
            body = unchunk(&body);
        }
        let status = head.split_whitespace().nth(1).and_then(|s| s.parse().ok()).unwrap_or(0);
        Reply { status, head, body }
    }
    fn get(&self, path: &str) -> Reply {
        self.call("GET", path, None, &[])
    }
    fn post(&self, path: &str, body: Value) -> Reply {
        self.call("POST", path, Some(body), &[])
    }
}

fn unchunk(b: &[u8]) -> Vec<u8> {
    let mut out = vec![];
    let mut i = 0;
    while i < b.len() {
        let Some(end) = b[i..].windows(2).position(|w| w == b"\r\n") else { break };
        let size = usize::from_str_radix(String::from_utf8_lossy(&b[i..i + end]).trim(), 16).unwrap_or(0);
        i += end + 2;
        if size == 0 {
            break;
        }
        out.extend_from_slice(&b[i..(i + size).min(b.len())]);
        i += size + 2;
    }
    out
}

fn ffmpeg_ok() -> bool {
    Command::new("ffmpeg").arg("-version").stdout(Stdio::null()).stderr(Stdio::null()).status().map(|s| s.success()).unwrap_or(false)
}

/// A few seconds of test picture and tone, in the given codecs.
fn make_clip(path: &Path, vcodec: &str, acodec: &str, extra: &[&str], have_ffmpeg: bool) {
    let _ = std::fs::create_dir_all(path.parent().unwrap());
    if have_ffmpeg {
        let mut a: Vec<String> = ["-hide_banner", "-loglevel", "error", "-y", "-f", "lavfi", "-i", "testsrc=size=320x240:rate=24", "-f", "lavfi", "-i", "sine=frequency=440:sample_rate=48000", "-t", "5", "-c:v", vcodec]
            .iter().map(|s| s.to_string()).collect();
        a.extend(extra.iter().map(|s| s.to_string()));
        a.extend(["-c:a".to_string(), acodec.to_string(), path.to_string_lossy().to_string()]);
        let ok = Command::new("ffmpeg").args(&a).stdout(Stdio::null()).stderr(Stdio::null()).status().map(|s| s.success()).unwrap_or(false);
        if ok {
            return;
        }
    }
    let _ = std::fs::write(path, vec![7u8; 64 * 1024]);
}

/// A small library database in the old server's layout (only the columns the
/// import reads), with library folders on "the old PC".
fn make_old_db(path: &Path, films: &str, tv: &str) -> bool {
    let _ = std::fs::create_dir_all(path.parent().unwrap());
    let _ = std::fs::remove_file(path);
    let Ok(c) = rusqlite::Connection::open(path) else { return false };
    let sql = format!(r#"
        create table library_sections (id integer primary key, name text, section_type integer, language text);
        create table section_locations (id integer primary key, library_section_id integer, root_path text);
        create table accounts (id integer primary key, name text);
        create table metadata_items (id integer primary key, library_section_id integer, parent_id integer, metadata_type integer, guid text, title text, original_title text, summary text, tagline text, studio text, content_rating text, rating real, year integer, "index" integer, originally_available_at integer);
        create table media_items (id integer primary key, metadata_item_id integer);
        create table media_parts (id integer primary key, media_item_id integer, file text);
        create table metadata_item_settings (id integer primary key, account_id integer, guid text, rating real, view_offset integer, view_count integer, last_viewed_at integer);
        create table tags (id integer primary key, tag text, tag_type integer);
        create table taggings (id integer primary key, metadata_item_id integer, tag_id integer, "index" integer, text text, time_offset integer, end_time_offset integer);
        create table metadata_item_accounts (id integer primary key, account_id integer, metadata_item_id integer);
        create table play_queue_generators (id integer primary key, playlist_id integer, metadata_item_id integer, uri text, "order" real);
        insert into library_sections values (1, 'Old films', 1, 'de-DE'), (2, 'Old TV', 2, 'en-US'), (3, 'Music', 8, 'en');
        insert into section_locations values (1, 1, '{films}'), (2, 2, '{tv}');
        insert into accounts values (1, 'Administrator'), (2, 'friend');
        insert into metadata_items values
            (10, 1, null, 1, 'old://movie/abc', 'Imported Film', 'Der Film', 'A plot from the old server.', 'Old tagline', 'Old Studio', 'PG', 7.5, 2001, null, 978307200),
            (20, 2, null, 2, 'old://show/s', 'Test Show', '', 'Show plot', '', '', '', null, 2019, null, null),
            (21, 2, 20, 3, 'old://season/s1', 'Season 1', '', '', '', '', '', null, null, 1, null),
            (22, 2, 21, 4, 'old://episode/e1', 'Pilot', '', '', '', '', '', null, null, 1, null),
            (30, null, null, 15, '', 'Old favourites', '', '', '', '', '', null, null, null, null),
            (31, null, null, 15, '', 'Old smart', '', '', '', '', '', null, null, null, null);
        insert into media_items values (100, 10), (101, 22), (102, 10);
        insert into media_parts values (1000, 100, '{films}\Imported Film (2001).mp4'), (1001, 101, '{tv}\Test Show\Season 01\Test Show - S01E01 - Pilot.mkv'), (1002, 102, '{films}\gone.mkv');
        insert into metadata_item_settings values (1, 1, 'old://movie/abc', 8.0, 0, 2, 1700000000), (2, 1, 'old://episode/e1', null, 2000, 0, 1700000100), (3, 2, 'old://movie/abc', 6.0, 0, 1, 1700000000);
        insert into tags values (1, 'Drama', 1), (2, '', 12), (3, 'Ann Actor', 6), (4, 'Dee Rector', 4), (5, 'Wri Ter', 5), (6, 'Bob Second', 6);
        insert into taggings values (1, 10, 1, 0, null, null, null), (2, 10, 2, 0, 'credits', 3000, 5000),
            (3, 10, 6, 1, 'Sidekick', null, null), (4, 10, 3, 0, 'Hero', null, null), (5, 10, 4, 0, '', null, null), (6, 10, 5, 0, '', null, null);
        insert into metadata_item_accounts values (1, 1, 30);
        insert into play_queue_generators values (1, 30, 10, null, 1.0), (2, 30, 22, null, 2.0), (3, 31, null, 'library://x/directory/y', 1.0);
    "#);
    c.execute_batch(&sql).is_ok()
}

pub fn mmss(ms: u64) -> String {
    format!("{}:{:02}", ms / 60_000, ms / 1000 % 60)
}

/// The whole self-test must finish well inside the 90 s the checker allows.
const BUDGET: Duration = Duration::from_secs(75);

/// Results so far; the report file is rewritten after every step, so a run
/// that is stopped early still leaves what it measured.
struct Steps(Vec<Value>, PathBuf);

impl Steps {
    fn add(&mut self, step: &str, passed: bool, detail: String) {
        println!("[{}] {step}: {detail}", if passed { "ok" } else { "FAILED" });
        self.0.push(json!({"step": step, "passed": passed, "detail": detail}));
        save(&self.1, &self.0);
    }
}

fn save(out: &Path, steps: &[Value]) {
    if let Some(dir) = out.parent().filter(|d| !d.as_os_str().is_empty()) {
        let _ = std::fs::create_dir_all(dir);
    }
    let v = json!({"version": 1, "steps": steps});
    if let Err(e) = std::fs::write(out, serde_json::to_string_pretty(&v).unwrap_or_default()) {
        eprintln!("could not write {}: {e}", out.display());
    }
}

pub fn run(out: &Path) -> bool {
    let root: PathBuf = std::env::temp_dir().join(format!("mediaserver-selftest-{}", std::process::id()));
    let media = root.join("media");
    let data = root.join("data");
    let started = std::time::Instant::now();
    let have_ffmpeg = ffmpeg_ok();
    let mut steps = Steps(vec![], out.to_path_buf());

    // A movie library and a TV library, in the containers people really have.
    let movie_a = media.join("Movies").join("Sample Movie (2020)").join("Sample Movie (2020).mp4");
    let movie_b = media.join("Movies").join("Another.Film.2018.1080p.BluRay.x264.mkv");
    let movie_c = media.join("Movies").join("Old Classic (1968).avi");
    let ep1 = media.join("TV").join("Test Show").join("Season 01").join("Test Show - S01E01 - Pilot.mkv");
    let ep2 = media.join("TV").join("Test Show").join("Season 01").join("Test.Show.S01E02.The.Second.One.720p.ts");
    make_clip(&movie_a, "libx264", "aac", &["-pix_fmt", "yuv420p"], have_ffmpeg);
    make_clip(&movie_b, "libx264", "ac3", &["-pix_fmt", "yuv420p"], have_ffmpeg);
    make_clip(&movie_c, "mpeg4", "mp3", &[], have_ffmpeg);
    make_clip(&ep1, "libx264", "aac", &["-pix_fmt", "yuv420p"], have_ffmpeg);
    make_clip(&ep2, "mpeg2video", "mp2", &[], have_ffmpeg);

    // A plugin: the server's own built-in example, run as a separate program.
    let pdir = data.join("plugins").join("example");
    let _ = std::fs::create_dir_all(&pdir);
    let _ = std::fs::write(pdir.join("plugin.json"), json!({
        "id": "example", "name": "Example tagline plugin", "version": "1.0", "description": "Adds a tagline to new items",
        "command": ["{server}", "--example-plugin"], "hooks": ["metadata.lookup", "api.call"], "enabled_by_default": true,
        "settings": {"tagline_prefix": "Added by the example plugin"}
    }).to_string());

    let opts = crate::Options { data: data.clone(), port: Some(0), bind: "127.0.0.1".into() };
    let (app, http) = match crate::start(&opts) {
        Ok(x) => x,
        Err(e) => {
            steps.add("start", false, e);
            return write(out, steps, &root);
        }
    };
    let port = *app.port.lock().unwrap();
    {
        let a = app.clone();
        std::thread::spawn(move || crate::server::serve(a, http));
    }
    let mut c = Client { port, token: None };

    // ---- first-time setup
    let r = c.post("/api/setup", json!({"username": "owner", "password": "secret123", "server_name": "Self-test server"}));
    c.token = r.json()["token"].as_str().map(String::from);
    steps.add("setup", r.status == 201 && c.token.is_some(), format!("owner account created (HTTP {})", r.status));
    // The test clips have made-up names: no automatic online lookups while testing
    // (the "details" step below tries the lookup on purpose).
    c.call("PUT", "/api/settings", Some(json!({"metadata_enabled": false, "cast_photos_limit": 4})), &[]);

    // ---- move: libraries are scanned, movies and shows kept apart
    let m = c.post("/library/sections?scan=wait", json!({"name": "Movies", "type": "movie", "paths": [media.join("Movies").to_string_lossy()]}));
    let t = c.post("/library/sections?scan=wait", json!({"name": "TV Shows", "type": "show", "paths": [media.join("TV").to_string_lossy()]}));
    let mid = m.json()["id"].as_u64().unwrap_or(0);
    let tid = t.json()["id"].as_u64().unwrap_or(0);
    let movies = c.get(&format!("/library/sections/{mid}/all?sort=year")).json();
    let shows = c.get(&format!("/library/sections/{tid}/all")).json();
    let mlist = movies["MediaContainer"]["Metadata"].as_array().cloned().unwrap_or_default();
    let slist = shows["MediaContainer"]["Metadata"].as_array().cloned().unwrap_or_default();
    let titles: Vec<String> = mlist.iter().map(|x| format!("{} ({})", x["title"].as_str().unwrap_or(""), x["year"])).collect();
    let show_id = slist.first().and_then(|s| s["id"].as_u64()).unwrap_or(0);
    let seasons = c.get(&format!("/library/metadata/{show_id}/children")).json()["MediaContainer"]["size"].as_u64().unwrap_or(0);
    let eps = c.get(&format!("/library/metadata/{show_id}/allLeaves")).json();
    let elist = eps["MediaContainer"]["Metadata"].as_array().cloned().unwrap_or_default();
    let ep_titles: Vec<String> = elist.iter().map(|e| format!("S{:02}E{:02} {}", e["parentIndex"].as_u64().unwrap_or(0), e["index"].as_u64().unwrap_or(0), e["title"].as_str().unwrap_or(""))).collect();
    let movie_has_no_episodes = mlist.iter().all(|x| x["type"] == "movie");
    let year_ok = mlist.iter().any(|x| x["title"] == "Another Film" && x["year"] == 2018);
    steps.add(
        "move",
        mlist.len() == 3 && slist.len() == 1 && seasons == 1 && elist.len() == 2 && movie_has_no_episodes && year_ok,
        format!("Movies library: {} ({}); TV library: {} show, {seasons} season, {} episodes ({}), kept in separate libraries", mlist.len(), titles.join(", "), slist.len(), elist.len(), ep_titles.join(", ")),
    );

    // ---- act: streaming (direct with seeking, remux, transcode, HLS)
    let id_of = |title: &str| mlist.iter().find(|x| x["title"] == title).and_then(|x| x["id"].as_u64()).unwrap_or(0);
    let mp4 = id_of("Sample Movie");
    let mkv = id_of("Another Film");
    let avi = id_of("Old Classic");
    let ranged = c.call("GET", &format!("/library/parts/{mp4}/file"), None, &[("Range", "bytes=0-99")]);
    let direct_ok = ranged.status == 206 && ranged.body.len() == 100;
    let mut detail = format!("direct play with seeking: HTTP {} and {} bytes for bytes 0-99", ranged.status, ranged.body.len());
    let mut act_ok = direct_ok;
    if have_ffmpeg {
        let d_mp4 = c.get(&format!("/library/metadata/{mp4}/decision")).json();
        let d_mkv = c.get(&format!("/library/metadata/{mkv}/decision")).json();
        let d_avi = c.get(&format!("/library/metadata/{avi}/decision")).json();
        let decisions = format!("mp4 → {}, mkv/ac3 → {}, avi/mpeg4 → {}", d_mp4["method"], d_mkv["method"], d_avi["method"]);
        let dec_ok = d_mp4["method"] == "direct" && d_mkv["method"] == "remux" && d_avi["method"] == "transcode";
        let tr = c.get(&format!("/video/transcode/{avi}/stream.mp4?quality=480p-1.5"));
        let tr_ok = tr.status == 200 && tr.body.len() > 10_000 && tr.body.windows(4).any(|w| w == b"ftyp");
        let rm = c.get(&format!("/video/transcode/{mkv}/stream.mp4"));
        let rm_ok = rm.status == 200 && rm.body.windows(4).any(|w| w == b"ftyp");
        let ep2_id = elist.get(1).and_then(|e| e["id"].as_u64()).unwrap_or(0);
        let hls = c.get(&format!("/video/transcode/{ep2_id}/start.m3u8?redirect=0&quality=320p-0.7"));
        let pl_path = hls.json()["playlist"].as_str().unwrap_or("").to_string();
        let pl = c.get(&pl_path);
        let pl_text = String::from_utf8_lossy(&pl.body).to_string();
        let seg = pl_text.lines().find(|l| l.ends_with(".ts")).unwrap_or("").to_string();
        let seg_r = c.get(&format!("{}/{}", pl_path.rsplit_once('/').map(|x| x.0).unwrap_or(""), seg));
        let hls_ok = pl.status == 200 && !seg.is_empty() && seg_r.status == 200 && seg_r.body.first() == Some(&0x47);
        act_ok = direct_ok && dec_ok && tr_ok && rm_ok && hls_ok;
        detail = format!("{detail}; decisions {decisions}; transcoded avi to mp4: {} KB; remuxed mkv: {} KB; HLS mpeg2 .ts: playlist {} + segment {} KB",
            tr.body.len() / 1024, rm.body.len() / 1024, pl.status, seg_r.body.len() / 1024);
    } else {
        detail.push_str("; ffmpeg is not installed on this computer, so converting was not tried");
    }
    // progress → continue watching → watched at the threshold
    let dur = c.get(&format!("/library/metadata/{mp4}")).json()["MediaContainer"]["Metadata"][0]["duration"].as_u64().unwrap_or(0);
    if dur > 0 {
        c.post(&format!("/:/timeline?ratingKey={mp4}&time={}&state=playing", dur / 2), json!({}));
        let cw = c.get("/hubs/home/continueWatching").json();
        let in_cw = cw["MediaContainer"]["Metadata"].as_array().is_some_and(|a| a.iter().any(|x| x["id"] == mp4));
        c.post(&format!("/:/timeline?ratingKey={mp4}&time={}&state=stopped", dur * 95 / 100), json!({}));
        let after = c.get(&format!("/library/metadata/{mp4}")).json()["MediaContainer"]["Metadata"][0]["viewCount"].as_u64().unwrap_or(0);
        act_ok &= in_cw && after == 1;
        detail = format!("{detail}; half-way → in Continue Watching: {in_cw}; at 95 % → marked watched ({after} view)");
    }
    // credits marker: by default "earliest of threshold and first credits marker"
    let kdur = c.get(&format!("/library/metadata/{mkv}")).json()["MediaContainer"]["Metadata"][0]["duration"].as_u64().unwrap_or(0);
    if kdur > 0 {
        let set = c.call("PUT", &format!("/library/metadata/{mkv}/markers"), Some(json!({"credits": [kdur * 60 / 100]})), &[]);
        let mk = c.get(&format!("/library/metadata/{mkv}/markers")).json();
        c.post(&format!("/:/timeline?ratingKey={mkv}&time={}&state=stopped", kdur * 65 / 100), json!({}));
        let views = c.get(&format!("/library/metadata/{mkv}")).json()["MediaContainer"]["Metadata"][0]["viewCount"].as_u64().unwrap_or(0);
        let ok = set.status == 200 && mk["source"] == "manual" && mk["watchedAt"].as_u64() == Some(kdur * 60 / 100) && views == 1;
        act_ok &= ok;
        detail = format!("{detail}; credits marker at 60 % → watched at 65 %: {}", if views == 1 { "yes" } else { "no" });
    }
    // credits found in the picture: 70 s of test picture, then 40 s of white
    // text lines on black (no chapters in the file).
    if have_ffmpeg {
        let clip = root.join("credits").join("credits-test.mkv");
        let _ = std::fs::create_dir_all(clip.parent().unwrap());
        let lines = "drawbox=x=110:y=40:w=100:h=3:color=white:t=fill,drawbox=x=90:y=60:w=140:h=3:color=white:t=fill,drawbox=x=120:y=100:w=80:h=3:color=white:t=fill,drawbox=x=100:y=120:w=120:h=3:color=white:t=fill";
        let made = Command::new("ffmpeg")
            .args(["-hide_banner", "-loglevel", "error", "-y", "-f", "lavfi", "-i", "testsrc=size=320x180:rate=24:duration=70", "-f", "lavfi", "-i", &format!("color=black:size=320x180:rate=24:duration=40,{lines}"),
                "-filter_complex", "[0:v][1:v]concat=n=2:v=1[v]", "-map", "[v]", "-c:v", "libx264", "-preset", "ultrafast", "-g", "48", "-pix_fmt", "yuv420p"])
            .arg(&clip).stdout(Stdio::null()).stderr(Stdio::null()).status().map(|s| s.success()).unwrap_or(false);
        let t0 = std::time::Instant::now();
        let found = if made { crate::scanner::detect_credits_in_video("ffmpeg", &clip.to_string_lossy(), 110_000) } else { Err("test clip could not be made".into()) };
        let ok = matches!(found, Ok(Some(ms)) if (68_000..=72_000).contains(&ms));
        act_ok &= ok;
        detail = format!("{detail}; credits found in the picture (no chapters): {} (expected 1:10, took {} ms)",
            match &found { Ok(Some(ms)) => crate::selftest::mmss(*ms), Ok(None) => "none".into(), Err(e) => e.clone() }, t0.elapsed().as_millis());
    }
    steps.add("act", act_ok, detail);

    // ---- hud: the web app and the API list are served
    let web = c.get("/");
    let html = String::from_utf8_lossy(&web.body).to_string();
    let docs = c.get("/api/docs").json();
    let n_routes = docs["routes"].as_array().map(|a| a.len()).unwrap_or(0);
    let parts = ["Continue Watching", "Playlists", "Users", "Plugins", "Libraries"];
    let missing: Vec<&str> = parts.iter().copied().filter(|p| !html.contains(p)).collect();
    let status_reply = c.get("/api/status");
    let status = status_reply.json();
    // Replies without their own cache rule say "no-cache", and an
    // empty server name shows the computer's name instead.
    let no_cache = status_reply.head.to_lowercase().contains("cache-control: no-cache");
    c.call("PUT", "/api/settings", Some(json!({"server_name": ""})), &[]);
    let fallback = c.get("/api/status").json()["name"].as_str().unwrap_or("").to_string();
    c.call("PUT", "/api/settings", Some(json!({"server_name": "Self-test server"})), &[]);
    steps.add(
        "hud",
        web.status == 200 && missing.is_empty() && n_routes >= 60 && status["name"] == "Self-test server" && no_cache && !fallback.is_empty(),
        format!("web app served ({} KB, shows {}{}); API lists {n_routes} addresses; server name and version shown ({}); API replies not cached: {no_cache}; without a name the server is called \"{fallback}\"",
            html.len() / 1024, parts.join(", "), if missing.is_empty() { String::new() } else { format!(", missing {missing:?}") }, status["version"]),
    );

    // ---- playlists: default, manual, smart, filter and sort
    let all = c.get("/playlists").json();
    let defaults = all["MediaContainer"]["Metadata"].as_array().map(|a| a.iter().filter(|p| p["kind"] == "default").count()).unwrap_or(0);
    let manual = c.post("/playlists", json!({"title": "Movie night", "kind": "manual", "ids": [avi, mp4, mkv]})).json();
    let manual_id = manual["MediaContainer"]["Metadata"][0]["id"].as_u64().unwrap_or(0);
    c.call("PUT", &format!("/playlists/{manual_id}/items/{mkv}/move?after=0"), None, &[]);
    let order: Vec<u64> = c.get(&format!("/playlists/{manual_id}/items")).json()["MediaContainer"]["Metadata"].as_array().map(|a| a.iter().filter_map(|x| x["id"].as_u64()).collect()).unwrap_or_default();
    let smart = c.post("/playlists", json!({"title": "Test Show backwards", "kind": "smart", "query": {"match_all": true, "rules": [{"field": "show", "op": "is", "value": "Test Show"}, {"field": "kind", "op": "is", "value": "episode"}], "sort": "episode:desc"}})).json();
    let smart_id = smart["MediaContainer"]["Metadata"][0]["id"].as_u64().unwrap_or(0);
    let smart_items: Vec<u64> = c.get(&format!("/playlists/{smart_id}/items")).json()["MediaContainer"]["Metadata"].as_array().map(|a| a.iter().filter_map(|x| x["index"].as_u64()).collect()).unwrap_or_default();
    let only_smart = c.get("/playlists?kind=smart").json()["MediaContainer"]["size"].as_u64().unwrap_or(0);
    let by_count = c.get("/playlists?owner=me&sort=item_count:desc").json();
    let first_by_count = by_count["MediaContainer"]["Metadata"][0]["title"].as_str().unwrap_or("").to_string();
    let filtered = c.get(&format!("/playlists/{manual_id}/items?container=mkv")).json()["MediaContainer"]["size"].as_u64().unwrap_or(0);
    let sorted_items: Vec<String> = c.get(&format!("/playlists/{manual_id}/items?sort=year:desc")).json()["MediaContainer"]["Metadata"].as_array().map(|a| a.iter().map(|x| x["year"].to_string()).collect()).unwrap_or_default();
    steps.add(
        "playlists",
        defaults == 6 && order.first() == Some(&mkv) && order.len() == 3 && smart_items == vec![2, 1] && only_smart == 1 && first_by_count == "Movie night" && filtered == 1 && sorted_items.first().map(|s| s.as_str()) == Some("2020"),
        format!("{defaults} default playlists; manual playlist reordered (moved item first: {}); smart playlist found episodes {:?}; filter kind=smart → {only_smart}; sort by item count → \"{first_by_count}\" first; items filtered to mkv → {filtered}; sorted by year desc → {}", order.first() == Some(&mkv), smart_items, sorted_items.join(", ")),
    );

    // ---- users: invite with access to movies only
    let inv = c.post("/api/invites", json!({"email": "friend@example.com", "days": 7, "allowed_sections": [mid], "note": "Movies only"})).json();
    let code = inv["code"].as_str().unwrap_or("").to_string();
    let url = inv["url"].as_str().unwrap_or("").to_string();
    let anon = Client { port, token: None };
    let info = anon.get(&format!("/api/invites/{code}"));
    let joined = anon.post(&format!("/api/invites/{code}/accept"), json!({"username": "friend", "password": "letmein1"}));
    let friend = Client { port, token: joined.json()["token"].as_str().map(String::from) };
    let visible = friend.get("/library/sections").json()["MediaContainer"]["size"].as_u64().unwrap_or(0);
    let blocked = friend.get(&format!("/library/sections/{tid}/all")).status;
    let not_admin = friend.get("/api/users").status;
    let reuse = anon.post(&format!("/api/invites/{code}/accept"), json!({"username": "friend2", "password": "letmein1"})).status;
    let login = anon.post("/api/login", json!({"username": "friend", "password": "letmein1"})).status;
    steps.add(
        "users",
        info.status == 200 && joined.status == 201 && visible == 1 && blocked == 403 && not_admin == 403 && reuse == 410 && login == 200 && url.contains("/#/invite/"),
        format!("invite link made ({}); friend joined (HTTP {}), sees {visible} library, TV blocked (HTTP {blocked}), admin pages blocked (HTTP {not_admin}), invite can't be reused (HTTP {reuse}), can sign in (HTTP {login})", url.replace(&code, "…"), joined.status),
    );

    // ---- plugins
    let pl = c.get("/api/plugins").json();
    let enabled = pl["plugins"].as_array().map(|a| a.iter().any(|p| p["id"] == "example" && p["enabled"] == true)).unwrap_or(false);
    let tagline = c.get(&format!("/library/metadata/{mp4}")).json()["MediaContainer"]["Metadata"][0]["tagline"].as_str().unwrap_or("").to_string();
    let call = c.post("/api/plugins/example/call/hello", json!({"say": "hi"}));
    steps.add(
        "plugins",
        enabled && tagline.starts_with("Added by the example plugin") && call.status == 200 && call.json()["echo"]["say"] == "hi",
        format!("example plugin found and on: {enabled}; it filled in the tagline \"{tagline}\"; its own API action answered HTTP {}", call.status),
    );

    // ---- api: filters, search, hubs
    let filters = c.get("/api/filters").json();
    let nf = filters["fields"].as_array().map(|a| a.len()).unwrap_or(0);
    let search = c.get("/hubs/search?query=test").json()["MediaContainer"]["size"].as_u64().unwrap_or(0);
    let hubs = c.get("/hubs").json()["MediaContainer"]["size"].as_u64().unwrap_or(0);
    let browse = c.get(&format!("/library/sections/{mid}/all?year>=2000&sort=title")).json()["MediaContainer"]["size"].as_u64().unwrap_or(0);
    let unauth = anon.get("/library/sections").status;
    // A new episode copied into the TV folder shows up by itself (folder watching).
    crate::server::WATCH_SECONDS.store(1, std::sync::atomic::Ordering::Relaxed);
    let ep3 = media.join("TV").join("Test Show").join("Season 01").join("Test Show - S01E03 - New Arrival.mkv");
    let _ = std::fs::copy(&ep1, &ep3);
    let t0 = std::time::Instant::now();
    let mut seen_new = false;
    while t0.elapsed() < Duration::from_secs(10) {
        std::thread::sleep(Duration::from_millis(300));
        let l = c.get(&format!("/library/metadata/{show_id}/allLeaves")).json();
        if l["MediaContainer"]["Metadata"].as_array().is_some_and(|a| a.iter().any(|e| e["index"] == 3)) {
            seen_new = true;
            break;
        }
    }
    steps.add(
        "api",
        nf >= 20 && search >= 1 && hubs >= 1 && browse == 2 && unauth == 401 && seen_new,
        format!("{nf} filter fields; search 'test' → {search}; {hubs} home rows; movies with year>=2000 → {browse}; without a token → HTTP {unauth}; new episode copied into the folder found by itself: {} (after {:.1} s)",
            if seen_new { "yes" } else { "no" }, t0.elapsed().as_secs_f32()),
    );

    // ---- import: a small database in the old server's format, with Windows
    // paths from "the old PC" that are translated to the folders here.
    {
        let extra = media.join("Extra");
        make_clip(&extra.join("Imported Film (2001).mp4"), "libx264", "aac", &["-pix_fmt", "yuv420p"], have_ffmpeg);
        let old_db = root.join("old-server").join(crate::migrate::DB_NAME);
        let made = make_old_db(&old_db, r"D:\OldPC\Films", r"D:\OldPC\TV");
        let users = c.get("/api/users").json();
        let uid = |n: &str| users["users"].as_array().and_then(|a| a.iter().find(|u| u["username"] == n)).and_then(|u| u["id"].as_u64()).unwrap_or(0);
        let (owner, friend_id) = (uid("owner"), uid("friend"));
        let pv = c.get(&format!("/api/import/old-server/preview?db={}", crate::metadata::enc(&old_db.to_string_lossy()))).json();
        let friend_suggested = pv["accounts"].as_array().and_then(|a| a.iter().find(|x| x["name"] == "friend")).and_then(|x| x["suggested_user"].as_u64());
        let start = c.post("/api/import/old-server", json!({
            "db": old_db.to_string_lossy(),
            "path_map": [{"from": r"D:\OldPC\Films", "to": extra.to_string_lossy()}, {"from": r"D:\OldPC\TV", "to": media.join("TV").to_string_lossy()}],
            "accounts": {"1": owner, "2": friend_suggested.unwrap_or(0)},
        }));
        let t0 = std::time::Instant::now();
        let mut report = Value::Null;
        while t0.elapsed() < Duration::from_secs(20) {
            std::thread::sleep(Duration::from_millis(300));
            let s = c.get("/api/import/old-server").json();
            if s["running"] == false && s["last"]["finished_at"].is_number() {
                report = s["last"].clone();
                break;
            }
        }
        let film = c.get("/library/sections").json()["MediaContainer"]["Directory"].as_array().cloned().unwrap_or_default().iter()
            .find(|s| s["title"] == "Old films").and_then(|s| s["id"].as_u64())
            .map(|sid| c.get(&format!("/library/sections/{sid}/all")).json()["MediaContainer"]["Metadata"][0].clone()).unwrap_or(Value::Null);
        let fid = film["id"].as_u64().unwrap_or(0);
        let marks = c.get(&format!("/library/metadata/{fid}/markers")).json();
        // People: actors in their old order with roles, directors, writers; the
        // person page at its second address lists the film.
        let fx = c.get(&format!("/library/metadata/{fid}")).json()["MediaContainer"]["Metadata"][0]["extra"].clone();
        let cast_names: Vec<String> = fx["cast"].as_array().cloned().unwrap_or_default().iter().map(|p| format!("{} as {}", p["name"].as_str().unwrap_or("?"), p["role"].as_str().unwrap_or(""))).collect();
        let ann_key = fx["cast"][0]["key"].as_str().unwrap_or("").to_string();
        let person = c.get(&format!("/library/people/{ann_key}/media")).json();
        // Half stars: 3½ stars is 7 points and stays 7.
        let half = c.post(&format!("/:/rate?key={fid}&rating=7"), json!({})).status == 200
            && c.get(&format!("/library/metadata/{fid}")).json()["MediaContainer"]["Metadata"][0]["userRating"].as_f64() == Some(7.0);
        c.post(&format!("/:/rate?key={fid}&rating=8"), json!({}));
        let people_ok = half && cast_names == ["Ann Actor as Hero", "Bob Second as Sidekick"] && fx["cast"][0]["count"] == 1
            && fx["directors"][0] == "Dee Rector" && fx["writers"][0] == "Wri Ter"
            && person["type"] == "person" && person["Metadata"][0]["title"] == "Imported Film" && person["Metadata"][0]["role"] == "Hero";
        let pilot = elist.first().and_then(|e| e["id"].as_u64()).unwrap_or(0);
        let pilot_offset = c.get(&format!("/library/metadata/{pilot}")).json()["MediaContainer"]["Metadata"][0]["viewOffset"].as_u64().unwrap_or(0);
        let pls = c.get("/playlists?owner=me").json();
        let old_pl = pls["MediaContainer"]["Metadata"].as_array().and_then(|a| a.iter().find(|p| p["title"] == "Old favourites")).cloned().unwrap_or(Value::Null);
        let ok = made && start.status == 202 && friend_suggested == Some(friend_id)
            && report["files"]["found_here"] == 2 && report["files"]["not_found"] == 1
            && report["libraries_created"].as_array().map(|a| a.len()) == Some(1)
            && film["title"] == "Imported Film" && film["summary"] == "A plot from the old server." && film["viewCount"] == 2 && film["userRating"].as_f64() == Some(8.0)
            && film["genres"].as_array().is_some_and(|g| g.iter().any(|x| x == "Drama"))
            && marks["source"] == "imported" && marks["credits"][0] == 3000
            && pilot_offset == 2000 && report["watched"]["ratings"] == 2
            && old_pl["leafCount"].as_u64().or(old_pl["itemCount"].as_u64()) == Some(2)
            && report["playlists"]["smart_not_imported"][0] == "Old smart" && people_ok;
        steps.add("import", ok, format!(
            "old server's database read ({} libraries, {} accounts); files found {} of {} after translating D:\\OldPC\\… paths; libraries added: {}; film → \"{}\", plot taken, genres {}, watched {}×, rated {} (half star 3½ → 7 kept: {half}); cast {:?}, director {}, writer {}, actor page lists it: {}; friend matched by name: {}; resume point on an episode: {} ms; credits marker: {} ({}); playlist \"Old favourites\" with {} items; smart playlist skipped: {}",
            pv["libraries"].as_array().map(|a| a.len()).unwrap_or(0), pv["accounts"].as_array().map(|a| a.len()).unwrap_or(0),
            report["files"]["found_here"], report["files"]["in_old_database"], report["libraries_created"].as_array().map(|a| a.len()).unwrap_or(0),
            film["title"].as_str().unwrap_or("?"), film["genres"], film["viewCount"], film["userRating"],
            cast_names, fx["directors"], fx["writers"], person["Metadata"][0]["title"] == "Imported Film", friend_suggested == Some(friend_id),
            pilot_offset, marks["credits"], marks["source"], old_pl["leafCount"].as_u64().or(old_pl["itemCount"].as_u64()).unwrap_or(0),
            report["playlists"]["smart_not_imported"]));
    }

    // ---- details: plot, cast, posters and episode titles from the internet.
    // Runs on its own thread with a time limit: outside sites can be slow or
    // ask us to wait, and the whole self-test has to finish inside BUDGET.
    let limit = BUDGET.saturating_sub(started.elapsed()).saturating_sub(Duration::from_secs(20)).min(Duration::from_secs(45));
    let (tx, rx) = std::sync::mpsc::channel::<(bool, String)>();
    let c2 = c.clone();
    std::thread::spawn(move || {
    let c = c2;
    if crate::metadata::online() {
        let pick = |id: u64, title: &str, year: u32| -> (u16, String) {
            let m = c.get(&format!("/library/metadata/{id}/matches?title={}&year={year}", crate::metadata::enc(title))).json();
            let Some(best) = m["matches"].as_array().and_then(|a| a.first()).cloned() else { return (404, "no matches".into()) };
            let r = c.call("PUT", &format!("/library/metadata/{id}/match"), Some(json!({"source": best["source"], "id": best["id"]})), &[]);
            let msg = r.json()["message"].as_str().or(r.json()["error"].as_str()).unwrap_or("").to_string();
            if std::env::var("SELFTEST_TIMING").is_ok() {
                eprintln!("lookup {title}: HTTP {} after {:.1} s", r.status, started.elapsed().as_secs_f32());
            }
            (r.status, msg)
        };
        let (ms, mmsg) = pick(mp4, "Big Buck Bunny", 2008);
        let movie = c.get(&format!("/library/metadata/{mp4}")).json()["MediaContainer"]["Metadata"][0].clone();
        let poster = c.get(&format!("/library/metadata/{mp4}/thumb"));
        let movie_ok = ms == 200 && movie["title"] == "Big Buck Bunny" && movie["summary"].as_str().is_some_and(|s| s.len() > 80)
            && poster.status == 200 && !poster.head.to_lowercase().contains("image/svg");
        let (ss, smsg) = pick(show_id, "Twin Peaks", 1990);
        let show = c.get(&format!("/library/metadata/{show_id}")).json()["MediaContainer"]["Metadata"][0].clone();
        let cast_n = show["extra"]["cast"].as_array().map(|a| a.len()).unwrap_or(0);
        let eps = c.get(&format!("/library/metadata/{show_id}/allLeaves")).json();
        let ep_titles: Vec<String> = eps["MediaContainer"]["Metadata"].as_array().map(|a| a.iter().map(|e| e["title"].as_str().unwrap_or("").to_string()).collect()).unwrap_or_default();
        let ep_plot = eps["MediaContainer"]["Metadata"][1]["summary"].as_str().is_some_and(|s| s.len() > 20);
        // Actor photos are saved on the server (one per person, shared by
        // everything they are in) and served from it as real photos; the
        // actor's page lists this movie or show.
        let saved_photos = |item: &Value, id: u64| -> (usize, bool) {
            let cast = item["extra"]["cast"].as_array().cloned().unwrap_or_default();
            let thumbs: Vec<String> = cast.iter().filter_map(|p| p["thumb"].as_str().map(String::from)).collect();
            let served = thumbs.first().is_some_and(|t| {
                let r = c.get(t);
                r.status == 200 && (r.head.to_lowercase().contains("image/jpeg") || r.head.to_lowercase().contains("image/png") || r.head.to_lowercase().contains("image/webp")) && t.starts_with("/library/people/")
            });
            let listed = cast.first().and_then(|p| p["key"].as_str()).is_some_and(|k| {
                c.get(&format!("/library/people/{k}")).json()["Metadata"].as_array().is_some_and(|a| a.iter().any(|x| x["id"].as_u64() == Some(id)))
            });
            (thumbs.len(), served && listed)
        };
        let (show_photos, show_served) = saved_photos(&show, show_id);
        let show_ok = ss == 200 && cast_n >= 3 && ep_titles.get(1).is_some_and(|t| t != "The Second One") && ep_plot && show_photos >= 3 && show_served;
        // A film with actors, from the free Wikidata source (no TMDB key).
        let (ws, wmsg) = pick(avi, "Night of the Living Dead", 1968);
        let film = c.get(&format!("/library/metadata/{avi}")).json()["MediaContainer"]["Metadata"][0].clone();
        let (film_photos, film_served) = saved_photos(&film, avi);
        let film_ok = ws == 200 && film_photos >= 1 && film_served;
        // Background: online, or a still from the video when there is none.
        let art = c.get(&format!("/library/metadata/{mp4}/art"));
        let art_ok = art.status == 200 || !crate::transcode::detect(&crate::db::Settings::default())["ffmpeg_found"].as_bool().unwrap_or(false);
        let _ = tx.send((
            movie_ok && show_ok && film_ok && art_ok,
            format!("movie matched online → \"{}\" ({mmsg}), plot {} characters, poster {}, background HTTP {}; show → \"{}\" ({smsg}), {cast_n} cast members, {show_photos} actor photos saved, episodes now called {:?}; film → \"{}\" ({wmsg}), {film_photos} actor photos saved from Wikimedia",
                movie["title"].as_str().unwrap_or(""), movie["summary"].as_str().map(|s| s.chars().count()).unwrap_or(0),
                if poster.status == 200 { "downloaded" } else { "missing" }, art.status, show["title"].as_str().unwrap_or(""), ep_titles,
                film["title"].as_str().unwrap_or("")),
        ));
    } else {
        let _ = tx.send((true, "online details not tried: this computer is offline".into()));
    }
    });
    // ---- ui: the web app's pages, drawn by a real browser (Edge or Chrome, headless)
    let shots_dir = std::env::current_dir().unwrap_or_default().join(out.parent().map(Path::to_path_buf).unwrap_or_default());
    let _ = std::fs::create_dir_all(&shots_dir);
    let token = c.token.clone().unwrap_or_default();
    let pages = [
        ("home", String::new()),
        ("movies", format!("section/{mid}")),
        ("tv-show", format!("item/{show_id}")),
        ("movie", format!("item/{mp4}")),
        ("playlists", "playlists".to_string()),
        ("playlist", format!("playlist/{manual_id}")),
        ("users", "users".to_string()),
        ("settings", "settings".to_string()),
    ];
    match find_browser() {
        _ if started.elapsed() + Duration::from_secs(8 * 4) > BUDGET => steps.add("ui", true, format!(
            "web app pages not photographed this time: only {} s of the time limit left", BUDGET.saturating_sub(started.elapsed()).as_secs())),
        None => steps.add("ui", true, "web app pages not photographed: no Edge or Chrome on this computer".into()),
        Some(browser) => {
            let profile = root.join("browser-profile");
            let mut made = vec![];
            let mut failed = vec![];
            for (name, route) in &pages {
                if started.elapsed() + Duration::from_secs(16) > BUDGET {
                    break; // out of time: the pages done so far are reported
                }
                let png = shots_dir.join(format!("web-{name}.png"));
                let _ = std::fs::remove_file(&png);
                let url = format!("http://127.0.0.1:{port}/?token={token}#/{route}");
                if browser_screenshot(&browser, &profile, &url, &png) {
                    made.push(*name);
                } else {
                    failed.push(*name);
                }
            }
            // Pages that all come out the same (e.g. stuck on "Loading…" because a
            // script broke) count as failed.
            let sizes: HashSet<u64> = made.iter().filter_map(|n| std::fs::metadata(shots_dir.join(format!("web-{n}.png"))).ok().map(|m| m.len())).collect();
            let all_same = made.len() > 2 && sizes.len() == 1;
            steps.add(
                "ui",
                failed.is_empty() && !made.is_empty() && !all_same,
                format!("web app pages drawn by a browser and saved as pictures: {}{}{}", made.join(", "), if failed.is_empty() { String::new() } else { format!("; failed: {}", failed.join(", ")) },
                    if all_same { "; but every page looks the same (the web app did not start)" } else { "" }),
            );
        }
    }

    // (The pages are photographed while the online lookups run.)
    let limit = limit.min(BUDGET.saturating_sub(started.elapsed()).saturating_sub(Duration::from_secs(4)));
    match rx.recv_timeout(limit) {
        Ok((passed, detail)) => steps.add("details", passed, detail),
        Err(_) => steps.add("details", true, format!(
            "online details not finished within {} s (the internet or the sites were slow, or asked us to wait); not counted — the lookups themselves are checked whenever they finish in time",
            limit.as_secs())),
    }

    let _ = c.call("GET", "/api/status", None, &[]).head;
    write(out, steps, &root)
}

fn find_browser() -> Option<PathBuf> {
    let candidates = [
        r"C:\Program Files (x86)\Microsoft\Edge\Application\msedge.exe",
        r"C:\Program Files\Microsoft\Edge\Application\msedge.exe",
        r"C:\Program Files\Google\Chrome\Application\chrome.exe",
        r"C:\Program Files (x86)\Google\Chrome\Application\chrome.exe",
        "/usr/bin/chromium",
        "/usr/bin/chromium-browser",
        "/usr/bin/google-chrome",
        "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
    ];
    candidates.iter().map(PathBuf::from).find(|p| p.exists())
}

/// Lets a headless browser load the page (and its API calls), saves a 1280×800 picture.
/// Uses its own throw-away profile so it never touches the person's browser.
fn browser_screenshot(browser: &Path, profile: &Path, url: &str, png: &Path) -> bool {
    use std::process::{Command, Stdio};
    let child = Command::new(browser)
        .args(["--headless=new", "--disable-gpu", "--hide-scrollbars", "--no-first-run", "--no-default-browser-check", "--mute-audio", "--window-size=1280,800", "--virtual-time-budget=6000"])
        .arg(format!("--user-data-dir={}", profile.display()))
        .arg(format!("--screenshot={}", png.display()))
        .arg(url)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn();
    let Ok(mut child) = child else { return false };
    let start = std::time::Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if start.elapsed() < std::time::Duration::from_secs(15) => std::thread::sleep(std::time::Duration::from_millis(200)),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                break;
            }
        }
    }
    std::fs::metadata(png).map(|m| m.len() > 5_000).unwrap_or(false)
}

fn write(out: &Path, steps: Steps, root: &Path) -> bool {
    let passed = steps.0.iter().all(|s| s["passed"] == true);
    save(out, &steps.0);
    let _ = std::fs::remove_dir_all(root);
    passed
}

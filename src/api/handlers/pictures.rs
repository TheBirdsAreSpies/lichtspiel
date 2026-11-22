//! Posters, backgrounds, cast photos, video frames and pictures chosen by hand.

use super::*;

/// The wide background picture from the online details (the show's for seasons and episodes).
pub fn art(app: &Arc<App>, user: &User, iid: &str) -> Resp {
    let path = {
        let db = app.db.lock().unwrap();
        let Some(it) = iid.parse::<u64>().ok().and_then(|i| db.items.get(&i)) else { return fail(404, "Not found") };
        if !db.can_see_item(user, it) {
            return fail(403, "No access");
        }
        let top_id = App::top_level(&db, it.id).unwrap_or(it.id);
        let top = db.items.get(&top_id);
        let online = it.extra.get("art").or_else(|| top.and_then(|t| t.extra.get("art"))).and_then(|v| v.as_str()).map(String::from);
        // No background online: a picture from the video itself (the movie's file,
        // or a show's first episode), a third of the way in, past opening titles.
        let video = top.and_then(|t| t.file.as_ref().map(|f| (f.path.clone(), f.duration_ms)))
            .or_else(|| db.leaves(top_id).first().and_then(|l| db.items.get(l)).and_then(|l| l.file.as_ref().map(|f| (f.path.clone(), l.duration_ms))));
        (online, video, top_id)
    };
    let (online, video, top_id) = path;
    let video = video.filter(|(p, _)| app.may_send(Path::new(p), false));
    // (Only real photos: a drawing saved by an older version is skipped.)
    if let Some((p, mime)) = online.filter(|p| app.may_send(Path::new(p), true)).and_then(|p| crate::library::metadata::file_image_type(Path::new(&p)).map(|m| (p, m))) {
        return with_cache(serve_file(Path::new(&p), mime, None, false));
    }
    let cache = app.data_dir.join("cache").join("art").join(format!("{top_id}.jpg"));
    if !std::fs::metadata(&cache).is_ok_and(|m| m.len() > 0) {
        let Some((src, dur)) = video else { return fail(404, "No background picture") };
        let _ = std::fs::create_dir_all(cache.parent().unwrap_or(Path::new(".")));
        let at = if dur > 0 { dur as f64 / 1000.0 / 3.0 } else { 60.0 };
        if !make_picture(&app.settings().ffmpeg, &cache, |tmp| transcode::art_args(&src, at, tmp)) {
            return fail(404, "No background picture");
        }
    }
    with_cache(serve_file(&cache, "image/jpeg", None, false))
}

/// An actor photo saved with the online details (metadata/{id}/cast/{n}.jpg).
pub fn cast_photo(app: &Arc<App>, user: &User, iid: &str, n: &str) -> Resp {
    let top = {
        let db = app.db.lock().unwrap();
        let Some(it) = iid.parse::<u64>().ok().and_then(|i| db.items.get(&i)) else { return fail(404, "Not found") };
        if !db.can_see_item(user, it) {
            return fail(403, "No access");
        }
        App::top_level(&db, it.id).unwrap_or(it.id)
    };
    let Ok(n) = n.parse::<u32>() else { return fail(404, "Not found") };
    let p = app.metadata_dir(top).join("cast").join(format!("{n}.jpg"));
    let Some(mime) = crate::library::metadata::file_image_type(&p) else { return fail(404, "No photo") };
    with_cache(serve_file(&p, mime, None, false))
}

/// Copies being made right now (at most two at a time, so a page of fresh posters does not start a
/// hundred ffmpeg processes at once and slow the whole server down).
static RESIZING: std::sync::Mutex<std::collections::BTreeSet<PathBuf>> = std::sync::Mutex::new(std::collections::BTreeSet::new());

/// A smaller copy of a poster (`width` px wide, never enlarged) from cache/thumbs. When it is not there
/// yet, ffmpeg makes it in the background and None is returned, so this request gets the original; the
/// next one finds the small copy.
fn scaled_poster(app: &Arc<App>, id: u64, src: &Path, width: u32) -> Option<PathBuf> {
    let out = app.data_dir.join("cache").join("thumbs").join(format!("{id}_w{width}.jpg"));
    let modified = |p: &Path| std::fs::metadata(p).and_then(|m| m.modified()).ok();
    if std::fs::metadata(&out).is_ok_and(|m| m.len() > 0) && modified(&out) >= modified(src) { return Some(out); }
    {
        let mut busy = RESIZING.lock().unwrap();
        if busy.len() >= 2 || !busy.insert(out.clone()) { return None; }
    }
    let (ffmpeg, src, key) = (app.settings().ffmpeg, src.to_path_buf(), out.clone());
    std::thread::spawn(move || {
        let _ = std::fs::create_dir_all(out.parent().unwrap_or(Path::new(".")));
        // Written under a name of its own and then renamed, so nobody reads a half-written picture.
        let tmp = out.with_file_name(format!("{id}_w{width}.tmp.jpg"));
        let ran = Command::new(ffmpeg)
            .args(["-hide_banner", "-loglevel", "error", "-nostdin", "-y", "-i"]).arg(&src)
            .args(["-frames:v", "1", "-vf", &format!("scale='min({width},iw)':-2"), "-q:v", "4"]).arg(&tmp)
            .stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null())
            .status().is_ok_and(|s| s.success());
        if !(ran && std::fs::rename(&tmp, &out).is_ok()) { let _ = std::fs::remove_file(&tmp); }
        RESIZING.lock().unwrap().remove(&key);
    });
    None
}

pub fn thumb(app: &Arc<App>, user: &User, iid: &str, width: Option<u32>) -> Resp {
    let (poster, file, duration, title, kind, id) = {
        let db = app.db.lock().unwrap();
        let Some(it) = iid.parse::<u64>().ok().and_then(|i| db.items.get(&i)) else { return fail(404, "Not found") };
        if !db.can_see_item(user, it) {
            return fail(403, "No access");
        }
        let mut poster = it.poster.clone();
        if poster.is_none() && it.kind == ItemKind::Season {
            poster = it.parent_id.and_then(|p| db.items.get(&p)).and_then(|s| s.poster.clone());
        }
        // A song without its own picture shows its album's.
        if poster.is_none() && it.kind == ItemKind::Track {
            poster = it.parent_id.and_then(|p| db.items.get(&p)).and_then(|a| a.poster.clone());
        }
        (poster, it.file.as_ref().map(|f| f.path.clone()), it.file.as_ref().map_or(it.duration_ms, |f| f.duration_ms), it.title.clone(), it.kind, it.id)
    };
    // The type is read from the file itself (a .jpg may really be a PNG);
    // something that is not a photo at all is skipped.
    if let Some((p, mime)) = poster.filter(|p| app.may_send(Path::new(p), true)).and_then(|p| crate::library::metadata::file_image_type(Path::new(&p)).map(|m| (p, m))) {
        if let Some(small) = width.filter(|w| *w >= 64).and_then(|w| scaled_poster(app, id, Path::new(&p), w)) {
            return with_cache(serve_file(&small, "image/jpeg", None, false));
        }
        // A small copy was asked for but is still being made: the original now, kept only briefly,
        // so the browser asks again soon and then gets the small one.
        if width.is_some_and(|w| w >= 64) {
            let mut r = serve_file(Path::new(&p), mime, None, false);
            r.add_header(header("Cache-Control", "private, max-age=60"));
            return r;
        }
        return with_cache(serve_file(Path::new(&p), mime, None, false));
    }
    let cache: PathBuf = app.data_dir.join("cache").join("thumbs").join(format!("{id}.jpg"));
    if std::fs::metadata(&cache).map(|m| m.len() > 0).unwrap_or(false) {
        return with_cache(serve_file(&cache, "image/jpeg", None, false));
    }
    // The first episode of a show or season is looked for only now: searching the library for it on every
    // request (also for the many that have a poster) held up every other request.
    let source = match file {
        Some(f) => Some((f, duration)),
        None if !kind.playable() => {
            let db = app.db.lock().unwrap();
            db.leaves(id).first().and_then(|leaf| db.items.get(leaf)).and_then(|l| l.file.as_ref().map(|f| (f.path.clone(), l.duration_ms)))
        }
        None => None,
    };
    if let Some((src, dur)) = source.filter(|(p, _)| app.may_send(Path::new(p), false)) {
        let at = if dur > 0 { (dur as f64 / 1000.0 * 0.1).min(300.0) } else { 5.0 };
        let ffmpeg = app.settings().ffmpeg;
        // Music: the cover art inside the file; a still of the "picture" would be nothing.
        let music = matches!(kind, ItemKind::Artist | ItemKind::Album | ItemKind::Track);
        let ran = make_picture(&ffmpeg, &cache, |tmp| if music { transcode::cover_args(&src, tmp) } else { transcode::thumb_args(&src, at, tmp) });
        if ran {
            return with_cache(serve_file(&cache, "image/jpeg", None, false));
        }
    }
    bytes(200, "image/svg+xml", placeholder_svg(&title, kind.as_str(), id).into_bytes())
}

/// Posters, backgrounds and seek-bar pictures made with ffmpeg at the same time: a page of new titles
/// waits its turn instead of starting a hundred ffmpeg processes at once.
pub const PICTURES_AT_ONCE: usize = 3;
pub(super) static PICTURES: Slots = (std::sync::Mutex::new(0), std::sync::Condvar::new());
/// How many pictures are being made, and the signal that one is done.
pub(super) type Slots = (std::sync::Mutex<usize>, std::sync::Condvar);

/// Runs ffmpeg with `args(tmp)` to make the picture `out`. It is written under a name of its own and
/// renamed when done, so nobody reads a half-written file. False when ffmpeg failed or no turn came within 20 s.
pub fn make_picture(ffmpeg: &str, out: &Path, args: impl FnOnce(&str) -> Vec<String>) -> bool {
    make_picture_in(&PICTURES, ffmpeg, out, args)
}

/// make_picture() with turns from `slots` (a test has its own, so other tests making pictures at the same time do not take its turns).
fn make_picture_in(slots: &Slots, ffmpeg: &str, out: &Path, args: impl FnOnce(&str) -> Vec<String>) -> bool {
    let (count, freed) = slots;
    {
        let n = count.lock().unwrap_or_else(|e| e.into_inner());
        let (mut n, waited) = freed.wait_timeout_while(n, Duration::from_secs(20), |n| *n >= PICTURES_AT_ONCE).unwrap_or_else(|e| e.into_inner());
        if waited.timed_out() { return false; }
        *n += 1;
    }
    let tmp = out.with_extension(format!("{}.tmp.jpg", db::random_hex(4)));
    let ran = Command::new(ffmpeg).args(args(&tmp.to_string_lossy())).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).status().is_ok_and(|s| s.success());
    *count.lock().unwrap_or_else(|e| e.into_inner()) -= 1;
    freed.notify_one();
    let done = ran && std::fs::metadata(&tmp).is_ok_and(|m| m.len() > 0) && std::fs::rename(&tmp, out).is_ok();
    if !done { let _ = std::fs::remove_file(&tmp); }
    done
}

/// A small still at a moment of a movie or episode, for the player's seek bar
/// preview: ?t= seconds into the whole video (a split movie counts as one).
/// Rounded to 10 seconds and kept in cache/frames.
/// Takes one small picture at `at` seconds into `src` and saves it as `out` (already there = done).
pub fn make_frame(ffmpeg: &str, src: &str, at: f64, out: &std::path::Path) -> bool {
    if std::fs::metadata(out).map(|m| m.len() > 0).unwrap_or(false) { return true; }
    make_picture(ffmpeg, out, |tmp| vec![
        "-hide_banner".into(), "-loglevel".into(), "error".into(), "-nostdin".into(), "-y".into(),
        "-ss".into(), format!("{at:.2}"), "-i".into(), src.to_string(), "-frames:v".into(), "1".into(),
        "-vf".into(), "scale=240:-2".into(), "-q:v".into(), "6".into(), tmp.to_string(),
    ])
}

/// The second whose picture stands for a chapter: a little after its start (chapters often begin black).
pub fn chapter_picture_at(start_ms: u64, end_ms: u64) -> u64 {
    (start_ms as f64 / 1000.0 + ((end_ms as f64 - start_ms as f64) / 1000.0).min(6.0) / 2.0).floor() as u64
}

pub fn frame(app: &Arc<App>, ctx: &Ctx, user: &User, iid: &str) -> Resp {
    let want = ctx.param("t").and_then(|t| t.parse::<f64>().ok()).unwrap_or(0.0).max(0.0);
    // ?exact=1 (chapter pictures) keeps the second; the seek bar's pictures are rounded to 10 s.
    let step = if ctx.param("exact") == Some("1") { want.floor() as u64 } else { (want / 10.0).floor() as u64 * 10 };
    let (it, _) = match playable_file(app, user, iid, 0) { Ok(x) => x, Err(r) => return r };
    // Which file the moment is in, and the time inside that file.
    let mut at = step as f64;
    let mut src = None;
    for f in it.all_files() {
        let d = f.duration_ms as f64 / 1000.0;
        if d <= 0.0 || at < d { src = Some(f.path.clone()); break; }
        at -= d;
    }
    let Some(src) = src.or_else(|| it.last_file().map(|f| f.path.clone())) else { return fail(404, "No video file") };
    if !app.may_send(Path::new(&src), false) { return fail(403, "This file is outside the library folders"); }
    let dir = app.data_dir.join("cache").join("frames");
    let _ = std::fs::create_dir_all(&dir);
    let out = dir.join(format!("{}-{step}.jpg", it.id));
    if !make_frame(&app.settings().ffmpeg, &src, at, &out) { return fail(404, "No picture at that moment"); }
    let mut r = serve_file(&out, "image/jpeg", None, false);
    r.add_header(header("Cache-Control", "private, max-age=86400"));
    r
}

/// Pictures may be kept for a day: a changed poster or background has a new address (?v= the item's
/// change time), so a kept copy is never an outdated one.
pub fn with_cache(mut r: Resp) -> Resp {
    r.add_header(header("Cache-Control", "private, max-age=86400"));
    r
}



pub fn b64_decode(s: &str) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(s.len() * 3 / 4);
    let (mut acc, mut bits) = (0u32, 0);
    for c in s.bytes().filter(|c| !c.is_ascii_whitespace() && *c != b'=') {
        let v = match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' | b'-' => 62,
            b'/' | b'_' => 63,
            _ => return None,
        };
        acc = (acc << 6) | v as u32;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
            acc &= (1 << bits) - 1;
        }
    }
    Some(out)
}

pub const IMAGE_SLOTS: [&str; 4] = ["poster", "art", "logo", "square"];

/// Saves a picture for a poster, background, logo or square poster. Body: {data: base64} or {url}.
/// Pictures set by hand are locked: refreshing the online details keeps them.
pub fn set_image(app: &Arc<App>, iid: &str, slot: &str, body: &Value) -> Resp {
    let Some(id) = iid.parse::<u64>().ok() else { return fail(404, "Not found") };
    if !IMAGE_SLOTS.contains(&slot) {
        return fail(404, "No such picture");
    }
    let bytes = if let Some(d) = body["data"].as_str() {
        let d = d.split_once(',').map_or(d, |(_, rest)| rest);
        match b64_decode(d) {
            Some(b) => b,
            None => return fail(400, "The picture data is not valid"),
        }
    } else if let Some(u) = body["url"].as_str().filter(|u| u.starts_with("http://") || u.starts_with("https://")) {
        let agent = ureq::AgentBuilder::new().timeout(Duration::from_secs(20)).resolver(crate::server::public_only).build();
        let mut buf = Vec::new();
        match agent.get(u).call() {
            Ok(r) => { let _ = r.into_reader().take(8 * 1024 * 1024).read_to_end(&mut buf); }
            Err(e) => return fail(502, &format!("The picture could not be downloaded: {e}")),
        }
        buf
    } else {
        return fail(400, "Send the picture as {data} (base64) or {url}");
    };
    if crate::library::metadata::image_type(&bytes).is_none() {
        return fail(415, "That is not a JPEG, PNG, WebP or GIF picture");
    }
    let dir = app.data_dir.join("custom").join(id.to_string());
    let _ = std::fs::create_dir_all(&dir);
    let path = dir.join(slot);
    if let Err(e) = std::fs::write(&path, &bytes) {
        return fail(500, &format!("The picture could not be saved: {e}"));
    }
    {
        let mut db = app.db.lock().unwrap();
        let Some(it) = db.items.get_mut(&id) else { return fail(404, "Not found") };
        let p = path.to_string_lossy().to_string();
        if slot == "poster" { it.poster = Some(p); } else { it.extra.insert(slot.into(), json!(p)); }
        if !it.locked.iter().any(|l| l == slot) { it.locked.push(slot.into()); }
        it.updated_at = now();
    }
    drop_picture_caches(app, id);
    app.save();
    ok(json!({"ok": true}))
}

/// Takes the hand-chosen picture away again (a refresh brings the online one back).
pub fn clear_image(app: &Arc<App>, iid: &str, slot: &str) -> Resp {
    let Some(id) = iid.parse::<u64>().ok() else { return fail(404, "Not found") };
    if !IMAGE_SLOTS.contains(&slot) {
        return fail(404, "No such picture");
    }
    {
        let mut db = app.db.lock().unwrap();
        let Some(it) = db.items.get_mut(&id) else { return fail(404, "Not found") };
        if slot == "poster" { it.poster = None; } else { it.extra.remove(slot); }
        it.locked.retain(|l| l != slot);
        it.updated_at = now();
    }
    let _ = std::fs::remove_file(app.data_dir.join("custom").join(id.to_string()).join(slot));
    drop_picture_caches(app, id);
    app.save();
    ok(json!({"ok": true}))
}

fn drop_picture_caches(app: &Arc<App>, id: u64) {
    let _ = std::fs::remove_file(app.data_dir.join("cache").join("thumbs").join(format!("{id}.jpg")));
    let _ = std::fs::remove_file(app.data_dir.join("cache").join("art").join(format!("{id}.jpg")));
}

/// The logo or square poster chosen for an item.
pub fn custom_image(app: &Arc<App>, user: &User, iid: &str, slot: &str) -> Resp {
    let path = {
        let db = app.db.lock().unwrap();
        let Some(it) = iid.parse::<u64>().ok().and_then(|i| db.items.get(&i)) else { return fail(404, "Not found") };
        if !db.can_see_item(user, it) {
            return fail(403, "No access");
        }
        it.extra.get(slot).and_then(|v| v.as_str()).map(String::from)
    };
    let Some(p) = path.filter(|p| app.may_send(Path::new(p), true)) else { return fail(404, "No picture") };
    let Some(mime) = crate::library::metadata::file_image_type(Path::new(&p)) else { return fail(404, "No picture") };
    with_cache(serve_file(Path::new(&p), mime, None, false))
}



#[cfg(all(test, unix))]
mod picture_tests {
    use super::*;

    /// A stand-in "ffmpeg" (a shell script) in a fresh folder; `$out` is its last argument (the picture).
    /// The script is written by a `sh` of its own, not by this process: a process another test starts
    /// at that moment would take along this process's open script and running it would fail with
    /// "Text file busy" (Linux).
    fn fake_ffmpeg(name: &str, script: &str) -> (PathBuf, String) {
        use std::io::Write;
        let root = std::env::temp_dir().join(format!("lichtspiel-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let ff = root.join("ffmpeg.sh");
        let mut sh = Command::new("sh").args(["-c", "cat > \"$0\" && chmod 755 \"$0\""]).arg(&ff).stdin(Stdio::piped()).spawn().unwrap();
        sh.stdin.take().unwrap().write_all(format!("#!/bin/sh\nfor out; do :; done\n{script}\n").as_bytes()).unwrap();
        assert!(sh.wait().unwrap().success());
        (root, ff.to_string_lossy().to_string())
    }

    #[test]
    fn a_picture_is_never_seen_half_written() {
        let (root, ff) = fake_ffmpeg("half-picture", "printf half > \"$out\"; sleep 0.5; printf rest >> \"$out\"");
        let out = root.join("1-0.jpg");
        let (ff2, out2) = (ff.clone(), out.clone());
        let maker = std::thread::spawn(move || make_frame(&ff2, "x.mkv", 1.0, &out2));
        let start = Instant::now();
        while start.elapsed() < Duration::from_millis(400) {
            if let Ok(b) = std::fs::read(&out) {
                assert_ne!(b, b"half", "another request could read the picture while ffmpeg was still writing it");
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(maker.join().unwrap());
        assert_eq!(std::fs::read(&out).unwrap(), b"halfrest");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn pictures_are_made_a_few_at_a_time() {
        let (root, ff) = fake_ffmpeg("many-pictures", "d=$(dirname \"$0\")/running; mkdir -p \"$d\"; touch \"$d/$$\"; ls \"$d\" | wc -l >> \"$d.log\"; sleep 0.3; rm \"$d/$$\"; printf x > \"$out\"");
        // Turns of their own: other tests make pictures at the same time (with a real ffmpeg, slow on a busy machine).
        let slots: &'static Slots = Box::leak(Box::new((std::sync::Mutex::new(0), std::sync::Condvar::new())));
        let makers: Vec<_> = (0..9).map(|n| {
            let (ff, out) = (ff.clone(), root.join(format!("1-{n}.jpg")));
            std::thread::spawn(move || make_picture_in(slots, &ff, &out, |tmp| vec![format!("-ss {n}"), tmp.to_string()]))
        }).collect();
        for m in makers { assert!(m.join().unwrap()); }
        let most = std::fs::read_to_string(root.join("running.log")).unwrap().lines().filter_map(|l| l.trim().parse::<usize>().ok()).max().unwrap();
        assert!(most <= PICTURES_AT_ONCE, "{most} ffmpeg processes ran at once");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn no_picture_is_taken_from_a_video_outside_the_libraries() {
        let (root, ff) = fake_ffmpeg("outside-picture", "touch \"$(dirname \"$0\")/ran\"; printf x > \"$out\"");
        let app = App::open(&root.join("data"));
        let user: User = serde_json::from_value(json!({"id": 1, "username": "o", "admin": true, "password_hash": "", "salt": "", "created_at": 0})).unwrap();
        {
            let mut db = app.db.lock().unwrap();
            db.settings.ffmpeg = ff;
            // The movie's library folder is elsewhere (its file is not in any library).
            db.sections.push(serde_json::from_value(json!({"id": 1, "name": "Filme", "kind": "movie", "paths": [root.join("movies").to_string_lossy()], "created_at": 0})).unwrap());
            let mut i: Item = serde_json::from_value(json!({"id": 1, "section_id": 1, "kind": "movie", "title": "x", "sort_title": "x", "added_at": 0, "updated_at": 0})).unwrap();
            i.file = Some(db::MediaFile { path: "/etc/hosts".into(), duration_ms: 1000, ..Default::default() });
            db.items.insert(1, i);
        }
        assert_eq!(art(&app, &user, "1").status_code().0, 404);
        assert!(!root.join("ran").exists(), "ffmpeg read a file outside the libraries for a background");
        let _ = thumb(&app, &user, "1", None);
        assert!(!root.join("ran").exists(), "ffmpeg read a file outside the libraries for a poster");
        let _ = std::fs::remove_dir_all(&root);
    }
}

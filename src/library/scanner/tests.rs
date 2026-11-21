//! Tests for the scanner.

use super::*;

#[test]
fn trailing_article_moves_to_the_front() {
    assert_eq!(parse_title_year("Affe im Menschen, Der").0, "Der Affe im Menschen");
    assert_eq!(parse_title_year("Affe im Menschen, Der (1990)"), ("Der Affe im Menschen".to_string(), Some(1990)));
    assert_eq!(parse_title_year("Simpsons, The").0, "The Simpsons");
    assert_eq!(parse_title_year("Hallo, Welt (2001)").0, "Hallo, Welt");
}

#[test]
fn subtitle_kinds_are_told_apart() {
    let j = |t: &str| serde_json::from_str::<Value>(t).unwrap();
    assert_eq!(subtitle_flags(&j(r#"{"codec_type":"subtitle","disposition":{"forced":1}}"#)), (true, false));
    assert_eq!(subtitle_flags(&j(r#"{"codec_type":"subtitle","disposition":{"hearing_impaired":1}}"#)), (false, true));
    assert_eq!(subtitle_flags(&j(r#"{"codec_type":"subtitle","tags":{"title":"English SDH"}}"#)), (false, true));
    assert_eq!(subtitle_flags(&j(r#"{"codec_type":"subtitle","tags":{"title":"Deutsch (Erzwungen)"}}"#)), (true, false));
    assert_eq!(subtitle_flags(&j(r#"{"codec_type":"subtitle","tags":{"title":"Descriptive"}}"#)), (false, false));
    assert_eq!(subtitle_flags(&j(r#"{"codec_type":"audio","disposition":{"forced":1}}"#)), (false, false));
}

#[test]
fn chapters_are_read_with_titles() {
    let v: Value = serde_json::from_str(r#"{"chapters":[{"start_time":"0.000000","end_time":"60.5","tags":{"title":"Intro"}},{"start_time":"60.5","end_time":"120","tags":{}}]}"#).unwrap();
    assert_eq!(parse_chapters(&v), vec![(0, 60500, "Intro".to_string()), (60500, 120000, "Chapter 2".to_string())]);
}

#[test]
fn chapters_of_a_real_file() {
    let dir = std::env::temp_dir().join("lichtspiel-chapter-test");
    let _ = std::fs::create_dir_all(&dir);
    let meta = dir.join("m.txt");
    std::fs::write(&meta, ";FFMETADATA1
[CHAPTER]
TIMEBASE=1/1000
START=0
END=3000
title=One
[CHAPTER]
TIMEBASE=1/1000
START=3000
END=6000
title=Two
").unwrap();
    let f = dir.join("c.mkv");
    let ok = Command::new("ffmpeg").args(["-nostdin", "-loglevel", "error", "-y", "-f", "lavfi", "-i", "testsrc=size=160x90:rate=10:duration=6", "-i"]).arg(&meta)
        .args(["-map_metadata", "1", "-c:v", "libx264", "-preset", "ultrafast"]).arg(&f).status().map(|s| s.success()).unwrap_or(false);
    if !ok { return; } // no ffmpeg here
    let ch = read_chapters("ffprobe", &f.to_string_lossy()).unwrap();
    assert_eq!(ch.iter().map(|c| (c.0, c.2.as_str())).collect::<Vec<_>>(), vec![(0, "One"), (3000, "Two")]);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn ignore_patterns() {
    assert!(glob_match("*.sample.mkv", "film.sample.mkv"));
    assert!(!glob_match("*.mkv", "a/b.mkv"));
    assert!(glob_match("extras/**", "extras/2019/a.mkv"));
    assert!(glob_match("**/b.mkv", "a/x/b.mkv"));
    assert!(glob_match("c?1", "cd1"));
}

#[test]
fn scan_follows_plex_naming() {
    let root = std::env::temp_dir().join(format!("lichtspiel-scan-test-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let files = [
        "Avatar (2009)/Avatar (2009).mkv",
        "Avatar (2009)/Avatar (2009)-trailer.mkv",
        "Avatar (2009)/Featurettes/Making of.mkv",
        "The Dark Knight (2008)/The Dark Knight (2008) - pt2.mkv",
        "The Dark Knight (2008)/The Dark Knight (2008) - pt1.mkv",
        "Shorts/Some Short (2010).mkv",
        "Skip me/Hidden (2001).mkv",
        "Avatar (2009)/junk.sample.mkv.part.mkv",
        "Lonely/Trailers/Teaser (2015).mkv",
    ];
    for f in files {
        let p = root.join(f);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(&p, b"x").unwrap();
    }
    std::fs::write(root.join(IGNORE_FILE), "# test\nSkip me/\n*.part.mkv\n").unwrap();
    let mut db: Db = serde_json::from_value(serde_json::json!({"version": 1, "machine_id": "x", "next_id": 1, "settings": crate::data::db::Settings::default(),
        "sections": [{"id": 1, "name": "Filme", "kind": "movie", "paths": [root.to_string_lossy()], "created_at": 0}],
        "items": {}, "users": [], "invites": [], "tokens": [], "playlists": [], "views": {}, "plugins": {}})).unwrap();
    let section = db.sections[0].clone();
    let scan = |db: &mut Db| { scan_section(&section, "no-such-ffprobe", |f| f(db)); };
    scan(&mut db);
    let titles = |db: &Db| {
        let mut v: Vec<(String, usize, Option<String>)> = db.items.values().map(|i| {
            let parent = i.parent_id.and_then(|p| db.items.get(&p)).map(|p| p.title.clone());
            (format!("{}:{}", if i.is_extra() { i.extra["extra_type"].as_str().unwrap() } else { i.kind.as_str() }, i.title), i.parts.len(), parent)
        }).collect();
        v.sort();
        v
    };
    let want = vec![
        ("featurette:Making of".to_string(), 0, Some("Avatar".to_string())),
        ("movie:Avatar".into(), 0, None),
        ("movie:Some Short".into(), 0, None),
        // An extras folder without a movie next to it: read as a movie.
        ("movie:Teaser".into(), 0, None),
        ("movie:The Dark Knight".into(), 1, None),
        ("trailer:Trailer".into(), 0, Some("Avatar".to_string())),
    ];
    assert_eq!(titles(&db), want);
    let knight = db.items.values().find(|i| i.title == "The Dark Knight").unwrap();
    assert!(knight.file.as_ref().unwrap().path.ends_with("pt1.mkv"));
    // A second scan changes nothing.
    let ids: Vec<u64> = db.items.keys().copied().collect();
    scan(&mut db);
    assert_eq!(titles(&db), want);
    assert_eq!(db.items.keys().copied().collect::<Vec<_>>(), ids);
    // A third part shows up: still one movie, now with two further parts.
    std::fs::write(root.join("The Dark Knight (2008)/The Dark Knight (2008) - pt3.mkv"), b"x").unwrap();
    scan(&mut db);
    assert_eq!(db.items.values().find(|i| i.title == "The Dark Knight").unwrap().parts.len(), 2);
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn files_of_one_movie_become_versions() {
    let root = std::env::temp_dir().join(format!("lichtspiel-versions-test-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    for f in ["Exit 8 (2025)/Exit.8.2025.1080p.WebRip.mkv", "Exit 8 (2025)/Exit.8.2025.1080p.BD.mkv", "Heat (1995)/Heat (1995).mkv",
              "Dune (2021)/Dune (2021) {edition-Extended}.mkv", "Dune (2021)/Dune (2021).mkv"] {
        let p = root.join(f);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(&p, b"x").unwrap();
    }
    let mut db: Db = serde_json::from_value(serde_json::json!({"version": 1, "machine_id": "x", "next_id": 1, "settings": crate::data::db::Settings::default(),
        "sections": [{"id": 1, "name": "Filme", "kind": "movie", "paths": [root.to_string_lossy()], "created_at": 0}],
        "items": {}, "users": [], "invites": [], "tokens": [], "playlists": [], "views": {}, "plugins": {}})).unwrap();
    let section = db.sections[0].clone();
    let scan = |db: &mut Db| { scan_section(&section, "no-such-ffprobe", |f| f(db)); };
    scan(&mut db);
    let shown = |db: &Db| { let mut v: Vec<String> = db.items.values().filter(|i| !i.hidden()).map(|i| i.title.clone()).collect(); v.sort(); v };
    // Exit 8 is one title with two versions; Heat is alone; the {edition} file stays a title of its own.
    assert_eq!(shown(&db), vec!["Dune", "Dune", "Exit 8", "Heat"]);
    let main = db.items.values().find(|i| i.title == "Exit 8" && !i.hidden()).unwrap().id;
    assert_eq!(db.items.values().filter(|i| i.version_of() == Some(main)).count(), 1);
    // A second scan changes nothing; one split by hand stays apart.
    scan(&mut db);
    assert_eq!(shown(&db), vec!["Dune", "Dune", "Exit 8", "Heat"]);
    let v = db.items.values().find(|i| i.version_of() == Some(main)).unwrap().id;
    let it = db.items.get_mut(&v).unwrap();
    it.extra.remove("version_of");
    it.extra.insert("no_merge".into(), serde_json::json!(true));
    scan(&mut db);
    assert_eq!(shown(&db), vec!["Dune", "Dune", "Exit 8", "Exit 8", "Heat"]);
    let _ = std::fs::remove_dir_all(&root);
}

#[cfg(unix)]
#[test]
fn links_to_files_outside_the_library_are_skipped() {
    let root = std::env::temp_dir().join(format!("lichtspiel-links-test-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let lib = root.join("lib");
    for f in ["Heat (1995)/Heat (1995).mkv", "Inside (2002)/x", "Leak (2001)/x"] {
        std::fs::create_dir_all(lib.join(f).parent().unwrap()).unwrap();
    }
    std::fs::write(lib.join("Heat (1995)/Heat (1995).mkv"), b"x").unwrap();
    std::fs::write(root.join("secret.mkv"), b"x").unwrap();
    std::os::unix::fs::symlink(root.join("secret.mkv"), lib.join("Leak (2001)/Leak (2001).mkv")).unwrap();
    std::os::unix::fs::symlink(lib.join("Heat (1995)/Heat (1995).mkv"), lib.join("Inside (2002)/Inside (2002).mkv")).unwrap();
    // A link into another library is fine: that file can be seen there anyway.
    let other = root.join("other");
    std::fs::create_dir_all(other.join("Kids (2000)")).unwrap();
    std::fs::create_dir_all(lib.join("Curated (2003)")).unwrap();
    std::fs::write(other.join("Kids (2000)/Kids (2000).mkv"), b"x").unwrap();
    std::os::unix::fs::symlink(other.join("Kids (2000)/Kids (2000).mkv"), lib.join("Curated (2003)/Curated (2003).mkv")).unwrap();
    let mut db: Db = serde_json::from_value(serde_json::json!({"version": 1, "machine_id": "x", "next_id": 1, "settings": crate::data::db::Settings::default(),
        "sections": [{"id": 1, "name": "Filme", "kind": "movie", "paths": [lib.to_string_lossy()], "created_at": 0},
            {"id": 2, "name": "Andere", "kind": "movie", "paths": [other.to_string_lossy()], "created_at": 0}],
        "items": {}, "users": [], "invites": [], "tokens": [], "playlists": [], "views": {}, "plugins": {}})).unwrap();
    let section = db.sections[0].clone();
    scan_section(&section, "no-such-ffprobe", |f| f(&mut db));
    let mut titles: Vec<String> = db.items.values().map(|i| i.title.clone()).collect();
    titles.sort();
    assert_eq!(titles, vec!["Curated", "Heat", "Inside"]);
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn subtitles_in_the_store_are_found_next_to_those_beside_the_video() {
    let root = std::env::temp_dir().join(format!("lichtspiel-substore-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let (media, store) = (root.join("media").join("Movies").join("Heat (1995)"), root.join("data").join("subtitles"));
    std::fs::create_dir_all(&media).unwrap();
    std::fs::write(media.join("Heat (1995).mkv"), b"x").unwrap();
    std::fs::write(media.join("Heat (1995).en.srt"), b"x").unwrap();
    let mirrored = store_dir(&store, &media);
    assert!(mirrored.starts_with(&store) && mirrored.ends_with(Path::new("media").join("Movies").join("Heat (1995)")), "{}", mirrored.display());
    std::fs::create_dir_all(&mirrored).unwrap();
    std::fs::write(mirrored.join("Heat (1995).de.srt"), b"x").unwrap();
    std::fs::write(mirrored.join("Other (2000).de.srt"), b"x").unwrap();
    let found = find_subtitles_in(&media, "Heat (1995)", Some(&store));
    let _ = std::fs::remove_dir_all(&root);
    let names: Vec<String> = found.iter().map(|p| Path::new(p).file_name().unwrap().to_string_lossy().to_string()).collect();
    assert_eq!(names.len(), 2, "{found:?}");
    assert!(names.contains(&"Heat (1995).de.srt".to_string()) && names.contains(&"Heat (1995).en.srt".to_string()));
    #[cfg(windows)]
    assert_eq!(store_dir(Path::new("S"), Path::new(r"C:\Movies\X")), Path::new("S").join("C").join("Movies").join("X"));
}

/// A library in a temporary folder with these files, scanned once, and a scan function.
fn library_with(name: &str, kind: &str, files: &[&str], auto: bool) -> (PathBuf, Db) {
    let root = std::env::temp_dir().join(format!("lichtspiel-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    for f in files {
        let p = root.join(f);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(&p, b"x").unwrap();
    }
    let mut settings = crate::data::db::Settings::default();
    settings.empty_trash_automatically = auto;
    let db: Db = serde_json::from_value(serde_json::json!({"version": 1, "machine_id": "x", "next_id": 1, "settings": settings,
        "sections": [{"id": 1, "name": "L", "kind": kind, "paths": [root.to_string_lossy()], "created_at": 0}],
        "items": {}, "users": [], "invites": [], "tokens": [], "playlists": [], "views": {}, "plugins": {}})).unwrap();
    (root, db)
}

fn watched(db: &mut Db, id: u64) {
    db.views.insert(format!("1:{id}"), crate::data::db::ViewState { view_count: 1, finished: true, ..Default::default() });
}

fn listed(db: &Db, kind: ItemKind) -> Vec<(u64, String, bool)> {
    let mut v: Vec<(u64, String, bool)> = db.items.values().filter(|i| i.kind == kind && i.version_of().is_none())
        .map(|i| (i.id, i.file.as_ref().map(|f| Path::new(&f.path).file_name().unwrap().to_string_lossy().to_string()).unwrap_or_default(), i.extra.contains_key(MISSING))).collect();
    v.sort();
    v
}

#[test]
fn a_movie_file_swapped_for_another_keeps_its_title_and_watched_state() {
    for auto in [false, true] {
        let (root, mut db) = library_with("swap", "movie", &["The Invite (2023)/The Invite (2023).mkv", "Heat (1995).mkv"], auto);
        let section = db.sections[0].clone();
        let scan = |db: &mut Db| scan_section(&section, "no-such-ffprobe", |f| f(db));
        scan(&mut db);
        let id = listed(&db, ItemKind::Movie).into_iter().find(|t| t.1.starts_with("The Invite")).unwrap().0;
        watched(&mut db, id);
        // Swapped at once, with another name (a better version).
        std::fs::remove_file(root.join("The Invite (2023)/The Invite (2023).mkv")).unwrap();
        std::fs::write(root.join("The Invite (2023)/The.Invite.2023.2160p.mkv"), b"xx").unwrap();
        let r = scan(&mut db);
        assert_eq!(listed(&db, ItemKind::Movie).iter().filter(|t| t.0 == id).map(|t| (t.1.as_str(), t.2)).collect::<Vec<_>>(), vec![("The.Invite.2023.2160p.mkv", false)], "auto {auto}");
        assert_eq!(listed(&db, ItemKind::Movie).len(), 2);
        assert!(r.added.is_empty() && r.updated.contains(&id), "{r:?}");
        assert_eq!(db.views.get(&format!("1:{id}")).map(|v| v.view_count), Some(1));
        // Copied in first, the old one deleted later: the same.
        std::fs::write(root.join("The Invite (2023)/The Invite (2023) 1080p.mkv"), b"xxx").unwrap();
        scan(&mut db);
        std::fs::remove_file(root.join("The Invite (2023)/The.Invite.2023.2160p.mkv")).unwrap();
        let r = scan(&mut db);
        assert_eq!(listed(&db, ItemKind::Movie).iter().filter(|t| t.0 == id).map(|t| (t.1.as_str(), t.2)).collect::<Vec<_>>(), vec![("The Invite (2023) 1080p.mkv", false)], "auto {auto}");
        assert_eq!(db.items.values().filter(|i| i.kind == ItemKind::Movie).count(), 2, "no version left behind");
        assert_eq!(r.removed.len(), 1, "the title the new file had for a while goes: {r:?}");
        assert_eq!(db.views.get(&format!("1:{id}")).map(|v| v.view_count), Some(1));
        let _ = std::fs::remove_dir_all(&root);
    }
}

#[test]
fn a_subtitle_file_put_next_to_a_known_video_is_noticed_and_found() {
    let (root, mut db) = library_with("latesrt", "movie", &["Heat (1995)/Heat (1995).mkv"], false);
    let section = db.sections[0].clone();
    let scan = |db: &mut Db| scan_section(&section, "no-such-ffprobe", |f| f(db));
    scan(&mut db);
    let subs = |db: &Db| db.items.values().find_map(|i| i.file.as_ref().map(|f| f.external_subtitles.len())).unwrap();
    assert_eq!(subs(&db), 0);
    let paths = vec![root.to_string_lossy().to_string()];
    let before = folder_signature(&paths, false);
    // Folder times count in seconds.
    std::thread::sleep(std::time::Duration::from_millis(1100));
    std::fs::write(root.join("Heat (1995)/Heat (1995).de.srt"), b"1").unwrap();
    let after = folder_signature(&paths, false);
    let r = scan(&mut db);
    let added = subs(&db);
    std::fs::remove_file(root.join("Heat (1995)/Heat (1995).de.srt")).unwrap();
    scan(&mut db);
    let removed = subs(&db);
    let _ = std::fs::remove_dir_all(&root);
    assert_ne!(before, after, "the folder watcher must notice the new subtitle file");
    assert_eq!((added, removed), (1, 0));
    assert_eq!(r.updated.len(), 1);
    assert!(r.added.is_empty());
}

#[test]
fn another_movie_in_the_same_library_folder_is_not_taken_for_a_swap() {
    let (root, mut db) = library_with("noswap", "movie", &["Heat (1995).mkv"], false);
    let section = db.sections[0].clone();
    let scan = |db: &mut Db| scan_section(&section, "no-such-ffprobe", |f| f(db));
    scan(&mut db);
    let id = listed(&db, ItemKind::Movie)[0].0;
    watched(&mut db, id);
    std::fs::remove_file(root.join("Heat (1995).mkv")).unwrap();
    std::fs::write(root.join("Ronin (1998).mkv"), b"x").unwrap();
    let r = scan(&mut db);
    let _ = std::fs::remove_dir_all(&root);
    assert_eq!(r.added.len(), 1);
    let t = listed(&db, ItemKind::Movie);
    assert_eq!(t.iter().map(|t| (t.1.as_str(), t.2)).collect::<Vec<_>>(), vec![("Heat (1995).mkv", true), ("Ronin (1998).mkv", false)]);
    assert!(db.views.get(&format!("1:{}", r.added[0])).is_none());
}

// An online match renames the show; episodes added later (and a second show made before this fix) end up in it.
#[test]
fn a_show_renamed_by_its_match_takes_in_later_episodes() {
    let (root, mut db) = library_with("showrename", "show", &["Ringe der Macht/Season 1/Ringe der Macht - S01E01.mkv"], false);
    let section = db.sections[0].clone();
    let scan = |db: &mut Db| scan_section(&section, "no-such-ffprobe", |f| f(db));
    scan(&mut db);
    let show = db.items.values().find(|i| i.kind == ItemKind::Show).unwrap().id;
    let rename = |db: &mut Db, id: u64| {
        let it = db.items.get_mut(&id).unwrap();
        it.extra.insert("file_title".into(), Value::from(it.title.clone()));
        it.title = "Der Herr der Ringe: Die Ringe der Macht".into();
        it.sort_title = sort_title(&it.title);
    };
    rename(&mut db, show);
    std::fs::write(root.join("Ringe der Macht/Season 1/Ringe der Macht - S01E02.mkv"), b"x").unwrap();
    std::fs::create_dir_all(root.join("Ringe der Macht/Season 2")).unwrap();
    std::fs::write(root.join("Ringe der Macht/Season 2/Ringe der Macht - S02E01.mkv"), b"x").unwrap();
    scan(&mut db);
    let shows: Vec<u64> = db.items.values().filter(|i| i.kind == ItemKind::Show).map(|i| i.id).collect();
    assert_eq!(shows, vec![show]);
    // A second show as the old scanner made it: its season 2 and episodes go into the first, it goes.
    let s2 = db.items.values().find(|i| i.kind == ItemKind::Season && i.index == Some(2)).unwrap().id;
    let s1 = db.items.values().find(|i| i.kind == ItemKind::Season && i.index == Some(1)).unwrap().id;
    let dup = db.new_id();
    let mut d = db.items[&show].clone();
    d.id = dup;
    d.extra.remove("file_title");
    d.title = "Ringe der Macht".into();
    d.sort_title = sort_title(&d.title);
    db.items.insert(dup, d);
    rename(&mut db, dup);
    let dup_s1 = db.new_id();
    let mut s = db.items[&s1].clone();
    s.id = dup_s1;
    s.parent_id = Some(dup);
    db.items.insert(dup_s1, s);
    let e2 = db.items.values().find(|i| i.kind == ItemKind::Episode && i.parent_id == Some(s1) && i.index == Some(2)).unwrap().id;
    db.items.get_mut(&e2).unwrap().parent_id = Some(dup_s1);
    db.items.get_mut(&s2).unwrap().parent_id = Some(dup);
    scan(&mut db);
    let _ = std::fs::remove_dir_all(&root);
    assert!(!db.items.contains_key(&dup) && !db.items.contains_key(&dup_s1));
    assert_eq!(db.items[&e2].parent_id, Some(s1));
    assert_eq!(db.items[&s2].parent_id, Some(show));
}

#[test]
fn an_episode_file_swapped_for_another_keeps_its_watched_state() {
    let (root, mut db) = library_with("epswap", "show", &["Dark/Season 1/Dark - S01E01.mkv", "Dark/Season 1/Dark - S01E02.mkv"], false);
    let section = db.sections[0].clone();
    let scan = |db: &mut Db| scan_section(&section, "no-such-ffprobe", |f| f(db));
    scan(&mut db);
    let id = listed(&db, ItemKind::Episode).into_iter().find(|t| t.1.contains("E01")).unwrap().0;
    watched(&mut db, id);
    std::fs::remove_file(root.join("Dark/Season 1/Dark - S01E01.mkv")).unwrap();
    std::fs::write(root.join("Dark/Season 1/Dark.S01E01.Secrets.1080p.mkv"), b"xx").unwrap();
    scan(&mut db);
    let _ = std::fs::remove_dir_all(&root);
    let t = listed(&db, ItemKind::Episode);
    assert_eq!(t.len(), 2);
    assert!(t.contains(&(id, "Dark.S01E01.Secrets.1080p.mkv".into(), false)), "{t:?}");
    assert_eq!(db.views.get(&format!("1:{id}")).map(|v| v.view_count), Some(1));
}

#[test]
fn trash_keeps_gone_files_until_emptied() {
    let root = std::env::temp_dir().join(format!("lichtspiel-trash-test-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    for f in ["Avatar (2009)/Avatar (2009).mkv", "Avatar (2009)/Avatar (2009)-trailer.mkv", "Heat (1995)/Heat (1995).mkv"] {
        let p = root.join(f);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(&p, b"x").unwrap();
    }
    let mut settings = crate::data::db::Settings::default();
    settings.empty_trash_automatically = false;
    let mut db: Db = serde_json::from_value(serde_json::json!({"version": 1, "machine_id": "x", "next_id": 1, "settings": settings,
        "sections": [{"id": 1, "name": "Filme", "kind": "movie", "paths": [root.to_string_lossy()], "created_at": 0}],
        "items": {}, "users": [], "invites": [], "tokens": [], "playlists": [], "views": {}, "plugins": {}})).unwrap();
    let section = db.sections[0].clone();
    let scan = |db: &mut Db| scan_section(&section, "no-such-ffprobe", |f| f(db));
    scan(&mut db);
    assert_eq!(db.items.len(), 3);
    let missing = |db: &Db| { let mut v: Vec<String> = db.items.values().filter(|i| i.extra.contains_key(MISSING)).map(|i| i.title.clone()).collect(); v.sort(); v };
    // Gone from disk: kept, marked unavailable.
    std::fs::remove_file(root.join("Avatar (2009)/Avatar (2009).mkv")).unwrap();
    assert!(scan(&mut db).removed.is_empty());
    assert_eq!(missing(&db), vec!["Avatar"]);
    // Back again: available again.
    std::fs::write(root.join("Avatar (2009)/Avatar (2009).mkv"), b"x").unwrap();
    scan(&mut db);
    assert!(missing(&db).is_empty());
    // Gone again and the trash emptied: the movie and its trailer leave.
    std::fs::remove_file(root.join("Avatar (2009)/Avatar (2009).mkv")).unwrap();
    scan(&mut db);
    assert_eq!(empty_trash(&mut db, 1).len(), 2);
    assert_eq!(db.items.values().map(|i| i.title.clone()).collect::<Vec<_>>(), vec!["Heat"]);
    // A library folder that cannot be read empties nothing, even with the automatic setting.
    db.settings.empty_trash_automatically = true;
    std::fs::remove_dir_all(&root).unwrap();
    scan(&mut db);
    assert_eq!(missing(&db), vec!["Heat"]);
}

#[test]
fn music_library_makes_artists_albums_and_tracks() {
    let root = std::env::temp_dir().join(format!("lichtspiel-music-test-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    for f in ["Queen/Opera/02 - Death on Two Legs.mp3", "Queen/Opera/01 - Intro.mp3", "Queen/Jazz/01 - Mustapha.flac", "Blur/Blur/03 - Song 2.ogg", "notes.txt"] {
        let p = root.join(f);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(&p, b"x").unwrap();
    }
    let mut db: Db = serde_json::from_value(serde_json::json!({"version": 1, "machine_id": "x", "next_id": 1, "settings": crate::data::db::Settings::default(),
        "sections": [{"id": 1, "name": "Musik", "kind": "music", "paths": [root.to_string_lossy()], "created_at": 0}],
        "items": {}, "users": [], "invites": [], "tokens": [], "playlists": [], "views": {}, "plugins": {}})).unwrap();
    let section = db.sections[0].clone();
    let r = scan_section(&section, "no-such-ffprobe", |f| f(&mut db));
    assert_eq!(r.files_seen, 4);
    let count = |db: &Db, k: ItemKind| db.items.values().filter(|i| i.kind == k).count();
    assert_eq!((count(&db, ItemKind::Artist), count(&db, ItemKind::Album), count(&db, ItemKind::Track)), (2, 3, 4));
    let opera = db.items.values().find(|i| i.kind == ItemKind::Album && i.title == "Opera").unwrap();
    let songs: Vec<String> = db.children(opera.id).iter().map(|t| t.title.clone()).collect();
    assert_eq!(songs, vec!["Intro", "Death on Two Legs"]);
    // The filters of a music library know the kind of release.
    let tr = db.children(opera.id)[0].clone();
    assert!(matches!(crate::library::query::field(&db, 0, &tr, "release"), crate::library::query::FieldValue::Text(v) if v == vec!["album".to_string()]));
    let albums: Vec<&Item> = db.items.values().filter(|i| i.kind == ItemKind::Album).collect();
    let f = crate::library::query::section_filters(&db, 0, &albums, ItemKind::Album, true, false);
    assert!(f.iter().any(|x| x["filter"] == "release" && x["values"][0]["value"] == "album" && x["values"][0]["count"] == 3));
    // Playing an artist means all its songs, in album order.
    let queen = db.items.values().find(|i| i.kind == ItemKind::Artist && i.title == "Queen").unwrap();
    assert_eq!(db.leaves(queen.id).len(), 3);
    // Taking a file away leaves no empty album behind.
    std::fs::remove_file(root.join("Queen/Jazz/01 - Mustapha.flac")).unwrap();
    scan_section(&section, "no-such-ffprobe", |f| f(&mut db));
    assert_eq!((count(&db, ItemKind::Artist), count(&db, ItemKind::Album), count(&db, ItemKind::Track)), (2, 2, 3));
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn partial_scan_only_touches_its_folder() {
    let root = std::env::temp_dir().join(format!("lichtspiel-partial-test-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    for f in ["Avatar (2009)/Avatar (2009).mkv", "Heat (1995)/Heat (1995).mkv"] {
        let p = root.join(f);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(&p, b"x").unwrap();
    }
    let mut db: Db = serde_json::from_value(serde_json::json!({"version": 1, "machine_id": "x", "next_id": 1, "settings": crate::data::db::Settings::default(),
        "sections": [{"id": 1, "name": "Filme", "kind": "movie", "paths": [root.to_string_lossy()], "created_at": 0}],
        "items": {}, "users": [], "invites": [], "tokens": [], "playlists": [], "views": {}, "plugins": {}})).unwrap();
    let section = db.sections[0].clone();
    assert_eq!(section.scan_interval_minutes, None);
    let only = root.join("Heat (1995)");
    // Only Heat is looked at: Avatar is not added yet.
    let r = scan_section_in(&section, "no-such-ffprobe", Some(&only), |f| f(&mut db));
    assert_eq!(r.files_seen, 1);
    assert_eq!(db.items.values().map(|i| i.title.clone()).collect::<Vec<_>>(), vec!["Heat"]);
    // A full scan finds both; then a file gone from the other folder is not noticed by a partial scan.
    scan_section(&section, "no-such-ffprobe", |f| f(&mut db));
    assert_eq!(db.items.len(), 2);
    std::fs::remove_file(root.join("Avatar (2009)/Avatar (2009).mkv")).unwrap();
    let r = scan_section_in(&section, "no-such-ffprobe", Some(&only), |f| f(&mut db));
    assert!(r.removed.is_empty());
    assert_eq!(db.items.len(), 2);
    // The full scan sees it.
    assert_eq!(scan_section(&section, "no-such-ffprobe", |f| f(&mut db)).removed.len(), 1);
    let _ = std::fs::remove_dir_all(&root);
}

/// A rescan with nothing changed of 5,000 movie folders, 3,000 of them with a trailer: the time the
/// library is locked (all of it, and the longest single stretch).
/// `cargo test --release --no-default-features -- --ignored --nocapture rescan_lock_cost`
#[test]
#[ignore]
fn rescan_lock_cost() {
    let root = std::env::temp_dir().join(format!("lichtspiel-rescan-bench-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    for n in 0..5000 {
        let dir = root.join(format!("Film {n} ({})", 1950 + n % 70));
        std::fs::create_dir_all(dir.join("Trailers")).unwrap();
        std::fs::write(dir.join(format!("Film {n} ({}).mkv", 1950 + n % 70)), b"x").unwrap();
        if n % 5 < 3 { std::fs::write(dir.join("Trailers").join("Trailer.mkv"), b"x").unwrap(); }
    }
    let mut db: Db = serde_json::from_value(serde_json::json!({"version": 1, "machine_id": "x", "next_id": 1, "settings": crate::data::db::Settings::default(),
        "sections": [{"id": 1, "name": "Filme", "kind": "movie", "paths": [root.to_string_lossy()], "created_at": 0}],
        "items": {}, "users": [], "invites": [], "tokens": [], "playlists": [], "views": {}, "plugins": {}})).unwrap();
    let section = db.sections[0].clone();
    scan_section(&section, "no-such-ffprobe", |f| f(&mut db));
    assert_eq!(db.items.values().filter(|i| i.is_extra()).count(), 3000);
    let (mut sums, mut longest) = (vec![], vec![]);
    for _ in 0..5 {
        let (mut sum, mut top) = (std::time::Duration::ZERO, std::time::Duration::ZERO);
        scan_section(&section, "no-such-ffprobe", |f| {
            let t0 = std::time::Instant::now();
            f(&mut db);
            let d = t0.elapsed();
            sum += d;
            top = top.max(d);
        });
        sums.push(sum);
        longest.push(top);
    }
    sums.sort();
    longest.sort();
    println!("rescan_lock_cost: library locked {:?} in all, longest stretch {:?} (medians of 5)", sums[2], longest[2]);
    assert_eq!(db.items.values().filter(|i| i.is_extra()).count(), 3000);
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn plex_split_parts_and_id_tags() {
    assert_eq!(split_part("The Dark Knight (2008) - pt1"), ("The Dark Knight (2008)".into(), Some(1)));
    assert_eq!(split_part("The Dark Knight (2008) - cd2"), ("The Dark Knight (2008)".into(), Some(2)));
    assert_eq!(split_part("Movie.2008.part3"), ("Movie.2008".into(), Some(3)));
    assert_eq!(split_part("Deathly Hallows Part 2"), ("Deathly Hallows Part 2".into(), None));
    assert_eq!(split_part("Apt 2"), ("Apt 2".into(), None));
    assert_eq!(split_part("Script1"), ("Script1".into(), None));
    assert_eq!(id_tag("Batman Begins (2005) {imdb-tt0372784}"), Some(("imdb", "tt0372784".into())));
    assert_eq!(id_tag("Avatar (2009) {edition-Extended} {tmdb-19995}"), Some(("tmdb", "19995".into())));
    assert_eq!(id_tag("Show [tvdbid=81189]"), Some(("tvdb", "81189".into())));
    assert_eq!(id_tag("Film {imdb-12}"), None);
    assert_eq!(id_tag("Film {edition-Final Cut}"), None);
    assert_eq!(parse_title_year("Apocalypse.Now.1979.Final.Cut.Remastered.German.AC3.DL.1080p.BluRay.x265-FuN {edition-Final Cut}"), ("Apocalypse Now".into(), Some(1979)));
    assert_eq!(parse_title_year("Batman Begins (2005) {imdb-tt0372784}"), ("Batman Begins".into(), Some(2005)));
}
#[test]
fn credits_in_picture() {
    let frame = |dark: u8, text_px: usize| {
        let mut f = vec![dark; FRAME_W * FRAME_H];
        for p in f.iter_mut().take(text_px) {
            *p = 230;
        }
        f
    };
    assert_eq!(frame_kind(&frame(10, 400)), FrameKind::Credits);
    assert_eq!(frame_kind(&frame(5, 0)), FrameKind::Black);
    assert_eq!(frame_kind(&frame(120, 0)), FrameKind::Picture);
    // 60 s of film, 5 s black, 40 s of credits with one bright flash, 10 s black.
    let mut k = vec![FrameKind::Picture; 60];
    k.extend([FrameKind::Black; 5]);
    k.extend([FrameKind::Credits; 20]);
    k.push(FrameKind::Picture);
    k.extend([FrameKind::Credits; 19]);
    k.extend([FrameKind::Black; 10]);
    assert_eq!(credits_start(&k, 20), Some(65));
    // A dark scene of a few seconds is not credits.
    let mut short = vec![FrameKind::Picture; 60];
    short.extend([FrameKind::Credits; 8]);
    short.extend([FrameKind::Picture; 30]);
    assert_eq!(credits_start(&short, 20), None);
}
#[test]
fn titles() {
    assert_eq!(parse_title_year("The.Matrix.1999.1080p.BluRay.x264"), ("The Matrix".into(), Some(1999)));
    assert_eq!(parse_title_year("Abyss.-.Abgrund.des.Todes.THEATRICAL.1989.German.DL.AC3D.1080p.WebHD.x265-FuN"), ("Abyss - Abgrund des Todes".into(), Some(1989)));
    assert_eq!(parse_title_year("Blade Runner 2049 (2017)"), ("Blade Runner 2049".into(), Some(2017)));
    assert_eq!(parse_title_year("Up (2009)"), ("Up".into(), Some(2009)));
}
#[test]
fn episodes() {
    assert_eq!(parse_episode("Show - S01E02 - Title").map(|e| (e.0, e.1)), Some((1, 2)));
    assert_eq!(parse_episode("show.s10e05.720p").map(|e| (e.0, e.1)), Some((10, 5)));
    assert_eq!(parse_episode("Show 3x07 Name").map(|e| (e.0, e.1)), Some((3, 7)));
}

#[cfg(test)]
mod trailer_tests {
    use super::*;

    #[test]
    fn original_extra_folders_and_trailer_names() {
        for (name, kind) in [("Trailers", "trailer"), ("trailer", "trailer"), ("Trailer_", "trailer"), ("Deleted Scenes", "deleted"), ("deleted", "deleted"),
            ("Behind the Scenes", "behindthescenes"), ("behind-the-scenes", "behindthescenes"), ("Featurettes", "featurette"), ("Interviews", "interview"),
            ("Scenes", "scene"), ("Shorts", "short"), ("Other", "other"), ("Bonus Extras", "other"), ("extra", "other")] {
            assert_eq!(extra_folder_kind(name), Some(kind), "{name}");
        }
        for name in ["Trailer Park Boys", "Shortcuts", "Scenery", "Extraordinary", "Others Here"] {
            assert_eq!(extra_folder_kind(name), None, "{name}");
        }
        let root = Path::new("/m");
        let x = extra_of(root, Path::new("/m/Heat (1995)/trailer.mkv")).expect("trailer.mkv");
        assert_eq!((x.kind, x.prefix.clone()), ("trailer", None));
        assert_eq!(x.movie_dir, Path::new("/m/Heat (1995)"));
        assert_eq!(extra_of(root, Path::new("/m/Heat (1995)/movie-trailer.mp4")).map(|x| x.kind), Some("trailer"));
        assert_eq!(extra_of(root, Path::new("/m/Heat (1995)/Trailer_/Teaser.mkv")).map(|x| x.kind), Some("trailer"));
        assert_eq!(extra_of(root, Path::new("/m/Heat (1995)/Heat (1995)-trailer.mkv")).map(|x| x.kind), Some("trailer"));
        assert!(extra_of(root, Path::new("/m/Heat (1995)/Heat (1995).mkv")).is_none());
    }
}

#[cfg(test)]
mod watch_bench {
    /// How long one look of folder watching takes on 50,000 files (500 folders of 100).
    /// Run with `cargo test --release folder_signature_on_50k_files -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn folder_signature_on_50k_files() {
        let root = std::env::temp_dir().join("lichtspiel-watch-bench");
        if !root.join("done").exists() {
            for d in 0..500 {
                let dir = root.join(format!("Show {d}")).join("Season 1");
                std::fs::create_dir_all(&dir).unwrap();
                for f in 0..100 { std::fs::write(dir.join(format!("Show {d} S01E{f:03}.mkv")), b"").unwrap(); }
            }
            std::fs::write(root.join("done"), b"").unwrap();
        }
        let paths = vec![root.to_string_lossy().to_string()];
        let _ = super::folder_signature(&paths, false);
        let t = std::time::Instant::now();
        for _ in 0..5 { let _ = super::folder_signature(&paths, false); }
        eprintln!("folder_signature over 50,000 files: {:.0} ms per look", t.elapsed().as_secs_f64() * 1000.0 / 5.0);
    }
}

//! The preview of what the old database holds, and the import itself, step by step.

use super::*;

/// What the old database holds, with suggested folders and account matches.
pub fn preview(app: &App, src: &Path) -> Result<Value, String> {
    let old = read(src, &app.data_dir.join("cache").join("old-server"))?;
    let db = app.db.lock().unwrap();
    let ours: Vec<String> = db.sections.iter().flat_map(|s| s.paths.clone()).collect();
    let count = |k: i64| old.items.values().filter(|i| i.kind == k).count();
    let watched_by = |a: i64| old.settings.iter().filter(|s| s.account == a && s.views > 0).count();
    Ok(json!({
        "db": src.to_string_lossy(),
        "libraries": old.sections.iter().map(|s| json!({
            "id": s.id, "name": s.name, "type": match s.kind { 1 => "movie", 2 => "show", 8 => "music", 13 => "photo", _ => "other" },
            "imported": matches!(s.kind, 1 | 2),
            "folders": s.paths.iter().map(|p| json!({"old": p, "suggested": suggest(p, &ours), "exists_here": Path::new(p).is_dir()})).collect::<Vec<_>>(),
        })).collect::<Vec<_>>(),
        "accounts": old.accounts.iter().filter(|(_, n)| !n.trim().is_empty() || true).map(|(id, name)| {
            let matched = if *id == 1 { None } else { db.users.iter().find(|u| u.username.eq_ignore_ascii_case(name.trim())).map(|u| u.id) };
            json!({"id": id, "name": if name.trim().is_empty() { format!("account {id}") } else { name.clone() }, "owner": *id == 1,
                "watched": watched_by(*id), "suggested_user": matched})
        }).filter(|a| a["owner"] == true || a["watched"].as_u64().unwrap_or(0) > 0 || a["suggested_user"].is_number()).collect::<Vec<_>>(),
        "counts": {"movies": count(1), "shows": count(2), "episodes": count(4), "files": old.parts.len(),
            "watched_states": old.settings.iter().filter(|s| s.views > 0).count(),
            "resume_points": old.settings.iter().filter(|s| s.offset_ms > 0).count(),
            "ratings": old.settings.iter().filter(|s| s.rating.is_some()).count(),
            "playlists": old.playlists.iter().filter(|p| !p.smart).count(), "smart_playlists": old.playlists.iter().filter(|p| p.smart).count(),
            "credits_markers": old.markers.iter().filter(|m| m.1 == "credits").count()},
        "warnings": old.warnings,
    }))
}


#[derive(Debug, Clone)]
pub struct Options {
    pub db: PathBuf,
    pub path_map: Vec<(String, String)>,
    pub libraries: bool,
    pub watched: bool,
    pub playlists: bool,
    pub markers: bool,
    pub details: bool,
    /// old account id → user here (missing = skipped)
    pub accounts: HashMap<i64, u64>,
}

/// The error of an import that was cancelled between two of its steps.
pub const CANCELLED: &str = "Cancelled";

fn set_job(app: &App, label: &str, done: usize, total: usize) {
    *app.job.lock().unwrap() = Some((format!("Importing: {label}"), done, total));
}

/// Runs the whole import (call it on a background thread). Returns a report.
pub fn run(app: &Arc<App>, o: &Options) -> Result<Value, String> {
    set_job(app, "reading the old database", 0, 6);
    let old = read(&o.db, &app.data_dir.join("cache").join("old-server"))?;
    let mut report = json!({"started_at": now(), "db": o.db.to_string_lossy(), "warnings": old.warnings});

    // 1. Libraries: make the missing ones and scan them (only new files are read).
    let mut created = vec![];
    let mut scanned = vec![];
    if o.libraries {
        let todo: Vec<&OldSection> = old.sections.iter().filter(|s| matches!(s.kind, 1 | 2)).collect();
        for (n, s) in todo.iter().enumerate() {
            set_job(app, &format!("library \"{}\"", s.name), n, todo.len());
            let paths: Vec<String> = s.paths.iter().map(|p| map_path(p, &o.path_map)).collect();
            let kind = if s.kind == 2 { SectionKind::Show } else { SectionKind::Movie };
            let id = {
                let mut db = app.db.lock().unwrap();
                let keys: HashSet<String> = paths.iter().map(|p| path_key(p)).collect();
                match db.sections.iter().find(|x| x.kind == kind && x.paths.iter().any(|p| keys.contains(&path_key(p)))) {
                    Some(x) => x.id,
                    None => {
                        let id = db.new_id();
                        let lang: String = s.language.chars().take(2).collect::<String>().to_lowercase();
                        let language = if db::LANGUAGES.iter().any(|l| l.0 == lang) && lang != db.settings.metadata_language { lang } else { String::new() };
                        db.sections.push(Section { id, name: s.name.clone(), kind, paths: paths.clone(), created_at: now(), scanned_at: 0, credits_detection: true, intro_detection: true, language, scan_interval_minutes: None });
                        created.push(json!({"id": id, "name": s.name, "folders": paths}));
                        id
                    }
                }
            };
            app.save();
            // Another scan of this library may be running (folder watching): wait for it.
            for _ in 0..600 {
                if app.scan_now(id).is_some() {
                    break;
                }
                std::thread::sleep(std::time::Duration::from_millis(500));
            }
            scanned.push(s.name.clone());
        }
    }
    report["libraries_created"] = json!(created);
    report["libraries_scanned"] = json!(scanned);

    // 2. Match files: old item → item here.
    if app.job_cancelled() { return Err(CANCELLED.into()); }
    set_job(app, "matching files", 2, 6);
    let mut map: HashMap<i64, u64> = HashMap::new();
    let mut missing = vec![];
    {
        let db = app.db.lock().unwrap();
        let by_path: HashMap<String, u64> = db.items.values().filter_map(|i| i.file.as_ref().map(|f| (path_key(&f.path), i.id))).collect();
        for (oid, file) in &old.parts {
            let here = map_path(file, &o.path_map);
            match by_path.get(&path_key(&here)) {
                Some(id) => {
                    map.insert(*oid, *id);
                }
                None => missing.push(here),
            }
        }
        // Seasons and shows follow from their episodes.
        let leaves: Vec<(i64, u64)> = map.iter().map(|(a, b)| (*a, *b)).collect();
        for (oid, id) in leaves {
            let (mut o_cur, mut h_cur) = (oid, id);
            for _ in 0..2 {
                let (Some(op), Some(hp)) = (old.items.get(&o_cur).and_then(|i| i.parent), db.items.get(&h_cur).and_then(|i| i.parent_id)) else { break };
                if !old.items.get(&op).is_some_and(|i| matches!(i.kind, 2 | 3)) {
                    break;
                }
                map.entry(op).or_insert(hp);
                (o_cur, h_cur) = (op, hp);
            }
        }
    }
    let matched_files = old.parts.len() - missing.len();
    report["files"] = json!({"in_old_database": old.parts.len(), "found_here": matched_files, "not_found": missing.len(),
        "not_found_examples": missing.iter().take(12).collect::<Vec<_>>()});
    // One guid can stand for several items here (the same movie in "Movies" and "Movies 4K").
    let mut by_guid: HashMap<&str, Vec<u64>> = HashMap::new();
    for i in old.items.values().filter(|i| !i.guid.is_empty()) {
        if let Some(h) = map.get(&i.id) { by_guid.entry(i.guid.as_str()).or_default().push(*h); }
    }
    let items_of = |guid: &str| by_guid.get(guid).cloned().unwrap_or_default();

    let mut db = app.db.lock().unwrap();
    // The day the old server added things (not the day of this import): "Recently added" and the
    // season premieres in Continue Watching depend on it.
    for (oid, id) in &map {
        let added = old.items.get(oid).map_or(0, |i| i.added);
        if let Some(it) = db.items.get_mut(id).filter(|_| added > 0) {
            it.added_at = it.added_at.min(added as u64);
        }
    }

    // 3. Watched state, resume points, ratings.
    if app.job_cancelled() { return Err(CANCELLED.into()); }
    set_job(app, "watched state and ratings", 3, 6);
    let (mut watched, mut resume, mut rated, mut skipped_accounts) = (0, 0, 0, HashSet::new());
    if o.watched {
        for s in &old.settings {
            let Some(user) = o.accounts.get(&s.account).copied() else {
                skipped_accounts.insert(s.account);
                continue;
            };
            if !db.users.iter().any(|u| u.id == user) {
                continue;
            }
            for item in items_of(&s.guid) {
                let leaf = db.items.get(&item).is_some_and(|i| i.kind.playable());
                let v = db.view_mut(user, item);
                if leaf && s.views > 0 && (s.views as u32) > v.view_count {
                    v.view_count = s.views as u32;
                    watched += 1;
                }
                if leaf && s.offset_ms > 0 && s.last_viewed >= v.last_viewed_at as i64 {
                    v.view_offset_ms = s.offset_ms as u64;
                    resume += 1;
                }
                // Finished there after it was last played here: the resume point here is from before.
                if leaf && s.views > 0 && s.offset_ms == 0 && v.view_offset_ms > 0 && s.last_viewed >= v.last_viewed_at as i64 {
                    v.view_offset_ms = 0;
                }
                if s.last_viewed as u64 > v.last_viewed_at {
                    v.last_viewed_at = s.last_viewed as u64;
                }
                if let Some(r) = s.rating {
                    v.rating = Some((r as f32).clamp(0.0, 10.0));
                    rated += 1;
                }
            }
        }
    }
    // The play history: every viewing the old server logged; for watched items it has no log of, one viewing at the last time.
    let mut history = 0;
    if o.watched {
        let mut logged: HashSet<(u64, u64)> = HashSet::new();
        // Viewings already in the history (an earlier import) are not added again.
        let mut had: HashSet<(u64, u64, u64)> = db.history.iter().map(|e| (e.user_id, e.item_id, e.at)).collect();
        for (account, guid, at) in &old.views {
            let Some(user) = o.accounts.get(account).copied() else { continue };
            for item in items_of(guid) {
                if !db.users.iter().any(|u| u.id == user) || !db.items.get(&item).is_some_and(|i| i.kind.playable()) { continue; }
                logged.insert((user, item));
                if !had.insert((user, item, *at as u64)) { continue; }
                db.record_play_at(user, item, false, *at as u64);
                history += 1;
            }
        }
        for s in old.settings.iter().filter(|s| s.views > 0 && s.last_viewed > 0) {
            let Some(user) = o.accounts.get(&s.account).copied() else { continue };
            for item in items_of(&s.guid) {
                if logged.contains(&(user, item)) || !db.users.iter().any(|u| u.id == user) || !db.items.get(&item).is_some_and(|i| i.kind.playable()) { continue; }
                if !had.insert((user, item, s.last_viewed as u64)) { continue; }
                db.record_play_at(user, item, false, s.last_viewed as u64);
                history += 1;
            }
        }
    }
    report["watched"] = json!({"watched": watched, "history_entries": history, "resume_points": resume, "ratings": rated,
        "accounts_skipped": old.accounts.iter().filter(|(id, _)| skipped_accounts.contains(id)).map(|(_, n)| n.clone()).collect::<Vec<_>>()});

    // 4. Credits markers the old server found (it uses a trained model for that).
    if app.job_cancelled() { return Err(CANCELLED.into()); }
    set_job(app, "credits markers", 4, 6);
    let mut markers = 0;
    if o.markers {
        let mut per_item: HashMap<u64, Vec<u64>> = HashMap::new();
        for (oid, kind, start, _end) in &old.markers {
            if kind == "credits" && *start > 0 {
                if let Some(id) = map.get(oid) {
                    per_item.entry(*id).or_default().push(*start as u64);
                }
            }
        }
        for (id, mut c) in per_item {
            c.sort();
            c.dedup();
            if let Some(f) = db.items.get_mut(&id).and_then(|i| i.file.as_mut()) {
                f.credits_ms = c;
                f.credits_source = "imported".into();
                f.credits_checked = db::CREDITS_DETECTOR_VERSION;
                f.credits_errors = 0;
                markers += 1;
            }
        }
    }
    report["credits_markers"] = json!(markers);

    // 5. Titles, plots, genres … (kept: online details won't replace them).
    if app.job_cancelled() { return Err(CANCELLED.into()); }
    set_job(app, "titles and plots", 5, 6);
    let mut details = 0;
    let (mut people, mut people_photos): (usize, Vec<(String, String)>) = (0, vec![]);
    if o.details {
        for (oid, id) in &map {
            let Some(oi) = old.items.get(oid) else { continue };
            let Some(it) = db.items.get_mut(id) else { continue };
            let mut m = json!({});
            for (k, v) in [("title", &oi.title), ("summary", &oi.summary), ("tagline", &oi.tagline), ("studio", &oi.studio), ("content_rating", &oi.content_rating)] {
                if !v.trim().is_empty() {
                    m[k] = json!(v.trim());
                }
            }
            // Season names stay "Season 1"; their plot is taken.
            if oi.kind == 3 {
                m.as_object_mut().map(|o| o.remove("title"));
            }
            if let Some(y) = oi.year {
                m["year"] = json!(y);
            }
            if let Some(r) = oi.rating {
                m["rating"] = json!((r * 10.0).round() / 10.0);
            }
            if let Some(g) = old.genres.get(oid).filter(|g| !g.is_empty()) {
                m["genres"] = json!(g);
            }
            // Actors, directors and writers (shared people).
            if let Some(cast) = old.cast.get(oid).filter(|c| !c.is_empty()) {
                let list: Vec<Value> = cast
                    .iter()
                    .map(|(name, role, photo)| {
                        let mut c = json!({"name": name, "role": role});
                        c["key"] = json!(crate::server::person_key(&c));
                        if photo.starts_with("http://") || photo.starts_with("https://") {
                            c["photo"] = json!(photo);
                            people_photos.push((c["key"].as_str().unwrap_or("").to_string(), photo.clone()));
                        }
                        c
                    })
                    .collect();
                people += list.len();
                m["extra"]["cast"] = json!(list);
            }
            for (k, src) in [("directors", &old.directors), ("writers", &old.writers), ("collections", &old.collections), ("producers", &old.producers), ("countries", &old.countries)] {
                if let Some(v) = src.get(oid).filter(|v| !v.is_empty()) {
                    m["extra"][k] = json!(v);
                }
            }
            if m.as_object().is_some_and(|o| o.is_empty()) {
                continue;
            }
            if !it.extra.contains_key("file_title") {
                it.extra.insert("file_title".into(), json!(it.title));
            }
            apply_metadata(it, &m, true);
            if !oi.original_title.trim().is_empty() && oi.original_title != oi.title {
                it.extra.insert("original_title".into(), json!(oi.original_title));
            }
            if !oi.aired.is_empty() {
                it.extra.insert("originally_available".into(), json!(oi.aired));
            }
            it.extra.insert("imported_from_old_server".into(), json!(now()));
            details += 1;
        }
    }
    report["details"] = json!(details);
    report["cast_entries"] = json!(people);

    // The match Plex made: the online lookup takes exactly that entry instead of searching by title.
    let mut linked = 0;
    let mut fresh: Vec<u64> = vec![];
    let mut without: Vec<String> = vec![];
    let mut candidates = 0;
    for (oid, id) in &map {
        let Some(oi) = old.items.get(oid) else { continue };
        if !matches!(oi.kind, 1 | 2) { continue; }
        candidates += 1;
        let tags = old.external.get(oid).map(|v| v.as_slice()).unwrap_or(&[]);
        let Some((src, eid)) = plex_external_id(&oi.guid, tags) else {
            if without.len() < 5 { without.push(format!("{} \u{2192} {}{}", oi.title, oi.guid, if tags.is_empty() { String::new() } else { format!(" {tags:?}") })); }
            continue;
        };
        let Some(it) = db.items.get_mut(id) else { continue };
        it.extra.insert("plex_id".into(), json!({"source": src, "id": eid}));
        linked += 1;
        // Not matched here yet: its details are fetched at once, whatever an earlier miss was.
        if !it.extra.contains_key("metadata_source") && it.extra.get("unmatched_by_hand").and_then(|v| v.as_bool()) != Some(true) {
            fresh.push(*id);
        }
    }
    report["matches"] = json!(linked);
    report["match_candidates"] = json!(candidates);
    report["match_examples_without_id"] = json!(without);

    // 6. Playlists (manual ones; smart ones use the old server's own rule format).
    if app.job_cancelled() { return Err(CANCELLED.into()); }
    set_job(app, "playlists", 6, 6);
    let (mut made, mut updated) = (vec![], vec![]);
    let mut smart_skipped = vec![];
    if o.playlists {
        for p in &old.playlists {
            if p.smart {
                smart_skipped.push(p.title.clone());
                continue;
            }
            let Some(owner) = o.accounts.get(&p.account).copied() else { continue };
            let mut items: Vec<u64> = vec![];
            for oi in &p.items {
                let Some(h) = map.get(oi) else { continue };
                for l in db.leaves(*h) {
                    if !items.contains(&l) {
                        items.push(l);
                    }
                }
            }
            if items.is_empty() {
                continue;
            }
            let t = now();
            if let Some(ex) = db.playlists.iter_mut().find(|x| x.owner_id == owner && x.kind == PlaylistKind::Manual && x.title == p.title) {
                ex.items = items;
                ex.updated_at = t;
                updated.push(p.title.clone());
            } else {
                let id = db.new_id();
                db.playlists.push(Playlist { id, owner_id: owner, title: p.title.clone(), summary: p.summary.clone(), kind: PlaylistKind::Manual, items, query: None, default_key: None, shared: false, pinned: false, created_at: t, updated_at: t });
                made.push(p.title.clone());
            }
        }
    }
    report["playlists"] = json!({"created": made, "updated": updated, "smart_not_imported": smart_skipped});
    drop(db);
    app.save();
    // Actor photos the old server knew: one per person, shared by everything they're in.
    people_photos.sort();
    people_photos.dedup_by(|a, b| a.0 == b.0);
    let mut photos = 0;
    let total = people_photos.len();
    for (n, (key, url)) in people_photos.iter().enumerate() {
        if key.is_empty() {
            continue;
        }
        if n % 20 == 0 {
            set_job(app, &format!("actor photos {n} of {total}"), 5, 6);
        }
        let dest = app.person_photo(key);
        if crate::library::metadata::file_image_type(&dest).is_some() || crate::library::metadata::download(url, &dest) {
            photos += 1;
        }
    }
    if photos > 0 {
        let mut db = app.db.lock().unwrap();
        for id in map.values() {
            let Some(it) = db.items.get_mut(id) else { continue };
            let Some(Value::Array(cast)) = it.extra.get_mut("cast") else { continue };
            for c in cast.iter_mut() {
                let key = c["key"].as_str().unwrap_or("").to_string();
                if !key.is_empty() && c["thumb"].is_null() && crate::library::metadata::file_image_type(&app.person_photo(&key)).is_some() {
                    c["thumb"] = json!(format!("/library/people/{key}/thumb"));
                }
            }
        }
        drop(db);
        app.save();
    }
    report["actor_photos"] = json!(photos);
    report["finished_at"] = json!(now());
    let _ = std::fs::write(app.data_dir.join("cache").join("old-server").join("last-import.json"), serde_json::to_string_pretty(&report).unwrap_or_default());
    // Online details for what's new (posters, cast); imported titles and plots stay.
    let ids: Vec<u64> = map.values().copied().collect();
    app.queue_online_details(&ids, false);
    // Titles with Plex's number are looked up for sure (a miss from before does not hold them back).
    let online = app.db.lock().unwrap().settings.metadata_enabled;
    if online && !fresh.is_empty() {
        app.queue_online_details(&fresh, true);
    }
    Ok(report)
}

/// A one-line summary of a report.
pub fn summary(r: &Value) -> String {
    format!(
        "Import finished: {} of {} files found, {} libraries added, {} watched, {} resume points, {} ratings, {} playlists, {} credits markers, details for {} items",
        r["files"]["found_here"], r["files"]["in_old_database"], r["libraries_created"].as_array().map(|a| a.len()).unwrap_or(0),
        r["watched"]["watched"], r["watched"]["resume_points"], r["watched"]["ratings"],
        r["playlists"]["created"].as_array().map(|a| a.len()).unwrap_or(0) + r["playlists"]["updated"].as_array().map(|a| a.len()).unwrap_or(0),
        r["credits_markers"], r["details"]
    )
}

pub fn options_from(body: &Value, me: u64) -> Result<Options, String> {
    let given = body["db"].as_str().map(str::trim).filter(|s| !s.is_empty()).map(PathBuf::from);
    let db = match given {
        Some(p) => find_db(&p).ok_or_else(|| format!("No {DB_NAME} found at {}", p.display()))?,
        None => detect().ok_or("The old server's data folder was not found; enter where it is")?,
    };
    let path_map: Vec<(String, String)> = body["path_map"]
        .as_array()
        .map(|a| a.iter().filter_map(|m| Some((m["from"].as_str()?.to_string(), m["to"].as_str()?.to_string()))).filter(|(f, t)| !f.trim().is_empty() && !t.trim().is_empty()).collect())
        .unwrap_or_default();
    let mut accounts: HashMap<i64, u64> = HashMap::new();
    if let Some(m) = body["accounts"].as_object() {
        for (k, v) in m {
            if let (Ok(old), Some(here)) = (k.parse::<i64>(), v.as_u64().filter(|u| *u > 0)) {
                accounts.insert(old, here);
            }
        }
    } else {
        accounts.insert(1, me);
    }
    let on = |k: &str| body.get(k).and_then(Value::as_bool).unwrap_or(true);
    Ok(Options { db, path_map, libraries: on("libraries"), watched: on("watched"), playlists: on("playlists"), markers: on("markers"), details: on("details"), accounts })
}

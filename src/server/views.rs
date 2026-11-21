//! The JSON shapes of items, libraries, playlists and users, and the lists built from them (hubs,
//! continue watching, on deck), plus placeholder posters.

use super::*;

/// Who is whose child and which items are merged versions of which, found in one pass over the
/// library. A list of items builds it once; without it every item would scan the whole library.
pub struct Index {
    kids: HashMap<u64, Vec<u64>>,
    versions: HashMap<u64, Vec<u64>>,
}


impl Index {
    pub fn build(db: &Db) -> Index {
        let mut kids: HashMap<u64, Vec<u64>> = HashMap::new();
        let mut versions: HashMap<u64, Vec<u64>> = HashMap::new();
        for i in db.items.values() {
            if let Some(p) = i.parent_id.filter(|_| !db.is_version(i)) { kids.entry(p).or_default().push(i.id); }
            if let Some(v) = i.version_of() { versions.entry(v).or_default().push(i.id); }
        }
        Index { kids, versions }
    }

    /// Everything playable below an item (like `Db::leaves`).
    pub fn leaves(&self, db: &Db, id: u64) -> Vec<u64> {
        let mut out = vec![];
        self.collect_leaves(db, id, &mut out);
        out
    }

    fn collect_leaves(&self, db: &Db, id: u64, out: &mut Vec<u64>) {
        let Some(item) = db.items.get(&id) else { return };
        if item.kind.playable() { out.push(id); return; }
        for c in self.kids.get(&id).into_iter().flatten() { self.collect_leaves(db, *c, out); }
    }
}


pub fn item_json(db: &Db, user: &User, it: &Item, detail: bool) -> Value {
    item_json_ix(db, user, it, detail, None)
}


/// Like `item_json`; with an index (for lists) the children and versions are looked up, not searched for.
pub fn item_json_ix(db: &Db, user: &User, it: &Item, detail: bool, ix: Option<&Index>) -> Value {
    let v = db.view(user.id, it.id);
    let mut o = json!({
        "ratingKey": it.id.to_string(),
        "id": it.id,
        "key": format!("/library/metadata/{}", it.id),
        "type": it.kind.as_str(),
        "metadataType": it.kind.code(),
        "title": it.title,
        "titleSort": it.sort_title,
        "librarySectionID": it.section_id,
        "summary": it.summary,
        "tagline": it.tagline,
        "genres": it.genres,
        "addedAt": it.added_at,
        "updatedAt": it.updated_at,
        "duration": it.duration_ms,
        // "?v=" changes with the item, so a browser that kept an old (or broken)
        // picture fetches the new one instead of showing it from its cache.
        "thumb": format!("/library/metadata/{}/thumb?v={}", it.id, it.updated_at),
        "favorite": v.favorite,
        "watchlist": v.watchlist_at > 0,
    });
    if v.watchlist_at > 0 { o["watchlistedAt"] = json!(v.watchlist_at); }
    if let Some(y) = it.year { o["year"] = json!(y); }
    if let Some(d) = it.extra.get("originally_available").and_then(|v| v.as_str()).filter(|d| !d.is_empty()) { o["originallyAvailableAt"] = json!(d); }
    if let Some(r) = it.extra.get("ratings").and_then(|r| r.as_object()) {
        let scores: serde_json::Map<String, Value> = r.iter().filter_map(|(k, v)| v.get("score").and_then(|s| s.as_str()).map(|s| (k.clone(), json!(s)))).collect();
        if !scores.is_empty() { o["scores"] = Value::Object(scores); }
    }
    if let Some(l) = it.extra.get("loudness_lufs").filter(|v| v.is_number()) { o["loudness"] = l.clone(); }
    if it.extra.contains_key(crate::library::scanner::MISSING) { o["missing"] = json!(true); }
    if let Some(r) = it.rating { o["rating"] = json!(r); }
    if let Some(r) = v.rating { o["userRating"] = json!(r); }
    if !it.content_rating.is_empty() { o["contentRating"] = json!(it.content_rating); }
    if !it.studio.is_empty() { o["studio"] = json!(it.studio); }
    if it.kind == ItemKind::Movie {
        // {edition-Theatrical} in the file name (or set by hand), shown under the poster.
        let e = it.extra.get("edition").and_then(|v| v.as_str()).map(String::from)
            .unwrap_or_else(|| it.file.as_ref().map(|f| crate::library::query::edition_of(&f.path)).unwrap_or_default());
        if !e.is_empty() { o["editionTitle"] = json!(e); }
    }
    if let Some(i) = it.index { o["index"] = json!(i); }
    for (k, name) in [("logo", "clearLogo"), ("square", "squareThumb")] {
        if it.extra.get(k).and_then(|v| v.as_str()).is_some() { o[name] = json!(format!("/library/metadata/{}/{k}?v={}", it.id, it.updated_at)); }
    }
    if it.kind.playable() {
        o["viewOffset"] = json!(v.view_offset_ms);
        o["viewCount"] = json!(v.view_count);
        if v.last_viewed_at > 0 { o["lastViewedAt"] = json!(v.last_viewed_at); }
    } else {
        let leaves = match ix { Some(ix) => ix.leaves(db, it.id), None => db.leaves(it.id) };
        let seen = leaves.iter().filter(|l| db.view(user.id, **l).view_count > 0).count();
        o["leafCount"] = json!(leaves.len());
        o["viewedLeafCount"] = json!(seen);
        o["childCount"] = json!(match ix { Some(ix) => ix.kids.get(&it.id).map_or(0, |k| k.len()), None => db.children(it.id).len() });
        if it.kind == ItemKind::Album { o["releaseType"] = it.extra.get("release").cloned().unwrap_or(json!("album")); }
    }
    if let Some(p) = it.parent_id.and_then(|p| db.items.get(&p)) {
        o["parentRatingKey"] = json!(p.id.to_string());
        o["parentTitle"] = json!(p.title);
        if let Some(i) = p.index { o["parentIndex"] = json!(i); }
        if let Some(g) = p.parent_id.and_then(|g| db.items.get(&g)) {
            o["grandparentRatingKey"] = json!(g.id.to_string());
            o["grandparentTitle"] = json!(g.title);
            o["grandparentThumb"] = json!(format!("/library/metadata/{}/thumb?v={}", g.id, g.updated_at));
        }
    }
    // Where the end credits start (ms), for the player's "Skip credits".
    if detail && it.kind.playable() {
        let credits = db.credits_markers(it);
        if !credits.is_empty() { o["credits"] = json!(credits); }
        // Where the intro runs (ms), for the player's "Skip intro".
        if let Some((s, e)) = db.intro_marker(it) { o["intro"] = json!({"start": s, "end": e}); }
    }
    // The file, then (if merged by hand) the other versions of it: 4K first.
    {
        let mut versions: Vec<&Item> = match ix {
            Some(ix) => ix.versions.get(&it.id).into_iter().flatten().filter_map(|i| db.items.get(i)).collect(),
            None => db.items.values().filter(|x| x.version_of() == Some(it.id)).collect(),
        };
        versions.sort_by_key(|x| (std::cmp::Reverse(x.file.as_ref().and_then(|f| f.video()).map(|v| v.height).unwrap_or(0)), x.id));
        let mut list: Vec<Value> = vec![];
        for m in std::iter::once(it).chain(versions) {
            let Some(f) = &m.file else { continue };
        let vid = f.video();
            let aud = f.audio();
            let mut media = json!({
                "id": m.id,
                "container": f.container,
                "formatName": f.format_name,
                "size": f.size,
                "bitrate": f.bitrate / 1000,
                "videoCodec": vid.map(|v| v.codec.clone()).unwrap_or_default(),
                "width": vid.map(|v| v.width).unwrap_or(0),
                "height": vid.map(|v| v.height).unwrap_or(0),
                "bitDepth": vid.map(|v| v.bit_depth).unwrap_or(0),
                "audioCodec": aud.map(|a| a.codec.clone()).unwrap_or_default(),
                "audioChannels": aud.map(|a| a.channels).unwrap_or(0),
                "probed": f.probed,
                "deepAnalysisVersion": if crate::playback::deep::is_current(f) { f.deep_version } else { 0 },
                "requiredBandwidths": if crate::playback::deep::is_current(f) { f.required_kbps.iter().map(|k| k.to_string()).collect::<Vec<_>>().join(",") } else { String::new() },
                // A split movie has a Part per file; they play one after the other.
                "Part": m.all_files().iter().enumerate().map(|(n, p)| json!({
                    "key": if n == 0 { format!("/library/parts/{}/file", m.id) } else { format!("/library/parts/{}/file?part={n}", m.id) },
                    "part": n, "size": p.size, "container": p.container, "duration": p.duration_ms,
                })).collect::<Vec<_>>(),
            });
            if detail {
                media["streams"] = json!(f.streams);
                let mut subs: Vec<Value> = f
                    .streams
                    .iter()
                    .filter(|s| s.kind == "subtitle")
                    .enumerate()
                    .map(|(n, s)| json!({"key": format!("/library/parts/{}/subtitles/{}", m.id, n), "language": s.language, "codec": s.codec, "title": s.title, "forced": s.forced, "sdh": s.sdh, "embedded": true,
                        "offsetMs": crate::api::handlers::subtitle_timing(m, &n.to_string()).map(|t| t.offset_ms)}))
                    .collect();
                for (n, p) in f.external_subtitles.iter().enumerate() {
                    subs.push(json!({"key": format!("/library/parts/{}/subtitles/x{}", m.id, n), "file": Path::new(p).file_name().map(|s| s.to_string_lossy().to_string()), "embedded": false,
                        "offsetMs": crate::api::handlers::subtitle_timing(m, &format!("x{n}")).map(|t| t.offset_ms)}));
                }
                media["subtitles"] = json!(subs);
                if user.admin {
                    media["file"] = json!(f.path);
                }
            }
            media["edition"] = json!(m.extra.get("edition").and_then(|v| v.as_str()).map(String::from).unwrap_or_else(|| crate::library::query::edition_of(&f.path)));
            list.push(media);
            }
        if !list.is_empty() { o["Media"] = json!(list); }
    }
    // Trailers, featurettes, deleted scenes … of a movie.
    if detail && it.kind == ItemKind::Movie {
        let mut extras: Vec<&Item> = db.items.values().filter(|x| x.parent_id == Some(it.id) && x.is_extra()).collect();
        extras.sort_by(|a, b| (a.extra["extra_type"].as_str() != Some("trailer"), &a.sort_title).cmp(&(b.extra["extra_type"].as_str() != Some("trailer"), &b.sort_title)));
        if !extras.is_empty() {
            o["Extras"] = json!(extras.iter().map(|x| json!({
                "ratingKey": x.id.to_string(), "id": x.id, "type": "clip", "title": x.title,
                "extraType": x.extra["extra_type"], "duration": x.duration_ms,
                "thumb": format!("/library/metadata/{}/thumb?v={}", x.id, x.updated_at),
            })).collect::<Vec<_>>());
        }
    }
    if !it.extra.is_empty() {
        o["extra"] = Value::Object(it.extra.clone());
        if !db.settings.trailers_online { o["extra"].as_object_mut().map(|m| m.remove("trailers")); }
        // Each person on a detail page carries
        // how many items of the same library they appear in.
        if detail {
            if let Some(Value::Array(cast)) = o["extra"].get_mut("cast") {
                // People are counted by name.
                let by_name = |c: &Value| name_key(c["name"].as_str().unwrap_or(""));
                let keys: Vec<String> = cast.iter().map(by_name).collect();
                let mut counts: HashMap<&str, u64> = HashMap::new();
                for other in db.items.values().filter(|i| i.section_id == it.section_id && i.parent_id.is_none()) {
                    if let Some(Value::Array(oc)) = other.extra.get("cast") {
                        let mut seen: Vec<String> = vec![];
                        for c in oc {
                            let k = by_name(c);
                            if let Some(mine) = keys.iter().find(|x| **x == k) {
                                if !seen.contains(&k) {
                                    *counts.entry(mine.as_str()).or_default() += 1;
                                    seen.push(k);
                                }
                            }
                        }
                    }
                }
                for (c, k) in cast.iter_mut().zip(keys.iter()) {
                    c["count"] = json!(counts.get(k.as_str()).copied().unwrap_or(1));
                }
            }
        }
    }
    o
}


pub fn section_json(db: &Db, s: &Section) -> Value {
    let top = db.items.values().filter(|i| i.section_id == s.id && i.parent_id.is_none() && i.version_of().is_none()).count();
    let playable = db.items.values().filter(|i| i.section_id == s.id && i.kind.playable() && !i.hidden()).count();
    let trash = db.items.values().filter(|i| i.section_id == s.id && i.extra.contains_key(crate::library::scanner::MISSING)).count();
    json!({
        "key": s.id.to_string(), "id": s.id, "title": s.name, "type": s.kind.as_str(),
        "Location": s.paths.iter().map(|p| json!({"path": p})).collect::<Vec<_>>(),
        "paths": s.paths, "createdAt": s.created_at, "scannedAt": s.scanned_at, "creditsDetection": s.credits_detection, "introDetection": s.intro_detection, "language": s.language, "scanIntervalMinutes": s.scan_interval_minutes,
        "count": top, "videoCount": playable, "trash": trash,
    })
}


pub fn playlist_json(db: &Db, user: &User, p: &Playlist) -> (Value, usize, u64) {
    playlist_json_with(db, user, p, query::playlist_items(db, user, p))
}


/// `playlist_json` for items that were worked out already (a smart playlist runs over the whole library).
pub fn playlist_json_with(db: &Db, user: &User, p: &Playlist, items: Vec<u64>) -> (Value, usize, u64) {
    let duration: u64 = items.iter().filter_map(|i| db.items.get(i)).map(|i| i.duration_ms).sum();
    let thumb = items.first().map(|i| format!("/library/metadata/{i}/thumb"));
    // Up to four different pictures spread over the list, for the playlist's own picture.
    let mut thumbs: Vec<String> = vec![];
    let step = (items.len() / 4).max(1);
    for k in 0..items.len().min(4) {
        let top = items.get((k * step + (p.id as usize % step.max(1))).min(items.len() - 1)).and_then(|i| db.items.get(i));
        let top = top.map(|t| if t.kind == crate::data::db::ItemKind::Episode { t.parent_id.and_then(|s| db.items.get(&s)).and_then(|s| s.parent_id).and_then(|s| db.items.get(&s)).unwrap_or(t) } else { t });
        if let Some(t) = top { let u = format!("/library/metadata/{}/thumb?v={}", t.id, t.updated_at); if !thumbs.contains(&u) { thumbs.push(u); } }
    }
    let owner = if p.owner_id == 0 { "server".to_string() } else { db.user(p.owner_id).map(|u| u.username.clone()).unwrap_or_default() };
    let kind = if p.is_default() { "default" } else if p.kind == PlaylistKind::Smart { "smart" } else { "manual" };
    let v = json!({
        "ratingKey": p.id.to_string(), "id": p.id, "key": format!("/playlists/{}/items", p.id),
        "type": "playlist", "playlistType": if !items.is_empty() && items.iter().all(|i| db.items.get(i).is_some_and(|x| x.kind == crate::data::db::ItemKind::Track)) { "audio" } else { "video" }, "title": p.title, "summary": p.summary,
        "smart": p.kind == PlaylistKind::Smart, "kind": kind,
        "default": p.is_default(), "defaultKey": p.default_key, "query": p.query,
        "leafCount": items.len(), "duration": duration, "thumb": thumb, "composite": thumb, "thumbs": thumbs,
        "owner": owner, "ownerId": p.owner_id, "shared": p.shared, "pinned": p.pinned,
        "addedAt": p.created_at, "updatedAt": p.updated_at,
        "editable": p.owner_id == user.id || user.admin,
    });
    (v, items.len(), duration)
}


pub fn page<T: Clone>(ctx: &Ctx, list: &[T]) -> (Vec<T>, usize) {
    let off = ctx.num("offset").or(ctx.num("start")).unwrap_or(0) as usize;
    let lim = ctx.num("limit").or(ctx.num("size")).map(|l| l as usize).unwrap_or(usize::MAX);
    (list.iter().skip(off).take(lim).cloned().collect(), off)
}


pub fn user_json(u: &User) -> Value {
    json!({"id": u.id, "username": u.username, "email": u.email, "admin": u.admin, "created_at": u.created_at,
        "allowed_sections": u.allowed_sections, "allow_transcode": u.allow_transcode.unwrap_or(true), "allow_delete": u.allow_delete, "allow_download": u.admin || u.allow_download.unwrap_or(true), "invited_by": u.invited_by,
        "home": u.home, "has_pin": !u.pin_hash.is_empty(), "max_age": u.max_age, "block_unrated": u.block_unrated, "full_name": u.full_name, "display_name": u.display_name(), "two_factor": !u.totp_secret.is_empty()})
}


pub fn invite_json(db: &Db, i: &Invite, base: &str) -> Value {
    let status = if i.used_by.is_some() { "used" } else if i.expires_at < now() { "expired" } else { "open" };
    json!({"code": i.code, "url": format!("{base}/#/invite/{}", i.code), "email": i.email, "note": i.note, "created_at": i.created_at,
        "expires_at": i.expires_at, "allowed_sections": i.allowed_sections, "admin": i.admin, "status": status,
        "used_by": i.used_by.and_then(|u| db.user(u)).map(|u| u.username.clone()), "created_by": db.user(i.created_by).map(|u| u.username.clone())})
}


pub fn visible_playlist(p: &Playlist, user: &User) -> bool {
    p.owner_id == 0 || p.owner_id == user.id || p.shared
}


pub fn paths_from(ctx: &Ctx) -> Vec<String> {
    match ctx.body.get("paths") {
        Some(Value::Array(a)) => a.iter().filter_map(|p| p.as_str()).map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect(),
        Some(Value::String(s)) => s.split(['\n', ';']).map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect(),
        _ => ctx.params.iter().filter(|(k, _)| k == "location" || k == "path").map(|(_, v)| v.clone()).collect(),
    }
}


pub fn ids_from(ctx: &Ctx) -> Vec<u64> {
    let mut out = vec![];
    if let Some(Value::Array(a)) = ctx.body.get("ids") {
        out.extend(a.iter().filter_map(|v| v.as_u64().or_else(|| v.as_str().and_then(|s| s.parse().ok()))));
    }
    if let Some(s) = ctx.param("ids") {
        out.extend(s.split(',').filter_map(|x| x.trim().parse::<u64>().ok()));
    }
    if let Some(uri) = ctx.param("uri") {
        // as a list: .../library/metadata/12,13
        if let Some(tail) = uri.rsplit("/library/metadata/").next() {
            out.extend(tail.split(',').filter_map(|x| x.trim().parse::<u64>().ok()));
        }
    }
    out
}


pub fn with_item(app: &Arc<App>, user: &User, iid: &str, f: impl FnOnce(&Db, &Item) -> Resp) -> Resp {
    let db = app.db.lock().unwrap();
    match iid.parse::<u64>().ok().and_then(|i| db.items.get(&i)) {
        Some(it) if db.can_see_item(user, it) => f(&db, it),
        Some(_) => fail(403, "You don't have access to this library"),
        None => fail(404, "Not found"),
    }
}


/// One row of the home page; `ix` is built once for all the rows.
pub fn hub_ix(db: &Db, user: &User, key: &str, title: &str, ids: Vec<u64>, ix: &Index) -> Value {
    let list: Vec<Value> = ids.iter().filter_map(|i| db.items.get(i)).map(|i| item_json_ix(db, user, i, false, Some(ix))).collect();
    json!({"hubIdentifier": key, "title": title, "size": list.len(), "Metadata": list})
}


pub fn recently_added(db: &Db, user: &User, kind: Option<ItemKind>, limit: usize) -> Vec<u64> {
    let mut v: Vec<&Item> = db
        .items
        .values()
        .filter(|i| db.can_see_item(user, i))
        .filter(|i| match kind {
            Some(k) => i.kind == k,
            None => i.kind.playable(),
        } && !i.hidden())
        .collect();
    v.sort_by(|a, b| b.added_at.cmp(&a.added_at).then(b.id.cmp(&a.id)));
    v.into_iter().take(limit).map(|i| i.id).collect()
}


/// The shows the given episodes belong to, each once, in the order of their newest episode.
pub fn shows_of(db: &Db, ids: &[u64], limit: usize) -> Vec<u64> {
    let mut out: Vec<u64> = vec![];
    for id in ids {
        let mut cur = db.items.get(id);
        for _ in 0..4 {
            match cur {
                Some(i) if i.kind == ItemKind::Show => { if !out.contains(&i.id) { out.push(i.id); } break; }
                Some(i) => cur = i.parent_id.and_then(|p| db.items.get(&p)),
                None => break,
            }
        }
        if out.len() >= limit { break; }
    }
    out
}


/// One week in seconds, for the Continue Watching window:
/// last viewed + weeks × one week >= now.
pub const WEEK_SECONDS: u64 = 604800;


/// Next unwatched episode of each show the user has been watching.
pub fn on_deck(db: &Db, user: &User) -> Vec<u64> {
    let window = db.settings.continue_watching_weeks * WEEK_SECONDS;
    let mut shows: Vec<(u64, u64)> = vec![];
    // One pass over all items instead of one per show and season (db.leaves scans everything each time).
    let mut kids: HashMap<u64, Vec<u64>> = HashMap::new();
    for i in db.items.values() {
        if let Some(p) = i.parent_id { kids.entry(p).or_default().push(i.id); }
    }
    fn leaves_of(db: &Db, kids: &HashMap<u64, Vec<u64>>, id: u64, out: &mut Vec<u64>) {
        let Some(item) = db.items.get(&id) else { return };
        if item.kind.playable() { out.push(id); return; }
        for c in kids.get(&id).into_iter().flatten() { leaves_of(db, kids, *c, out); }
    }
    for show in db.items.values().filter(|i| i.kind == ItemKind::Show && db.can_see_item(user, i)) {
        let mut eps = vec![];
        leaves_of(db, &kids, show.id, &mut eps);
        query::sort_items(db, user.id, &mut eps, "episode_order");
        let Some(pos) = eps.iter().rposition(|e| db.view(user.id, *e).view_count > 0) else { continue };
        let when = db.view(user.id, eps[pos]).last_viewed_at;
        let Some(next) = eps.iter().skip(pos + 1).find(|e| db.view(user.id, **e).view_count == 0) else { continue };
        // Season premieres: a second
        // pass without the time window for the shows returns, i.e. shows
        // with an episode 1 of a season other than season 1 that was ADDED to the
        // library inside the window (index=1, parents.index<>1, added_at > cutoff).
        let cutoff = now().saturating_sub(window);
        let premiere = db.settings.continue_watching_include_premieres
            && eps.iter().filter_map(|e| db.items.get(e)).any(|e| {
                e.index == Some(1)
                    && e.added_at > cutoff
                    && e.parent_id.and_then(|s| db.items.get(&s)).and_then(|s| s.index) != Some(1)
            });
        if now().saturating_sub(when) > window && !premiere {
            continue;
        }
        shows.push((*next, when));
    }
    shows.sort_by(|a, b| b.1.cmp(&a.1));
    shows.into_iter().map(|s| s.0).collect()
}


/// Started-but-unfinished items plus the next episode of shows in progress,
/// newest first, limited by the window in weeks and a count.
pub fn continue_watching(db: &Db, user: &User) -> Vec<u64> {
    let window = db.settings.continue_watching_weeks * WEEK_SECONDS;
    // Only this person's watch states ("user:item" keys), not every item in the library.
    let t_now = now();
    let mut started: Vec<(u64, u64)> = db
        .views
        .range(format!("{}:", user.id)..format!("{};", user.id))
        .filter(|(_, v)| v.view_offset_ms > 0 && t_now.saturating_sub(v.last_viewed_at) <= window)
        .filter_map(|(k, v)| {
            let id = k.split_once(':')?.1.parse::<u64>().ok()?;
            let i = db.items.get(&id)?;
            (i.kind.playable() && !i.hidden() && db.can_see_item(user, i)).then_some((id, v.last_viewed_at))
        })
        .collect();
    for id in on_deck(db, user) {
        if !started.iter().any(|s| s.0 == id) {
            let when = db
                .items
                .get(&id)
                .and_then(|e| e.parent_id)
                .and_then(|season| db.items.get(&season))
                .and_then(|s| s.parent_id)
                .map(|show| db.leaves(show).iter().map(|l| db.view(user.id, *l).last_viewed_at).max().unwrap_or(0))
                .unwrap_or(0);
            started.push((id, when));
        }
    }
    // Taken off by hand: stays off until the person watches (the show or the title) again.
    started.retain(|(id, when)| { let h = db.view(user.id, *id).cw_hidden_at; h == 0 || *when > h });
    started.sort_by(|a, b| b.1.cmp(&a.1));
    started.truncate(db.settings.continue_watching_limit.max(1));
    started.into_iter().map(|s| s.0).collect()
}


/// A gradient poster with the title, for items without a picture.
pub fn placeholder_svg(title: &str, kind: &str, id: u64) -> String {
    let hue = (id.wrapping_mul(47) % 360) as u32;
    let esc = |s: &str| s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;");
    let mut lines = vec![String::new()];
    for w in title.split_whitespace() {
        if lines.last().unwrap().len() + w.len() > 14 && !lines.last().unwrap().is_empty() {
            lines.push(String::new());
        }
        let l = lines.last_mut().unwrap();
        if !l.is_empty() {
            l.push(' ');
        }
        l.push_str(w);
    }
    lines.truncate(4);
    let text: String = lines
        .iter()
        .enumerate()
        .map(|(i, l)| format!("<text x='100' y='{}' text-anchor='middle' font-family='sans-serif' font-size='19' font-weight='bold' fill='#fff'>{}</text>", 130 + i as i32 * 24, esc(l)))
        .collect();
    format!(
        "<svg xmlns='http://www.w3.org/2000/svg' width='200' height='300' viewBox='0 0 200 300'><defs><linearGradient id='g' x1='0' y1='0' x2='1' y2='1'><stop offset='0' stop-color='hsl({hue},45%,38%)'/><stop offset='1' stop-color='hsl({},50%,14%)'/></linearGradient></defs><rect width='200' height='300' fill='url(#g)'/>{text}<text x='100' y='280' text-anchor='middle' font-family='sans-serif' font-size='12' fill='rgba(255,255,255,.6)'>{}</text></svg>",
        (hue + 40) % 360,
        kind.to_uppercase()
    )
}

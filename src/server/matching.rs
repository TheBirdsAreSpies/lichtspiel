//! Matching titles with an online details source: looking them up, applying the details, undoing a match,
//! and the pictures that come with them.

use super::*;

impl App {
    /// Where a person's shared photo is kept.
    pub fn person_photo(&self, key: &str) -> PathBuf {
        self.data_dir.join("metadata").join("people").join(format!("{key}.jpg"))
    }


    /// Deletes saved posters, backgrounds, stills and actor photos that aren't
    /// real photos. Returns the movies and shows they belonged to.
    pub fn remove_broken_pictures(&self) -> Vec<u64> {
        // Only our own saved pictures: files in the user's media folders are never touched.
        let bad = |p: &str| Path::new(p).starts_with(&self.data_dir) && Path::new(p).is_file() && crate::metadata::file_image_type(Path::new(p)).is_none();
        let mut tops = HashSet::new();
        let mut db = self.db.lock().unwrap();
        let ids: Vec<u64> = db.items.values().filter(|it| it.poster.as_deref().is_some_and(bad) || it.extra.get("art").and_then(|v| v.as_str()).is_some_and(bad)).map(|it| it.id).collect();
        for id in ids {
            let top = Self::top_level(&db, id).unwrap_or(id);
            let Some(it) = db.items.get_mut(&id) else { continue };
            if let Some(p) = it.poster.clone().filter(|p| bad(p)) {
                let _ = std::fs::remove_file(&p);
                it.poster = None;
                tops.insert(top);
            }
            if let Some(p) = it.extra.get("art").and_then(|v| v.as_str()).map(String::from).filter(|p| bad(p)) {
                let _ = std::fs::remove_file(&p);
                it.extra.remove("art");
                tops.insert(top);
            }
        }
        drop(db);
        if let Ok(rd) = std::fs::read_dir(self.data_dir.join("metadata")) {
            for e in rd.flatten() {
                let Ok(files) = std::fs::read_dir(e.path().join("cast")) else { continue };
                for f in files.flatten() {
                    if bad(&f.path().to_string_lossy()) {
                        let _ = std::fs::remove_file(f.path());
                        if let Some(id) = e.file_name().to_str().and_then(|s| s.parse::<u64>().ok()) {
                            tops.insert(id);
                        }
                    }
                }
            }
        }
        let mut v: Vec<u64> = tops.into_iter().collect();
        v.sort();
        if !v.is_empty() {
            self.save();
            for id in &v {
                let _ = std::fs::remove_file(self.data_dir.join("cache").join("thumbs").join(format!("{id}.jpg")));
            }
        }
        v
    }


    /// Asks metadata plugins about new items and merges their answers.
    pub fn lookup_metadata(self: &Arc<Self>, ids: &[u64]) {
        let listeners = self.listeners("metadata.lookup");
        if listeners.is_empty() {
            return;
        }
        for id in ids {
            let data = {
                let db = self.db.lock().unwrap();
                let Some(it) = db.items.get(id) else { continue };
                let mut v = serde_json::to_value(it).unwrap_or(Value::Null);
                // The library's name, so a plugin can be meant for some libraries only (e.g. "Anime").
                if let Some(s) = db.sections.iter().find(|s| s.id == it.section_id) {
                    v["section_name"] = json!(s.name);
                }
                if let Some(parent) = it.parent_id.and_then(|p| db.items.get(&p)) {
                    v["parent_title"] = json!(parent.title);
                    if let Some(gp) = parent.parent_id.and_then(|g| db.items.get(&g)) {
                        v["show_title"] = json!(gp.title);
                    }
                }
                v
            };
            for (pl, settings) in &listeners {
                match plugins::invoke(pl, settings, "metadata.lookup", &data, &self.server_info_for(pl)) {
                    Ok(answer) => {
                        if let Some(meta) = answer.get("metadata") {
                            let mut db = self.db.lock().unwrap();
                            if let Some(it) = db.items.get_mut(id) {
                                apply_metadata(it, meta, false);
                            }
                        }
                        self.plugins.lock().unwrap().note(&pl.manifest.id, "metadata.lookup", true, format!("item {id}"));
                    }
                    Err(e) => self.plugins.lock().unwrap().note(&pl.manifest.id, "metadata.lookup", false, e),
                }
            }
        }
    }


    /// An id tag ("{imdb-tt0068646}", "{tmdb-238}", "{tvdb-81189}") in the names
    /// below the library folder: for a movie its file name and folders, for a show
    /// its own folder (an episode's file name may carry the episode's id instead).
    pub fn id_tag_of(db: &Db, it: &Item) -> Option<(&'static str, String)> {
        let path = match it.kind {
            ItemKind::Movie => it.file.as_ref()?.path.clone(),
            ItemKind::Show => db.leaves(it.id).iter().filter_map(|l| db.items.get(l)?.file.as_ref().map(|f| f.path.clone())).next()?,
            _ => return None,
        };
        let path = Path::new(&path);
        let section = db.sections.iter().find(|s| s.id == it.section_id)?;
        let rel = section.paths.iter().find_map(|r| path.strip_prefix(r).ok()).unwrap_or(path);
        let names: Vec<String> = rel.components().map(|c| c.as_os_str().to_string_lossy().to_string()).collect();
        let names: Vec<&String> = match it.kind {
            ItemKind::Show if names.len() >= 2 => vec![&names[0]],
            ItemKind::Show => vec![],
            _ => names.iter().rev().collect(),
        };
        // Without a tag in the names, the match Plex made (kept by the import) decides.
        names.into_iter().find_map(|n| scanner::id_tag(n)).or_else(|| {
            let p = it.extra.get("plex_id")?;
            let src = match p["source"].as_str()? { "imdb" => "imdb", "tmdb" => "tmdb", "tvdb" => "tvdb", _ => return None };
            Some((src, p["id"].as_str()?.to_string()))
        })
    }


    /// Title and year from the folder a movie's file is in, when that folder holds
    /// only this movie (not the library folder itself, not a folder of many movies).
    pub fn folder_title(db: &Db, it: &Item) -> Option<(String, Option<u32>)> {
        if it.kind != ItemKind::Movie {
            return None;
        }
        let path = Path::new(&it.file.as_ref()?.path);
        let section = db.sections.iter().find(|s| s.id == it.section_id)?;
        let is_root = |d: &Path| section.paths.iter().any(|r| scanner_same_dir(Path::new(r), d));
        let mut dir = path.parent()?;
        if is_root(dir) {
            return None;
        }
        // Parts of the same movie ("… - pt1", "… - pt2") do not count as other movies.
        let base = |p: &Path| p.file_stem().map(|s| scanner::split_part(&s.to_string_lossy()).0.to_lowercase()).unwrap_or_default();
        let mine = base(path);
        let others = db.items.values().filter(|o| o.id != it.id && o.kind == ItemKind::Movie)
            .filter_map(|o| o.file.as_ref().map(|f| Path::new(&f.path)))
            .filter(|p| p.parent() == Some(dir) && base(p) != mine).count();
        if others > 0 {
            return None;
        }
        // A folder that says nothing about the movie ("CD1", "Disc 2", or named like the
        // file itself: "Der Pate (1972)/funxd/funxd.mkv"): its parent's name is the title.
        let file_title = scanner::parse_title_year(&mine).0.to_lowercase();
        for _ in 0..2 {
            let name = dir.file_name()?.to_string_lossy().to_string();
            let (t, _) = scanner::parse_title_year(&name);
            let generic = t.trim().is_empty() || t.to_lowercase() == file_title || is_disc_folder(&name);
            match dir.parent() {
                Some(up) if generic && !is_root(up) && up.file_name().is_some() => dir = up,
                _ => break,
            }
        }
        let (title, year) = scanner::parse_title_year(&dir.file_name()?.to_string_lossy());
        Some((title, year)).filter(|(t, _)| !t.trim().is_empty() && t.to_lowercase() != file_title)
    }


    /// The movie, show or other video an item belongs to (itself for movies and shows).
    pub fn top_level(db: &Db, id: u64) -> Option<u64> {
        let mut cur = db.items.get(&id)?;
        while let Some(p) = cur.parent_id.and_then(|p| db.items.get(&p)) {
            cur = p;
        }
        Some(cur.id)
    }


    /// Puts movies and shows (found from any of their items) in line for online details.
    pub fn queue_online_details(&self, ids: &[u64], force: bool) {
        let tops: Vec<u64> = {
            let db = self.db.lock().unwrap();
            if !db.settings.metadata_enabled && !force {
                return;
            }
            let mut t: Vec<u64> = ids.iter().filter_map(|i| Self::top_level(&db, *i)).collect();
            t.sort();
            t.dedup();
            t.retain(|i| db.items.get(i).is_some_and(|it| matches!(it.kind, ItemKind::Movie | ItemKind::Show)));
            t
        };
        let mut q = self.meta_queue.lock().unwrap();
        let before = q.len();
        for id in tops {
            if !q.iter().any(|(i, _)| *i == id) {
                q.push_back((id, force));
            }
        }
        let added = q.len() > before;
        drop(q);
        if added {
            self.yield_to_details();
        }
    }


    /// Details come before credits and intros: an automatic detection that is running steps aside after
    /// the file it is on and starts again once no details are waiting (the scheduler does that).
    pub fn yield_to_details(&self) {
        if !self.job_auto.load(AtomicOrdering::SeqCst) {
            return;
        }
        let detecting = self.job.lock().unwrap().as_ref().is_some_and(|j| j.0 == "Detecting credits" || j.0 == "Detecting intros");
        if detecting {
            self.job_yield.store(true, AtomicOrdering::SeqCst);
            self.job_cancel.store(true, AtomicOrdering::SeqCst);
        }
    }


    /// Queues every matched movie and show in the libraries `which` picks for a
    /// fresh download (after a language change). Returns how many were queued.
    pub fn refetch_details(&self, which: impl Fn(&Section) -> bool) -> usize {
        let ids: Vec<u64> = {
            let db = self.db.lock().unwrap();
            let secs: Vec<u64> = db.sections.iter().filter(|s| which(s)).map(|s| s.id).collect();
            db.items
                .values()
                .filter(|i| secs.contains(&i.section_id) && matches!(i.kind, ItemKind::Movie | ItemKind::Show))
                .filter(|i| i.extra.get("metadata_status").and_then(|v| v.as_str()) == Some("matched"))
                .map(|i| i.id)
                .collect()
        };
        self.queue_online_details(&ids, true);
        ids.len()
    }


    pub fn metadata_dir(&self, id: u64) -> PathBuf {
        self.data_dir.join("metadata").join(id.to_string())
    }


    /// Finds and applies online details for a movie or show, its seasons and
    /// episodes. `choice` = (source, id) from "Fix match"; otherwise the stored
    /// match is refreshed, or the best confident search result is taken.
    pub fn match_online(self: &Arc<Self>, id: u64, choice: Option<(String, String)>, force: bool) -> Result<String, String> {
        use crate::metadata;
        let mut settings = self.settings();
        let (kind, title, year, stored, seasons, status_recent, folder, tag, unmatched_by_hand) = {
            let db = self.db.lock().unwrap();
            let it = db.items.get(&id).ok_or("No such item")?;
            // The library's own language, else the server's.
            settings.metadata_language = db.language_for_section(it.section_id);
            // A language chosen for this title wins.
            if let Some(l) = it.extra.get("language_override").and_then(|v| v.as_str()).filter(|l| db::LANGUAGES.iter().any(|x| x.0 == *l)) {
                settings.metadata_language = l.to_string();
            }
            if !matches!(it.kind, ItemKind::Movie | ItemKind::Show) {
                return Err("Details are looked up for movies and shows".into());
            }
            let s = |k: &str| it.extra.get(k).and_then(|v| v.as_str()).map(String::from);
            let stored = s("metadata_source").zip(s("metadata_id"));
            let seasons: Vec<u32> = db.children(id).iter().filter_map(|c| c.index).collect();
            // A miss from before the folder name was tried counts as not checked yet.
            let recent = s("metadata_status").as_deref() == Some("unmatched")
                && it.extra.get("match_version").and_then(|v| v.as_u64()) == Some(MATCH_VERSION)
                && it.extra.get("metadata_checked_at").and_then(|v| v.as_u64()).is_some_and(|t| now().saturating_sub(t) < 7 * 86400);
            // Use the name from the file, not a name a previous wrong match gave it.
            let title = s("file_title").unwrap_or_else(|| it.title.clone());
            let folder = Self::folder_title(&db, it).filter(|(t, _)| !t.eq_ignore_ascii_case(&title));
            let tag = Self::id_tag_of(&db, it);
            let by_hand = it.extra.get("unmatched_by_hand").and_then(|v| v.as_bool()) == Some(true);
            (it.kind, title, it.year, stored, seasons, recent, folder, tag, by_hand)
        };
        // An id in the file or folder name ("{imdb-tt0068646}") decides the match,
        // unless one is picked by hand in "Fix match".
        let tagged = match (&choice, &tag) {
            (None, Some((src, tid))) => match metadata::resolve_id(&settings, kind, src, tid) {
                Ok(x) => Some(x),
                Err(e) => {
                    elog!("details for item {id}: {{{src}-{tid}}}: {e}");
                    None
                }
            },
            _ => None,
        };
        if choice.is_none() && unmatched_by_hand {
            // Only "Refresh details" asked for by hand looks for a match again.
            if !force { return Err("The match was removed by hand; use Match… to pick one".into()); }
            if let Some(it) = self.db.lock().unwrap().items.get_mut(&id) { it.extra.remove("unmatched_by_hand"); }
        }
        if choice.is_none() && tagged.is_none() && stored.is_none() && status_recent && !force {
            return Err("No match was found recently; use Fix match".into());
        }
        // A match made before a TMDB key was entered (Wikidata, TVmaze) is moved to TMDB when it finds the
        // title for sure: those matches often have no poster, and a refresh would keep them for ever.
        let stored = match stored {
            Some((ref src, _)) if choice.is_none() && tagged.is_none() && src != "tmdb" && !settings.tmdb_api_key.trim().is_empty() => {
                let better = metadata::search(&settings, kind, &title, year).ok()
                    .and_then(|c| c.into_iter().next()).filter(|c| c.source == "tmdb" && metadata::acceptable(c));
                better.map(|c| (c.source, c.id)).or(stored)
            }
            other => other,
        };
        let (had_choice, had_tag, had_stored) = (choice.is_some(), tagged.is_some(), stored.is_some());
        let (source, mid) = match choice.or(tagged).or(stored) {
            Some(x) => x,
            None => {
                // Files are sometimes named after the release ("funxd.mkv"): then the
                // name of the movie's own folder ("Der Pate (1972)") is tried too, and
                // first when only the folder has a year.
                let pick = |r: &Result<Vec<metadata::Candidate>, String>| r.as_ref().ok().and_then(|c| c.first().cloned()).filter(metadata::acceptable);
                let folder_first = year.is_none() && folder.as_ref().is_some_and(|f| f.1.is_some());
                let by_folder = || folder.as_ref().map(|(ft, fy)| metadata::search(&settings, kind, ft, fy.or(year)));
                let mut found = None;
                let mut best = None;
                // A search that got no answer (no internet) is not a miss.
                let mut folder_failed = false;
                let mut by_folder_pick = || { let r = by_folder(); folder_failed |= matches!(r, Some(Err(_))); r.as_ref().and_then(pick) };
                if folder_first {
                    best = by_folder_pick();
                }
                if best.is_none() {
                    let r = metadata::search(&settings, kind, &title, year);
                    best = pick(&r);
                    found = Some(r);
                }
                if best.is_none() && !folder_first {
                    best = by_folder_pick();
                }
                let found = found.unwrap_or_else(|| Ok(vec![]));
                let tried = match &folder {
                    Some((ft, fy)) => format!("\"{title}\" or \"{ft}{}\"", fy.map(|y| format!(" ({y})")).unwrap_or_default()),
                    None => format!("\"{title}\""),
                };
                match best {
                    Some(b) => (b.source, b.id),
                    None => {
                        let mut db = self.db.lock().unwrap();
                        if let Some(it) = db.items.get_mut(&id).filter(|_| found.is_ok() && !folder_failed) {
                            it.extra.insert("metadata_status".into(), json!("unmatched"));
                            it.extra.insert("metadata_checked_at".into(), json!(now()));
                            it.extra.insert("match_version".into(), json!(MATCH_VERSION));
                        }
                        return Err(match found {
                            Err(e) => e,
                            Ok(c) if c.is_empty() => format!("Nothing found online for {tried}"),
                            Ok(c) => format!("No sure match for {tried} (closest: {} {})", c[0].title, c[0].year.map(|y| format!("({y})")).unwrap_or_default()),
                        });
                    }
                }
            }
        };
        let mut d = match metadata::fetch(&settings, kind, &source, &mid, &seasons) {
            Ok(d) => d,
            // The online entry is not there (any more, or it is of the other kind).
            Err(e) if e.ends_with("HTTP 404") => {
                if !had_choice && !had_tag && had_stored {
                    // The match kept from before is stale: forget it and look the title up again.
                    elog!("details for item {id}: {source}-{mid} is gone online; looking the title up again");
                    if let Some(it) = self.db.lock().unwrap().items.get_mut(&id) {
                        it.extra.remove("metadata_source");
                        it.extra.remove("metadata_id");
                    }
                    return self.match_online(id, None, force);
                }
                return Err(format!("{source}-{mid} was not found online (HTTP 404)"));
            }
            Err(e) => return Err(e),
        };
        // What the settings leave out (Settings → Metadata).
        if !settings.meta_cast {
            for k in ["cast", "directors", "writers", "producers", "creators"] { d.meta["extra"][k] = json!([]); }
        }
        if !settings.meta_artwork {
            d.poster_url = None;
            d.art_url = None;
            for s in d.seasons.iter_mut() { s.poster_url = None; }
            for e in d.episodes.iter_mut() { e.still_url = None; }
        }
        if !settings.meta_episodes {
            d.seasons.clear();
            d.episodes.clear();
        }
        if !settings.meta_ratings {
            if let Some(o) = d.meta.as_object_mut() { o.remove("rating"); o.remove("audienceRating"); }
            if let Some(o) = d.meta["extra"].as_object_mut() { o.remove("ratings"); }
        }
        // Pictures (downloaded without holding the library lock).
        let dir = self.metadata_dir(id);
        let got = |url: &Option<String>, name: &str, dir: &Path| -> Option<String> {
            let dest = dir.join(name);
            url.as_ref().filter(|u| metadata::download(u, &dest)).map(|_| dest.to_string_lossy().to_string())
        };
        let poster = got(&d.poster_url, "poster.jpg", &dir);
        let art = got(&d.art_url, "art.jpg", &dir);
        // Actor photos are kept on this server too (cast/0.jpg …), served at
        // /library/metadata/{id}/cast/{n}, so pages don't depend on other sites.
        // Actors are shared by the whole library: one photo per person
        // (metadata/people/<key>.jpg), downloaded once, whatever they play in.
        let mut cast = d.meta["extra"]["cast"].as_array().cloned().unwrap_or_default();
        let _ = std::fs::remove_dir_all(dir.join("cast"));
        for c in cast.iter_mut().take(settings.cast_photos_limit) {
            let key = person_key(c);
            c["key"] = json!(key);
            let dest = self.person_photo(&key);
            let have = metadata::file_image_type(&dest).is_some();
            let url = c["photo"].as_str().map(String::from);
            if have || url.is_some_and(|u| metadata::download(&u, &dest)) {
                c["thumb"] = json!(format!("/library/people/{key}/thumb"));
            }
        }
        for c in cast.iter_mut().skip(settings.cast_photos_limit) {
            c["key"] = json!(person_key(c));
        }
        let (season_items, episode_items) = {
            let db = self.db.lock().unwrap();
            let seasons: Vec<(u64, u32)> = db.children(id).iter().filter_map(|c| c.index.map(|n| (c.id, n))).collect();
            let mut eps = vec![];
            for (sid, n) in &seasons {
                for e in db.children(*sid) {
                    if let Some(i) = e.index {
                        eps.push((e.id, *n, i));
                    }
                }
            }
            (seasons, eps)
        };
        let mut season_pics = HashMap::new();
        for (sid, n) in &season_items {
            if let Some(s) = d.seasons.iter().find(|s| s.index == *n) {
                if let Some(p) = got(&s.poster_url, "poster.jpg", &self.metadata_dir(*sid)) {
                    season_pics.insert(*sid, p);
                }
            }
        }
        let mut episode_pics = HashMap::new();
        for (eid, sn, en) in &episode_items {
            if let Some(e) = d.episodes.iter().find(|e| e.season == *sn && e.episode == *en) {
                let dest = self.metadata_dir(*eid).join("still.jpg");
                if dest.exists() {
                    episode_pics.insert(*eid, dest.to_string_lossy().to_string());
                } else if let Some(p) = got(&e.still_url, "still.jpg", &self.metadata_dir(*eid)) {
                    episode_pics.insert(*eid, p);
                }
            }
        }
        let mut db = self.db.lock().unwrap();
        let Some(it) = db.items.get_mut(&id) else { return Err("The item went away".into()) };
        // People brought over from the old server stay when the online source has none.
        let kept: Vec<(&str, Value)> = if it.extra.contains_key("imported_from_old_server") {
            ["cast", "directors", "writers", "producers", "countries", "collections"].into_iter().filter(|k| d.meta["extra"][*k].as_array().is_none_or(|a| a.is_empty())).filter_map(|k| it.extra.get(k).cloned().map(|v| (k, v))).collect()
        } else {
            vec![]
        };
        // Forget what an earlier match said before applying this one.
        for k in ["cast", "directors", "writers", "producers", "countries", "creators", "ids", "art", "original_title", "runtime_minutes", "originally_available", "metadata_status", "metadata_checked_at", "ratings", "imdb_ratings_checked", "unmatched_by_hand", "trailers"] {
            if it.locked.iter().any(|l| l == k) { continue; }
            it.extra.remove(k);
        }
        if !it.extra.contains_key("file_title") {
            it.extra.insert("file_title".into(), json!(it.title));
        }
        let mut meta = d.meta.clone();
        if !cast.is_empty() {
            meta["extra"]["cast"] = json!(cast);
        }
        for (k, v) in kept {
            meta["extra"][k] = v;
        }
        if let Some(p) = &poster {
            meta["poster"] = json!(p);
        }
        meta["extra"]["metadata_status"] = json!("matched");
        meta["extra"]["pictures_version"] = json!(PICTURES_VERSION);
        meta["extra"]["matched_at"] = json!(now());
        if let Some(a) = &art {
            meta["extra"]["art"] = json!(a);
        }
        apply_metadata(it, &meta, false);
        use_original_title(it);
        let new_title = it.title.clone();
        let _ = std::fs::remove_file(self.data_dir.join("cache").join("thumbs").join(format!("{id}.jpg")));
        if art.is_some() {
            let _ = std::fs::remove_file(self.data_dir.join("cache").join("art").join(format!("{id}.jpg")));
        }
        let mut n_eps = 0;
        for (sid, n) in &season_items {
            if let (Some(s), Some(item)) = (d.seasons.iter().find(|s| s.index == *n), db.items.get_mut(sid)) {
                let mut m = json!({"summary": s.summary});
                if let Some(p) = season_pics.get(sid) {
                    m["poster"] = json!(p);
                }
                apply_metadata(item, &m, false);
            }
        }
        for (eid, sn, en) in &episode_items {
            let Some(e) = d.episodes.iter().find(|e| e.season == *sn && e.episode == *en) else { continue };
            let Some(item) = db.items.get_mut(eid) else { continue };
            if !item.extra.contains_key("file_title") {
                item.extra.insert("file_title".into(), json!(item.title));
            }
            let mut m = json!({"summary": e.summary, "extra": {"originally_available": e.aired}});
            if !e.title.is_empty() {
                m["title"] = json!(e.title);
            }
            if let Some(r) = e.rating {
                m["rating"] = json!(r);
            }
            if let Some(p) = episode_pics.get(eid) {
                m["poster"] = json!(p);
            }
            apply_metadata(item, &m, false);
            n_eps += 1;
        }
        drop(db);
        Ok(if kind == ItemKind::Show {
            format!("{new_title}: details from {source}, {n_eps} episodes")
        } else {
            format!("{new_title}: details from {source}")
        })
    }


    /// "Unmatch": forgets the online details of a movie or show (and of its
    /// seasons and episodes) and goes back to what the files say. Fields edited
    /// by hand stay. Automatic matching leaves it alone until "Match…" is used.
    pub fn unmatch(&self, id: u64) -> Result<String, String> {
        let mut db = self.db.lock().unwrap();
        let it = db.items.get(&id).ok_or("No such item")?;
        if !matches!(it.kind, ItemKind::Movie | ItemKind::Show) {
            return Err("Details are looked up for movies and shows".into());
        }
        let kind = it.kind;
        // Title and year as a scan of the files gives them.
        let leaf = if kind == ItemKind::Movie { Some(id) } else { db.leaves(id).first().copied() };
        let file = leaf.and_then(|l| db.items.get(&l)).and_then(|l| l.file.as_ref()).map(|f| PathBuf::from(&f.path));
        let section = db.sections.iter().find(|s| s.id == it.section_id).cloned();
        let root = match (&file, &section) {
            (Some(f), Some(sec)) => sec.paths.iter().map(PathBuf::from).filter(|r| f.starts_with(r)).max_by_key(|r| r.as_os_str().len()),
            _ => None,
        };
        let parsed = match (&file, &section, &root) {
            (Some(f), Some(sec), Some(r)) => Some(scanner::classify(sec, r, f)),
            _ => None,
        };
        let (scan_title, year) = match parsed {
            Some(p) if kind == ItemKind::Show => p.show.unwrap_or((String::new(), None)),
            Some(p) => (p.title, p.year),
            None => (String::new(), it.year),
        };
        let title = it.extra.get("file_title").and_then(|v| v.as_str()).map(String::from).filter(|t| !t.trim().is_empty())
            .or(Some(scan_title).filter(|t| !t.trim().is_empty()))
            .unwrap_or_else(|| it.title.clone());
        let local_poster = |db: &Db, i: u64| -> Option<String> {
            let f = PathBuf::from(&db.items.get(&i)?.file.as_ref()?.path);
            scanner::find_poster(f.parent()?, &f.file_stem()?.to_string_lossy())
        };
        let own_poster = if kind == ItemKind::Movie {
            local_poster(&db, id)
        } else {
            // A show's picture lies in the show's own folder.
            file.as_ref().zip(root.as_ref()).and_then(|(f, r)| {
                let first = f.strip_prefix(r).ok()?.components().next()?;
                scanner::find_poster(&r.join(first.as_os_str()), "__none__")
            })
        };
        let mut ids = vec![id];
        for s in db.children(id).iter().map(|c| c.id).collect::<Vec<_>>() {
            ids.push(s);
            ids.extend(db.children(s).iter().map(|e| e.id));
        }
        let posters: HashMap<u64, Option<String>> =
            ids.iter().map(|i| (*i, if *i == id { own_poster.clone() } else { local_poster(&db, *i) })).collect();
        for i in &ids {
            let dir = self.metadata_dir(*i);
            let Some(it) = db.items.get_mut(i) else { continue };
            let locked = it.locked.clone();
            let can = |f: &str| !locked.iter().any(|l| l == f);
            if *i == id {
                if can("title") {
                    it.title = title.clone();
                    it.sort_title = db::sort_title(&title);
                }
                if can("year") {
                    it.year = year;
                }
                if can("tagline") { it.tagline.clear(); }
                if can("content_rating") { it.content_rating.clear(); }
                if can("studio") { it.studio.clear(); }
                if can("genres") { it.genres.clear(); }
                for k in ["collections", "cast", "directors", "writers", "producers", "countries", "creators", "ids", "art", "original_title", "runtime_minutes",
                    "originally_available", "ratings", "imdb_ratings_checked", "trailers", "metadata_source", "metadata_id", "metadata_language", "matched_at",
                    "pictures_version", "metadata_checked_at", "match_version"] {
                    if it.locked.iter().any(|l| l == k) { continue; }
                    it.extra.remove(k);
                }
                it.extra.insert("metadata_status".into(), json!("unmatched"));
                it.extra.insert("unmatched_by_hand".into(), json!(true));
            } else if it.kind == ItemKind::Episode {
                if can("title") {
                    if let Some(t) = it.extra.get("file_title").and_then(|v| v.as_str()).map(String::from) {
                        it.sort_title = db::sort_title(&t);
                        it.title = t;
                    }
                }
                it.extra.remove("originally_available");
            }
            if can("summary") { it.summary.clear(); }
            if can("rating") { it.rating = None; }
            it.updated_at = now();
            // Pictures that came with the match go; ones next to the files come back.
            if it.poster.as_deref().is_none_or(|p| Path::new(p).starts_with(&dir)) {
                it.poster = posters.get(i).cloned().flatten();
            }
            let _ = std::fs::remove_dir_all(&dir);
            let _ = std::fs::remove_file(self.data_dir.join("cache").join("thumbs").join(format!("{i}.jpg")));
            let _ = std::fs::remove_file(self.data_dir.join("cache").join("art").join(format!("{i}.jpg")));
        }
        drop(db);
        Ok(format!("{title}: match removed"))
    }


    /// Works through the queue, one item at a time, gently.
    pub fn metadata_worker(self: &Arc<Self>) {
        loop {
            let next = self.meta_queue.lock().unwrap().pop_front();
            let Some((id, force)) = next else {
                std::thread::sleep(Duration::from_secs(2));
                continue;
            };
            if !self.settings().metadata_enabled && !force {
                continue;
            }
            // Another worker is on this title right now (both would write the same poster): it waits its turn.
            {
                let mut busy = self.meta_busy.lock().unwrap();
                if busy.contains(&id) {
                    drop(busy);
                    self.meta_queue.lock().unwrap().push_back((id, force));
                    std::thread::sleep(Duration::from_millis(400));
                    continue;
                }
                busy.push(id);
            }
            let r = self.match_online(id, None, force);
            self.meta_busy.lock().unwrap().retain(|&b| b != id);
            {
                // What the web app shows while details are being updated.
                let title = self.db.lock().unwrap().items.get(&id).map(|i| i.title.clone()).unwrap_or_default();
                let mut p = self.meta_progress.lock().unwrap();
                p.done += 1;
                if r.is_err() {
                    p.failed += 1;
                }
                p.last = json!({"id": id, "title": title, "ok": r.is_ok(), "message": match &r { Ok(m) | Err(m) => m.clone() }, "at": now()});
            }
            if let Err(e) = &r {
                elog!("details for item {id}: {e}");
            }
            // Local information from plugins (e.g. .nfo files) wins over online details.
            let leaves = {
                let db = self.db.lock().unwrap();
                let mut v = vec![id];
                v.extend(db.children(id).iter().map(|c| c.id));
                v.extend(db.leaves(id));
                v
            };
            if r.is_ok() {
                self.lookup_metadata(&leaves);
                self.fire("library.item.updated", json!({"id": id}));
            } else {
                // Found nowhere online: a plugin may still know the title (e.g. an anime on AniList).
                self.lookup_metadata(&[id]);
            }
            self.save();
            std::thread::sleep(Duration::from_millis(400));
        }
    }

}

/// Copies fields from a plugin answer or an API edit into an item.
/// Fields edited by hand are locked; plugins don't overwrite them.
/// A short, safe name for a person: the details source's own person id when
/// there is one (tmdb-31, tvmaze-12, wd-Q2680), else one made from the name.
pub fn person_key(c: &Value) -> String {
    let id = c["person_id"].as_str().unwrap_or("");
    let ok = !id.is_empty() && !id.ends_with('-') && !id.ends_with("null") && id.len() <= 40 && id.chars().all(|ch| ch.is_ascii_alphanumeric() || ch == '-');
    if ok {
        return id.to_lowercase();
    }
    name_key(c["name"].as_str().unwrap_or(""))
}


/// A person key from a name alone (same name → same key).
pub fn name_key(name: &str) -> String {
    use sha2::{Digest, Sha256};
    let norm: String = name.to_lowercase().split_whitespace().collect::<Vec<_>>().join(" ");
    format!("n-{}", Sha256::digest(norm.as_bytes()).iter().take(8).map(|b| format!("{b:02x}")).collect::<String>())
}


/// "Use the original title" chosen for this title: the original title becomes the title.
pub fn use_original_title(it: &mut Item) {
    if it.extra.get("use_original_title").and_then(|v| v.as_str()) != Some("yes") {
        return;
    }
    if let Some(o) = it.extra.get("original_title").and_then(|v| v.as_str()).map(|s| s.trim().to_string()).filter(|s| !s.is_empty()) {
        it.sort_title = db::sort_title(&o);
        it.title = o;
    }
}


pub fn apply_metadata(it: &mut Item, meta: &Value, by_hand: bool) {
    let locked = it.locked.clone();
    let can = |f: &str| by_hand || !locked.iter().any(|l| l == f);
    let lock = |f: &str, it: &mut Item| {
        if by_hand && !it.locked.iter().any(|l| l == f) {
            it.locked.push(f.into());
        }
    };
    if let Some(v) = meta.get("title").and_then(|v| v.as_str()) {
        if can("title") && !v.trim().is_empty() {
            it.title = v.trim().into();
            it.sort_title = db::sort_title(v);
            lock("title", it);
        }
    }
    if let Some(v) = meta.get("sort_title").and_then(|v| v.as_str()) {
        if can("sort_title") {
            it.sort_title = v.to_lowercase();
            lock("sort_title", it);
        }
    }
    macro_rules! text {
        ($k:literal, $f:ident) => {
            if let Some(v) = meta.get($k).and_then(|v| v.as_str()) {
                if can($k) {
                    it.$f = v.into();
                    lock($k, it);
                }
            }
        };
    }
    text!("summary", summary);
    text!("tagline", tagline);
    text!("content_rating", content_rating);
    text!("studio", studio);
    if let Some(v) = meta.get("year") {
        if can("year") {
            it.year = v.as_u64().map(|y| y as u32).or_else(|| v.as_str().and_then(|s| s.parse().ok()));
            lock("year", it);
        }
    }
    if let Some(v) = meta.get("rating") {
        if can("rating") {
            it.rating = v.as_f64().map(|r| r as f32).or_else(|| v.as_str().and_then(|s| s.parse().ok()));
            lock("rating", it);
        }
    }
    if let Some(v) = meta.get("genres").and_then(|v| v.as_array()) {
        if can("genres") {
            it.genres = v.iter().filter_map(|g| g.as_str().map(|s| s.trim().to_string())).filter(|s| !s.is_empty()).collect();
            lock("genres", it);
        }
    }
    if let Some(v) = meta.get("poster").and_then(|v| v.as_str()) {
        if can("poster") && Path::new(v).exists() {
            it.poster = Some(v.into());
        }
    }
    if let Some(Value::Object(extra)) = meta.get("extra") {
        for (k, v) in extra {
            if !by_hand && locked.iter().any(|l| l == k) {
                continue;
            }
            it.extra.insert(k.clone(), v.clone());
        }
    }
    it.normalize();
    it.updated_at = now();
}


/// "CD1", "Disc 2", "Part 3", "DVD1" … — a folder for one part of a movie.
fn is_disc_folder(name: &str) -> bool {
    let n = name.trim().to_lowercase();
    ["cd", "disc", "disk", "dvd", "part", "pt"].iter().any(|p| {
        n.strip_prefix(p).is_some_and(|rest| { let r = rest.trim_start_matches([' ', '.', '_', '-']); !r.is_empty() && r.chars().all(|c| c.is_ascii_digit()) })
    })
}


/// The same folder, however the path is written (separators, a trailing slash, letter case on Windows).
fn scanner_same_dir(a: &Path, b: &Path) -> bool {
    crate::migrate::path_key(&a.to_string_lossy()) == crate::migrate::path_key(&b.to_string_lossy())
}


#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::MediaFile;

    #[test]
    fn folder_title_only_for_a_movie_folder_of_its_own() {
        let mut db: Db = serde_json::from_value(json!({"version": 1, "machine_id": "x", "next_id": 9, "settings": crate::db::Settings::default(),
            "sections": [{"id": 1, "name": "Filme", "kind": "movie", "paths": ["/m"], "created_at": 0}],
            "items": {}, "users": [], "invites": [], "tokens": [], "playlists": [], "views": {}, "plugins": {}})).unwrap();
        let movie = |id: u64, path: &str| -> Item {
            let mut i: Item = serde_json::from_value(json!({"id": id, "section_id": 1, "kind": "movie", "title": "funxd", "sort_title": "funxd",
                "added_at": 0, "updated_at": 0})).unwrap();
            i.file = Some(MediaFile { path: path.into(), ..Default::default() });
            i
        };
        for i in [movie(1, "/m/Der Pate (1972)/funxd.mkv"), movie(2, "/m/loose.mkv"), movie(3, "/m/Sammlung/a.mkv"), movie(4, "/m/Sammlung/b.mkv")] {
            db.items.insert(i.id, i);
        }
        assert_eq!(App::folder_title(&db, &db.items[&1]), Some(("Der Pate".into(), Some(1972))));
        // Right in the library folder: no folder name to use.
        assert_eq!(App::folder_title(&db, &db.items[&2]), None);
        // A folder holding several movies is not a title.
        assert_eq!(App::folder_title(&db, &db.items[&3]), None);
    }


    #[test]
    fn folder_title_looks_past_disc_and_release_folders() {
        let mut db: Db = serde_json::from_value(json!({"version": 1, "machine_id": "x", "next_id": 9, "settings": crate::db::Settings::default(),
            "sections": [{"id": 1, "name": "Filme", "kind": "movie", "paths": ["/m/"], "created_at": 0}],
            "items": {}, "users": [], "invites": [], "tokens": [], "playlists": [], "views": {}, "plugins": {}})).unwrap();
        let movie = |id: u64, path: &str| -> Item {
            let mut i: Item = serde_json::from_value(json!({"id": id, "section_id": 1, "kind": "movie", "title": "funxd", "sort_title": "funxd",
                "added_at": 0, "updated_at": 0})).unwrap();
            i.file = Some(MediaFile { path: path.into(), ..Default::default() });
            i
        };
        for i in [movie(1, "/m/Der Pate (1972)/funxd/funxd.mkv"), movie(2, "/m/Heat (1995)/CD1/funxd.mkv"),
            movie(3, "/m/Der.Pate.1972.German.DL.1080p.BluRay.x264-FuNXD/funxd.mkv"), movie(4, "/m/funxd/funxd.mkv")] {
            db.items.insert(i.id, i);
        }
        assert_eq!(App::folder_title(&db, &db.items[&1]), Some(("Der Pate".into(), Some(1972))));
        assert_eq!(App::folder_title(&db, &db.items[&2]), Some(("Heat".into(), Some(1995))));
        assert_eq!(App::folder_title(&db, &db.items[&3]), Some(("Der Pate".into(), Some(1972))));
        // Only the library folder above: nothing better than the file name.
        assert_eq!(App::folder_title(&db, &db.items[&4]), None);
        assert!(is_disc_folder("CD1") && is_disc_folder("Disc 2") && !is_disc_folder("Cdrama"));
    }


    #[test]
    fn broken_pictures_cleanup_leaves_media_folders_and_unchanged_items_alone() {
        let root = std::env::temp_dir().join(format!("lichtspiel-broken-pictures-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let media = root.join("media");
        std::fs::create_dir_all(&media).unwrap();
        let app = App::open(&root.join("data"));
        std::fs::create_dir_all(app.data_dir.join("metadata/1")).unwrap();
        let ours = app.data_dir.join("metadata/1/poster.jpg");
        let theirs = media.join("poster.jpg");
        std::fs::write(&ours, b"<svg/>").unwrap();
        std::fs::write(&theirs, b"").unwrap();
        {
            let mut db = app.db.lock().unwrap();
            for (id, p) in [(1u64, Some(&ours)), (2, Some(&theirs)), (3, None)] {
                let mut i: Item = serde_json::from_value(json!({"id": id, "section_id": 1, "kind": "movie", "title": "x", "sort_title": "x", "added_at": 0, "updated_at": 0})).unwrap();
                i.poster = p.map(|p| p.to_string_lossy().to_string());
                db.items.insert(id, i);
            }
            let _ = db.items.take_changes();
        }
        let broken = app.remove_broken_pictures();
        assert_eq!(broken, vec![1]);
        assert!(!ours.exists());
        assert!(theirs.exists(), "a file in the user's media folder was deleted");
        assert_eq!(app.db.lock().unwrap().items[&2].poster.as_deref(), Some(&*theirs.to_string_lossy()));
        // Nothing broken is left: a second run changes no item (no row is rewritten).
        assert!(app.remove_broken_pictures().is_empty());
        let mut db = app.db.lock().unwrap();
        assert!(matches!(db.items.take_changes(), crate::store::Changed::Keys(k) if k.is_empty()));
        drop(db);
        let _ = std::fs::remove_dir_all(&root);
    }

}

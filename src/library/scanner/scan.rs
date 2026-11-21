//! Scanning a library: progress, finding new, changed and gone files, and putting them in the library.

use super::*;

/// Things that changed during a scan, reported to plugins.
#[derive(Default, Debug)]
pub struct ScanReport {
    pub added: Vec<u64>,
    pub updated: Vec<u64>,
    pub removed: Vec<u64>,
    pub files_seen: usize,
}

/// One file to look at, gathered before the lock on the library is taken.
struct Found {
    path: PathBuf,
    root: PathBuf,
    size: u64,
    mtime: u64,
}

/// How far a running scan is, per library: shown in the top bar of the web app.
#[derive(Clone, Default)]
pub struct ScanStep {
    /// "listing" (walking the folders; `done` = videos found so far, `current` = the folder), "reading" (file by file) or "finishing".
    pub phase: &'static str,
    pub done: usize,
    pub total: usize,
    /// The file it is on.
    pub current: String,
}
static PROGRESS: std::sync::Mutex<std::collections::BTreeMap<u64, ScanStep>> = std::sync::Mutex::new(std::collections::BTreeMap::new());

pub fn progress_of(section: u64) -> Option<ScanStep> {
    PROGRESS.lock().unwrap().get(&section).cloned()
}

pub(super) fn step(section: u64, phase: &'static str, done: usize, total: usize, current: &str) {
    PROGRESS.lock().unwrap().insert(section, ScanStep { phase, done, total, current: current.to_string() });
}

/// Takes the library out of the progress list when its scan ends (also on an early return).
struct ProgressGuard(u64);
impl Drop for ProgressGuard {
    fn drop(&mut self) {
        PROGRESS.lock().unwrap().remove(&self.0);
    }
}

/// Scans one library. The slow part (walking folders, ffprobe) happens
/// without holding the library lock; `with_db` is called for short updates.
/// (The server scans through `scan_section_in`; this whole-library form is what the tests use.)
#[cfg(test)]
pub fn scan_section<F>(section: &Section, ffprobe: &str, with_db: F) -> ScanReport
where
    F: FnMut(&mut dyn FnMut(&mut Db)),
{
    scan_section_in(section, ffprobe, None, with_db)
}

/// Like [`scan_section`], but with `only` set just that folder (inside one of the library's folders) is
/// looked at: files elsewhere are neither added nor taken out.
pub fn scan_section_in<F>(section: &Section, ffprobe: &str, only: Option<&Path>, mut with_db: F) -> ScanReport
where
    F: FnMut(&mut dyn FnMut(&mut Db)),
{
    let _progress = ProgressGuard(section.id);
    step(section.id, "listing", 0, 0, "");
    let mut found = vec![];
    // The same folders the server sends files from (all libraries), so links between libraries keep working.
    let mut folders: Vec<String> = vec![];
    with_db(&mut |db: &mut Db| folders = db.sections.iter().flat_map(|s| s.paths.clone()).collect());
    for root in &section.paths {
        let root = PathBuf::from(root);
        if only.is_some_and(|o| !o.starts_with(&root)) { continue; }
        let mut files = vec![];
        walk(only.unwrap_or(&root), &mut files, 0, section.kind == SectionKind::Music, Some((section.id, found.len())));
        // A link to a file outside the libraries would let anyone who can see the library read that file.
        for f in files {
            if f.link && !crate::server::inside_folders(&folders, &f.path) { elog!("{} is a link to a file outside the libraries and is skipped", f.path.display()); continue; }
            found.push(Found { size: f.size, mtime: f.mtime, path: f.path, root: root.clone() });
        }
    }
    let mut report = ScanReport { files_seen: found.len(), ..Default::default() };

    // Which files are new or changed?
    let mut known: HashMap<String, (u64, u64, u64)> = HashMap::new();
    // The subtitle files each known video had, to notice ones put next to it since.
    let mut known_subs: HashMap<u64, Vec<String>> = HashMap::new();
    // The further parts each known movie has (path, size, change time).
    let mut known_parts: HashMap<u64, Vec<(String, u64, u64)>> = HashMap::new();
    // Known extras: item id -> (kind of extra, movie it belongs to).
    let mut known_extras: HashMap<u64, (String, Option<u64>)> = HashMap::new();
    // Files read by an older probe: only their streams are read again.
    let mut stale: HashSet<String> = HashSet::new();
    with_db(&mut |db: &mut Db| {
        for it in db.items.values() {
            if it.section_id == section.id {
                if !it.parts.is_empty() {
                    known_parts.insert(it.id, it.parts.iter().map(|f| (f.path.clone(), f.size, f.modified)).collect());
                }
                if it.is_extra() {
                    known_extras.insert(it.id, (it.extra["extra_type"].as_str().unwrap_or("").to_string(), it.parent_id));
                }
                if let Some(f) = &it.file {
                    known.insert(f.path.clone(), (it.id, f.size, f.modified));
                    known_subs.insert(it.id, f.external_subtitles.clone());
                    if f.probed && f.probe_version < crate::data::db::PROBE_VERSION {
                        stale.insert(f.path.clone());
                    }
                }
            }
        }
    });

    let seen: HashSet<String> = found.iter().map(|f| f.path.to_string_lossy().to_string()).collect();
    let key_of = |f: &Found| f.path.to_string_lossy().to_string();
    let mut sub_cache: SubtitleCache = HashMap::new();

    // Movies split over files ("… - pt1", "… - pt2" in one folder) become one
    // movie: the first part is its file, the others are its further parts.
    let mut parts_of: HashMap<String, Vec<usize>> = HashMap::new();
    let mut later_parts: HashSet<String> = HashSet::new();
    let mut extras: Vec<(usize, ExtraFile)> = vec![];
    let mut main: Vec<usize> = vec![];
    {
        let mut groups: HashMap<(PathBuf, String), Vec<(u32, usize)>> = HashMap::new();
        for (i, f) in found.iter().enumerate() {
            if matches!(section.kind, SectionKind::Movie | SectionKind::Show) {
                if let Some(x) = extra_of(&f.root, &f.path) {
                    // TV extras are left out (they would turn into odd episodes).
                    if section.kind == SectionKind::Movie {
                        extras.push((i, x));
                    }
                    continue;
                }
            }
            main.push(i);
            if section.kind == SectionKind::Movie {
                let stem = f.path.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
                if let (base, Some(n)) = split_part(&stem) {
                    let dir = f.path.parent().unwrap_or(Path::new("")).to_path_buf();
                    groups.entry((dir, base.to_lowercase())).or_default().push((n, i));
                }
            }
        }
        for (_, mut g) in groups {
            if g.len() < 2 {
                continue;
            }
            g.sort();
            // Plex stacks up to 8 parts.
            g.truncate(8);
            let first = key_of(&found[g[0].1]);
            for (_, i) in &g[1..] {
                later_parts.insert(key_of(&found[*i]));
            }
            parts_of.insert(first, g[1..].iter().map(|(_, i)| *i).collect());
        }
        main.retain(|i| !later_parts.contains(&key_of(&found[*i])));
    }

    let total_files = main.len() + extras.len();
    let mut done_files = 0usize;
    step(section.id, "reading", 0, total_files, "");

    // Shows and seasons of this library by what find_or_make matches on, so
    // a new episode doesn't search the whole library twice.
    let mut containers: Option<HashMap<ContainerKey, u64>> = None;
    // First the movies, shows and videos; then the extras, which need the movie
    // they belong to. An extra whose movie isn't found is read as a movie itself.
    for pass in 0..2 {
        let list: Vec<(usize, Option<(&'static str, u64, String)>)> = if pass == 0 {
            main.iter().map(|i| (*i, None)).collect()
        } else {
            let mut v = vec![];
            // The library's movies by folder (id, file name without the part), in id order: one pass, not one per extra.
            let mut by_dir: HashMap<PathBuf, Vec<(u64, String)>> = HashMap::new();
            with_db(&mut |db: &mut Db| {
                for m in db.items.values().filter(|m| m.section_id == section.id && m.kind == ItemKind::Movie) {
                    let Some(f) = &m.file else { continue };
                    let p = Path::new(&f.path);
                    let Some(dir) = p.parent() else { continue };
                    let stem = p.file_stem().map(|s| split_part(&s.to_string_lossy()).0.to_lowercase()).unwrap_or_default();
                    by_dir.entry(dir.to_path_buf()).or_default().push((m.id, stem));
                }
            });
            for (i, x) in &extras {
                let movies = by_dir.get(&x.movie_dir).map(Vec::as_slice).unwrap_or_default();
                let parent = match &x.prefix {
                    Some(pre) => movies.iter().find(|m| m.1 == pre.to_lowercase()).or(if movies.len() == 1 { movies.first() } else { None }),
                    None => movies.first(),
                };
                v.push((*i, parent.map(|m| (x.kind, m.0, x.title.clone()))));
            }
            v
        };
        for (idx, as_extra) in list {
            let f = &found[idx];
            done_files += 1;
            step(section.id, "reading", done_files, total_files, &f.path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default());
            let key = key_of(f);
            let parts_here: Vec<&Found> = parts_of.get(&key).map(|v| v.iter().map(|i| &found[*i]).collect()).unwrap_or_default();
            let existing_id = known.get(&key).map(|k| k.0);
            let mut changed = match known.get(&key) {
                Some((_, size, mtime)) => *size != f.size || *mtime != f.mtime,
                None => true,
            };
            if let Some(id) = existing_id {
                // The parts changed (one added, removed or replaced)?
                let now_parts: Vec<(String, u64, u64)> = parts_here.iter().map(|p| (key_of(p), p.size, p.mtime)).collect();
                if known_parts.get(&id).cloned().unwrap_or_default() != now_parts {
                    changed = true;
                }
                // Became an extra, or stopped being one?
                match (&as_extra, known_extras.get(&id)) {
                    (Some((kind, parent, _)), Some((k, p))) => {
                        if *kind != k.as_str() || Some(*parent) != *p {
                            let (kind, parent) = (*kind, *parent);
                            with_db(&mut |db: &mut Db| {
                                if let Some(it) = db.items.get_mut(&id) {
                                    it.parent_id = Some(parent);
                                    it.extra.insert("extra_type".into(), Value::from(kind));
                                }
                            });
                        }
                    }
                    (None, None) => {}
                    _ => changed = true,
                }
            }
            if !changed {
                // The video is as before, but a subtitle file may have been put next to it or taken away.
                let id = known[&key].0;
                let stem = f.path.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
                let subs = find_subtitles_cached(&mut sub_cache, f.path.parent().unwrap_or(Path::new(".")), &stem);
                if known_subs.get(&id) != Some(&subs) {
                    with_db(&mut |db: &mut Db| {
                        if let Some(it) = db.items.get_mut(&id) {
                            let Some(file) = it.file.as_mut() else { return };
                            let old = std::mem::replace(&mut file.external_subtitles, subs.clone());
                            if let Some(sync) = it.extra.get_mut("subsync") { crate::library::subsync::renumber_side_files(sync, &old, &subs); }
                        }
                    });
                    report.updated.push(id);
                }
                if stale.contains(&key) {
                    let mut m = MediaFile { path: key.clone(), ..Default::default() };
                    if probe(ffprobe, &mut m) {
                        let id = known[&key].0;
                        with_db(&mut |db: &mut Db| {
                            if let Some(f) = db.items.get_mut(&id).and_then(|i| i.file.as_mut()) {
                                f.streams = m.streams.clone();
                                f.probe_version = m.probe_version;
                            }
                        });
                    }
                }
                continue;
            }
            let read = |f: &Found| {
                let mut media = MediaFile {
                    path: f.path.to_string_lossy().to_string(),
                    size: f.size,
                    modified: f.mtime,
                    container: ext_of(&f.path),
                    ..Default::default()
                };
                probe(ffprobe, &mut media);
                media
            };
            let mut music_tags = None;
            let mut media = if section.kind == SectionKind::Music {
                let mut m = MediaFile { path: f.path.to_string_lossy().to_string(), size: f.size, modified: f.mtime, container: ext_of(&f.path), ..Default::default() };
                music_tags = probe_tags(ffprobe, &mut m);
                m
            } else { read(f) };
            let parts: Vec<MediaFile> = parts_here.iter().map(|p| read(p)).collect();
            let duration = media.duration_ms + parts.iter().map(|p| p.duration_ms).sum::<u64>();
            let dir = f.path.parent().unwrap_or(Path::new("."));
            let stem = f.path.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
            media.external_subtitles = find_subtitles(dir, &stem);
            let poster = if as_extra.is_some() { None } else { find_poster(dir, &stem) };
            let parsed = match &as_extra {
                Some((_, _, title)) => Parsed { kind: ItemKind::Video, title: title.clone(), year: None, show: None, season: None, episode: None, music: None },
                None if section.kind == SectionKind::Music => {
                    let m = crate::library::music::info(&f.root, &f.path, music_tags.as_ref());
                    Parsed { kind: ItemKind::Track, title: m.title.clone(), year: m.year, show: None, season: None, episode: m.track, music: Some(m) }
                }
                None => classify(section, &f.root, &f.path),
            };
            // Cover pictures: the album's folder, and the artist's folder one level up.
            let (album_cover, artist_cover) = if section.kind == SectionKind::Music {
                let rel = f.path.strip_prefix(&f.root).ok();
                let artist_dir = rel.filter(|r| r.components().count() >= 3).and_then(|r| r.components().next()).map(|c| f.root.join(c.as_os_str()));
                (find_poster(dir, "__none__"), artist_dir.and_then(|d| find_poster(&d, "__none__")))
            } else { (None, None) };
            let show_poster = if section.kind == SectionKind::Show {
                let rel = f.path.strip_prefix(&f.root).ok();
                rel.and_then(|r| r.components().next())
                    .map(|c| f.root.join(c.as_os_str()))
                    .and_then(|d| find_poster(&d, "__none__"))
            } else {
                None
            };

            with_db(&mut |db: &mut Db| {
                let t = now();
                let parent = if let Some((_, movie, _)) = &as_extra {
                    Some(*movie)
                } else if let Some(m) = &parsed.music {
                    let cache = containers.get_or_insert_with(|| container_index(db, section.id));
                    let artist = find_or_make(db, cache, section.id, ItemKind::Artist, None, &m.artist, None, None, artist_cover.clone());
                    Some(find_or_make(db, cache, section.id, ItemKind::Album, Some(artist), &m.album, m.year, None, album_cover.clone()))
                } else if let Some((show_title, show_year)) = &parsed.show {
                    let cache = containers.get_or_insert_with(|| container_index(db, section.id));
                    let show_id = find_or_make(db, cache, section.id, ItemKind::Show, None, show_title, *show_year, None, show_poster.clone());
                    let season_no = parsed.season.unwrap_or(1);
                    let season_title = if season_no == 0 { "Specials".to_string() } else { format!("Season {season_no}") };
                    Some(find_or_make(db, cache, section.id, ItemKind::Season, Some(show_id), &season_title, None, Some(season_no), None))
                } else {
                    None
                };
                let id = match existing_id.and_then(|id| db.items.get_mut(&id)) {
                    Some(item) => {
                        item.file = Some(media.clone());
                        item.parts = parts.clone();
                        item.duration_ms = duration;
                        item.updated_at = t;
                        item.kind = parsed.kind;
                        if !item.locked.contains(&"title".to_string()) {
                            item.title = parsed.title.clone();
                            item.sort_title = sort_title(&parsed.title);
                        }
                        item.parent_id = parent;
                        item.index = parsed.episode;
                        if poster.is_some() {
                            item.poster = poster.clone();
                        }
                        report.updated.push(item.id);
                        item.id
                    }
                    None => {
                        let id = db.new_id();
                        db.items.insert(
                            id,
                            Item {
                                id,
                                section_id: section.id,
                                kind: parsed.kind,
                                parent_id: parent,
                                title: parsed.title.clone(),
                                sort_title: sort_title(&parsed.title),
                                year: parsed.year,
                                index: parsed.episode,
                                summary: String::new(),
                                tagline: String::new(),
                                genres: vec![],
                                rating: None,
                                content_rating: String::new(),
                                studio: String::new(),
                                added_at: t,
                                updated_at: t,
                                duration_ms: duration,
                                file: Some(media.clone()),
                                parts: parts.clone(),
                                poster: poster.clone(),
                                extra: Default::default(),
                                locked: vec![],
                            },
                        );
                        report.added.push(id);
                        id
                    }
                };
                if let (Some(m), Some(it)) = (&parsed.music, db.items.get_mut(&id)) {
                    it.genres = m.genres.clone();
                    match m.disc { Some(d) => it.extra.insert("disc".into(), Value::from(d)), None => it.extra.remove("disc") };
                    let album = it.parent_id;
                    // The album takes the year and genres of its songs.
                    if let Some(a) = album.and_then(|a| db.items.get_mut(&a)) {
                        if a.year.is_none() { a.year = m.year; }
                        if a.genres.is_empty() { a.genres = m.genres.clone(); }
                        // A release is a live album, single ... as soon as one song says so.
                        if a.extra.get("release").and_then(|r| r.as_str()).is_none_or(|r| r == "album") { a.extra.insert("release".into(), Value::from(m.release)); }
                    }
                }
                if let Some(it) = db.items.get_mut(&id) {
                    match &as_extra {
                        Some((kind, _, _)) => {
                            it.extra.insert("extra_type".into(), Value::from(*kind));
                        }
                        None => {
                            it.extra.remove("extra_type");
                        }
                    }
                }
            });
        }
    }

    step(section.id, "finishing", total_files, total_files, "");
    // Remove what disappeared (or, with "Empty trash automatically" off, keep it
    // as unavailable), then empty seasons and shows. A library folder that cannot
    // be read (a drive or share not mounted) never empties anything.
    let offline: Vec<PathBuf> = section.paths.iter().map(PathBuf::from)
        .filter(|r| std::fs::read_dir(r).map(|mut d| d.next().is_none()).unwrap_or(true))
        .collect();
    with_db(&mut |db: &mut Db| {
        // A file swapped for another of the same movie or episode (a better version, a new name): the
        // new file moves into the old title, so watched state, ratings, playlists and details stay.
        for (old, new) in adopt_replacements(db, section.id, &seen, &later_parts, &offline, only) {
            let was_new = report.added.contains(&new);
            report.added.retain(|i| *i != new);
            report.updated.retain(|i| *i != new);
            if !was_new { report.removed.push(new); }
            if !report.updated.contains(&old) { report.updated.push(old); }
        }
        let auto = db.settings.empty_trash_automatically;
        let mut gone = vec![];
        let mut trash = vec![];
        let mut back = vec![];
        for i in db.items.values().filter(|i| i.section_id == section.id) {
            let Some(f) = i.file.as_ref() else { continue };
            if only.is_some_and(|o| !Path::new(&f.path).starts_with(o)) { continue; }
            if later_parts.contains(&f.path) {
                // Now a later part of a split movie.
                gone.push(i.id);
            } else if !seen.contains(&f.path) {
                let unreachable = offline.iter().any(|r| Path::new(&f.path).starts_with(r));
                if auto && !unreachable { gone.push(i.id) } else if !i.extra.contains_key(MISSING) { trash.push(i.id) }
            } else if i.extra.contains_key(MISSING) {
                back.push(i.id);
            }
        }
        for id in &gone {
            db.items.remove(id);
        }
        for id in &trash {
            if let Some(it) = db.items.get_mut(id) { it.extra.insert(MISSING.into(), Value::from(now())); }
        }
        for id in &back {
            if let Some(it) = db.items.get_mut(id) { it.extra.remove(MISSING); }
        }
        report.removed.extend(gone);
        for id in merge_duplicate_shows(db, section.id) {
            if !report.updated.contains(&id) { report.updated.push(id); }
        }
        report.removed.extend(prune_empty(db, section.id));
        // Shows and seasons take the newest episode's date, so they rise in
        // "recently added", and the length of all their episodes.
        let kids = children_map(db);
        fn below(db: &Db, kids: &HashMap<u64, Vec<u64>>, id: u64, newest: &mut Option<u64>, total: &mut u64) {
            for c in kids.get(&id).into_iter().flatten() {
                let Some(it) = db.items.get(c) else { continue };
                if it.kind.playable() {
                    *newest = (*newest).max(Some(it.added_at));
                    *total += it.duration_ms;
                } else {
                    below(db, kids, *c, newest, total);
                }
            }
        }
        let containers: Vec<u64> = db
            .items
            .values()
            .filter(|i| i.section_id == section.id && !i.kind.playable())
            .map(|i| i.id)
            .collect();
        for id in containers {
            let (mut newest, mut total) = (None, 0u64);
            below(db, &kids, id, &mut newest, &mut total);
            let Some(it) = db.items.get(&id) else { continue };
            let updated = newest.map_or(it.updated_at, |n| it.updated_at.max(n));
            // Only what really changed is marked for saving.
            if updated != it.updated_at || total != it.duration_ms {
                if let Some(it) = db.items.get_mut(&id) {
                    it.updated_at = updated;
                    it.duration_ms = total;
                }
            }
        }
        auto_merge_versions(db, section.id);
        if let Some(s) = db.sections.iter_mut().find(|s| s.id == section.id) {
            s.scanned_at = now();
        }
    });
    report
}

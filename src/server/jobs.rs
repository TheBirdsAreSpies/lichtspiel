//! Background jobs on the library: finding intros and credits, IMDb ratings, and cancelling a running job.

use super::*;

impl App {
    /// "Detect credits": looks for end credits in the given items' files (on a
    /// background thread, shown as a job in the web app): chapters named like
    /// "Credits" first, else the picture of the last part of the video. It skips libraries with
    /// credits detection off and, unless `force`, files that are already up to
    /// date or failed more than 3 times. Returns how many videos it will check.
    pub fn detect_credits(self: &Arc<Self>, ids: Vec<u64>, force: bool) -> Result<usize, String> {
        if force {
            self.auto_paused.store(false, AtomicOrdering::SeqCst);
        }
        let mut job = self.job.lock().unwrap();
        if job.is_some() {
            return Err("Another job (credits detection or ratings) is already running; try again in a moment".into());
        }
        let (files, ffmpeg, ffprobe, picture) = {
            let db = self.db.lock().unwrap();
            if db.settings.credits_markers_behaviour == "never" {
                return Err("Credits markers are switched off in Settings (Generate credits video markers: never)".into());
            }
            let mut leaves = vec![];
            for id in ids {
                match db.items.get(&id) {
                    Some(i) if i.kind.playable() => leaves.push(id),
                    Some(_) => leaves.extend(db.leaves(id)),
                    None => {}
                }
            }
            let files: Vec<(u64, String, u64)> = leaves
                .iter()
                .filter_map(|l| db.items.get(l))
                .filter(|i| db.credits_on(i))
                // A split movie's credits are in its last part.
                .filter_map(|i| i.last_file().map(|f| (i, f)))
                .filter(|(_, f)| force || (f.credits_checked < db::CREDITS_DETECTOR_VERSION && f.credits_errors <= db::CREDITS_MAX_ERRORS))
                .map(|(i, f)| (i.id, f.path.clone(), if i.parts.is_empty() { f.duration_ms.max(i.duration_ms) } else { f.duration_ms }))
                .collect();
            (files, db.settings.ffmpeg.clone(), db.settings.ffprobe.clone(), db.settings.credits_from_picture)
        };
        let total = files.len();
        if total == 0 {
            return Err(if force { "No videos to check (or credits detection is off for this library)".into() } else { "Everything is already up to date".into() });
        }
        *job = Some(("Detecting credits".into(), 0, total));
        self.job_cancel.store(false, AtomicOrdering::SeqCst);
        self.job_yield.store(false, AtomicOrdering::SeqCst);
        self.job_auto.store(!force, AtomicOrdering::SeqCst);
        drop(job);
        self.set_job_items("Detecting credits", &files.iter().map(|f| f.0).collect::<Vec<_>>());
        let app = self.clone();
        std::thread::spawn(move || {
            let (found, failed) = (std::sync::atomic::AtomicUsize::new(0), std::sync::atomic::AtomicUsize::new(0));
            // The old Plex server's markers first (same file path): no need to search the picture for those.
            let old_db = crate::data::migrate::detect();
            if old_db.is_none() {
                elog!("credits detection: no old Plex database found (set OLD_SERVER), so every video is searched");
            }
            let from_plex = old_db
                .and_then(|p| crate::data::migrate::credits_by_path(&p, &app.data_dir.join("cache").join("old-markers")).map_err(|e| elog!("old server's credits markers: {e}")).ok())
                .unwrap_or_default();
            let taken = std::sync::atomic::AtomicUsize::new(0);
            elog!("credits detection: {} files with credits markers in the old server's database; {} videos to check", from_plex.len(), total);
            // Several files at once: each one is a separate ffmpeg that mostly waits for the disk.
            let (next, done) = (std::sync::atomic::AtomicUsize::new(0), std::sync::atomic::AtomicUsize::new(0));
            let workers = std::thread::available_parallelism().map_or(2, |n| n.get()).clamp(2, 4);
            std::thread::scope(|sc| {
                for _ in 0..workers {
                    sc.spawn(|| loop {
                        if app.job_cancelled() {
                            break;
                        }
                        let n = next.fetch_add(1, AtomicOrdering::SeqCst);
                        let Some((id, path, duration)) = files.get(n) else { break };
                        let id = *id;
                        let r = match from_plex.get(path) {
                            Some(c) if !force => {
                                let t = taken.fetch_add(1, AtomicOrdering::SeqCst) + 1;
                                if t % 1000 == 0 {
                                    elog!("credits detection: {t} videos took their markers from the old server so far");
                                }
                                Ok((c.clone(), "imported"))
                            }
                            _ => crate::library::scanner::find_credits(&ffmpeg, &ffprobe, path, *duration, picture),
                        };
                        if let Some(f) = app.db.lock().unwrap().items.get_mut(&id).and_then(|i| i.last_file_mut()) {
                            match r {
                                Ok((c, how)) => {
                                    found.fetch_add(usize::from(!c.is_empty()), AtomicOrdering::SeqCst);
                                    f.credits_ms = c;
                                    f.credits_source = how.into();
                                    f.credits_checked = db::CREDITS_DETECTOR_VERSION;
                                    f.credits_errors = 0;
                                }
                                Err(e) => {
                                    failed.fetch_add(1, AtomicOrdering::SeqCst);
                                    f.credits_errors += 1;
                                    elog!("credits detection for item {id}: {e}");
                                    if f.credits_errors > db::CREDITS_MAX_ERRORS {
                                        elog!("credits detection for item {id} has failed too many times; it will not be retried automatically");
                                    }
                                }
                            }
                        }
                        let d = done.fetch_add(1, AtomicOrdering::SeqCst) + 1;
                        *app.job.lock().unwrap() = Some(("Detecting credits".into(), d, total));
                        // Kept now and then, so a restart during a long first run loses little.
                        if d % 50 == 0 { app.save(); }
                    });
                }
            });
            let (found, failed) = (found.into_inner(), failed.into_inner());
            elog!("credits detection ended: {} of {total} videos took the old server's markers, the rest was searched in the picture", taken.into_inner());
            app.save();
            *app.job.lock().unwrap() = None;
            let cancelled = app.job_cancel.swap(false, AtomicOrdering::SeqCst);
            app.job_auto.store(false, AtomicOrdering::SeqCst);
            // Stopped for the details: no result to report, the scheduler takes it up again later.
            if app.job_yield.swap(false, AtomicOrdering::SeqCst) {
                return;
            }
            let mut p = app.meta_progress.lock().unwrap();
            p.done += 1;
            let failed_txt = if failed > 0 { format!(", {failed} could not be read") } else { String::new() };
            p.last = json!({"id": null, "title": "Credits detection", "ok": failed == 0,
                "message": format!("Credits detection {}: credits found in {found} of {total} videos{failed_txt}", if cancelled { "cancelled" } else { "finished" }), "at": now()});
        });
        Ok(total)
    }


    /// "Detect intros": compares the sound at the start of every episode of a
    /// season with the others (intro.rs),
    /// on a background thread shown as a job. `ids` may be
    /// shows, seasons or episodes. Unless `force`, seasons whose episodes are
    /// all up to date are skipped ("Already up to date"). Returns how many
    /// episodes it will listen to.
    /// Asks the running job (credits, intros, analysis, optimizing) to stop after the file it is on.
    pub fn cancel_job(&self) -> bool {
        let running = self.job.lock().unwrap().is_some();
        if running {
            self.job_cancel.store(true, AtomicOrdering::SeqCst);
            if self.job_auto.load(AtomicOrdering::SeqCst) {
                self.auto_paused.store(true, AtomicOrdering::SeqCst);
            }
        }
        // Details waiting for their turn are dropped too; the one being fetched right now finishes.
        let waiting = {
            let mut q = self.meta_queue.lock().unwrap();
            let n = q.len();
            q.clear();
            n > 0
        };
        running || waiting
    }


    /// A readable name for an item in lists of work: "Show · S1E2 Title" for an episode, else its title.
    pub fn label_of(db: &Db, id: u64) -> String {
        let Some(it) = db.items.get(&id) else { return format!("#{id}") };
        match it.kind {
            ItemKind::Episode => {
                let season = it.parent_id.and_then(|p| db.items.get(&p));
                let show = season.and_then(|s| s.parent_id).and_then(|p| db.items.get(&p));
                format!("{} \u{b7} S{}E{} {}", show.map(|s| s.title.as_str()).unwrap_or(""), season.and_then(|s| s.index).unwrap_or(0), it.index.unwrap_or(0), it.title)
            }
            ItemKind::Season => {
                let show = it.parent_id.and_then(|p| db.items.get(&p));
                format!("{} \u{b7} {}", show.map(|s| s.title.as_str()).unwrap_or(""), it.title)
            }
            _ => it.title.clone(),
        }
    }


    /// Remembers what the job `label` is going to work through, for the activity panel.
    pub fn set_job_items(&self, label: &str, ids: &[u64]) {
        let names: Vec<String> = {
            let db = self.db.lock().unwrap();
            ids.iter().map(|i| Self::label_of(&db, *i)).collect()
        };
        *self.job_items.lock().unwrap() = (label.to_string(), names);
    }


    pub fn job_cancelled(&self) -> bool {
        self.job_cancel.load(AtomicOrdering::SeqCst)
    }


    pub fn detect_intros(self: &Arc<Self>, ids: Vec<u64>, force: bool) -> Result<usize, String> {
        if force {
            self.auto_paused.store(false, AtomicOrdering::SeqCst);
        }
        use crate::library::intro;
        let mut job = self.job.lock().unwrap();
        if job.is_some() {
            return Err("Another job (credits or intro detection, or ratings) is already running; try again in a moment".into());
        }
        type Episode = (u64, String, u64, u64);
        let (seasons, ffmpeg): (Vec<(u64, Vec<Episode>)>, String) = {
            let db = self.db.lock().unwrap();
            if db.settings.intro_markers_behaviour == "never" {
                return Err("Intro markers are switched off in Settings (Generate intro video markers: never)".into());
            }
            let mut season_ids: Vec<u64> = vec![];
            for id in ids {
                let Some(it) = db.items.get(&id) else { continue };
                match it.kind {
                    ItemKind::Show => season_ids.extend(db.children(id).iter().filter(|c| c.kind == ItemKind::Season).map(|c| c.id)),
                    ItemKind::Season => season_ids.push(id),
                    ItemKind::Episode => season_ids.extend(it.parent_id),
                    _ => {}
                }
            }
            let mut seen = std::collections::HashSet::new();
            season_ids.retain(|s| seen.insert(*s));
            let mut seasons = vec![];
            for sid in season_ids {
                let eps: Vec<&Item> = db.children(sid).into_iter().filter(|e| e.kind == ItemKind::Episode && e.file.is_some() && db.intro_on(e)).collect();
                // One episode has nothing to be compared with.
                if eps.len() < 2 {
                    continue;
                }
                let stale = eps.iter().any(|e| e.file.as_ref().is_some_and(|f| {
                    (intro::is_due(f.intro_checked, !f.intro_ms.is_empty()) && f.intro_errors <= db::CREDITS_MAX_ERRORS) || intro::recap_due(f.recap_checked, f.recap_errors)
                }));
                if !force && !stale {
                    continue;
                }
                seasons.push((sid, eps.iter().filter_map(|e| e.file.as_ref().map(|f| (e.id, f.path.clone(), f.size, f.modified))).collect()));
            }
            (seasons, db.settings.ffmpeg.clone())
        };
        let total: usize = seasons.iter().map(|s| s.1.len()).sum();
        if total == 0 {
            return Err(if force { "No season with at least two episodes to compare (or intro detection is off for this library)".into() } else { "Everything is already up to date".into() });
        }
        *job = Some(("Detecting intros".into(), 0, total));
        self.job_cancel.store(false, AtomicOrdering::SeqCst);
        self.job_yield.store(false, AtomicOrdering::SeqCst);
        self.job_auto.store(!force, AtomicOrdering::SeqCst);
        drop(job);
        self.set_job_items("Detecting intros", &seasons.iter().flat_map(|s| s.1.iter().map(|e| e.0)).collect::<Vec<_>>());
        let app = self.clone();
        let cache = self.data_dir.join("cache").join("intro");
        std::thread::spawn(move || {
            let (mut done, mut found, mut failed) = (0, 0, 0);
            // The old Plex server's intro markers first (same file path): those episodes are not listened to.
            let from_plex = crate::data::migrate::detect()
                .and_then(|p| crate::data::migrate::intros_by_path(&p, &app.data_dir.join("cache").join("old-markers")).map_err(|e| elog!("old server's intro markers: {e}")).ok())
                .unwrap_or_default();
            elog!("intro detection: {} files with intro markers in the old server's database; {total} episodes to check", from_plex.len());
            let mut taken = 0;
            'seasons: for (season, eps) in seasons {
                let (mut known, mut eps, compare_only) = split_known(eps, |e| from_plex.get(&e.1).is_some_and(|m| m.len() == 2));
                {
                    let mut db = app.db.lock().unwrap();
                    for (id, path, ..) in &known {
                        let Some(f) = db.items.get_mut(id).and_then(|i| i.file.as_mut()) else { continue };
                        f.intro_ms = from_plex.get(path).cloned().unwrap_or_default();
                        f.intro_checked = intro::DETECTOR_VERSION;
                        f.intro_errors = 0;
                        taken += 1;
                        found += 1;
                    }
                    // A single episode left has nothing to be compared with.
                    if eps.len() < 2 {
                        for (id, ..) in &eps {
                            if let Some(f) = db.items.get_mut(id).and_then(|i| i.file.as_mut()) {
                                f.intro_ms.clear();
                                f.intro_checked = intro::DETECTOR_VERSION;
                                f.intro_errors = 0;
                            }
                        }
                    }
                }
                done += known.len() - compare_only;
                known.clear();
                *app.job.lock().unwrap() = Some(("Detecting intros".into(), done, total));
                if eps.len() < 2 {
                    done += eps.len();
                    eps.clear();
                    if !app.find_recaps(season, &ffmpeg, &cache, force) {
                        break 'seasons;
                    }
                    continue;
                }
                // Several episodes at once: each one is a separate ffmpeg that mostly waits for the disk or the network.
                let results: Vec<std::sync::Mutex<Option<Result<Vec<u32>, String>>>> = eps.iter().map(|_| std::sync::Mutex::new(None)).collect();
                let (next, finished) = (std::sync::atomic::AtomicUsize::new(0), std::sync::atomic::AtomicUsize::new(0));
                let workers = std::thread::available_parallelism().map_or(2, |n| n.get()).clamp(2, 4);
                std::thread::scope(|sc| {
                    for _ in 0..workers {
                        sc.spawn(|| loop {
                            // A season is only judged when all its episodes were listened to.
                            if app.job_cancelled() {
                                break;
                            }
                            let n = next.fetch_add(1, AtomicOrdering::SeqCst);
                            let Some((_, path, size, modified)) = eps.get(n) else { break };
                            let file = cache.join(intro::cache_name(path, *size, *modified));
                            let print = match intro::read_cached(&file) {
                                Some(p) => Ok(p),
                                None => intro::fingerprint_file(&ffmpeg, path).inspect(|p| intro::write_cached(&file, p)),
                            };
                            *results[n].lock().unwrap() = Some(print);
                            let d = finished.fetch_add(1, AtomicOrdering::SeqCst) + 1;
                            *app.job.lock().unwrap() = Some(("Detecting intros".into(), (done + d.saturating_sub(compare_only)).min(total), total));
                        });
                    }
                });
                if app.job_cancelled() {
                    break 'seasons;
                }
                done += eps.len() - compare_only;
                let (mut prints, mut errors) = (vec![], vec![]);
                for r in results {
                    match r.into_inner().unwrap().unwrap_or_else(|| Err("not read".into())) {
                        Ok(p) => {
                            prints.push(p);
                            errors.push(None);
                        }
                        Err(e) => {
                            prints.push(vec![]);
                            errors.push(Some(e));
                        }
                    }
                }
                let intros = intro::detect_season(&prints);
                let mut db = app.db.lock().unwrap();
                for (((id, ..), found_intro), error) in eps.iter().zip(intros).zip(errors).skip(compare_only) {
                    let Some(f) = db.items.get_mut(id).and_then(|i| i.file.as_mut()) else { continue };
                    match error {
                        None => {
                            found += usize::from(found_intro.is_some());
                            f.intro_ms = found_intro.map(|(s, e)| vec![s, e]).unwrap_or_default();
                            f.intro_checked = intro::DETECTOR_VERSION;
                            f.intro_errors = 0;
                        }
                        Some(e) => {
                            failed += 1;
                            f.intro_errors += 1;
                            f.intro_ms.clear();
                            elog!("intro detection for item {id}: {e}");
                        }
                    }
                }
                drop(db);
                app.save();
                // The intros are known now, so they don't count as a recap.
                if !app.find_recaps(season, &ffmpeg, &cache, force) {
                    break 'seasons;
                }
            }
            elog!("intro detection ended: {taken} of {total} episodes took the old server's intro markers");
            app.save();
            *app.job.lock().unwrap() = None;
            let cancelled = app.job_cancel.swap(false, AtomicOrdering::SeqCst);
            app.job_auto.store(false, AtomicOrdering::SeqCst);
            if app.job_yield.swap(false, AtomicOrdering::SeqCst) {
                return;
            }
            let mut p = app.meta_progress.lock().unwrap();
            p.done += 1;
            let failed_txt = if failed > 0 { format!(", {failed} could not be read") } else { String::new() };
            p.last = json!({"id": null, "title": "Intro detection", "ok": failed == 0,
                "message": format!("Intro detection {}: intros found in {found} of {total} episodes{failed_txt}", if cancelled { "cancelled" } else { "finished" }), "at": now()});
        });
        Ok(total)
    }


    /// Recaps ("Previously on") of a season's episodes, after its intros were
    /// found: each episode's start against the whole sound of the one or two
    /// episodes before it (for the first one: the last episode of the season
    /// before). Specials are left out. False when the job was cancelled.
    fn find_recaps(self: &Arc<Self>, season: u64, ffmpeg: &str, cache: &std::path::Path, force: bool) -> bool {
        use crate::library::intro;
        // id, path, size, modified
        type File = (u64, String, u64, u64);
        type Parts = Vec<(f32, f32)>;
        // (episode, its intro, the episodes before it with their intro and credits)
        let tasks: Vec<(File, Parts, Vec<(File, Parts)>)> = {
            let db = self.db.lock().unwrap();
            let Some(this) = db.items.get(&season) else { return true };
            let episodes = |sid: u64| -> Vec<&Item> { db.children(sid).into_iter().filter(|e| e.kind == ItemKind::Episode && e.file.is_some()).collect() };
            let file = |e: &Item| e.file.as_ref().map(|f| (e.id, f.path.clone(), f.size, f.modified));
            let secs = |(s, e): (u64, u64)| (s as f32 / 1000.0, e as f32 / 1000.0);
            let intro_of = |e: &Item| -> Parts { db.intro_marker(e).map(secs).into_iter().collect() };
            let not_counted = |e: &Item| -> Parts {
                let mut v = intro_of(e);
                v.extend(db.credits_markers(e).first().map(|c| (*c as f32 / 1000.0, f32::MAX)));
                v
            };
            let number = this.index.unwrap_or(0);
            // The last episode of the season before (specials don't count).
            let before = this.parent_id.and_then(|show| {
                db.children(show).into_iter().filter(|s| s.kind == ItemKind::Season && s.index.is_some_and(|i| i > 0 && i < number)).max_by_key(|s| s.index)
            });
            let last_before: Option<&Item> = before.and_then(|s| episodes(s.id).into_iter().next_back());
            let eps = episodes(season);
            let mut tasks = vec![];
            for (k, e) in eps.iter().enumerate() {
                let Some(f) = e.file.as_ref() else { continue };
                if number == 0 || !db.intro_on(e) || (!force && !intro::recap_due(f.recap_checked, f.recap_errors)) {
                    continue;
                }
                let earlier: Vec<&Item> = if k > 0 { eps[k.saturating_sub(2)..k].to_vec() } else { last_before.into_iter().collect() };
                let Some(me) = file(e) else { continue };
                tasks.push((me, intro_of(e), earlier.into_iter().filter_map(|p| Some((file(p)?, not_counted(p)))).collect()));
            }
            tasks
        };
        let print = |(_, path, size, modified): &File, whole: bool| -> Result<Vec<u32>, String> {
            let name = if whole { intro::cache_name_whole(path, *size, *modified) } else { intro::cache_name(path, *size, *modified) };
            let file = cache.join(name);
            match intro::read_cached(&file) {
                Some(p) => Ok(p),
                None if whole => intro::fingerprint_whole_file(ffmpeg, path).inspect(|p| intro::write_cached(&file, p)),
                None => intro::fingerprint_file(ffmpeg, path).inspect(|p| intro::write_cached(&file, p)),
            }
        };
        let mut wholes: HashMap<u64, Result<Vec<u32>, String>> = HashMap::new();
        for (me, skip, earlier) in tasks {
            if self.job_cancelled() {
                return false;
            }
            let mut found = || -> Result<Option<(u64, u64)>, String> {
                if earlier.is_empty() {
                    return Ok(None);
                }
                let head = print(&me, false)?;
                for (f, _) in &earlier {
                    if !wholes.contains_key(&f.0) {
                        wholes.insert(f.0, print(f, true));
                    }
                }
                let mut list = vec![];
                for (f, not) in &earlier {
                    let whole = wholes[&f.0].as_ref().map_err(|e| format!("episode {} before it: {e}", f.0))?;
                    list.push((whole.as_slice(), not.clone()));
                }
                Ok(intro::find_recap(&head, &skip, &list))
            };
            let result = found();
            let mut db = self.db.lock().unwrap();
            let Some(f) = db.items.get_mut(&me.0).and_then(|i| i.file.as_mut()) else { continue };
            match result {
                Ok(r) => {
                    f.recap_ms = r.map(|(s, e)| vec![s, e]).unwrap_or_default();
                    f.recap_checked = intro::RECAP_VERSION;
                    f.recap_errors = 0;
                }
                Err(e) => {
                    f.recap_errors += 1;
                    elog!("recap detection for item {}: {e}", me.0);
                }
            }
        }
        self.save();
        true
    }


    /// Seasons still waiting for automatic intro detection (with at least two
    /// episodes, so there is something to compare): first those with an intro
    /// from an older detector, then those with an episode never looked at,
    /// newest first.
    pub fn intros_pending(&self) -> Vec<u64> {
        let db = self.db.lock().unwrap();
        if db.settings.intro_markers_behaviour == "never" {
            return vec![];
        }
        let mut due: Vec<(bool, u64, u64)> = db
            .items
            .values()
            .filter(|i| i.kind == ItemKind::Episode && db.intro_on(i))
            .filter_map(|i| {
                let f = i.file.as_ref()?;
                let wanted = (crate::library::intro::is_due(f.intro_checked, !f.intro_ms.is_empty()) && f.intro_errors <= db::CREDITS_MAX_ERRORS)
                    || crate::library::intro::recap_due(f.recap_checked, f.recap_errors);
                wanted.then_some((f.intro_checked == 0, i.added_at, i.parent_id?))
            })
            .collect();
        due.sort_by_key(|&(new, added, _)| (new, std::cmp::Reverse(added)));
        let mut seen = std::collections::HashSet::new();
        let mut v: Vec<u64> = due.into_iter().map(|d| d.2).filter(|s| seen.insert(*s)).collect();
        v.retain(|s| db.children(*s).iter().filter(|e| e.kind == ItemKind::Episode && e.file.is_some()).count() >= 2);
        v
    }


    /// Videos still waiting for automatic credits detection.
    pub fn credits_pending(&self) -> Vec<u64> {
        let db = self.db.lock().unwrap();
        if db.settings.credits_markers_behaviour == "never" {
            return vec![];
        }
        db.items
            .values()
            .filter(|i| db.credits_on(i))
            .filter(|i| i.last_file().is_some_and(|f| f.credits_checked < db::CREDITS_DETECTOR_VERSION && f.credits_errors <= db::CREDITS_MAX_ERRORS))
            .map(|i| i.id)
            .collect()
    }


    /// IMDb ratings from IMDb's own ratings file, for every movie and show with
    /// an IMDb id. The file is downloaded again when it is a week old.
    pub fn update_imdb_ratings(self: &Arc<Self>, force: bool) -> Result<String, String> {
        self.imdb_update(force, crate::library::metadata::download_imdb_ratings)
    }


    pub(super) fn imdb_update(self: &Arc<Self>, force: bool, download: fn(&Path) -> Result<(), String>) -> Result<String, String> {
        use crate::library::metadata;
        let file = self.data_dir.join("cache").join("imdb-ratings.tsv.gz");
        let age = std::fs::metadata(&file).and_then(|m| m.modified()).ok().and_then(|t| t.elapsed().ok());
        let stale = age.is_none_or(|a| a > Duration::from_secs(7 * 86400));
        let file_stamp = |f: &Path| std::fs::metadata(f).and_then(|m| m.modified()).ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map(|d| d.as_secs()).unwrap_or(0);
        let wanted: HashMap<String, Vec<u64>> = {
            let db = self.db.lock().unwrap();
            if !db.settings.imdb_ratings_file && !force {
                return Ok("IMDb ratings file is switched off".into());
            }
            let stamp = file_stamp(&file);
            let mut w: HashMap<String, Vec<u64>> = HashMap::new();
            for it in db.items.values().filter(|i| matches!(i.kind, ItemKind::Movie | ItemKind::Show)) {
                let Some(imdb) = it.extra.get("ids").and_then(|i| i["imdb"].as_str()).filter(|s| s.starts_with("tt")) else { continue };
                let done = it.extra.get("imdb_ratings_checked").and_then(|v| v.as_u64()) == Some(stamp);
                if force || stale || !done {
                    w.entry(imdb.to_string()).or_default().push(it.id);
                }
            }
            w
        };
        if wanted.is_empty() {
            return Ok("IMDb ratings are up to date".into());
        }
        {
            let mut job = self.job.lock().unwrap();
            if job.is_some() {
                return Err("Another job is running".into());
            }
            *job = Some(("Updating IMDb ratings".into(), 0, wanted.len()));
        }
        let mut failed = None;
        let result = (|| {
            if stale || force {
                if let Err(e) = download(&file) {
                    if !file.exists() {
                        return Err(e);
                    }
                    // The old file is used, but the update counts as failed, so it waits before trying again.
                    failed = Some(e);
                }
            }
            let ids: std::collections::HashSet<String> = wanted.keys().cloned().collect();
            let found = metadata::read_imdb_ratings(&file, &ids)?;
            let stamp = file_stamp(&file);
            let mut db = self.db.lock().unwrap();
            for (imdb, items) in &wanted {
                for id in items {
                    let Some(it) = db.items.get_mut(id) else { continue };
                    if let Some((r, n)) = found.get(imdb) {
                        let mut ratings = it.extra.get("ratings").cloned().filter(|v| v.is_object()).unwrap_or(json!({}));
                        ratings["imdb"] = metadata::imdb_score(*r, *n, imdb);
                        it.extra.insert("ratings".into(), ratings);
                    }
                    it.extra.insert("imdb_ratings_checked".into(), json!(stamp));
                }
            }
            Ok(format!("IMDb ratings updated for {} of {} titles", found.len(), wanted.len()))
        })();
        let result = match (result, failed) {
            (Ok(m), Some(e)) => Err(format!("{e} ({m} from the old file)")),
            (r, _) => r,
        };
        self.save();
        *self.job.lock().unwrap() = None;
        let mut p = self.meta_progress.lock().unwrap();
        p.done += 1;
        p.last = json!({"id": null, "title": "IMDb ratings", "ok": result.is_ok(), "message": match &result { Ok(m) | Err(m) => m.clone() }, "at": now()});
        result
    }
}

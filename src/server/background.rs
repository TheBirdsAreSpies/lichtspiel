//! What runs in the background while the server is up: watching folders, details, plugins, daily jobs.

use super::*;

/// Splits a season into the episodes that take the old server's intro marker (`known`) and those to listen to.
/// Returns (known, listen, how many of the first in `listen` are known episodes only there to compare with).
pub fn split_known<E: Clone>(eps: Vec<E>, known: impl Fn(&E) -> bool) -> (Vec<E>, Vec<E>, usize) {
    let (known, mut listen): (Vec<E>, Vec<E>) = eps.into_iter().partition(|e| known(e));
    // A single new episode has nothing to be compared with but the known ones (their markers stay).
    if listen.len() != 1 || known.is_empty() {
        return (known, listen, 0);
    }
    let skip = known.len();
    listen.splice(0..0, known.iter().cloned());
    (known, listen, skip)
}


/// Movies and shows without details go back in line at start: the line is lost on a restart, and a title
/// missed recently is skipped by match_online until a week has passed (or the matcher learned more, MATCH_VERSION).
pub fn details_retry_list(db: &Db) -> Vec<u64> {
    db.items.values()
        .filter(|i| matches!(i.kind, ItemKind::Movie | ItemKind::Show) && !i.extra.contains_key("metadata_source"))
        .filter(|i| i.extra.get("unmatched_by_hand").and_then(|v| v.as_bool()) != Some(true))
        // (The same test as `recent` in match_online: those would only be turned away.)
        .filter(|i| !(i.extra.get("metadata_status").and_then(|v| v.as_str()) == Some("unmatched")
            && i.extra.get("match_version").and_then(|v| v.as_u64()) == Some(MATCH_VERSION)
            && i.extra.get("metadata_checked_at").and_then(|v| v.as_u64()).is_some_and(|t| now().saturating_sub(t) < 7 * 86400)))
        .map(|i| i.id).collect()
}


pub fn start_background(app: &Arc<App>) {
    // Dashboard samples: processor, memory, bytes sent, streams.
    let a = app.clone();
    std::thread::spawn(move || {
        let mut last_t = crate::stats::times();
        let stats_file = a.data_dir.join("stats.json");
        a.stats.load(&stats_file);
        let mut last_bytes = a.stats.bytes_out.load(std::sync::atomic::Ordering::Relaxed);
        let mut last_remote = a.stats.bytes_remote.load(std::sync::atomic::Ordering::Relaxed);
        let mut taken: u64 = 0;
        let mut last_user: HashMap<u64, u64> = HashMap::new();
        let mut last_at = Instant::now();
        loop {
            std::thread::sleep(Duration::from_secs(crate::stats::SAMPLE_SECONDS));
            let secs = last_at.elapsed().as_secs_f64().max(0.001);
            last_at = Instant::now();
            let t = crate::stats::times();
            let cpu = match (last_t, t) { (Some(x), Some(y)) => crate::stats::cpu_between(x, y), _ => None };
            last_t = t;
            let mem = crate::stats::memory();
            let bytes = a.stats.bytes_out.load(std::sync::atomic::Ordering::Relaxed);
            let out_bps = (bytes.saturating_sub(last_bytes) as f64 / secs) as u64;
            last_bytes = bytes;
            let remote = a.stats.bytes_remote.load(std::sync::atomic::Ordering::Relaxed);
            let out_remote_bps = ((remote.saturating_sub(last_remote) as f64 / secs) as u64).min(out_bps);
            last_remote = remote;
            let mut per_user = HashMap::new();
            for (uid, c) in a.stats.user_bytes.lock().unwrap().iter() {
                let b = c.load(std::sync::atomic::Ordering::Relaxed);
                let before = last_user.insert(*uid, b).unwrap_or(b);
                per_user.insert(*uid, (b.saturating_sub(before) as f64 / secs) as u64);
            }
            *a.stats.user_bps.lock().unwrap() = per_user;
            a.stats.push(crate::stats::Sample {
                at: now(),
                cpu: cpu.map(|c| c.0),
                cpu_server: cpu.map(|c| c.1),
                mem_used: mem.as_ref().map(|m| m.used),
                mem_total: mem.as_ref().map(|m| m.total),
                mem_server: mem.as_ref().map(|m| m.process).filter(|p| *p > 0),
                out_bps,
                out_remote_bps,
                streams: a.stream_count(),
                transcodes: a.active_transcodes(),
            });
            // The minute and hour averages go to disk every five minutes.
            taken += 1;
            if taken % (300 / crate::stats::SAMPLE_SECONDS) == 0 {
                a.stats.save(&stats_file);
            }
        }
    });
    // Stop idle HLS transcodes and clean their folders.
    let a = app.clone();
    std::thread::spawn(move || loop {
        std::thread::sleep(Duration::from_secs(5));
        let mut stale = vec![];
        {
            let mut map = a.hls.lock().unwrap();
            let ids: Vec<String> = map.iter().filter(|(_, s)| s.last_access.elapsed() > Duration::from_secs(90)).map(|(k, _)| k.clone()).collect();
            for id in ids {
                if let Some(s) = map.remove(&id) {
                    stale.push(s);
                }
            }
        }
        for mut s in stale {
            let _ = s.child.kill();
            let _ = s.child.wait();
            let _ = std::fs::remove_dir_all(&s.dir);
        }
        // Players that stopped reporting: their play has ended.
        let gone: Vec<Playing> = {
            let mut map = a.playing.lock().unwrap();
            let keys: Vec<String> = map.iter().filter(|(_, p)| p.updated.elapsed() >= Duration::from_secs(120)).map(|(k, _)| k.clone()).collect();
            keys.iter().filter_map(|k| map.remove(k)).collect()
        };
        a.close_sessions(gone);
    });
    // Online details for new movies and shows.
    let a = app.clone();
    for _ in 0..3 { let a = a.clone(); std::thread::spawn(move || a.metadata_worker()); }
    // Converting downloads, one after the other.
    crate::downloads::start(app);
    // Watched state with a Plex server, both ways (when switched on).
    crate::plexsync::start(app);
    crate::trakt::start(app);
    let retry = details_retry_list(&app.db.lock().unwrap());
    if !retry.is_empty() {
        app.queue_online_details(&retry, false);
    }
    // Watch the library folders: when the files in a library change (added,
    // removed, renamed), wait until they stop changing (copying finished) and
    // scan that library. Only changed files are read again.
    let a = app.clone();
    std::thread::spawn(move || {
        let mut last: HashMap<u64, u64> = HashMap::new();
        let mut changed: HashSet<u64> = HashSet::new();
        loop {
            // (In 1 s steps, so a changed interval takes effect at once.)
            let mut waited = 0;
            while waited < WATCH_SECONDS.load(std::sync::atomic::Ordering::Relaxed) {
                std::thread::sleep(Duration::from_secs(1));
                waited += 1;
            }
            let (on, sections) = {
                let db = a.db.lock().unwrap();
                (db.settings.watch_folders, db.sections.clone())
            };
            if !on {
                last.clear();
                changed.clear();
                continue;
            }
            last.retain(|id, _| sections.iter().any(|s| s.id == *id));
            for s in sections {
                let sig = scanner::folder_signature(&s.paths, s.kind == db::SectionKind::Music);
                match last.insert(s.id, sig) {
                    // First look (also catches files added while the server was off).
                    None => {
                        changed.insert(s.id);
                    }
                    // Still changing: wait one more round.
                    Some(old) if old != sig => {
                        changed.insert(s.id);
                    }
                    Some(_) => {
                        if changed.contains(&s.id) && a.scan_now(s.id).is_some() {
                            changed.remove(&s.id);
                        }
                    }
                }
            }
        }
    });
    // Credits detection "as soon as possible" (within a minute of new files; "scheduled" runs
    // in maintenance::tick) and IMDb ratings (weekly file).
    let a = app.clone();
    std::thread::spawn(move || {
        let mut imdb_retry = Instant::now();
        loop {
            std::thread::sleep(Duration::from_secs(60));
            let due = a.settings().credits_markers_behaviour == "asap";
            // Credits and intros come last: only when no details are waiting or being fetched.
            let idle = a.meta_queue.lock().unwrap().is_empty() && a.meta_busy.lock().unwrap().is_empty() && a.job.lock().unwrap().is_none();
            let paused = a.auto_paused.load(AtomicOrdering::SeqCst);
            if due && idle && !paused {
                let pending = a.credits_pending();
                if !pending.is_empty() {
                    let _ = a.detect_credits(pending, false);
                    continue;
                }
            }
            // Intro detection: the same choices ("Generate intro video markers").
            let intro_due = a.settings().intro_markers_behaviour == "asap";
            if intro_due && idle && !paused {
                let pending = a.intros_pending();
                if !pending.is_empty() {
                    let _ = a.detect_intros(pending, false);
                    continue;
                }
            }
            if idle && Instant::now() >= imdb_retry && a.settings().imdb_ratings_file {
                if let Err(e) = a.update_imdb_ratings(false) {
                    elog!("IMDb ratings: {e}");
                    imdb_retry = Instant::now() + Duration::from_secs(6 * 3600);
                }
            }
        }
    });
    // Scheduled maintenance: backup, optimize, clean up (inside the maintenance window).
    let a = app.clone();
    std::thread::spawn(move || loop {
        std::thread::sleep(Duration::from_secs(60));
        crate::maintenance::tick(&a);
    });
    // Once a day: schedule.daily for plugins. The last date goes in a file, so
    // a restart does not send it twice and a day missed while off is sent at start.
    let a = app.clone();
    std::thread::spawn(move || loop {
        a.daily_tick();
        std::thread::sleep(Duration::from_secs(60));
    });
    // Plays that stay paused too long are ended.
    let a = app.clone();
    std::thread::spawn(move || loop {
        std::thread::sleep(Duration::from_secs(30));
        let minutes = a.settings().paused_stop_minutes;
        if minutes == 0 { continue; }
        let stale: Vec<(u64, u64)> = a.playing.lock().unwrap().values()
            .filter(|p| p.state == "paused" && p.updated.elapsed() >= Duration::from_secs(minutes as u64 * 60)).map(|p| (p.user_id, p.item_id)).collect();
        for (u, i) in stale { a.stop_playback(u, i, "Playback was ended because it stayed paused for too long"); }
    });
    // Scheduled library scans.
    let a = app.clone();
    std::thread::spawn(move || {
        let mut last: HashMap<u64, Instant> = HashMap::new();
        loop {
            std::thread::sleep(Duration::from_secs(30));
            let (enabled, global, ids) = {
                let db = a.db.lock().unwrap();
                (db.settings.scheduled_scans_enabled, db.settings.scan_interval_minutes, db.sections.iter().map(|s| (s.id, s.scan_interval_minutes)).collect::<Vec<_>>())
            };
            if !enabled {
                continue;
            }
            for (id, own) in ids {
                let interval = own.unwrap_or(global);
                if interval == 0 { continue; }
                let due = last.get(&id).is_none_or(|t| t.elapsed() >= Duration::from_secs(interval * 60));
                if due {
                    last.insert(id, Instant::now());
                    a.scan_now(id);
                }
            }
        }
    });
}


#[cfg(test)]
mod tests {
    use super::*;
    use crate::server::tests::test_app;

    fn item(id: u64, kind: &str, extra: Value) -> Item {
        serde_json::from_value(json!({"id": id, "section_id": 1, "kind": kind, "title": "x", "sort_title": "x", "added_at": 0, "updated_at": 0, "extra": extra})).unwrap()
    }

    #[cfg(unix)]
    #[test]
    fn many_new_titles_do_not_start_a_plugin_for_each_at_once() {
        let (app, root) = test_app("plugin-storm");
        let dir = root.join("plugins").join("counter");
        std::fs::create_dir_all(dir.join("run")).unwrap();
        // Records how many copies of itself are running.
        std::fs::write(dir.join("plugin.json"), r#"{"id": "counter", "name": "counter", "hooks": ["library.item.added"], "command": ["sh", "-c", "touch run/$$; ls run | wc -l >> log; sleep 0.01; rm run/$$"]}"#).unwrap();
        app.plugins.lock().unwrap().reload();
        app.db.lock().unwrap().plugins.insert("counter".into(), serde_json::from_value(json!({"enabled": true})).unwrap());
        for id in 0..500 { app.fire("library.item.added", json!({"id": id})); }
        let log = dir.join("log");
        let lines = || std::fs::read_to_string(&log).unwrap_or_default().lines().count();
        for _ in 0..600 { if lines() >= 500 { break; } std::thread::sleep(Duration::from_millis(50)); }
        let most = std::fs::read_to_string(&log).unwrap_or_default().lines().filter_map(|l| l.trim().parse::<usize>().ok()).max().unwrap_or(0);
        let n = lines();
        let _ = std::fs::remove_dir_all(&root);
        assert_eq!(n, 500, "not every event reached the plugin");
        assert!(most <= crate::plugins::MAX_RUNNING, "{most} plugin programs ran at once");
    }

    #[test]
    fn a_new_title_tells_plugins_its_files_and_streams() {
        let (app, root) = test_app("item-detail");
        let mut m = item(7, "movie", json!({"ids": {"imdb": "tt0133093"}}));
        m.year = Some(1999);
        m.file = Some(serde_json::from_value(json!({"path": "/media/Matrix (1999).mkv", "size": 5, "modified": 0, "container": "mkv",
            "streams": [{"kind": "audio", "index": 1, "codec": "ac3", "language": "ger", "channels": 6}], "external_subtitles": ["/media/Matrix (1999).en.srt"]})).unwrap());
        app.db.lock().unwrap().items.insert(7, m);
        let d = app.item_detail(7);
        let _ = std::fs::remove_dir_all(&root);
        assert_eq!(d["type"], "movie");
        assert_eq!(d["ids"]["imdb"], "tt0133093");
        assert_eq!(d["files"][0]["path"], "/media/Matrix (1999).mkv");
        assert_eq!(d["files"][0]["streams"][0]["language"], "ger");
        assert_eq!(d["files"][0]["external_subtitles"][0], "/media/Matrix (1999).en.srt");
        assert!(app.item_detail(99).is_null());
    }

    #[test]
    fn subtitles_a_plugin_saved_in_the_data_folder_are_found_and_may_be_sent() {
        let (app, root) = test_app("substore");
        let media = root.join("media").join("Heat (1995)");
        std::fs::create_dir_all(&media).unwrap();
        let video = media.join("Heat (1995).mkv");
        std::fs::write(&video, b"x").unwrap();
        let mut m = item(9, "movie", json!({}));
        m.file = Some(serde_json::from_value(json!({"path": video.to_string_lossy(), "size": 1, "modified": 0, "container": "mkv"})).unwrap());
        app.db.lock().unwrap().items.insert(9, m);
        let dir = crate::scanner::store_dir(&app.data_dir.join("subtitles"), &media);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("Heat (1995).de.srt"), b"1").unwrap();
        app.refresh_subtitles(9);
        let subs = app.db.lock().unwrap().items.get(&9).unwrap().file.as_ref().unwrap().external_subtitles.clone();
        let sendable = subs.first().is_some_and(|p| app.may_send(Path::new(p), false));
        let elsewhere = app.may_send(&app.data_dir.join("library.db"), false);
        let _ = std::fs::remove_dir_all(&root);
        assert_eq!(subs, vec![dir.join("Heat (1995).de.srt").to_string_lossy().to_string()]);
        assert!(sendable && !elsewhere);
    }

    #[test]
    fn a_plugin_that_reads_the_library_gets_every_title_with_its_key_and_no_other_does() {
        let (app, root) = test_app("plugin-library");
        let mut m = item(7, "movie", json!({}));
        m.file = Some(serde_json::from_value(json!({"path": "/media/Heat (1995).mkv", "size": 5, "modified": 0, "container": "mkv"})).unwrap());
        let gone = item(8, "movie", json!({crate::scanner::MISSING: 1}));
        let show = item(9, "show", json!({}));
        for i in [m, gone, show] { app.db.lock().unwrap().items.insert(i.id, i); }
        let manifest = |id: &str, reads: bool, admins: bool| -> plugins::Manifest { serde_json::from_value(json!({"id": id, "name": id, "version": "1", "command": ["x"],
            "user_panel": true, "panel_admins_only": admins, "reads_library": reads})).unwrap() };
        {
            let mut p = app.plugins.lock().unwrap();
            p.list.push(plugins::Plugin { manifest: manifest("doctor", true, true), dir: root.clone() });
            p.list.push(plugins::Plugin { manifest: manifest("other", false, false), dir: root.clone() });
            let mut db = app.db.lock().unwrap();
            for id in ["doctor", "other"] { db.plugins.entry(id.into()).or_default().enabled = true; }
        }
        let (doctor, other) = { let p = app.plugins.lock().unwrap(); (p.get("doctor").cloned().unwrap(), p.get("other").cloned().unwrap()) };
        let address = app.server_info_for(&doctor)["library"].as_str().unwrap_or("").to_string();
        let key = address.split("key=").nth(1).unwrap_or("").to_string();
        let ask = |pid: &str, key: &str| {
            let ctx = Ctx { method: "GET".into(), path: format!("/api/plugins/{pid}/library"), params: vec![("key".into(), key.into())], body: Value::Null,
                user: None, host: String::new(), range: None, token: None, remote: Some([127, 0, 0, 1].into()), remote_trusted: true, lan_extra: false, device: "t".into() };
            crate::handlers::plugin_library(&app, &ctx, pid)
        };
        let listed = ask("doctor", &key);
        let codes = [ask("doctor", "wrong").status_code().0, ask("other", &key).status_code().0, ask("doctor", "").status_code().0];
        let (admin, kid) = { let db = app.db.lock().unwrap(); (db.users[0].clone(), db.users[1].clone()) };
        let panels = |u: &User| { let r = crate::handlers::plugin_panels(&app, u); let mut b = Vec::new(); r.into_reader().read_to_end(&mut b).unwrap(); serde_json::from_slice::<Value>(&b).unwrap()["panels"].as_array().map(|a| a.len()).unwrap_or(0) };
        let (for_admin, for_kid) = (panels(&admin), panels(&kid));
        let mut body = Vec::new();
        listed.into_reader().read_to_end(&mut body).unwrap();
        let v: Value = serde_json::from_slice(&body).unwrap_or(Value::Null);
        let _ = std::fs::remove_dir_all(&root);
        assert!(address.contains("/api/plugins/doctor/library?key=") && !key.is_empty());
        assert!(app.server_info_for(&other).get("library").is_none());
        assert_eq!(v["items"].as_array().map(|a| a.len()), Some(1));
        assert_eq!(v["items"][0]["files"][0]["path"], "/media/Heat (1995).mkv");
        assert_eq!(codes, [403, 403, 403]);
        assert_eq!((for_admin, for_kid), (2, 1));
    }

    #[test]
    fn schedule_daily_is_sent_once_a_day_and_after_a_restart_only_if_missed() {
        let (app, root) = test_app("daily");
        let first = app.daily_tick();
        let again = app.daily_tick();
        let stored = std::fs::read_to_string(root.join("plugins").join(".daily-last")).unwrap_or_default();
        std::fs::write(root.join("plugins").join(".daily-last"), "2000-01-01").unwrap();
        let missed = app.daily_tick();
        let _ = std::fs::remove_dir_all(&root);
        assert!(first && !again && missed);
        assert_eq!(stored, crate::db::today());
    }

    #[test]
    fn titles_without_details_are_queued_again_at_start() {
        let mut db = Db::default();
        let old = MATCH_VERSION - 1;
        for i in [item(1, "movie", json!({})), item(2, "show", json!({})), item(3, "movie", json!({"metadata_status": "unmatched", "match_version": old})),
            item(4, "movie", json!({"metadata_source": "tmdb", "metadata_id": "1"})), item(5, "movie", json!({"unmatched_by_hand": true})), item(6, "episode", json!({})),
            // Missed by this matcher: not again within a week.
            item(7, "movie", json!({"metadata_status": "unmatched", "match_version": MATCH_VERSION, "metadata_checked_at": now() - 86400})),
            item(8, "movie", json!({"metadata_status": "unmatched", "match_version": MATCH_VERSION, "metadata_checked_at": now() - 8 * 86400}))] {
            db.items.insert(i.id, i);
        }
        let mut got = details_retry_list(&db);
        got.sort();
        assert_eq!(got, vec![1, 2, 3, 8]);
    }

    #[test]
    fn a_title_a_worker_is_busy_with_waits_for_it() {
        let (app, root) = test_app("meta-busy");
        app.db.lock().unwrap().items.insert(7, item(7, "movie", json!({})));
        app.meta_busy.lock().unwrap().push(7);
        app.meta_queue.lock().unwrap().push_back((7, true));
        let a = app.clone();
        std::thread::spawn(move || a.metadata_worker());
        std::thread::sleep(Duration::from_millis(1500));
        let (queued, done) = (app.meta_queue.lock().unwrap().iter().any(|q| q.0 == 7), app.meta_progress.lock().unwrap().done);
        let _ = std::fs::remove_dir_all(&root);
        assert_eq!(done, 0, "a second worker looked the title up while another one was on it");
        assert!(queued, "the title was dropped instead of waiting");
    }

    #[test]
    fn a_new_episode_is_compared_with_the_episodes_plex_marked() {
        // 9 episodes with the old server's marker and 1 new one: the new one is listened to with them.
        let (known, listen, skip) = split_known((1..=10).collect::<Vec<u32>>(), |e| *e <= 9);
        assert_eq!((known.len(), listen.len(), skip), (9, 10, 9));
        // Two new ones are enough by themselves.
        let (known, listen, skip) = split_known((1..=5).collect::<Vec<u32>>(), |e| *e <= 3);
        assert_eq!((known.len(), listen, skip), (3, vec![4, 5], 0));
        // A single episode with nothing to compare with stays alone.
        let (_, listen, skip) = split_known(vec![1u32], |_| false);
        assert_eq!((listen, skip), (vec![1], 0));
    }

    #[test]
    fn a_failed_imdb_download_with_an_old_file_is_reported_so_it_waits() {
        let (app, root) = test_app("imdb-stale");
        let file = app.data_dir.join("cache").join("imdb-ratings.tsv.gz");
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        {
            use std::io::Write;
            let mut gz = flate2::write::GzEncoder::new(std::fs::File::create(&file).unwrap(), flate2::Compression::default());
            gz.write_all(b"tconst\taverageRating\tnumVotes\ntt0000001\t7.5\t100\n").unwrap();
        }
        std::fs::File::options().write(true).open(&file).unwrap().set_modified(std::time::SystemTime::now() - Duration::from_secs(30 * 86400)).unwrap();
        app.db.lock().unwrap().items.insert(1, item(1, "movie", json!({"ids": {"imdb": "tt0000001"}})));
        let r = app.imdb_update(true, |_| Err("offline".into()));
        let rating = app.db.lock().unwrap().items[&1].extra.get("ratings").cloned();
        let _ = std::fs::remove_dir_all(&root);
        assert!(r.is_err(), "the scheduler would try again in a minute: {r:?}");
        assert!(rating.is_some(), "the old file's ratings were not used");
    }
}

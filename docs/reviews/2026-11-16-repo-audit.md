# Repo audit, 2026-11-16

Status: all findings handled, see [the review overview](../review-2026-11-15.md).

Branch `review/ponytail-fixes` at `96ff123`. Read only, no code changed. `cargo test --release --no-default-features`: 130 passed, 0 failed, 4 ignored.

What this repo does: Lichtspiel is a home streaming server that replaces Plex. One Rust program scans movie and TV folders, fetches details online, plays or converts videos with ffmpeg, and serves a plain-JS web app, a JSON API and a Flutter app. It runs in Docker on a Synology NAS, and every push to `main` deploys (the container is replaced, so the server restarts often).

Load I assumed: one household, up to 10 users, about 5 players at once, 5k–50k items, used at home and over the internet. Plugins and the Plex import or sync are switched on at some point.

Line numbers are from `96ff123`. None of the findings below repeat an earlier ID unless marked "(known: …)".

## Must fix

### A1. Restoring a backup hands out old item numbers again, so new titles show other titles' pictures, and Plex sync can undo "watched" on Plex (`src/maintenance.rs:163-187`, `restore()`)

Status: Fixed in `bd1f462`.

- **What this is:** "Restore" puts an older copy of the library back in place, including the counter that hands out item numbers (`next_id`, `src/db.rs:1296`).
- **Problem:** After restoring a 3-day-old backup, the next titles found get numbers that titles added in those 3 days had. Files named after the number are still on disk: `cache/thumbs/{id}.jpg`, `cache/art/{id}.jpg`, the seek-bar pictures `cache/frames/{id}-{t}.jpg` (`make_frame` reuses any file that exists, `handlers.rs:952`) and `metadata/{id}/still.jpg` (taken as it is, `server.rs:1344-1346`). So a new episode shows another video's poster, still and seek-bar previews. Plex sync also keys what both sides agreed on by item number (`plex-sync-state.json`, "user:item"). After a restore, a title that is watched on Plex but not here looks "unwatched here, unchanged on Plex", so the next sync sends an unwatch to Plex (`plexsync.rs` `pick()`). That hits every title watched since the backup, and any reused number.
- **Fix:** In `restore()`, keep the counter going forward with `loaded.next_id = loaded.next_id.max(app.db.lock().unwrap().next_id);` before swapping. Then delete `plex-sync-state.json`, so the next sync merges like a first run (watched on either side stays watched). That is two lines.
- **If we skip it:** Wrong pictures after every restore. With Plex sync on, Plex loses what was watched since the backup, and that is the copy people trust to recover.

### A2. With the webhook plugin on, a big scan or import starts one thread and one Python process per new title, all at once (`src/server.rs:547-552` in `scan_folder_now`; `fire()` at `:454-486`; `plugins.rs:142-200`)

Status: Fixed in `bd1f462`.

- **What this is:** After a scan, each added item fires `library.item.added`. `fire()` spawns a new thread for every event, and each thread starts the plugin's program.
- **Problem:** The bundled webhook plugin listens to `library.item.added` by default (`plugins/webhook-notify/plugin.json`). Picture a first scan or a Plex import of 20,000 episodes: 20,000 threads start at the same moment, each launching `python3 main.py` (about 10 MB each). The NAS runs out of memory or processes, and Discord/ntfy gets 20,000 posts. There is no limit anywhere on how many plugin runs happen at once.
- **Fix:** In `scan_folder_now`, send the per-item events from one background thread, one after the other (or one `library.scan.finished` with the count, which already exists, and skip per-item events when `added.len()` is large). About 5 lines, no new code path.
- **If we skip it:** Turning on notifications and then importing from Plex can bring down the NAS.

### A3. Titles that miss their details once (internet down, or a restart before their turn) never get them automatically (`src/server.rs:1255-1266` in `match_online`; start-up retry `:2343-2355`; queue `meta_queue` in memory)

Status: Fixed in `bd1f462`.

- **What this is:** New movies and shows go into an in-memory queue, and 3 workers look each one up online.
- **Problem:** Two cases. First, when the online search fails because the network is down, `search()` returns an error. `match_online` still marks the title `"unmatched"` with today's date and the current `MATCH_VERSION`, so it is skipped for 7 days. After that nothing puts it back in the queue: the start-up retry only takes titles with an older `MATCH_VERSION`, and the 3-day "refresh" task only rescans files (`maintenance.rs:297`), which queues only new files. Second, the queue is lost on restart, and deploys restart the server often. A first scan of 20k titles that is cut off half-way leaves the rest with no `metadata_status` at all, and nothing ever queues them again. Concrete case: the router reboots during a scan that adds a season, and those episodes' show stays without a poster until someone presses "Refresh metadata".
- **Fix:** (a) Only write `"unmatched"` when the search really answered (`found` is `Ok`), not on `Err`. (b) At start, queue movies and shows that have no `metadata_source` and are not `unmatched_by_hand` (where the current retry list is built). That is one filter change and one extra filter.
- **If we skip it:** Posterless titles pile up after every outage or deploy, and nothing in the UI says why.

### A4. Optimized copies (gigabytes each), downloaded pictures and caches of removed titles stay on disk for good, where nobody can see them (`src/actions.rs:235-356`; `src/server.rs:1163`; `src/maintenance.rs:40-70`)

Status: Fixed in `bd1f462`.

- **What this is:** "Optimize" writes `data/optimized/{id}.mp4`, and online details write `data/metadata/{id}/` (poster, background, stills, cast).
- **Problem:** These files are only deleted by "Delete optimized" and by "Delete" from the ⋯ menu. A title can also leave the library because its file was replaced or renamed (new item number), the trash was emptied, or the library was removed. In those cases the files stay. "Optimized versions" in Maintenance lists only titles still in the library (`optimized_list` walks `db.items`), so "Delete all optimized versions" cannot see the leftovers either. The weekly clean-up only looks at `cache/` and `transcode/`. Concrete case: optimize a season of 10 episodes (about 15 GB), then upgrade the files to better versions. The old 15 GB stay in `/config/optimized` with no way to find them in the app.
- **Fix:** In `cleanup()`, delete `optimized/{n}.mp4` and `metadata/{n}/` whose number `n` is not in `db.items` (one `read_dir` each, about 15 lines). Leave `metadata/people/` alone.
- **If we skip it:** The config volume fills up slowly and silently. When it is full, library saves fail.

### A5. Running the Plex import a second time doubles the play history, and the app tells people to do exactly that (`src/migrate.rs:677-692`; message `src/routes.rs:727`)

Status: Fixed in `bd1f462`.

- **What this is:** The import copies every viewing Plex logged into the play history (`record_play_at`), which feeds the dashboard charts, "most played" and play counts.
- **Problem:** `record_play_at` always adds an entry, and the "already logged" set only lives for one run. A second import adds every viewing again. A cancelled import says "import again to continue", and cancelling after step 3 has already added the history in memory (it is saved later), so following that advice doubles it. Two runs show every film played twice, and "top users" and hours watched are inflated for good. History is capped at 20,000 entries, so the duplicates also push out real older plays.
- **Fix:** Before step 3, collect the existing `(user, item, at)` triples from `db.history` into a `HashSet` and skip any entry already there. About 3 lines.
- **If we skip it:** The dashboard history is wrong after a second import and cannot be cleaned without editing the database.

## Should fix

### A6. A show whose old episodes have Plex intro markers never gets an intro for a new episode (`src/server.rs:932-958`, `detect_intros`)

Status: Fixed in `bd1f462`.

- **What this is:** Intro detection compares the sound of the episodes of a season. Episodes that have a marker from the old Plex database take that marker and are not listened to.
- **Problem:** The episodes that have Plex markers are split off first (`partition`). Picture an imported show with Plex markers on 9 episodes, plus 1 new episode. Only 1 episode is left, which hits "a single episode left has nothing to be compared with", so the new episode is marked checked with no intro and never tried again. Every new weekly episode of an imported show ends up without "Skip intro".
- **Fix:** When fewer than 2 unknown episodes remain but `known` is not empty, fingerprint the known ones too (cached, `cache/intro`) and compare with them, keeping their Plex markers. Or simply don't split off the known episodes when `eps.len() < 2`. A few lines.
- **If we skip it:** "Skip intro" disappears for everything added after the move from Plex.

### A7. The same movie in two Plex libraries (for example "Movies" and "Movies 4K") gets its watched state on only one copy (`src/migrate.rs:631`)

Status: Fixed in `bd1f462`.

- **What this is:** Plex keeps watched state and ratings per guid. The import maps each guid to one item here.
- **Problem:** `by_guid` is a `HashMap<&str, u64>`, so when two old items share a guid (Plex's new agent gives the same `plex://movie/…` to both copies), the last one wins. Plex shows the film as watched in both libraries, but after the import only one copy is watched here, and the other keeps showing in "Unwatched" and in smart playlists.
- **Fix:** Make it `HashMap<&str, Vec<u64>>` and loop over the list in the settings, history and ratings steps. About 6 lines.
- **If we skip it:** People with a 4K library see half their watched films as unwatched after moving from Plex.

### A8. Two details workers can work on the same show at once and write half a poster (`src/server.rs:649-672` `queue_online_details`, `:1528-1570` `metadata_worker`; `metadata.rs:183-185`)

Status: Fixed in `bd1f462`.

- **What this is:** 3 workers take titles from the queue. The queue skips a title that is already waiting, but not one that a worker is working on right now (`meta_busy`).
- **Problem:** Episodes copied into a show folder one by one cause a scan after each pause, and each scan queues the show again. A big show takes minutes (stills for every episode), so a second worker takes the same show while the first is still on it. Both download to the same `poster.part` / `still.part` and rename it. One can rename the other's half-written file into place, and both delete `cast/` under each other. The result is a cut-off poster, which still passes the "is it a JPEG" check.
- **Fix:** In `metadata_worker`, if the popped id is in `meta_busy`, push it to the back of the queue and continue. Two lines.
- **If we skip it:** Now and then a show has a broken or half-grey poster until someone refreshes it by hand.

### A9. "Credits/intro markers: as a scheduled task" runs 24 hours after each start, at any hour, and never runs on a server that restarts daily (`src/server.rs:2400-2440`)

Status: Fixed in `bd1f462`.

- **What this is:** The setting says "as a scheduled task", and every other scheduled task waits for the maintenance window (`maintenance::tick`).
- **Problem:** These two use `Instant::now()` at start plus 24 h, with no window check. On the NAS every push to `main` restarts the server, so with one deploy a day they never run. Without deploys they start 24 h after the last restart, for example at 20:00 during movie night, with up to 4 ffmpeg processes.
- **Fix:** Move the two "scheduled" cases into `maintenance::tick` with `due("credits", 1)` / `due("intros", 1)` (stored in `maintenance_last`, so restarts don't reset them) and delete the two `Instant` timers.
- **If we skip it:** People who picked "scheduled" to keep evenings quiet get either no markers or a busy NAS in the evening.

### A10. Credits and intro detection save only at the very end, so a restart throws away hours of work (`src/server.rs:797`, `:1024`)

Status: Fixed in `bd1f462`.

- **What this is:** These jobs run over thousands of files (4 at a time) and change each item in memory.
- **Problem:** `app.save()` is called once, when the whole job ends. A first run over 50k videos takes days. Other saves (someone playing) carry the progress along, but a quiet night has none, and a deploy in the morning restarts the server. Everything done since the last unrelated save is lost and detected again.
- **Fix:** Call `app.save()` every 50 files in the credits loop and after each season in the intro loop. One line each.
- **If we skip it:** Each deploy during a big first detection wastes hours of NAS CPU, and the job may never finish on a busy deploy day.

### A11. Every credits or intro run copies the whole old Plex database (`src/server.rs:739-746`, `:924-928`; `migrate.rs:197-209`)

Status: Fixed in `bd1f462`.

- **What this is:** When an old Plex database is found (Docker mounts `/oldserver`), each detection job first reads Plex's markers from a fresh copy in `cache/old-markers`.
- **Problem:** `open_copy` always copies the file. Plex databases of large libraries are hundreds of MB to a few GB. With the default "asap", a job starts within a minute of new files, steps aside when details arrive (`yield_to_details`) and starts again later, each time copying the database again, once for credits and once for intros. On a NAS hard disk that is tens of seconds of full disk load each time, for data that never changes after the move.
- **Fix:** In `open_copy`, skip the copy when the existing copy is newer than the source (compare the modified times). Three lines.
- **If we skip it:** Needless gigabytes of disk traffic per day while new files keep arriving.

### A12. Folder watching walks every file of every library every 15 seconds (`src/server.rs:2360-2398`; `scanner.rs:166-185` `folder_signature`)

Status: Fixed in `bd1f462`.

- **What this is:** "Watch the library folders" (on by default) notices new or changed files by hashing path, size and time of every file.
- **Problem:** With 50k files that is about 50k `stat` calls plus a `readdir` per folder every 15 s, all day (about 3,000 per second on average, estimated from the code, not measured). On a Synology with hard disks this keeps the disks from ever sleeping and uses CPU for nothing on a library that changes a few times a week.
- **Fix:** Raise the interval to 60 s (the `WATCH_SECONDS` default), or first compare only the folders' own modified times and walk the files only when one changed. The first is a one-number change.
- **If we skip it:** Constant disk wake-ups and background load on the NAS. Measure it on the real NAS before choosing.

### A13. The weekly clean-up deletes the seek-bar pictures that "Create preview pictures" made, and they are never made again (`src/maintenance.rs:56-67` `old_files`, `:239-268` `make_thumbnails`)

Status: Fixed in `bd1f462`.

- **What this is:** "Create preview pictures" makes all seek-bar pictures ahead of time and marks the item `thumbnails_done`. "Remove old cache files" (on by default) deletes every file in `cache/` older than 7 days.
- **Problem:** `cache/frames` is inside `cache/`, so a week later the pictures are gone, but `thumbnails_done` stays set, so they are never made ahead again. The setting only helps for one week. The same clean-up also deletes the intro fingerprints (`cache/intro`), so a new episode makes the whole season be listened to again.
- **Fix:** Skip `cache/frames` and `cache/intro` in `old_files` (or only delete files of items that are gone, together with A4). One condition.
- **If we skip it:** The setting costs NAS time every week and gives nothing after the first week.

## Nice to have

### A14. The maintenance window moves by an hour when the clocks change (`src/maintenance.rs:115-119`, `:287-288`; `web/pages.js:1013`)

Status: Fixed in `5357b4c`.

- **What this is:** The window ("2–5 local time") is turned into server time with a fixed `utc_offset_minutes`, sent once when the admin saves the "Scheduled tasks" tab.
- **Problem:** In Rome, a window saved in summer (+120) runs from 01:00 to 04:00 in winter, until someone saves that tab again. Backups, optimizing and clean-up then start an hour earlier than set.
- **Fix:** Send `utc_offset_minutes` with every settings save (drop the `tab === 'tasks'` condition). It is still a fixed offset, but it is corrected on any admin visit. One line.
- **If we skip it:** An hour's drift twice a year. Mostly harmless.

### A15. With a stale IMDb file and no internet, the IMDb update runs every minute (`src/server.rs:1114-1121`, scheduler `:2437-2442`)

Status: Fixed in `bd1f462`.

- **What this is:** The weekly IMDb ratings file is downloaded again when it is older than 7 days.
- **Problem:** If the download fails but an old file exists, the error is ignored and the run "succeeds" with the old file, so the 6-hour back-off never starts. The next minute the file is still stale, so it tries again: a download attempt, a full read of the 9 MB file and a save every minute while offline, plus a new "IMDb ratings updated" message each time.
- **Fix:** Return the download error (or set `imdb_retry`) when the stale file could not be refreshed. One line.
- **If we skip it:** Wasted CPU and log noise during an outage.

### A16. Four copies of the same calendar-date code (`src/db.rs:12-24` `today`, `src/migrate.rs:404-416` `db_date`, `src/rss.rs:21-32` `civil`; inverse in `query.rs:~270-280` and `migrate.rs:168-177`)

Status: Fixed in `bd1f462`.

- **What this is:** "days since 1970 → year, month, day" (Hinnant's method) is written out three times, and the reverse twice.
- **Problem:** These are near-copies that must stay identical. `log.rs` already reuses `rss::civil`, which shows the shared helper exists.
- **Fix:** Make `db::today` and `migrate::db_date` call `rss::civil` (or move `civil` into `db.rs`). About 20 lines fewer.
- **If we skip it:** Only extra code to read. No bug today (all copies pass their tests).

Left out: 4 smaller ones. Plex sync and the import insert history entries one by one into a 20k list while holding the library lock (a few seconds once). `cache/thumbs/{id}_w{width}` takes any width from the address. Seasonal home rows use the UTC date. HLS folders left by a crash wait for the weekly clean-up. Re-reported known items: none. `cupertino_icons` is still unused (known: C18).

Checked and fine: folder watching, scheduled scans, maintenance "refresh" and the import never scan the same library twice at once (`scanning` set). Plex sync has a run guard (`RUNNING`) and no redirects. `server.log` rotates at 1 MB (2 MB at most). The dashboard keeps bounded samples. History and sessions are capped (20k / 50k). The downloads queue has one worker and removes `.part` files on start. Backups use `VACUUM INTO` (a consistent copy) and keep the newest N. Restore checks the name for path tricks. Every Cargo dependency is used (grep over `src/`).

Verdict: The core is solid and well tested (130 tests pass). Fix A1 (restore) and A2 (plugin storm) first, then A3 and A4, which quietly lose details and disk space at this load.

Lean: -20 lines, -1 dependency possible (`cupertino_icons`, known C18).

Not checked: `music.rs`, `subsync.rs`, `related.rs`, `deep.rs` and `intro.rs` internals (only their callers and jobs), `transcode.rs` command lines, `console.rs`, `selftest.rs`, `relay.rs`, the Flutter client and the plugins (covered by the diff reviews), and most of `web/pages.js` beyond settings, tasks, backups, Plex sync and the dashboard loop. Nothing was run on the NAS. A12's cost is estimated from the code, not measured, and `--self-test` was not run.

# Branch review: server, 2026-11-16

Status: all findings handled, see [the review overview](../review-2026-11-15.md).

Scope: `git diff 5b30e62 -- src Cargo.toml Cargo.lock vendor/tiny_http/src`.

Assumed load: a household server with up to 10 users, about 5 players at once, 5k–50k items, on the LAN and possibly reachable from the internet (Docker on a Synology).
State reviewed: HEAD 96ff123.

**What this change does:** It hardens the server. Sign-in tokens are now stored only as a SHA-256 fingerprint. Picture, video and download addresses carry a short-lived signed "ticket" (an HMAC-signed note naming the user, the item and the sign-in) instead of the token. Requests from other websites are refused, and so is the "no sign-in network" admin for them. Files are only sent when they really lie inside a library folder (`may_send`/`inside_folders`, which follow links). The relay must now prove it knows the secret, and relayed callers are never treated as local. It also adds limits: request headers (through a vendored tiny_http with two small patches), ffmpeg picture jobs (3 at a time), HLS streams per person (4), the transcode-limit race (`STARTING`), one-time 2FA codes (`use_step`) and open problem reports. Saves now write the play history only when it changed, and full writes keep rows that could not be read.

## Must fix

### S-R1. Restoring an older backup breaks every picture and video until a restart (`src/maintenance.rs:173-180`, `src/server.rs:202`)

Status: Fixed in `bd1f462`.

- **What this is:** Restore loads a backup and puts it in place of the live library. On start, `App::open` creates a `ticket_key` when there is none, but restore does not.
- **Problem:** A backup made before this branch has no `ticket_key` row, so `Store::load` gives `""`. After the restore, `auth::ticket` signs with the empty key, and `read_ticket` refuses every ticket because of its `key.is_empty()` check. Every poster, video, subtitle and download in the web app then answers 401 until the server restarts. The existing test makes its backup with the current version, so it already has a key and does not catch this.
- **Fix:** In `restore`, keep the live key: `loaded.ticket_key = app.db.lock().unwrap().ticket_key.clone();` (this also keeps tickets already handed out working). A cleaner option is to move this and `hash_old_tokens()` into one `Db::upgrade()` that both `App::open` and `restore` call. Extend `an_old_backup_keeps_its_sign_ins_as_hashes` so it clears `ticket_key` before taking the backup.
- **If we skip it:** After a restore, the web app looks broken (no pictures, nothing plays), and nothing says why.

### S-R2. Links into another library are now deleted by the next scan (`src/scanner.rs:1033`)

Status: Fixed in `bd1f462`.

- **What this is:** The scanner skips a linked file unless it points inside the folders of *this* library (`section.paths`).
- **Problem:** Some setups build a curated library from links into another library, for example "Kinderfilme" at `/media/kids` linking to `/media/movies/Heat.mkv`. `may_send` allows such a file (it checks all libraries), but the scanner drops it. The scan then sees the file as gone. With the default `empty_trash_automatically: true`, the items are removed straight away (`scanner.rs:1357-1363`), and their watch state is no longer tied to anything. Links pointing completely outside the libraries are dropped the same way, and the admin is never told.
- **Fix:** Check against the same folders that `may_send` uses. Inside the existing `with_db` call, collect `db.sections.iter().flat_map(|s| s.paths.clone())` and pass that to `inside_folders`. Add one `elog!` per skipped link ("…is a link to a file outside the libraries and is skipped"). Extend `links_to_files_outside_the_library_are_skipped` with a link into a second library.
- **If we skip it:** After the upgrade, households with linked libraries lose those titles and their watched marks without warning.

### S-R3. The cross-site check misses the most common setup: plain http on the home network (`src/server.rs:2590-2605`, form parsing at `src/server.rs:2553`)

Status: Fixed in `bd1f462`.

- **What this is:** `other_site` relies only on `Sec-Fetch-Site`. It blocks non-GET requests and the no-sign-in-network admin for other websites.
- **Problem:** The Fetch Metadata spec tells browsers to send `Sec-Fetch-*` headers only to "potentially trustworthy" URLs (https or localhost), so `http://192.168.1.5:8484` gets none. A concrete attack: a Firefox user at home, with `no_auth_networks = 192.168.1.0/24`, opens a malicious page. That page auto-submits `<form method=POST action="http://192.168.1.5:8484/api/...">`. The request arrives with no `Sec-Fetch-Site`, so `other_site` is false and the caller becomes the first admin. The server also accepts form-encoded bodies, so the admin POST runs (change settings, add a user, delete a library). The attacker cannot read the answer, but does not need to. This is not new (before the branch it was worse, with `Access-Control-Allow-Origin: *`). But the fix that claims "other websites can no longer use the server" does not cover this case.
- **Fix:** Browsers send `Origin` on every cross-origin POST, over http too. Also treat a request as `other_site` when an `Origin` header is present and its `host[:port]` differs from the `Host` header (about 3 lines in the header loop plus the condition). Add one test: a POST with `Origin: http://evil.example`, no `Sec-Fetch-Site`, from a no-sign-in network, gets 403. (Behind a reverse proxy that rewrites `Host`, compare against `X-Forwarded-Host` from trusted proxies, or apply the check only when the no-sign-in admin would be used.)
- **If we skip it:** Any website a household member opens can change the server as admin, wherever "no sign-in networks" is set and the server runs on plain http.

## Should fix

### S-R4. The 4-streams-per-person cap stops a stream that is still playing, and cleans up while holding `STARTING` (`src/handlers.rs:631-646`)

Status: Fixed in `bd1f462`.

- **What this is:** `hls_start` kills the person's least recently used HLS session once they have 4.
- **Problem:** It does not check whether that session is idle. A family that shares one account across 5 devices (TV, tablets, laptop) is inside the assumed load. When the 5th device starts, the device that fetched a segment longest ago (seconds ago) stops mid-film. The kill, `wait()` and `remove_dir_all` of a long session's segment folder also run while the global `STARTING` lock is held, so every other start on the server waits for a disk cleanup on the NAS drives.
- **Fix:** Pick only sessions idle for more than about 20 s: `.filter(|s| s.user_id == user.id && s.last_access.elapsed() > Duration::from_secs(20))`. If none qualify, let the start go ahead (`max_transcodes` still bounds the CPU). Remove the session from the map under the lock, but do the kill and folder removal after `_turn` is dropped (or in a spawned thread).
- **If we skip it:** People who share an account get playback cut off at random, and starts get slow while big folders are deleted.

### S-R5. The seek-bar still (`frame`) reads later parts without the folder check (`src/handlers.rs:965-982`)

Status: Fixed in `bd1f462`.

- **What this is:** `playable_file` checks part 0 with `may_send`. `frame` then picks the file for the requested moment from `it.all_files()`.
- **Problem:** For a split movie whose part 2 lies outside the libraries, `/library/parts/{id}/frame?t=<a time in part 2>` makes ffmpeg read that file and returns a picture of it. Today this only happens for imported or not-yet-rescanned items, but it is the one file path the "never sent" rule still misses.
- **Fix:** After choosing `src`: `if !app.may_send(Path::new(&src), false) { return fail(403, "This file is outside the library folders"); }`. One line.
- **If we skip it:** A small leak (pictures of a file outside the libraries) stays open next to a fix that claims to close all of them.

## Nice to have

### S-R6. Restore keeps the old file's unreadable rows (`src/store.rs:399,415`, `src/maintenance.rs:181`)

Status: Fixed in `bd1f462`.

- **What this is:** `Store.unreadable_items`/`unreadable_views` are filled at start and keep those rows during full writes.
- **Problem:** Restore swaps in a different library but keeps the lists from the live file. So an unreadable item 2 from the live file survives a restore of a backup that never had it, and a newer version will show it again. Restore promises that "everything goes back".
- **Fix:** Clear both lists in `restore` after the swap (a small `Store::forget_unreadable()`, or set the fields directly). `forget_written` must keep them, as it does now.
- **If we skip it:** Rare stale entries after a restore, only seen with a newer version.

### S-R7. The same signature and compare code exists in two or three copies (`src/auth.rs:81-98`, `src/auth.rs:53-56`, `src/relay.rs:49`)

Status: Fixed in `bd1f462`.

- **What this is:** `ticket` and `read_ticket` each build the HMAC hex string with the same line. `auth::verify`, `read_ticket` and `relay::same` each hold their own constant-time compare (one that takes the same time whether or not the strings match early).
- **Problem:** If the signature format changes in one place and not the other, every ticket fails. Three copies of the compare must stay identical.
- **Fix:** One `fn sig(key, msg) -> String` in auth.rs used by both, and one `pub fn same(a, b)` (move `relay::same` to auth.rs) used by all three.
- **If we skip it:** Only upkeep cost.

## Clippy (`cargo clippy --release --no-default-features --all-targets`, base 5b30e62 vs HEAD, compared by message + code text)

- New warnings on this branch: **none** (0 new; 1 base warning is gone).
- `cargo test --release --no-default-features`: 130 passed, 0 failed, 4 ignored.

## Checked and fine

- Token hashing: every token writer (`new_token`) hashes. Lookups, logout, rotate, list and revoke compare hashes. Old 40-letter tokens are hashed on start and on restore. Ids keep the same 12 hex letters as before.
- Tickets are bound to the scope, the user and a live sign-in, are GET/HEAD only, and are refused with an empty key. `/api/ticket?path=` accepts only exact scopes, and download URLs match them.
- Relayed callers are no longer local, no-auth or trusted. The relay proof uses a separate tag, and an old relay gives a clear error.
- New locks: `STARTING` → `db` (settings) → `hls`/`progressive`, and nothing takes them the other way. `PICTURES` is not held while ffmpeg runs. `use_step` stands alone. Poisoning is handled. The `store` → `db` order is unchanged.
- Changed-only saves: `written_history/sessions` are updated only after commit and reset on failure. The `NOT IN ()` with an empty list is valid in SQLite. The view keys are built from integers only.
- Vendored tiny_http: the diff against crates.io 0.12.0 is exactly the marked changes. The 0x16 check runs only on the first byte of plain connections. The 431 path mirrors the existing 400 path. The Dockerfile copies `vendor/`, and Cargo.lock has no `source` line.
- Scanner extras by folder: same parent choice as before (BTreeMap order gives the lowest id first).
- Callers of the changed signatures (`matches_all`, `rule_matches`, `make_frame`, `new_token`, `ticket`) are all updated.

Verdict: fix 1, 2 and 3 first.
Lean: -4 lines possible.
Not checked: that browsers really leave out `Sec-Fetch-Site` on plain http (taken from the Fetch Metadata spec, not tried in a browser); the web/ and Flutter sides; Windows path handling in `inside_folders`. Risks to know: an automatic CI rollback to `lichtspiel:previous` signs every device out once (the older version cannot read hashed tokens), and home servers on this version refuse an older relay until the relay is updated.

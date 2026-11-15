# Open items after the 2026-11-15/16 review

The review of 2026-11-15 and 2026-11-16 found 126 problems. Almost all are fixed; [review-2026-11-15.md](review-2026-11-15.md) lists every one with its commit. This page lists only what is still open: 13 items postponed as nice to have, 4 partly fixed, and a few known limits that are worth real follow-up work. Every "where" below was checked again against the code at commit `2e6689d`.

## Getting started

### Layout

| Folder | What it is |
|---|---|
| `src/` | The Rust server (one binary, `lichtspiel-server`) |
| `vendor/tiny_http/` | The HTTP library, copied in and patched (header limits; look for `lichtspiel:` comments) |
| `web/` | The web app, plain JS with no build step, compiled into the binary |
| `clients/lichtspiel/` | The Flutter app (Windows, Android, macOS, iOS/iPadOS) |
| `plugins/` | Bundled Python plugins (`nfo-reader`, `trakt`, `webhook-notify`) |
| `Dockerfile`, `docker-compose.yml`, `.gitea/workflows/deploy.yml` | The image, the Synology setup, and the automatic deploy |

`AGENTS.md` and `README.md` describe the architecture in more detail.

### Build and test

| What | Command (from the repo root unless noted) |
|---|---|
| Server tests, as CI runs them | `cargo test --release --no-default-features` |
| Server build (desktop, with the status window) | `cargo build --release` |
| Client tests | `flutter test` in `clients/lichtspiel` |
| Client lint | `flutter analyze` in `clients/lichtspiel` |
| One plugin's tests | `python3 -I -m unittest discover -s plugins/<name>` |
| Web app syntax check | `node --check web/*.js` (there is no web test suite) |

Some server tests need ffmpeg on the PATH and skip themselves without it.

### Conventions

- Don't run `cargo fmt` over the tree. The code is not rustfmt-formatted; match the style around you.
- Lock order: take `store` before `db` (only `save` takes both). Call `app.save()` after a change.
- Items and watch states live in `Tracked` maps. Change them through that API; going around it does not compile.
- A new API route needs its match arm in `route()` and a line in `ROUTES` (`src/routes.rs`); `ROUTES` is the API documentation.
- Web text: the English string is the key (`t('Sign in')`). Add the German text to `web/lang/de.js` whenever you add or change UI text.
- The Flutter client's UI text is German and written directly in the code.
- `cargo build --release --no-default-features` (the Docker build) must keep compiling. Anything that needs a window goes behind `#[cfg(feature = "console")]`.

### Commits and branches

Commits use Conventional Commits with a scope, and the subject describes what the user sees, in lowercase. For example: `fix(web): editing details from a poster's menu changes the address`. Common scopes: `web`, `client`, `server`, `settings`, `plugins`, `ci`.

**Pushing to `main` deploys to the NAS.** Always work on a branch and leave the merge to the owner.

## Open items

| ID | Area | Title | Status | Size |
|---|---|---|---|---|
| C7 | Client | Big photos are refused on Mac and Windows | partly | M |
| C12 | Client | Minor player errors bring up the error screen | partly | M |
| C16 | Client | The queue and the admin flag survive sign-out | postponed | S |
| C17 | Client | The token in the browser history on Windows | postponed (probably fixed) | S |
| C18 | Client | Copies of a time formatter, one unused dependency | postponed | S |
| W11 | Web | "Edit details" from a poster's ⋯ menu doesn't change the address | postponed | S |
| W12 | Web | With "Level the volume" on, a video can play without sound | postponed | S |
| W13 | Web | Two quick seeks while converting can land on the older position | postponed | S |
| W14 | Web | A huge library builds one empty card per title up front | postponed | M |
| W15 | Web | One person's play queue shows up for the next | postponed | S |
| W16 | Web | A `?token=` link ignores "sign in automatically: off" | postponed | S |
| P8 | Plugins | Trakt can delete a lock that is still held | postponed | S |
| P9 | Plugins | Trakt `tokens.json` is readable by everyone | postponed | S |
| P10 | Plugins | Any signed-in user can fire webhook test messages | postponed | S |
| D8 | Deploy | Compose and `deploy.yml` define the same container twice | partly | M |
| D10 | Deploy | The build is not locked | postponed | S |
| S11 | Server/tests | No test that every route needs sign-in or admin | partly | M |

Sizes: S is under an hour and a few lines, M is a few hours or needs a decision, L is a day or more. Paths in the client section are relative to `clients/lichtspiel/`.

### Client

Report: [reviews/2026-11-15-client.md](reviews/2026-11-15-client.md).

#### C7. Big photos are refused on Mac and Windows

- **Status:** partly fixed in `bdcc8c1`. On a computer the message now says to shrink the picture first ("Das Bild ist zu groß (höchstens 3 MB) – bitte vorher verkleinern"). The picture itself is still not shrunk.
- **Where:** `lib/screens/edit.dart:231` (the picker), `:234-235` (the size check and message).
- **Problem:** image_picker ignores `maxWidth`, `maxHeight` and `imageQuality` on desktop. A Mac user picks a 6 MB camera JPEG and is told it's too big; the same photo works from an iPhone, where the picker scales it down.
- **Fix:** shrink the picture in the app on desktop before upload (that needs an image library, a new dependency), or let the server accept a bigger body and shrink it there. The server reads at most 4 MiB of body (`src/server.rs:2590`), which is about 3 MB of picture after base64, so raising the client limit alone does not help.
- **Size:** M.

#### C12. Minor player errors bring up the error screen

- **Status:** partly fixed in `bdcc8c1`. The player now pauses before the error screen shows, so the sound no longer runs behind it. Errors are not filtered yet.
- **Where:** `lib/screens/player.dart:464-470` (the error listener), `:868` (the error text replaces the picture).
- **Problem:** media_kit passes many mpv log lines on as errors, and some are recoverable. With any quality other than "Original", one failed HLS segment request (a `tcp:` line) goes straight to "Wiedergabe fehlgeschlagen", even though mpv would have carried on.
- **Fix:** ignore an error while playback is still moving, for example when `player.state.playing` is true and the position keeps advancing over the next second or two. The media_kit docs don't say which errors are recoverable, so test it with a real failed segment.
- **Size:** M.

#### C16. The queue and the admin flag survive sign-out

- **Status:** postponed.
- **Where:** `lib/main.dart:53-61` (`signOut`); the state lives in `lib/screens/player.dart:63` (`PlayerScreen.userQueue`), `lib/widgets/poster.dart:17` (`Poster.admin`) and `lib/screens/downloads.dart:22-24` (`DownloadRights`).
- **Problem:** a parent queues two films and signs out; the child's home profile signs in and sees the parent's "Warteschlange". When the child's episode ends, `findNext` (`player.dart:511`) takes the parent's film as next, which the server then refuses if the child may not see it.
- **Fix:** in `signOut`, add `PlayerScreen.userQueue.clear(); Poster.admin = false; DownloadRights.allowed = false;`.
- **Size:** S.

#### C17. The token in the browser history on Windows

- **Status:** postponed in the overview, but **probably fixed, needs a check on Windows.**
- **Where:** `lib/screens/downloads.dart:29-41` (`saveFile`), `lib/api.dart:379-381` (`ticketLink`).
- **Problem (as reported):** on platforms other than iOS and macOS, "Speichern" opened the file in the system browser with `?token=<account token>`, which stays in the browser history.
- **What the code does now:** since `fa82202`, `saveFile` asks the server for a ticket (`/api/ticket?path=…`) and opens the address with `?ticket=` instead of the token. A file ticket is tied to that one address and to the sign-in, and runs out after at most 6 hours (`src/auth.rs:90`); signing out ends it. So the history keeps only a short-lived, single-file ticket.
- **Fix:** run "Speichern" once on Windows and look at the address in the browser history: it should show `ticket=` and no `token=`. If it does, mark C17 fixed. The original one-line idea (`if (!Platform.isAndroid) return _download(…)`, so Windows downloads in the app) is still possible if you'd rather keep the browser out completely.
- **Size:** S.

#### C18. Copies of a time formatter, one unused dependency

- **Status:** postponed (cleanup).
- **Where:** `lib/screens/player.dart:51` (`_clock`), `:117` (`clock`), `lib/screens/detail.dart:21` (`fmtClock`), and a fourth copy found in this re-check, `lib/screens/dashboard.dart:23` (`fmtClockMs`, takes `num`). `pubspec.yaml:36` has `cupertino_icons`, and nothing in `lib/` uses it.
- **Problem:** nothing breaks; each copy has to be changed on its own, and the icon font ships for nothing.
- **Fix:** keep `fmtClock` in `detail.dart`, delete the other three and import it (the dashboard passes a `num`, so call it with `.toInt()`). Remove `cupertino_icons` from `pubspec.yaml`.
- **Size:** S.

### Web

Report: [reviews/2026-11-15-web.md](reviews/2026-11-15-web.md). Remember the rebuild: the web files are compiled into the server.

#### W11. "Edit details" from a poster's ⋯ menu doesn't change the address

- **Status:** postponed.
- **Where:** `web/pages.js:1280` (the ⋯ menu calls `editItem` with no `saved` callback) → `web/app.js:1384` (`saved ? saved() : itemPage(it.id)`). The same pattern is in `fixMatch`, `web/app.js:1303` (`else itemPage(it.id)`), and in the selection bar, `web/app.js:393`.
- **Problem:** on a library page, open a card's ⋯ → Edit details → Save. The item page appears, but the address still says `#/section/…`. Back leaves the library entirely, a reload shows the library again, and the library page's scroll listeners stay active.
- **Fix:** in `editItem` and `fixMatch`, use `location.hash = '#/item/' + it.id` instead of calling `itemPage(it.id)` directly.
- **Size:** S.

#### W12. With "Level the volume" on, a video can play without sound

- **Status:** postponed (needs Safari to reproduce).
- **Where:** `web/app.js:1778-1791` (`levelVolume`, which creates the `AudioContext`), `:2010` (the video's `play` listener). Since the report, `levelVolume` runs at the start of `load()` (`:1794`) and calls `LEVEL.ctx.resume()` there, but not on a user click.
- **Problem:** once the video is routed through the `AudioContext`, all sound goes through it, and a context started without a user gesture stays suspended. Open a `#/play/123` link with leveling on: autoplay is blocked, the user clicks ▶, the picture plays and there is no sound. Safari is the most likely to show this.
- **Fix:** in the `play` listener at `:2010`, add `if (LEVEL.ctx && LEVEL.ctx.state === 'suspended') LEVEL.ctx.resume();`. That runs inside the user's click.
- **Size:** S.

#### W13. Two quick seeks while converting can land on the older position

- **Status:** postponed.
- **Where:** `web/app.js:1792-1807` (`load`; after the awaits it only checks `P.it !== it`).
- **Problem:** two `load()` calls for the same title both pass the check. Click the seek bar twice quickly during a conversion: if the first `/decision` answer arrives last, playback jumps back to the first position, and each call has started its own ffmpeg stream.
- **Fix:** `const seq = ++P.loadSeq;` at the start of `load`, and `if (P.it !== it || seq !== P.loadSeq) return;` after the awaits.
- **Size:** S.

#### W14. A huge library builds one empty card per title up front

- **Status:** postponed.
- **Where:** `web/app.js:428-435` (`grid._reserve`), `:763-764` (reserves every remaining title and observes each placeholder).
- **Problem:** the "Songs" view of an 80,000-track music library builds 80,000 placeholder cards (about 320,000 elements) and observes each one before anything shows. The tab freezes for seconds, worst on a phone. Not measured.
- **Fix:** measure first with a big generated library. If it's slow, reserve placeholders only for a few thousand titles ahead of the loaded range and keep one tall spacer element for the rest.
- **Size:** M.

#### W15. One person's play queue shows up for the next

- **Status:** postponed.
- **Where:** `web/pages.js:1377-1378` (the queue is kept in `localStorage` under `queue`), `web/app.js:125` (`logout` doesn't clear it).
- **Problem:** on a family TV with home profiles, A queues episodes and uses "Switch user". B sees "Queue (12)" with A's titles, and ▶ tries to play titles B may not see.
- **Fix:** in `logout()`, add `Q.list = []; saveQueue();`. Or key the storage by user: `'queue:' + S.me.id`.
- **Size:** S.

#### W16. A `?token=` link ignores "sign in automatically: off"

- **Status:** postponed.
- **Where:** `web/app.js:8-9` (always writes the token to `localStorage`); `saveToken` is at `:14`.
- **Problem:** a sign-in link from the status window or the self-test keeps the token after the browser closes, even when the user chose session-only sign-in.
- **Fix:** call `saveToken(tok)` instead of `localStorage.setItem('token', tok)`, and move that block below `saveToken`.
- **Size:** S.

### Plugins

Report: [reviews/2026-11-15-ops.md](reviews/2026-11-15-ops.md).

#### P8. Trakt can delete a lock that is still held

- **Status:** postponed (rare).
- **Where:** `plugins/trakt/main.py:74-89` (`lock`, stale check at `:83-84`).
- **Problem:** a lock older than 60 s is deleted, but a plugin run may take up to 90 s, and the age check and the delete are two separate steps. A slow "Bring in my Trakt history" is running when a playback event deletes its lock; both runs then write `tokens.json`, and a freshly rotated refresh token can be lost, which disconnects Trakt.
- **Fix:** use `fcntl.flock` on an open lock file; the OS releases it when the process dies, so no stale check is needed (check Windows, where `fcntl` doesn't exist). The smallest change is to raise the stale age above the plugin time limit (`> 120`).
- **Size:** S.

#### P9. Trakt `tokens.json` is readable by everyone

- **Status:** postponed.
- **Where:** `plugins/trakt/main.py:30` (the path), `:110-114` (`save`).
- **Problem:** the access and refresh tokens are written with the default mode 0644 into the config folder, which may be reachable over SMB. Any NAS account that can read that share can copy them and act on the users' Trakt accounts.
- **Fix:** `os.chmod(tmp, 0o600)` before `os.replace` in `save()`.
- **Size:** S.

#### P10. Any signed-in user can fire webhook test messages

- **Status:** postponed.
- **Where:** `src/routes.rs:1853` → `src/handlers.rs:1139` (`plugin_call`, no admin check); `plugins/webhook-notify/main.py:72` (every `api.call` sends a test message).
- **Problem:** a non-admin user, or a script with their token, calls `POST /api/plugins/webhook-notify/call/test` in a loop. The Discord or ntfy channel is spammed, and each call starts a Python process that can run up to 15 s.
- **Fix:** in webhook-notify, answer `api.call` only when `data.get("admin")` is true; the server already sends `admin` (`handlers.rs:1146`).
- **Size:** S.

### Deploy

Report: [reviews/2026-11-15-ops.md](reviews/2026-11-15-ops.md).

#### D8. Compose and `deploy.yml` define the same container twice

- **Status:** partly fixed in `cd9586b`. Each file now says the other exists and that mounts and devices must match. They are not merged.
- **Where:** `docker-compose.yml:1-30` and `.gitea/workflows/deploy.yml:68-79` (`start()` with `docker run`).
- **Problem:** both list mounts, ports, devices and the restart policy under the same name `lichtspiel`. A change to one is easy to miss in the other. If someone sets it up through Container Manager → Project and then turns on CI, the first deploy runs `docker rm -f` on the compose-made container and Container Manager shows a broken project.
- **Fix:** keep one definition. For example, let `deploy.yml` run `docker compose up -d --no-build` with the image it just built (`image: lichtspiel:latest` in compose). Keep the rollback and the `--group-add` for `/dev/dri` working.
- **Size:** M.

#### D10. The build is not locked

- **Status:** postponed.
- **Where:** `Dockerfile:4` (`FROM rust:1-slim-bookworm`), `:17`, `:25` and `:29` (the cargo calls, none with `--locked`).
- **Problem:** if `Cargo.toml` changes without a matching `Cargo.lock`, CI quietly builds versions nobody tested, and a new Rust release can break a push that changed nothing.
- **Fix:** add `--locked` to the three cargo calls. Optionally pin the image to a fixed `rust:1.xx-slim-bookworm`.
- **Size:** S.

### Server and tests

Report: [reviews/2026-11-15-server.md](reviews/2026-11-15-server.md).

#### S11. No test that every route needs sign-in or admin

- **Status:** partly fixed in `c4f405f`. `routes_need_sign_in_or_admin` (`src/server.rs:3468`) checks the routes touched in that commit only.
- **Where:** `src/routes.rs:246-290` (the routes open without sign-in), `:292-296` (the sign-in gate and `need_admin`), `ROUTES` at `src/routes.rs:17`.
- **Problem:** access depends on where a match arm sits and on about 66 hand-written `need_admin()` checks. Someone adds `("GET", ["api", "settings", "export"])` to the open block, or forgets an admin check, and every test still passes.
- **Fix:** one unit test that walks every `ROUTES` entry outside a short allow-list, calls `route()` with no user and expects 401, then calls it with a non-admin user for the entries whose description starts with "Admin:" and expects 403. Path parameters (`{id}`) need a stand-in value.
- **Size:** M.

## Known limits worth a look

From "Known limits" in the overview, only the ones that are real follow-up work.

| Limit | Where | Suggested direction |
|---|---|---|
| Restore doesn't run `Db::after_load`, so the old default server name isn't renamed and default playlists aren't created until the next start. | `src/maintenance.rs:179-213` (`restore` calls `upgrade()` only); `after_load` is `src/db.rs:1284` | Call `loaded.after_load()` next to `loaded.upgrade()` in `restore`, and add a restore test for it. |
| A Plex sync running during a restore can write `plex-sync-state.json` again for the replaced library. | `src/maintenance.rs:209` (deletes the file); `src/plexsync.rs:35` (`RUNNING`), `:154` (writes it) | Refuse a restore while `RUNNING` is set, or make the sync drop its state write when a restore happened during the run. |
| A reverse proxy on plain http that rewrites `Host` gets 403 on the web app's own POSTs. | `src/server.rs:2632` (`Origin` compared with `Host`), `:2648` | Compare with `X-Forwarded-Host` from trusted proxies, or apply the check only when the no-sign-in admin would be used (see the S-R3 report). Until then: keep `Host` or use https. |
| The client doesn't cut off a body that stalls after the headers. | `clients/lichtspiel/lib/api.dart:194-196` (`_send`: the timer is cancelled once the headers arrive) | An idle timeout on the body stream (no bytes for N seconds) rather than a total limit, so big lists over slow lines still work (C-R2). |
| The download dialog stays open if the download fails before the dialog's first frame. | `clients/lichtspiel/lib/screens/downloads.dart:61-66` (`dialogContext` is set in the builder), `:136` (closes only if it is set) | Close it through the root navigator once the dialog is up, or wait for the first frame before starting the download. |
| dart:io can't close a socket stuck in a TLS handshake. | `clients/lichtspiel/lib/api.dart:172-173` (a `shortcut:` comment) | Nothing simple in dart:io today; revisit when it gains a handshake timeout. One leaked socket per failed try. |
| An IPv6 address typed without brackets is not handled. | `clients/lichtspiel/lib/api.dart:87-93` (`normalize`) | If the text has two or more `:` and no `[`, wrap the host in brackets before adding the port. Add a test. |
| A client that trickles header bytes slowly still holds a thread. | `vendor/tiny_http/src/client.rs:42-45` (per-line and count limits only) | An overall deadline for reading the request line and headers (for example 30 s) in the vendored reader. |
| No rate limit on home-network discovery (GDM). | `src/gdm.rs:45-50` | A small per-sender limit (one answer per address per second). It already answers only home-network senders. |
| `?token=` in addresses is still accepted, for older clients and scripts. | `src/server.rs:2581-2586` | Log when it is used, then drop it once the Flutter app and scripts are all on `X-Token`. Keep `?ticket=`. |
| The macOS release build carries `com.apple.security.get-task-allow` (a debug entitlement). | Not in `clients/lichtspiel/macos/Runner/Release.entitlements`; Xcode adds it while signing, because the project signs ad hoc (`CODE_SIGN_IDENTITY = "-"`, `macos/Runner.xcodeproj/project.pbxproj:688`) and `CODE_SIGN_INJECT_BASE_ENTITLEMENTS` is not set (so it defaults to on). Seen with `codesign -d --entitlements :-` on a local Release build. | Set `CODE_SIGN_INJECT_BASE_ENTITLEMENTS = NO` for the Release configuration, or sign with a Developer ID for real releases (notarization refuses this entitlement). Check again with `codesign`. |
| Nothing in round 4 was tried on the real NAS, in a browser or on devices. | | See the checklist below. |

## Testing on real hardware

None of this has been run for real yet:

- [ ] Deploy to the NAS: the container runs as non-root (uid 1026, gid 100) after `sudo chown -R 1026:100` on the config folder, and the deploy adds `--group-add` for `/dev/dri` itself.
- [ ] After the deploy, the dashboard still shows "hw" for hardware transcoding.
- [ ] Open the web app and check the browser console for CSP errors (blocked scripts or frames).
- [ ] The relay: update Lichtspiel on the relay computer first, then the NAS, and check that remote access still works.
- [ ] The macOS app: an `https://` address that only speaks http falls back to http on the home network (`<nas-lan-ip>`), and does **not** fall back when the server is reached from outside (`<your-server>`).
- [ ] Ticket renewal during a very long playback in a real browser (more than 6 hours).
- [ ] C12 and C17 above need a real device (a failed HLS segment; "Speichern" on Windows).

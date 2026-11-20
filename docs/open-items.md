# Open items after the 2026-11-15/16 review

The review of 2026-11-15 and 2026-11-16 found 126 problems. Almost all are fixed; [review-2026-11-15.md](review-2026-11-15.md) lists every one with its commit. This page lists only what is still open: 11 items postponed as nice to have, 5 partly fixed, and a few known limits that are worth real follow-up work. Every "where" below was checked again against the code at commit `ddb46ee` (2026-11-20).

Changes since the first version of this page (`2e6689d`):

- **P8** (Trakt lock) no longer applies: the Trakt plugin was replaced by Trakt built into the server (`7dd9693`), which changes its file under an in-process lock.
- **P9** (Trakt tokens readable by everyone) still applies, now to the server's `trakt.json`.
- **W11** is partly fixed: "Fix match" no longer calls the item page directly; "Edit details" still does.
- The known limit "a reverse proxy that rewrites `Host` gets 403" is fixed (`925310e`).
- The review branches are merged into `main` and pushed, so the deploy has run.

## Getting started

### Layout

| Folder | What it is |
|---|---|
| `src/` | The Rust server (one binary, `lichtspiel-server`), including the built-in Trakt sync (`src/trakt.rs`) |
| `vendor/tiny_http/` | The HTTP library, copied in and patched (header limits; look for `lichtspiel:` comments) |
| `web/` | The web app, plain JS with no build step, compiled into the binary |
| `clients/lichtspiel/` | The Flutter app (Windows, Android, macOS, iOS/iPadOS) |
| `plugins/` | Bundled Python plugins (`nfo-reader`, `webhook-notify`) |
| `Dockerfile`, `docker-compose.yml`, `.gitea/workflows/deploy.yml` | The image, the Synology setup, and the automatic deploy |

`README.md` describes the architecture in more detail.

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

### Commits and pushing

Commits use Conventional Commits with a scope, and the subject describes what the user sees, in lowercase. For example: `fix(web): editing details from a poster's menu changes the address`. Common scopes: `web`, `app`, `client`, `server`, `settings`, `plugins`, `ci`.

**Pushing to `main` deploys to the NAS.** Push only when the owner asks for it.

## Open items

| ID | Area | Title | Status | Size |
|---|---|---|---|---|
| C7 | Client | Big photos are refused on Mac and Windows | partly | M |
| C12 | Client | Minor player errors bring up the error screen | partly | M |
| C16 | Client | The queue and the admin flag survive sign-out | postponed | S |
| C17 | Client | The token in the browser history on Windows | postponed (probably fixed) | S |
| C18 | Client | Copies of a time formatter, one unused dependency | postponed | S |
| W11 | Web | "Edit details" from a poster's ⋯ menu doesn't change the address | partly | S |
| W12 | Web | With "Level the volume" on, a video can play without sound | postponed | S |
| W13 | Web | Two quick seeks while converting can land on the older position | postponed | S |
| W14 | Web | A huge library builds one empty card per title up front | postponed | M |
| W15 | Web | One person's play queue shows up for the next | postponed | S |
| W16 | Web | A `?token=` link ignores "sign in automatically: off" | postponed | S |
| P9 | Server | Trakt tokens (`trakt.json`) are readable by everyone | postponed | S |
| P10 | Plugins | Any signed-in user can fire webhook test messages | postponed | S |
| D8 | Deploy | Compose and `deploy.yml` define the same container twice | partly | M |
| D10 | Deploy | The build is not locked | postponed | S |
| S11 | Server/tests | No test that every route needs sign-in or admin | partly | M |

Sizes: S is under an hour and a few lines, M is a few hours or needs a decision, L is a day or more. Paths in the client section are relative to `clients/lichtspiel/`.

### Client

Report: [reviews/2026-11-15-client.md](reviews/2026-11-15-client.md).

#### C7. Big photos are refused on Mac and Windows

- **Status:** partly fixed in `bdcc8c1`. On a computer the message now says to shrink the picture first ("Das Bild ist zu groß (höchstens 3 MB) – bitte vorher verkleinern"). The picture itself is still not shrunk.
- **Where:** `lib/screens/edit.dart:358` (the picker), `:362` (the size check and message).
- **Problem:** image_picker ignores `maxWidth`, `maxHeight` and `imageQuality` on desktop. A Mac user picks a 6 MB camera JPEG and is told it's too big; the same photo works from an iPhone, where the picker scales it down.
- **Fix:** shrink the picture in the app on desktop before upload (that needs an image library, a new dependency), or let the server accept a bigger body and shrink it there. The server reads at most 4 MiB of body (`src/server.rs:2672`), which is about 3 MB of picture after base64, so raising the client limit alone does not help.
- **Size:** M.

#### C12. Minor player errors bring up the error screen

- **Status:** partly fixed in `bdcc8c1`. The player now pauses before the error screen shows, so the sound no longer runs behind it. Errors are not filtered yet.
- **Where:** `lib/screens/player.dart:466-475` (the error listener), `:873` (the error text replaces the picture).
- **Problem:** media_kit passes many mpv log lines on as errors, and some are recoverable. With any quality other than "Original", one failed HLS segment request (a `tcp:` line) goes straight to "Wiedergabe fehlgeschlagen", even though mpv would have carried on.
- **Fix:** ignore an error while playback is still moving, for example when `player.state.playing` is true and the position keeps advancing over the next second or two. The media_kit docs don't say which errors are recoverable, so test it with a real failed segment.
- **Size:** M.

#### C16. The queue and the admin flag survive sign-out

- **Status:** postponed.
- **Where:** `lib/main.dart:55-63` (`signOut`); the state lives in `lib/screens/player.dart:63` (`PlayerScreen.userQueue`), `lib/widgets/poster.dart:21` (`Poster.admin`) and `lib/screens/downloads.dart:24-27` (`DownloadRights`).
- **Problem:** a parent queues two films and signs out; the child's home profile signs in and sees the parent's "Warteschlange". When the child's episode ends, `findNext` (`player.dart:513`) takes the parent's film as next, which the server then refuses if the child may not see it.
- **Fix:** in `signOut`, add `PlayerScreen.userQueue.clear(); Poster.admin = false; DownloadRights.allowed = false;`.
- **Size:** S.

#### C17. The token in the browser history on Windows

- **Status:** postponed in the overview, but **probably fixed, needs a check on Windows.**
- **Where:** `lib/screens/downloads.dart:31-36` (`saveFile`), `lib/api.dart:380` (`ticketLink`).
- **Problem (as reported):** on platforms other than iOS and macOS, "Speichern" opened the file in the system browser with `?token=<account token>`, which stays in the browser history.
- **What the code does now:** since `fa82202`, `saveFile` asks the server for a ticket (`/api/ticket?path=…`) and opens the address with `?ticket=` instead of the token. A file ticket is tied to that one address and to the sign-in, and runs out after at most 6 hours (`src/auth.rs:92`); signing out ends it. So the history keeps only a short-lived, single-file ticket.
- **Fix:** run "Speichern" once on Windows and look at the address in the browser history: it should show `ticket=` and no `token=`. If it does, mark C17 fixed. The original one-line idea (`if (!Platform.isAndroid) return _download(…)`, so Windows downloads in the app) is still possible if you'd rather keep the browser out completely.
- **Size:** S.

#### C18. Copies of a time formatter, one unused dependency

- **Status:** postponed (cleanup). There are now six copies instead of four.
- **Where:** `lib/screens/detail.dart:23` (`fmtClock`), `lib/screens/player.dart:51` (`_clock`), `:118` (`clock`), `lib/screens/dashboard.dart:24` (`fmtClockMs`, takes `num`), `lib/screens/edit.dart:52` (`_clock`, takes `dynamic`), `lib/widgets/item_extras.dart:102` (`_clock`, takes `num`). `pubspec.yaml:36` has `cupertino_icons`, and nothing in `lib/` uses it.
- **Problem:** nothing breaks; each copy has to be changed on its own, and the icon font ships for nothing.
- **Fix:** keep `fmtClock` in `detail.dart` (or move it to a small shared file), delete the other copies and import it (callers with a `num` pass `.toInt()`; compare the output of each copy first, they may differ in details such as hours). Remove `cupertino_icons` from `pubspec.yaml`.
- **Size:** S.

### Web

Report: [reviews/2026-11-15-web.md](reviews/2026-11-15-web.md). Remember the rebuild: the web files are compiled into the server.

#### W11. "Edit details" from a poster's ⋯ menu doesn't change the address

- **Status:** partly. `fixMatch` now calls `itemPage` only when the address already is that item's page (`web/app.js:1469`). `editItem` still calls it directly.
- **Where:** `web/pages.js:1361` (the ⋯ menu calls `editItem` with no `saved` callback) and the selection bar, `web/app.js:516` (same) → `web/app.js:1579` (`saved ? saved() : itemPage(it.id)`).
- **Problem:** on a library page, open a card's ⋯ → Edit details → Save. The item page appears, but the address still says `#/section/…`. Back leaves the library entirely, a reload shows the library again, and the library page's scroll listeners stay active.
- **Fix:** in `editItem`, use `location.hash = '#/item/' + it.id` instead of calling `itemPage(it.id)` directly (or do as `fixMatch` does).
- **Size:** S.

#### W12. With "Level the volume" on, a video can play without sound

- **Status:** postponed (needs Safari to reproduce).
- **Where:** `web/app.js:1974-1986` (`levelVolume`, which creates the `AudioContext` and calls `LEVEL.ctx.resume()` at `:1985`), `:1990` (called at the start of `load()`), `:2206` (the video's `play` listener). The resume runs when loading, not on a user click.
- **Problem:** once the video is routed through the `AudioContext`, all sound goes through it, and a context started without a user gesture stays suspended. Open a `#/play/123` link with leveling on: autoplay is blocked, the user clicks ▶, the picture plays and there is no sound. Safari is the most likely to show this.
- **Fix:** in the `play` listener at `:2206`, add `if (LEVEL.ctx && LEVEL.ctx.state === 'suspended') LEVEL.ctx.resume();`. That runs inside the user's click.
- **Size:** S.

#### W13. Two quick seeks while converting can land on the older position

- **Status:** postponed.
- **Where:** `web/app.js:1988-2003` (`load`; after the awaits it only checks `P.it !== it`).
- **Problem:** two `load()` calls for the same title both pass the check. Click the seek bar twice quickly during a conversion: if the first `/decision` answer arrives last, playback jumps back to the first position, and each call has started its own ffmpeg stream.
- **Fix:** `const seq = ++P.loadSeq;` at the start of `load`, and `if (P.it !== it || seq !== P.loadSeq) return;` after the awaits.
- **Size:** S.

#### W14. A huge library builds one empty card per title up front

- **Status:** postponed.
- **Where:** `web/app.js:554-560` (`grid._reserve`), `:927-928` (reserves every remaining title and observes each placeholder).
- **Problem:** the "Songs" view of an 80,000-track music library builds 80,000 placeholder cards (about 320,000 elements) and observes each one before anything shows. The tab freezes for seconds, worst on a phone. Not measured.
- **Fix:** measure first with a big generated library. If it's slow, reserve placeholders only for a few thousand titles ahead of the loaded range and keep one tall spacer element for the rest.
- **Size:** M.

#### W15. One person's play queue shows up for the next

- **Status:** postponed.
- **Where:** `web/pages.js:1473-1474` (the queue is kept in `localStorage` under `queue`), `web/app.js:194` (`logout` doesn't clear it).
- **Problem:** on a family TV with home profiles, A queues episodes and uses "Switch user". B sees "Queue (12)" with A's titles, and ▶ tries to play titles B may not see.
- **Fix:** in `logout()`, add `Q.list = []; saveQueue();`. Or key the storage by user: `'queue:' + S.me.id`.
- **Size:** S.

#### W16. A `?token=` link ignores "sign in automatically: off"

- **Status:** postponed.
- **Where:** `web/app.js:9` (always writes the token to `localStorage`); `saveToken` is at `:14`.
- **Problem:** a sign-in link from the status window or the self-test keeps the token after the browser closes, even when the user chose session-only sign-in.
- **Fix:** call `saveToken(tok)` instead of `localStorage.setItem('token', tok)`, and move that block below `saveToken`.
- **Size:** S.

### Plugins and Trakt

Report: [reviews/2026-11-15-ops.md](reviews/2026-11-15-ops.md).

#### P9. Trakt tokens are readable by everyone

- **Status:** postponed. Reported for the old plugin's `tokens.json`; the built-in Trakt sync has the same problem with `trakt.json`.
- **Where:** `src/trakt.rs:28` (the file name), `:108-113` (`write`).
- **Problem:** the access and refresh tokens of every user are written with the default mode (usually 0644) into the data folder, which may be reachable over SMB. Any NAS account that can read that share can copy them and act on the users' Trakt accounts.
- **Fix:** on Unix, set the mode of the temporary file to 0600 before the rename (`std::os::unix::fs::PermissionsExt`, behind `#[cfg(unix)]`), or create it with `OpenOptions` and `.mode(0o600)`.
- **Size:** S.

#### P10. Any signed-in user can fire webhook test messages

- **Status:** postponed.
- **Where:** `src/routes.rs:1885` → `src/handlers.rs:1149` (`plugin_call`, no admin check); `plugins/webhook-notify/main.py:72` (every `api.call` sends a test message).
- **Problem:** a non-admin user, or a script with their token, calls `POST /api/plugins/webhook-notify/call/test` in a loop. The Discord or ntfy channel is spammed, and each call starts a Python process that can run up to 15 s.
- **Fix:** in webhook-notify, answer `api.call` only when `data.get("admin")` is true; the server already sends `admin` (`handlers.rs:1156`).
- **Size:** S.

### Deploy

Report: [reviews/2026-11-15-ops.md](reviews/2026-11-15-ops.md).

#### D8. Compose and `deploy.yml` define the same container twice

- **Status:** partly fixed in `cd9586b`. Each file now says the other exists and that mounts and devices must match. They are not merged.
- **Where:** `docker-compose.yml` (whole file) and `.gitea/workflows/deploy.yml:76-83` (`start()` with `docker run`).
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

- **Status:** partly fixed in `c4f405f`. `routes_need_sign_in_or_admin` (`src/server.rs:3552`) checks the routes touched in that commit only.
- **Where:** `src/routes.rs:255-304` (the routes open without sign-in), `:306-310` (the sign-in gate and `need_admin`), `ROUTES` at `src/routes.rs:17`.
- **Problem:** access depends on where a match arm sits and on about 67 hand-written `need_admin()` checks. Someone adds `("GET", ["api", "settings", "export"])` to the open block, or forgets an admin check, and every test still passes.
- **Fix:** one unit test that walks every `ROUTES` entry outside a short allow-list, calls `route()` with no user and expects 401, then calls it with a non-admin user for the entries whose description starts with "Admin:" and expects 403. Path parameters (`{id}`) need a stand-in value.
- **Size:** M.

## Known limits worth a look

From "Known limits" in the overview, only the ones that are real follow-up work.

| Limit | Where | Suggested direction |
|---|---|---|
| Restore doesn't run `Db::after_load`, so the old default server name isn't renamed and default playlists aren't created until the next start. | `src/maintenance.rs:179-214` (`restore` calls `upgrade()` only, `:203`); `after_load` is `src/db.rs:1284` | Call `loaded.after_load()` next to `loaded.upgrade()` in `restore`, and add a restore test for it. |
| A Plex sync running during a restore can write `plex-sync-state.json` again for the replaced library. | `src/maintenance.rs:209` (deletes the file); `src/plexsync.rs:35` (`RUNNING`), `:135` (the state file) | Refuse a restore while `RUNNING` is set, or make the sync drop its state write when a restore happened during the run. |
| The client doesn't cut off a body that stalls after the headers. | `clients/lichtspiel/lib/api.dart:195-197` (`_send`: the timer is cancelled once the headers arrive) | An idle timeout on the body stream (no bytes for N seconds) rather than a total limit, so big lists over slow lines still work (C-R2). |
| The download dialog stays open if the download fails before the dialog's first frame. | `clients/lichtspiel/lib/screens/downloads.dart:63-68` (`dialogContext` is set in the builder), `:137` (closes only if it is set) | Close it through the root navigator once the dialog is up, or wait for the first frame before starting the download. |
| dart:io can't close a socket stuck in a TLS handshake. | `clients/lichtspiel/lib/api.dart:173` (a `shortcut:` comment) | Nothing simple in dart:io today; revisit when it gains a handshake timeout. One leaked socket per failed try. |
| An IPv6 address typed without brackets is not handled. | `clients/lichtspiel/lib/api.dart:87-93` (`normalize`) | If the text has two or more `:` and no `[`, wrap the host in brackets before adding the port. Add a test. |
| A client that trickles header bytes slowly still holds a thread. | `vendor/tiny_http/src/client.rs:42-45` (per-line and count limits only) | An overall deadline for reading the request line and headers (for example 30 s) in the vendored reader. |
| No rate limit on home-network discovery (GDM). | `src/gdm.rs:45-50` | A small per-sender limit (one answer per address per second). It already answers only home-network senders. |
| `?token=` in addresses is still accepted, for older clients and scripts. | `src/server.rs:2663-2667` | Log when it is used, then drop it once the Flutter app and scripts are all on `X-Token`. Keep `?ticket=`. |
| The macOS release build carries `com.apple.security.get-task-allow` (a debug entitlement). | Not in `clients/lichtspiel/macos/Runner/Release.entitlements`; Xcode adds it while signing, because the project signs ad hoc (`CODE_SIGN_IDENTITY = "-"`, `macos/Runner.xcodeproj/project.pbxproj:688`) and `CODE_SIGN_INJECT_BASE_ENTITLEMENTS` is not set (so it defaults to on). Seen with `codesign -d --entitlements :-` on a local Release build. | Set `CODE_SIGN_INJECT_BASE_ENTITLEMENTS = NO` for the Release configuration, or sign with a Developer ID for real releases (notarization refuses this entitlement). Check again with `codesign`. |

## Testing on real hardware

The review branches are deployed now, but these checks have not been recorded as done. Tick them off once tried:

- [ ] The container runs as non-root (uid 1026, gid 100) after `sudo chown -R 1026:100` on the config folder, and the deploy adds `--group-add` for `/dev/dri` itself.
- [ ] The dashboard still shows "hw" for hardware transcoding.
- [ ] Open the web app and check the browser console for CSP errors (blocked scripts or frames).
- [ ] The relay: Lichtspiel on the relay computer is updated, and remote access still works.
- [ ] The macOS app: an `https://` address that only speaks http falls back to http on the home network (`<nas-lan-ip>`), and does **not** fall back when the server is reached from outside (`<your-server>`).
- [ ] Ticket renewal during a very long playback in a real browser (more than 6 hours).
- [ ] C12 and C17 above need a real device (a failed HLS segment; "Speichern" on Windows).

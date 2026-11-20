# Open items after the 2026-11-15/16 review

The review of 2026-11-15 and 2026-11-16 found 126 problems. Almost all are fixed; [review-2026-11-15.md](review-2026-11-15.md) lists every one with its commit. This page lists only what is still open: 1 item postponed as nice to have, 3 partly fixed, and a few known limits that are worth real follow-up work. Every "where" below was checked again against the code at commit `ddb46ee` (2026-11-20).

Changes since the first version of this page (`2e6689d`):

- **P8** (Trakt lock) no longer applies: the Trakt plugin was replaced by Trakt built into the server (`7dd9693`), which changes its file under an in-process lock.
- The known limit "a reverse proxy that rewrites `Host` gets 403" is fixed (`925310e`).
- The review branches are merged into `main` and pushed, so the deploy has run.
- On 2026-11-20 the small (S) items were fixed: W11, W12, W13, W15 and W16 in `3b997c3`; C16 and C18 in `62edc5f`; P9 (`trakt.json` is now mode 0600) in `d341d67`; P10 in `76ae467`; D10 in `f12f477`. W12 still wants a try in Safari.
- **C17** is fixed by `fa82202`, checked on 2026-11-20: on Windows "Speichern" opens the file with `?ticket=` and never `?token=` (`lib/screens/downloads.dart:32-44`). Against a running server the ticket loads its own address (200), is refused for another address, without sign-in and after signing out (401 each), and does not contain the token. Not clicked in the Windows app itself; `launchUrl` passes the link on unchanged.
- **S11** is fixed: `every_route_needs_sign_in_and_every_admin_route_needs_an_admin` (`src/server.rs`) calls every `ROUTES` entry without sign-in (401 expected, except a short open list) and every "Admin:" entry as a profile (403 expected). Checked that it fails when a route is moved into the open block.
- Two known limits are fixed: a restore now runs `Db::after_load` like a start does (old default server name, default playlists; test `a_restored_old_backup_gets_the_new_server_name_and_the_default_playlists`), and the app puts brackets around an IPv6 address typed without them (`Api.normalize`, with a test).

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

`README.md` and the guides it links in `docs/` describe how everything works.

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
| W14 | Web | A huge library builds one empty card per title up front | postponed | M |
| D8 | Deploy | Compose and `deploy.yml` define the same container twice | partly | M |
| D11 | Deploy | The Docker image was 616 MB; now 425 MB with Jellyfin's ffmpeg | check on the NAS | S |

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

### Web

Report: [reviews/2026-11-15-web.md](reviews/2026-11-15-web.md). Remember the rebuild: the web files are compiled into the server.

#### W14. A huge library builds one empty card per title up front

- **Status:** postponed.
- **Where:** `web/app.js:554-560` (`grid._reserve`), `:927-928` (reserves every remaining title and observes each placeholder).
- **Problem:** the "Songs" view of an 80,000-track music library builds 80,000 placeholder cards (about 320,000 elements) and observes each one before anything shows. The tab freezes for seconds, worst on a phone. Not measured.
- **Fix:** measure first with a big generated library. If it's slow, reserve placeholders only for a few thousand titles ahead of the loaded range and keep one tall spacer element for the rest.
- **Size:** M.

### Deploy

Report: [reviews/2026-11-15-ops.md](reviews/2026-11-15-ops.md).

#### D8. Compose and `deploy.yml` define the same container twice

- **Status:** partly fixed in `cd9586b`. Each file now says the other exists and that mounts and devices must match. They are not merged.
- **Where:** `docker-compose.yml` (whole file) and `.gitea/workflows/deploy.yml:76-83` (`start()` with `docker run`).
- **Problem:** both list mounts, ports, devices and the restart policy under the same name `lichtspiel`. A change to one is easy to miss in the other. If someone sets it up through Container Manager → Project and then turns on CI, the first deploy runs `docker rm -f` on the compose-made container and Container Manager shows a broken project.
- **Fix:** keep one definition. For example, let `deploy.yml` run `docker compose up -d --no-build` with the image it just built (`image: lichtspiel:latest` in compose). Keep the rollback and the `--group-add` for `/dev/dri` working.
- **Size:** M.

#### D11. The Docker image is about 616 MB, the server in it about 12 MB

- **Status:** mostly done on 2026-11-20, waiting for a check on the NAS. The image now uses Jellyfin's ffmpeg build (`jellyfin-ffmpeg7`) instead of Debian's: **616 MB → 425 MB** (the package layer 529 MB → 337 MB). The server's `--self-test` passes all 19 steps in the new image (transcoding, HLS, remux, credits detection, deep analysis).
- **Measured** (`docker history` on the NAS, and package by package in `debian:bookworm-slim`): Debian's `ffmpeg` alone added 440 MB (LLVM 112 MB, Mesa/OpenGL, speech synthesis, ICU …), the Intel VA drivers 16 MB, `mesa-va-drivers` only 12 MB, python3 with ca-certificates 47 MB. `jellyfin-ffmpeg7` with everything it needs is 279 MB and brings its own VA drivers (Intel i965 and iHD, AMD radeonsi), VAAPI, QSV and NVENC. The first estimate (350 MB, ~100 MB for `mesa-va-drivers`) was wrong.
- **Still to check on the NAS:** after the next deploy, the dashboard shows "hw" for a converted video, and a converted video plays. Docker Desktop on Windows has no `/dev/dri`, so hardware transcoding could not be tried here. If it fails, going back is one commit (the old `apt-get install` line).
- **What else is left:** Python (47 MB) is needed for the plugins and the health check. Alpine would need a musl build of the server and gains little now. Below ~350 MB is unlikely while ffmpeg with hardware drivers is in the image.
- **Size:** S (the NAS check).

## Known limits worth a look

From "Known limits" in the overview, only the ones that are real follow-up work.

| Limit | Where | Suggested direction |
|---|---|---|
| A Plex sync running during a restore can write `plex-sync-state.json` again for the replaced library. | `src/maintenance.rs:209` (deletes the file); `src/plexsync.rs:35` (`RUNNING`), `:135` (the state file) | Refuse a restore while `RUNNING` is set, or make the sync drop its state write when a restore happened during the run. |
| The client doesn't cut off a body that stalls after the headers. | `clients/lichtspiel/lib/api.dart:195-197` (`_send`: the timer is cancelled once the headers arrive) | An idle timeout on the body stream (no bytes for N seconds) rather than a total limit, so big lists over slow lines still work (C-R2). |
| The download dialog stays open if the download fails before the dialog's first frame. | `clients/lichtspiel/lib/screens/downloads.dart:63-68` (`dialogContext` is set in the builder), `:137` (closes only if it is set) | Close it through the root navigator once the dialog is up, or wait for the first frame before starting the download. |
| dart:io can't close a socket stuck in a TLS handshake. | `clients/lichtspiel/lib/api.dart:173` (a `shortcut:` comment) | Nothing simple in dart:io today; revisit when it gains a handshake timeout. One leaked socket per failed try. |
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
- [ ] C12 above needs a real device (a failed HLS segment).

# Open items after the 2026-11-15/16 review

The review of 2026-11-15 and 2026-11-16 found 126 problems. Almost all are fixed; [review-2026-11-15.md](review-2026-11-15.md) lists every one with its commit. This page lists only what is still open: 1 partly fixed item, and a few known limits that are worth real follow-up work. Every "where" below was checked again against the code at commit `ddb46ee` (2026-11-20).

Changes since the first version of this page (`2e6689d`):

- **P8** (Trakt lock) no longer applies: the Trakt plugin was replaced by Trakt built into the server (`7dd9693`), which changes its file under an in-process lock.
- The known limit "a reverse proxy that rewrites `Host` gets 403" is fixed (`925310e`).
- The review branches are merged into `main` and pushed, so the deploy has run.
- On 2026-11-20 the small (S) items were fixed: W11, W12, W13, W15 and W16 in `3b997c3`; C16 and C18 in `62edc5f`; P9 (`trakt.json` is now mode 0600) in `d341d67`; P10 in `76ae467`; D10 in `f12f477`. W12 still wants a try in Safari.
- **C17** is fixed by `fa82202`, checked on 2026-11-20: on Windows "Speichern" opens the file with `?ticket=` and never `?token=` (`lib/screens/downloads.dart:32-44`). Against a running server the ticket loads its own address (200), is refused for another address, without sign-in and after signing out (401 each), and does not contain the token. Not clicked in the Windows app itself; `launchUrl` passes the link on unchanged.
- **S11** is fixed: `every_route_needs_sign_in_and_every_admin_route_needs_an_admin` (`src/server.rs`) calls every `ROUTES` entry without sign-in (401 expected, except a short open list) and every "Admin:" entry as a profile (403 expected). Checked that it fails when a route is moved into the open block.
- On 2026-11-20/21 six more known limits were fixed: a restore is refused while a Plex sync runs and no sync starts during a restore (`f89c3fc`); the download dialog closes when a download fails before its first frame (`0bb0d4d`); home-network discovery answers each sender at most 5 times a second (`bb70be4`); the app gives up an answer whose body stops coming for as long as the request timeout (`336f783`, `855a517`); a client has 30 s for a request's header, and a connection that sends nothing is closed after 30 s (`db35e69`, see `vendor/tiny_http/README.lichtspiel.md`); the macOS Release build sets `CODE_SIGN_INJECT_BASE_ENTITLEMENTS = NO` (`925d81a`, not yet checked with `codesign` on a Mac).
- **C7** is fixed (`456679e`): on a computer a picture over 3 MB is shrunk in the app before upload (`lib/picture.dart`, with the `image` package): turned upright, at most 3000 px, JPEG at quality 90 to 70 and smaller until it fits; a picture with see-through parts (a logo) stays a PNG. A 24-megapixel photo takes about 2 s, in an isolate.
- The container runs as non-root on the NAS (confirmed by the owner on 2026-11-21).
- **D8** is fixed (2026-11-21): `docker-compose.yml` is the only description of the container. `deploy.yml` starts it with `docker compose -p lichtspiel up -d --no-build` (the rollback with `LICHTSPIEL_IMAGE=lichtspiel:previous`) and fills in its Gitea variables; the graphics chip's group goes in as `LICHTSPIEL_GPU_GROUP`. `LICHTSPIEL_DOCKER_ARGS` and `LICHTSPIEL_MEDIA_GROUPS` are gone (a warning while still set). Not yet run on the NAS: the first deploy shows whether the runner has `docker compose`.
- **W14** is fixed (2026-11-21): a library of more than 2,000 titles has only the cards around the screen (the screen and 1,500 px above and below, whole rows); a block above and below stands in for the other rows, as tall as they are. Scrolling moves the window once the screen comes within 750 px of its edge; loaded cards are kept and put back, empty ones made anew. Measured in headless Edge at 1280 px with 80,000 titles: opening 0.13 s (0.24 s with the CPU slowed 4x), the scroll bar dragged to the end 5 ms (19 ms), never more than about 90 cards on the page. A letter lands on its first title, another window width keeps the title at the top, and coming back to the page finds the same place. Smaller libraries get all their cards as before.
- Two known limits are fixed: a restore now runs `Db::after_load` like a start does (old default server name, default playlists; test `a_restored_old_backup_gets_the_new_server_name_and_the_default_playlists`), and the app puts brackets around an IPv6 address typed without them (`Api.normalize`, with a test).

## Getting started

### Layout

| Folder | What it is |
|---|---|
| `src/` | The Rust server (one binary, `lichtspiel-server`), including the built-in Trakt sync (`src/trakt.rs`) |
| `vendor/tiny_http/` | The HTTP library, copied in and patched (header limits and a 30 s header deadline; look for `lichtspiel:` comments) |
| `web/` | The web app, plain JS with no build step, compiled into the binary |
| `clients/lichtspiel/` | The Flutter app (Windows, Android, macOS, iOS/iPadOS) |
| `plugins/` | Bundled Python plugins (`nfo-reader`, `webhook-notify`, `subtitles`, `wishlist`, `library-doctor`, `anime-details`, `smart-home`, `new-digest`, `push-notify`, `scrobble-more`) |
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
| C12 | Client | Minor player errors bring up the error screen | partly | M |
| D11 | Deploy | The Docker image was 616 MB; now 425 MB with Jellyfin's ffmpeg | check on the NAS | S |

Sizes: S is under an hour and a few lines, M is a few hours or needs a decision, L is a day or more. Paths in the client section are relative to `clients/lichtspiel/`.

### Client

Report: [reviews/2026-11-15-client.md](reviews/2026-11-15-client.md).

#### C12. Minor player errors bring up the error screen

- **Status:** partly fixed in `bdcc8c1`. The player now pauses before the error screen shows, so the sound no longer runs behind it. Since 2026-11-21 an error that comes while the film is playing is checked first: if the position moves on by at least 1.5 s within 3 s, it is ignored (only `debugPrint`). Not yet tried with a real failed HLS segment.
- **Where:** `lib/screens/player.dart:466-475` (the error listener), `:873` (the error text replaces the picture).
- **Problem:** media_kit passes many mpv log lines on as errors, and some are recoverable. With any quality other than "Original", one failed HLS segment request (a `tcp:` line) goes straight to "Wiedergabe fehlgeschlagen", even though mpv would have carried on.
- **Fix:** ignore an error while playback is still moving, for example when `player.state.playing` is true and the position keeps advancing over the next second or two. The media_kit docs don't say which errors are recoverable, so test it with a real failed segment.
- **Size:** M.

### Deploy

Report: [reviews/2026-11-15-ops.md](reviews/2026-11-15-ops.md).

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
| dart:io can't close a socket stuck in a TLS handshake. | `clients/lichtspiel/lib/api.dart:173` (a `shortcut:` comment) | Nothing simple in dart:io today; revisit when it gains a handshake timeout. One leaked socket per failed try. |

## Testing on real hardware

The review branches are deployed now, but these checks have not been recorded as done. Tick them off once tried:

- [x] The container runs as non-root (uid 1026, gid 100) after `sudo chown -R 1026:100` on the config folder, and the deploy adds `--group-add` for `/dev/dri` itself.
- [ ] The dashboard still shows "hw" for hardware transcoding.
- [ ] Open the web app and check the browser console for CSP errors (blocked scripts or frames).
- [ ] The relay: Lichtspiel on the relay computer is updated, and remote access still works.
- [ ] The macOS app: an `https://` address that only speaks http falls back to http on the home network (`<nas-lan-ip>`), and does **not** fall back when the server is reached from outside (`<your-server>`).
- [ ] Ticket renewal during a very long playback in a real browser (more than 6 hours).
- [ ] C12 above needs a real device (a failed HLS segment).
- [ ] The macOS Release build has no `get-task-allow` any more: `codesign -d --entitlements :- Lichtspiel.app`.

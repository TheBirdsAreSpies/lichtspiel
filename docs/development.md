# Development

For developers: where things are, how to build and test, and the conventions. What is still to do is in [todo.md](todo.md).

## Layout

| Folder | What it is |
|---|---|
| `src/` | The Rust server (one binary, `lichtspiel-server`), including the built-in Trakt sync (`src/connect/trakt.rs`) |
| `vendor/tiny_http/` | The HTTP library, copied in and patched (header limits and a 30 s header deadline; look for `lichtspiel:` comments) |
| `web/` | The web app, plain JS with no build step, compiled into the binary |
| `clients/lichtspiel/` | The Flutter app (Windows, Android, macOS, iOS/iPadOS) |
| `plugins/` | Bundled Python plugins (`nfo-reader`, `webhook-notify`, `subtitles`, `wishlist`, `library-doctor`, `cleanup`, `smart-collections`, `letterboxd-import`, `anime-details`, `smart-home`, `new-digest`, `push-notify`, `scrobble-more`, `wrapped`) |
| `Dockerfile`, `docker-compose.yml`, `.gitea/workflows/deploy.yml` | The image, the Synology setup, and the automatic deploy |

`README.md` and the guides it links in `docs/` describe how everything works.

## Build and test

| What | Command (from the repo root unless noted) |
|---|---|
| Server tests, as CI runs them | `cargo test --release --no-default-features` |
| Server build (desktop, with the status window) | `cargo build --release` |
| Client tests | `flutter test` in `clients/lichtspiel` |
| Client lint | `flutter analyze` in `clients/lichtspiel` |
| One plugin's tests | `python3 -I -m unittest discover -s plugins/<name>` |
| Web app lint and type check | `npm install` once, then `npm run check` (ESLint, and TypeScript checking the plain JS with `web/tsconfig.json` and `web/globals.d.ts`; nothing is built, the files are served as they are; there is no web test suite) |

Some server tests need ffmpeg on the PATH and skip themselves without it.

## Conventions

- Don't run `cargo fmt` over the tree. The code is not rustfmt-formatted; match the style around you.
- Lock order: take `store` before `db` (only `save` takes both). Call `app.save()` after a change.
- Keep `db` locked only briefly: every request waits for it. Held over a second, the server log says "the library was locked for … by file:line" (`src/server/held.rs`).
- Items and watch states live in `Tracked` maps. Change them through that API; going around it does not compile.
- A new API route needs its match arm in the fitting group in `src/api/routes/` (`public.rs` for addresses open to everyone, otherwise `account.rs`, `libraries.rs`, `items.rs` …; the groups are tried in the order listed in `route()`) and a line in `ROUTES` (`src/api/routes/docs.rs`); `ROUTES` is the API documentation.
- Web text: the English string is the key (`t('Sign in')`). Add the German text to `web/lang/de.js` whenever you add or change UI text.
- The Flutter client's UI text is German and written directly in the code.
- Line endings: most files are LF, a few are CRLF (`web/lang/de.js`, `web/app.js`, all of `src/api/routes/` …; `git ls-files --eol` shows them). Keep each file as it is; `sed -i` and editors that normalize a whole file change every line.
- A change to a bundled plugin's code needs a higher `version` in its `plugin.json` (`1.1` → `1.2`). A server that already has the plugin copies in the new `.py` files and `plugin.json` only when that version goes up (`src/server/bundled.rs`). Without the higher version, the NAS keeps running the old code.
- `cargo build --release --no-default-features` (the Docker build) must keep compiling. Anything that needs a window goes behind `#[cfg(feature = "console")]`.

## Commits and pushing

Commits use Conventional Commits with a scope, and the subject describes what the user sees, in lowercase. For example: `fix(web): editing details from a poster's menu changes the address`. Common scopes: `web`, `app`, `client`, `server`, `settings`, `plugins`, `ci`.

**Pushing to `main` deploys to the NAS.** Push only when the owner asks for it.

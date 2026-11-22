# Development

For developers: where things are, how to build and test, and the conventions. What is still to do is in [todo.md](todo.md).

## Layout

| Folder | What it is |
|---|---|
| `src/` | The Rust server (one binary, `lichtspiel-server`), including the built-in Trakt sync (`src/connect/trakt.rs`) |
| `vendor/tiny_http/` | The HTTP library, copied in and patched (header limits and a 30 s header deadline; look for `lichtspiel:` comments) |
| `web/` | The web app, plain JS with no build step, compiled into the binary |
| `web-vue/` | The Vue web app at `/vue/` (Vue 3, TypeScript, Vite); its build (`web-vue/dist`) is compiled into the binary |
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
| Vue web app: build | `npm ci` once, then `npm run build` in `web-vue` (vue-tsc, then Vite into `web-vue/dist`); `npm run check` only type checks |
| Vue web app: develop | `npm run dev` in `web-vue`: Vite with live reload, passing the API on to a running server (`LICHTSPIEL_URL`, default `http://localhost:8484`) |

Some server tests need ffmpeg on the PATH and skip themselves without it.

### The Vue web app

`web-vue/` is a second web app next to `web/`, served at `/vue/` (hash addresses: `/vue/#/item/12`).
It talks to the same JSON API and keeps the sign-in under the same keys as `web/`, so both share it.
So far it has signing in (password, two-step code, home profiles, first setup, with the phone by a code,
a forgotten password, invites; and signing in another device, `#/link/CODE`), home, libraries with
their filters (`?f.genre=…`, `web-vue/src/filters.ts`; advanced rules as `?rules=[…]`) and collections, a title's page (with "Add to …"
a playlist or collection, the choice of version, and "Edit details" for the owner), people, search, the player (with quality, speed, night mode, chapters, a still preview on the seek bar, skipping
intro, recap and credits, the countdown to the next episode, "Are you still watching?", the sleep timer,
"What happened so far", subtitle timing for the owner and the keyboard shortcuts on `?`), your own settings (account, playback, devices,
Trakt, watchlist feed; saved under the classic app's names, so both use them), the
watchlist, downloads (`#/downloads`; the button on an item's page), the dashboard, users and invites and the plugins for the owner (`#/dashboard`, `#/users`, `#/plugins`;
the charts are `web-vue/src/components/LineChart.vue` and `BarChart.vue`), the plugins' own panels
(Settings → Account, or a page in the left menu: `web-vue/src/components/PluginPanel.vue`), and playlists (smart ones with their rules; playing one goes on through it: `web-vue/src/queue.ts`); the rest is still
only in `web/` (see [todo.md](todo.md)).

- `build.rs` puts every file of `web-vue/dist` into the program (`VUE_FILES`, served by `vue_file` in
  `src/server/http.rs`). Build the Vue app first, then the server. Without a build `/vue/` says how
  to make one, so `cargo build` needs no Node. The Docker build has a Node stage that builds it.
- Text works as in `web/`: the English string is the key (`t('Sign in')`, `web-vue/src/i18n.ts`), and
  the German text goes into the **same** `web/lang/de.js`; Vite reads it at build time
  (`vite.config.ts`). So a change there needs a new build of both.
- Pictures, video and subtitles carry a ticket (`picture()`, `ticket()` in `web-vue/src/api.ts`), as in `web/`.

## Conventions

- Don't run `cargo fmt` over the tree. The code is not rustfmt-formatted; match the style around you.
- Lock order: take `store` before `db` (only `save` takes both). Call `app.save()` after a change.
- Keep `db` locked only briefly: every request waits for it. Held over a second, the server log says "the library was locked for … by file:line" (`src/server/held.rs`).
- Items and watch states live in `Tracked` maps. Change them through that API; going around it does not compile.
- A new API route needs its match arm in the fitting group in `src/api/routes/` (`public.rs` for addresses open to everyone, otherwise `account.rs`, `libraries.rs`, `items.rs` …; the groups are tried in the order listed in `route()`) and a line in `ROUTES` (`src/api/routes/docs.rs`); `ROUTES` is the API documentation.
- Web text: the English string is the key (`t('Sign in')`). Add the German text to `web/lang/de.js` whenever you add or change UI text.
- App text (Flutter) works the same way: the English string is the key (`tr('Sign in')`, `lib/i18n.dart`),
  and the German text goes into `clients/lichtspiel/lib/lang/de.dart`. Placeholders are `{name}`:
  `tr('{n} movies', {'n': n})`. The app's tests run in German (`test/flutter_test_config.dart`).
- Line endings: most files are LF, a few are CRLF (`web/lang/de.js`, `web/app.js`, all of `src/api/routes/` …; `git ls-files --eol` shows them). Keep each file as it is; `sed -i` and editors that normalize a whole file change every line.
- A change to a bundled plugin's code needs a higher `version` in its `plugin.json` (`1.1` → `1.2`). A server that already has the plugin copies in the new `.py` files and `plugin.json` only when that version goes up (`src/server/bundled.rs`). Without the higher version, the NAS keeps running the old code.
- `cargo build --release --no-default-features` (the Docker build) must keep compiling. Anything that needs a window goes behind `#[cfg(feature = "console")]`.

## Commits and pushing

Commits use Conventional Commits with a scope, and the subject describes what the user sees, in lowercase. For example: `fix(web): editing details from a poster's menu changes the address`. Common scopes: `web`, `app`, `client`, `server`, `settings`, `plugins`, `ci`.

**Pushing to `main` deploys to the NAS.** Push only when the owner asks for it.

The repository has two remotes: `origin` (Gitea on the NAS, which tests and deploys with
`.gitea/workflows/deploy.yml`) and `github`, a copy updated by hand with `git push github main`.
There `.github/workflows/ci.yml` runs every check from the table above (server, web, plugins, app) and
deploys nothing; Gitea ignores that folder.

### Releases

A release is a tag `vX.Y.Z` on GitHub. `.github/workflows/release.yml` then runs the server tests,
builds the Docker image for amd64 and arm64, publishes it as `ghcr.io/thebirdsarespies/lichtspiel`
(tags `X.Y.Z`, `X.Y` and `latest`) and makes a GitHub release with notes from the commits. A tag with
a dash (`v0.2.0-rc.1`) becomes a pre-release and gets only its own image tag. The NAS does not use
these images. The same workflow builds the app for Android (APK), Windows (zip) and macOS (zip, not
notarized) and attaches them to the release.

On Gitea the tag starts `.gitea/workflows/app.yml`: the NAS runner builds the Android APK in a Flutter
container (`clients/lichtspiel/android.Dockerfile`) and attaches it to the Gitea release of the tag,
which it makes when it is missing. Windows and macOS need runners with those systems, so they come
only from GitHub. The app's version is the tag's (`v0.2.0` → 0.2.0, build number 200).

1. Raise `version` in `Cargo.toml`, then run `cargo build --release --no-default-features` so
   `Cargo.lock` follows (the builds use `--locked`). Commit: `chore(release): X.Y.Z`.
2. `git tag vX.Y.Z`, then `git push && git push origin vX.Y.Z && git push github main && git push github vX.Y.Z`.

The GitHub workflow stops at once when the tag and `Cargo.toml` disagree.

**Signing the Android app.** Android installs a new APK over the old one only when both are signed
with the same key, so the release builds take it from the secrets `ANDROID_KEYSTORE` (the `.jks` file
in base64) and `ANDROID_KEYSTORE_PASSWORD`. Put the same two on GitHub (Settings → Secrets and
variables → Actions) and on Gitea (Settings → Actions → Secrets), so APKs from either update each
other. Without them the APK is signed with a throwaway key and the run warns. Make the key once and
keep the file and password somewhere safe (a lost key means everyone has to uninstall the app once):

```
keytool -genkeypair -keystore lichtspiel.jks -alias lichtspiel -keyalg RSA -keysize 4096 -validity 36500
base64 -w0 lichtspiel.jks     # this text is ANDROID_KEYSTORE (PowerShell: [Convert]::ToBase64String([IO.File]::ReadAllBytes("lichtspiel.jks")))
```

Locally, `LICHTSPIEL_KEYSTORE=<file>` and `LICHTSPIEL_KEYSTORE_PASSWORD` sign `flutter build apk
--release` the same way (`android/app/build.gradle.kts`; alias `lichtspiel` unless
`LICHTSPIEL_KEY_ALIAS` says otherwise). If Gitea's own run token may not make releases, add a
`RELEASE_TOKEN` secret there (an access token with read and write on the repository).

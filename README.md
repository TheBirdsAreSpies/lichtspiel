# Lichtspiel

**Lichtspiel** (from the old German word *Lichtspielhaus*, cinema) is a self-hosted streaming server for your own videos. It runs
anywhere Docker runs (a Synology or other NAS, a Linux server, a Raspberry Pi; images for Intel/AMD and
ARM), or as a single program on Windows or Linux.

## What it does

- **Movies and TV shows kept apart**, with shows grouped show → season → episode from the file names.
- **Plays every video container**: what a browser can play is sent as it is, everything else is converted on the fly by ffmpeg (also with Intel Quick Sync / VAAPI or NVIDIA NVENC).
- **A player of its own**: one seek bar with picture preview, subtitles, audio tracks, quality, speed, **Skip intro**, **Skip credits** and the next episode after a countdown.
- **Details from the internet**: plot, cast, posters, ratings (IMDb, Rotten Tomatoes, Metacritic, TMDB) and trailers; works without any key, better with a TMDB key.
- **Playlists**, smart playlists and default playlists; library filters for every detail; a **watchlist**; collections; rows for the time of year.
- **Downloads** in the original quality or converted for any device.
- **Users and invites**, each with their own watched state, ratings and playlists; per-person rights to download, convert and delete.
- **Dashboard** for admins: what is playing, bandwidth, processor and memory, free disk space, play history.
- **Trakt** built in, **sync with a running Plex server**, and an **import** of everything from your old server.
- **Plugins** in any language; an `.nfo` reader, a webhook / Discord notifier, a wish list (TMDB alone, or with Radarr / Sonarr), a subtitle downloader, a library check and anime details from AniList are included.
- **English and German**, a **full API**, and an **MCP server** for AI assistants such as Claude.

Every feature in detail: [docs/features.md](docs/features.md).

## Compared with other media servers

| | Lichtspiel | Plex | Jellyfin | Emby |
|---|---|---|---|---|
| Price | free | free core; many features need Plex Pass | free | free core; many features need Emby Premiere |
| Open source | yes (AGPL-3.0) | no | yes (GPL) | no (since 2018) |
| Account with an outside service | no, everything stays on your server | yes, a plex.tv sign-in | no | optional |
| Watching from outside your home | free (reverse proxy or own relay) | needs Plex Pass or Remote Watch Pass | free | free |
| Hardware conversion (Quick Sync / VAAPI / NVENC) | free | Plex Pass | free | Premiere |
| Skip intro / skip credits | built in | Plex Pass | with a plugin | Premiere |
| Downloads for offline watching | built in, original or converted | Plex Pass | yes | Premiere |
| Trakt | built in | via third-party tools | with a plugin | with a plugin |
| Import from an old server, sync with a running Plex | yes | — | via third-party tools | via third-party tools |
| Music | yes (artists, albums, songs, lyrics) | yes | yes | yes |
| Photos, live TV / DVR | no | yes | yes | yes |
| Apps | web app (casts to Chromecast); app for Windows, Android, macOS, iOS | apps for almost every device and smart TV | many apps, incl. smart TVs | many apps, incl. smart TVs |
| Server written in | Rust, one program | C++ | C# / .NET | C# / .NET |
| Maturity | young, one small project | large company, many years | large community, many years | company, many years |

In short: Lichtspiel is for **movies and TV shows** (and music), runs without any outside account,
and has everything included with no paid tier. Plex, Jellyfin and Emby do more (photos,
live TV) and have native apps for smart TVs, Roku, Apple TV and game consoles; on a
smart TV, Lichtspiel runs in the TV's browser, or the web app casts to a Chromecast. The details on Plex and Emby are as of
late 2026 and their paid tiers change from time to time.

## The interface

The main interface is a **web app**: open `http://<server>:8484` in any browser (computer,
phone, tablet, smart-TV browser). There you browse Movies and TV Shows, play videos, manage
playlists and smart playlists, invite people, switch plugins on and off and change settings.
Nothing needs installing on the devices you watch on.

A second, newer web app built with **Vue** is at `http://<server>:8484/vue/` (in the works: so
far signing in, home, libraries, a title's page, search and the player; everything else is still in
the web app above). Both share the sign-in.

There is also an **app** for Windows, Android, macOS, iPhone and iPad (Flutter, English
and German): see [clients/lichtspiel/README.md](clients/lichtspiel/README.md).

On a computer, the very first start opens the web app in your browser so you can create the
owner account. A small desktop **status window** (home rows, what's playing, playlists,
users, plugins) is **optional**: start the server with `--console` to get it. Clicking
**Open web app**, a poster, a menu entry or a card in it opens that page in the browser. The
Docker version has no window; you just use the web app.

Pictures of the web app pages are in `shots/web-*.png` (made by the self-test with a real
browser), and the status window is in `shots/now.png`.

## Quick start

- **Synology (Container Manager):** create `docker/lichtspiel` for the data, copy this folder to
  the NAS, check the paths in `docker-compose.yml`, create a project from it and open
  `http://<your-nas-ip>:8484`. Step by step, with automatic deploys from Gitea:
  [docs/install.md](docs/install.md).
- **Any other Docker host:** put your folders, user and groups in a `.env` file next to
  `docker-compose.yml` and run `docker compose up -d`, or use the ready-made image
  `ghcr.io/thebirdsarespies/lichtspiel`:
  [docs/install.md](docs/install.md#any-other-docker-host).
- **Windows, for trying it out:** install ffmpeg, `cargo build --release`, run
  `target\release\lichtspiel-server.exe` and open `http://localhost:8484`. Options:
  [docs/install.md](docs/install.md#run-it-on-windows-for-trying-it-out).

## Documentation

| Page | What is in it |
|---|---|
| [docs/features.md](docs/features.md) | Every feature in detail: player, playlists, filters, downloads, dashboard, trash, Trakt … |
| [docs/install.md](docs/install.md) | Synology install, any other Docker host, automatic build and deploy (Gitea), running on Windows, command-line options |
| [docs/remote-access.md](docs/remote-access.md) | Reaching it from outside: reverse proxy, HTTPS, redirect, relay |
| [docs/metadata.md](docs/metadata.md) | Where movie and show details come from; naming files the Plex way; extras and trailers; `.lichtspielignore` |
| [docs/watching.md](docs/watching.md) | When something counts as watched, credits and intro detection, theme music, deep analysis |
| [docs/libraries.md](docs/libraries.md) | Automatic scanning, keeping in step with a Plex server, importing from your old server (on Docker: `OLD_SERVER`) |
| [docs/api.md](docs/api.md) | The API in short |
| [docs/plugins.md](docs/plugins.md) | Writing a plugin: `plugin.json`, events, answers, user panels |
| [docs/development.md](docs/development.md) | For developers: layout, build and test commands, conventions |
| [docs/todo.md](docs/todo.md) | What is still open: checks on real hardware, known problems, feature ideas |

## What's in this folder

| Path | What |
|---|---|
| `src/` | the server (Rust) |
| `web/` | the web app (one page, plain HTML + JavaScript, built into the server) |
| `web-vue/` | the Vue web app at `/vue/` (Vue 3 + TypeScript, built with Vite; the build goes into the server) |
| `clients/lichtspiel/` | the app for Windows, Android, macOS and iOS (Flutter) |
| `plugins/` | the bundled plugins (each copied into the data folder once; one deleted there stays deleted) |
| `vendor/tiny_http/` | the HTTP library, copied in and patched |
| `Dockerfile`, `docker-compose.yml` | Synology / Docker install |
| `.gitea/workflows/deploy.yml` | automatic build and deploy to the Synology after a push to `main` |
| `.gitea/workflows/app.yml`, `.github/workflows/release.yml` | a version tag builds the app (Android on Gitea; Android, Windows, macOS on GitHub) for the release |
| `docs/` | the documentation above |
| `shots/` | status-window picture, web app pictures and the last self-test report |

## The data folder

| Path | What |
|---|---|
| `lichtspiel.db` | the library, users, watch state, playlists and settings (SQLite; a save writes only what changed) |
| `metadata/` | posters, backgrounds and actor photos |
| `plugins/` | installed plugins |
| `trakt.json` | the Trakt settings and everyone's Trakt logins |
| `stats.json` | the dashboard's bandwidth, processor and memory history |
| `themes/` | show theme music cut from an episode's intro |
| `downloads/` | copies converted for the Downloads page (deleted after *Keep converted downloads* days) |
| `cache/`, `transcode/` | thumbnails, seek-bar previews and transcoding work files; safe to delete |

Older versions kept the library in `library.json`. The first start of this version moves it
into `lichtspiel.db` and renames the old file to `library.json.migrated`.
For a backup, stop the server and copy the folder. While the server is running,
`lichtspiel.db-wal` belongs to `lichtspiel.db` and must be copied with it.

## License

Lichtspiel is free software under the **GNU Affero General Public License, version 3 or
later** ([LICENSE](LICENSE)). You may use, change and pass it on; whoever passes on a changed
version, or runs one for other people over a network, must offer them its source code under
the same license. The bundled `vendor/tiny_http/` keeps its own MIT / Apache-2.0 license.

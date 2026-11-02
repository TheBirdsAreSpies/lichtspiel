# Lichtspiel

**Lichtspiel** (from the old German word *Lichtspielhaus*, cinema) is a self-hosted streaming server for your own videos, made to run on a Synology NAS (or any
Linux/Windows machine):

- **Movies and TV shows kept apart** — each library is either Movies, TV Shows or Other
  videos. TV shows are grouped show → season → episode automatically from file names like
  `Show Name/Season 1/Show Name - S01E02.mkv`, `1x02` or `Episode 2`.
- **Plays every video container** — mkv, avi, mp4, ts, m2ts, wmv, flv, vob, webm, mov and
  about 40 more. Files a browser can play are sent as they are (with seeking); everything else
  is converted on the fly (H.264/AAC) by ffmpeg, as a progressive MP4 or as HLS. Quality
  choices from Original down to 320p, audio track choice, subtitles (embedded and `.srt` /
  `.vtt` / `.ass` side files), hardware conversion (Intel Quick Sync / VAAPI on Synology,
  NVIDIA NVENC).
- **Playlists** — your own (hand-picked, drag order), **smart playlists** (rules such as
  "genre is Comedy AND year ≥ 2000 AND not watched, newest first, max 50") and six **default
  playlists** the server keeps up to date (Continue Watching, Recently Added Movies, Recently
  Added Episodes, Unwatched Movies, Favorites, Top Rated). Playlists can be filtered (kind,
  owner, title, size, shared, pinned) and sorted (title, last changed, size, length …); the
  contents of any playlist can be filtered and sorted again.
- **Users and invites** — create an invite link (valid 7 days by default, single use),
  choose which libraries the person may see and whether the server may convert video for
  them, then send them the link. They pick their own name and password. Each user has their
  own watched state, resume points, ratings, favourites and playlists.
- **Plugins (mods)** — drop a folder into the `plugins` folder; plugins can fill in movie
  details, react to events (new item, playback, invites …) and add their own API calls. Two
  are included: an `.nfo` file reader and a webhook / Discord notifier.
- **A full API** — every feature in the web page is an HTTP + JSON call. The list is built
  in: open `http://<server>:8484/api/docs` or the **API** page in the web app.

## The interface

The main interface is a **web app**: open `http://<server>:8484` in any browser (computer,
phone, tablet, smart-TV browser). There you browse Movies and TV Shows, play videos, manage
playlists and smart playlists, invite people, switch plugins on and off and change settings.
Nothing needs installing on the devices you watch on.

On a computer, the very first start opens the web app in your browser so you can create the
owner account. A small desktop **status window** (home rows, what's playing, playlists,
users, plugins) is **optional**: start the server with `--console` to get it. Clicking
**Open web app**, a poster, a menu entry or a card in it opens that page in the browser. The
Synology/Docker version has no window; you just use the web app.

## Movie and show details

New movies and TV shows get their details from the internet automatically: **plot, cast
(with roles and photos), directors, writers, genres, rating, age rating, studio, poster and
background picture**, plus **season posters and every episode's title, plot, picture and air
date**. The movie or show page shows all of it, with links to IMDb, TMDB, TVmaze or
Wikipedia.

- Without any setup, movies come from **Wikidata + Wikipedia** (the plot is the article's
  plot section) and TV shows from **TVmaze**.
- For better matches, cast photos for movies and plots in your language, get a free API
  key at themoviedb.org (Settings → API) and paste it under **Settings → Movie and show
  details → TMDB API key**.
- **Plots from IMDb:** get a free key at omdbapi.com and paste it under **Settings → Movie
  and show details → OMDb API key**. From then on movie and show plots are IMDb's (in
  English); your movies get them in the background right after you save the key. (IMDb has
  no free service of its own and doesn't allow reading its pages, so OMDb is the way to get
  its plots.) Without the key the plot comes from Wikipedia or TMDB as before.
- **Language:** choose the language of titles, plots and episode names under **Settings →
  Movie and show details**; each library can have its own on the **Libraries** page ("Same as
  server" by default). Changing it downloads the details again in the background. Show
  plots come from Wikipedia in your language; episode names are only translated with a TMDB
  key.
- **Progress:** while details are being downloaded (or credits detected, or ratings
  updated) the top bar shows what is happening and how much is left, and open pages update
  themselves when new details arrive.
- **Ratings:** the page shows scores from **IMDb**, **Rotten Tomatoes** (Tomatometer),
  **Metacritic** and **TMDB** as plain text badges. Rotten Tomatoes and Metacritic come from
  Wikidata; IMDb from IMDb's free ratings file (about 9 MB, downloaded once a week; can be
  switched off in Settings); TMDB needs a TMDB key. Your own rating is given with **five
  stars, half stars included**: click the left half of a star for a half star (each half star
  is 1 point on the 0–10 scale used by the API, so 3½ stars = 7).
- If something was matched wrongly, or not at all, open it and press **Fix match…**: search
  by title and year and click the right entry. **Refresh details** downloads them again.
- All pictures (posters, backgrounds, actor photos, episode pictures) are saved in the data
  folder under `metadata`, so pages load them from your own server. When no background is
  found online, a still from the video is used.
- **Actors are shared:** each actor's photo is saved once (`metadata/people`) and used by every
  movie and show they appear in. Under each actor you see how many items of the same library
  they are in; click them to see everything of theirs in your libraries (API:
  `/library/people/{key}/media`).
- **Only real photos are used as posters.** Some Wikipedia articles show the film's logo (an
  SVG drawing) instead of a poster; such pictures, title cards and landscape images are
  skipped. Broken pictures saved by an older version are removed when the server starts and
  downloaded again.
- Details you change yourself with **Edit details** are never overwritten.
- For best results name files like `Movies/The Matrix (1999)/The Matrix (1999).mkv` and
  `TV Shows/Twin Peaks/Season 1/Twin Peaks - S01E02.mkv`.

## When something counts as watched

A video counts as watched at **whichever comes first: 90 % or the start of
the end credits**. The credits are found in two ways:

1. a **chapter** in the file called "Credits", "End Credits", "Abspann", "Générique" …;
2. otherwise the server **looks at the picture** of the last part of the video (the last 30 %,
   1 to 15 minutes): end credits are a dark screen with a little bright text, for at least
   20 seconds. Only key frames are read, so a file takes seconds, not minutes. (Credits shown over moving
   pictures can be missed.)

Credits detection runs by itself: with **Settings → Generate credits video markers** on "as a
scheduled task and when media is added" (the default) new files are checked within a minute;
on "as a scheduled task" once a day. To run it now, press **Detect credits** next to a library
on the **Libraries** page, or **Credits… → Detect credits again** on a movie or episode (API:
`POST /library/sections/{id}/credits`, `POST /library/metadata/{id}/credits`). A file that fails more than 3 times is not retried automatically, and libraries with
"Enable credits detection" off are skipped. You can always type the time yourself under
**Credits…** (e.g. `1:52:30`). Without a credits time, 90 % is used. **Settings → Mark videos
as watched** offers other choices.

## New files are found by themselves

With **Settings → Scan my library automatically** on (the default here), the server looks at
the library folders every 15 seconds. When files are added, renamed or removed it waits until
they stop changing (so a file that is still being copied isn't read half-way) and then scans
that library; only new or changed files are read. Files added while the server was off are
found right after it starts.

## Import everything from your old media server

On the **Libraries** page, under **Import from your old media server**, point the server at
the old server's library database (`com.plexapp.plugins.library.db`, usually in
`Plug-in Support/Databases`). It is found by itself in the usual places on Windows and in
the Docker folder `/oldserver` (see `docker-compose.yml`; or set `MEDIASERVER_OLD_SERVER`).
The old database is only read, from a copy; nothing of the old server is changed.

1. Press **Look inside**: you see the old libraries with their folders and the old accounts.
2. If the files are now at another place (e.g. `D:\Movies` on the old PC, `/media/Movies`
   in Docker), type the new folder next to each old one.
3. Choose which of your users each old account becomes (same names are matched by
   themselves; the old owner is you).
4. Press **Import**. The top bar shows the progress; a report is shown when it's done.

What comes over: the **libraries** (created if missing, then scanned), **watched state and
how often, resume points and your ratings** for every chosen account, **credits markers**,
**details** (title, original title, plot, tagline, studio, age rating, release date, genres;
these are then kept as if you had edited them), **actors with their roles, directors and
writers** (with the actor photos the old server had) and **normal playlists**. Smart playlists are
listed in the report but not imported (their filters are written differently). Running the
import again is safe: playlists with the same name are updated, not doubled.

Pictures of the web app pages are in `shots/web-*.png` (made by the self-test with a real
browser), and the status window is in `shots/now.png`.

## Install on the Synology (Container Manager)

You need DSM 7.2 or newer with **Container Manager** installed (Package Center).

1. In **File Station**, create the folder `docker/mediaserver` (it holds the server's
   database, settings and plugins). Your videos can stay where they are, e.g. in the shared
   folder `video` with sub-folders `Movies` and `TV Shows`.
2. Copy this whole `app` folder to the NAS, e.g. to `docker/mediaserver-build`.
3. Open `docker-compose.yml` and check the two paths on the left side of the `volumes:` lines:
   - `/volume1/docker/mediaserver` → where the server keeps its data
   - `/volume1/video` → your videos (mounted read-only as `/media`)
   If you have no Intel graphics (or your model has none), delete the `devices:` lines.
4. Container Manager → **Project** → **Create** → name `mediaserver`, path
   `docker/mediaserver-build`, "Use existing docker-compose.yml" → Next → Done.
   The first build takes 10–20 minutes on a NAS (it compiles the server).
5. Open `http://<your-nas-ip>:8484` in a browser. The first visit asks you to create the
   owner account.
6. **Libraries → Add library**: name "Movies", type Movies, folder `/media/Movies`; then
   "TV Shows", type TV shows, folder `/media/TV Shows`. The scan starts immediately and the
   details (plot, cast, posters) are downloaded in the background.
7. **Settings → Server → "Scan my library periodically"**: tick it so new files show up by
   themselves (it is off by default; the interval is hourly).
8. **Settings → Transcoder → Hardware acceleration** is **Automatic** by default: if the
   graphics chip works (most Synology "+" models with an Intel CPU) it is used, otherwise
   the processor. The panel says which one is in use. If video converted by the graphics
   chip looks blocky, tick **"Graphics chip: convert by quality instead of bitrate"**.

The Docker image was built and tested on an Intel/AMD (x86-64) computer: the full self-test
passes inside it, including converting video and downloading movie details. It was not
tried on a real Synology, so the graphics-chip conversion there is untested.

To reach it from outside your home, use the Synology reverse proxy (Control Panel → Login
Portal → Advanced → Reverse Proxy) with HTTPS, and put that address into Settings →
"Address used in invite links" so invite links point to it.

## Run it on Windows (for trying it out)

1. Install **ffmpeg** (e.g. `winget install ffmpeg`) so `ffmpeg` and `ffprobe` are on the PATH,
   or set their full paths in Settings.
2. `cargo build --release` in this folder, then run `target\release\mediaserver.exe`.
3. Open `http://localhost:8484`.

Data goes to a `data` folder next to where you start it. Options:

| Option | Environment variable | Meaning |
|---|---|---|
| `--data <folder>` | `MEDIASERVER_DATA` | where the database, plugins and cache live |
| `--port <n>` | `MEDIASERVER_PORT` | port (default 8484, or the one saved in Settings) |
| `--bind <address>` | `MEDIASERVER_BIND` | network address to listen on (default all) |
| `--console` | | also open the optional desktop status window |
| `--no-browser` | | don't open the browser on the first start |
| `--self-test <file.json>` | | builds test videos, runs every feature, writes a report |
| `--screenshot <file.png>` | | draws the status window and saves a picture |

## The API in short

Sign in once, then send the token with every call:

```
POST /api/login            {"username":"me","password":"…"}   → {"token":"…"}
GET  /library/sections     X-Token: <token>
GET  /library/sections/1/all?type=movie&genre=Drama&year>=2000&unwatched=1&sort=rating:desc
GET  /library/metadata/42/children
GET  /video/transcode/42/stream.mp4?quality=720p-4&offset=600
POST /playlists            {"title":"Short comedies","kind":"smart",
                            "query":{"match_all":true,"rules":[
                              {"field":"genre","op":"is","value":"Comedy"},
                              {"field":"duration","op":"lt","value":95}],
                             "sort":"year:desc","limit":50}}
GET  /playlists?kind=smart&sort=updated_at:desc
POST /api/invites          {"email":"friend@example.com","days":7,"allowed_sections":[1]}
```

The token can also go in `Authorization: Bearer <token>` or `?token=<token>`. Lists come
back as `{"MediaContainer": {"size": …, "Metadata": [ … ]}}`. `GET /api/filters` lists every
field, operator and sort key; `GET /api/docs` lists every call.

## Writing a plugin

A plugin is a folder in `<data>/plugins/<name>/` with a `plugin.json`:

```json
{
  "id": "my-plugin", "name": "My plugin", "version": "1.0",
  "description": "What it does",
  "command": ["python3", "main.py"],
  "hooks": ["metadata.lookup", "library.item.added", "api.call"],
  "settings": {"greeting": "hello"},
  "timeout_seconds": 20
}
```

For each event the server starts `command` inside the plugin folder and writes one JSON object
to its standard input:

```json
{"event": "metadata.lookup", "data": { …the item… }, "settings": { … }, "server": {"url": "http://127.0.0.1:8484", "version": "…", "data_dir": "…"}}
```

The plugin may print a JSON answer as its last line of output:

- `metadata.lookup` → `{"metadata": {"title": "…", "summary": "…", "year": 1999, "genres": ["…"], "rating": 7.5, "extra": {"any": "field"}}}`
- `api.call` → any JSON; it is returned to whoever called `POST /api/plugins/<id>/call/<action>`.

Events: `metadata.lookup`, `library.item.added`, `library.item.removed`,
`library.scan.finished`, `playback.started`, `playback.progress`, `playback.stopped`,
`playback.watched`, `user.invited`, `user.joined`, `playlist.created`, `playlist.updated`,
`server.started`, `api.call`.

New plugins start switched off; switch them on in the **Plugins** page (where their settings,
errors and a log of recent runs are shown too). Any language works — the Docker image includes
Python 3.

## What's in this folder

| Path | What |
|---|---|
| `src/` | the server (Rust) |
| `web/` | the web app (one page, plain HTML + JavaScript, built into the server) |
| `plugins/` | the two bundled plugins (copied into the data folder on first start) |
| `Dockerfile`, `docker-compose.yml` | Synology / Docker install |
| `shots/` | status-window picture and the last self-test report |

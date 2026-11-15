# Lichtspiel

**Lichtspiel** (from the old German word *Lichtspielhaus*, cinema) is a self-hosted streaming server for your own videos, made to run on a Synology NAS (or any
Linux/Windows machine):

- **Movies and TV shows kept apart** — each library is either Movies, TV Shows or Other
  videos. TV shows are grouped show → season → episode automatically from file names like
  `Show Name/Season 1/Show Name - S01E02.mkv`, `1x02` or `Episode 2`.
- **Plays every video container** — mkv, avi, mp4, ts, m2ts, wmv, flv, vob, webm, mov and
  about 40 more. Files a browser can play are sent as they are (with seeking); everything else
  is converted on the fly (H.264/AAC) by ffmpeg, as a progressive MP4 or as HLS. Quality
  choices from Original down to 320p, playback speed (0.5× to 2×, also with the < and > keys),
  audio track choice, subtitles (embedded and `.srt` /
  `.vtt` / `.ass` side files), hardware conversion (Intel Quick Sync / VAAPI on Synology,
  NVIDIA NVENC).
- **The player** has its own controls: one seek bar for the whole video (also while converting
  and for movies split over files) with what is loaded, a yellow mark where the credits start
  and a **picture preview** when you point at it; back 10 s / forward 30 s, volume, the time
  played or left and when it ends; a **subtitle menu** (with sizes; the language picked last
  comes back by itself) and **⚙** for quality, speed, audio track and *Play the next episode by
  itself*; picture in picture and full screen (double click). During the credits it offers
  **Skip credits** / **Next episode**; at the end the next episode (or what is queued) starts
  after a 10-second countdown. Keyboard: Space/K, ←/→ (Shift: 1 min), ↑/↓, M, F, P, C, N, 0–9,
  < > — **?** lists them all. Media keys and the lock screen work too. `#/play/<id>?t=90` is a
  link that starts playing.
- **Playlists** — your own (hand-picked, drag order), **smart playlists** (rules such as
  "genre is Comedy AND year ≥ 2000 AND not watched, newest first, max 50") and six **default
  playlists** the server keeps up to date (Continue Watching, Recently Added Movies, Recently
  Added Episodes, Unwatched Movies, Favorites, Top Rated). Playlists can be filtered (kind,
  owner, title, size, shared, pinned) and sorted (title, last changed, size, length …); the
  contents of any playlist can be filtered and sorted again.
- **Library filters** — filters on every library page (29 for movies,
  14 for shows, 18 for episodes): genre, year, decade, age rating, collection, director, actor,
  writer, producer, country, studio / network, resolution (16k … SD),
  HDR, Dolby Vision, HDR10+, Dolby Atmos, unwatched, in progress, unmatched, video / audio /
  subtitle format, audio channels, audio and subtitle language, edition, label, folder and
  duplicates. They sit in one **Filter ▾** menu on a library page: each
  opens its values with how many items have them, yes/no filters switch on and off, and the
  active ones show as chips (click one to remove it). Labels and folder are
  only for the owner and duplicates sits apart at the end as the advanced filter. Labels, collections and edition can be set
  under **Edit details**.
- **Library pages** — each library has **Library**, **Collections**, **Playlists** and
  **Categories** tabs. A movie page shows rows of other editions of the same
  film, related movies, **More by** the director and **More with** each of the first two actors
  (shows: related shows and More with).
- **The "⋯" menu** on every poster and on a title's page: **Watch together** (a link; whoever
  plays, pauses or jumps moves everyone), Share, Add to queue, Add to (playlist or collection),
  mark watched, and for the owner Refresh metadata, Analyze (read the files again), Fix match,
  **Optimize** (a saved MP4 copy that plays directly instead of being converted), Grant access;
  then Download…, View play history, Report a problem (the owner sees them under
  **Reported problems**) and Media info.
- **Watchlist** — **+ Watchlist** on a title's page, *Add to watchlist* in any poster's "⋯"
  menu, or for several picked posters at once. The **Watchlist** page (left menu) sorts by when
  you added them, title, year or rating, and the home page shows it as a row. A movie or episode
  leaves the list once you have watched it, a show once every episode is watched. Smart playlists
  can use it too (*On your watchlist*). Kept per person.
- **Downloads** — **Download…** on a title's page, in any poster's "⋯" menu (movies,
  episodes, seasons, whole shows) or for several picked posters. Pick a quality: *Original*
  hands out the file as it is, right away; 1080p 8 Mbps down to 320p converts it on the server to
  an MP4 (H.264, stereo AAC) that plays everywhere — one after the other in the background, a
  movie in several parts joined into one file. A file that already fits the quality is sent as
  it is. The **Downloads** page (left menu, with a count while something converts) shows each
  one with its progress, size and a **Save** button, tries failed ones again and clears finished
  ones; admins can see everyone's. Converted copies are deleted 7 days after they were made
  (*Settings → Keep converted downloads*). Under **Users & invites** each person can be allowed
  to download or not (*May download*; on by default); without it the plain
  file download is refused too. Converting also needs *May convert*.
- **Dashboard** (admins, the pulse button at the top — grey when quiet, yellow with the number
  of streams while something plays): what is playing right now — who, on which device
  ("Plex for Samsung — TV 2020", "Chrome · Windows"), local or remote with the address and
  bandwidth, picture and sound as source → output ("1080p (HEVC Main) → 720p (H.264) —
  Transcode (hw)", "German (AC3 5.1) → AAC Stereo — Transcode") and the position. **Stop** ends
  a playback with an optional message: converted streams end at once, the web player closes and
  shows the message. Below: tiles and charts for bandwidth (local and remote), processor and
  memory (all, Lichtspiel or the system) and streams over real time, the last 12 / 24 hours,
  7 / 30 / 90 days, the last year or everything (minute and hour averages are kept in
  `stats.json` in the data folder), a list of users with who is active (seen in the last
  5 minutes), and the **play history** as a chart per day (per week for a year): plays or hours,
  movies and episodes stacked, for everyone or one person, with the most played titles, top users
  and the latest plays. Every play of a minute or more counts, with the time really watched
  (pauses don't count); marked watched by hand counts as a whole play. Processor and memory are
  read on Linux (Synology) and Windows.
- **The left menu is yours**: "⋯" next to *Libraries* → **Edit the menu** to sort it (drag, or
  the arrows) and to unpin libraries or Playlists; unpinned ones wait under **More**. Kept per
  person on the server.
- **A library's "⋯" menu** (in the left menu, on its page and under Libraries): **Scan library
  files**, **Grant access**, **Manage library** → Edit (name, language, credits detection and
  folders in one window), Refresh all metadata, **Empty trash**, Analyze, Delete; and Unpin.
- **Trash**: with *Empty trash automatically after every scan* off (Settings), files gone from
  disk stay as *Unavailable* — watched state and details kept — until the trash is emptied. A
  library folder that cannot be read (a share not mounted) never empties anything.
- **Deleting files**: with *Allow media deletion* on (the default), admins — and people ticked
  *May delete files* under Users & invites — get **Delete** in a poster's "⋯" menu. It deletes the
  movie's or episode's files (a season or show: all its episodes) from disk; only files inside the
  library's folders are touched.
- **Rows for the time of year** — the home page adds a row while the season lasts:
  **Halloween** (1 Oct – 2 Nov: horror and anything with Halloween in it), **Christmas**
  (1–26 Dec), **New Year's Eve** (27 Dec – 1 Jan) and **Valentine's Day** (7–14 Feb). Titles
  that name the day come first, then fitting genres by rating; a show's episode joins when its
  own title fits. `GET /hubs?date=12-24` shows the rows of another day.
- **Users and invites** — create an invite link (valid 7 days by default, single use),
  choose which libraries the person may see and whether the server may convert video for
  them, then send them the link. They pick their own name and password. Each user has their
  own watched state, resume points, ratings, favourites and playlists.
- **Plugins (mods)** — drop a folder into the `plugins` folder; plugins can fill in movie
  details, react to events (new item, playback, invites …) and add their own API calls. Three
  are included: an `.nfo` file reader, a webhook / Discord notifier and **Trakt** (below).
- **Trakt** — switch the *Trakt* plugin on under **Plugins** and give it a Trakt app's Client ID
  and Client Secret (create one at <https://trakt.tv/oauth/applications/new>, redirect URI
  `urn:ietf:wg:oauth:2.0:oob`). Then everyone connects their own Trakt account under
  **Settings → Trakt** (a code to type in at trakt.tv/activate). While a movie or episode plays,
  Trakt shows "watching now" (paused when paused); once it is watched it goes into the Trakt
  history, and so does anything marked as watched by hand. **Bring in my Trakt history** marks
  everything watched on Trakt as watched here too (matched by IMDb / TMDB / TVDB ids, so the
  details download should be on). The logins are kept in the plugin's folder (`tokens.json`).
- **English and German** — the web app follows the browser's language; anyone can switch it
  under **Settings → Your account** (kept per browser). Language packs live in `web/lang/`
  (`de.js`); to add another, copy it, translate the texts, and list it in `src/server.rs`
  (`WEB_LANGS`) and `web/index.html`.
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
  **Metacritic** and **TMDB**, each with the site's mark (drawn by the web app; Rotten
  Tomatoes shows the tomato from 60 % and the splat below, Metacritic's square takes the
  colour of the score). Rotten Tomatoes and Metacritic come from
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

## Naming files (the Plex way)

Movies are read the way Plex's naming guide describes:

| What | Example |
|---|---|
| Movie in its own folder, or on its own | `Avatar (2009)/Avatar (2009).mkv`, `Avatar (2009).mkv` |
| Exact match by id (folder or file name; `{tvdb-…}` for shows, in the show's folder) | `Batman Begins (2005) {imdb-tt0372784}`, `{tmdb-272}` |
| Edition | `Blade Runner (1982) {edition-Final Cut}.mkv` |
| Split movie (up to 8 parts, played one after the other) | `The Dark Knight (2008) - pt1.mp4`, `- pt2.mp4` (also cd, disc, disk, dvd, part) |
| Extras in a subfolder of the movie's folder | `Avatar (2009)/Trailers/Teaser.mkv` — also `Behind The Scenes`, `Deleted Scenes`, `Featurettes`, `Interviews`, `Scenes`, `Shorts`, `Other`, `Extras` |
| Extras next to the movie | `Avatar (2009)-trailer.mkv` — also `-behindthescenes`, `-deleted`, `-featurette`, `-interview`, `-scene`, `-short`, `-other` |
| A trailer file on its own | `Avatar (2009)/trailer.mkv` or `movie-trailer.mkv` |

**Trailers:** the **▶ Trailer** button on a movie or show page plays a trailer file when there
is one. Otherwise it plays the first online trailer found while matching (from TMDB, or from
Wikidata when no TMDB key is set). Online trailers play in the page from YouTube, nothing is
downloaded, and they also appear in the **Extras** row marked "online". Titles matched before
this version get their online trailers after **Refresh metadata**.

Extras are shown on the movie's page, not as movies of their own; an extras folder without a
movie next to it is read as ordinary movies. When a file name finds nothing online (a release
name like `funxd.mkv`), the name of the movie's own folder is tried. Extras in TV libraries are
left out.

**Leaving files out:** a `.lichtspielignore` file in any library folder lists what the scan
skips in that folder and below, one pattern per line (`#` starts a comment):

```
Samples/
*.sample.mkv
Extras/2019/*
```

A pattern without `/` matches a file or folder name at any depth; one with `/` matches the
path from the folder the file is in. `*` stands for any characters except `/`, `**` for any
characters, `?` for one. Upper and lower case count the same.

## Deep analysis

The bitrate of a file is only its average: a busy scene can need far more for a while, and a
player with only a little video stored ahead of it then stops to wait. **Deep analysis**
(*Ausführliche Analyse*) reads every packet of the file, without decoding anything, and plays them
back on paper. For eight connection speeds, from 5 to 500 Mbit/s, it finds the lowest speed at which
the player's store never runs empty. The player's store holds 7.2 seconds of the connection, and
playback starts after 5 seconds of filling it.

It only matters when a stream has a speed limit (a quality chosen in the player, or the limit for
people outside the home network). A video is sent as it is when the measured speed plus 5 % fits the
limit. A video that was not analyzed is taken to need twice its bitrate, so it is converted sooner.
On the home network without a limit it changes nothing.

Run it with **Deep analysis** in the menu of a video or a library, or turn on **Settings → Scheduled
tasks → Run deep analysis** (off by default): one video at a time inside the
maintenance window, the latest changed first, and again when a file changes. It reads every file
completely, so a large library keeps the disks busy for a long time. **Media info** shows the result
under *Connection speed needed*. Plain **Analyze** only reads the file's header, which takes
seconds.

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

### Theme music

A movie's or show's page plays its theme music quietly, once, and fades it out when you leave.
The music comes from a `theme.mp3` in its folder (also `theme-1.mp3`, or .m4a, .flac, .ogg, .wav,
.aac). A show without one gets its theme from an online list, by its TheTVDB number, the first
time its page opens; it is then kept in `data/themes`. Movies only have music from a file. Turn
the playing off under **Settings → Web → Play theme music**, and the download off under **Download
show theme music**. Browsers may need one click on the page first.

### Skip intro (TV shows)

The server finds a show's intro by **listening**: it reads the sound of the first 10 minutes
of every episode in a season and compares the episodes with each other. The stretch of sound
(longer than 20 seconds) that most episodes share is the intro, even when it starts at a
different time in each episode because of a cold open. While it plays, the player shows
**Skip intro**, which jumps to its end; the seek bar marks it in blue.

It needs at least two episodes of a season, and runs by itself like credits detection
(**Settings → Generate intro video markers**, default "as a scheduled task and when media is
added"). To run it now, press **Detect intros** next to a TV library on the **Libraries** page
(API: `POST /library/sections/{id}/intros`, `POST /library/metadata/{id}/intros` for one show,
season or episode). Each TV library has **Enable intro detection** to switch it off. When fewer
than half of a season's episodes share an intro, the season gets none. An
intro can also be set by hand with `PUT /library/metadata/{id}/markers` and
`{"intro": [start ms, end ms]}`.

## New files are found by themselves

With **Settings → Scan my library automatically** on (the default here), the server looks at
the library folders every 60 seconds. When files are added, renamed or removed it waits until
they stop changing (so a file that is still being copied isn't read half-way) and then scans
that library; only new or changed files are read. Files added while the server was off are
found right after it starts.

## Keep in step with a running Plex server

On the **Libraries** page, under **Sync with Plex**, enter the Plex server's address
(e.g. `http://192.168.1.10:32400`) and one X-Plex-Token per person (Plex Web: ⋯ on a movie →
Get info → View XML; the address ends in `X-Plex-Token=…`). Every few minutes (or with
**Sync now**) watched state, resume points and ratings of movies and episodes go both ways.
Videos are matched by their file path; when Plex sees the folders under other paths,
**Test connection** suggests a folder translation. A change on one side goes to the other;
when both sides changed, the side watched more recently wins. The first sync loses nothing:
watched on either side stays watched and a rating set on either side is kept.
Under **Direction** you can make one side the master instead: *Plex → here only* or
*Here → Plex only* copy the master's state as it is and never send anything the other way.

## Import everything from your old server

On the **Libraries** page, under **Import from your old server**, point the server at
the old server's library database (`com.plexapp.plugins.library.db`, usually in
`Plug-in Support/Databases`). It is found by itself in the usual places on Windows and in
the Docker folder `/oldserver` (on a Synology set `OLD_SERVER` to the old data folder in a `.env`
file next to `docker-compose.yml`, see there; or set `LICHTSPIEL_OLD_SERVER`).
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

1. In **File Station**, create the folder `docker/lichtspiel` (it holds the server's
   database, settings and plugins). Your videos can stay where they are, e.g. in the shared
   folder `video` with sub-folders `Movies` and `TV Shows`.
2. Copy this whole `app` folder to the NAS, e.g. to `docker/lichtspiel-build`.
3. Open `docker-compose.yml` and check the two paths on the left side of the `volumes:` lines:
   - `/volume1/docker/lichtspiel` → where the server keeps its data
   - `/volume1/video` → your videos (mounted read-only as `/media`)
   If you have no Intel graphics (or your model has none), delete the `devices:` lines.
4. Container Manager → **Project** → **Create** → name `lichtspiel`, path
   `docker/lichtspiel-build`, "Use existing docker-compose.yml" → Next → Done.
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
   HDR video (HDR10, HLG) that has to be converted is tone mapped to normal colours, so it does
   not look grey; switch it off with **"Convert HDR video to normal colours"** (it needs an
   ffmpeg with the `zscale` and `tonemap` filters). Dolby Vision profile 5 is not handled.

The Docker image was built and tested on an Intel/AMD (x86-64) computer: the full self-test
passes inside it, including converting video and downloading movie details. It was not
tried on a real Synology, so the graphics-chip conversion there is untested.

### Automatic build and deploy (Gitea)

`.gitea/workflows/deploy.yml` builds and starts a new version on the Synology after every push to
`main`: tests (`docker build --target test .`), the image, the container is replaced, then a health
check. If the new version does not become healthy, the previous one is started again and the run
fails (you get Gitea's usual mail). Run it by hand under *Actions → Build and deploy → Run workflow*.

It needs a Gitea runner (`act_runner`) with the label `nas` on the Synology that can use its
Docker (the same runner that deploys other projects there). The workflow uses these settings (Gitea: repository →
Settings → Actions → Variables); all are optional:

| Variable | Default | |
|---|---|---|
| `LICHTSPIEL_CONFIG` | `/volume1/docker/lichtspiel` | folder for the database and settings (`/config`) |
| `LICHTSPIEL_MEDIA` | `/volume1/video` | folder with the videos, mounted read-only as `/media` |
| `LICHTSPIEL_PORT` | `8484` | port on the Synology |
| `LICHTSPIEL_DOCKER_ARGS` | `--device /dev/dri:/dev/dri` | more `docker run` options; set a single space on a model without an Intel graphics chip |

The container is called `lichtspiel`; the workflow removes one with that name and creates it
again, with the data folder untouched. If you started Lichtspiel before as a Container Manager
project, delete that project (not the folder) so the two do not fight over the name and the port.
The first run takes 10–20 minutes (it compiles everything); later runs reuse the compiled
dependencies and take a few minutes.

To reach it from outside your home, use the Synology reverse proxy (Control Panel → Login
Portal → Advanced → Reverse Proxy) with HTTPS, and put that address into Settings →
"Address used in invite links" so invite links point to it.

## Reaching it from outside

Under **Settings → Network**:

- **HTTPS**: enter a certificate file (PEM, with the chain) and its key file (PEM), then restart.
  The port then speaks HTTPS instead of HTTP. Both files are checked when you save (a key that
  does not belong to the certificate is refused). A renewed certificate (certbot, acme.sh) is
  picked up within about 30 seconds, without a restart; if the new files are wrong the old
  certificate stays in use and the log says why. There is no built-in Let's Encrypt client.
  `--tls-cert` / `--tls-key` (or `LICHTSPIEL_TLS_CERT` / `LICHTSPIEL_TLS_KEY`) override the settings.
- **Pass http:// on to https:// from port**: a second port (e.g. 80) that answers plain
  http:// with a redirect to the https:// address. Takes effect after a restart.
- **Relay**: when the router can't forward a port, run `lichtspiel --relay --relay-secret <secret>`
  on a computer that can be reached from the internet (`--relay-public` is where callers come in,
  default `0.0.0.0:8484`; `--relay-tunnel` is where the home server calls in, default
  `0.0.0.0:8485`; also `LICHTSPIEL_RELAY_*`). Then enter that computer's `name:8485` and the
  secret in the settings. The home server keeps a few connections open to the relay and hands
  each caller to its own web server, so the relay only passes bytes on. The secret itself is
  never sent. The traffic is private only when the home server speaks HTTPS with a
  certificate for the relay's name; with plain HTTP everything crosses the internet readable.
  One relay serves one home server. Tested on one computer only, not over a real network.

## Run it on Windows (for trying it out)

1. Install **ffmpeg** (e.g. `winget install ffmpeg`) so `ffmpeg` and `ffprobe` are on the PATH,
   or set their full paths in Settings.
2. `cargo build --release` in this folder, then run `target\release\lichtspiel-server.exe`.
3. Open `http://localhost:8484`.

Data goes to a `data` folder next to where you start it. Options:

| Option | Environment variable | Meaning |
|---|---|---|
| `--data <folder>` | `LICHTSPIEL_DATA` | where the database, plugins and cache live |
| `--port <n>` | `LICHTSPIEL_PORT` | port (default 8484, or the one saved in Settings) |
| `--bind <address>` | `LICHTSPIEL_BIND` | network address to listen on (default all) |
| `--console` | | also open the optional desktop status window |
| `--no-browser` | | don't open the browser on the first start |
| `--tls-cert <file>`, `--tls-key <file>` | `LICHTSPIEL_TLS_CERT`, `LICHTSPIEL_TLS_KEY` | serve HTTPS with this certificate and key |
| `--relay` | | run as the relay instead (see "Reaching it from outside") |
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
POST /api/downloads        {"ids":[42],"quality":"720p-4"}   → then GET /api/downloads/{id}/file
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
- `api.call` → any JSON; it is returned to whoever called `POST /api/plugins/<id>/call/<action>`
  (`data` holds `action`, `body`, `params`, `user` and `admin`). An answer with
  `"mark_watched": [{"id": 42} | {"type": "movie", "ids": {"imdb": "tt…"}} | {"type": "show", "ids": {…}} |
  {"type": "episode", "show": {"ids": {"tvdb": 123}}, "season": 1, "episode": 2}, …]` (optional
  `watched_at`, `plays`) marks those as watched for the caller; the caller gets `matched`,
  `marked` and `not_found` back instead.

Playback events (`playback.started`, `playback.progress` — sent when a player pauses or plays
on — `playback.stopped`, `playback.watched`) carry `user`, `time` (ms), `progress` (percent),
`state` and `item`: `{"id", "type", "title", "year", "duration_ms", "ids": {"imdb", "tmdb", "tvdb" …}}`,
for an episode also `"season"`, `"episode"` and `"show": {"title", "year", "ids"}`. A title
marked as watched by hand sends `playback.watched` with `"manual": true`.

With `"user_panel": true` in `plugin.json` each person gets a panel of the plugin under
**Settings**. The page asks the plugin with `api.call` action `panel` and shows
`{"title", "text", "link", "buttons": [{"label", "action", "primary", "confirm"}]}`; a button
calls its action. That answer may say `{"message"}` or `{"error"}`, or show a code to type in
elsewhere — `{"message", "code", "link", "poll": {"action": "poll", "seconds": 5}}` — in which
case the page calls the `poll` action every few seconds until an answer comes without `poll`.
The body of these calls includes `lang` (the page's language).

Events: `metadata.lookup`, `library.item.added`, `library.item.removed`,
`library.scan.finished`, `playback.started`, `playback.progress`, `playback.stopped`,
`playback.watched`, `item.problem`, `download.ready`, `user.invited`, `user.joined`, `playlist.created`, `playlist.updated`,
`server.started`, `api.call`.

New plugins start switched off; switch them on in the **Plugins** page (where their settings,
errors and a log of recent runs are shown too). Any language works — the Docker image includes
Python 3.

## What's in this folder

| Path | What |
|---|---|
| `src/` | the server (Rust) |
| `web/` | the web app (one page, plain HTML + JavaScript, built into the server) |
| `plugins/` | the bundled plugins (each copied into the data folder once; one deleted there stays deleted) |
| `Dockerfile`, `docker-compose.yml` | Synology / Docker install |
| `shots/` | status-window picture and the last self-test report |

## The data folder

| Path | What |
|---|---|
| `lichtspiel.db` | the library, users, watch state, playlists and settings (SQLite; a save writes only what changed) |
| `metadata/` | posters, backgrounds and actor photos |
| `plugins/` | installed plugins |
| `downloads/` | copies converted for the Downloads page (deleted after *Keep converted downloads* days) |
| `cache/`, `transcode/` | thumbnails, seek-bar previews and transcoding work files; safe to delete |

Older versions kept the library in `library.json`. The first start of this version moves it
into `lichtspiel.db` and renames the old file to `library.json.migrated`.
For a backup, stop the server and copy the folder. While the server is running,
`lichtspiel.db-wal` belongs to `lichtspiel.db` and must be copied with it.

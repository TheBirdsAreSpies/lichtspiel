# Features

Everything Lichtspiel does, in detail. The short overview is in the [README](../README.md).

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
  are included: an `.nfo` file reader, a webhook / Discord notifier and **Subtitles**, which
  downloads missing subtitles for new movies and episodes from OpenSubtitles and saves them
  next to the video (`Film.de.srt`). Subtitles needs a free API key from
  <https://www.opensubtitles.com/consumers> in its settings (`api_key`; `username` and
  `password` of an OpenSubtitles account give more downloads a day), the languages
  (`languages`, default `["de", "en"]`) and a writable media folder. Languages the video
  already has are skipped; once the daily download limit is reached it waits until the next
  day. `POST /api/plugins/subtitles/call/test` checks the key.
- **Trakt** — built in. An admin creates a Trakt app (<https://trakt.tv/oauth/applications/new>, redirect
  URI `urn:ietf:wg:oauth:2.0:oob`) and enters its Client ID and Client Secret under **Settings → Your
  account → Trakt**. Then everyone connects their own Trakt account there (a code to type in at
  trakt.tv/activate). While a movie or episode plays, Trakt shows "watching now" (paused when paused);
  once it is watched it goes into the Trakt history, and so does anything marked as watched by hand.
  **Sync now** (and, when switched on, an automatic sync every hour or so) brings watched titles,
  ratings and the watchlist together in both directions. Nothing is ever removed on either side; when
  a rating differs, the side that changed it since the last sync wins. Titles are matched by IMDb / TMDB
  / TVDB ids, so the details download should be on. The logins and settings are kept in
  `data/trakt.json` (the `/config` volume in Docker), so they survive image updates.
- **English and German** — the web app follows the browser's language; anyone can switch it
  under **Settings → Your account** (kept per browser). Language packs live in `web/lang/`
  (`de.js`); to add another, copy it, translate the texts, and list it in `src/server.rs`
  (`WEB_LANGS`) and `web/index.html`.
- **A full API** — every feature in the web page is an HTTP + JSON call. The list is built
  in: open `http://<server>:8484/api/docs` or the **API** page in the web app.

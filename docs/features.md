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
- **Swapping a file**: a movie or episode whose file is replaced by another one (a better
  version, another name) stays the same title: watched state, ratings, playlists and details
  stay. The new file counts as the same movie when it has the same title and year, or is the
  only other movie in the old file's own folder; an episode when it has the same season and
  number. This works with the old file deleted first or the new one copied in first, and a title
  already in the trash takes the new file over at the next scan.
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
  details, react to events (new item, playback, invites …) and add their own API calls. Eight
  are included: an `.nfo` file reader, a webhook / Discord notifier, the **Wish list**,
  **Subtitles**, the **Library doctor**, **Anime details**, **Smart home** and **New on Lichtspiel**.
  **New on Lichtspiel** collects the movies and episodes added and sends everyone who wants it one
  message a week (`every`: `"week"` or `"day"`; `weekday`, default `"friday"`; a day missed while
  the server was off is made up for) with the posters: an e-mail, and/or a Telegram message with
  the posters as an album. Episodes are grouped by show (`3 new episodes · S01E01–E02, S01E04`),
  titles link to their page when the address used in invite links is set, and `skip_libraries`
  leaves libraries out by name. Under **Settings → Account** each person enters their e-mail
  address, connects Telegram (a link to the bot and a code, sent there with Start) and can ask for
  a preview; admins can also send it to everyone at once. E-mail needs an SMTP server
  (`smtp_host`, `smtp_port`, `smtp_security`: `"starttls"`, `"ssl"` or `"none"`, `smtp_user`,
  `smtp_password`, `smtp_from`), Telegram the token of a bot made with @BotFather
  (`telegram_bot_token`).
  **Smart home** switches Home Assistant scenes when playback starts, pauses and stops (dim the
  lights, lights back on). It needs the Home Assistant address (`home_assistant_url`) and a
  long-lived access token (`home_assistant_token`; your profile in Home Assistant → Security), and
  one rule per room in `rules`: `{"name": "Living room", "players": ["Samsung"], "users": [],
  "types": ["movie", "episode"], "play": "scene.movie", "pause": "scene.pause", "stop":
  "scene.lights_on"}`. `players` are parts of the player's name (empty = every player), `users`
  names people and `types` the kinds of title (empty = all). An action is a scene, script or
  automation (`scene.…`, `script.…`, `automation.…`), a Home Assistant webhook (`webhook:<id>`) or
  any `http(s)://` address, which gets the event as JSON (`event`, `user`, `player`, `type`,
  `title`, `show`, `year`, `progress`). Under **Settings → Account** everyone can switch it off for
  themselves; admins also see the players seen lately with the rules they fall under, and can try
  each action.
  **Anime details** finds anime on AniList (no key needed) and adds the AniList score, links to
  AniList and MyAnimeList and the animation studio (`studio`; instead of the TV channel). If the
  settings say so it also takes AniList's title (`title`: `"romaji"`, `"english"` or `"native"`;
  empty keeps the title), its summary in English (`summary`: `"missing"` = only when there is
  none, `"always"`, `"never"`) and its genres (`genres`). Anime are the titles in the libraries
  named in `libraries`; with none named, titles whose original title is Japanese and that are
  animated.
  The **Library doctor** looks through the library after each scan that changed something, once
  a day and on **Check now**, and shows admins under **Settings → Account** what may need a hand:
  titles that are there twice (versions merged on purpose do not count), videos below a width
  (`min_width`, default 1280 = HD; 0 = do not check), videos with no audio in the chosen
  languages (`languages`, default `["de"]`; it notes subtitles in them) or whose audio has no
  language at all, files whose streams could not be read, movies without a year, episodes
  without season or number, and titles for which no details were found online. Each entry links
  to its title and can be ignored; `skip_libraries` leaves libraries out by name (an anime
  library in Japanese …).
  The **Wish list** gives everyone a panel under **Settings → Account**: search for a movie or show,
  wish for it, and see how the wishes stand (wished, downloading with its percentage, downloaded,
  here). A wish goes straight to Radarr (movies) or Sonarr (shows), which look for it; a title
  wished by several people goes there once. When the title arrives in the library the wish is
  marked as here, and leaves the list 30 days later. Admins see every wish and who made it and
  can remove one. Settings: `radarr_url` and `radarr_api_key` and/or `sonarr_url` and
  `sonarr_api_key` (Settings → General in Radarr / Sonarr), optionally the quality profile and root
  folder by name (`radarr_quality_profile`, `radarr_root_folder`, … — empty takes the first), at
  most `max_open_wishes` open wishes per person (default 10, 0 = no limit), and `only_admins`.
  **Subtitles**
  downloads missing subtitles for new movies and episodes from OpenSubtitles. They go to the
  server's data folder, `<data>/subtitles/` with the video's folder below it (for
  `/media/Movies/Heat (1995)/Heat (1995).mkv`:
  `<data>/subtitles/media/Movies/Heat (1995)/Heat (1995).de.srt`), so the media folder may stay
  read-only; with `"next_to_video": true` they go next to the video instead. Subtitles needs a
  free API key from <https://www.opensubtitles.com/consumers> in its settings (`api_key`;
  `username` and `password` of an OpenSubtitles account give more downloads a day) and the
  languages (`languages`, default `["de", "en"]`). Languages the video
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

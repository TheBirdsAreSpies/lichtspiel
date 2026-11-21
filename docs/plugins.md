# Writing a plugin

Back to the [README](../README.md).

A plugin is a folder in `<data>/plugins/<name>/` with a `plugin.json`. Put it there and press
**Reload plugins**, or zip the folder and press **Upload plugin (.zip)** on the Plugins page (admins
only; `POST /api/plugins/upload` with `{data: base64}`). The zip may hold `plugin.json` at the top
or in one folder that holds everything (GitHub's "Download ZIP" works). The plugin goes into
`<data>/plugins/<id>/`, switched off unless it says `enabled_by_default`. Uploading a plugin that is
installed already asks first, then writes the zip's files over it; its settings and the files the zip
does not have (tokens, a cache) stay. At most 35 MB zipped and 200 MB unpacked; zip files with a
password or over 4 GB (ZIP64) are not taken. A plugin runs any program it likes on the server, so only
upload plugins you trust.

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
{"event": "metadata.lookup", "data": { …the item… }, "settings": { … }, "server": {"url": "http://127.0.0.1:8484", "version": "…", "data_dir": "…", "name": "…", "public_url": "…"}}
```

`name` is the server's name and `public_url` the address people reach it at, from the settings (empty
when not set; for links in messages: `<public_url>/web/#/item/42`).

The plugin may print a JSON answer as its last line of output:

- `metadata.lookup` → `{"metadata": {"title": "…", "summary": "…", "year": 1999, "genres": ["…"], "rating": 7.5, "extra": {"any": "field"}}}`
  (`data` is the item with `section_name`, and for an episode `parent_title` and `show_title`).
  It is asked when a title is new and again after the online details were looked up — for a
  movie or show also when none were found —, so what it answers wins over them. A field in
  `extra` is replaced as a whole: to add to `ids` or `ratings`, send them back with the
  item's own.
- `api.call` → any JSON; it is returned to whoever called `POST /api/plugins/<id>/call/<action>`
  (`data` holds `action`, `body`, `params`, `user`, `admin` and `may_delete` — whether the caller may
  delete files, see below). An answer with
  `"mark_watched": [{"id": 42} | {"type": "movie", "ids": {"imdb": "tt…"}} | {"type": "show", "ids": {…}} |
  {"type": "episode", "show": {"ids": {"tvdb": 123}}, "season": 1, "episode": 2}, …]` (optional
  `watched_at`, `plays`) marks those as watched for the caller; the caller gets `matched`,
  `marked` and `not_found` back instead. An answer with `"delete_media": [42, …]` (movie, episode,
  season or show ids) deletes their files as if the caller had pressed **Delete** on each: only
  when **Allow media deletion** is on and the caller may delete. The caller gets `deleted` (how
  many went) and, if any could not, an `error` instead of the plugin's `message`.

An answer to any event with `"collections": {"IMDb Top 250": [42, …], …}` makes each named
collection hold exactly those titles: they are put in it, and titles the plugin put in it before
that are not listed any more are taken out (an empty list empties it). Collections given by hand
are left alone, and the plugin's stay when details are downloaded again. The caller of an
`api.call` gets `collections_added` and `collections_removed` back.

An `api.call` answer may bring things in for whoever called (an import from elsewhere):
`"set_ratings": [{"id", "rating"}]` (0–10, only where they have no rating yet),
`"add_to_watchlist": [ids]` and `"save_playlist": {"title", "summary", "ids"}` (their own playlist
of that title is filled anew, else a new one is made). Titles they cannot see are left out; the
answer gets `rated`, `watchlisted`, `playlist_id` and `playlist_items` instead. A panel may ask for
a file — `"upload": {"label", "action", "accept"}` — whose action gets `{"file"}` (base64, at most
2.5 MB) and `{"name"}` in the body (only in the web page), and a `poll` answer may say what it
waits for in `"wait"`.

Playback events (`playback.started`, `playback.progress` — sent when a player pauses or plays
on — `playback.stopped`, `playback.watched`) carry `user`, `time` (ms), `progress` (percent),
`state`, `player` and `item`: `{"id", "type", "title", "year", "duration_ms", "genres", "poster", "ids": {"imdb", "tmdb", "tvdb" …}}`,
for an episode also `"season"`, `"episode"` and `"show": {"title", "year", "ids", "poster", "genres"}`. `poster` is the
picture's file on the server's computer (or `null`). `player` says
which device plays: `{"device": "Plex for Samsung — TV 2020" | "Chrome on Windows" …, "ip", "local",
"client"}`. A title marked as watched by hand sends `playback.watched` with `"manual": true` (and no
`player`).

`library.item.added` carries `id`, `title`, `section_id` and `item`: the same fields as a playback
event's item plus `section_id` and `files` — for each file `path`, `size`, `container`,
`duration_ms`, `streams` (`kind`, `codec`, `language`, `channels`, `width`, `height`, `forced`,
`sdh`) and `external_subtitles` (the subtitle files next to it). A show or season has no files.

An answer with `"refresh_subtitles": true` to an event about an item (one with `id`) makes the
server look for that item's subtitle files again at once: next to the video, and in
`<data>/subtitles/` below the video's folder (`/media/Movies/X` → `<data>/subtitles/media/Movies/X`,
`C:\Movies\X` → `<data>/subtitles/C/Movies/X`), where a plugin may save subtitles when the media
folder is read-only.

`calendar` asks for what is coming (for **Coming soon**, every three hours) with `{"start", "end"}`
(YYYY-MM-DD); the answer `{"entries": [{"type": "movie" | "episode", "title", "year", "season",
"episode", "name", "date", "poster", "note"}]}` lists them (`title` is the show's for an episode;
`poster` only as an `https://` address; `note` e.g. `"in cinemas"`, `"digital release"`, `"on disc"`).
An episode of a show in the library is linked to it.

`disk.low` is sent when a disk that holds a library or the data folder has less free space than
**Warn when a disk is nearly full** (Settings → Library, 5 % unless changed): `{"holds": ["Movies",
"data"], "paths", "free_bytes", "total_bytes", "free_percent", "threshold_percent"}` (`"data"` is the
data folder). The disks are looked at every 30 minutes; one is reported once, and again only after
it had 2 points more room in between (or the server restarted).

`schedule.daily` is sent once a day (UTC) with `{"date": "YYYY-MM-DD", "last": "…"}`; a day missed
while the server was off is sent right after the next start.

With `"user_panel": true` in `plugin.json` each person gets a panel of the plugin under
**Settings**. The page asks the plugin with `api.call` action `panel` and shows
`{"title", "text", "link", "buttons": [{"label", "action", "args", "primary", "confirm"}]}`; a button
calls its action, with its `args` (any JSON) in the body (`"danger": true` makes its
`confirm` question red). The panel may also have a search field,
`"input": {"placeholder", "action", "label", "value"}`, whose action gets `{"query"}`, and a list,
`"items": [{"title", "subtitle", "status", "status_kind", "image", "buttons"}]` (`status_kind`
`"default"` shows the status green, `"smart"` in the accent colour; `image` is an `https://`
address or one on the server). An answer may say `{"message"}` or `{"error"}`, bring a panel to
show at once (`{"panel": {…}}`, e.g. search results), or show a code to type in elsewhere —
`{"message", "code", "link", "poll": {"action": "poll", "seconds": 5}}` — in which case the page
calls the `poll` action every few seconds until an answer comes without `poll`. The body of these
calls includes `lang` (the page's language). An entry may link to a page of the app with
`"href": "#/item/42"`; a list where no entry has an `image` gets no picture column. With
`"panel_admins_only": true` only admins see the panel (the plugin still checks `data.admin`).

With `"reads_library": true` in `plugin.json` the plugin gets `server.library`, an address with
its own key that answers `GET` with every movie and episode — `{"sections": [{"id", "name",
"kind", "paths"}], "items": [...]}`, each item like `library.item.added`'s `item`, a version
merged into another with `version_of`, plus `added_at`, `last_played` (unix seconds, 0 = never),
`watched_by` and `in_progress_by` (user names) — and `users` (every user name), so it can look
through the whole library. Titles in the trash are left out.

The server sends UTF-8 on standard input. On Windows, Python reads standard input in the console's
code page, so read the bytes (`json.loads(sys.stdin.buffer.read())`) and answer with escaped JSON
(`json.dumps(…)` as it is, without `ensure_ascii=False`).

Events: `metadata.lookup`, `library.item.added`, `library.item.removed`,
`library.scan.finished`, `playback.started`, `playback.progress`, `playback.stopped`,
`playback.watched`, `item.problem`, `download.ready`, `user.invited`, `user.joined`, `playlist.created`, `playlist.updated`,
`server.started`, `schedule.daily`, `disk.low`, `calendar`, `api.call`.

New plugins start switched off; switch them on in the **Plugins** page (where their settings,
errors and a log of recent runs are shown too). Any language works — the Docker image includes
Python 3.

Ideas for more plugins are in [todo.md](todo.md).

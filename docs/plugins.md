# Writing a plugin

Back to the [README](../README.md).

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
  (`data` is the item with `section_name`, and for an episode `parent_title` and `show_title`).
  It is asked when a title is new and again after the online details were looked up — for a
  movie or show also when none were found —, so what it answers wins over them. A field in
  `extra` is replaced as a whole: to add to `ids` or `ratings`, send them back with the
  item's own.
- `api.call` → any JSON; it is returned to whoever called `POST /api/plugins/<id>/call/<action>`
  (`data` holds `action`, `body`, `params`, `user` and `admin`). An answer with
  `"mark_watched": [{"id": 42} | {"type": "movie", "ids": {"imdb": "tt…"}} | {"type": "show", "ids": {…}} |
  {"type": "episode", "show": {"ids": {"tvdb": 123}}, "season": 1, "episode": 2}, …]` (optional
  `watched_at`, `plays`) marks those as watched for the caller; the caller gets `matched`,
  `marked` and `not_found` back instead.

Playback events (`playback.started`, `playback.progress` — sent when a player pauses or plays
on — `playback.stopped`, `playback.watched`) carry `user`, `time` (ms), `progress` (percent),
`state`, `player` and `item`: `{"id", "type", "title", "year", "duration_ms", "genres", "ids": {"imdb", "tmdb", "tvdb" …}}`,
for an episode also `"season"`, `"episode"` and `"show": {"title", "year", "ids"}`. `player` says
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

`schedule.daily` is sent once a day (UTC) with `{"date": "YYYY-MM-DD", "last": "…"}`; a day missed
while the server was off is sent right after the next start.

With `"user_panel": true` in `plugin.json` each person gets a panel of the plugin under
**Settings**. The page asks the plugin with `api.call` action `panel` and shows
`{"title", "text", "link", "buttons": [{"label", "action", "args", "primary", "confirm"}]}`; a button
calls its action, with its `args` (any JSON) in the body. The panel may also have a search field,
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
merged into another with `version_of` — so it can look through the whole library. Titles in the
trash are left out.

The server sends UTF-8 on standard input. On Windows, Python reads standard input in the console's
code page, so read the bytes (`json.loads(sys.stdin.buffer.read())`) and answer with escaped JSON
(`json.dumps(…)` as it is, without `ensure_ascii=False`).

Events: `metadata.lookup`, `library.item.added`, `library.item.removed`,
`library.scan.finished`, `playback.started`, `playback.progress`, `playback.stopped`,
`playback.watched`, `item.problem`, `download.ready`, `user.invited`, `user.joined`, `playlist.created`, `playlist.updated`,
`server.started`, `schedule.daily`, `api.call`.

New plugins start switched off; switch them on in the **Plugins** page (where their settings,
errors and a log of recent runs are shown too). Any language works — the Docker image includes
Python 3.


Plugins planned next, in order: [plugin-ideas.md](plugin-ideas.md).

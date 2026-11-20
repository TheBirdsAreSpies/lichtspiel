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


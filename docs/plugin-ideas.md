# Plugin roadmap

Back to [Writing a plugin](plugins.md).

Plugins planned for Lichtspiel, built one after another in this order. Each one is a folder in
`plugins/` with tests (`python3 -I -m unittest discover -s plugins/<name>`), starts switched off and
is described in [features.md](features.md) once it is done. Tick an entry off with its commit.

## Step 0: what the plugins need from the server first

Today `library.item.added` carries only `id`, `title` and `section_id`, and a plugin has no way to
ask the server for more (it gets no sign-in). Several plugins below need more, so these come first:

- [x] **Item details in library events.** `library.item.added` carries `item` like the playback
  events do (type, title, year, ids, show/season/episode) plus the item's `files` (paths,
  streams, subtitle files next to it). Needed by 1, 3, 4. (`library.item.removed` stays as it
  is: the item is gone by then.)
- [x] **A daily event.** `schedule.daily` once a day (and once after a start if the last one was
  missed), so a plugin can do periodic work without waiting for a scan. Needed by 2, 6.

## The plugins

| # | Plugin | Hooks | What it does | Status |
|---|---|---|---|---|
| 1 | **Subtitles** (`subtitles`) | `library.item.added`, `api.call` | Downloads `.srt` files in the chosen languages from OpenSubtitles (needs a free API key) and puts them next to the video as `Name.de.srt`. Later: fetching them for the whole library at once needs a way for a plugin to list the library. | [x] |
| 2 | **Wish list** (`wishlist`) | `user_panel`, `api.call`, `library.item.added`, `schedule.daily` | Each person searches for a movie or show in their panel and wishes for it; the plugin hands the wish to Radarr / Sonarr and shows its state (wished, downloading, here). When the title arrives the wish is marked as here. The plugin panel got a search field, lists and buttons with arguments for it. | [x] |
| 3 | **Library doctor** (`library-doctor`) | `library.scan.finished`, `api.call`, `user_panel` (admins) | Finds duplicates, low resolutions, files without a German (or chosen) audio track or subtitles, and file names the scanner could not read well. The report is a panel. Plugins got `reads_library` (an address listing the whole library) and admin-only panels for it. | [x] |
| 4 | **Anime details** (`anime-details`) | `metadata.lookup` | Better titles, summaries and episode numbering for anime from AniList (no key needed), for libraries chosen in the settings. Built with the AniList / MyAnimeList ids and score, the animation studio, and title, summary and genres as chosen; episode numbering is left out, as AniList has no episode lists. | [x] |
| 5 | **Smart home** (`smart-home`) | `playback.started`, `playback.progress`, `playback.stopped` | Calls a Home Assistant scene or webhook when playback starts, pauses and stops (dim the lights, lights back on). Per person and per player, so only the living-room TV dims the living room. Built with rules (players, people, kinds of title) in the settings; playback events got `player` for it. Everyone can switch it off for themselves; admins see the players seen lately. | [x] |
| 6 | **New on Lichtspiel** (`new-digest`) | `library.item.added`, `schedule.daily`, `user_panel` | Collects what was added and sends one message a week (e-mail over SMTP, or Telegram) with posters. Each person can switch it on or off in their panel. Built with e-mail (posters inside) and Telegram (posters as an album, connected with a code), weekly or daily; the titles and posters are read from the library when it is sent, after the online details were found. Items in events got `poster` and plugins the server's `name` and `public_url` for it. | [x] |
| 7 | **Telegram / Pushover** (`push-notify`) | like `webhook-notify` | The same events as `webhook-notify`, formatted for Telegram bots and Pushover, which need their own message format. (Discord, ntfy and Home Assistant already work with `webhook-notify`.) Built with one message per scan for new titles, posters, and Telegram chats and groups connected with a code in an admin panel. | [x] |
| 8 | **SIMKL / MyAnimeList** (`scrobble-more`) | `playback.watched`, `user_panel` | Marks what someone finished as watched on SIMKL and MyAnimeList, signed in per person through the panel with a code, like Trakt. (Letterboxd has no open API, so it is left out.) | [ ] |
| 9 | **Lichtspiel Wrapped** (`wrapped`) | `playback.watched`, `user_panel` | A look back at the year for each person: hours watched, favourite genres, longest binge, most watched show. | [ ] |

## Notes

- On a Synology the media folders are mounted read-only into the container, so plugin 1 keeps its
  subtitles in `<data>/subtitles/` (the server looks there too); `"next_to_video": true` puts them
  next to the video on a writable mount.
- Plugins that keep state (2, 6, 9) keep it in a JSON file in their own folder, which a newer
  bundled version never overwrites.

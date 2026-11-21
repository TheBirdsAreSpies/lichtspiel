# To do

What is still open: checks on real hardware, two known problems, and feature ideas for later.
Tick an entry off with its commit when it is done. Build, test and conventions are in
[development.md](development.md).

## Checks on real hardware

- [ ] **NAS:** the dashboard shows "hw" for a converted video (Jellyfin's ffmpeg,
  `jellyfin-ffmpeg7`, since 2026-11-20), and a converted video plays. If it fails, going back is
  one commit (the old `apt-get install` line in the `Dockerfile`).
- [ ] **Relay:** Lichtspiel on the relay computer is updated, and remote access still works.
- [ ] **macOS app:** an `https://` address that only speaks http falls back to http on the home
  network, and does **not** fall back when the server is reached from outside.
- [ ] **macOS Release build:** no `get-task-allow` any more:
  `codesign -d --entitlements :- Lichtspiel.app`.
- [ ] **Browser:** ticket renewal during a playback of more than 6 hours.
- [ ] **App player (see below):** a failed HLS segment does not bring up the error screen.

## Known problems

- **Minor player errors bring up the error screen** (app). media_kit passes many mpv log lines on as
  errors, and some are recoverable: with any quality other than "Original", one failed HLS segment
  (a `tcp:` line) could show "Wiedergabe fehlgeschlagen" although mpv would carry on. Since
  2026-11-21 an error during playback is ignored when the position moves on by at least 1.5 s
  within 3 s (`clients/lichtspiel/lib/screens/player.dart`, the error listener). Not yet tried with
  a real failed segment.
- **dart:io can't close a socket stuck in a TLS handshake** (`clients/lichtspiel/lib/api.dart`, a
  `shortcut:` comment). One leaked socket per failed try; nothing simple in dart:io today, revisit
  when it gains a handshake timeout.

## Feature ideas

Collected on 2026-11-21. Already there, so not on this list: age limits per person, TOTP, home
profiles, TMDB collections, skip intro / credits, watch together, downloads, Trakt, the eleven
bundled plugins.

### First

- [x] **Sleep timer in the player** — "stop after this episode", or after 30 / 60 / 90 minutes.
  Small, in the web app and the app; useful with shows that play the next episode by themselves.
- [x] **Subtitle sync** — subtitles from OpenSubtitles are often a few seconds off. The server
  already listened to the sound when an admin asked in the player; now every subtitle file is
  synchronized once by itself (maintenance window, and right after the plugin downloads one).
- [x] **Clean-up suggestions (plugin, admins)** — like Maintainerr: titles nobody watched for X
  months, movies everyone has seen, the biggest files per hour of running time. Deleted only on a
  click, and only with *Allow media deletion* on. Helps when the NAS fills up. Built as the `cleanup` plugin.

### Later

- [x] **Night mode / louder dialogue** — even out the sound so speech is clear and explosions don't
  boom. Built in the players instead of the converter (WebAudio in the browser, an mpv filter in
  the app), so it works with direct play too; *Lift dialogue* for 5.1 → stereo was already there.
- [ ] **"Coming soon" row and calendar** — new episodes of shows in the library (Sonarr or TMDB)
  and movies waiting in Radarr. Goes with the wish list.
- [ ] **Smart collections like Kometa (plugin)** — "IMDb Top 250", "Oscar winners", Trakt lists,
  filled from the library automatically (`reads_library`).
- [ ] **Letterboxd import** — Letterboxd has no open API, but its CSV export can bring in watched
  state and ratings once.
- [ ] **Warning when a disk fills up** — a message through `push-notify` when a library's disk is
  nearly full.
- [ ] **Chromecast from the web app** — the biggest of these; closes the "smart TV without its own
  app" gap from the README comparison.

### Parked

- **Cinema trailers** before a movie — undecided, see the options discussed on 2026-11-05.

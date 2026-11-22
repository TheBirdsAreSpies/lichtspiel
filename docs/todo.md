# To do

What is still open: checks on real hardware, known problems, and ideas. When something is done,
take it off this list (the commit and [features.md](features.md) say what was built). Build, test
and conventions are in [development.md](development.md).

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
- [ ] **Chromecast:** a movie played as it is and a converted one with a seek, subtitles on the TV
  (moved for the converted stream), pause / volume from the browser, and the next episode at the end.
- [ ] **App player:** with a quality other than "Original", a failed HLS segment does not bring up
  the error screen (see the first known problem).

## Known problems

- **Minor player errors bring up the error screen** (app). media_kit passes many mpv log lines on as
  errors, and some are recoverable: one failed HLS segment (a `tcp:` line) could show
  "Wiedergabe fehlgeschlagen" although mpv would carry on. Since 2026-11-21 an error during
  playback is ignored when the position moves on by at least 1.5 s within 3 s
  (`clients/lichtspiel/lib/screens/player.dart`, the error listener). Not yet tried with a real
  failed segment.
- **dart:io can't close a socket stuck in a TLS handshake** (`clients/lichtspiel/lib/api.dart`, a
  `shortcut:` comment). One leaked socket per failed try; nothing simple in dart:io today, revisit
  when it gains a handshake timeout.

## Ideas

Collected on 2026-11-22, the first three are the most useful.

### Family and recommendations

- **Age limit per user (kids profile):** an admin sets a highest age rating per user; titles above
  it are hidden everywhere (libraries, search, home rows, collections, API, MCP). The age rating
  is already in the metadata.
- **"Because you watched X":** home rows built only from the own library, by likeness of genre,
  director and cast, and by what the others in the household rated well.
- **Forgotten gems:** a home row of highly rated movies nobody has watched for over a year.
- **Sign in on a TV without a password:** the TV shows a code, confirmed on the phone.

### Server

- **Convert ahead:** for a title that only plays converted (Chromecast, on the road), the server
  makes a fitting version in the maintenance window, so the NAS has nothing to do while playing.
- **Quality check:** the Library doctor reports movies in low resolution or without a German audio
  track; with Radarr it could ask for a better version.
- **Statistics page:** who watches how much, the most watched shows, times of day, load on the
  NAS — like Wrapped, but all year.

### Watching

- **Skip "previously on":** recaps at the start of an episode found by sound like the intro, with
  a skip button.
- **"What happened so far":** after weeks away from a show, a button that plays the last minutes
  of the previous episode.
- **"Something short":** say how much time there is (e.g. 40 minutes) and get fitting episodes or
  short movies from what you are watching or have on the watchlist.
- **Random night:** a dice button on a library page that keeps the active filters.

### From earlier

- **Smart collections:** Trakt lists, Kometa's poster overlays, and home rows for collections.
- **Letterboxd import in the app:** the app shows the panel, but the export file can only be
  uploaded in the web page.

### Parked

- **Cinema trailers** before a movie — undecided, see the options discussed on 2026-11-05.

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
- **Flutter after 3.47.9 deprecates `package:flutter/material.dart`** (app) for
  `package:material_ui/material_ui.dart`: `flutter analyze` then reports every file that imports it
  (37 on 2026-11-22) and fails. GitHub CI is held at 3.47.9 (`flutter-version` in
  `.github/workflows/ci.yml`). Before updating Flutter: switch the imports to `material_ui` (add it to
  `pubspec.yaml`), then raise the version in `ci.yml`.

## Ideas

Collected on 2026-11-22.

### Vue web app (`web-vue/`, branch `feat/vue-frontend`)

- **Still missing there** (only in `web/` so far): the server's settings.

### Server

- **Convert ahead:** for a title that only plays converted (Chromecast, on the road), the server
  makes a fitting version in the maintenance window, so the NAS has nothing to do while playing
  (by hand this is **Optimize** in a title's "⋯" menu).
- **Quality check:** the Library doctor reports movies in low resolution or without a German audio
  track; with Radarr it could ask for a better version.

### From earlier

- **Smart collections:** Trakt lists, Kometa's poster overlays, and home rows for collections.
- **Letterboxd import in the app:** the app shows the panel, but the export file can only be
  uploaded in the web page.

### Parked

- **Cinema trailers** before a movie — undecided, see the options discussed on 2026-11-05.
- **Photos, live TV and DVR** — not needed for now; the plan is in
  [plans/photos-and-live-tv.md](plans/photos-and-live-tv.md).

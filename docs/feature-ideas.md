# Feature ideas

Ideas for later, collected on 2026-11-21. None of them is built yet; tick one off with its commit
when it is. Plugins among them go into [plugin-ideas.md](plugin-ideas.md) once they are planned.

Already there, so not on this list: age limits per person, TOTP, home profiles, TMDB collections,
skip intro / credits, watch together, downloads, Trakt, the eleven bundled plugins.

## First

- [ ] **Sleep timer in the player** — "stop after this episode", or after 30 / 60 / 90 minutes.
  Small, in the web app and the app; useful with shows that play the next episode by themselves.
- [ ] **Subtitle sync (plugin)** — subtitles from OpenSubtitles are often a few seconds off. A
  plugin shifts them to fit the sound (`ffsubsync` or `alass`). Goes with the `subtitles` plugin.
- [ ] **Clean-up suggestions (plugin, admins)** — like Maintainerr: titles nobody watched for X
  months, movies everyone has seen, the biggest files per hour of running time. Deleted only on a
  click, and only with *Allow media deletion* on. Helps when the NAS fills up.

## Later

- [ ] **Night mode / louder dialogue** — when converting, even out the sound (ffmpeg `loudnorm` /
  `dynaudnorm`) so speech is clear and explosions don't boom; most useful for 5.1 down to stereo.
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

## Parked

- **Cinema trailers** before a movie — undecided, see the options discussed on 2026-11-05.

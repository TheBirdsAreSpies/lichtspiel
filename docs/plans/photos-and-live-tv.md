# Plan: photos and live TV

**Status: parked** (written 2026-11-22, not needed for now). These are the two things the README's
comparison says Lichtspiel lacks. Photos come first: they reuse what music libraries brought.
Live TV starts with one open question, which TV source is used (see below).

When work starts, move the steps to [todo.md](../todo.md). When they are done, describe them in
[features.md](../features.md) and [libraries.md](../libraries.md) (and [watching.md](../watching.md) for live TV),
take "Photos, live TV / DVR" out of the README's comparison, and delete this file.

---

## Part 1: Photo libraries

Built like music libraries (commit `d12d4d2` and the ones after it): a new kind of library, its own
scanner path, and its own pages.

### 1.1 Data

- `SectionKind::Photo` in `src/data/db/library.rs` (`"photo" | "photos" | "13"` in `parse`, since
  13 is Plex's number, which `src/data/migrate/import.rs` already writes as `"photo"`).
  `top_kind` → `ItemKind::Folder`.
- New `ItemKind`s: `Folder` (one per folder, nested like the folders on disk) and `Photo`. Videos
  in a photo library become `Video` items, so phone clips play the normal way.
- Per photo: date taken, width and height, orientation, camera make and model, lens, exposure
  (aperture, shutter, ISO), GPS position (if present). Stored with the item. Mind the
  lock order and the `Tracked` maps (see development.md).
- Libraries settings: "Photos" as a kind (`src/api/routes/libraries.rs`, the "type must be …"
  message), web and app library forms.

### 1.2 Scanner

- `src/library/photo.rs`, next to `music.rs`: `is_photo(path)` (jpg, jpeg, png, webp, gif, heic,
  heif, avif), `info(root, path, exif)` → `PhotoInfo`.
- Scanner (`src/library/scanner/`): in a photo section, folders become `Folder` items, pictures
  become `Photo` items, videos become `Video` items. Skip `@eaDir` (Synology's thumbnail folders),
  `.thumbnails` and hidden folders.
- **EXIF:** a small reader in our own code (JPEG APP1 / TIFF IFDs, and the `Exif` box in HEIC).
  Only the tags listed above, no new crate. If there is no date taken: the date in the file name
  (`IMG_20240712_…`, `PXL_…`, `2024-07-12 …`), else the file's modified time.
- Deep analysis, preview pictures, intro and credit detection, subtitles: skip photos (as for
  songs).
- Tests in `src/library/scanner/tests.rs`: a photo library with folders, a file without EXIF, an
  `@eaDir` folder that is skipped.

### 1.3 Pictures in smaller sizes

- Never send the original to a grid. Two cached sizes per photo in the data folder
  (`metadata/photos/<id>/`): `thumb` (≈ 400 px) and `screen` (≈ 2048 px on the long side),
  JPEG. Made by ffmpeg like `cover_args` / `art_args` in `src/playback/transcode.rs`
  (`photo_args(source, max_px, out)`), with the EXIF orientation applied (ffmpeg does not do that
  for JPEG; add `transpose`/`hflip`/`vflip` from the orientation we read).
- Made in the background after a scan, and on demand on first request (then cached). Removed with
  the item (housekeeping).
- **Check first:** whether the NAS's `jellyfin-ffmpeg7` decodes HEIC/HEIF and AVIF. If not, HEIC is
  listed but shown with a placeholder until there is a way. RAW files (cr2, nef, arw, dng) are left
  out.
- The original stays downloadable ("Download" in the viewer).

### 1.4 API

New routes (with lines in `ROUTES`, `src/api/routes/docs.rs`):

| Route | What |
|---|---|
| `GET /library/sections/{id}/photos?folder=&from=&to=&sort=` | photos and subfolders of a folder, or the whole library by date |
| `GET /library/sections/{id}/timeline` | how many photos per month, for the timeline's scroll bar |
| `GET /library/metadata/{id}/photo?size=thumb\|screen\|original` | the picture |
| `GET /library/metadata/{id}` | already exists; for a photo it also has the EXIF fields |

Edit (title, date taken) goes through the existing edit routes.

### 1.5 Web (`web/`)

- Library page for a photo library: **Folders** (grid of folders with a cover, then photos) and
  **Timeline** (all photos grouped by month, newest first, a month scroller on the side). Lazy
  loading of `thumb` pictures.
- **Viewer:** full screen, arrows / swipe / keys, zoom (`screen` picture, `original` on a double
  click), info panel (date, camera, exposure, size, a map link for the position), download.
  Videos open in the normal player.
- **Slideshow:** from a folder or the timeline, 5 s per photo, optional shuffle.
- **Chromecast:** the default media receiver shows images (`image/jpeg` with the `screen` URL).
- Home page: an "On this day" row (photos from this date in earlier years). Optional.
- Search finds folders by name.
- German texts in `web/lang/de.js`, `npm run check`.

### 1.6 App (`clients/lichtspiel/`)

- `screens/photos.dart`: folder grid and timeline, like the web page.
- `screens/photo_viewer.dart`: swipe, pinch zoom (`InteractiveViewer`, no new package needed),
  info sheet, slideshow.
- Library list (`libraries.dart`, `library.dart`) opens a photo library there.
- `flutter analyze`, `flutter test`.

### 1.7 Later

Map of all photos, albums across folders, sharing a folder with a link, face recognition (big,
probably never).

### 1.8 Order

1. Data + scanner + EXIF + tests. 2. Smaller sizes + API. 3. Web folders, viewer. 4. Timeline,
slideshow, Chromecast. 5. App. 6. Docs and README.

---

## Part 2: Live TV and DVR

Larger, and two steps: watching first, recording second.

### 2.0 Open question: the TV source

Lichtspiel cannot receive TV by itself. Decide on one source to build first; the rest come later
behind the same interface (`trait Tuner`: channel list, open a stream for a channel, how many at
once).

| Source | How it is read | Notes |
|---|---|---|
| **IPTV M3U** list (e.g. ARD/ZDF's free live streams) | HTTP/HLS URLs from the list | no hardware, easiest to start with and test |
| **FRITZ!Box Cable** | its M3U (`dvb/m3u/tvhd.m3u` …), RTSP streams | common in Germany; RTSP via ffmpeg; few parallel streams |
| **HDHomeRun** (cable / antenna) | `discover.json`, `lineup.json`, `http://…:5004/auto/v<ch>` | found by UDP broadcast, may need `network_mode: host` |
| **Sat>IP** / **TVHeadend** | RTSP, or TVHeadend's HTTP API and M3U | satellite; TVHeadend also brings a guide and does the tuning |

Build **M3U** first (it covers IPTV, FRITZ!Box and TVHeadend's M3U), then HDHomeRun.

### 2.1 Step 1: watching live

**Data**
- `Tuner` (kind, address, how many streams at once), `Channel` (number, name, logo, stream URL,
  tuner, hidden yes/no, favourite), `Programme` (channel, start, end, title, subtitle,
  description, category, season/episode, picture).
- Kept in the database, the guide only for the next 14 days (older entries removed).

**TV guide (EPG)**
- XMLTV from a URL or a file (TVHeadend, or a guide service), read every few hours in the
  background. Channels matched by `tvg-id` from the M3U, else by name.
- Without XMLTV, "now / next" only, if the source provides it.

**Streaming** (`src/playback/live.rs`)
- One ffmpeg per channel being watched, shared by every viewer on it. A tuner slot is taken when
  the first viewer starts and freed 30 s after the last one leaves. "All tuners busy" as an error
  the clients show.
- Input is MPEG-TS, often interlaced (MPEG-2 or H.264, 1080i/576i) with MP2/AC3/E-AC3 sound:
  deinterlace (`yadif`, or the hardware deinterlacer for vaapi/qsv), sound to AAC stereo, a
  picture that is already H.264 progressive is copied.
- `live_hls_args`: like `hls_args`, but a rolling window (`-hls_list_size` ≈ 10,
  `-hls_flags delete_segments`) plus a buffer of the last 30–60 min kept on disk for pause and
  rewind (timeshift). Reuses `video_args` and the hardware settings.
- The NAS's CPU: check how many deinterlaced streams it manages (one per channel watched).
- Dashboard: live sessions shown with the channel and tuner.

**API**

| Route | What |
|---|---|
| `GET/POST/DELETE /api/livetv/tuners` | admin: sources (M3U URL, HDHomeRun address, XMLTV URL) |
| `GET /api/livetv/channels` | channels with what is on now and next |
| `GET /api/livetv/guide?from=&to=&channels=` | the guide grid |
| `POST /api/livetv/channels/{id}/watch` | start / join a stream → HLS URL |
| `PUT /api/livetv/channels/{id}` | admin: hide, renumber, favourite |

**Web**
- A "Live TV" entry in the navigation (only when a source is set up).
- Guide grid: channels down, time across, "now" line, click → details → "Watch".
- Live player: the normal player with a "live" mode: no progress bar beyond the buffer, "Back to
  live", channel up/down, a small guide overlay.
- Settings → Live TV: sources, guide URL, channel list (hide, order, favourites).
- Chromecast: live HLS (`streamType: LIVE`).

**App**
- `screens/live_tv.dart` (guide grid), the player in live mode, settings screen for admins.

**Docker**
- HDHomeRun discovery needs `network_mode: host` or the tuner's address typed in. M3U/RTSP only
  need the NAS to reach the box. Note both in [install.md](../install.md).

### 2.2 Step 2: recording (DVR)

- **Recording rules:** one programme, or a series (by title, new episodes only or all, one
  channel or any), padding before and after (default 2 / 5 min), keep the last N episodes.
- **Scheduler** in `src/server/background.rs`: plans the next 14 days from the guide and the
  rules, finds conflicts with the number of tuners (warns, records by priority).
- **Recording:** ffmpeg copies the stream (no conversion) to MPEG-TS, then remuxes to MKV once
  done. Saved to a folder chosen in the settings, named like the scanner expects
  (`Show/Season 01/Show - S01E02 - Title.mkv`, `Movie (Year)/Movie (Year).mkv`), then a scan of
  that folder, so recordings show up as normal titles with metadata. Shared tuner: a recording
  of the channel being watched uses the same stream.
- **Optional:** skipping ads (comskip as chapters), converting to H.264 in the maintenance window
  (like Optimize).
- **API, web, app:** "Record" and "Record series" in the guide, a recordings page (planned, running,
  conflicts, rules), push / webhook plugins told when a recording starts and ends.

### 2.3 Order

1. Decide on the source. 2. M3U + XMLTV + data + channel list. 3. Live streaming with shared
tuners, web guide and player. 4. Timeshift. 5. App. 6. HDHomeRun. 7. DVR. 8. Docs and README.

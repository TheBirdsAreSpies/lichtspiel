# Install and run

On a Synology with Container Manager, on any other Docker host, with automatic deploys from Gitea, or on Windows for trying it out. To reach the server from outside your home, see [remote-access.md](remote-access.md). Back to the [README](../README.md).

## Install on the Synology (Container Manager)

You need DSM 7.2 or newer with **Container Manager** installed (Package Center).

1. In **File Station**, create the folder `docker/lichtspiel` (it holds the server's
   database, settings and plugins). Your videos can stay where they are, e.g. in the shared
   folder `video` with sub-folders `Movies` and `TV Shows`.
2. Copy this whole `app` folder to the NAS, e.g. to `docker/lichtspiel-build`.
3. Open `docker-compose.yml` and check the two paths after `:-` in the first two `volumes:` lines:
   - `/volume1/docker/lichtspiel` → where the server keeps its data
   - `/volume1/video` → your videos (mounted read-only as `/media`)
   If you have no Intel graphics (or your model has none), delete the `devices:` lines.
   For hardware transcoding the server also needs the group of the graphics chip: put
   `LICHTSPIEL_GPU_GROUP=<number>` in a `.env` file next to `docker-compose.yml`, with the number
   from `ls -n /dev/dri` (the group of `renderD128`).
4. Container Manager → **Project** → **Create** → name `lichtspiel`, path
   `docker/lichtspiel-build`, "Use existing docker-compose.yml" → Next → Done.
   The first build takes 10–20 minutes on a NAS (it compiles the server).
5. Open `http://<your-nas-ip>:8484` in a browser. The first visit asks you to create the
   owner account.
6. **Libraries → Add library**: name "Movies", type Movies, folder `/media/Movies`; then
   "TV Shows", type TV shows, folder `/media/TV Shows`. The scan starts immediately and the
   details (plot, cast, posters) are downloaded in the background.
7. **Settings → Server → "Scan my library periodically"**: tick it so new files show up by
   themselves (it is off by default; the interval is hourly).
8. **Settings → Transcoder → Hardware acceleration** is **Automatic** by default: if the
   graphics chip works (most Synology "+" models with an Intel CPU) it is used, otherwise
   the processor. The panel says which one is in use. If video converted by the graphics
   chip looks blocky, tick **"Graphics chip: convert by quality instead of bitrate"**.
   HDR video (HDR10, HLG) that has to be converted is tone mapped to normal colours, so it does
   not look grey; switch it off with **"Convert HDR video to normal colours"** (it needs an
   ffmpeg with the `zscale` and `tonemap` filters). Dolby Vision profile 5 is not handled.

The Docker image was built and tested on an Intel/AMD (x86-64) computer: the full self-test
passes inside it, including converting video and downloading movie details. It was not
tried on a real Synology, so the graphics-chip conversion there is untested.

### Ready-made image instead of building

Each release is also published as an image for Intel/AMD and ARM:
`ghcr.io/thebirdsarespies/lichtspiel` (tags `latest`, a version like `0.2.0`, or `0.2` for its
fixes). It saves the 10–20 minutes of building. Put `LICHTSPIEL_IMAGE=ghcr.io/thebirdsarespies/lichtspiel:latest`
in the `.env` file next to `docker-compose.yml` (steps 2–3 stay the same), then over SSH in that folder:

```
docker compose pull
docker compose up -d --no-build
```

The same two lines update it to the newest release. Without `--no-build` Docker builds from the
source instead.

## Any other Docker host

Lichtspiel is an ordinary Docker image, so it runs on any machine with Docker and Docker Compose:
a Linux server, Unraid, TrueNAS, a QNAP, a Raspberry Pi 4/5 (64-bit) and so on. Only the defaults in
`docker-compose.yml` are made for a Synology (`/volume1/…`, user `1026:100`, group `101`); change
them in a `.env` file next to it instead of editing the file:

```
LICHTSPIEL_IMAGE=ghcr.io/thebirdsarespies/lichtspiel:latest   # leave out to build from the source
LICHTSPIEL_CONFIG=/srv/lichtspiel        # database, settings, plugins; the user below must own it
LICHTSPIEL_MEDIA=/srv/videos             # your videos, mounted read-only as /media
LICHTSPIEL_USER=1000:1000                # uid:gid the server runs as (`id` shows yours)
LICHTSPIEL_VIDEO_GROUP=1000              # a group that may read the videos (`ls -n` on the folder)
LICHTSPIEL_GPU_GROUP=105                 # the group of /dev/dri/renderD128 (`ls -n /dev/dri`)
LICHTSPIEL_PORT=8484
```

Then, in that folder:

```
mkdir -p /srv/lichtspiel && sudo chown 1000:1000 /srv/lichtspiel
docker compose pull && docker compose up -d --no-build   # or `docker compose up -d --build`
```

and open `http://<host>:8484`. Without an Intel/AMD graphics chip (no `/dev/dri`, e.g. a Raspberry
Pi) delete the `devices:` lines, otherwise the container does not start; videos are then converted
on the processor. Steps 5–8 of the Synology install (owner account, libraries, scanning, hardware
acceleration) are the same everywhere.

### Automatic build and deploy (Gitea)

`.gitea/workflows/deploy.yml` builds and starts a new version on the Synology after every push to
`main`: tests (`docker build --target test .`), the image, the container is replaced, then a health
check. If the new version does not become healthy, the previous one is started again and the run
fails (you get Gitea's usual mail). Run it by hand under *Actions → Build and deploy → Run workflow*.

It needs a Gitea runner (`act_runner`) with the label `nas` on the Synology that can use its
Docker (the same runner that deploys other projects there). The workflow uses these settings (Gitea: repository →
Settings → Actions → Variables); all are optional:

| Variable | Default | |
|---|---|---|
| `LICHTSPIEL_CONFIG` | `/volume1/docker/lichtspiel` | folder for the database and settings (`/config`) |
| `LICHTSPIEL_MEDIA` | `/volume1/video` | folder with the videos, mounted read-only as `/media` |
| `LICHTSPIEL_PORT` | `8484` | port on the Synology |
| `LICHTSPIEL_USER` | `1026:100` | the user the server runs as (uid:gid); it must own the config folder |
| `LICHTSPIEL_OLD_SERVER` | none | the old Plex server's data folder for the import, read-only as `/oldserver` |

The container itself is described only in `docker-compose.yml`: the workflow starts it from there with
`docker compose up` and fills in these variables. Everything else (the graphics chip under `devices:`,
the groups under `group_add:`, host networking) is changed in that file, and then applies to a manual
install and to the deploy alike. The group of the graphics chip is looked up by the workflow itself.
`LICHTSPIEL_DOCKER_ARGS` and `LICHTSPIEL_MEDIA_GROUPS` from earlier versions are no longer used; the
run warns while they are still set.

The container is called `lichtspiel` (Compose project `lichtspiel`); the workflow removes one with
that name and creates it again, with the data folder untouched. If you started Lichtspiel before as a
Container Manager project, delete that project (not the folder) so the two do not fight over the
name and the port. The runner needs `docker compose` or `docker-compose`; Container Manager brings it.
The first run takes 10–20 minutes (it compiles everything); later runs reuse the compiled
dependencies and take a few minutes.

## Run it on Windows (for trying it out)

1. Install **ffmpeg** (e.g. `winget install ffmpeg`) so `ffmpeg` and `ffprobe` are on the PATH,
   or set their full paths in Settings.
2. `cargo build --release` in this folder, then run `target\release\lichtspiel-server.exe`.
3. Open `http://localhost:8484`.

Data goes to a `data` folder next to where you start it. Options:

| Option | Environment variable | Meaning |
|---|---|---|
| `--data <folder>` | `LICHTSPIEL_DATA` | where the database, plugins and cache live |
| `--port <n>` | `LICHTSPIEL_PORT` | port (default 8484, or the one saved in Settings) |
| `--bind <address>` | `LICHTSPIEL_BIND` | network address to listen on (default all) |
| `--console` | | also open the optional desktop status window |
| `--no-browser` | | don't open the browser on the first start |
| `--tls-cert <file>`, `--tls-key <file>` | `LICHTSPIEL_TLS_CERT`, `LICHTSPIEL_TLS_KEY` | serve HTTPS with this certificate and key |
| `--relay` | | run as the relay instead (see "Reaching it from outside") |
| `--self-test <file.json>` | | builds test videos, runs every feature, writes a report |
| `--screenshot <file.png>` | | draws the status window and saves a picture |

## A transcode helper on another computer

A NAS without a strong graphics chip can leave converting to another computer at home, for example a
Windows PC with an NVIDIA card. That computer runs the same Lichtspiel program as a **transcode
helper**: the server sends it each conversion for playback, the helper reads the video from the
server over the network (no shared folders or drive letters needed), converts it with its own ffmpeg
and graphics chip and sends the result back. Only conversions go there; a video that is only
repackaged (direct stream) stays on the server, as does everything else (scans, preview pictures,
intro and credits detection).

1. On the helper computer, install a current **ffmpeg** (`winget install Gyan.FFmpeg`). An old one
   (such as the 4.2 that comes with ImageMagick) may not be able to use a new graphics card.
2. Copy `lichtspiel-server.exe` there and start it, with a secret of your own:

   ```
   lichtspiel-server.exe --transcode-helper --helper-secret "a long secret"
   ```

   It listens on port 8486 (`--helper-listen 0.0.0.0:8486`), finds the graphics chip by itself
   (`--hw auto|none|vaapi|qsv|nvenc`, `--hw-device 1` for the second card) and prints the address to
   enter. Allow the port in the Windows firewall for the home network. `--ffmpeg C:\path\to\ffmpeg.exe`
   if ffmpeg is not on the PATH. Check with `ffmpeg -version` in a new window which one Windows
   finds: the system PATH comes before the user's, so an old ffmpeg there (ImageMagick, another
   program) wins over the new one. Move the new ffmpeg's folder to the top of the system PATH, or
   give it with `--ffmpeg`. The same works on Linux and macOS (`LICHTSPIEL_HELPER_SECRET` …
   as environment variables).
3. On the server: **Settings → Transcoder → Transcode helper**, enter its address
   (`http://192.168.1.50:8486`) and the secret. The page then says whether the helper answers and
   what it converts with; the dashboard marks streams converted there.

When the helper is off or can't be reached, the server converts itself again (it asks again after
10 seconds). The helper reads the video at the address its requests come from and the server's
port; if that does not work (Docker with another port mapping, a reverse proxy), enter the address
under **Address of this server for the helper**. Keep the helper in the home network only: whoever
knows the secret can make it run ffmpeg. Use the same Lichtspiel version on both; the settings page
warns when they differ.

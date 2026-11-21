# Install and run

On a Synology with Container Manager, with automatic deploys from Gitea, or on Windows for trying it out. To reach the server from outside your home, see [remote-access.md](remote-access.md). Back to the [README](../README.md).

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


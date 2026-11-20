# Lichtspiel for Synology (Container Manager) or any Docker host.
# Builds for the machine it runs on (Intel/AMD x86_64 or ARM64 Synology models).

FROM rust:1-slim-bookworm AS build
# What the tests need (ffmpeg, python3). Installed first, before any file is copied, so this layer
# stays cached: after a code change only the program itself is compiled again, not downloaded again.
RUN apt-get update && apt-get install -y --no-install-recommends ffmpeg python3 ca-certificates && rm -rf /var/lib/apt/lists/*
WORKDIR /src
COPY Cargo.toml Cargo.lock build.rs ./
# Our copy of tiny_http (the web server library, with limits on request headers): Cargo.toml
# points at it, so it has to be here before the first cargo build.
COPY vendor ./vendor
# The dependencies first, with an empty program: this layer stays cached while only the source
# changes, so a build after a small change takes minutes instead of a quarter of an hour.
# (No BuildKit needed: the Synology's Docker often has no buildx.)
RUN mkdir src && echo 'fn main() {}' > src/main.rs \
 && cargo build --release --locked --no-default-features \
 && rm -rf src
COPY src ./src
COPY web ./web
# The commit the image is built from (shown in the web app; the Gitea workflow passes it).
ARG GIT_SHA=
# The status window is left out: a NAS has no screen. (touch: the copied files are older than the
# empty program built above, and cargo would take it for up to date.)
RUN touch src/main.rs && cargo build --release --locked --no-default-features

# `docker build --target test .` runs the tests (the automatic build on Gitea does this first).
FROM build AS test
RUN cargo test --release --locked --no-default-features

FROM debian:bookworm-slim
# ffmpeg does the transcoding. Jellyfin's build instead of Debian's: it is made for media servers and
# brings its own VA drivers (Intel i965 and iHD for the Intel-based Synology models DS218+, DS220+,
# DS920+, DS923+ …, and AMD), so the GPU converts video (hw_accel = vaapi) without the desktop libraries
# Debian's ffmpeg pulls in. Measured: 279 MB instead of 468 MB. python3 fetches the repository key
# (apt reads it as it is), so no curl or gnupg is left in the image.
RUN apt-get update \
 && apt-get install -y --no-install-recommends python3 ca-certificates \
 && python3 -c 'import urllib.request; urllib.request.urlretrieve("https://repo.jellyfin.org/jellyfin_team.gpg.key", "/usr/share/keyrings/jellyfin.asc")' \
 && echo "deb [signed-by=/usr/share/keyrings/jellyfin.asc] https://repo.jellyfin.org/debian bookworm main" > /etc/apt/sources.list.d/jellyfin.list \
 && apt-get update \
 && apt-get install -y --no-install-recommends jellyfin-ffmpeg7 \
 && ln -s /usr/lib/jellyfin-ffmpeg/ffmpeg /usr/local/bin/ffmpeg \
 && ln -s /usr/lib/jellyfin-ffmpeg/ffprobe /usr/local/bin/ffprobe \
 && rm -rf /var/lib/apt/lists/*
COPY --from=build /src/target/release/lichtspiel-server /usr/local/bin/lichtspiel-server
COPY plugins /opt/lichtspiel/plugins
ENV LICHTSPIEL_DATA=/config \
    LICHTSPIEL_PORT=8484 \
    LICHTSPIEL_BUNDLED_PLUGINS=/opt/lichtspiel/plugins
# Not root: the server, its plugins and ffmpeg run as uid 1026, gid 100 ("users"), the IDs of the first
# user and the users group on a Synology. A config folder from an older (root) version must be handed
# over once on the NAS: sudo chown -R 1026:100 /volume1/docker/lichtspiel (or choose other IDs with
# compose `user:` / the LICHTSPIEL_USER variable of the deploy). For hardware transcoding the user also
# needs the group of /dev/dri/renderD128 (compose `group_add`, docker run `--group-add`).
RUN useradd --uid 1026 --gid 100 --home-dir /config --no-create-home --shell /usr/sbin/nologin lichtspiel \
 && mkdir -p /config && chown 1026:100 /config
VOLUME ["/config"]
EXPOSE 8484
# The health check: the server may speak HTTPS (Settings) and may listen only on the network card
# chosen in Settings, so try http and https on 127.0.0.1 and on every address of the container.
RUN printf '%s\n' \
  'import os, ssl, subprocess, sys, urllib.request' \
  'port = os.environ.get("LICHTSPIEL_PORT", "8484")' \
  'try:' \
  '    hosts = subprocess.run(["hostname", "-I"], capture_output=True, text=True, timeout=3).stdout.split()' \
  'except Exception:' \
  '    hosts = []' \
  'ctx = ssl._create_unverified_context()' \
  'for h in ["127.0.0.1"] + hosts:' \
  '    for scheme in ("http", "https"):' \
  '        try:' \
  '            urllib.request.urlopen("%s://%s:%s/identity" % (scheme, "[%s]" % h if ":" in h else h, port), timeout=3, context=ctx)' \
  '            sys.exit(0)' \
  '        except Exception:' \
  '            pass' \
  'sys.exit(1)' > /usr/local/bin/lichtspiel-health
# Checked every 15 s (Docker runs the first check one interval after the start).
HEALTHCHECK --interval=15s --timeout=20s CMD python3 /usr/local/bin/lichtspiel-health
USER 1026:100
CMD ["lichtspiel-server"]

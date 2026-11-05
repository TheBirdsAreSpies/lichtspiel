# Lichtspiel for Synology (Container Manager) or any Docker host.
# Builds for the machine it runs on (Intel/AMD x86_64 or ARM64 Synology models).

FROM rust:1-slim-bookworm AS build
WORKDIR /src
COPY Cargo.toml Cargo.lock ./
COPY src ./src
COPY web ./web
# The status window is left out: a NAS has no screen.
# The cache mounts keep the downloaded crates and the compiled dependencies between builds, so a
# build after a small change takes minutes instead of a quarter of an hour (needs BuildKit, the default).
RUN --mount=type=cache,target=/usr/local/cargo/registry     --mount=type=cache,target=/src/target     cargo build --release --no-default-features  && cp target/release/lichtspiel-server /lichtspiel-server

# `docker build --target test .` runs the tests (the automatic build on Gitea does this first).
FROM build AS test
RUN apt-get update && apt-get install -y --no-install-recommends ffmpeg python3 ca-certificates && rm -rf /var/lib/apt/lists/*
RUN --mount=type=cache,target=/usr/local/cargo/registry     --mount=type=cache,target=/src/target     cargo test --release --no-default-features

FROM debian:bookworm-slim
# ffmpeg does the transcoding; the VA drivers let Intel-based Synology models
# (DS218+, DS220+, DS920+, DS923+ …) convert video with the GPU (hw_accel = vaapi).
RUN apt-get update \
 && apt-get install -y --no-install-recommends ffmpeg python3 ca-certificates mesa-va-drivers \
 && if [ "$(dpkg --print-architecture)" = "amd64" ]; then apt-get install -y --no-install-recommends i965-va-driver intel-media-va-driver; fi \
 && rm -rf /var/lib/apt/lists/*
COPY --from=build /lichtspiel-server /usr/local/bin/lichtspiel-server
COPY plugins /opt/lichtspiel/plugins
ENV LICHTSPIEL_DATA=/config \
    LICHTSPIEL_PORT=8484 \
    LICHTSPIEL_BUNDLED_PLUGINS=/opt/lichtspiel/plugins
VOLUME ["/config"]
EXPOSE 8484
HEALTHCHECK --interval=60s --timeout=5s CMD python3 -c "import urllib.request;urllib.request.urlopen('http://127.0.0.1:8484/identity')" || exit 1
CMD ["lichtspiel-server"]

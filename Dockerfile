# Lichtspiel for Synology (Container Manager) or any Docker host.
# Builds for the machine it runs on (Intel/AMD x86_64 or ARM64 Synology models).

FROM rust:1-slim-bookworm AS build
WORKDIR /src
COPY Cargo.toml Cargo.lock ./
COPY src ./src
COPY web ./web
# The status window is left out: a NAS has no screen.
RUN cargo build --release --no-default-features

FROM debian:bookworm-slim
# ffmpeg does the transcoding; the VA drivers let Intel-based Synology models
# (DS218+, DS220+, DS920+, DS923+ …) convert video with the GPU (hw_accel = vaapi).
RUN apt-get update \
 && apt-get install -y --no-install-recommends ffmpeg python3 ca-certificates mesa-va-drivers \
 && if [ "$(dpkg --print-architecture)" = "amd64" ]; then apt-get install -y --no-install-recommends i965-va-driver intel-media-va-driver; fi \
 && rm -rf /var/lib/apt/lists/*
COPY --from=build /src/target/release/lichtspiel-server /usr/local/bin/lichtspiel-server
COPY plugins /opt/lichtspiel/plugins
ENV LICHTSPIEL_DATA=/config \
    LICHTSPIEL_PORT=8484 \
    LICHTSPIEL_BUNDLED_PLUGINS=/opt/lichtspiel/plugins
VOLUME ["/config"]
EXPOSE 8484
HEALTHCHECK --interval=60s --timeout=5s CMD python3 -c "import urllib.request;urllib.request.urlopen('http://127.0.0.1:8484/identity')" || exit 1
CMD ["lichtspiel-server"]

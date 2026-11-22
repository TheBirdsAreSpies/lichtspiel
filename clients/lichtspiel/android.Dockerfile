# The tools for building the Android app on the NAS (.gitea/workflows/app.yml): Java, the Android SDK and
# Flutter. There is no ready image with the Flutter the app is checked with (3.47.9, as in ci.yml), so the
# newest one there is gives Java and the Android SDK, and its Flutter is swapped the way it was put there.
# Raise FLUTTER_VERSION together with ci.yml and release.yml.
FROM ghcr.io/cirruslabs/flutter:3.44.0
ARG FLUTTER_VERSION=3.47.9
ENV FLUTTER_VERSION=${FLUTTER_VERSION}
RUN rm -rf "$FLUTTER_HOME" \
    && git clone --depth 1 --branch "$FLUTTER_VERSION" https://github.com/flutter/flutter.git "$FLUTTER_HOME" \
    && flutter --version \
    && flutter precache --android

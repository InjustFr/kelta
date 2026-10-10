# Gate S1 (BUILD_PLAN §6): kelta-ci plus a headless X server, WebKitWebDriver and tauri-driver.
FROM kelta-ci

RUN apt-get update && apt-get install -y --no-install-recommends \
      xvfb xauth dbus webkit2gtk-driver \
    && rm -rf /var/lib/apt/lists/*

RUN cargo install tauri-driver --locked --version 2.0.6

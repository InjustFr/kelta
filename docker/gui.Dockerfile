# Gate G2 (BUILD_PLAN §6): the Ubuntu build image plus a headless Wayland compositor and a screenshot tool.
FROM kelta-ci

RUN apt-get update && apt-get install -y --no-install-recommends \
      sway grim dbus fonts-dejavu-core libgl1-mesa-dri \
    && rm -rf /var/lib/apt/lists/*

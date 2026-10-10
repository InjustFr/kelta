#!/usr/bin/env bash
# Security gate S1 (BUILD_PLAN §6, ARCHITECTURE §11.3) on Linux/WebKitGTK, from the dev machine:
# builds the real app in ubuntu:24.04 (Docker) and runs fixtures/sandbox/run.mjs under tauri-driver +
# WebKitWebDriver + Xvfb. The macOS (WKWebView) half is manual: see docs/BUILD_PLAN.md §6.
# The target dir lives in the kelta-sandbox-target volume so reruns are incremental.
set -euo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")/.."

docker build -f docker/ubuntu-build.Dockerfile -t kelta-ci .
docker build -f docker/sandbox-check.Dockerfile -t kelta-sandbox .
docker run --rm -v "$PWD":/src:ro -v kelta-cargo:/root/.cargo/registry -v kelta-pnpm:/root/.local/share/pnpm/store \
  -v kelta-sandbox-target:/work/target \
  kelta-sandbox bash -lc '
    set -euo pipefail
    rsync -a --delete --exclude target --exclude node_modules --exclude .git /src/ /work/ && cd /work
    pnpm install --frozen-lockfile
    pnpm --filter @kelta/ui run build
    # custom-protocol: serve ui/dist like a release build instead of the devUrl.
    cargo build -p kelta-desktop --features tauri/custom-protocol --locked
    dbus-run-session -- xvfb-run -a node fixtures/sandbox/run.mjs /work/target/debug/kelta
  '

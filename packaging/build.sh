#!/usr/bin/env bash
# Builds the installers with the release overlay (ARCHITECTURE §2 "Bundling").
#   Linux: bash packaging/build.sh            -> deb + AppImage
#   macOS: bash packaging/build.sh            -> host-arch dmg
#          UNIVERSAL=1 bash packaging/build.sh -> universal dmg (needs both rust targets)
# Signing and notarization read APPLE_* variables when set; without them the dmg is unsigned.
# TAURI_SIGNING_PRIVATE_KEY (+ _PASSWORD) also makes the signed updater bundles (`.app.tar.gz`, AppImage,
# each with its `.sig`); without it the build has no update artifacts. KELTA_VERSION overrides the app version.
set -euo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")/.."

host="$(rustc -vV | sed -n 's/^host: //p')"
bins=apps/desktop/src-tauri/binaries
mkdir -p "$bins"

# Wall time of each phase in the log; cargo also writes target/cargo-timings/*.html (--timings).
t0=$SECONDS
phase() { echo "[build.sh] $1: $((SECONDS - t0)) s"; t0=$SECONDS; }

# Sidecars (externalBin): kelta-ctl and the keltad session daemon.
sidecars=(kelta-ctl keltad)
# Selecting kelta-desktop (none of its targets: its build script needs the sidecars) unifies dependency features
# with the app build below, so `tauri build` reuses the shared dependencies instead of compiling them again.
build_sidecars() {
  cargo build --release --locked -p kelta-ctl -p kelta-term -p kelta-desktop --features tauri/custom-protocol \
    --bin kelta-ctl --bin keltad --timings "$@"
}

if [[ "$(uname)" == Darwin && "${UNIVERSAL:-0}" == 1 ]]; then
  for t in aarch64-apple-darwin x86_64-apple-darwin; do
    build_sidecars --target "$t"
    phase "sidecars $t"
    for b in "${sidecars[@]}"; do cp "target/$t/release/$b" "$bins/$b-$t"; done
  done
  for b in "${sidecars[@]}"; do
    lipo -create -output "$bins/$b-universal-apple-darwin" \
      "$bins/$b-aarch64-apple-darwin" "$bins/$b-x86_64-apple-darwin"
  done
  target=(--target universal-apple-darwin)
else
  build_sidecars
  phase sidecars
  for b in "${sidecars[@]}"; do cp "target/release/$b" "$bins/$b-$host"; done
  target=()
fi

pnpm install --frozen-lockfile
phase "pnpm install"
# `app` makes bundle/macos/Kelta.app(.tar.gz), the bundle the updater installs.
if [[ "$(uname)" == Darwin ]]; then bundles=app,dmg; else bundles=deb,appimage; fi
extra=()
[[ -z "${TAURI_SIGNING_PRIVATE_KEY:-}" ]] || extra+=(--config '{"bundle":{"createUpdaterArtifacts":true}}')
[[ -z "${KELTA_VERSION:-}" ]] || extra+=(--config "{\"version\":\"$KELTA_VERSION\"}")
cd apps/desktop/src-tauri
# CI=true makes tauri pass --skip-jenkins to bundle_dmg.sh: no Finder AppleScript, which otherwise
# opens the volume in Finder and leaves it mounted (on the Desktop) after every build.
# ${a[@]+"${a[@]}"}: macOS /bin/bash 3.2 calls an empty array unbound under `set -u`.
CI=true ../../../ui/node_modules/.bin/tauri build --config ../../../packaging/tauri.release.json \
  ${extra[@]+"${extra[@]}"} --bundles "$bundles" ${target[@]+"${target[@]}"} -- --timings
phase "tauri build (frontend, app, bundles)"

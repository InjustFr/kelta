#!/usr/bin/env bash
# Builds the installers with the release overlay (ARCHITECTURE §2 "Bundling").
#   Linux: bash packaging/build.sh            -> deb + AppImage
#   macOS: bash packaging/build.sh            -> host-arch dmg
#          UNIVERSAL=1 bash packaging/build.sh -> universal dmg (needs both rust targets)
# Signing and notarization read APPLE_* variables when set; without them the dmg is unsigned.
set -euo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")/.."

host="$(rustc -vV | sed -n 's/^host: //p')"
bins=apps/desktop/src-tauri/binaries
mkdir -p "$bins"

# Sidecars (externalBin): kelta-ctl and the keltad session daemon.
sidecars=(kelta-ctl keltad)
build_sidecars() { cargo build --release --locked -p kelta-ctl -p kelta-term --bin kelta-ctl --bin keltad "$@"; }

if [[ "$(uname)" == Darwin && "${UNIVERSAL:-0}" == 1 ]]; then
  for t in aarch64-apple-darwin x86_64-apple-darwin; do
    build_sidecars --target "$t"
    for b in "${sidecars[@]}"; do cp "target/$t/release/$b" "$bins/$b-$t"; done
  done
  for b in "${sidecars[@]}"; do
    lipo -create -output "$bins/$b-universal-apple-darwin" \
      "$bins/$b-aarch64-apple-darwin" "$bins/$b-x86_64-apple-darwin"
  done
  target=(--target universal-apple-darwin)
else
  build_sidecars
  for b in "${sidecars[@]}"; do cp "target/release/$b" "$bins/$b-$host"; done
  target=()
fi

pnpm install --frozen-lockfile
if [[ "$(uname)" == Darwin ]]; then bundles=dmg; else bundles=deb,appimage; fi
cd apps/desktop/src-tauri
../../../ui/node_modules/.bin/tauri build --config ../../../packaging/tauri.release.json \
  --bundles "$bundles" "${target[@]}"

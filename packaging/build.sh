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

if [[ "$(uname)" == Darwin && "${UNIVERSAL:-0}" == 1 ]]; then
  for t in aarch64-apple-darwin x86_64-apple-darwin; do
    cargo build --release --locked -p kelta-ctl --target "$t"
    cp "target/$t/release/kelta-ctl" "$bins/kelta-ctl-$t"
  done
  lipo -create -output "$bins/kelta-ctl-universal-apple-darwin" \
    "$bins/kelta-ctl-aarch64-apple-darwin" "$bins/kelta-ctl-x86_64-apple-darwin"
  target=(--target universal-apple-darwin)
else
  cargo build --release --locked -p kelta-ctl
  cp target/release/kelta-ctl "$bins/kelta-ctl-$host"
  target=()
fi

pnpm install --frozen-lockfile
if [[ "$(uname)" == Darwin ]]; then bundles=dmg; else bundles=deb,appimage; fi
cd apps/desktop/src-tauri
../../../ui/node_modules/.bin/tauri build --config ../../../packaging/tauri.release.json \
  --bundles "$bundles" "${target[@]}"

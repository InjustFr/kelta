#!/usr/bin/env bash
# Local nightly: macOS dmg + signed updater bundle (Apple silicon) on this Mac and, with --linux, the Linux
# deb + AppImage for arm64 in Docker (--amd64 adds amd64, under QEMU on an arm64 Mac: the slowest step), published
# to the rolling "nightly" prerelease by scripts/publish-nightly.sh (ARCHITECTURE §2 "Bundling"). CI publishes the
# same nightly every night (.github/workflows/nightly.yml): this is the manual path.
# Usage: bash scripts/nightly-local.sh [--linux] [--amd64]   (HEAD of a clean tree; moves the remote `nightly` tag)
# Needs the updater key (CONTRIBUTING "Nightly builds"): ${KELTA_UPDATER_KEY:-~/.tauri/kelta-updater.key}
# and its password in the login Keychain (service `kelta-updater`).
set -euo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")/.."

linux=0 amd64=0
for arg in "$@"; do
  case "$arg" in
    --linux) linux=1 ;;
    --amd64) linux=1 amd64=1 ;;
    *) echo "usage: nightly-local.sh [--linux] [--amd64]" >&2; exit 2 ;;
  esac
done

# An unsigned nightly would strand every installed copy: refuse before building anything.
key_file="${KELTA_UPDATER_KEY:-$HOME/.tauri/kelta-updater.key}"
[[ -f "$key_file" ]] || { echo "updater signing key missing: $key_file (see CONTRIBUTING)" >&2; exit 1; }
password="$(security find-generic-password -s kelta-updater -w)" ||
  { echo "updater key password missing from the Keychain (service kelta-updater)" >&2; exit 1; }
! grep -q 'REPLACE-WITH' packaging/tauri.release.json ||
  { echo "packaging/tauri.release.json still has the placeholder updater pubkey" >&2; exit 1; }
# Not exported: only the signing builds below get the key, never the gate (build scripts, tests, Docker).
key="$(cat "$key_file")"

[[ -z "$(git status --porcelain)" ]] || { echo "dirty tree: commit or stash first" >&2; exit 1; }
# The public nightly is main only: refuse unreviewed branches.
{ git fetch -q origin main && git merge-base --is-ancestor HEAD origin/main; } || { echo "HEAD is not on origin/main" >&2; exit 1; }
# The nightly is built from a commit that passes every gate step, not only the affected ones.
bash scripts/ci-local.sh --full
sha="$(git rev-parse HEAD)"
# The updater compares this version; the Cargo version (bin/<version>/ dirs) stays as it is.
KELTA_VERSION="$(python3 -c 'import json; print(json.load(open("apps/desktop/src-tauri/tauri.conf.json"))["version"])')"
KELTA_VERSION="$KELTA_VERSION-nightly.$(date -u +%Y%m%d%H%M)"
export KELTA_VERSION
dist="$PWD/target/nightly-dist"
rm -rf "$dist" && mkdir -p "$dist"

# The bundler keeps older versioned files: clear them so only this build is published.
rm -rf target/release/bundle
# Ad-hoc signed ("-"): the linker-only signature makes Gatekeeper call the downloaded app "damaged".
APPLE_SIGNING_IDENTITY=- TAURI_SIGNING_PRIVATE_KEY="$key" TAURI_SIGNING_PRIVATE_KEY_PASSWORD="$password" bash packaging/build.sh
cp target/release/bundle/dmg/*.dmg "$dist"
# Versioned name: the new bundle is uploaded beside the old one before latest.json points at it.
for ext in tar.gz tar.gz.sig; do
  cp "target/release/bundle/macos/Kelta.app.$ext" "$dist/Kelta_${KELTA_VERSION}_aarch64.app.$ext"
done

# Linux only on request: a broken Linux build (#207) must not block the macOS nightly.
arches=""
if ((linux)); then arches="arm64"; fi
if ((amd64)); then arches="$arches amd64"; fi
for arch in $arches; do
  docker build --platform "linux/$arch" -f docker/ubuntu-build.Dockerfile -t "kelta-nightly-$arch" .
  # Source comes from `git archive` so only HEAD is built; /work/target is a per-arch volume (warm rebuilds).
  git archive HEAD | TAURI_SIGNING_PRIVATE_KEY="$key" TAURI_SIGNING_PRIVATE_KEY_PASSWORD="$password" docker run --rm -i --platform "linux/$arch" \
    -v "kelta-nightly-target-$arch":/work/target -v kelta-cargo:/root/.cargo/registry \
    -v kelta-pnpm:/root/.local/share/pnpm/store -v "$dist":/out \
    -e APPIMAGE_EXTRACT_AND_RUN=1 -e TAURI_SIGNING_PRIVATE_KEY -e TAURI_SIGNING_PRIVATE_KEY_PASSWORD \
    -e KELTA_VERSION "kelta-nightly-$arch" \
    bash -lc 'tar -x -C /work && cd /work && rm -rf target/release/bundle && bash packaging/build.sh \
      && cp target/release/bundle/deb/*.deb target/release/bundle/appimage/*.AppImage* /out/'
done

bash scripts/publish-nightly.sh "$dist" "$KELTA_VERSION" "$sha"

#!/usr/bin/env bash
# Local nightly: macOS dmg + signed updater bundle (Apple silicon) on this Mac and, with --linux, the Linux
# deb + AppImage for arm64 and amd64 in Docker (amd64 under QEMU on an arm64 Mac: slow), published to the
# rolling "nightly" prerelease with gh, with the updater manifest `latest.json` (ARCHITECTURE §2 "Bundling").
# Usage: bash scripts/nightly-local.sh [--linux]   (HEAD of a clean tree; moves the remote `nightly` tag to HEAD)
# Needs the updater key (CONTRIBUTING "Nightly builds"): ${KELTA_UPDATER_KEY:-~/.tauri/kelta-updater.key}
# and its password in the login Keychain (service `kelta-updater`).
set -euo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")/.."

linux=0
for arg in "$@"; do
  case "$arg" in
    --linux) linux=1 ;;
    *) echo "usage: nightly-local.sh [--linux]" >&2; exit 2 ;;
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
TAURI_SIGNING_PRIVATE_KEY="$key" TAURI_SIGNING_PRIVATE_KEY_PASSWORD="$password" bash packaging/build.sh
cp target/release/bundle/dmg/*.dmg "$dist"
# Versioned name: the new bundle is uploaded beside the old one before latest.json points at it.
for ext in tar.gz tar.gz.sig; do
  cp "target/release/bundle/macos/Kelta.app.$ext" "$dist/Kelta_${KELTA_VERSION}_aarch64.app.$ext"
done

# Linux only on request: a broken Linux build (#207) must not block the macOS nightly.
arches=""
if ((linux)); then arches="arm64 amd64"; fi
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

# Notes: commit + history since the previous nightly (if it is known locally).
git fetch -q origin tag nightly --force 2>/dev/null || true
prev="$(git rev-parse -q --verify 'refs/tags/nightly^{commit}' || true)"
notes="$(mktemp)"
{
  echo "Local build $KELTA_VERSION of \`$sha\`. Installed copies update themselves (Settings → General)."
  echo
  if [[ -n "$prev" ]]; then git log --oneline "$prev..HEAD"; else git log --oneline -20; fi
  echo
  echo "macOS: the app is not signed, see the README (right-click Open, or xattr -dr com.apple.quarantine)."
} > "$notes"
bash scripts/update-manifest.sh "$dist" "$KELTA_VERSION" "$notes" \
  https://github.com/InjustFr/kelta/releases/download/nightly
(cd "$dist" && shasum -a 256 -- * > SHA256SUMS)

# Update path: upload the new (versioned) assets first, then latest.json, then delete the old assets, so
# an installed copy never reads a manifest pointing at a missing file. A failed upload leaves the old tag,
# notes and manifest with the old build (re-run to fix). Create path: the tag must exist first.
git tag -f nightly "$sha"
if gh release view nightly >/dev/null 2>&1; then
  find "$dist" -type f ! -name latest.json -print0 | xargs -0 gh release upload nightly --clobber
  gh release upload nightly "$dist/latest.json" --clobber
  git push -f origin refs/tags/nightly
  gh release edit nightly --prerelease --title nightly --notes-file "$notes"
  # Delete assets of older builds (names change with the version).
  gh release view nightly --json assets --jq '.assets[].name' | while read -r a; do
    [[ -e "$dist/$a" ]] || gh release delete-asset nightly "$a" --yes
  done
else
  git push -f origin refs/tags/nightly
  gh release create nightly --prerelease --verify-tag --title nightly --notes-file "$notes" "$dist"/*
fi
rm "$notes"
gh release view nightly --json url,assets --jq '.url, (.assets[] | "\(.name) \(.size)")'

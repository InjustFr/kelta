#!/usr/bin/env bash
# Local nightly: macOS dmg (host arch) on this Mac, Linux deb + AppImage for arm64 and amd64 in Docker
# (amd64 under QEMU on an arm64 Mac: slow), published to the rolling "nightly" prerelease with gh.
# Usage: bash scripts/nightly-local.sh   (HEAD of a clean tree; moves the remote `nightly` tag to HEAD)
set -euo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")/.."

[[ -z "$(git status --porcelain)" ]] || { echo "dirty tree: commit or stash first" >&2; exit 1; }
# The public nightly is main only: refuse unreviewed branches.
git fetch -q origin main && git merge-base --is-ancestor HEAD origin/main || { echo "HEAD is not on origin/main" >&2; exit 1; }
sha="$(git rev-parse HEAD)"
dist="$PWD/target/nightly-dist"
rm -rf "$dist" && mkdir -p "$dist"

# The bundler keeps older versioned files: clear them so only this build is published.
rm -rf target/release/bundle
bash packaging/build.sh
cp target/release/bundle/dmg/*.dmg "$dist"

for arch in arm64 amd64; do
  docker build --platform "linux/$arch" -f docker/ubuntu-build.Dockerfile -t "kelta-nightly-$arch" .
  # Source comes from `git archive` so only HEAD is built; /work/target is a per-arch volume (warm rebuilds).
  git archive HEAD | docker run --rm -i --platform "linux/$arch" \
    -v "kelta-nightly-target-$arch":/work/target -v kelta-cargo:/root/.cargo/registry \
    -v kelta-pnpm:/root/.local/share/pnpm/store -v "$dist":/out \
    -e APPIMAGE_EXTRACT_AND_RUN=1 "kelta-nightly-$arch" \
    bash -lc 'tar -x -C /work && cd /work && rm -rf target/release/bundle && bash packaging/build.sh \
      && cp target/release/bundle/deb/*.deb target/release/bundle/appimage/*.AppImage /out/'
done

(cd "$dist" && shasum -a 256 -- * > SHA256SUMS)

# Notes: commit + history since the previous nightly (if it is known locally).
git fetch -q origin tag nightly --force 2>/dev/null || true
prev="$(git rev-parse -q --verify 'refs/tags/nightly^{commit}' || true)"
notes="$(mktemp)"
{
  echo "Unsigned local build of \`$sha\`."
  echo
  if [[ -n "$prev" ]]; then git log --oneline "$prev..HEAD"; else git log --oneline -20; fi
  echo
  echo "macOS: the app is not signed, see the README (right-click Open, or xattr -dr com.apple.quarantine)."
} > "$notes"

# Update path: upload first, so a failed upload leaves the old tag and notes with the old build
# (--clobber replaces files one by one: a partial failure can still mix builds; re-run to fix).
# Tag and notes move only once the new files are in. Create path: the tag must exist first.
git tag -f nightly "$sha"
if gh release view nightly >/dev/null 2>&1; then
  gh release upload nightly "$dist"/* --clobber
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

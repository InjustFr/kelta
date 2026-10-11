#!/usr/bin/env bash
# Publishes <dist> (installers + signed updater bundles of <sha>, version <version>) to the rolling "nightly"
# prerelease with gh: release notes, the updater manifest `latest.json` (update-manifest.sh), SHA256SUMS and the
# `nightly` tag. Used by scripts/nightly-local.sh and .github/workflows/nightly.yml; needs the git history (notes)
# and the right to push the tag.
# Usage: publish-nightly.sh <dist> <version> <sha>
set -euo pipefail
[[ $# -eq 3 ]] || { echo "usage: publish-nightly.sh <dist> <version> <sha>" >&2; exit 2; }
dist=$1 version=$2 sha=$3
cd "$(dirname "${BASH_SOURCE[0]}")/.."

# Notes: commit + history since the previous nightly (if it is known locally).
git fetch -q origin tag nightly --force 2>/dev/null || true
prev="$(git rev-parse -q --verify 'refs/tags/nightly^{commit}' || true)"
notes="$(mktemp)"
{
  echo "Build $version of \`$sha\`. Installed copies update themselves (Settings → General)."
  echo
  if [[ -n "$prev" ]]; then git log --oneline "$prev..$sha"; else git log --oneline -20 "$sha"; fi
  echo
  echo "macOS: the app is not signed, see the README (right-click Open, or xattr -dr com.apple.quarantine)."
} > "$notes"
bash scripts/update-manifest.sh "$dist" "$version" "$notes" \
  https://github.com/InjustFr/kelta/releases/download/nightly
(cd "$dist" && shasum -a 256 -- * > SHA256SUMS)

# Update path: upload the new (versioned) assets first, then latest.json, then delete the old assets, so
# an installed copy never reads a manifest pointing at a missing file. A failed upload leaves the old tag,
# notes and manifest with the old build (re-run to fix). Create path: the tag must exist first.
# The tag moves through the API: CI's GITHUB_TOKEN may not git-push a ref that brings in workflow files.
move_tag() {
  local refs=repos/InjustFr/kelta/git/refs
  if git ls-remote --exit-code origin refs/tags/nightly >/dev/null; then
    gh api -X PATCH "$refs/tags/nightly" -f sha="$sha" -F force=true >/dev/null
  else
    gh api "$refs" -f ref=refs/tags/nightly -f sha="$sha" >/dev/null
  fi
}
if gh release view nightly >/dev/null 2>&1; then
  find "$dist" -type f ! -name latest.json -print0 | xargs -0 gh release upload nightly --clobber
  gh release upload nightly "$dist/latest.json" --clobber
  move_tag
  gh release edit nightly --prerelease --title nightly --notes-file "$notes"
  # Delete assets of older builds (names change with the version).
  gh release view nightly --json assets --jq '.assets[].name' | while read -r a; do
    [[ -e "$dist/$a" ]] || gh release delete-asset nightly "$a" --yes
  done
else
  move_tag
  gh release create nightly --prerelease --verify-tag --title nightly --notes-file "$notes" "$dist"/*
fi
rm "$notes"
gh release view nightly --json url,assets --jq '.url, (.assets[] | "\(.name) \(.size)")'

#!/usr/bin/env bash
# Self-test for update-manifest.sh: the macOS entry (Apple silicon), Linux entries
# only for built AppImages, and a refusal when a bundle has no `.sig`.
set -euo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")"
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
fail() { echo "update-manifest.test: $*" >&2; exit 1; }
base=https://example.test/nightly
echo 'notes with "quotes"' >"$tmp/notes"
d="$tmp/dist"
mkdir "$d"
touch "$d/Kelta_0.1.0-nightly.1_aarch64.app.tar.gz" "$d/Kelta_0.1.0-nightly.1_aarch64.dmg" "$d/Kelta_0.1.0_amd64.deb"
echo SIGMAC >"$d/Kelta_0.1.0-nightly.1_aarch64.app.tar.gz.sig"

field() { python3 -c "import json,sys; m=json.load(open(sys.argv[1])); print(eval(sys.argv[2]))" "$d/latest.json" "$1"; }

bash update-manifest.sh "$d" 0.1.0-nightly.1 "$tmp/notes" "$base/"
[ "$(field 'm["version"]')" = 0.1.0-nightly.1 ] || fail "version"
[ "$(field 'm["notes"]')" = 'notes with "quotes"' ] || fail "notes"
[ "$(field 'sorted(m["platforms"])')" = "['darwin-aarch64']" ] || fail "macOS only: $(cat "$d/latest.json")"
[ "$(field 'm["platforms"]["darwin-aarch64"]["url"]')" = "$base/Kelta_0.1.0-nightly.1_aarch64.app.tar.gz" ] || fail "url"
[ "$(field 'm["platforms"]["darwin-aarch64"]["signature"]')" = SIGMAC ] || fail "signature"
field 'm["pub_date"]' | grep -Eq '^[0-9]{4}-[0-9]{2}-[0-9]{2}T[0-9]{2}:[0-9]{2}:[0-9]{2}Z$' || fail "pub_date"

touch "$d/Kelta_0.1.0-nightly.1_amd64.AppImage" "$d/Kelta_0.1.0-nightly.1_aarch64.AppImage"
echo SIGX64 >"$d/Kelta_0.1.0-nightly.1_amd64.AppImage.sig"
echo SIGA64 >"$d/Kelta_0.1.0-nightly.1_aarch64.AppImage.sig"
bash update-manifest.sh "$d" 0.1.0-nightly.1 "$tmp/notes" "$base"
[ "$(field 'sorted(m["platforms"])')" = "['darwin-aarch64', 'linux-aarch64', 'linux-x86_64']" ] || fail "Linux entries"
[ "$(field 'm["platforms"]["linux-x86_64"]["signature"]')" = SIGX64 ] || fail "linux signature"

rm "$d/Kelta_0.1.0-nightly.1_aarch64.AppImage.sig"
if out=$(bash update-manifest.sh "$d" 0.1.0-nightly.1 "$tmp/notes" "$base" 2>&1); then
  fail "a missing .sig must fail"
fi
[[ "$out" == *"aarch64.AppImage.sig is missing"* ]] || fail "unexpected error: $out"
echo "update-manifest.test: ok"

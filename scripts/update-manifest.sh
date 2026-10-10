#!/usr/bin/env bash
# Writes <dist>/latest.json, the Tauri updater manifest (ARCHITECTURE §2 "Bundling"), for the signed
# updater bundles in <dist>: the macOS `*.app.tar.gz` (required) and the Linux AppImages (when built).
# Every bundle needs its `.sig`: a manifest without one would strand the installed copies.
# Usage: update-manifest.sh <dist> <version> <notes-file> <base-url>
set -euo pipefail
[[ $# -eq 4 ]] || { echo "usage: update-manifest.sh <dist> <version> <notes-file> <base-url>" >&2; exit 2; }
dist=$1 version=$2 notes=$3 base=${4%/}
die() { echo "update-manifest: $*" >&2; exit 1; }

entries=()
add() { # <target> <bundle>
  [[ -f "$2.sig" ]] || die "$(basename "$2").sig is missing (build with TAURI_SIGNING_PRIVATE_KEY)"
  entries+=("$1" "$base/$(basename "$2")" "$2.sig")
}

shopt -s nullglob
macs=("$dist"/*.app.tar.gz)
[[ ${#macs[@]} -eq 1 ]] || die "want one .app.tar.gz in $dist, found ${#macs[@]}"
# The nightly Mac is Apple silicon (packaging/build.sh builds the host arch).
add darwin-aarch64 "${macs[0]}"
for f in "$dist"/*.AppImage; do
  case "$f" in
    *_amd64.AppImage) add linux-x86_64 "$f" ;;
    *_aarch64.AppImage) add linux-aarch64 "$f" ;;
    *) die "unknown AppImage architecture: $(basename "$f")" ;;
  esac
done

python3 - "$version" "$notes" "$dist/latest.json" "${entries[@]}" <<'PY'
import datetime, json, sys

version, notes, out, *rest = sys.argv[1:]
platforms = {}
for i in range(0, len(rest), 3):
    target, url, sig = rest[i : i + 3]
    with open(sig) as f:
        platforms[target] = {"url": url, "signature": f.read().strip()}
with open(notes) as f:
    text = f.read().strip()
now = datetime.datetime.now(datetime.timezone.utc).replace(microsecond=0)
manifest = {
    "version": version,
    "notes": text,
    "pub_date": now.isoformat().replace("+00:00", "Z"),
    "platforms": platforms,
}
with open(out, "w") as f:
    json.dump(manifest, f, indent=2)
    f.write("\n")
PY

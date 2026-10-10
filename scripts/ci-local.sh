#!/usr/bin/env bash
# Local mirror of .github/workflows/ci.yml. Run it before opening a PR so GitHub CI only confirms.
#   scripts/ci-local.sh [--base <ref>] [--linux]
# Runs scripts/qa.sh, the Playwright e2e on mock IPC and cargo-deny. scripts/linux-check.sh (Ubuntu in
# Docker) runs with --linux, or automatically when the diff against --base (default origin/main) touches
# Linux code: apps/desktop/src-tauri, packaging, docker, crates/*/src/**/*linux*, cfg(target_os = "linux").
# Stops at the first failing step. Exit code 0 only when every step that ran passed.
set -euo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")/.."

base=origin/main
linux=0
while [ $# -gt 0 ]; do
  case "$1" in
    --base) base="${2:?--base needs a ref}"; shift 2 ;;
    --linux) linux=1; shift ;;
    -h | --help) sed -n '2,7p' "$0" | sed 's/^# \{0,1\}//'; exit 0 ;;
    *) echo "unknown argument: $1 (try --help)" >&2; exit 2 ;;
  esac
done

if ! command -v cargo-deny >/dev/null 2>&1; then
  echo "ci-local: cargo-deny is not on PATH; install it with \`brew install cargo-deny\` or \`cargo install cargo-deny\`" >&2
  exit 1
fi

names=() results=() secs=()
failed=0
record() { names+=("$1"); results+=("$2"); secs+=("$3"); }
skip() { printf '\n##### %s: skipped (%s)\n' "$1" "$2" >&2; record "$1" "skipped ($2)" "-"; }
run() { # run <name> <command...>
  local name="$1" t0=$SECONDS
  shift
  if [ "$failed" = 1 ]; then record "$name" "not run" "-"; return 0; fi
  printf '\n##### %s\n' "$name"
  if "$@"; then record "$name" pass "$((SECONDS - t0))"; else record "$name" FAIL "$((SECONDS - t0))"; failed=1; fi
}

free_port() { node -e 'const s=require("net").createServer().listen(0,"127.0.0.1",()=>{console.log(s.address().port);s.close()})'; }
e2e() {
  pnpm --filter @kelta/ui exec playwright install chromium
  E2E_PORT="$(free_port)" CI=1 pnpm --filter @kelta/ui run e2e
}

linux_relevant() {
  git rev-parse --verify -q "$base^{commit}" >/dev/null || { echo "ci-local: base '$base' not found, no Linux auto-detection" >&2; return 1; }
  local files f
  files="$({ git diff --name-only "$base"...HEAD; git diff --name-only HEAD; } | sort -u)"
  printf '%s\n' "$files" | grep -Eq '^(apps/desktop/src-tauri/|packaging/|docker/|crates/[^/]+/src/(.*/)?[^/]*linux[^/]*$)' && return 0
  while IFS= read -r f; do
    [ -f "$f" ] && grep -q 'target_os = "linux"' "$f" && return 0
  done <<<"$files"
  return 1
}

run qa bash scripts/qa.sh
run e2e e2e

run cargo-deny cargo deny check licenses bans sources

if [ "$(uname -s)" = Linux ]; then
  skip linux-check "host is Linux"
elif [ "$linux" = 1 ] || linux_relevant; then
  if command -v docker >/dev/null 2>&1; then
    run linux-check bash scripts/linux-check.sh
  elif [ "$linux" = 1 ]; then
    echo "ci-local: docker not found but --linux was given" >&2
    run linux-check false
  else
    echo "ci-local: WARNING Linux code changed but docker is missing, skipping linux-check" >&2
    skip linux-check "no docker"
  fi
else
  skip linux-check "no Linux paths changed"
fi

printf '\n%-14s %-30s %s\n' STEP RESULT SECONDS
for i in "${!names[@]}"; do
  printf '%-14s %-30s %s\n' "${names[$i]}" "${results[$i]}" "${secs[$i]}"
done
[ "$failed" = 0 ] || exit 1

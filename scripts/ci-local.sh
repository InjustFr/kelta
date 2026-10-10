#!/usr/bin/env bash
# Local mirror of .github/workflows/ci.yml. Run it before opening a PR so GitHub CI only confirms.
#   scripts/ci-local.sh [--base <ref>] [--full] [--linux]
# Runs scripts/qa.sh, the Playwright e2e on mock IPC and cargo-deny, only for what the diff against --base
# (default origin/main, plus uncommitted and untracked files) affects, as computed by scripts/affected.py:
# the Rust steps on the changed workspace packages and their reverse dependencies, the UI half when ui/,
# packages/ or the pnpm/eslint config changed, e2e when the UI, apps/desktop or kelta-proto changed, cargo-deny
# when Cargo.lock or deny.toml changed. --full (release tags, nightly) runs every step, as does a change under
# scripts/ or a failed detection. scripts/linux-check.sh (Ubuntu in Docker) runs with --linux or --full, or
# when the diff touches Linux code: apps/desktop/src-tauri, packaging, docker, crates/*/src/**/*linux*,
# cfg(target_os = "linux"). Heavy steps wait for one of KELTA_GATE_SLOTS (default 2) machine-wide slots.
# Each run logs to its own file under ${TMPDIR}/kelta-gate/logs/. Stops at the first failing step. Exit code 0
# only when every step that ran passed.
set -euo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")/.."

base=origin/main
linux=0
full=0
while [ $# -gt 0 ]; do
  case "$1" in
    --base) base="${2:?--base needs a ref}"; shift 2 ;;
    --linux) linux=1; shift ;;
    --full) full=1; linux=1; shift ;;
    -h | --help) sed -n '2,13p' "$0" | sed 's/^# \{0,1\}//'; exit 0 ;;
    *) echo "unknown argument: $1 (try --help)" >&2; exit 2 ;;
  esac
done

log="${TMPDIR:-/tmp}/kelta-gate/logs/ci-local-$(date +%Y%m%d-%H%M%S)-$$.log"
mkdir -p "$(dirname "$log")"
exec > >(tee "$log") 2>&1
echo "ci-local: log $log"

rust=all ui=1 e2e=1 deny=1
if [ "$full" = 0 ]; then
  if plan="$(python3 scripts/affected.py plan "$base")"; then
    eval "$plan"
  else
    echo "ci-local: change detection failed, running every step" >&2
  fi
fi
echo "ci-local: rust=$rust ui=$ui e2e=$e2e deny=$deny"

if [ "$deny" = 1 ] && ! command -v cargo-deny >/dev/null 2>&1; then
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

qa_args=()  # empty: the full qa.sh (linux-check after a change qa.sh had nothing to run for)
case "$rust,$ui" in
  none,0) skip qa "nothing affected" ;;
  none,1) qa_args=(ui) ;;
  all,1) qa_args=(all) ;;
  all,0) qa_args=(rust) ;;
  *,1) read -ra qa_args <<<"all $rust" ;;
  *,0) read -ra qa_args <<<"rust $rust" ;;
esac
[ ${#qa_args[@]} -eq 0 ] || run qa bash scripts/qa.sh "${qa_args[@]}"
if [ "$e2e" = 1 ]; then run e2e e2e; else skip e2e "no UI, desktop or kelta-proto change"; fi
if [ "$deny" = 1 ]; then
  run cargo-deny cargo deny check licenses bans sources
else
  skip cargo-deny "Cargo.lock and deny.toml unchanged"
fi

if [ "$(uname -s)" = Linux ]; then
  skip linux-check "host is Linux"
elif [ "$linux" = 1 ] || linux_relevant; then
  if command -v docker >/dev/null 2>&1; then
    run linux-check perl scripts/gate-slot.pl bash scripts/linux-check.sh "${qa_args[@]}"
  elif [ "$linux" = 1 ] && [ "$full" = 0 ]; then
    echo "ci-local: docker not found but --linux was given" >&2
    run linux-check false
  else
    echo "ci-local: WARNING Linux code changed but docker is missing, skipping linux-check" >&2
    skip linux-check "no docker"
  fi
else
  skip linux-check "no Linux paths changed"
fi

printf '\n%-14s %-50s %s\n' STEP RESULT SECONDS
for i in "${!names[@]}"; do
  printf '%-14s %-50s %s\n' "${names[$i]}" "${results[$i]}" "${secs[$i]}"
done
# nextest prints a test that failed and then passed on its retry as FLAKY: list them, never hide them.
flaky="$(grep -E '^ *FLAKY ' "$log" | sort -u || true)"
[ -z "$flaky" ] || printf '\nFLAKY (failed, then passed on retry):\n%s\n' "$flaky"
echo "ci-local: log $log"
[ "$failed" = 0 ] || exit 1

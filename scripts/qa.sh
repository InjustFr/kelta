#!/usr/bin/env bash
# BUILD_PLAN §7: the QA commands, in order. Run from anywhere; operates on the repo root.
# Usage: qa.sh [all|ui|rust] [package...]   (default all; CI runs the halves in separate jobs)
# Packages limit the Rust steps to those workspace packages (default: the whole workspace); scripts/ci-local.sh
# passes the affected ones. Rust tests use cargo-nextest (--retries 1, retried passes print as FLAKY) when it
# is installed, else cargo test. Heavy cargo steps wait for a machine-wide slot (scripts/gate-slot.pl).
# With `all`, the UI checks run in the background during the Rust steps; their log prints when they end.
set -euo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")/.."

part="${1:-all}"
case "$part" in all | ui | rust) ;; *) echo "usage: qa.sh [all|ui|rust] [package...]" >&2; exit 2 ;; esac
pkgs=("${@:2}")
want() { [[ "$part" == all || "$part" == "$1" ]]; }
section() { printf '\n==> %s\n' "$*"; }

# The UI build runs for every part: tauri::generate_context! needs ui/dist.
section "pnpm install --frozen-lockfile"
pnpm install --frozen-lockfile

section "pnpm --filter @kelta/ui run build"
pnpm --filter @kelta/ui run build

ui_checks() {
  section "pnpm -r --if-present run check"
  pnpm -r --if-present run check

  section "pnpm -r --if-present run lint"
  pnpm -r --if-present run lint

  section "pnpm -r --if-present run test"
  pnpm -r --if-present run test
}
ui_pid=
# Prints the log of the UI checks running alongside the Rust steps once they end; fails when they failed.
ui_wait() {
  [ -n "$ui_pid" ] || return 0
  local rc=0
  wait "$ui_pid" || rc=$?
  ui_pid=
  section "UI checks (ran alongside the Rust steps): $([ "$rc" = 0 ] && echo passed || echo "FAILED (exit $rc)")"
  cat "$ui_log"
  rm -f "$ui_log"
  return "$rc"
}
if [[ "$part" == all ]]; then
  ui_log="$(mktemp "${TMPDIR:-/tmp}/kelta-qa-ui.XXXXXX")"
  ui_checks >"$ui_log" 2>&1 &
  ui_pid=$!
  # A failing Rust step exits early: still wait for the UI checks and show their log.
  trap 'rc=$?; ui_wait || rc=1; exit "$rc"' EXIT
elif want ui; then
  ui_checks
fi

# Scans both TypeScript and Rust sources; cheap, so it runs in every part.
section "bash scripts/check-no-timers.sh"
bash scripts/check-no-timers.sh
bash scripts/check-no-timers.test.sh
python3 scripts/affected_test.py

if want rust; then
  slot() { perl scripts/gate-slot.pl "$@"; }
  if [ ${#pkgs[@]} -eq 0 ]; then
    scope=(--workspace) fmt_scope=(--all) doc_scope=(--workspace)
  else
    scope=() fmt_scope=() doc_scope=()
    for p in "${pkgs[@]}"; do scope+=(-p "$p"); done
    fmt_scope=("${scope[@]}")
    # cargo test --doc fails on a package without a library target.
    for p in $(python3 scripts/affected.py libs "${pkgs[@]}"); do doc_scope+=(-p "$p"); done
  fi

  section "cargo fmt ${fmt_scope[*]} -- --check"
  cargo fmt "${fmt_scope[@]}" -- --check

  section "cargo clippy ${scope[*]} --all-targets --locked -- -D warnings"
  slot cargo clippy "${scope[@]}" --all-targets --locked -- -D warnings

  # --no-fail-fast: one run lists every failing test, not just the first failing binary.
  if command -v cargo-nextest >/dev/null 2>&1; then
    section "cargo nextest run ${scope[*]} --locked --no-fail-fast --retries 1"
    slot cargo nextest run "${scope[@]}" --locked --no-fail-fast --retries 1 --no-tests=warn
    if [ ${#doc_scope[@]} -gt 0 ]; then
      section "cargo test --doc ${doc_scope[*]} --locked"
      slot cargo test --doc "${doc_scope[@]}" --locked --no-fail-fast
    fi
  else
    section "cargo test ${scope[*]} --locked --no-fail-fast (cargo-nextest not installed)"
    slot cargo test "${scope[@]}" --locked --no-fail-fast
  fi
fi

ui_wait
section "QA passed ($part)"

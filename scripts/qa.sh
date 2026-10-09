#!/usr/bin/env bash
# BUILD_PLAN §7: the QA commands, in order. Run from anywhere; operates on the repo root.
# Usage: qa.sh [all|ui|rust]   (default all; CI runs the halves in separate jobs)
set -euo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")/.."

part="${1:-all}"
case "$part" in all | ui | rust) ;; *) echo "usage: qa.sh [all|ui|rust]" >&2; exit 2 ;; esac
want() { [[ "$part" == all || "$part" == "$1" ]]; }
section() { printf '\n==> %s\n' "$*"; }

# The UI build runs for every part: tauri::generate_context! needs ui/dist.
section "pnpm install --frozen-lockfile"
pnpm install --frozen-lockfile

section "pnpm --filter @kelta/ui run build"
pnpm --filter @kelta/ui run build

if want ui; then
  section "pnpm -r --if-present run check"
  pnpm -r --if-present run check

  section "pnpm -r --if-present run lint"
  pnpm -r --if-present run lint

  section "pnpm -r --if-present run test"
  pnpm -r --if-present run test
fi

# Scans both TypeScript and Rust sources; cheap, so it runs in every part.
section "bash scripts/check-no-timers.sh"
bash scripts/check-no-timers.sh

if want rust; then
  section "cargo fmt --all -- --check"
  cargo fmt --all -- --check

  section "cargo clippy --workspace --all-targets --locked -- -D warnings"
  cargo clippy --workspace --all-targets --locked -- -D warnings

  # --no-fail-fast: one run lists every failing test, not just the first failing binary.
  section "cargo test --workspace --locked --no-fail-fast"
  cargo test --workspace --locked --no-fail-fast

  section "cargo run -p xtask --locked -- codegen --check"
  cargo run -p xtask --locked -- codegen --check
fi

section "QA passed ($part)"

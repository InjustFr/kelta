#!/usr/bin/env bash
# BUILD_PLAN §7: the QA commands, in order. Run from anywhere; operates on the repo root.
set -euo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")/.."

section() { printf '\n==> %s\n' "$*"; }

section "pnpm install --frozen-lockfile"
pnpm install --frozen-lockfile

section "pnpm --filter @kelta/ui run build"
pnpm --filter @kelta/ui run build

section "pnpm -r --if-present run check"
pnpm -r --if-present run check

section "pnpm -r --if-present run lint"
pnpm -r --if-present run lint

section "pnpm -r --if-present run test"
pnpm -r --if-present run test

section "bash scripts/check-no-timers.sh"
bash scripts/check-no-timers.sh

section "cargo fmt --all -- --check"
cargo fmt --all -- --check

section "cargo clippy --workspace --all-targets --locked -- -D warnings"
cargo clippy --workspace --all-targets --locked -- -D warnings

section "cargo test --workspace --locked"
cargo test --workspace --locked

section "cargo run -p xtask --locked -- codegen --check"
cargo run -p xtask --locked -- codegen --check

section "QA passed"

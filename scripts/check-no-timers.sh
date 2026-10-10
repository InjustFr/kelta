#!/usr/bin/env bash
# BUILD_PLAN §1.5: no periodic timers or ad hoc threads outside allowlisted modules.
# Portable between BSD and GNU userland (grep -E, awk, find only).
# Usage: check-no-timers.sh [root]   (default: the repo; scripts/check-no-timers.test.sh passes a fixture)
set -euo pipefail
cd "${1:-$(dirname "${BASH_SOURCE[0]}")/..}"

fail=0
report() { printf 'check-no-timers: %s\n' "$*" >&2; fail=1; }

# ---- Frontend: setInterval and recursive requestAnimationFrame -------------
ts_dirs=()
for d in ui/src packages/plugin-sdk/src; do [ -d "$d" ] && ts_dirs+=("$d"); done

if [ ${#ts_dirs[@]} -gt 0 ]; then
  ts_files=$(find "${ts_dirs[@]}" -type f \( -name '*.ts' -o -name '*.js' -o -name '*.svelte' -o -name '*.mjs' \) \
    ! -path '*/lib/gen/*' ! -name '*.test.ts' ! -name '*.spec.ts' ! -path 'ui/src/lib/terminal/raf.ts' | sort)

  while IFS= read -r f; do
    [ -n "$f" ] || continue
    awk -v F="$f" '
      /^[[:space:]]*(\/\/|\*|\/\*)/ { next }
      /setInterval[[:space:]]*\(/ { printf "%s:%d: setInterval is forbidden\n", F, NR; bad=1 }
      # remember function definitions: function x / const x = / x = (
      match($0, /function[[:space:]]+[A-Za-z_$][A-Za-z0-9_$]*/) {
        s = substr($0, RSTART, RLENGTH); sub(/function[[:space:]]+/, "", s); cur = s; defline[cur] = NR
      }
      match($0, /(const|let|var)[[:space:]]+[A-Za-z_$][A-Za-z0-9_$]*[[:space:]]*=[[:space:]]*(async[[:space:]]*)?(\(|function|[A-Za-z_$][A-Za-z0-9_$]*[[:space:]]*=>)/) {
        s = substr($0, RSTART, RLENGTH); sub(/^(const|let|var)[[:space:]]+/, "", s); sub(/[[:space:]]*=.*/, "", s); cur = s; defline[cur] = NR
      }
      /requestAnimationFrame[[:space:]]*\(/ {
        line = $0
        # rAF(name) or rAF(() => name()) where name is a function defined earlier in this file
        for (n in defline) {
          if (line ~ ("requestAnimationFrame[[:space:]]*\\([[:space:]]*" n "[[:space:]]*[,)]") ||
              line ~ ("requestAnimationFrame[[:space:]]*\\([^)]*=>[[:space:]]*" n "[[:space:]]*\\(")) {
            printf "%s:%d: recursive requestAnimationFrame (%s)\n", F, NR, n; bad=1
          }
        }
      }
      END { exit bad }
    ' "$f" >&2 || fail=1
  done <<<"$ts_files"
fi

# ---- Rust: interval / sleep / thread::spawn ---------------------------------
rs_dirs=()
for d in crates apps; do [ -d "$d" ] && rs_dirs+=("$d"); done

# Bare `interval(` / `time::interval(` catch `use tokio::time::{interval, ..}` and `use tokio::time;`.
# shortcut: bare `spawn(`/`sleep(` from `use std::thread::{..}` are not caught (too many false hits), add if it happens.
pattern='tokio::time::interval(_at)?|(^|[^A-Za-z0-9_:.])(time::)?interval(_at)?[[:space:]]*[(]|std::thread::(spawn|sleep)|thread::(spawn|sleep)[[:space:]]*[(]'

if [ ${#rs_dirs[@]} -gt 0 ]; then
  rs_files=$(find "${rs_dirs[@]}" -type f -name '*.rs' \
    ! -path '*/tests/*' ! -path '*/benches/*' ! -path '*/target/*' ! -path '*/gen/*' \
    ! -name '*_test.rs' ! -name '*_tests.rs' ! -name 'tests.rs' ! -name 'test_*.rs' \
    ! -path 'crates/kelta-core/src/scheduler*' ! -path 'crates/kelta-core/src/store*' \
    ! -path 'crates/kelta-term/src/reader*' | sort)

  while IFS= read -r f; do
    [ -n "$f" ] || continue
    awk -v F="$f" -v PAT="$pattern" '
      # Skip only the #[cfg(test)] item: up to its closing brace, or its `;` when it has no body.
      # shortcut: braces inside strings/chars are counted too, fine for test modules and helpers.
      /^[[:space:]]*#\[cfg\(test\)\]/ { skip = 1; depth = 0; opened = 0; sub(/.*#\[cfg\(test\)\]/, "") }
      skip {
        o = gsub(/[{]/, "{"); c = gsub(/[}]/, "}"); depth += o - c; if (o) opened = 1
        if ((opened && depth <= 0) || (!opened && $0 ~ /;[[:space:]]*$/)) skip = 0
        next
      }
      { prev2 = prev; prev = cur; cur = $0 }
      /^[[:space:]]*\/\// { next }
      $0 ~ PAT {
        if ($0 ~ /allowlisted:/ || prev ~ /allowlisted:/ || prev2 ~ /allowlisted:/) next
        printf "%s:%d: forbidden timer/thread API: %s\n", F, NR, $0; bad = 1
      }
      END { exit bad }
    ' "$f" >&2 || fail=1
  done <<<"$rs_files"
fi

if [ "$fail" -ne 0 ]; then
  echo "check-no-timers: FAILED (see BUILD_PLAN §1.5)" >&2
  exit 1
fi
echo "check-no-timers: ok"

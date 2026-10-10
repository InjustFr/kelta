#!/usr/bin/env bash
# Self-test for check-no-timers.sh: the fixture's `// BAD` lines, and only those, are reported.
set -euo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")"
fixture=fixtures/check-no-timers
if out=$(bash check-no-timers.sh "$fixture" 2>&1); then
  echo "check-no-timers.test: expected a failure, got: $out" >&2
  exit 1
fi
want=$(grep -n '// BAD' "$fixture/crates/demo/src/lib.rs" | cut -d: -f1 | sort -n | tr '\n' ' ')
got=$(printf '%s\n' "$out" | sed -n 's/^crates\/demo\/src\/lib.rs:\([0-9]*\):.*/\1/p' | sort -n | tr '\n' ' ')
if [ "$want" != "$got" ]; then
  printf 'check-no-timers.test: reported lines [%s], want [%s]\n%s\n' "$got" "$want" "$out" >&2
  exit 1
fi
echo "check-no-timers.test: ok"

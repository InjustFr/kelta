#!/usr/bin/env bash
# Gate G2 (BUILD_PLAN §6): Linux terminal latency under a headless Wayland compositor.
# Builds the release app in Docker ubuntu:24.04, runs it under headless sway with WebKitGTK and runs
# kelta-bench echo-latency, ink-redraw and flood (DOM renderer). Saves one screenshot of the window
# to docs/images/linux-headless.png. Exit code 0 only when every scenario is within budget.
set -euo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")/.."

if [ "${1:-}" != --inside ]; then
  docker build -f docker/ubuntu-build.Dockerfile -t kelta-ci .
  docker build -f docker/gui.Dockerfile -t kelta-gui .
  mkdir -p docs/images
  exec docker run --rm --shm-size=1g -v "$PWD":/src:ro -v "$PWD/docs/images":/out \
    -v kelta-cargo:/root/.cargo/registry -v kelta-pnpm:/root/.local/share/pnpm/store \
    -v kelta-gui-target:/work/target \
    kelta-gui bash -lc 'rsync -a --delete --exclude target --exclude node_modules --exclude .git /src/ /work/ \
      && cd /work && dbus-run-session -- bash scripts/linux-gui-check.sh --inside'
fi

# `open` needs each fixture project to be a work tree.
# shortcut: done here on the throwaway copy, not by kelta-bench; move it into bench/src/run.rs if the bench runs elsewhere.
for p in bench/fixtures/3p10s/projects/*/; do git -C "$p" init -q; done
pnpm install --frozen-lockfile
pnpm --filter @kelta/ui run build
cargo build --release --locked -p kelta-desktop --features tauri/custom-protocol
cargo build --release --locked -p kelta-bench -p tui-sim

export XDG_RUNTIME_DIR=/tmp/xdg
mkdir -p -m 700 "$XDG_RUNTIME_DIR"
printf 'output HEADLESS-1 resolution 1440x900\ndefault_border none\n' >/tmp/sway.conf
WLR_BACKENDS=headless WLR_RENDERER=pixman WLR_LIBINPUT_NO_DEVICES=1 sway -c /tmp/sway.conf >/tmp/sway.log 2>&1 &
for _ in $(seq 50); do [ -S "$XDG_RUNTIME_DIR/wayland-1" ] && break; sleep 0.2; done
[ -S "$XDG_RUNTIME_DIR/wayland-1" ] || { cat /tmp/sway.log; exit 1; }
# Absolute: kelta-bench gives the app its own XDG_RUNTIME_DIR.
export WAYLAND_DISPLAY="$XDG_RUNTIME_DIR/wayland-1" GDK_BACKEND=wayland
# Docker has no user namespaces for bubblewrap; test container only.
export WEBKIT_DISABLE_SANDBOX_THIS_IS_DANGEROUS=1

status=0
for s in flood echo-latency ink-redraw; do
  target/release/kelta-bench --scenario "$s" --budgets bench/budgets-g2.toml || status=1
done

# The proof screenshot, from an extra unmeasured flood run (grim would perturb the measured ones): the
# last capture taken while the app was still up, i.e. before the scenario's last mark (`inflight_kib`),
# after which kelta-bench closes it.
target/release/kelta-bench --scenario flood >/dev/null 2>&1 &
bench=$!
shot=0
while kill -0 "$bench" 2>/dev/null; do
  if grep -qs app_ready_ms /tmp/kb-*/marks.json; then
    WAYLAND_DISPLAY=wayland-1 grim /tmp/new.png
    if grep -qs app_ready_ms /tmp/kb-*/marks.json && ! grep -qs inflight_kib /tmp/kb-*/marks.json; then
      cp /tmp/new.png /out/linux-headless.png && shot=1
    fi
  fi
  sleep 0.5
done
[ "$shot" = 1 ] || { echo "linux-gui-check: no screenshot taken" >&2; status=1; }
exit "$status"

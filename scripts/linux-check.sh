#!/usr/bin/env bash
# BUILD_PLAN §7: run the QA inside ubuntu:24.04 from the dev machine. Arguments go to scripts/qa.sh
# (default: everything).
set -euo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")/.."

# The image CI runs in (.github/workflows/ci-image.yml), built here only when it cannot be pulled.
# shortcut: an edited Dockerfile is used only once main publishes it; `docker build -t <image>` it by hand until then.
image=ghcr.io/injustfr/kelta-ci:latest
docker pull -q "$image" || docker build -f docker/ubuntu-build.Dockerfile -t "$image" .
# /work/target is a volume: a rerun only rebuilds what changed.
docker run --rm -v "$PWD":/src:ro -v kelta-cargo:/root/.cargo/registry -v kelta-pnpm:/root/.local/share/pnpm/store \
  -v kelta-linux-target:/work/target \
  "$image" bash -lc 'rsync -a --delete --exclude target --exclude node_modules --exclude .git /src/ /work/ && cd /work && bash scripts/qa.sh "$@"' qa "$@"

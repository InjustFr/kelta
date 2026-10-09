#!/usr/bin/env bash
# BUILD_PLAN §7: run the full QA inside ubuntu:24.04 from the dev machine.
set -euo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")/.."

docker build -f docker/ubuntu-build.Dockerfile -t kelta-ci .
docker run --rm -v "$PWD":/src:ro -v kelta-cargo:/root/.cargo/registry -v kelta-pnpm:/root/.local/share/pnpm/store \
  kelta-ci bash -lc 'rsync -a --delete --exclude target --exclude node_modules --exclude .git /src/ /work/ && cd /work && bash scripts/qa.sh'

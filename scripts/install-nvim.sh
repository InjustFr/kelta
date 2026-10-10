#!/usr/bin/env bash
# Installs the pinned Neovim release into PREFIX (default /usr/local): CI and docker/ubuntu-build.Dockerfile
# share it so the nvim tests run against one version (distro packages lag, e.g. Ubuntu 24.04 ships 0.9).
set -euo pipefail
version=v0.12.6
prefix="${1:-/usr/local}"
case "$(uname -s)" in Linux) os=linux ;; Darwin) os=macos ;; *) echo "unsupported OS" >&2; exit 1 ;; esac
case "$(uname -m)" in x86_64 | amd64) arch=x86_64 ;; aarch64 | arm64) arch=arm64 ;; *) echo "unsupported arch" >&2; exit 1 ;; esac
curl -fsSL "https://github.com/neovim/neovim/releases/download/${version}/nvim-${os}-${arch}.tar.gz" \
  | tar -xz -C "$prefix" --strip-components=1
"$prefix/bin/nvim" --version | head -1

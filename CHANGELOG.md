# Changelog

All notable changes are listed here. The format follows Keep a Changelog, versions follow semver.

## [Unreleased]

### Added

- Platform shell: graphics workarounds applied before the webview starts (NVIDIA detection, `--safe-graphics`,
  automatic safe retry after a failed launch), window decorations modes, geometry restore, macOS app menu.
- Background mode: closing the window destroys the webview and keeps sessions alive; Dock click, a second
  `kelta` or `kelta-ctl toggle` reopens it.
- Diagnostics probes: graphics, WebKitGTK, notification daemon, Secret Service, tool versions, hooks, sockets.
- Packaging: deb, AppImage, universal dmg, desktop entry with a Safe graphics action, AppStream metadata,
  AUR and Homebrew cask templates.
- Release workflow (signed and notarized dmg on tags) and a rolling unsigned `nightly` prerelease.
- `kelta-bench` harness with memory, latency, wakeup and bundle-size budgets.

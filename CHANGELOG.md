# Changelog

All notable changes are listed here. The format follows Keep a Changelog, versions follow semver.

## [Unreleased]

### Fixed

- AppImage: terminals, Claude sessions and every other child process start from the user's environment, without
  the AppImage runtime's `PYTHONHOME`, `LD_LIBRARY_PATH`, `GTK_*` or `$APPDIR` entries in `PATH`. This now covers
  `keltad` and every other spawn path, any variable pointing into the mount or at the `.AppImage` file, and
  AppImageLauncher's `*_APPIMAGE`, `DESKTOPINTEGRATION` and `GIO_LAUNCHED_DESKTOP_FILE*`.

### Changed

- Plugin `exec:` grants match the command exactly: a command with a `/` needs a grant naming that path
  (`exec:./bin/deploy.sh`), so `exec:<basename>` no longer covers path commands. Paths resolve inside the
  plugin directory; names run from the login shell's `PATH`, which a plugin's `env` cannot override.

### Added

- Detached windows: a pane or a tab moves to its own window (pane menu, rail row menu, `pane.detach`) to put
  it on another screen, sessions untouched; closing that window brings the tab back, and detached windows reopen
  where they were after a restart.
- Platform shell: graphics workarounds applied before the webview starts (NVIDIA detection, `--safe-graphics`,
  automatic safe retry after a failed launch), window decorations modes, geometry restore, macOS app menu.
- Background mode: closing the window destroys the webview and keeps sessions alive; Dock click, a second
  `kelta` or `kelta-ctl toggle` reopens it.
- Diagnostics probes: graphics, WebKitGTK, notification daemon, Secret Service, tool versions, hooks, sockets.
- Packaging: deb, AppImage, universal dmg, desktop entry with a Safe graphics action, AppStream metadata,
  AUR and Homebrew cask templates.
- Release workflow (signed and notarized dmg on tags) and a rolling unsigned `nightly` prerelease.
- `kelta-bench` harness with memory, latency, wakeup and bundle-size budgets.

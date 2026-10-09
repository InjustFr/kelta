# Getting started

## Install

See "Try the latest build" in the [README](../../README.md): dmg on macOS, AppImage or deb on Linux.
`kelta-ctl` is installed next to the app and copied to `<data>/bin/current` so scripts and compositor bindings
have a stable path.

## First launch

1. Open a project: `+` in the project rail, or `kelta-ctl open ~/code/shop`.
2. Add an account (Settings > Accounts) for your tracker and code host. Tokens go to the OS keyring; if you have
   none, use `env:` or `command:` references ([keyring setup](keyring-and-notifications.md)).
3. Pick a ticket in the board and press **Start work**: Kelta creates a worktree and opens Claude and nvim
   side by side.

Settings > Diagnostics checks the things Kelta depends on: graphics, WebKitGTK, notification daemon, keyring,
`claude`, `nvim`, `git`, `gh`, `glab`, hook health and socket paths.

## Window behaviour

- macOS: closing the window keeps every session running (background mode). Click the Dock icon to come back.
- Linux: closing the window quits. Set `window.close_behavior = "background"` to keep sessions running; then
  `kelta`, a second launch or `kelta-ctl toggle` reopens the window.
- `window.decorations`: `auto` (no title bar on Hyprland and Sway), `native`, `none`, `custom`. Restart to apply.
- `window.restore_geometry` restores the size and position of the last session.

## Command line

`kelta [--safe-graphics] [--config-dir DIR] [--set key=value] [PATH...]`. Starting `kelta` while it runs forwards
the arguments to the running instance and raises its window.

`kelta-ctl`: `toggle`, `palette`, `open <path>`, `focus-project <id>`, `start <ticket> [--project]`,
`new --template <id>`, `emit <custom.event>`, `trust <repo>`, `editor-open <file>[:line]`, `version`.

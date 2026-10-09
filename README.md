# Kelta

Claude Code and nvim side by side in built-in terminals, with your tracker tickets and pull request
reviews next to them, many projects in one window, and a very small memory footprint.

Open source (MIT). Desktop app built with Tauri v2, Svelte 5 and Rust. macOS 13+ and Ubuntu 24.04+
(Hyprland, Sway, GNOME).

<!-- screenshots: add docs/images/*.png and link them here -->

## Features

- Take a ticket (Jira, Redmine, GitHub Issues, GitLab Issues, Linear), build it in a git worktree with Claude
  Code and nvim side by side.
- Review the pull requests and merge requests you are asked to review (GitHub, GitLab).
- Several projects in one window. Switching never stops a session.
- Tools (lazygit, lazydocker, `sl web`), triggers and plugins. See [docs/user](docs/user) and
  [docs/plugins-guide](docs/plugins-guide).
- Closing the window keeps sessions running on macOS (background mode). `kelta-ctl toggle` brings it back.
- Budget: about 220 MB total for 3 projects and 10 live sessions, 60 MB with the window closed.

## Try the latest build

Every push to `main` publishes unsigned builds to the rolling
[nightly prerelease](https://github.com/InjustFr/kelta/releases/tag/nightly):

| Platform | File |
|---|---|
| macOS 13+ (universal) | `Kelta_*_universal.dmg` |
| Linux, any distro | `kelta_*_amd64.AppImage` |
| Debian, Ubuntu 24.04+ | `kelta_*_amd64.deb` |

The release notes list the commit and the lanes merged so far. The builds are for testing; expect rough edges.

### macOS

The app is not signed. Open the dmg, drag Kelta to Applications, then either right-click the app and
choose **Open** the first time, or remove the quarantine flag:

```sh
xattr -dr com.apple.quarantine /Applications/Kelta.app
```

### Linux

```sh
chmod +x kelta_*_amd64.AppImage
./kelta_*_amd64.AppImage
```

or `sudo apt install ./kelta_*_amd64.deb`. It needs `libwebkit2gtk-4.1` and GTK 3.

Wayland notes:

- Hyprland and Sway: the window has no title bar (`window.decorations = "auto"`). Bind
  `kelta-ctl toggle` to a key. See [docs/user/compositors.md](docs/user/compositors.md).
- NVIDIA proprietary driver: Kelta turns off the DMABUF renderer and explicit sync for you. If the
  window stays blank, run `kelta --safe-graphics` ([docs/user/graphics.md](docs/user/graphics.md)).
- GTK3 renders at integer scales, so fractional scaling can blur text.
- Notifications need a notification daemon (mako, dunst, swaync) and stored tokens need a Secret
  Service provider. See [docs/user/keyring-and-notifications.md](docs/user/keyring-and-notifications.md).

## Build from source

Requirements: Rust 1.99, Node 22, pnpm 9, and on Linux `libwebkit2gtk-4.1-dev libgtk-3-dev
librsvg2-dev libayatana-appindicator3-dev libssl-dev libxdo-dev patchelf`.

```sh
pnpm install --frozen-lockfile
pnpm --filter @kelta/ui run build
cargo run -p kelta-desktop          # dev build (uses the Vite dev server: pnpm dev)
bash packaging/build.sh             # installers: deb + AppImage on Linux, dmg on macOS
```

Contributing: see [CONTRIBUTING.md](CONTRIBUTING.md). Security: see [SECURITY.md](SECURITY.md).

## License

MIT

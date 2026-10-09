# Writing a Kelta plugin

A plugin is a folder with a `kelta-plugin.toml` manifest. It can contribute tools, triggers, palette commands,
key bindings, session templates, settings, and screens (small web pages shown in a tab or pane). The reference
is [docs/PLUGINS.md](../PLUGINS.md); this guide walks through the common cases.

## 1. A tools-only plugin

Create `~/code/tools-pack/kelta-plugin.toml`:

```toml
id = "tools-pack"
name = "Tools pack"
version = "0.1.0"
kelta_api = "^0.1"
description = "k9s and btop as Kelta tools"
author = "You"
license = "MIT"
permissions = ["sessions.spawn", "exec:k9s", "exec:btop"]

[[contributes.tools]]
id = "k9s"
label = "Kubernetes"
kind = "pty"
command = "k9s"
args = ["--readonly"]
check = ["k9s", "version"]
install_hint = "brew install k9s"

[[contributes.tools]]
id = "btop"
label = "btop"
kind = "pty"
command = "btop"
```

Load it while developing: add the folder to `plugins.dev_paths` in your global `config.toml`, restart, then grant
the permissions Kelta lists in Settings > Plugins. Tools appear in the palette as `tools-pack/k9s`.

To install it like a user would: `kelta-ctl plugin install ~/code/tools-pack` (a directory, a git URL with an
optional tag, or a `.tar.gz`). Kelta shows the permissions in plain language and the manifest SHA-256 first.

## 2. A palette command

```toml
[[contributes.commands]]
id = "pods"
title = "Open pods"
when = "always"
do = [{ action = "open_tool", tool = "tools-pack/k9s" }]
```

Actions (`ActionDef`) are listed in PLUGINS.md section 3.1. Commands and triggers run only after the plugin is
activated (`activation = ["onStartup"]`, `onCommand:<id>`, `onProjectOpen`, ...).

## 3. Permissions

Ask for the least you need: `exec:<command>` per executable, `net:<host>` per host (https only),
`tickets.read`, `prs.write`, `events:<glob>`, `notify`, and so on (PLUGINS.md section 5). A new version that adds
permissions stays disabled for those capabilities until the user grants them again. Plugins cannot read secrets.

## 4. A screen

```toml
[[contributes.screens]]
id = "dashboard"
title = "Dashboard"
entry = "dist/index.html"
scope = "project"
placement = ["tab"]
```

The page is served from `kelta-plugin://<plugin-id>/dist/index.html` in a sandboxed iframe: no Tauri IPC, no
network, no storage. It talks to Kelta through `@kelta/plugin-sdk` (`packages/plugin-sdk`), which wraps the host
API of PLUGINS.md section 7; every call is checked against the manifest permissions.

```ts
import { connect } from "@kelta/plugin-sdk";
const kelta = await connect();
```

## 5. Settings

`[contributes.settings] schema = "settings.schema.json"` mounts a flat JSON Schema at `plugins.<id>`; Kelta
validates it and renders the form in Settings.

## 6. Checklist before sharing

- `id` is lowercase with dashes, not starting with `kelta`.
- `kelta_api` matches the host (`^0.1`).
- Every `exec:` and `net:` permission is used; nothing else is requested.
- `platforms` is set if the plugin only works on one OS.
- A tool has `check` and `install_hint` so users see why it is missing.

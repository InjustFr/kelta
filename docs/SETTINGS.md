# Kelta v0.1 — Settings (contract)

Settings are Rust structs in `kelta-proto::settings` (`#[derive(Serialize, Deserialize, JsonSchema, TS, Default)]`, `#[serde(default, deny_unknown_fields)]` except `plugins.*` and `extra_*` maps). `cargo run -p xtask -- codegen` writes `schema/settings.schema.json` and `schema/project.schema.json` (flattened, no `$ref`); CI fails on drift. User files are TOML; the first line may be `#:schema https://kelta.dev/schema/0.1/settings.schema.json` for Taplo completion.

## 1. Files, layers, precedence

| Layer (low → high) | `Layer` enum | Location | Scope |
|---|---|---|---|
| Defaults | `Default` | compiled (`Default` impls) | all keys |
| Plugin defaults | `Plugin` | each enabled plugin's settings schema defaults, under `plugins.<id>` | `plugins.<id>.*` only |
| Global | `Global` | `$XDG_CONFIG_HOME/kelta/config.toml` (macOS: `~/.config/kelta/config.toml`, honours `XDG_CONFIG_HOME`) | all keys |
| Project | `Project` | `<config>/kelta/projects/<project-id>.toml` | `[project]` table + any key whose schema has `x-kelta-scope` containing `project` |
| Repo-local | `Repo` | `<repo>/.kelta/config.toml` (any repo of the project; primary repo wins on conflict) | keys with `x-kelta-scope` containing `repo` (§4). **Exec-capable keys inert until trusted.** |
| Runtime | `Runtime` | env `KELTA__SECTION__KEY=value` (double underscore = nesting, value parsed as TOML), CLI `--set key=value`, `--safe-graphics`, `--config-dir <dir>` | all keys, not persisted |

Other files: `<config>/kelta/keybindings.toml` (optional; merged into `[keys]` at Global layer), `<config>/kelta/plugins/` is **not** used (plugins live in the data dir). Plugin grants and repo trust are in SQLite (never in TOML).

**Merge rules**
- Tables deep-merge; scalars: higher layer wins.
- Plain arrays: replaced by the higher layer.
- Keyed lists (`x-kelta-merge: "by_id"`): `tools`, `triggers`, `session_templates`, `editor.presets`, `commands` merge by `id`; a higher layer's entry with the same id replaces the lower one entirely; `{ id = "x", enabled = false }` disables an inherited entry.
- Maps (`accounts`, `claude.profiles`, `claude.prompt_templates`, `env` tables): merge by key; higher wins per key.
- `settings_effective(project_id)` = Default ⊕ Plugin ⊕ Global ⊕ Project ⊕ trusted Repo ⊕ Runtime, with the winning layer recorded per dotted path (`sources`).

**Annotations** (JSON Schema extensions, emitted by schemars attributes): `x-kelta-category` (General, Window, Keys, Terminal, Linux graphics, Projects, Accounts, Claude, Editors, Worktree & work, Reviews, Polling, Notifications, Tools, Triggers, Plugins, Performance), `x-kelta-order` (int), `x-kelta-scope` (`["global","project","repo"]` subset; default `["global","project"]`), `x-kelta-secret` (value is a SecretRef; UI shows "Set token…"), `x-kelta-restart` (requires restart), `x-kelta-exec` (runs commands → trust-gated at Repo layer), `x-kelta-merge` (`by_id`), `enumDescriptions`.

## 2. Global schema (every key, type, default)

Types: `str`, `bool`, `int`, `float`, `enum(a|b)`, `list<T>`, `map<K,V>`, `SecretRef` (string, §5), `Chord` (string like `ctrl+shift+k`, `cmd+opt+left`, `mod+t`; `mod` = Cmd on macOS / Ctrl+Shift on Linux), `Template` (string with `{placeholders}`, §6), `Duration` as integer with unit suffix in the key name.

### [app]
| Key | Type | Default | Notes |
|---|---|---|---|
| `theme` | enum(system\|dark\|light) | `system` | |
| `restore_mode` | enum(lazy\|eager\|none) | `lazy` | Dormant session respawn policy |
| `confirm_quit_with_running` | bool | `true` | confirm if Claude Working/NeedsInput |
| `log_level` | enum(error\|warn\|info\|debug\|trace) | `info` | restart |

### [window]
| `decorations` | enum(auto\|native\|none\|custom) | `auto` | restart; auto = none on Hyprland/Sway, native elsewhere |
| `close_behavior` | enum(auto\|background\|quit) | `auto` | auto = background on macOS, quit on Linux |
| `restore_geometry` | bool | `true` | |

### [keys]
| `prefix` | Chord \| "off" | `ctrl+shift+space` | app-prefix key |
| `prefix_timeout_ms` | int (200..5000) | `1000` | one-shot timer |
| `bindings` | map<ActionId, list<Chord>> | `{}` (overrides only; platform defaults per SPEC §4 live in the ActionId catalog, `ui/src/lib/gen/actions.ts`, so the schema is OS-independent) | effective = catalog default ⊕ override; `[]` unbinds |
| `prefix_bindings` | map<ActionId, str> | SPEC §4 | single key after prefix |
| `list_keys` | bool | `true` | single-key shortcuts in lists/boards |

### [terminal]
| `font_family` | str | `"JetBrains Mono, Menlo, DejaVu Sans Mono, monospace"` | system fonts only |
| `font_size` | float (6..40) | `13` | |
| `line_height` | float (1.0..2.0) | `1.15` | |
| `letter_spacing` | float | `0` | |
| `renderer` | enum(auto\|webgl\|dom) | `auto` | auto = webgl macOS, dom Linux |
| `cursor_style` | enum(block\|bar\|underline) | `block` | |
| `cursor_blink` | bool | `false` | focused pane only |
| `scrollback` | table {shell, claude, editor, tool, setup, custom: int} | `{3000, 3000, 500, 500, 1000, 3000}` | Rust model lines |
| `view_scrollback` | int (100..10000) | `1000` | xterm lines + snapshot history |
| `max_live_views` | int (1..12) | `4` | hidden xterm instances kept (LRU) |
| `memory_cap_mb` | int (32..2048) | `160` | total scrollback budget across sessions |
| `option_as_meta` | enum(none\|left\|right\|both) | `both` | macOS |
| `copy_on_select` | bool | `false` | CLIPBOARD; Linux PRIMARY is always set on select when `primary_selection` |
| `primary_selection` | bool | `true` | Linux: select → PRIMARY, middle-click paste |
| `confirm_multiline_paste` | bool | `true` | only when bracketed paste is off |
| `osc52` | enum(off\|write\|read-write) | `write` | |
| `bell` | enum(attention\|visual\|none) | `attention` | |
| `shift_enter` | map<SessionKindName, enum(passthrough\|esc-cr\|newline)> | `{claude = "esc-cr", default = "passthrough"}` | |
| `keyboard_protocol` | enum(legacy) | `legacy` | `kitty` added in v0.2 |
| `shell` | str | `""` | empty = `$SHELL` |
| `env` | map<str,str> | `{}` | added to every session |
| `minimum_contrast_ratio` | float | `1` | xterm option |

### [linux.graphics]  (x-kelta-scope global, x-kelta-restart; read by `platform::pre_init` with a minimal TOML parse)
| `profile` | enum(auto\|default\|safe) | `auto` | safe = all workarounds |
| `auto_nvidia` | bool | `true` | NVIDIA proprietary → dmabuf off + explicit sync off |
| `disable_dmabuf` | bool | `false` | `WEBKIT_DISABLE_DMABUF_RENDERER=1` |
| `disable_compositing` | bool | `false` | `WEBKIT_DISABLE_COMPOSITING_MODE=1` |
| `nvidia_disable_explicit_sync` | bool | `false` | `__NV_DISABLE_EXPLICIT_SYNC=1` |
| `gdk_backend` | enum(auto\|wayland\|x11) | `auto` | `GDK_BACKEND` |

### [polling]
| `focused_secs` | int (≥ min_secs) | `120` | |
| `background_secs` | int (0 = off) | `600` | window unfocused |
| `when_closed` | enum(off\|background) | `off` | background mode |
| `min_secs` | int (≥ 30) | `30` | Redmine floor 60 enforced in code |

### [notifications]
| `enabled` | bool | `true` | |
| `claude_needs_input` | bool | `true` | |
| `claude_done` | bool | `true` | |
| `review_requested` | bool | `true` | keeps review subscriptions polling in background |
| `ci_failed_mine` | bool | `true` | |
| `pr_approved` / `pr_changes_requested` | bool | `true` / `true` | |
| `bell_background` | bool | `false` | |
| `only_when_unfocused` | bool | `true` | or pane hidden |
| `quiet_hours` | str `"HH:MM-HH:MM"` \| "" | `""` | local time |

### [claude]  (scope global+project; `binary`, `extra_args`, `extra_hooks` are x-kelta-exec)
| `binary` | str | `"claude"` | resolved in login PATH |
| `min_version` | str (semver) | `"2.1.200"` | older → warning, hooks still attempted |
| `hook_transport` | enum(command\|http) | `command` | SessionStart always command |
| `mcp` | bool | `true` | Kelta MCP server for Claude sessions |
| `allowed_tools` | list<str> | `["mcp__kelta__*"]` | passed via `--allowedTools` |
| `append_system_prompt` | str | `""` | appended to generated `context.md` |
| `extra_args` | list<str> | `[]` | appended before the prompt |
| `extra_hooks` | JSON-compatible table | `{}` | merged into generated `--settings` hooks |
| `profiles` | map<str, ClaudeProfile> | `default = {model="opus", effort="high", permission_mode="acceptEdits"}`, `review = {model="opus", effort="high", permission_mode="plan"}`, `plan = {model="opus", effort="high", permission_mode="plan"}` | ClaudeProfile = {model: str, effort: enum(low\|medium\|high\|xhigh\|max), permission_mode: enum(default\|acceptEdits\|plan\|auto\|dontAsk\|bypassPermissions)} |
| `prompt_templates` | map<str, Template> | `ticket = "Work on {ticket.key}: {ticket.title}. The full ticket is in {run}/ticket.md. Read it, then propose a short plan before editing."`, `review = "Review PR {pr.url} ({pr.head} → {pr.base}). Focus on correctness, tests and risks. Do not edit files."`, `standalone = ""` | |
| `ide_bridge` | bool | `false` | v0.2; ignored in v0.1 |

### [editor]
| `default` | str (preset id) | `"nvim"` | |
| `follow_claude_edits` | enum(off\|reload\|open) | `reload` | |
| `review_args` | list<str> | `[]` | appended for review sessions, e.g. `["-c","DiffviewOpen origin/{base}...HEAD"]` |
| `presets` | list<EditorPreset> by id | §2.1 | x-kelta-merge by_id, x-kelta-exec |

EditorPreset: `{ id: str, label: str, command: str, args: list<Template>, open: enum(rpc|keys|command|none), open_keys: Template?, open_cmd: list<Template>?, external: bool = false, restore: enum(mksession|none) = none, enabled: bool = true }`. Placeholders: `{sock}`, `{path}` (initial path, default `.`), `{file}`, `{line}`, `{cwd}`, `{sid8}`.

#### 2.1 Default presets
```toml
[editor]
presets = [
  { id = "nvim",   command = "nvim",  args = ["--listen", "{sock}", "{path}"], open = "rpc", restore = "mksession" },
  { id = "vim",    command = "vim",   args = ["{path}"], open = "keys", open_keys = "<C-\\><C-N>:edit +{line} {file}<CR>" },
  { id = "helix",  command = "hx",    args = ["{path}"], open = "none" },
  { id = "emacs",  command = "emacs", args = ["-nw", "--eval", "(progn (setq server-name \"kelta-{sid8}\") (server-start))", "{path}"],
    open = "command", open_cmd = ["emacsclient", "-s", "kelta-{sid8}", "-n", "+{line}", "{file}"] },
  { id = "vscode", external = true, command = "code", args = ["{path}"], open = "command", open_cmd = ["code", "-g", "{file}:{line}"] },
  { id = "zed",    external = true, command = "zed",  args = ["{path}"], open = "command", open_cmd = ["zed", "{file}:{line}"] },
]
```
Multi-line inline tables are TOML 1.1 (parsed by `toml` 1.1.8 / `toml_edit` 0.25.17). External presets: the editor pane of a template becomes a shell; the editor is launched outside Kelta.

### [worktree]  (scope global+project+repo; `setup` x-kelta-exec)
| `root` | Template | `"~/.kelta-worktrees/{project}/{repo}/{key}-{slug}"` | outside the repo |
| `branch_template` | Template | `"{type}/{key}-{slug}"` | validated with `git check-ref-format --branch` |
| `type_map` | map<str,str> | `{bug="fix", story="feat", task="feat", feature="feat", default="chore"}` | issue type (lowercased) → `{type}` |
| `slug_max` | int | `40` | cut at word boundary, `[a-z0-9-]` |
| `include` | list<glob> | `[".env", ".env.*"]` | copied from main checkout; `.worktreeinclude` patterns appended |
| `setup` | list<str> | `[]` | each run in the visible setup pane, argv split shell-words, no shell unless `sh -c` written |
| `setup_blocking` | bool | `true` | Claude waits for setup exit 0 |
| `fetch_timeout_secs` | int | `20` | |
| `cleanup` | enum(ask\|auto\|never) | `ask` | on PR merged / ticket done |

### [work]
| `plan_preview` | bool | `true` | show StartWorkPlan sheet |
| `default_template` | str | `"claude+editor"` | |
| `review_template` | str | `"review"` | |
| `on_start` | {assign_me: bool = true, transition_to: TransitionTarget? = {category = "in_progress"}, comment: Template? = none} | | TransitionTarget = `{category = "todo"\|"in_progress"\|"in_review"\|"done"}` or `{name = "In Progress"}` |
| `on_pr` | {transition_to: TransitionTarget? = {category="in_review"}, comment: Template? = "PR: {pr.url}"} | | |
| `on_merge` | {transition_to: TransitionTarget? = {category="done"}, offer_cleanup: bool = true} | | |
| `pr.title_template` | Template | `"{ticket.key}: {ticket.title}"` | |
| `pr.body_template` | Template | `"{ticket.url}\n\n{closes}"` | `{closes}` = `Closes #n` for GitHub issues |
| `pr.draft` | bool | `false` | |

### [reviews]
| `include_team_requests` | bool | `true` | |
| `include_drafts` | bool | `false` | |
| `ticket_key_regex` | str | `"[A-Z][A-Z0-9]+-\\d+|#\\d+"` | linked tickets |
| `repos_allow` / `repos_deny` | list<glob> | `[]` / `[]` | |

### [web]
| `embed_default` | enum(auto\|iframe\|proxy\|external) | `auto` | |
| `keep_alive` | bool | `false` | keep hidden iframes |

### [performance]
| `hud` | bool | `false` | StatusBar memory |

### [accounts.<id>]  (map; scope global only; ids `[a-z0-9-]+`)
| Key | Type | Default | Applies to |
|---|---|---|---|
| `kind` | enum(jira\|redmine\|github\|gitlab) | — (required) | all |
| `base_url` | str (URL) | github: `https://api.github.com`; gitlab: `https://gitlab.com` | all (jira/redmine required) |
| `flavor` | enum(auto\|cloud\|dc) | `auto` | jira |
| `auth` | enum(basic\|bearer\|api_key\|token) | per kind: jira cloud `basic`, dc `bearer`, redmine `api_key`, github/gitlab `token` | |
| `email` / `user` | str | — | jira cloud email / basic user |
| `secret` | SecretRef | — (required) github default `gh-cli`, gitlab default `glab-cli` | x-kelta-secret |
| `text_format` | enum(textile\|markdown) | `textile` | redmine |
| `poll_secs` | int? | none | override |
| `web_url` | str? | derived | browser links (GHE/GitLab) |

### [[session_templates]]  (by_id; scope global+project+repo)
`{ id, label, layout: TemplateNode, enabled = true }` where `TemplateNode = { split = "row"|"column", ratios = [f32], children = [TemplateNode] } | { session = "claude"|"editor"|"shell"|"setup"|"tool:<id>", name?: str, profile?: str /*claude profile*/, command?: Template /*shell: run this then stay interactive*/ }`. Defaults:
```toml
[[session_templates]]
id = "claude+editor"
label = "Claude + editor"
layout = { split = "row", ratios = [0.5, 0.5], children = [ { session = "claude" }, { session = "editor" } ] }

[[session_templates]]
id = "review"
label = "Review"
layout = { split = "row", ratios = [0.5, 0.5], children = [
  { session = "claude", profile = "review" },
  { split = "column", ratios = [0.75, 0.25], children = [ { session = "editor" }, { session = "shell", command = "git diff --stat origin/{base}...HEAD" } ] },
] }

[[session_templates]]
id = "claude"
label = "Claude"
layout = { session = "claude" }

[[session_templates]]
id = "editor"
label = "Editor"
layout = { session = "editor" }

[[session_templates]]
id = "shell"
label = "Shell"
layout = { session = "shell" }
```

### [[tools]], [[triggers]], [[commands]]
Schemas in PLUGINS.md §2-3 (same schema in config and plugin manifests). by_id; scope global+project+repo; x-kelta-exec.

### [plugins]
| `dev_paths` | list<path> | `[]` | unpacked plugin dirs loaded in dev mode (global only; still require grants) |
| `disabled` | list<PluginId> | `[]` | |
| `<id>` | table | plugin schema defaults | validated against the plugin's settings schema |

## 3. Project file `projects/<id>.toml`

```toml
[project]
id = "shop"                  # must equal the file stem
name = "Shop"
color = "#e07a5f"
icon = "S"                   # 1-2 chars or sprite icon name
default_template = "claude+editor"

[[project.repos]]
id = "api"
path = "~/code/shop-api"
primary = true
remote = "origin"
base = "main"
code_host = { account = "github-work", repo = "acme/shop-api" }

[[project.repos]]
id = "web"
path = "~/code/shop-web"
remote = "origin"
base = "develop"
code_host = { account = "gitlab-acme", repo = "shop/web" }

[project.tracker]
account = "jira-acme"
views = [
  { id = "mine",   label = "My open", jql = "project = SHOP AND assignee = currentUser() AND statusCategory != Done ORDER BY updated DESC" },
  { id = "sprint", label = "Sprint",  jql = "project = SHOP AND sprint in openSprints()" },
]
columns = [
  { id = "todo",   label = "To do",       categories = ["todo"] },
  { id = "doing",  label = "In progress", categories = ["in_progress"] },
  { id = "review", label = "Review",      names = ["In Review", "Code Review"] },
  { id = "done",   label = "Done",        categories = ["done"] },
]
status_map = { start = { category = "in_progress" }, review = { name = "In Review" }, done = { category = "done" } }
repo_rules = [ { match = { component = "frontend" }, repo = "web" }, { match = { label = "api" }, repo = "api" } ]

# any project-scoped global key may be overridden:
[env]
DATABASE_URL = "postgres://localhost/shop_dev"
[worktree]
setup = ["pnpm install --frozen-lockfile"]
[claude.profiles.default]
model = "sonnet"
[[tools]]
id = "lazydocker"
enabled = true
```

Tracker view schema per kind (`views[]` entries; `id`, `label` always):
- jira: `jql` (required), `board_id?` (columns from board config when set).
- redmine: `project_id?`, `query_id?`, `assigned_to = "me"|"any"` (default `me`), `status = "open"|"closed"|"*"` (default `open`).
- github: `repo?` (`owner/name`), `search?` (search query), `project_v2? = { owner, number, status_field = "Status" }`.
- gitlab: `project?` (`group/sub/proj`), `scope = "assigned_to_me"|"all"`, `labels?`, `workflow_scope = "workflow"`.

`status_map` keys: `start`, `review`, `done` → TransitionTarget. Overrides `work.on_*` targets for this project.

## 4. Repo-local `.kelta/config.toml`

Allowed top-level keys: `tools`, `triggers`, `session_templates`, `commands`, `worktree.{include, setup, setup_blocking, branch_template}`, `env`, `claude.append_system_prompt`, `editor.review_args`. Anything else → validation error "not allowed in repo config". Keys marked `x-kelta-exec` (`tools`, `triggers`, `commands`, `worktree.setup`, `env`) are loaded but **inert** until `repo_trust(path, sha256)` matches the current content; a banner offers "Review & trust" (shows the file). Trust is per content hash; any edit requires re-trust. `kelta-ctl trust <repo>` does the same from the CLI after printing the file.

## 5. Secrets

Config holds only `SecretRef` strings, never tokens:

| SecretRef | Resolution |
|---|---|
| `keyring:<name>` | OS keyring via keyring-core: service `dev.kelta`, user `<name>`. macOS Keychain (`apple-native-keyring-store`, `keychain` feature); Linux Secret Service (`zbus-secret-service-keyring-store`). |
| `gh-cli` | `gh auth token --hostname <host of base_url>` (read-only reuse; never copied) |
| `glab-cli` | plaintext `config.yml` token for host (glab path search order), else `glab auth status --show-token --hostname <host>` |
| `command:<argv>` | argv split shell-words, exec without a shell, 5 s timeout, stdout trimmed (e.g. `command:pass show jira/acme`, `command:op read op://…`, `command:secret-tool lookup service kelta account jira`) |
| `env:<VAR>` | process env (login env included) |

- Resolution runs off the UI thread with a 5 s timeout, result cached in memory only (zeroized on drop), invalidated on settings change or 401. Never written to disk, never sent to the UI, never logged.
- `secret_set` writes only `keyring:` refs (Accounts wizard "Set token…"). On Linux with no `org.freedesktop.secrets` provider (common on Sway/Hyprland) or a locked collection, `secret_backends_status` reports it and the wizard proposes `command:`/`env:` and shows the `gnome-keyring-daemon --start --components=secrets` / KeePassXC snippet.
- macOS dev builds: Keychain prompts on each rebuild — CONTRIBUTING recommends `env:` refs in development.

## 6. Templates and placeholders

Simple `{path}` substitution (no expressions). Filters: `{x|slug}`, `{x|shell}` (POSIX single-quote), `{x|json}`, `{a|b}` = first non-empty of `a` or `b` (e.g. `{worktree|project.root}`). Unknown placeholder → validation error at load time.

Available: `project.{id,name,root}`, `repo.{id,path,name}`, `worktree`, `branch`, `base`, `key` (branch key), `slug`, `type`, `ticket.{key,title,url,file}`, `pr.{url,number,head,base,title}`, `session.{id,name,cwd}`, `sid8`, `run` (session runtime dir), `port` (free port allocated per tool instance), `config_dir`, `data_dir`, `home`, `user`.

## 7. Hot reload

- `notify-debouncer-full` (250 ms) watches the **directories** `<config>/kelta/`, `<config>/kelta/projects/`, and `<repo>/.kelta/` of every repo of open projects (editors rename-save).
- Pipeline: parse (toml) → per-layer schema validation (`jsonschema`) → merge → semantic validation (template placeholders, unknown preset/template ids, duplicate ids) → diff against last good → apply → `settings.changed{layers, paths, requires_restart}`.
- On error: keep last good config, toast `file:line:col message`, issue list in Settings.
- Applied live: theme, fonts, terminal options (new views; font changes re-fit visible views), keys, tools, triggers, templates, commands, accounts (providers recreated lazily), polling, notifications, plugin enable/disable. `x-kelta-restart` keys show a "Restart Kelta" toast. Claude/editor/worktree keys apply to **new** sessions only.
- UI writes: `toml_edit` (comments and order preserved, only the touched key changes), atomic temp-file + rename, content hash recorded to ignore the watcher echo.

## 8. Example global `config.toml`

```toml
#:schema https://kelta.dev/schema/0.1/settings.schema.json
[app]
theme = "system"

[window]
decorations = "auto"

[keys]
prefix = "ctrl+shift+space"
[keys.bindings]
"palette.open" = ["mod+k"]

[terminal]
font_family = "JetBrains Mono, monospace"
font_size = 13
renderer = "auto"
max_live_views = 4
shift_enter = { claude = "esc-cr", default = "passthrough" }

[linux.graphics]
auto_nvidia = true

[notifications]
quiet_hours = "22:00-08:00"

[claude]
binary = "claude"
[claude.profiles.default]
model = "opus"
effort = "high"
permission_mode = "acceptEdits"

[editor]
default = "nvim"
follow_claude_edits = "reload"

[accounts.jira-acme]
kind = "jira"
base_url = "https://acme.atlassian.net"
email = "me@acme.com"
secret = "keyring:jira-acme"

[accounts.redmine-client]
kind = "redmine"
base_url = "https://redmine.client.example"
secret = "command:pass show redmine/client"

[accounts.github-work]
kind = "github"
secret = "gh-cli"

[accounts.gitlab-acme]
kind = "gitlab"
base_url = "https://gitlab.acme.example"
secret = "glab-cli"

[[tools]]
id = "lazydocker"
label = "Docker"
kind = "pty"
command = "lazydocker"
cwd = "{repo.path|project.root}"
check = ["lazydocker", "--version"]
install_hint = "brew install lazydocker"
```

## 9. Settings UI

`Mod+,` opens the Settings pane: layer selector (Global / Project ▾ / Repo ▾ — Repo read-only until trusted), category list + search (key, title, description), schema-generated form (bool → toggle, enum → select with descriptions, int/float → number with min/max, str → text, Template → text with placeholder help, list<str> → chips, keyed lists → list editor with add/disable/override, map → key/value editor, SecretRef → source picker + "Set token…" + "Test"), per-key **source badge** (Default/Plugin/Global/Project/Repo/Runtime), "reset at this layer", restart marker, inline validation errors from Rust. Special sections: Accounts (wizard + Test connection + backend status), Projects (create from folder, repos, tracker views/columns/status map), Session templates (layout mini-editor), Tools, Triggers (list, origin, log, test with sample payload), Plugins (enable, grants, settings), Keys (recorder + conflict checker + compositor snippets), Linux graphics, Performance (live memory per component and per session, live views, keep-alive screens), Diagnostics. Every section has **Edit TOML** (raw editor with validation; or "Open in editor" pane).

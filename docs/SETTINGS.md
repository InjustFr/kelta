# Kelta — Settings (contract)

Settings are Rust structs in `kelta-proto::settings` (`#[derive(Serialize, Deserialize, JsonSchema, TS, Default)]`, `#[serde(default, deny_unknown_fields)]` except `plugins.*`). `cargo run -p xtask -- codegen` writes `schema/settings.schema.json` and `schema/project.schema.json` (types inlined; only the recursive `ActionDef`, `Matcher` and `TemplateNode` stay `$defs`); CI fails on drift. User files are TOML; the first line may be `#:schema https://kelta.dev/schema/0.1/settings.schema.json` for Taplo completion (`0.1` is the schema's own version, not the app's).

## 1. Files, layers, precedence

| Layer (low → high) | `Layer` enum | Location | Scope |
|---|---|---|---|
| Defaults | `Default` | compiled (`Default` impls) | all keys |
| Plugin defaults | `Plugin` | each enabled plugin's settings schema defaults, under `plugins.<id>` | `plugins.<id>.*` only |
| Global | `Global` | `$XDG_CONFIG_HOME/kelta/config.toml` (macOS: `~/.config/kelta/config.toml`, honours `XDG_CONFIG_HOME`) | all keys |
| Project | `Project` | `<config>/kelta/projects/<project-id>.toml` | `[project]` table + any key whose schema has `x-kelta-scope` containing `project` |
| Repo-local | `Repo` | `<repo>/.kelta/config.toml` (any repo of the project; primary repo wins on conflict) | keys with `x-kelta-scope` containing `repo` (§4). **Exec-capable keys inert until trusted.** |
| Runtime | `Runtime` | env `KELTA__SECTION__KEY=value` (double underscore = nesting, value parsed as TOML), CLI `--set key=value`, `--safe-graphics`, `--config-dir <dir>` | all keys, not persisted |

Other files: `<config>/kelta/keybindings.toml` (optional; holds the `[keys]` table, bare (`prefix = …`, `[bindings]`) or under `[keys]`; merged into `[keys]` at Global layer; Settings → Keys writes its changes there; like `config.toml`, a symlinked one is written through to its target, the link kept, while project and repo files never follow links), `<config>/kelta/plugins/` is **not** used (plugins live in the data dir). Plugin grants and repo trust are in SQLite (never in TOML).

**Merge rules**
- Tables deep-merge; scalars: higher layer wins.
- Plain arrays: replaced by the higher layer.
- Keyed lists (`x-kelta-merge: "by_id"`): `tools`, `triggers`, `session_templates`, `editor.presets`, `commands`, `sources`, `workflows` merge by `id`; a higher layer's entry with the same id replaces the lower one entirely; `{ id = "x", enabled = false }` disables an inherited entry.
- Maps (`accounts`, `claude.profiles`, `claude.prompt_templates`, `env` tables): merge by key; higher wins per key.
- `settings_effective(project_id)` = Default ⊕ Plugin ⊕ Global ⊕ Project ⊕ trusted Repo ⊕ Runtime, with the winning layer recorded per dotted path (`sources`).

**Annotations** (JSON Schema extensions, emitted by schemars attributes): `x-kelta-category` (General, Window, Keys, Terminal, Linux graphics, Projects, Accounts, Claude, Editors, Worktree & work, Reviews, Polling, Notifications, Tools, Triggers, Plugins, Performance), `x-kelta-order` (int), `x-kelta-scope` (`["global","project","repo"]` subset; default `["global","project"]`), `x-kelta-secret` (value is a SecretRef; UI shows "Set token…"), `x-kelta-restart` (requires restart), `x-kelta-exec` (runs commands or steers Claude → trust-gated at Repo layer), `x-kelta-merge` (`by_id`), `enumDescriptions`.

## 2. Global schema (every key, type, default)

Types: `str`, `bool`, `int`, `float`, `enum(a|b)`, `list<T>`, `map<K,V>`, `SecretRef` (string, §5), `Chord` (string like `ctrl+shift+k`, `cmd+opt+left`, `mod+t`; `mod` = Cmd on macOS / Ctrl+Shift on Linux), `Template` (string with `{placeholders}`, §6), `Duration` as integer with unit suffix in the key name.

### [app]
| Key | Type | Default | Notes |
|---|---|---|---|
| `theme` | enum(system\|dark\|light) | `system` | |
| `language` | enum(system\|en\|fr\|de) | `system` | scope global; interface language (Settings → General); `system` follows the OS language, English when it is none of the three. Applies at once, no restart. Ticket and PR content, terminal output and config keys stay as they are |
| `restore_mode` | enum(lazy\|eager\|none) | `lazy` | Dormant session respawn policy |
| `confirm_quit_with_running` | bool | `true` | ask before quitting while Claude is working or waiting for you |
| `quit_sessions` | enum(ask\|keep\|stop) | `ask` | scope global; quitting (Cmd+Q / menu Quit / Dock Quit, a window close that quits) while keltad would keep sessions running: `ask` shows the quit dialog (session count, busy sessions, "Don't ask again" stores the answer here), `keep` leaves them running for the next start, `stop` kills every session (as `session_kill`) and ends keltad; they restore per `restore_mode`, as without keltad. No dialog with `session_host = "inprocess"`, `restore_mode = "none"` (nothing survives), in background mode, or on the updater's restart (always keeps) |
| `log_level` | enum(error\|warn\|info\|debug\|trace) | `info` | restart |
| `dark_theme` | str | `bezel-dark` | scope global; theme id used in dark mode (built-in or `[themes.<id>]` with `base = "dark"`); an unknown id or a theme of the other base falls back to `bezel-dark` |
| `light_theme` | str | `bezel-light` | scope global; same for light mode, fallback `bezel-light` |
| `updates` | bool | `true` | scope global; check for updates after start and on focus (at most every 6 h); `false` stops automatic checks, Settings → General "Check now" still works |

### [window]  (scope global)
| Key | Type | Default | Notes |
|---|---|---|---|
| `decorations` | enum(auto\|native\|none\|custom) | `auto` | restart; auto = none on Hyprland/Sway, native elsewhere |
| `close_behavior` | enum(auto\|background\|quit) | `auto` | auto = background on macOS, quit on Linux |
| `restore_geometry` | bool | `true` | |

### [keys]
| Key | Type | Default | Notes |
|---|---|---|---|
| `prefix` | Chord \| "off" | `ctrl+shift+space` | app-prefix key |
| `prefix_timeout_ms` | int (200..5000) | `1000` | one-shot timer |
| `bindings` | map<ActionId, list<Chord>> | `{}` (overrides only; platform defaults per SPEC §4 live in the ActionId catalog, `ui/src/lib/gen/actions.ts`, so the schema is OS-independent) | effective = catalog default ⊕ override; `[]` unbinds |
| `prefix_bindings` | map<ActionId, str> | SPEC §4 | single key after prefix |
| `list_keys` | bool | `true` | single-key shortcuts of lists and boards. `false`: plain printable keys (letters, digits, `?`, `/`, with or without Shift, no Ctrl/Cmd/Alt) no longer reach the panes, `j`/`k` included; arrows, `Enter`, `Escape`, `Space`, `Home`/`End`, `Tab` and every Ctrl/Cmd/Alt chord still work, as do text fields, terminals, dialogs, menus and a Kelta action you bound to a plain key. Applies at once |
| `hints` | bool | `true` | the key hint line under Tickets, Reviews and Now (five keys and `?`, DESIGN §6.8). `false` hides it; `?` still opens the full key list. Applies at once |

Every action id, label and default chord per platform is in the ActionId catalog (`crates/kelta-proto/src/actions.rs`, generated to `ui/src/lib/gen/actions.ts`); SPEC §4 lists them. Settings → Keys records a chord, flags conflicts and unbinds. `window.toggle` has no in-app chord: bind `kelta-ctl toggle` in your compositor. Work-flow actions (FLOW §7), rebindable through `bindings` / `prefix_bindings` like any ActionId:

| ActionId | macOS | Linux | Prefix | Behaviour |
|---|---|---|---|---|
| `toast.run_last` (palette "Run last toast action") | `cmd+shift+o` (⇧⌘O) | `ctrl+shift+a` | `o` | runs the primary action of the most recent toast still shown (e.g. "Opened PR #13" → Open); the chord is printed on that toast's button. Linux is not `Ctrl+Shift+O`: that is `pane.split_down`. |
| `work.ship` (palette "Work: Ship / Push") | none | none | none | Ship dialog for the focused tab's work item (push + PR); with a PR already open it is a plain push. |
| `work.finish` (palette "Work: Finish") | none | none | none | Finish dialog for the focused tab's work item, prefilled when its PR is merged or closed. |
| `work.finish_merged` (palette "Finish all merged") | none | none | none | one dialog listing merged items with clean worktrees (dirty ones are listed as skipped); the only bulk destructive action. |

Every sheet and dialog follows the same keys, which are not rebindable: `Tab` / `⇧Tab` move and stay inside, `Space` toggles the focused switch, `⌘↵` (`Ctrl+↵`) runs the primary button, `Esc` cancels; focus starts on the main input.

### [terminal]
| Key | Type | Default | Notes |
|---|---|---|---|
| `font_family` | str | `"JetBrains Mono, Menlo, DejaVu Sans Mono, monospace"` | system fonts only |
| `font_size` | float (6..40) | `13` | |
| `line_height` | float (1.0..2.0) | `1.15` | |
| `letter_spacing` | float | `0` | |
| `renderer` | enum(auto\|webgl\|dom) | `auto` | auto = webgl macOS, dom Linux |
| `cursor_style` | enum(block\|bar\|underline) | `block` | |
| `cursor_blink` | bool | `false` | focused pane only |
| `scrollback` | table {shell, claude, editor, tool, setup, custom: int} | `{3000, 3000, 500, 500, 1000, 3000}` | Rust model lines |
| `view_scrollback` | int (100..10000) | `1000` | xterm lines + snapshot history |
| `max_live_views` | int (1..12) | `2` | hidden xterm instances kept (LRU) |
| `memory_cap_mb` | int (32..2048) | `160` | total scrollback budget across sessions |
| `history_log` | bool | `true` | append scrolled-off lines (plain text) to `<data>/history/` per session: search + restore (ARCH §9.6) |
| `history_log_mb` | int (1..1024) | `16` | per-session log cap (two rotated halves) |
| `history_log_total_mb` | int (16..65536) | `512` | cap of all session logs; oldest files deleted first |
| `option_as_meta` | enum(none\|left\|right\|both) | `both` | macOS |
| `copy_on_select` | bool | `false` | CLIPBOARD; Linux PRIMARY is always set on select when `primary_selection` |
| `primary_selection` | bool | `true` | Linux: select → PRIMARY, middle-click paste |
| `confirm_multiline_paste` | bool | `true` | only when bracketed paste is off |
| `osc52` | enum(off\|write\|read-write) | `write` | |
| `bell` | enum(attention\|visual\|none) | `attention` | |
| `shift_enter` | map<SessionKindName, enum(passthrough\|esc-cr\|newline)> | `{claude = "esc-cr", default = "passthrough"}` | |
| `keyboard_protocol` | enum(kitty\|legacy) | `kitty` | kitty keyboard protocol for programs that enable it (`CSI > u`); `legacy` = model ignores it. Applied live (switching to `legacy` clears the mode) |
| `shell` | str | `""` | empty = `$SHELL` |
| `env` | map<str,str> | `{}` | added to every session |
| `minimum_contrast_ratio` | float | `1` | xterm option |
| `session_host` | enum(inprocess\|daemon) | `daemon` | restart; global. `daemon` = PTYs live in `keltad`: sessions survive quit and re-attach on the next start (falls back to `inprocess` when keltad cannot start) |

### [linux.graphics]  (x-kelta-scope global, x-kelta-restart; read by `platform::pre_init` with a minimal TOML parse)
| Key | Type | Default | Notes |
|---|---|---|---|
| `profile` | enum(auto\|default\|safe) | `auto` | safe = all workarounds; auto also turns compositing off without a GPU render node (`/dev/dri/renderD*`) |
| `auto_nvidia` | bool | `true` | NVIDIA proprietary → dmabuf off + explicit sync off |
| `disable_dmabuf` | bool | `false` | `WEBKIT_DISABLE_DMABUF_RENDERER=1` |
| `disable_compositing` | bool | `false` | `WEBKIT_DISABLE_COMPOSITING_MODE=1` |
| `nvidia_disable_explicit_sync` | bool | `false` | `__NV_DISABLE_EXPLICIT_SYNC=1` |
| `gdk_backend` | enum(auto\|wayland\|x11) | `auto` | `GDK_BACKEND` |

### [polling]
| Key | Type | Default | Notes |
|---|---|---|---|
| `focused_secs` | int (≥ min_secs) | `120` | |
| `background_secs` | int (0 = off) | `600` | window unfocused |
| `when_closed` | enum(off\|background) | `off` | background mode |
| `min_secs` | int (≥ 30) | `30` | Redmine floor 60 enforced in code |

### [notifications]
| Key | Type | Default | Notes |
|---|---|---|---|
| `enabled` | bool | `true` | |
| `claude_needs_input` | bool | `true` | |
| `claude_done` | bool | `true` | |
| `review_requested` | bool | `true` | keeps review subscriptions polling in background |
| `ci_failed_mine` | bool | `true` | my PR's head rollup turns failed or errored (cancelled) |
| `pr_approved` / `pr_changes_requested` | bool | `true` / `true` | |
| `bell_background` | bool | `false` | |
| `only_when_unfocused` | bool | `true` | or pane hidden |
| `quiet_hours` | str `"HH:MM-HH:MM"` \| "" | `""` | local time |

### [claude]  (scope global+project; `append_system_prompt` also repo; `binary`, `extra_args`, `extra_hooks`, `append_system_prompt`, `terminal_command` are x-kelta-exec; `mcp_offer` and `hooks_offer` are global only)
| Key | Type | Default | Notes |
|---|---|---|---|
| `binary` | str | `"claude"` | resolved in login PATH |
| `min_version` | str (semver) | `"2.1.200"` | older → warning, hooks still attempted |
| `hook_transport` | enum(command\|http) | `command` | SessionStart always command |
| `mcp` | bool | `true` | Kelta MCP server for Claude sessions |
| `mcp_board_writes` | bool | `true` | board-wide MCP writes (`transition_ticket_by_key`, `comment_ticket_by_key`, `update_ticket_body`, `close_ticket`); off = read-only board tools. Also needs `mcp` |
| `mcp_offer` | bool | `true` | at startup, offer to register Kelta's MCP in Claude Code (user scope) when it is found and not registered; the toast's Never sets it to `false`. Global |
| `hooks_offer` | bool | `true` | at startup, offer to register Kelta's hook relay (`kelta-ctl hook --outside`) in Claude Code's user settings when it is found and not registered, so Claude sessions Kelta did not start show in Now; the toast's Never sets it to `false`. Global. Settings → Claude has Install / Remove buttons for both registrations |
| `allowed_tools` | list<str> | `["mcp__kelta__*"]` | passed via `--allowedTools` |
| `append_system_prompt` | str (x-kelta-exec) | `""` | appended to generated `context.md` |
| `extra_args` | list<str> | `[]` | appended before the prompt |
| `extra_hooks` | JSON-compatible table | `{}` | merged into generated `--settings` hooks |
| `profiles` | map<str, ClaudeProfile> | `default = {model="opus", effort="high"}`, `review = {model="opus", effort="high", permission_mode="plan"}`, `plan = {model="opus", effort="high", permission_mode="plan"}` | ClaudeProfile = {model: str, effort: enum(low\|medium\|high\|xhigh\|max), permission_mode?: enum(default\|acceptEdits\|plan\|auto\|dontAsk\|bypassPermissions)}. `permission_mode` is forced with `--permission-mode`; unset, a session starts in the last mode a Claude session switched to (kept in the state database, shown in Settings → Claude; `dontAsk` and `bypassPermissions` are never remembered), else in Claude's own `permissions.defaultMode`. A resumed session re-resolves from the profile it was launched with: a mode that profile forces (as edited now) is kept, else the current remembered mode (sessions saved before this rule keep a launch-time `plan`) |
| `prompt_templates` | map<str, Template> | `ticket = "Work on {ticket.key}: {ticket.title}. The full ticket is in {run}/ticket.md. Read it, then propose a short plan before editing."`, `review = "Review PR {pr.url} ({pr.head} → {pr.base}). Focus on correctness, tests and risks. Do not edit files."`, `standalone = "{task}"`, `workflow = "Run the saved workflow \"{workflow}\" with the JSON in {args} as its args. The ticket is {ticket.key}: {ticket.title}; its full text is in {file}."`, `feedback = "The review of {pr.url} left feedback, collected in {file}. Address each item (or say why not), run the tests, and commit the fixes. Do not push."`, `checks = "Checks failed on {pr.url}. Their names, links and log tails are in {file}. Reproduce each failure locally, fix it, run the tests, and commit the fixes. Do not push."`, `conflicts = "The rebase of {branch} onto {onto} stopped at commit {step} of {total} with conflicts, listed in {file}. Resolve them, `git add` the files, then run `GIT_EDITOR=true git rebase --continue` until the rebase is done. Do not push."` | `feedback` is the editable prompt of the Fix with Claude sheet, `checks` the prompt of Send failures to Claude (`Enter` on a Checks failed row) and of the sheet when the feedback is failed checks only, `conflicts` the one of Ask Claude to resolve. Both are rendered by `work_send` with the work item's placeholders plus `{file}` (absolute path of the brief written to the private run dir: `feedback.md` / `conflicts.md`), `{pr.url}`, and during a rebase `{onto}`, `{step}`, `{total}`. `standalone`'s `{task}` is the task of a New work item or `kelta-ctl start --task`. `workflow` is the prompt of Run workflow on a ticket (#233): `{workflow}` (its name), `{ticket.key}`, `{ticket.title}`, `{ticket.url}`, `{file}` (`ticket.md`) and `{args}` (`args.json`, the workflow's args), both in the session's private run dir. |
| `ide_bridge` | bool | `false` | Claude sessions see Kelta as their IDE (loopback WebSocket + `~/.claude/ide/<port>.lock`, `openFile`/`openDiff` in the editor pane); ARCHITECTURE §8.5 |
| `budget_usd` | float? | none | Claude spend per work item (USD) past which its cost chip turns red; set it per project |
| `max_live` | int | `4` | live Claude processes start work runs at once; past it new items wait `Queued` with their worktree ready and start, oldest first, when a Claude exits (0 = no cap). Global |
| `queue_hold_pct` | float? | none | hold the queue while the 5h rate-limit usage is at or above this percentage. Global |
| `auto_park_after_mins` | int | `45` | park a work item's Claude (and its nvim, unless it has unsaved buffers) once it has been done and seen with no input for this many minutes; showing the item resumes the same conversation. Never a working or waiting Claude (0 = off) |
| `terminal_command` | list<str> | `[]` | opens a template's Claude when its `claude_host` is `terminal`; placeholders `{command}` (the script Kelta writes: it exports the session's hook and MCP variables and runs `claude`), `{cwd}`, `{shell}`. Empty = `open -na Terminal {command}` on macOS, `x-terminal-emulator -e {command}` on Linux. Example: `["ghostty", "--working-directory={cwd}", "-e", "{shell}", "-ic", "{command}"]` |

### [editor]
| Key | Type | Default | Notes |
|---|---|---|---|
| `default` | str (preset id) | `"nvim"` | |
| `follow_claude_edits` | enum(off\|reload\|open) | `reload` | |
| `review_args` | list<str> (x-kelta-exec) | `[]` | the diff editor's arguments: appended for review sessions and used by `work_diff` (Review diff, `d`) for own work items, e.g. `["-c","DiffviewOpen {range}"]`. `{range}` renders `<remote>/<base>` for own items (merge base to working tree, uncommitted work included) and `<remote>/<base>...HEAD` for review checkouts. Empty: Review diff runs `git diff $(git merge-base <remote>/<base> HEAD)` in a shell |
| `presets` | list<EditorPreset> by id | §2.1 | x-kelta-merge by_id, x-kelta-exec |

EditorPreset: `{ id: str, label: str, command: str, args: list<Template>, open: enum(rpc|keys|command|none), open_keys: Template?, open_cmd: list<Template>?, external: bool = false, restore: enum(mksession|none) = none, enabled: bool = true }`. Placeholders: `{sock}`, `{path}` (initial path, default `.`), `{file}`, `{line}`, `{cwd}`, `{sid8}`.

#### 2.1 Default presets
```toml
[editor]
presets = [
  { id = "nvim",   label = "Neovim", command = "nvim",  args = ["--listen", "{sock}", "{path}"], open = "rpc", restore = "mksession" },
  { id = "vim",    label = "Vim",    command = "vim",   args = ["{path}"], open = "keys", open_keys = "<C-\\><C-N>:edit +{line} {file}<CR>" },
  { id = "helix",  label = "Helix",  command = "hx",    args = ["{path}"], open = "none" },
  { id = "emacs",  label = "Emacs",  command = "emacs", args = ["-nw", "--eval", "(progn (setq server-name \"kelta-{sid8}\") (server-start))", "{path}"],
    open = "command", open_cmd = ["emacsclient", "-s", "kelta-{sid8}", "-n", "+{line}", "{file}"] },
  { id = "vscode", label = "VS Code", external = true, command = "code", args = ["{path}"], open = "command", open_cmd = ["code", "-g", "{file}:{line}"] },
  { id = "zed",    label = "Zed",    external = true, command = "zed",  args = ["{path}"], open = "command", open_cmd = ["zed", "{file}:{line}"] },
]
```
Multi-line inline tables are TOML 1.1 (parsed by `toml` 1.1.8 / `toml_edit` 0.25.17). External presets: the editor pane of a template becomes a shell; the editor is launched outside Kelta.

### [worktree]  (scope global+project+repo; `setup` and `teardown` x-kelta-exec; a repo file may set only `include`, `setup`, `setup_blocking` and `branch_template`)
| Key | Type | Default | Notes |
|---|---|---|---|
| `root` | Template | `"~/.kelta-worktrees/{project}/{repo}/{key}-{slug}"` | outside the repo |
| `branch_template` | Template | `"{type}/{key}-{slug}"` | validated with `git check-ref-format --branch` |
| `type_map` | map<str,str> | `{bug="fix", story="feat", task="feat", feature="feat", default="chore"}` | issue type (lowercased) → `{type}` |
| `slug_max` | int | `40` | cut at word boundary, `[a-z0-9-]` |
| `include` | list<glob> | `[".env", ".env.*"]` | copied from main checkout; `.worktreeinclude` patterns appended |
| `setup` | list<str> | `[]` | each run in the visible setup pane, argv split shell-words, no shell unless `sh -c` written |
| `setup_blocking` | bool | `true` | Claude waits for setup exit 0 |
| `fetch_timeout_secs` | int | `20` | |
| `cleanup` | enum(ask\|auto\|never) | `ask` | on PR merged / ticket done |
| `env_template` | str | `""` | worktree path ending in `.kelta` (e.g. `.env.kelta`, read from the worktree, else the main checkout), rendered at start work (`include_files` step, before the copies) to the same path without `.kelta`. Placeholders `{port}`, `{port.1}`…`{port.9}`; other braces stay verbatim. An existing target that differs is kept, with a warning |
| `teardown` | str (x-kelta-exec) | `""` | run on Finish in a pane below, in the worktree, with the item's env (e.g. `docker compose down`); failure stops the Finish unless forced |

### [ports]
| Key | Type | Default | Notes |
|---|---|---|---|
| `range` | str | `"20000-29999"` | `first-last`. Each new work item takes the next free block of 10 (no other item's block, first port bindable), freed on Finish; `""` = no ports. Every session, setup/teardown and tool process of the item gets `KELTA_PORT`, `KELTA_PORT_1..9` and `COMPOSE_PROJECT_NAME=kelta-<key-slug>` (ticket key, else branch). The WorkItem header shows the block |

### [work]
| Key | Type | Default | Notes |
|---|---|---|---|
| `plan_preview` | bool | `true` | show StartWorkPlan sheet |
| `finish_first` | bool | `true` | starting a new ticket while other work is unfinished lists that work first (Start anyway or go to it); the sheet's "Don't ask again" turns it off |
| `default_template` | str | `"claude+editor"` | the session template Start work uses; a project's `[project] default_template` overrides it for that project |
| `review_template` | str | `"review"` | |
| `scratch_branch_template` | Template | `"wip/{slug}"` | branch of a New work item (`⇧⌘N`, `kelta-ctl start --task`); `{slug}` = slug of the task's first line (`worktree.slug_max`). Validated like `git check-ref-format`; an existing branch or item is refused, never adopted |
| `on_start` | {assign_me: bool = true, transition_to: TransitionTarget? = {category = "in_progress"}, comment: Template? = none} | | TransitionTarget = `{category = "todo"\|"in_progress"\|"in_review"\|"done"}` or `{name = "In Progress"}` |
| `on_pr` | {transition_to: TransitionTarget? = {category="in_review"}, comment: Template? = "PR: {pr.url}"} | | |
| `on_merge` | {transition_to: TransitionTarget? = {category="done"}, offer_cleanup: bool = true} | | applied on `pr.merged` (also for merges while Kelta was closed, found on the next launch) only when unambiguous: a `name` target, or a single `category` match, and no required fields. Otherwise the ticket stays as is, the item reads "Merged: choose Done status" and the Finish dialog asks. The project's `status_map.done` wins over it. |
| `pr.title_template` | Template | `"{ticket.key}: {ticket.title}"` | prefills the Ship dialog title for ticket items; other items use the last commit subject |
| `pr.merge_method` | `"squash"` \| `"merge"` \| `"rebase"` | `"squash"` | default of the Merge when ready picker (`M`); GitLab takes squash or merge |
| `pr.body_template` | Template | `"{ticket.url}\n\n{closes}"` | `{closes}` = `Closes #n` for GitHub issues |
| `pr.draft` | bool | `false` | default of the Ship dialog's Draft switch (and of MCP `create_pr` without `draft`) |
| `return_brief_after_mins` | int | `20` | refocusing a work item after this long away shows the return strip (`next:` note, Claude's last message, `+N/−M since you reviewed`); never while Claude is working; `0` = off |

### [tickets]
| Key | Type | Default | Notes |
|---|---|---|---|
| `wip_limit` | int | `3` | soft WIP limit: the tickets pane's Doing group header warns above it; blocks nothing |

### [reviews]
| Key | Type | Default | Notes |
|---|---|---|---|
| `include_team_requests` | bool | `true` | |
| `include_drafts` | bool | `false` | review requests only: my own (authored) PRs always include drafts |
| `ticket_key_regex` | str | `"[A-Z][A-Z0-9]+-\\d+\|#\\d+"` | linked tickets; applied at startup and on reload (an invalid pattern keeps the previous one) |
| `repos_allow` / `repos_deny` | list<glob> | `[]` / `[]` | |
| `ignore_globs` | list<glob> | `["*.lock", "package-lock.json", "pnpm-lock.yaml", "go.sum", "*.min.js", "*.snap"]` | counted as generated (not review lines) in the Ready for review chip, with `linguist-generated` / `-diff` attributes |
| `sla_hours` | int | `24` | My PRs: the `waiting on @x · Nh` chip turns amber past this many hours |
| `nudge_template` | str | `"{reviewers} friendly ping: this is waiting on your review."` | My PRs: the comment of `n` → Comment ping; `{reviewers}` = `@anna @bob` |

### [web]
| Key | Type | Default | Notes |
|---|---|---|---|
| `embed_default` | enum(auto\|iframe\|proxy\|external) | `auto` | |
| `keep_alive` | bool | `false` | keep hidden iframes |

### [performance]
| Key | Type | Default | Notes |
|---|---|---|---|
| `hud` | bool | `false` | StatusBar memory |

### [env]  (scope global+project+repo; x-kelta-exec; category Projects)
| Key | Type | Default | Notes |
|---|---|---|---|
| `env` (a top-level table) | map<str,str> | `{}` | environment added to every session of the project, after `terminal.env` (it wins on the same name); merges by key, the higher layer wins per key |

### [accounts.<id>]  (map; scope global only; ids `[a-z0-9-]+`)
| Key | Type | Default | Applies to |
|---|---|---|---|
| `kind` | enum(jira\|redmine\|github\|gitlab\|linear\|bitbucket\|gitea\|plugin_tracker\|plugin_codehost) | — (required) | all |
| `plugin` | str? | — (required for `plugin_*`) | plugin id whose `[provider]` serves the account (PLUGINS §9); the whole account table and its resolved secret go to the plugin with each call |
| `base_url` | str (URL) | github: `https://api.github.com`; gitlab: `https://gitlab.com`; linear: `https://api.linear.app`; bitbucket: `https://api.bitbucket.org/2.0` | all (jira/redmine/gitea required; gitea accepts a trailing `/api/v1`) |
| `flavor` | enum(auto\|cloud\|dc) | `auto` | jira |
| `auth` | enum(basic\|bearer\|api_key\|token\|oauth) | `oauth` = set by browser sign-in (github/gitlab: Bearer, refreshed before expiry); per kind: jira cloud `basic`, dc `bearer`, redmine `api_key`, github/gitlab/gitea `token`, bitbucket `basic` (account `email` + Atlassian API token; `bearer` for repository/workspace access tokens), linear raw API key in `Authorization` (`bearer` for OAuth tokens) | |
| `email` / `user` | str | — | jira cloud and bitbucket email / basic user |
| `secret` | SecretRef | — (required; optional for `plugin_*`) github default `gh-cli`, gitlab default `glab-cli`, linear a personal API key (`keyring:`, `env:` or `command:`) | x-kelta-secret |
| `text_format` | enum(textile\|markdown) | `textile` | redmine |
| `poll_secs` | int? | none | override |
| `web_url` | str? | derived | browser links (GHE/GitLab/Gitea); bitbucket: only used to match git remotes (default `https://bitbucket.org`) |

### [oauth]  (scope global only; category Accounts)
| Key | Type | Default | Notes |
|---|---|---|---|
| `client_ids` | map<str,str> | `{}` | web host (`github.com`, `gitlab.com`, `gitlab.acme.example`) → OAuth App client id / GitLab application id. Public values. A host without one offers no "Sign in with GitHub / GitLab" (docs/user/oauth.md). |

### [themes.<id>]  (map; scope global only; category General)
A theme is a named, **partial** set of colour overrides on top of a Bezel base: unset tokens keep the Bezel value. The UI sets the overrides as inline custom properties on `<html>` (`ui/src/lib/theme`), tokens.css stays the base. Built-ins: `bezel-dark`, `bezel-light`; a user theme with a built-in id wins. Every colour is `#rrggbb` (schema `pattern`; a bad value rejects the whole file like any schema error, and the UI also ignores a non-hex value per token and lowercases the rest).

| Key | Type | Notes |
|---|---|---|
| `name` | str | required, display name |
| `base` | enum(dark\|light) | required; the slot of the other mode ignores the theme |
| `ui.<token>` | `#rrggbb` | `bezel`, `bezel_raised`, `well`, `bg_float`, `fg`, `fg_muted`, `fg_subtle`, `fg_chrome`, `border`, `border_strong`, `danger`, `warn`, `ok`, `info` → `--k-<token>` (`_` → `-`). Not themable: lamps, hover/active/overlay/shadow, `*_fg` |
| `ui.accent` | `#rrggbb` | never writes `--k-accent`: the `--k-project` hue when the active project has no colour (red and grey are still dropped, DESIGN §9) |
| `terminal.background` / `terminal.foreground` | `#rrggbb` | fallback `ui.well` / `ui.fg`, then the Bezel palette; also sets `--k-term-bg` / `--k-term-fg` |
| `terminal.cursor` / `terminal.selection` | `#rrggbb` | fallback the Bezel palette; also sets `--k-term-cursor` / `--k-term-selection` |
| `terminal.ansi` | list<`#rrggbb`> | exactly 16 (black..white, then the brights) or omitted |

A build without themes rejects a config that has `themes` (`deny_unknown_fields`). First paint shows Bezel until settings load.

### [[session_templates]]  (by_id; scope global+project+repo; x-kelta-exec)
`{ id, label, layout: TemplateNode, enabled = true, options = [StartOption], claude_host = "builtin"|"terminal"|"multiplexer" }` where `TemplateNode = { split = "row"|"column", ratios = [f32], children = [TemplateNode] } | { session = "claude"|"editor"|"shell"|"setup"|"tool:<id>", name?: str, profile?: str /*claude profile*/, command?: Template /*shell: run this then stay interactive*/ }`. Settings → Branch folders (the `worktree` section) lists them under Advanced (add, disable, override); the `layout` tree is edited as TOML.

`StartOption = { label, values = [str] }` (#357): options picked per work start in the Start sheet, under the prompt. One value is a checkbox, several a single choice (or none). The picked values replace the Claude prompt's `{options}` (`claude.prompt_templates`), else go after it, in the declared order. A source route with `options` (below) replaces the options of the template the plan starts with (`work.default_template`). A Resume shows none. The last picks are remembered per route or template (kept in Kelta's state database, not in TOML) and pre-selected next time; a start without the sheet (`S`, `kelta-ctl start`) uses them.

Defaults:
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

`claude_host` (#347): `builtin` runs Claude in its Kelta pane. `terminal` opens it in a window of your terminal (`claude.terminal_command`), `multiplexer` in a new window of the tmux session `kelta-<repo>` (created detached when missing). Kelta still writes the worktree, `ticket.md`, the `--settings` hooks and `mcp.json`, and the Claude pane shows a banner with a Focus button: it runs a wrapper that lives as long as that Claude, so status, Resume (same host), park and Finish behave as with `builtin`; closing the pane ends the external Claude. Sending a prompt from Kelta (notes, queued prompts) needs the built-in pane.

### [[sources]]  (by_id; scope global only; category Projects)
Ticket boards. A source belongs to no project: it feeds zero or more of them, and the Tickets pane's `All` scope lists every source, also those feeding no project or only closed ones.

```toml
[[sources]]
id = "product"
label = "Product board"
projects = ["web", "api"]          # projects this board feeds; may be empty
blocked = { label = "blocked" }    # how this tracker marks a ticket blocked (below)
# status name → chip colour, like a project `color` (`theme:red|green|yellow|blue|magenta|cyan`, `theme:bright-…` or hex); unmapped statuses keep their category colour
status_colors = { "In QA" = "theme:magenta", "Ready for release" = "#2f8c86" }
# tracker fields shown on the tickets (below)
fields = [{ id = "customfield_10042", label = "Verified" }, { id = "customfield_10051", label = "" }]

[sources.tracker]                  # the §3 tracker binding without repo_rules
account = "jira-acme"
views = [{ id = "mine", label = "My open", jql = "project = SHOP AND assignee = currentUser()" }]
columns = [{ id = "todo", label = "To do", categories = ["todo"] }]
status_map = { start = { category = "in_progress" }, done = { category = "done" } }

[[sources.routes]]                 # was repo_rules; names the project whose repo starts the ticket
match = { component = "Web" }     # component, label and type (all that are set must match, case-insensitive)
project = "web"
repo = "web"
options = [{ label = "Auto", values = ["--auto"] }, { label = "Mode", values = ["--mode=brainstorm", "--mode=quick", "--mode=skip"] }]
```

`blocked` says how the tracker marks a ticket blocked without changing its status: `"status"` (the status name reads blocked, on hold, waiting or impediment), `"flag"` (the Jira flag), `"relation"` (a Linear "blocked by" an open issue) or `{ label = "<name>" }`. Unset, it follows the provider: `flag` on Jira, `relation` on Linear, `status` elsewhere. A blocked ticket shows a Blocked chip, goes to Waiting in Flow and sorts last in its group; **Block / Unblock** (`b` in the detail) sets the flag, adds or removes the label (Jira, GitHub, GitLab, Gitea), or moves a `status` ticket to a blocked status and back to in progress (else to do). A Linear relation, or a label on Linear or Redmine, is set in the tracker: the action is off with that reason. A status name that reads blocked still puts the ticket in Waiting whatever the setting.

`fields` shows tracker fields Kelta has no column for (a Jira custom field, a GitHub Projects v2 field) on the source's tickets: each field with a value is a `label: value` chip on the row (the value alone when `label` is empty) and a line of the detail's meta grid. `id` is the Jira field id or the GitHub project field name (any case); Settings → Sources lists the tracker's fields to pick from (Jira, GitHub project views) and takes a typed id elsewhere. The values come with the list call; other trackers show none yet. An id the tracker does not have shows nothing and is flagged *Unknown field* in the settings, never a list error.

A route is the first one in order whose `match` fits the ticket; an empty `match` never fits. `component` is compared with the ticket's labels (tickets carry no component field), `label` with its labels, `type` with its issue type. `options` are the route's Start options (see `[[session_templates]]`).

Errors that reject the file: an empty or duplicate `id`; no `tracker.account`; duplicate view or column ids within a source; a non-empty `tracker.repo_rules` (use `routes`); a route whose `project` is not in `projects`; `sources` in a project or repo file. An unknown project id or account is not a file error: queries skip the project, and fetching reports the account. A `status_colors` value that is not a theme colour or hex, or a `fields` entry without an `id`, is a warning: that entry is dropped, the rest applies (IPC writes reject it).

`ProjectInfo.tracker` is the first source listing the project, with its routes for that project as `repo_rules` (`project_update` with a `tracker` still writes that source).

**Settings → Sources** (after Projects, global layer only) edits the list: one card per source with its **Label**; **Board** (the account, the view rows with whose tickets, current iteration and remove, and **Add view…**, which opens the picker on that source); **Status colours** (every status of the source's columns, plus any already coloured, each with the project colour picker); **Columns** and **Status map** (schema forms); **Projects** (chips of the non-Home projects; unticking one drops its routes); **Routes** (Match component / label / type and its value, a Project among the source's, a Repository of that project; add, remove, reorder; the first match wins). Removing a source is confirmed. **Add source** opens the picker, which creates a source labelled after the picked board. Settings → Projects shows "Fed by …" per project, each source name opening its card.

### [[tools]], [[triggers]], [[commands]]
Schemas in PLUGINS.md §2-3 (same schema in config and plugin manifests). by_id; x-kelta-exec. Settings → Tools edits `tools`; `triggers`, `commands` and `workflows` are edited as TOML (Settings → Triggers lists triggers with origin and log). `tools` has scope global only (tools are personal: `[[tools]]` in a project file is a scope issue, in a repo file the issue "`tools` is personal: add it in Settings → Tools", with the tools ignored and the rest of the repo file applied); `triggers` has scope global+project+repo; `commands` is global (a repo file may also hold `commands`, inert until trusted; a project file may not).

The `fix_checks` trigger action (PLUGINS §3.1) sends a red CI run on my PR to its work item's Claude with no key
press, once per head SHA and failed-check set, never over the item's `claude.budget_usd`. It is off unless a
trigger asks for it:

```toml
[[triggers]]
id = "fix-red-ci"
on = "pr.ci_changed"
match = { "payload.state" = "failure" }
do = [{ action = "fix_checks" }]
```

Enabled tools are rows in the rail's Tools section (config order, icon and `label`; `icon` is a sprite icon name or 1-2 characters, and without one a well-known tool id or label (lazydocker, lazygit, a shell, an editor) gets its icon, any other the first two letters of its label; a lamp while a terminal tool runs, `⌥⌘N` for the first nine; the New tab menu lists 12, the palette all) and an "Open <label>" palette entry. Add, edit and remove them in Settings → Tools (Global layer; the Project layer shows the merged list and **Go to Global**). A tool opens where you are: the shown project and its focused work item, else Home (from Tickets, Now or Reviews). `scope` sets how many instances a tool has and where they open (Settings → Tools: **One instance**): `work` (default) one per work item, else per project, at the tool's `placement` (default `split_right`); `project` one per project, as its own tab in that project (a project picker asks from Home or a global view); `user` one for the whole app, as its own tab in Home, wherever it is opened from (lazydocker). Opening a tool whose instance exists focuses it. Pressing its key again while it is in front goes back (`nav.back`, to Now, Tickets or Reviews included) for a tool with no project argument (`user`, or no `{project.*}`/`{repo.*}` placeholder); a tool that takes a project asks for another project instead. Running instances show as sub-lines under the tool in the rail's Tools section (not for `user`); a tab made only of tools is listed there, not in its project's band. A tool whose template fields (`command`, `args`, `cwd`, `env`, `url`, `start.*`) use a `{project.*}` or `{repo.*}` placeholder, even in a fallback like `{worktree|project.root}`, needs a project: with no project in context a project picker asks which one, and the palette adds "Open <label> for project…", which always asks.

Migration (once, on load): each `projects/<id>.toml` that has `[[tools]]` is copied to `projects/.trash/`, then its entries move to `config.toml`. A new id is appended unchanged (placeholders kept), an identical entry or an `id`/`enabled`-only override is dropped, and a different definition under a taken id becomes `<id>-<project id>` labelled "<label> (<project name>)". A toast "Tools moved to your global settings" offers **Review** (Settings → Tools). Repo files are never rewritten. No tool is built in or auto-detected.

```toml
[[tools]]                    # one for the whole app, as its own tab in Home (scope "user"); focused if already open
id = "lazydocker"
label = "Docker"
command = "lazydocker"       # kind defaults to "pty"
scope = "user"               # "work" (default): a pane next to the current one (placement defaults to "split_right"),
                             # one per work item, cwd = its worktree, else project root; "project": one tab per project
keybinding = "mod+shift+d"

[[tools]]                    # external: launched detached, cwd = worktree of the active work item, else project root, never killed by Kelta
id = "fork"
label = "Fork"
kind = "external"
command = "open"
args = ["-a", "Fork", "."]
check = ["test", "-d", "/Applications/Fork.app"]   # dims the button when the app is absent
```

### [[workflows]]  (by_id; scope global only; x-kelta-exec; category Tools)
Prompt or slash command workflows Claude runs in a background tab (#355), next to the saved `*.js` ones of
`.claude/workflows/`. `on` says where they are offered: `ticket` (default) in Run workflow (`w`) on a ticket, which
the run claims; `pr` in Run workflow (`w`) on a Reviews row or the review detail; `global` as "Run <label>" in the
palette, with no ticket. A second run on the same ticket or PR opens the running one. `cwd` defaults to the PR's
worktree or repo for `pr`, else the project's primary repo (`~` expanded).

| Key | Type | Default | Notes |
|---|---|---|---|
| `id` | string | | what a run names it; a `*.js` workflow with the same name wins |
| `label` | string | `id` | picker, tab and palette text |
| `description` | string? | | picker detail |
| `prompt` | string | | `{workflow}`; ticket: `{ticket.key}`, `{ticket.title}`, `{ticket.url}`, `{file}` (ticket Markdown), `{args}` (the run's `args.json`); pr: `{pr.number}`, `{pr.url}`, `{pr.head}`, `{pr.base}`, `{pr.repo}`, `{pr.title}`, `{ticket.key}` (linked ticket, else empty) |
| `cwd` | string? | see above | |
| `on` | `ticket` \| `pr` \| `global` | `ticket` | |
| `enabled` | bool | `true` | |

```toml
[[workflows]]
id = "describe-pr"
label = "Describe PR"
prompt = "/describe-pr {pr.url}"
on = "pr"
```

### [plugins]
| Key | Type | Default | Notes |
|---|---|---|---|
| `dev_paths` | list<path> | `[]` | unpacked plugin dirs loaded in dev mode (global only; still require grants). Settings → Plugins adds one with a Browse… folder picker |
| `disabled` | list<PluginId> | `[]` | |
| `<id>` | table | plugin schema defaults | validated against the plugin's settings schema |

## 3. Project file `projects/<id>.toml`

```toml
[project]
id = "shop"                  # [a-z0-9-]{1,40}, must equal the file stem; `home`, `inbox` and `global` are reserved
name = "Shop"
color = "theme:green"         # a theme slot (red, green, yellow, blue, magenta, cyan, or bright-…; follows the theme) or a custom "#rrggbb" (kept as typed; "#rgb" reads as "#rrggbb"; anything else is a warning and the colour is unset)
icon = "S"                   # 1-2 chars or sprite icon name
default_template = "claude+editor"   # checked against session_templates; sets this project's work.default_template (over the global one and this file's [work]; a repo .kelta/config.toml [work] still wins)

[[project.repos]]
id = "api"
path = "~/code/shop-api"
primary = true
remote = "origin"            # default
base = "main"                # default
code_host = { account = "github-work", repo = "acme/shop-api" }

[[project.repos]]
id = "web"
path = "~/code/shop-web"
remote = "origin"
base = "develop"
code_host = { account = "gitlab-acme", repo = "shop/web" }

[project.tracker]              # deprecated: moved to the global [[sources]] at startup (below)
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
```

Tracker view schema per kind (`views[]` entries; `id`, `label` always). Keys valid for every kind:
- `who = "mine"|"unassigned"|"anyone"` (optional). When set it replaces the provider's assignee fields (`assigned_to`, `scope`, an assignee clause you wrote yourself in `jql` is kept as is). Absent keeps the per-kind keys below. The Tickets pane's Who tab overrides it.
- `current_iteration = true|false` (default `false`): only the current sprint (Jira), cycle (Linear), iteration (GitHub Projects v2), started milestone (GitLab) or next open version (Redmine). Ignored on gitea.
- `account = "<id>"` (optional): take this source from another account; absent uses the binding's `account`. Board mode and columns stay on the binding account.

- jira: `jql` (required), `board_id?` (the board's filter narrows the view, ANDed with `jql`; columns from the board config), `statuses?` (status names; only tickets in those statuses are listed, unset or empty = every status the query lists). Settings → Sources sets both per view: the board from the account's boards, the statuses from the source's open columns.
- redmine: `project_id?`, `query_id?`, `assigned_to = "me"|"any"` (default `me`), `status = "open"|"closed"|"*"` (default `open`).
- github: `repo?` (`owner/name`), `search?` (search query), `project_v2? = { owner, number, status_field = "Status" }`, plus the shared `status` and (per-repo views) `assigned_to` keys below.
- gitlab: `project?` (`group/sub/proj`), `scope = "assigned_to_me"|"all"`, `labels?`, `workflow_scope = "workflow"`, plus the shared `status` key.
- gitea: `project?` (`owner/name`), `scope = "assigned_to_me"|"all"` (a `project` with `scope = "all"` lists that repository; with `assigned_to_me` the search results are filtered to it), `labels?`, plus the shared `status` key. Columns are Open and Closed; moves are Close / Reopen. Bitbucket Cloud has no tracker (its issues were removed in August 2026): bind a Jira account for tickets and use Bitbucket as the code host.
- linear: `team?` (team key, e.g. `ENG`), `project?` (project name), `scope = "assigned_to_me"|"all"` (default `assigned_to_me`), `labels?` (label names), plus the shared `status` key. Columns are the team's workflow states; moves resolve state ids by name at runtime.
- Shared keys: `status = "open"|"closed"|"*"` (default `open`) for redmine, github, gitlab, gitea and linear; `assigned_to = "me"|"any"` (default `me`) for redmine and per-repo github views.

The onboarding sheet writes one view `{ id = "mine", who = "mine", ... }` with no assignee clause; the Tickets pane then offers Unassigned and Team without editing. Settings > Sources lists each source's views (label, account, who, current iteration, Remove) and **Add view…** opens the picker.

`status_map` keys: `start`, `review`, `done` → TransitionTarget. Overrides `work.on_*` targets for this project.

**`[project.tracker]` is deprecated** (still read; kept for one release); the binding schema above is the one of `[sources.tracker]` (§2). At startup Kelta moves every block to the global `[[sources]]`:
- each project file is first copied to `projects/.trash/<id>-tracker-<secs>.toml`, then the global file is written, then the block is removed from the project file; a failure stops there, logs and shows an error toast;
- blocks with the same account, views (each view's account resolved), `columns` and `status_map` become one source feeding all their projects; `repo_rules` become `routes` naming their project;
- same views with other columns or status_map stay separate sources labelled `<label> · <project name>`; a source is labelled after its only view, else its account; its id is the label's slug, made unique with `-2`, `-3`, …;
- a block equal to an existing source only adds the project to it, so a second run changes nothing.

A block still on disk (hand-written, restored from `.trash`, or a failed move) is read the same way in memory until the next launch moves it. After a move, the toast "Ticket sources moved out of your projects" offers **Review** (Settings > Sources).

## 4. Repo-local `.kelta/config.toml`

Allowed top-level keys: `triggers`, `session_templates`, `commands`, `worktree.{include, setup, setup_blocking, branch_template}`, `env`, `claude.append_system_prompt`, `editor.review_args`. Anything else → validation error "not allowed in repo config". Keys marked `x-kelta-exec` (`triggers`, `session_templates`, `commands`, `worktree.setup`, `env`, `claude.append_system_prompt`, `editor.review_args`) are loaded but **inert** until `repo_trust(path, sha256)` matches the current content; a banner offers "Review & trust" (shows the file). Trust is per content hash; any edit requires re-trust. `kelta-ctl trust <repo>` does the same from the CLI: it prints the file and its sha256, asks for confirmation, and sends that hash. Both paths trust only the reviewed content: if the file changed in between, the call fails with `conflict`.

## 5. Secrets

Config holds only `SecretRef` strings, never tokens:

| SecretRef | Resolution |
|---|---|
| `keyring:<name>` | OS keyring via keyring-core: service `dev.kelta`, user `<name>`. macOS Keychain (`apple-native-keyring-store`, `keychain` feature); Linux Secret Service (`zbus-secret-service-keyring-store`). |
| `file:<name>` | entry `<name>` of `<data>/secrets.enc`: 0600, XChaCha20-Poly1305 over a JSON map, key = Argon2id(passphrase, per-file salt; 19 MiB, t=2, p=1), header authenticated. Locked until the passphrase is entered in the UI (startup prompt when an account uses `file:`, or Settings → Accounts → Secret storage); the derived key stays in memory for the run (zeroized on drop), the passphrase is wiped after derivation. Writes are serialized and atomic (temp file + rename). |
| `gh-cli` | `gh auth token --hostname <host of base_url>` (read-only reuse; never copied) |
| `glab-cli` | plaintext `config.yml` token for host (glab path search order), else `glab auth status --show-token --hostname <host>` |
| `command:<argv>` | argv split shell-words, exec without a shell, 5 s timeout, stdout trimmed (e.g. `command:pass show jira/acme`, `command:op read op://…`, `command:secret-tool lookup service kelta account jira`) |
| `env:<VAR>` | process env (login env included) |

- Resolution runs off the UI thread with a 5 s timeout, result cached in memory only (zeroized on drop), keyed by the ref and host so a settings reload keeps it (a changed `secret` is a new entry), invalidated on 401 and on a login-environment change; a `keyring:` value survives login-environment changes too (only `set` / `delete` / 401 drop it), so a Keychain read prompts at most once per run. A keyring read that times out (an unanswered access dialog) is a `timeout` error, never "missing", and nothing writes a replacement. Never written to disk, never sent to the UI, never logged.
- `secret_set` / `secret_delete` write only `keyring:` and `file:` refs (Accounts wizard "Set token…"). On Linux with no `org.freedesktop.secrets` provider (common on Sway/Hyprland) or a locked collection, `secret_backends_status` reports it and the wizard proposes `file:`/`command:`/`env:` and shows the `gnome-keyring-daemon --start --components=secrets` / KeePassXC snippet.
- Browser sign-in (`oauth_device_start` / `oauth_device_finish`, RFC 8628, GitHub and GitLab): the device code never leaves the core; `<ref>.oauth` holds `{client_id, token_url, access_token, refresh_token, expires_at}` in the account's `keyring:`/`file:` backend (one entry, so a rotated refresh token is never stored apart from its access token), with a copy of the access token at `<ref>`; closing the wizard cancels a pending sign-in (`oauth_device_cancel`). Removing an account in Settings deletes its `keyring:`/`file:` token and `<ref>.oauth` unless another account uses the same ref. With `auth = "oauth"`, the HTTP auth layer refreshes a token expiring within 60 s (one refresh at a time; rotated refresh tokens are stored), and a refused refresh is `needs_auth` ("sign in again"); a network failure stays `network`.
- `secret_unlock {passphrase, create}` (UI IPC only, no ctl equivalent) unlocks the encrypted file for the run; `create` makes it when missing (`not_found` otherwise). `secret_backends_status` reports it as `encrypted-file` (available = unlocked) and Diagnostics warns while an existing file is locked. A wrong passphrase and a modified file give the same `needs_auth` error.
- macOS dev builds: Keychain prompts on each rebuild — CONTRIBUTING recommends `env:` refs in development.

## 6. Templates and placeholders

Simple `{path}` substitution (no expressions). Filters: `{x|slug}`, `{x|shell}` (POSIX single-quote; a shell slot refuses any value holding a control character, tab included, since readline would act on it while the line is typed), `{x|json}`, `{a|b}` = first non-empty of `a` or `b` (e.g. `{worktree|project.root}`). An unknown placeholder is a validation error at load time in `worktree.root`, `worktree.branch_template`, `work.on_start.comment`, `work.on_pr.comment`, `work.pr.title_template`, `work.pr.body_template`, `claude.prompt_templates.*`, editor preset `args` / `open_cmd` / `open_keys`, a session template `command`, and a tool's `command`, `cwd`, `args` and `env`. In commands typed into a shell (a session template's `command`), every value is POSIX-quoted by default (`|shell` is implied; writing it does not quote twice), so a ticket title cannot run as shell code.

Available: `project.{id,name,root}`, `repo.{id,path,name}`, `worktree`, `branch`, `base`, `key` (branch key), `slug`, `type`, `ticket.{key,title,url,file}`, `pr.{url,number,head,base,title}`, `session.{id,name,cwd}`, `sid8`, `run` (session runtime dir), `port` (free port allocated per tool instance), `work.id` (the work item), `work.port` (first port of the work item's block, `[ports]`), `config_dir`, `data_dir`, `home`, `user`, `closes` (`work.pr.body_template`). Also valid anywhere: `settings.<path>`, `event.<path>` and `payload.<path>` (trigger events and plugin settings), `plugin.dir`. Every prompt template also accepts `options` (the Start options, §2 `[[session_templates]]`), `task`, `workflow`, `args`, `file`, `onto`, `step` and `total`; each is filled only where `claude.prompt_templates` says, and empty elsewhere. Editor presets add `sock`, `path`, `file`, `line`, `cwd`, `sid8`.

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
dark_theme = "solarized"

[themes.solarized]
name = "Solarized"
base = "dark"
[themes.solarized.ui]
well = "#002b36"
fg = "#93a1a1"
accent = "#268bd2"
[themes.solarized.terminal]
background = "#002b36"

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

[accounts.linear-acme]
kind = "linear"
secret = "keyring:linear-acme"   # personal API key from Linear > Settings > Security & access

[[tools]]
id = "lazydocker"
label = "Docker"
kind = "pty"
command = "lazydocker"
cwd = "{repo.path|project.root}"
check = ["lazydocker", "--version"]
install_hint = "brew install lazydocker"

[[sources]]
id = "my-open"
label = "My open"
projects = ["shop"]

[sources.tracker]
account = "jira-acme"
views = [{ id = "mine", label = "My open", who = "mine", jql = "project = SHOP" }]
status_map = { start = { category = "in_progress" }, done = { category = "done" } }

[[sources.routes]]
match = { component = "frontend" }
project = "shop"
repo = "web"
```

## 9. Settings UI

`Mod+,` opens the Settings pane: layer selector (Global / Project ▾ / Repo ▾; Repo is read-only until trusted), section list with a search over key, title and description, and a schema-generated form (bool → toggle, enum → select with descriptions, int/float → number with min/max, str → text, Template → text with placeholder help, list<str> → chips, keyed lists → list editor with add/disable/override, map → key/value editor, SecretRef → source picker + "Set token…" + "Test"). Each key shows a **source badge** (Default/Plugin/Global/Project/Repo/Runtime), "reset at this layer", a restart marker and inline validation errors from Rust. Rarely changed keys sit under a collapsed Advanced block of their section, still searchable. A banner lists errors (the last good configuration stays active) and warnings (entries ignored, the rest applies), and names keys that need a restart. **Edit TOML** in the header switches the pane to a raw editor with validation; keys no form edits say so and link to it.

Sections, in nav order:
- Basics: General (theme picker, `app.*`, `themes` as TOML, updates and "Check now"), Window, Terminal, Keys (recorder, conflict checker, per-action prefix key, reset, compositor snippets for `kelta-ctl toggle`), Notifications, Linux graphics (Linux only).
- Work: Projects (create from folder, name, icon, colour, remove; "Fed by …" per project; `env`), Sources (§2 `[[sources]]`), Branch folders (`worktree`, `work`, `tickets`, `ports`, `session_templates`), Reviews, Tags, Claude (also Install / Remove of Kelta's MCP and hook relay in Claude Code, and the remembered permission mode), Editors.
- Integrations: Accounts (wizard, Test connection, browser sign-in, secret backend status), Tools, Triggers (list, origin, run log, test with a sample payload; `commands` and `workflows` as TOML), Plugins (install, enable, grants, uninstall; `plugins.*` as form and TOML; `dev_paths` takes folders from a Browse… picker).
- Advanced: Performance (`polling`, `performance`, `web`; live memory per component and per session, optional overlay).

**Tags** is not a setting: local tags (names and colours) live in Kelta's state database, not in TOML (SPEC §3.4c). Diagnostics is its own pane, not a Settings section.

# Kelta — Architecture

Status: the reference for the code as it ships in 1.0. A change to a contract below (crates, IPC, events, MCP, storage, processes) updates this file in the same commit.
Companion docs: SPEC.md (product), SETTINGS.md (config schema), PLUGINS.md (extensibility), BUILD_PLAN.md (the original build lanes).

Kelta is an open-source (MIT) desktop workbench for macOS 13+ and Ubuntu 24.04+ (Hyprland, Sway, GNOME). One window holds several **projects**; each project has tabs of split panes containing terminal sessions (Claude Code, nvim, shells, TUI tools), web tools (sl web / ISL), plugin screens and built-in views (tickets, board, reviews, inbox, settings).

---

## 0. Decision record (why this shape)

| # | Decision | Rationale | Revisit if |
|---|---|---|---|
| D1 | **Tauri v2 + Svelte 5 + xterm.js 6.0.0**, Rust core | Electron misses the memory budget (Chromium multi-process baseline 250-350 MB). Native egui/GPUI cannot embed sl web in-window on native Wayland and is too risky for a one-pass parallel build. Tauri reuses the system webview (WKWebView / WebKitGTK). | Gate G1 or G2 fails after tuning (BUILD_PLAN §6) → replace only `apps/desktop` + `ui/` with a native front end over the Tauri-free crates. |
| D2 | **Rust is the terminal source of truth.** One headless `alacritty_terminal::Term` per session; xterm.js instances exist only for visible panes plus a small LRU (`terminal.max_live_views`, default 2) | Hidden sessions cost only a Rust grid (no JS heap/DOM). Re-attach = compact ANSI snapshot. Background sessions still answer terminal queries correctly. | — |
| D3 | PTYs live behind the `TerminalHost` trait: in **`keltad`** (`terminal.session_host = "daemon"`, default) or in the app process (`inprocess`, also the fallback when keltad cannot start) | `keltad` runs kelta-term's `PtyTerminalHost` unchanged in its own process and the app talks to it with `DaemonTerminalHost` (same frames, attach/detach, acks, snapshots), so sessions survive quitting the app with no UI change; the models live only in keltad (no duplicate in the app). Closing the window ("background mode") keeps sessions alive and drops WebKit. | keltad's per-session memory or socket hop shows up in `kelta-bench` → move the hot path to fd passing. |
| D4 | Blocking reader **thread per PTY** (256 KiB stack) + `poll(2)` with timeout for DEC 2026 sync deadlines | Simplest correct design; ~10-20 threads is cheap with `M_ARENA_MAX=2`. | Profiling shows thread overhead > 1 MB/session. |
| D5 | **Typed IPC**: one Tauri command per operation, DTOs in `kelta-proto`, TS generated with `ts-rs`, CI drift check | Compile-time agreement between 10 parallel agents. | — |
| D6 | Programs are **exec'd directly** with a login environment resolved once (sentinel-delimited) — no `$SHELL -c` wrapper | Works with fish/nushell, no quoting bugs, Dock-launched apps get the user's PATH. | — |
| D7 | **xterm 6.0.0 stable; kitty keyboard via the Rust model**: alacritty_terminal tracks the mode stack and answers `CSI ? u` (`kitty_keyboard` from `terminal.keyboard_protocol`, default `kitty`); the active flags reach the view in Keyboard frames (§6.1) and `ui/src/lib/terminal/kitty.ts` (port of alacritty's encoder) encodes keys while they are non-zero. Shift+Enter remapped per session kind when the program did not enable kitty | xterm 6.0.0 cannot encode kitty, so the view encodes; the model is the only source of the flags, so model and encoder agree. | xterm ships a stable kitty encoder → compare with ours. |
| D8 | Claude status from **async command hooks** calling an **absolute, version-stable** `kelta-ctl` path; HTTP hook transport optional | Independent of PATH/AppImage mount; `SessionStart` only supports command hooks. | Claude Code changes hook semantics → version gate `claude.min_version`. |
| D9 | Extensibility = declarative tools + declarative triggers + plugin manifests with **sandboxed iframe screens**; no resident plugin runtime | Zero memory when unused; strong sandbox. Provider plugins (process/KPP, PLUGINS §9) run a child process per plugin, spawned on first use and served through the same `Tracker`/`CodeHost` traits (`KppFactory` wraps the built-in factories). | WASM logic plugins if process plugins prove too heavy. |
| D10 | Plugin grants and repo trust live in **SQLite app state**, never in user/repo-editable TOML | A repo or a hand edit must not be able to grant permissions. | — |
| D11 | One **deadline-heap scheduler** in core is the only owner of periodic work; nothing polls when nothing is visible/subscribed | Idle CPU ≈ 0, ≤ 1 wakeup/s. Lint-enforced. | — |
| D12 | Markdown (ticket/PR bodies) rendered in Rust (`pulldown-cmark` + `ammonia`) to sanitized HTML | No markdown/sanitizer libs in the JS bundle. | — |
| D13 | Clipboard via `arboard` in Rust (CLIPBOARD + Linux PRIMARY), never `navigator.clipboard` | Avoids the WKWebView paste callout; enables middle-click paste. | — |
| D14 | Web tools embed `auto` = iframe → (HEAD probe finds X-Frame-Options/frame-ancestors) → local header-stripping proxy → external browser. Child webview (Tauri `unstable`) is v0.2 | Works on native Wayland in-window. | — |

---

## 1. Stack and pinned versions

All versions exact (`=x.y.z` in Cargo, no `^`/`~` in package.json). The pins are the workspace `Cargo.toml`, `package.json` and `ui/package.json`; this table follows them (checked 2026-10-11).

### 1.1 Rust (toolchain `1.99.0`, edition 2024, `rust-toolchain.toml` with `rustfmt`, `clippy`)

| Crate | Version | Features / notes | Used by |
|---|---|---|---|
| tauri | 2.12.2 | default features; no `unstable`, no `devtools` in release | desktop |
| tauri-build | 2.7.1 | app manifest lists every command (explicit permissions) | desktop |
| tauri-plugin-dialog | 2.8.2 | system folder / file picker (Browse… buttons, plugin install); only `dialog:allow-open` granted | desktop |
| tauri-plugin-notification | 2.5.1 | | desktop |
| tauri-plugin-single-instance | 2.5.2 | forwards argv of 2nd launch | desktop |
| tauri-plugin-updater | 2.13.3 | driven from Rust (`platform/update.rs`), no JS permissions; registered only when the build config has `plugins.updater` (release overlay) | desktop |
| tokio | 1.53.2 | rt-multi-thread, macros, net, process, sync, time, io-util, fs, signal | all async crates |
| portable-pty | 0.9.0 | spawn/openpty only, behind `PtyBackend` trait; rustix fallback | kelta-term |
| alacritty_terminal | 0.26.0 | `default-features = false` (no serde); headless `Term`, `vte::ansi::Processor` (vte 0.15 transitive) | kelta-term |
| rustix | 1.1.5 | pty, process, event (poll), net (peer creds), termios, fs | proto, term, server, work, plugins, ctl |
| libc | 0.2.190 | `mallopt(M_ARENA_MAX)`, `malloc_trim` (glibc only), `getpeereid` (macOS peer uid) | desktop/platform, term, server, core, trackers |
| reqwest | 0.13.5 | `default-features = false`, features `rustls-no-provider, json, gzip, http2, query, form, system-proxy` | kelta-http, kelta-core (plugin `http_fetch`) |
| rustls | 0.23.45 | `default-features = false`, features `ring, std, tls12`: the TLS provider `kelta_http::client_builder` installs (no aws-lc-sys C build, #191) | kelta-http |
| axum | 0.8.9 | `ws`; lazy loopback HTTP server (MCP, http hooks, IDE bridge, web proxy) | kelta-server, kelta-plugins |
| hyper | 1.12.0 | proxy client | kelta-server, kelta-plugins |
| hyper-util / http-body-util | 0.1.21 / 0.1.5 | `client-legacy, tokio, http1`: hyper 1.x client + `TokioIo` (already in the graph via axum) | kelta-server, kelta-plugins |
| tokio-tungstenite | 0.29.0 | dev only: WebSocket clients in the proxy and IDE bridge tests (same version as axum's `ws`) | kelta-plugins, kelta-server (tests) |
| rmpv | 1.3.1 | hand-rolled msgpack-RPC client for nvim (`nvim-rs` not used: stale) | kelta-work |
| rusqlite | 0.40.2 | `bundled`; WAL; `PRAGMA cache_size=-2000` | kelta-core |
| toml | 1.1.8 | | kelta-config, kelta-plugins |
| toml_edit | 0.25.17 | comment-preserving writes | kelta-config |
| schemars | 1.2.2 | settings JSON Schema | kelta-proto, xtask |
| jsonschema | 0.58.6 | `default-features = false` (no remote `$ref` resolution); validate settings + plugin manifests + plugin settings fragments | kelta-config, kelta-plugins |
| serde / serde_json | 1.0.229 / 1.0.151 | | all |
| notify | 8.2.0 | watch config dirs (FSEvents / inotify, no polling) | kelta-config |
| keyring-core | 1.0.0 | | kelta-secrets |
| chacha20poly1305 / argon2 / zeroize | 0.11.0 / 0.6.0 / 1.9.1 | encrypted secrets file (`file:` refs, `secret_unlock`): Argon2id key, ChaCha20-Poly1305, zeroized buffers | kelta-secrets |
| apple-native-keyring-store | 1.0.2 | feature `keychain` (macOS only target dep) | kelta-secrets |
| zbus-secret-service-keyring-store | 1.0.1 | feature `rt-tokio-crypto-rust` (Linux only) | kelta-secrets |
| zbus | 5.19.0 | `default-features = false`, feature `tokio`; Linux: notification daemon / Secret Service probes | kelta-secrets, desktop |
| arboard | 3.6.1 | `default-features = false`, feature `wayland-data-control` (no image data); CLIPBOARD + PRIMARY | kelta-core |
| pulldown-cmark / ammonia | 0.13.4 / 4.2.1 | markdown → sanitized HTML | kelta-providers-common (in kelta-http crate `markdown` module) |
| uuid | 1.27.0 | v4, v7, serde | proto |
| regex / globset | 1.13.1 / 0.4.20 | | many |
| which | 8.0.6 | (directories are resolved by hand in `kelta_proto::dirs`) | core, work, plugins |
| thiserror / anyhow | 2.0.21 / 1.0.104 | anyhow only in binaries/tests | all |
| tracing / tracing-subscriber | 0.1.44 / 0.3.23 | `env-filter`; file log written by the desktop's own writer (`lib.rs`, 5 MB, rotated at launch), `info` default (`KELTA_LOG`), stderr only in debug builds; keltad logs to the stderr file the app hands it | all |
| ts-rs | 12.0.1 | export DTOs to `ui/src/lib/gen/` | proto |
| async-trait | 0.1.92 | object-safe provider/core traits | proto + impls |
| base64 / sha2 / time / bytes / parking_lot / futures / semver / mime_guess | 0.23.1 / 0.11.0 / 0.3.55 / 1.12.1 / 0.12.5 / 0.3.34 / 1.0.28 / 2.0.5 | `semver`: plugins, config, work | various |
| objc2 / objc2-foundation | 0.6.5 / 0.3.2 | macOS only: WebContent-terminated delegate, bench WebContent pid, dock badge, preferred language | desktop (L10) |
| webkit2gtk | 2.0.2 (the version wry 0.57.0 resolves; Linux target dep) | Linux only: `CacheModel::DocumentViewer`, `web-process-terminated` via `with_webview` | desktop (L10) |
| dev: insta / wiremock / proptest / tempfile / criterion | 1.49.0 / 0.6.5 / 1.11.0 / 3.27.0 / 0.8.2 | | tests |

`toml`/`toml_edit` are published as `1.1.8+spec-1.1.0` / `0.25.17+spec-1.1.0`; `=x.y.z` matches them. License policy (`deny.toml`): MIT, MIT-0, Apache-2.0 (± LLVM exception), BSD-2/3-Clause, 0BSD, ISC, MPL-2.0, Zlib, Unicode-3.0, plus a single exception for `notify` (CC0-1.0); the graph is restricted to macOS/Linux targets.

`kelta-ctl` depends **only** on `std`, `serde`, `serde_json`, `rustix` (peer socket) — target < 1 MB stripped, < 5 ms per hook invocation.

### 1.2 Frontend (Node 22, pnpm 9 workspace; Node is not shipped)

| Package | Version | Notes |
|---|---|---|
| svelte | 5.57.2 | runes; no SvelteKit, no router lib |
| vite | 8.3.4 | |
| @sveltejs/vite-plugin-svelte | 7.3.1 | |
| typescript | 6.0.3 | not 7.x: svelte-check and typescript-eslint peer ranges |
| svelte-check | 4.7.6 | |
| @tauri-apps/api / @tauri-apps/cli | 2.12.2 / 2.12.1 | |
| @tauri-apps/plugin-dialog | 2.8.2 | `open()` only (`ui/src/shell/BrowseButton.svelte`) |
| @xterm/xterm | 6.0.0 | exact |
| @xterm/addon-fit / -unicode11 / -web-links | 0.11.0 / 0.9.0 / 0.12.0 | eager |
| @xterm/addon-webgl / -search | 0.19.0 / 0.16.0 | dynamic import |
| lucide-static | 1.50.0 | icons (ISC): the UI's own names are tree-shaken into the startup sprite, any other Lucide name is a lazy chunk (`ui/src/lib/ui/icons.ts`) |
| vitest / jsdom / @testing-library/svelte | 5.0.3 / 30.1.2 / 5.4.2 | |
| @xterm/headless | 6.0.0 | dev only: parser-hook tests (query swallowing) |
| @playwright/test | 1.64.0 | UI e2e against Vite dev server + IPC mock |
| eslint / typescript-eslint / eslint-plugin-svelte | 10.12.0 / 8.71.1 / 3.23.1 | |
| prettier / prettier-plugin-svelte | 3.9.9 / 4.1.1 | |
| @eslint/js / globals | 10.0.1 / 17.12.0 | root devDependencies for the flat `eslint.config.js` (the lint toolchain, `svelte` and `typescript` are also declared at the root) |
| @types/node | 22.20.5 | ui, `vite.config.ts` and Node-side test helpers |

`jsdom 30.1.2` declares `engines.node ^22.22.2`; it runs on Node 22.16+, and `.npmrc` sets `engine-strict=false`. `packageManager` is `pnpm@9.12.1`.

**Not used:** `@xterm/addon-canvas` (dead), `@xterm/addon-serialize` (Rust snapshots), `@xterm/addon-clipboard` (OSC 52 handled by the Rust model), any CSS framework, icon font, web font, markdown lib, state lib, i18n lib. Icons: one inline SVG sprite built from `lucide-static`.

**Interface language (#344):** `app.language` = `system` (default) | `en` | `fr` | `de`, applied without a restart. The webview translates with `ui/src/lib/i18n/index.svelte.ts`: English text is the key (`t('Close')`, `tn(one, other, n)` for plurals, `msg()` marks English kept as data), `fr.json` / `de.json` are lazy chunks loaded on first use, `system` follows `navigator.language`, and `i18n.test.ts` fails when a key used in the source is missing from a catalog. Text emitted outside the webview (OS notification titles, the native macOS menu) goes through `kelta_proto::i18n::tr`, a small fr/de table; `System` resolves to the OS language the desktop records at startup (macOS `NSLocale.preferredLanguages`, else `LC_ALL` / `LANG`). Toasts and errors built by the core stay English.

---

## 2. Processes

```
kelta (one process; Rust)
├─ main thread: tao event loop (AppKit / GTK3), Tauri IPC dispatch
├─ tokio runtime: worker_threads=2, max_blocking_threads=8, thread_stack_size=1 MiB
│    providers, git, scheduler, bus, server, config watch handling
├─ per-session PTY reader thread (stack 256 KiB): poll → read 64 KiB → parse → forward
├─ sqlite thread (single connection, WAL, channel of closures)
└─ notify watcher thread (debouncer)
WebKit helpers (OS-managed): WebContent (UI + all iframes), Network, GPU (macOS)
children (each in its own PTY + session/process group): claude, nvim, $SHELL -l, lazygit, lazydocker …
non-PTY children: web-tool servers (sl web …) owned by kelta-plugins, killed with their tool instance
kelta-ctl: short-lived CLI (Claude hooks, compositor keybinds, scripts)
keltad (terminal.session_host = daemon): PtyTerminalHost + reader threads + the PTY children above; outlives the app
```

- **Single instance:** a 2nd `kelta [args]` forwards argv to the running instance (`tauri-plugin-single-instance`) → `ctl.command` events.
- **Control socket:** `<runtime>/ctl.sock` (mode 0600, dir 0700, owner verified). Every connection's peer uid must be ours, else it is dropped: Linux `SO_PEERCRED`, macOS `getpeereid` (tokio's `peer_cred` also asks for the pid, which fails with ENOTCONN once a client that wrote its line has closed, as the statusline relay does, #366; `getpeereid` reads the credentials saved at connect). Line-delimited JSON (`kelta_proto::ctl`), one request per line; `kelta-ctl` speaks it without linking kelta-proto.
- **`kelta-ctl start --task "<text>" [--project <id>]`** sends `CtlCommand::StartTask{task, project}`: core plans `WorkSource::Branch{name: "", task}` for the project (default: active), runs the start-work saga without a sheet and focuses the project. Same branch rule and refusals as New work item (`⇧⌘N`); a ticket key and `--task` together are a usage error.
- **Lazy HTTP server:** axum on `127.0.0.1:<random>`; starts on first need (first Claude session with `claude.mcp = true` or `claude.hook_transport = "http"`); stops when its consumer count reaches 0 (event-driven, no idle timer). Core and kelta-work each hold a consumer per Claude session they spawn (refcounted, so double counting is harmless). MCP is a hand-rolled stateless Streamable-HTTP subset (`POST` → `application/json`, `GET`/`DELETE` → 405, no `Mcp-Session-Id`): rmcp's transport keeps sessions alive with periodic SSE pings, which the no-periodic-timer rule (§13) forbids. MCP tools: `get_ticket`, `transition_ticket`, `add_ticket_comment`, `open_in_editor`, `create_pr`, `list_review_requests`, `get_review_feedback` (Markdown of `CoreApi::work_feedback` for the session's work item: unresolved threads with `path:line`, review summaries, failed checks (check runs and legacy statuses) with their cleaned 80-line log tail; the layout of the Fix with Claude sheet's `feedback.md`), `add_review_comment{path, line, body}` (line comment on the pending review of the PR under review in the session's review-kind work item, via `CodeHost::add_pending_comment`; published by the user's a / c / m in the review detail), `notify`, `ask{question, options, body_md?}` (#356: a question with one button per option on the session's rows, in Now and in Peek; `question` ≤ 500 characters, 1 to 9 `options`, each one non-empty line ≤ 100 characters; PLUGINS §8). These session tools (`get_ticket` to `ask`) exist only on `/mcp/<sid>`: every Claude session Kelta spawns with `claude.mcp` on (`Core::spawn_claude`: ⌘T and template Claudes, the setup session, runs; work items likewise) gets its own `mcp.json` passed with `--mcp-config` (#493, #496). Board tools, on any ticket by key: `list_tickets{project?, view?, status?, assignee?, label?, tag?, free?, refresh?}` (`tag`: one of my local tags, #349; `free`: only tickets not `blocked` and without an unfinished work item, claimed or started, #503) (JSON rows of `CoreApi::ticket_list` = `tracker_list`, first page, cache first; each row carries `blocked` and `work_item`, the id of `TicketItem.work_item_id`, else null), `get_ticket_by_key{key, account?}` (the `get_ticket` Markdown plus `Account` and `Revision`, an FNV-1a hash of the description), `transition_ticket_by_key{key, account?, to}`, `comment_ticket_by_key{key, account?, markdown}`, `update_ticket_body{key, account?, body, revision}` (re-reads the description and answers a conflict without writing when its revision differs; `Tracker::set_body`), `close_ticket{key, account?, comment?}` (`Tracker::close`, GitHub issues; the cached rows are patched like any write), `move_ticket_step{key, account?, step}` (#463: `CoreApi::ticket_step_set` → `WorkService::move_step`, FLOW §4.9; replies `KEY is now at step <id>.`). A key resolves from the lists' first pages (`CoreApi::ticket_by_key`: cached, fetched when cold, without the list's PR / people enrichment), else from `account` + the key. The five writes are listed only while `claude.mcp` and `claude.mcp_board_writes` are on. List loads are single-flight per cache key (`Feeds::fetches`): concurrent cold misses, and `refresh` calls, wait for one provider fetch and share its page.
- **Global MCP (`/mcp`, ticket #234):** every tool except the session ones (board tools, `list_review_requests`, the background-run and setup tools below) for Claude sessions Kelta did not spawn, behind a per-install token (`Server::set_install_token`; per-session tokens are refused there). The token lives only in Claude Code's registration (`mcpServers.kelta.headers.Authorization`, which Claude must send anyway, in a 0600 file): Kelta reads it back from there and generates a new one only when there is no registration, never in the Keychain (#275: an ad-hoc signed build is a new code identity, so each update prompted for it). At startup (`Core::claude_mcp_startup`, after the `claude --version` probe), with `claude.binary` found, Kelta reads `mcpServers.kelta` from Claude Code's user config (`$CLAUDE_CONFIG_DIR/.claude.json`, else `~/.claude.json`; no `claude` process, for reads or writes): no entry → an Install / Never toast (dismiss = not now; Never sets `claude.mcp_offer = false`); an entry → Kelta holds an HTTP consumer for the app's life (binding the registered port again when free) and, when the URL or token differ, re-registers silently. Kelta writes the entry itself (`{"type":"http","url":"http://127.0.0.1:<port>/mcp","headers":{"Authorization":"Bearer <token>"}}`, other keys kept in their order and indentation, a temp file renamed over it, 0600, following a symlink, refusing an unparsable file, under Claude Code's own `<config>.lock` (proper-lockfile: a directory, stale after 10 s) so a concurrent Claude Code save re-reads it; 5 s after a write a one-shot check restores the entry if a save dropped it anyway, #322), not with `claude mcp add --header`: that argv would show the token to any local user in `ps` (#294). Install / Remove hold one lock, so concurrent ones take one HTTP reference and one token. Settings → Claude shows the state with Install / Remove (`claude_mcp_*` commands); Remove drops `mcpServers.kelta` (the token goes with the registration) and closes `/mcp`; a `dev.kelta`/`kelta-mcp` Keychain item left by older builds is not touched, since reading or deleting it would prompt.
- **Sessions Kelta did not start (#345):** Settings → Claude (and a startup Install / Never toast, `claude.hooks_offer`) registers `'<kelta-ctl>' hook --outside` in Claude Code's user settings (`$CLAUDE_CONFIG_DIR/settings.json`, else `~/.claude/settings.json`) for SessionStart, SessionEnd, UserPromptSubmit, PermissionRequest, PostToolUse, Notification, Stop and StopFailure (`Core::claude_hooks_*`, `crates/kelta-core/src/outside.rs`); other keys, groups and events stay in their order and as written, and Remove drops only the relay's groups. In a Kelta pane (`KELTA_SESSION_ID` set) the relay does nothing, so a Kelta session is never counted twice; elsewhere it sends `outside_hook {payload, pid}` (its parent: Claude, since `sh -c` execs a lone command) on the ctl socket. Core keeps an `OutsideSession` per Claude session id (cwd, title from the first prompt, status through the same `status::apply_hook`, branch read once, a listed ticket whose `branch_key` the branch or cwd carries) in memory and emits `outside.updated` / `outside.removed`; an entry goes on SessionEnd or, checked on each relayed hook and each `outside_list` (Now's refresh), when its pid is gone. The UI lists them dimmed in an Elsewhere band (rail) and section (Now), asking ones in Claude needs you (badge, Mod+J), and on their ticket's row. Enter, `g` or a double click on the Now row, a click on the rail row and Mod+J call `outside_jump` (`crates/kelta-core/src/outside_jump.rs`, #346): one `ps -wwAo pid=,ppid=,comm=` gives Claude's parent chain; a pane of the default tmux server whose `pane_pid` is in it is selected (`select-window` + `select-pane`, `switch-client` when no client is on its session) and the chain restarts from a client's pid; then the window of the closest parent is focused, on any workspace: Hyprland `hyprctl clients -j` + `dispatch focuswindow address:`, Sway `swaymsg -t get_tree` + `[con_id=] focus`, macOS `open -a` on the outermost `.app` bundle in a parent's path. Elsewhere (GNOME, a detached tmux session, a gone process) it answers where the session runs (the first parent that is not a shell, or `tmux <session:window.pane>`) and the UI opens Peek on Claude's last message with that note and the cwd. `outside_jump` also takes the id of a live Kelta session whose Claude runs in an external host (next item): the chain then starts from the host shell's pid in `host.pid`.
- **External Claude host (#347, `crates/kelta-core/src/claude_host.rs`):** a session template's `claude_host` = `builtin` (default, a Kelta pane) | `terminal` (a window of the user's terminal: `claude.terminal_command` with `{command}`, `{cwd}`, `{shell}`; empty = `open -na Terminal` on macOS, `x-terminal-emulator -e` on Linux) | `multiplexer` (a new window of tmux session `kelta-<repo>` on the default server, created when missing). Kelta still spawns the Claude session: its pane runs a `/bin/sh -c` wrapper that writes `claude.command` (0700, in the session's private runtime dir) exporting the session's Kelta variables (session id, hook and MCP tokens, project env, `PATH`; never `TERM`, `COLORTERM`, `TERM_PROGRAM*` or values equal to the login env), launches the host on it in the background, and waits on a 0600 FIFO for Claude's exit code. The pane lives exactly as long as that Claude, so hooks, status, Resume and Finish behave as for a built-in pane; closing the pane SIGHUPs the host shell's children (pid in `host.pid`). `ClaudeMeta.host` is set for these sessions; text Kelta would type into them (Fix with Claude, review notes, Send selection, trigger `send_keys`, plugin `sessions.send_text`) is refused (`Conflict: Claude runs outside Kelta…`, `SessionInfo::refuse_hosted`).
- **Background runs (#235):** `claim_ticket`, `report_progress` and `release_ticket`, on `/mcp/<sid>` and `/mcp` alike (not behind `mcp_board_writes`: they write only Kelta's work store), let a workflow that runs outside Kelta's start-work path (a `kelta-ticket-pass` lane driven by an orchestrator session) say which tickets it took and how far each got.
  - `claim_ticket{key, account?, project?, worktree?, branch?, run?: {id?, name?}, adopt?}` → `CoreApi::work_claim` → `WorkService::claim`: the key resolves like the board tools, the project from `project`, else the ticket's first project, else the calling session's. `worktree` (default: the project's primary repo) must be a `git worktree list` entry of one of the project's repos (`InvalidArgument` otherwise); `branch` defaults to its checkout and must pass `git check-ref-format --branch` (`InvalidArgument` otherwise). The new work item (`kind = ticket`, `state = active`, no sessions, no port block) carries `WorkItem.run: RunClaim{name, run_id, session_id, phase, state, note, sha}` with `state = running` and `session_id` = the caller (`None` on `/mcp`). The holder check and the insert are one SQLite transaction (`WorkStore::claim_item`, `q::work_claim`, under `start_lock` so an interactive start cannot slip in): an unfinished item of the same ticket wins and the call fails `Conflict: <key> is held by session <name> (<run name>)` (or `work item <id> (started in Kelta)`), except an `orphaned`, `blocked` or `failed` claim (its lane has ended; blocked and failed ones stay in Now's needs-you until taken over) or one of the same run (both `run.id` set and equal), or one the caller holds (#497: the held run's `session_id` is the caller's and the run ids are equal or one is missing; or the item has no run and the caller is in its `session_ids`), which is taken over (same id; the new run's worktree, branch and session). Lanes of one orchestrator share the `/mcp/<sid>` session, so only `run.id` tells them apart: each lane passes its own. A holder's re-claim without `worktree` keeps the item's worktree, branch and base. Tool errors start with the error code (`Conflict: …`, `InvalidArgument: …`) so a run can branch on it. A caller that is a live Kelta Claude session in no unfinished work item's `session_ids` (read from the store, so it holds after a restart) gives its Claude to the claimed item, whatever started it (a ticket, `pr` or `global` run, a trigger's run, a plain `⌘T` tab): `Core::adopt_claiming_session` → `WorkService::adopt_run` (#491, #494), then `session_link` sets `SessionInfo.work_item_id` and its ticket, so the sidebar and Background rows, `open_in_editor`, `get_ticket` and the session tools follow the item. A session already in a live item, the holder's re-claim (#497), a caller passing `adopt: false` and `/mcp` only record `session_id`.
  - `report_progress{key, phase, state: running|blocked|failed|done, note?, sha?}` → `work_report`: updates the run of the live claim on `key` (`phase` is free text, the workflow's `meta.phases` titles; an omitted `note` / `sha` keeps the last one). Only `state` drives the lamp. A claim with a `session_id` answers only to that session: `report_progress` and `release_ticket` from another session, or from `/mcp`, fail `Conflict`, unless the claim is orphaned (below). When several claims share the key, the caller's own wins, then an unowned one, then an orphaned one.
  - `release_ticket{key, outcome: merged|abandoned, sha?}` → `work_release`: `run.state = done`, then Finish. Finish on an item with `run` (an adopted worktree) never removes the worktree, deletes the branch, runs `worktree.teardown` or moves the ticket: the run owns those, and board status stays the workflow's job. `abandoned` therefore leaves the worktree on disk; `merged` finds it already removed by the run.
  - `link_pr{key, account?, pr, project?, status?}` (#453) → `CoreApi::work_link_pr` → `WorkService::link_ticket_pr`: `pr` resolves among my open authored PRs (`review_list(All, Authored)`) by URL or by a number only one of them has (`NotFound` / `InvalidArgument` otherwise); the project from `project`, else the calling session's when the PR's repo is bound in it, else the PR's, else the ticket's, else the session's. The reply is `Linked <key> to <url>: work item …`, or `Linked already: no change. …` when the PR's item already had the ticket. `status` (#457) then moves the ticket, also when it was linked already (a retry after a failed move): a role, `start` / `review` / `done`, maps through the project's `tracker.status_map`, else `work.on_start` / `on_pr` / `on_merge.transition_to` (as Start work, `on_pr` and the merge do); anything else is a status or transition name, matched as `transition_ticket_by_key` matches `to`. `status` needs `claude.mcp_board_writes` (it moves any ticket to any status): off, the call fails with `InvalidArgument` before linking anything. A failed move keeps the link: the result adds `Warning: <key> not moved to "<status>": <why>`. Without `status` the ticket stays. FLOW §2.1 has the rest.
  - Orphans: computed, never swept (#271). A `running` claim whose `session_id` is not a Live session (exited, Dormant after a restart, or gone) is orphaned: `claim_ticket` takes it over, `report_progress` / `release_ticket` accept any caller, and a non-owner's `report_progress` makes the claim the caller's (`session_id` = caller), never the dead session's again. `work_list` and `work.updated` show it `orphaned` (it keeps its last phase); `session.exited` republishes the session's claims so the UI updates.
  - UI: `phaseOf` reads a live `run` first: label `<run name> · <phase>` (`(orphaned)` appended), detail = `note`, lamp working (running), amber `error` (blocked), red `needs_input` (failed), none (orphaned); blocked and failed go to Now's needs-you section with the note as the reason. The ticket row shows that label with the note as its tooltip, the ticket detail a lamp + label in place of `local work`, and the claiming session's terminal pane lists its claims (`key · phase · state`). Enter on the row opens the item's tab (its template's editor and shell in the adopted worktree, plus the adopted Claude; an item with none, after `adopt: false` or a `/mcp` claim, has no Claude slot and `claude_uuid` is `None`).
  - Worked example, `~/.claude/workflows/kelta-ticket-pass.js` (applied to the script after merge; each call is one line in that stage's agent prompt, `KEY` = `InjustFr/kelta#<N>`, `WT` / `BR` the stage constants, `LANE` = the workflow instance id, one per lane and identical in every stage of it):

    | Stage | Call |
    |---|---|
    | Pick, step 4 (replaces the branch-name marker check and `git branch feat/ticket-<N> main` as the claim) | `claim_ticket{key: KEY, worktree: REPO, branch: "main", run: {id: LANE, name: "kelta-ticket-pass"}}` before moving the board item; on `Conflict` drop that candidate and pick another. Return the work item id. |
    | Implement, after `git worktree add WT BR` | `claim_ticket{key: KEY, worktree: WT, branch: BR, run: {id: LANE, name: "kelta-ticket-pass"}}` (same `LANE` as Pick: re-points the claim at the new worktree, same item; another lane gets `Conflict`), then `report_progress{key: KEY, phase: "Implement", state: "running"}` |
    | Review, each round | `report_progress{key: KEY, phase: "Review", state: "running", note: "round <r>"}` |
    | After the review loop, when it returns `blocked` | `report_progress{key: KEY, phase: "Review", state: "blocked", note: "<n> unresolved review findings"}` (or `note: "targeted checks fail"`); a one-line haiku agent, since the script itself cannot call MCP tools |
    | Gate | `report_progress{key: KEY, phase: "Gate", state: "running"}`; on `passed = false`: `state: "blocked", note: "gate failed: <step>"` |
    | Merge | `report_progress{key: KEY, phase: "Merge", state: "running"}`, then after step 4 (worktree removed) `release_ticket{key: KEY, outcome: "merged", sha: <merge sha>}`; on `merged = false`: `report_progress{key: KEY, phase: "Merge", state: "failed", note: <notes>}` |
    | Any stage agent returning nothing (crash) | the orchestrator calls `report_progress{key: KEY, phase: <stage>, state: "failed"}` |
- **Launch from Kelta (#233):** `workflows_list{ticket}` lists the saved workflows a run can use: `*.js` files in `<primary repo>/.claude/workflows/` (only the primary repo: the run's cwd, where Claude resolves names) then `~/.claude/workflows/`, a repo one shadowing a home one of the same name. `name` and `description` come from the file's `export const meta = { … }` literal, read by a small scanner (the first top-level `name:` / `description:` string, `'`, `"` or backtick; the JS never runs); a file without a `name` is skipped. The list is read again each time the picker opens (no watcher). `workflow_run{ticket, workflow}` (`Core::workflow_run`):
  1. the project is the one start work picks for the ticket (SPEC §3.1, `Core::ticket_project`: the rules without building a plan; none → `Add a repository to a project first`);
  2. Kelta claims first, through `WorkService::claim` with the default worktree and branch (the primary repo and its checkout) and `run = {name: <workflow>, run_id: <workflow>-<sid8>, session_id: <the new session>, phase: "Starting", state: running}`, so the chip shows at once. A second launch while a run with a live session holds the ticket spawns nothing and returns that session with `running: true` (the UI opens it); a ticket held by a work item that is not a run fails with the usual `Conflict: <key> is held by …`;
  3. it writes `ticket.md` (`ticket_markdown` of `tracker_get`) and `args.json` (`{"ticket": {key, title, url, file}, "run": <run id>}`, serde) to the session's private run dir, renders `claude.prompt_templates.workflow` with `{workflow}`, `{ticket.key}`, `{ticket.title}`, `{ticket.url}`, `{file}`, `{args}`, and spawns a plain Claude session (default profile, hooks file, `--session-id`, `RestorePolicy::ClaudeResume`) in the primary repo; a failure here calls `work_release` (ends the claim, never touches the repo);
  4. the session is linked to the ticket (`session_link(sid, None, Some(ticket))`: MCP `get_ticket` resolves for it) and opens in a new tab `<workflow> · <key>` with `focus: false`.
  The workflow takes the claim over by passing `args.run` as its `run.id` (same run id: same work item, its own worktree and branch). One that never calls the tools keeps `Starting` until its session exits, then the claim turns `orphaned`. Restoring the session resumes the Claude conversation, not the workflow. The run starts even when `claude.max_live` is reached; as a live Claude it counts against later starts. The UI toasts `<workflow> started on <key>` with Open (`session.reveal`) and Peek (`session.peek`).

  **Config workflows (#355):** `[[workflows]]` entries (`id`, `label`, `description`, `prompt`, `cwd`, `on`, SETTINGS §2) are prompts or slash commands. `on = "ticket"` ones are listed by `workflows_list` after the `*.js` ones (`source: config`, `path: null`, a `*.js` name shadows the id) and run by `workflow_run` as above, the prompt rendered from the entry instead of `prompt_templates.workflow`, with no phase (`phase: null`) and in `cwd` when set. `workflow_run_on{project_id, workflow, review}` runs an `on = "pr"` entry on `review` (placeholders from `review_get`: `{pr.number}`, `{pr.url}`, `{pr.head}`, `{pr.base}`, `{pr.repo}`, `{pr.title}`, `{ticket.key}` = first linked ticket; cwd: the worktree of the PR's unfinished work item, else the repo whose `code_host` binds the PR, else the primary repo) or an `on = "global"` one with `review: null`, in a background tab `<label> · <repo>#<n>` / `<label>`. These claim nothing; one run per PR is kept in memory (`Core::pr_runs`, PR → session; a second run on a PR whose session is live returns it with `running: true`). The UI lists `pr`/`global` entries from the global settings: `w` on a Reviews row and in the review detail (plus a Run workflow button when one exists), "Run <label>" in the palette.

    The pick stage keeps moving the board item to In Progress and the merge stage to Done: Kelta never moves a ticket on claim or release.
- **Setup with Claude (#343):** `get_setup`, `add_project`, `set_setting`, `test_account` and `request_token`, on `/mcp/<sid>` and `/mcp` alike, configure Kelta itself. Not behind a setting: a Claude Code session can already edit the TOML files; the tools add Kelta's validation. `get_setup` → projects (repos, remotes, code host bindings), accounts (secret references only), sources, editor / terminal preferences, `claude.mcp`, and `missing` (no project, no account, a project fed by no source, a repo without a code host account, an account without a secret reference or, for a tracker, without a source). `add_project{path}` → `CoreApi::project_add` (`project_detect` + `project_create`, with the draft's hints in the reply). `set_setting{path, value, layer?: global|project, project?}` → `CoreApi::settings_write` → `ConfigService::layer_set`, the write of `settings_set`: same validation (a raw token in an `x-kelta-secret` field is refused, the message never echoes it), `settings.changed` to the UI. `test_account{account}` → `CoreApi::account_test` (the `account_test` command's code). `request_token{account}` toasts "Enter token" → `settings.open{section: "accounts"}`: the user types the token in Kelta, never through MCP. `claude_setup` (onboarding's "Set up with Claude", palette action `claude.setup`) registers `/mcp` when Claude Code lacks it, then spawns Claude in Home with the setup prompt (`kelta-core/src/setup.rs`: look around, code hosts, projects, trackers and views, editor / terminal, Claude integration, `test_account` on every account and a summary). The onboarding sheet opens on a fresh install (no project of its own, `kelta.onboarding.done` unset).
- **Web-tool proxy:** kelta-plugins serves the proxy (`proxy::ensure_listener`) on its own loopback listener per proxied tool instance (started on first open, stopped with the instance), so each web tool keeps a distinct origin and needs no kelta-server port.
- **Session daemon (`keltad`):** `<runtime>/keltad.sock` (0600 in the 0700 runtime dir, peer uid checked on both ends); keltad writes `<runtime>/keltad.pid` on start and removes it on exit. Core launches it from the stable copy `<data>/bin/<version>/keltad` (`keltad --socket <path> --history <data>/history`, stderr → `<logs>/keltad.log`); it binds, forks into its own session, and the launcher connects once the parent exits. Messages: `u32 len` + `u32 json_len` + JSON head + raw bytes (input, frames). Each session's events go to the client that spawned or last adopted it. With `session_host = daemon` the on-disk history log (§9.6) lives in keltad, written only when `--history` is given. Core closes every session it removes (`kill` on the exited session), so keltad drops its model and route. On quit, sessions core would restore stay running (no SIGHUP); the others are killed and closed. On start, core `adopt`s every persisted session keltad still runs (Live, same hook token) before any Dormant respawn and kills the rest. keltad exits 30 s after it has no client and no running session (one-shot grace armed by the disconnect / exit, no polling), or at once on `shutdown` (quit with "stop all sessions": it SIGKILLs every session first; additive op, no protocol bump, an older keltad drops the connection and leaves after its grace). A keltad that refuses `Hello` (another `PROTOCOL_VERSION`, left by a previous Kelta) gets SIGTERM (pid from `keltad.pid` next to the socket, else `lsof -t` on the socket for a keltad older than the pidfile; it must be a process named `keltad` and not the app itself, because peer credentials name the dead pre-fork parent), its socket is removed and the new keltad launched: its PTYs get SIGHUP and core brings the kept sessions back from Dormant, so nothing runs twice.
- **Bundling:** `kelta-ctl` and `keltad` are added as `bundle.externalBin` only in the release config overlay `packaging/tauri.release.json` (`tauri build --config …`), so dev builds and `cargo clippy` never require the sidecar to exist.
- **Updates (client, `apps/desktop/src-tauri/src/platform/update.rs`):** tauri-plugin-updater driven from Rust, no JS permissions. The plugin is registered only when the build config has `plugins.updater` (the release overlay sets its minisign `pubkey`, not Apple signing) and the build is not a debug one. The channel comes from the running version: one containing `-nightly.` reads `https://github.com/InjustFr/kelta/releases/download/nightly/latest.json` and installs any version other than the running (or already installed) one; any other version reads `releases/latest/download/latest.json` (GitHub's latest skips prereleases) and installs only a higher one. The release channel's `latest.json` is published with every `vX.Y.Z` release by `release.yml`. Unsupported: dev and debug builds ("Development builds do not update."), Linux without `APPIMAGE` set, e.g. a `.deb` ("Updates come from your package manager."), a macOS app run from `/Volumes/` or an App Translocation path ("Move Kelta to Applications to get updates."). Automatic checks run after `app_ready` and on window focus when `app.updates` is on and the last check is ≥ 6 h old (in memory), never on a timer; `app_update_check` ("Check now") runs one at once; one check at a time. An update found is downloaded, verified and installed at once and runs from the next start: `UpdateState` goes `checking` → `downloading` → `ready{version}` (sticky until a newer install), or `up_to_date` / `failed{message}`; the end of each check emits the Tauri event `kelta://update` and the banner re-reads `app_update_status`. Kelta restarts only on the banner's Restart (`app_restart` → `request_restart`, so `Core::shutdown` runs and keltad keeps the kept sessions). A keltad on the same protocol keeps its old binary until it exits on its own; the stable copy is replaced atomically. Publishing: with `TAURI_SIGNING_PRIVATE_KEY` set, `packaging/build.sh` also makes the signed updater bundles (`.app.tar.gz`, AppImage, each with a `.sig`); `nightly.yml` stamps `<version>-nightly.<UTC yyyymmddHHMM>` (the Cargo version, hence `<data>/bin/<version>/`, is unchanged) and `scripts/publish-nightly.sh` uploads `latest.json` (`scripts/update-manifest.sh`) after the other assets; `release.yml` does the same for `vX.Y.Z`. Details: CONTRIBUTING.md ("CI", "Nightly builds", "Releases").
- **Background mode:** closing the window with `window.close_behavior = background` destroys the webview (WebKit processes exit) while core + PTYs keep running; `kelta`, Dock click or `kelta-ctl toggle` recreates it and views re-attach from snapshots.
- **Detached windows (#342):** a tab can move to its own window. The UI marks the tab `detached` in its layout (`Tab.detached`, saved with `layout_save`), then calls `window_detach{project_id, tab_id}`: `window::open_detached` creates (or focuses) the webview window `detached-<tab id>` on `index.html` with `window.__KELTA_DETACHED__ = {project_id, tab_id}`, so the page shows that one tab. The capability covers `["main", "detached-*"]`. Each window subscribes its own `Channel<UiEvent>`: ordinary events go to every window; `ctl.command`, `ui.open`, `app.quit_requested` and toasts go to the main window only (queued while it does not exist); `layout_save` sends `layout.changed` to the other windows. Focus and visibility (`UiBridge::window_state`) are any-of over the windows. Closing a detached window reattaches its tab first; quitting keeps tabs detached and reopens their windows at the next launch (geometry in `<data>/detached-windows.json`). Out of background mode, closing the main window quits even with detached windows open.

### 2.1 Filesystem locations

| Kind | Linux | macOS |
|---|---|---|
| config | `$XDG_CONFIG_HOME/kelta` (`~/.config/kelta`) | `~/.config/kelta` (honours `XDG_CONFIG_HOME`) |
| data (db, plugins, bin, logs) | `$XDG_DATA_HOME/kelta` (`~/.local/share/kelta`) | `~/Library/Application Support/dev.kelta.Kelta` |
| state db | `<data>/kelta.db` | same |
| window geometry (`window.restore_geometry`) | `<data>/window-geometry.json` (main window), `<data>/detached-windows.json` (detached windows by label, #342) | same |
| terminal history logs | `<data>/history/` (0700): `<session id>.log` + `<session id>.1.log` (0600), §9.6 | same |
| logs | `$XDG_STATE_HOME/kelta/logs/kelta.log` (5 MB × 2, rotated at launch; `keltad.log` beside it) | `~/Library/Logs/Kelta/kelta.log` |
| stable CLI copy | `<data>/bin/<version>/{kelta-ctl,keltad}` + `<data>/bin/current` symlink | same |
| runtime | `$XDG_RUNTIME_DIR/kelta` (fallback `/tmp/kelta-<uid>`) | `/tmp/kelta-<uid>` |
| per-session runtime | `<runtime>/s/<sid8>/` (0700): `claude-settings.json` (0600), `mcp.json` (0600), `ticket.md`, `context.md`, `nvim.sock`; an external Claude host (#347) adds `claude.command` (0700), `host.pid` and `host-<random>.fifo` (0600) | same |
| Claude IDE bridge (§8.5) | `<claude dir>/ide/<port>.lock` (0600, dir created 0700) per Claude session; `openDiff` proposals in `<runtime>/ide/<random>/` (0700) while shown | same |

`KELTA_RUNTIME_DIR` (both OSes) replaces the runtime dir and makes the instance skip the single-instance handshake, so a bench or test instance never talks to the user's running Kelta.

`<sid8>` = last 8 hex chars of the session uuid (the random tail of the v7 uuid; its first 32 bits are the millisecond clock), reserved by core under one lock (collision-checked). kelta-work names the runtime dirs of the sessions it spawns (Claude, editor) itself — `<runtime>/s/<key8>/`, recorded in its saga journal and `WorkItem.nvim_socket` — so core never assumes `<runtime>/s/<sid8>/` for work-item sessions. All socket paths are asserted < 100 bytes at startup.

---

## 3. Crates and module map

```
crates/kelta-proto      [scaffold]  ids, DTOs, IPC request/response types, UiEvent/BusEvent, settings structs (+Default, JsonSchema),
                                    plugin manifest/tool/trigger types, hook payloads, frame consts, ActionId catalog,
                                    service traits (§4), KeltaError, `term::command` (every child process, §7.2),
                                    `i18n` (fr/de table for text outside the webview)
crates/kelta-proto-testing  (dev-dep)  FakeCore, FakeTerminalHost, FakeTracker, FakeCodeHost, FakeSecrets, FakeSettings, in-memory stores,
                                    provider conformance contract, fixtures::load
crates/kelta-term       [L1]  TerminalHost impl: PtyBackend (portable-pty | rustix), login-env exec, reader threads, alacritty model,
                              query responder, snapshot encoder, flow control, scrollback memory budget;
                              `daemon`: the `keltad` binary and the `DaemonTerminalHost` client
crates/kelta-config     [L4]  layered load/merge/provenance, validation, toml_edit writes, hot reload, repo trust check helpers,
                              project files CRUD, early `linux.graphics` reader
crates/kelta-secrets    [L4]  SecretRef resolution chain, keyring stores, backend status
crates/kelta-http       [L5]  shared reqwest client (`client_builder`: installs the rustls ring provider), HttpCtx (retry,
                              backoff, rate limits, ETag LRU), markdown→HTML
crates/kelta-trackers   [L5]  Jira Cloud + DC, Redmine, GitHub Issues (+Projects v2), GitLab Issues, Gitea/Forgejo Issues, Linear; ADF walker
crates/kelta-codehosts  [L5]  GitHub (GraphQL list, REST actions, notifications gate), GitLab (REST), Bitbucket Cloud (REST 2.0), Gitea/Forgejo (REST v1)
crates/kelta-work       [L6]  git CLI ops, templates (branch/path/slug), work saga (journaled), Claude launcher, editor adapters
                              (nvim msgpack-RPC, vim keys, emacsclient, helix, external), review_start
crates/kelta-server     [L7]  ctl socket server, hook ingestion + status machine, lazy axum server, MCP endpoints
                              (`/mcp/<sid>`, `/mcp`), http hooks, Claude IDE bridge (§8.5)
crates/kelta-ctl        [L7]  CLI binary
crates/kelta-plugins    [L8]  manifest load/validate/install, registry, grants, plugin_call gate, kelta-plugin:// handler, tools
                              registry + web-tool lifecycle + proxy, trigger engine, KPP provider processes (`kpp`,
                              depends on kelta-http as a provider host)
crates/kelta-core       [L3]  AppState composition, ProjectRegistry, SessionRegistry, Layout store, Store (sqlite + migrations),
                              EventBus, Attention, Scheduler, ProviderRegistry + aggregation + caches + seen_reviews,
                              Notifier, clipboard, implements CoreApi; Claude Code integration: `claude_mcp` (global `/mcp`
                              registration), `outside` + `outside_jump` (sessions Kelta did not start, #345/#346),
                              `claude_host` (external terminal / tmux, #347), `claude_runs` (§7.7), `workflows`, `setup`;
                              `tags` (#349), `start` (last start options, #357), `park`, `team`
apps/desktop/src-tauri  [scaffold: Cargo.toml, build.rs, tauri.conf.json, capabilities/, main.rs, lib.rs, commands/mod.rs]
   src/commands/<domain>.rs  [owner per §6 table]; `names.rs` lists every command (build.rs makes the `allow-<command>`
                             permissions from it; tests keep it, `generate_handler!` and `capabilities/main.json` in sync)
   src/platform/**           [L10] pre_init (linux graphics env, NVIDIA detect, mallopt, crash guard), diagnostics probes,
                             updater client (`update.rs`), window geometry, dock badge
   src/window/**             [L10] window setup, decorations, menu, background mode, detached tab windows (#342), UiBridge impl
                             (bridge.rs: events fan-out, badge, notifications), webview crash + hang reload
xtask                   [scaffold] `codegen` (ts-rs + schema), `codegen --check`, `kpp-check` (PLUGINS §9.5)
bench                   [L10] kelta-bench
ui/                     Svelte app (ownership in BUILD_PLAN)
packages/plugin-sdk     [L8] @kelta/plugin-sdk
```

Dependency rule: every lane crate depends only on `kelta-proto`, plus `kelta-http` for the providers (trackers, codehosts) and `kelta-plugins` (KPP), and `kelta-work` for `kelta-server` (`kelta_work::files::ticket_markdown`, MCP ticket tools). `kelta-ctl` depends on no Kelta crate (std, serde, serde_json, rustix). `kelta-core` depends on all crates and wires them; `apps/desktop` depends on `kelta-core`, `kelta-config`, `kelta-plugins` (the `kelta-plugin://` handler) and `kelta-proto`. No crate other than desktop links Tauri or GTK/WebKit (`webkit2gtk`, Linux only).

---

## 4. Service traits (kelta-proto `api` module) — the inter-lane contract

All async traits use `#[async_trait]`, `Send + Sync`, return `Result<T, KeltaError>`.

```rust
// ---- errors -------------------------------------------------------------
#[derive(Serialize, Deserialize, TS, thiserror::Error)]
pub struct KeltaError { pub code: ErrorCode, pub message: String,
  pub detail: Option<serde_json::Value>, pub retry_after_ms: Option<u64> }
pub enum ErrorCode { NotFound, InvalidArgument, Conflict, PermissionDenied, NeedsAuth, RateLimited,
  Network, Upstream, Timeout, Unsupported, Untrusted, NeedsFields, Dirty, Cancelled, Internal }
// IpcError on the wire == KeltaError. (Scaffold stubs returned Unsupported("not implemented: <fn>"); none remain.)

// ---- terminal (impl: kelta-term) ---------------------------------------
pub trait TerminalHost: Send + Sync {
  fn spawn(&self, spec: PtySpawnSpec) -> Result<(), KeltaError>;            // id inside spec
  fn attach(&self, id: &SessionId, cols: u16, rows: u16, sink: Box<dyn FrameSink>) -> Result<AttachInfo, KeltaError>;
  fn detach(&self, id: &SessionId, generation: u32);
  fn write(&self, id: &SessionId, bytes: &[u8]) -> Result<(), KeltaError>;
  fn resize(&self, id: &SessionId, cols: u16, rows: u16) -> Result<(), KeltaError>;
  fn ack(&self, id: &SessionId, generation: u32, bytes: u32);
  fn kill(&self, id: &SessionId, signal: KillSignal) -> Result<(), KeltaError>; // Hup|Term|Kill, to process group; on an exited session: close (drop it)
  fn set_palette(&self, palette: TerminalPalette);                              // OSC 4/10/11/12 replies
  fn set_limits(&self, limits: TerminalLimits);                                 // scrollback per kind, memory cap, view_scrollback
  fn text_tail(&self, id: &SessionId, max_lines: u32) -> Result<String, KeltaError>; // plain text (search, persistence)
  fn history_tail(&self, id: &SessionId, max_lines: u32) -> Result<String, KeltaError>; // on-disk log (§9.6), also for sessions not running
  fn history_search(&self, ids: &[SessionId], query: &str, limit: u32) -> Result<Vec<HistoryHit>, KeltaError>;
  fn history_delete(&self, id: &SessionId);                                     // session row gone
  fn stats(&self) -> TerminalStats;                                             // per-session bytes, lines, inflight
  fn persistent(&self) -> bool { false }                                        // keltad: quit leaves restorable sessions running
  fn shutdown(&self) {}                                                         // keltad: kill every session and exit (quit with "stop all sessions")
  fn adopt(&self, id: &SessionId, events: Arc<dyn TerminalEvents>) -> Option<BTreeMap<String,String>> { None } // re-route a running session's events after an app restart; its spawn env
}
pub trait FrameSink: Send { fn send(&mut self, frame: Vec<u8>) -> bool; }  // false = channel closed → auto-detach
pub trait TerminalEvents: Send + Sync { fn on_event(&self, id: &SessionId, ev: TerminalEvent); }
pub enum TerminalEvent { Title(String), Cwd(PathBuf) /*OSC 7*/, Bell, Notify{title: Option<String>, body: String} /*OSC 9/777*/,
  ClipboardStore{kind: ClipboardKind, text: String}, ClipboardLoad{kind: ClipboardKind}, Activity /*first output since mark_seen*/,
  Exited{code: Option<i32>, signal: Option<i32>}, AckTimeout{generation: u32},
  MemoryCapReached{cap_mb: u32} /*§9.5, once per host; core raises a toast*/ }
pub struct PtySpawnSpec { pub id: SessionId, pub program: PathBuf /*absolute, resolved*/, pub args: Vec<String>,
  pub cwd: PathBuf, pub env: BTreeMap<String,String> /*complete env*/, pub cols: u16, pub rows: u16,
  pub scrollback_lines: u32, pub kind: SessionKind, pub events: Arc<dyn TerminalEvents> }

// ---- providers (impl: kelta-trackers / kelta-codehosts) ----------------
pub trait Tracker: Send + Sync { /* §8.1 */ }
pub trait CodeHost: Send + Sync { /* §8.2 */ }
// ProviderFactory lives in kelta_http::provider (re-exported as kelta_http::ProviderFactory), not in proto:
// it takes kelta_http::HttpCtx, and kelta-http depends on kelta-proto.
pub trait ProviderFactory: Send + Sync {   // impl in kelta-trackers + kelta-codehosts (two impls, core picks by kind)
  fn tracker(&self, account: &AccountConfig, http: HttpCtx, secrets: Arc<dyn SecretResolver>) -> Result<Arc<dyn Tracker>, KeltaError>;
  fn code_host(&self, account: &AccountConfig, http: HttpCtx, secrets: Arc<dyn SecretResolver>) -> Result<Arc<dyn CodeHost>, KeltaError>;
}

// ---- config/secrets (impl: kelta-config / kelta-secrets) ---------------
pub trait SettingsSource: Send + Sync {
  fn effective(&self, project: Option<&ProjectId>) -> Arc<Settings>;     // fully merged, validated
  fn project(&self, id: &ProjectId) -> Option<Arc<ProjectConfig>>;
  fn projects(&self) -> Vec<Arc<ProjectConfig>>;
}
pub trait SecretResolver: Send + Sync {
  async fn resolve(&self, r: &SecretRef, ctx: &SecretCtx) -> Result<Secret, KeltaError>; // Secret zeroizes on drop, !Serialize
  async fn set(&self, r: &SecretRef, value: &str) -> Result<(), KeltaError>;             // keyring refs only
  async fn delete(&self, r: &SecretRef) -> Result<(), KeltaError>;
  async fn backends_status(&self) -> Vec<SecretBackendStatus>;
  fn invalidate(&self, r: &SecretRef);
}

// ---- the core API (impl: kelta-core) consumed by work, server, plugins -
#[async_trait] pub trait CoreApi: Send + Sync {
  // sessions & layout
  async fn session_spawn(&self, req: SpawnRequest) -> Result<SessionInfo, KeltaError>;
  async fn session_write(&self, id: &SessionId, bytes: &[u8]) -> Result<(), KeltaError>;
  async fn session_kill(&self, id: &SessionId, force: bool) -> Result<(), KeltaError>;
  fn session_get(&self, id: &SessionId) -> Option<SessionInfo>;
  fn session_list(&self, project: Option<&ProjectId>) -> Vec<SessionInfo>;
  async fn session_apply_hook(&self, id: &SessionId, change: StatusChange) -> Result<(), KeltaError>;
  async fn session_set_usage(&self, id: &SessionId, usage: ClaudeUsage) -> Result<(), KeltaError>;   // statusline snapshot
  fn session_ask(&self, id: &SessionId, question: String, options: Vec<String>, body_md: Option<String>) -> Result<(), KeltaError>; // MCP ask, #356
  fn session_claude_runs_hook(&self, id: &SessionId, payload: &HookPayload);    // §7.7, + remembered permission mode
  fn claude_permission_mode(&self) -> Option<PermissionMode>;
  async fn layout_open(&self, project: &ProjectId, req: OpenPaneRequest) -> Result<PaneRef, KeltaError>; // new tab / split / focus
  // projects & settings
  fn project(&self, id: &ProjectId) -> Option<ProjectInfo>;
  fn settings(&self, project: Option<&ProjectId>) -> Arc<Settings>;
  // MCP setup tools (#343)
  fn project_list(&self) -> Vec<ProjectInfo>;
  fn project_add(&self, path: &Path) -> Result<(ProjectInfo, ProjectDraft), KeltaError>;
  fn settings_write(&self, layer: Layer, project: Option<&ProjectId>, path: &str, value: Value) -> Result<(), KeltaError>;
  async fn account_test(&self, account: &AccountId) -> Result<AccountTestResult, KeltaError>;
  // providers
  async fn tracker_for(&self, account: &AccountId) -> Result<Arc<dyn Tracker>, KeltaError>;
  async fn code_host_for(&self, account: &AccountId) -> Result<Arc<dyn CodeHost>, KeltaError>;
  async fn review_list(&self, scope: Scope, kind: ReviewKind) -> Result<Vec<ReviewItem>, KeltaError>;
  // tracker writes with core-owned side effects (cache invalidation, ticket.transitioned / ticket.commented);
  // `session` = the acting session (MCP tools), added as event context
  async fn ticket_transition(&self, ticket: &TicketRef, transition_id: &str, fields: Option<Value>, session: Option<&SessionId>) -> Result<Ticket, KeltaError>;
  async fn ticket_comment(&self, ticket: &TicketRef, markdown: &str, session: Option<&SessionId>) -> Result<(), KeltaError>;
  // MCP board tools: cached lists (tracker_list), description edit, close (default: unsupported / the tracker's)
  async fn ticket_list(&self, scope: Scope, view_id: Option<String>, cursor: Option<Cursor>, refresh: bool) -> Result<TicketPage, KeltaError>;
  async fn ticket_set_body(&self, ticket: &TicketRef, body_md: &str) -> Result<(), KeltaError>;
  async fn ticket_close(&self, ticket: &TicketRef) -> Result<Ticket, KeltaError>;
  async fn ticket_by_key(&self, key: &str, account: Option<&AccountId>) -> Result<Option<TicketItem>, KeltaError>; // MCP by-key tools
  async fn local_tags(&self) -> Result<LocalTags, KeltaError>;                  // MCP list_tickets `tag` (#349)
  // plugin screens / actions: Kelta's detail view of a ticket, assignment, a cached PR
  async fn ticket_get(&self, ticket: &TicketRef) -> Result<TicketDetail, KeltaError>;
  async fn ticket_assign(&self, ticket: &TicketRef, who: Assignee) -> Result<Ticket, KeltaError>;
  async fn review(&self, r: &ReviewRef) -> Option<Review>;
  // work & editor (core delegates to kelta-work)
  async fn work_for_session(&self, id: &SessionId) -> Option<WorkItem>; // the unfinished item first (#497)
  async fn ticket_for_session(&self, id: &SessionId) -> Option<TicketRef>;  // MCP get_ticket: the work item's ticket, or a running refine's (T10); default via work_for_session
  async fn work_get(&self, id: &WorkItemId) -> Option<WorkItem>;
  async fn work_list(&self, project: Option<&ProjectId>) -> Result<Vec<WorkItem>, KeltaError>;
  async fn work_create_pr(&self, id: &WorkItemId, draft: PrDraft, origin: ShipOrigin /*Ui|Mcp*/) -> Result<WorkItem, KeltaError>;
  async fn work_feedback(&self, id: &WorkItemId) -> Result<Feedback, KeltaError>;  // MCP get_review_feedback
  async fn work_fix_checks(&self, review: &Review) -> Result<String, KeltaError>;  // `fix_checks` trigger action (#252)
  async fn workflow_start(&self, project: Option<&ProjectId>, workflow: &str, ticket: Option<TicketRef>) -> Result<WorkflowStart, KeltaError>;  // `run_workflow` trigger action (#488): workflow_run on ticket, else workflow_run_on global
  // MCP background-run tools (#235; default: unsupported)
  async fn work_claim(&self, project: &ProjectId, ticket: TicketRef, worktree: Option<PathBuf>, branch: Option<String>, run: RunClaim, adopt: bool) -> Result<WorkItem, KeltaError>;
  async fn work_report(&self, key: &str, phase: Option<String>, state: RunState, note: Option<String>, sha: Option<String>) -> Result<WorkItem, KeltaError>;
  async fn work_release(&self, key: &str, sha: Option<String>) -> Result<WorkItem, KeltaError>;
  async fn work_link_pr(&self, project: &ProjectId, ticket: TicketRef, pr: Review) -> Result<(WorkItem, bool), KeltaError>;  // MCP link_pr (#453); false: linked already
  async fn ticket_step_set(&self, ticket: &TicketRef, step_id: Option<&str>) -> Result<TicketStep, KeltaError>;  // MCP move_ticket_step, Tauri ticket_step_set (#463); None = next step
  async fn editor_open(&self, target: EditorTarget, path: &Path, line: Option<u32>) -> Result<(), KeltaError>;
  async fn editor_diff(&self, target: EditorTarget, old: &Path, proposed: &Path, close: bool) -> Result<(), KeltaError>; // IDE bridge openDiff, nvim RPC only (§8.5)
  // tools/plugins/bus/ui
  async fn tool_open(&self, project: &ProjectId, tool: &ToolId, ctx: TemplateCtx, placement: Placement) -> Result<ToolHandle, KeltaError>;
  fn publish(&self, ev: BusEvent);
  fn subscribe(&self) -> tokio::sync::broadcast::Receiver<BusEvent>;
  async fn notify(&self, n: Notification) -> Result<(), KeltaError>;
  fn toast(&self, t: Toast);
  async fn clipboard_copy(&self, text: String) -> Result<(), KeltaError>;      // work_send to a Claude without Kelta's hooks
  fn login_path(&self) -> Option<String>;                                       // the login shell's PATH (plugin exec)
  async fn http_fetch(&self, req: ProxiedRequest) -> Result<ProxiedResponse, KeltaError>; // plugin net: (allowlist checked by caller); never follows redirects (own reqwest client in core)
  async fn ctl(&self, cmd: CtlCommand) -> Result<serde_json::Value, KeltaError>;            // dispatch of ctl socket commands
}

// ---- UI bridge (impl: apps/desktop window/bridge.rs, L10) consumed by kelta-core --------------
pub trait UiBridge: Send + Sync {
  fn emit(&self, ev: UiEvent);                          // fan-out to all events_subscribe channels; dropped if none
  fn set_badge(&self, needs_input: u32);                // macOS dock badge; Linux no-op
  fn request_attention(&self);                          // Linux urgency hint / macOS bounce (informational)
  fn notify(&self, n: Notification) -> Result<(), KeltaError>;  // tauri-plugin-notification
  fn window_state(&self) -> WindowState;                // {exists, visible, focused}, any-of over the windows (#342)
  fn main_window_visible(&self) -> bool;                // the main window alone shows the active tab
  fn reload_webview(&self, safe: bool);                 // hang/crash recovery (§12.4)
  fn webview_pids(&self) -> Vec<u32> { Vec::new() }     // WebKit helper pids for perf_snapshot (macOS)
}
// Services owned by other lanes that core calls (constructor signatures frozen in scaffold stubs):
// (exact signatures: BUILD_PLAN §2.2)
//   kelta_work::WorkService::new(core: Weak<dyn CoreApi>, store: Arc<dyn WorkStore>, dirs: Dirs) ; methods mirror §6 work_* commands (+ for_session)
//       core calls set_host(Arc<dyn kelta_work::WorkHost>) at boot: {ensure_http, release_http} over Server, run_blocking over PluginHost;
//       claude_restore_request(session, fallback) builds the argv when core restores a Dormant work-item Claude session
//   kelta_server::Server::new(core: Weak<dyn CoreApi>, dirs: Dirs) ; start_ctl(), ensure_http() -> port, ensure_http_at(preferred), register_session(sid, hook_token, mcp_token), set_install_token(token) (global `/mcp`)
//   kelta_plugins::PluginHost::new(core: Weak<dyn CoreApi>, dirs: Dirs, grants: Arc<dyn GrantStore>) ; tools, triggers, screens,
//       uri::handle(host: &PluginHost, req) (the handler needs the plugin directories)
//       core calls wire(Wiring{ui, settings, settings_writer}) at boot and start() (trigger engine's own bus subscription)
//       once the runtime is up; triggers run without a UI
//   kelta_config::ConfigService::load(dirs: &Dirs, overrides: RuntimeOverrides) ; impl SettingsSource ; watch(on_change) ; set_plugin_schemas
//   kelta_secrets::Secrets::new(settings: Arc<dyn SettingsSource>) ; impl SecretResolver
//   kelta_term::PtyTerminalHost::new(env: LoginEnv, limits: TerminalLimits) ; impl TerminalHost ; kelta_term::resolve_login_env(timeout)
```

`WorkStore`, `GrantStore`, `TrustStore` are small traits in proto implemented by kelta-core over SQLite (fakes in `kelta-proto-testing`).
`PluginSettingsSource { fn fragments(&self) -> Vec<(PluginId, serde_json::Value /*flat schema*/)> }` is implemented by `PluginHost` and handed by core to `ConfigService::set_plugin_schemas` (Plugin-defaults layer + validation of `plugins.<id>`). `Dirs { config, data, state, logs, runtime, bin }` is defined and resolved in proto (`Dirs::resolve(env, overrides)`, per §2.1) so every crate shares it.

---

## 5. Domain model (kelta-proto, all `Serialize + Deserialize + TS`)

```rust
pub struct ProjectId(String);   // slug [a-z0-9-]{1,40}; reserved: "home" (built-in Home), "inbox" (pseudo id), "global" (the global views' layout; a pre-#270 project `global` is refused, its tabs moved to `global-project`)
pub struct SessionId(String);   // uuid v7
pub struct WorkItemId(String);  // uuid v7
pub struct AccountId(String);   // settings key, e.g. "jira-acme"
pub struct TabId(String); pub struct PaneId(String); pub struct ToolInstanceId(String); pub struct ScreenInstanceId(String);
pub struct PluginId(String);    // [a-z0-9-]{3,40}
pub struct ToolId(String);      // "lazydocker" | "<plugin>/<tool>"

pub struct ProjectInfo { id, name, color: Option<String>, icon: Option<String>, repos: Vec<RepoInfo>,
  tracker: Option<TrackerBinding>, open: bool, active: bool, attention: AttentionSummary, builtin: bool /*home*/ }
pub struct RepoInfo { id: String, path: PathBuf, primary: bool, remote: String, base: String,
  code_host: Option<CodeHostBinding>, exists: bool }
pub struct TrackerBinding { account: AccountId, views: Vec<TrackerView>, columns: Vec<ColumnSpec>, status_map: StatusMap }
pub struct CodeHostBinding { account: AccountId, repo: String /* "acme/shop" | "grp/sub/proj" */ }

pub enum SessionKind { Shell, Claude, Editor{ adapter: String }, Tool{ tool_id: ToolId }, Setup, Custom }
pub enum SessionStatus { Starting, Running, Working, NeedsInput, WaitingUser, Done, Error, Exited, Unknown }
pub enum Attention { None = 0, Activity = 1, Done = 2, Error = 3, NeedsInput = 4 }      // ordered; max-aggregated
pub enum Lifecycle { Dormant, Live, Exited }
pub enum RestorePolicy { None, Relaunch, ShellInCwd, ClaudeResume{ uuid: String, profile: Option<String> /*#318*/ }, Editor{ session_file: Option<PathBuf> } }
pub enum CloseOnExit { Never, OnSuccess, Always }
pub struct SpawnRequest { id: Option<SessionId> /*caller-chosen (files naming it are written first); must be unused → else Conflict*/,
  project_id: ProjectId, kind: SessionKind, name: Option<String>,
  program: Option<String> /*None = $SHELL*/, args: Vec<String>, cwd: Option<PathBuf>, env: BTreeMap<String,String>,
  cols: u16, rows: u16, work_item_id: Option<WorkItemId>, restore: RestorePolicy, close_on_exit: CloseOnExit,
  template_id: Option<String> }
pub struct SessionInfo { id, project_id, kind, name: String, title: Option<String>, cwd: PathBuf, status: SessionStatus,
  status_source: StatusSource /*Hook|Heuristic|None*/, attention: Attention, seen: bool, lifecycle: Lifecycle,
  pid: Option<u32>, exit_code: Option<i32>, work_item_id: Option<WorkItemId>, claude: Option<ClaudeMeta>,
  editor: Option<EditorMeta>, cols: u16, rows: u16, created_at: String /*RFC3339*/ }
pub struct ClaudeMeta { session_uuid: String, model: Option<String>, preview: Option<String>, files_touched: Vec<PathBuf>, hooks_active: bool,
  tasks: Vec<ClaudeTask{id: Option<String>, title, state: Pending|InProgress|Completed}> /*Claude's task list from its hooks, #406*/,
  usage: Option<ClaudeUsage> /*last statusline snapshot, PLUGINS §8*/, question: Option<ClaudeQuestion{question, options, body_html}> /*open MCP `ask`, #356*/,
  host: Option<ClaudeHost> /*Terminal|Multiplexer: Claude runs outside the pane, #347; absent = builtin*/ }
pub struct EditorMeta { adapter: String, socket: Option<PathBuf> }
pub struct AttachInfo { generation: u32, cols: u16, rows: u16 }
pub struct StatusChange { status: SessionStatus, preview: Option<String>, file_edited: Option<PathBuf>, raw_event: String }
// status == Unknown means "unchanged" (e.g. PostToolUse): core keeps the previous status.

pub enum Scope { Project{ id: ProjectId }, All }
pub struct WorkItem { id, project_id, kind: WorkKind /*Ticket|Review|Branch*/, ticket: Option<TicketRef>, review: Option<ReviewRef>,
  repo_id: String, worktree: PathBuf, branch: String, base: String, claude_uuid: Option<String>, nvim_socket: Option<PathBuf>,
  session_ids: Vec<SessionId>, tab_id: Option<TabId>, pr_url: Option<String>, state: WorkState, steps: Vec<WorkStepStatus>,
  created_at: String, review_due: bool, claude_replied: bool,
  title: Option<String> /*scratch: task's first line, ≤72 chars*/, pr_title_needs_key: bool /*set by work_link when a PR exists; the next Ship/Push prefixes the key (kelta_work::pr_title_with_key) unless the title has one*/,
  sent_threads: Vec<String> /*thread ids the last Fix with Claude handed over*/,
  feedback_at: Option<String> /*watermark: newest comment/review time the last Fix with Claude showed; newer = New comments (FLOW §2.2 row 11a)*/,
  rebase: Option<RebaseState{onto, pre_head, remote_sha: Option<String>, conflicts: Vec<PathBuf>, step: u32, total: u32}>,
  claude_at, cost_usd, claude_message, delta: Option<ReviewDelta>, next_note, left_at, port_base, auto_finish, parked_at, nvim_kept,
  run: Option<RunClaim> /*background run, §2*/, ticket_source: Option<String> }
pub enum WorkSource { Ticket{ ticket }, Review{ review }, Branch{ name, task: Option<String>, repo: Option<String> } } // Branch with empty name: name from work.scratch_branch_template + task slug; task is the {task} of claude.prompt_templates.standalone; no tracker steps
// review_due / claude_replied (FLOW §2.3): set only by kelta-work from a real `Stop` hook of the item's
// Claude (each `Stop` snapshots the whole working tree — tracked plus untracked non-ignored, through a
// temporary index, HEAD/index/stash untouched — to `refs/kelta/wi/<id>/last`; a non-empty
// `git diff <reviewed> <last>` (`refs/kelta/wi/<id>/reviewed`, else the merge base with <base>) → review_due
// + `delta` (ReviewDelta chip), else claude_replied; never for review checkouts; hooks of one session apply
// in order; `claude_message` keeps the full last message). Both refs are deleted by Finish and the startup prune, cleared by `UserPromptSubmit`; review_due also by a UI `work_create_pr` (`origin = ui`; MCP `create_pr` is `origin = mcp`), Finish and
// `work_mark_reviewed`; claude_replied by Finish and `work_seen`. Store writes: `save` never writes these two (it keeps the
// stored values); `update(id, |w| ..)` re-loads under a write lock held only around load-modify-save and is
// their only writer. Long operations (`create_pr`, `finish`) end with `update` of their own fields.
// claude_uuid follows the session id of any hook of the item's Claude (/clear, in-Claude /resume).
pub enum WorkState { Planned, Starting, Queued{ pos: i32 } /*waiting for `claude.max_live`, #141*/, Active, PrOpen, Merged{ detail: Option<String> /*"choose Done status" | failed move*/ },
  PrClosed /*closed unmerged*/, Finished, Failed{ step: String, message: String } }
```

### 5.1 Layout model

```rust
pub struct Layout { project_id: ProjectId, tabs: Vec<Tab>, active_tab: Option<TabId>, rev: u64 }
pub struct Tab { id: TabId, title: String, work_item_id: Option<WorkItemId>, root: LayoutNode,
  focused_pane: Option<PaneId>, zoomed_pane: Option<PaneId>, detached: bool /*shown in its own window, #342*/ }
pub enum LayoutNode { Split{ dir: SplitDir /*Row|Column*/, ratios: Vec<f32> /*sum 1.0, each ≥ 0.05*/, children: Vec<LayoutNode> },
                      Pane{ id: PaneId, content: PaneContent } }
pub enum PaneContent {                       // serde tag = "kind"
  Terminal{ session_id: SessionId }, ClaudeRuns{ session_id: SessionId } /*#392*/, Web{ tool_instance_id: ToolInstanceId },
  PluginScreen{ plugin_id: PluginId, screen_id: String, instance_id: ScreenInstanceId, params: serde_json::Value },
  Tickets{ scope: Scope, view_id: Option<String>, source_id: Option<String>, mode: TicketsMode /*List|Board*/, who: Option<Who>, group: Option<TicketGroupBy> /*Flow|Status|Priority|Sprint|Assignee|Source|None; None = Flow*/, sort: Option<TicketSort> /*Priority|Updated|Age|Key; None = Priority*/, person: Option<String> /*assignee id, client-side*/ }, TicketDetail{ ticket: TicketRef },
  Reviews{ scope: Scope, tab: Option<ReviewKind>, density: Option<RowDensity> /*Comfortable|Compact; None = Comfortable*/ }, ReviewDetail{ review: ReviewRef }, Inbox{ density: Option<RowDensity> }, WorkItem{ id: WorkItemId },
  Settings{ section: Option<String> }, Diagnostics, Welcome, Empty }
pub struct OpenPaneRequest { content: PaneContent, placement: Placement, focus: bool, tab_title: Option<String>, work_item_id: Option<WorkItemId> }
pub enum Placement { NewTab, SplitRight, SplitDown, ReplaceFocused, Focused /*focus existing pane with same content if any*/ }
```

- Window = ProjectRail (Inbox, open projects, Home, "+") + active project workspace (LayoutNode tree) + StatusBar, under the ContextBar.
- Only the active project workspace is mounted. A session is shown in **at most one pane**; opening it elsewhere moves it.
- Layout is saved by the UI via `layout_save` debounced 500 ms after a change (timer exists only while dirty) with optimistic `rev` (stale rev → `Conflict`, UI refetches). The other windows get the saved layout as `layout.changed`.
- Project switch never touches processes; detached xterm instances go to the LRU (§9.4).

---

## 6. IPC command catalogue (Tauri commands)

Conventions: every command is `async`, takes one argument object (TS: `invoke('<name>', { ... })`), returns `Result<T, KeltaError>`. TS wrappers in `ui/src/lib/ipc/commands.ts` (scaffold, generated names + typed signatures), mocks in `ui/src/lib/ipc/mock.ts`. Binary payloads (`session_write` data, frames) use `Uint8Array` / `tauri::ipc::Response` / `Channel<InvokeResponseBody>`.

Wire format (frozen by the scaffold, checked by the fixture round-trips): enums serialize in `snake_case` (`ErrorCode` → `"not_found"`, `SessionStatus` → `"needs_input"`), except `PermissionMode` (Claude's camelCase values), `ClaudeEffort` (lowercase), `Osc52`/`ShiftEnter` (kebab-case), `PluginMethod` (`"tickets.list"`) and `Permission` (its string form). Tagged unions: `UiEvent`, `SessionKind`, `LayoutNode` tag `type`; `PaneContent`, `Scope`, `RestorePolicy`, `WorkState`, `WorkSource`, `ToolHandle`, `EditorTarget`, `Assignee` tag `kind`; `Cursor` adjacently tagged `{kind, value}`; `ActionDef` tag `action`; `CtlCommand` tag `cmd`. `Option` fields serialize as `null` (TS `T | null`; missing keys read as `None`); `u64`/`i64` are TS `number`. Small result DTOs not spelled out below (`SubscribeResult{sub_id}`, `LayoutSaveResult{rev}`, `ScreenOpenResult{instance_id, url}`, `PaneRef`, `ProjectPatch`, …) live in `kelta-proto`. The TS wrappers are camelCase functions taking the snake_case argument object (`projectClose({id, kill_sessions})`).

Every command below is listed in `apps/desktop/src-tauri/src/commands/names.rs` and granted as `allow-<command>` in `capabilities/main.json` (windows `["main", "detached-*"]`, local origin only); the same capability adds `core:default`, the window controls the custom decorations need (drag, minimize, toggle maximize, close, destroy, resize drag, set title) and `dialog:allow-open`.

| Command | Args | Returns | Owner file |
|---|---|---|---|
| **app** | | | `commands/app.rs` (L3) |
| `app_info` | `{}` | `AppInfo{version, platform, arch, data_dir, config_dir, runtime_dir, claude: Option<ToolVersion>, safe_graphics: bool, decorations: Decorations}` (`decorations` = what the window was built with: `native`/`none`/`custom`; `custom` → the UI draws the drag strip + resize handles) | |
| `app_ready` | `{t_ms: f64}` | `()` (desktop clears the launch crash guard and writes the `app_ready_ms` bench mark; core binds the runtime) | |
| `bench_mark` | `{key: String, value: f64}` | `()` (merges a UI-measured kelta-bench metric into `KELTA_BENCH_MARKS`; no-op without it) | |
| `events_subscribe` | `{channel: Channel<UiEvent>}` | `{sub_id: u64}` (one per window) | |
| `open_external` | `{url: String}` (http/https/mailto only) | `()` | |
| `perf_snapshot` | `{}` | `PerfSnapshot{processes: Vec<ProcMem{pid, name, role: Core|WebContent|Network|Gpu|Child, pss_or_footprint_kb}>, sessions: Vec<SessionMem>, live_views: u32, timers_armed: u32, http_server: bool}` | |
| `diagnostics_run` | `{}` | `Diagnostics{checks: Vec<Check{id, label, status: Ok|Warn|Fail, detail, fix: Option<String>}>}` | `commands/diagnostics.rs` (L10) |
| `clipboard_read` | `{kind: Clipboard|Primary}` | `String` | `commands/clipboard.rs` (L3) |
| `clipboard_write` | `{kind, text}` | `()` | `commands/clipboard.rs` (L3) |
| `notify_test` | `{}` | `()` | `commands/app.rs` (L3) |
| `notify_focus` | `{on: bool}` | `Vec<String>` (focus mode, #421: holds desktop notifications but a blocked Claude's while `notifications.focus_allow_blocked`; off returns the titles held) | `commands/app.rs` |
| `claude_mcp_status` | `{}` | `Option<bool>` (Kelta's MCP registered in Claude Code at user scope; `None` = Claude Code not found) | `commands/app.rs` |
| `claude_mcp_install` | `{}` | `()` (writes `mcpServers.kelta` in Claude Code's user config, opens the global `/mcp`) | `commands/app.rs` |
| `claude_setup` | `{}` | `SessionInfo` ("Set up with Claude", #343: `claude_mcp_install` when `kelta` is not registered, then a Claude session in Home told to configure Kelta through the MCP setup tools, in a new focused tab) | `commands/app.rs` |
| `claude_mcp_remove` | `{}` | `()` (drops `mcpServers.kelta`, and with it the install token) | `commands/app.rs` |
| `claude_hooks_status` | `{}` | `Option<bool>` (Kelta's hook relay in Claude Code's user settings; `None` = Claude Code not found, #345) | `commands/app.rs` |
| `claude_hooks_install` | `{}` | `()` (adds the relay's hook groups to Claude Code's user settings) | `commands/app.rs` |
| `claude_hooks_remove` | `{}` | `()` (drops the relay's hook groups, nothing else) | `commands/app.rs` |
| `outside_list` | `{}` | `Vec<OutsideSession>` (Claude sessions Kelta did not start; drops those whose process is gone) | `commands/app.rs` |
| `outside_jump` | `{id}` | `Option<String>` (selects the session's tmux pane, then focuses its terminal window; `None` when it did, else where it runs, #346) | `commands/app.rs` |
| `claude_permission_mode` | `{}` | `Option<PermissionMode>` (the last mode a Claude session switched to, used when the profile forces none; `None` = Claude's default) | `commands/app.rs` |
| `app_update_status` | `{}` | `UpdateStatus` (running version, channel, `state`, `checked_at`, `restart_needs_confirm`) | `commands/app.rs` |
| `app_update_check` | `{}` | `UpdateStatus` after checking and installing what it found ("Check now") | `commands/app.rs` |
| `app_restart` | `{}` | `()` (`request_restart`: the normal exit path, then the installed build starts) | `commands/app.rs` |
| `app_quit` | `{stop}` | `()` (the quit dialog's answer: `app.exit`, `stop` = `Core::stop_sessions_on_quit` first) | `commands/app.rs` |
| `app_close` | `{}` | `()` (the UI's close handler, after saving its layout and preventing the default close: the quit question per `app.quit_sessions`, else destroys the window) | `commands/app.rs` |
| `window_detach` | `{project_id, tab_id}` | `()` (opens or focuses the window of a tab already marked `detached`, #342, §2) | `commands/app.rs` |
| **settings** | | | `commands/settings.rs` (L4) |
| `settings_schema` | `{}` | `serde_json::Value` (flattened JSON Schema incl. `plugins.<id>`) | |
| `settings_effective` | `{project_id?}` | `EffectiveSettings{value: Value, sources: BTreeMap<String /*dotted path*/, Layer>}` | |
| `settings_layer_get` | `{layer: Layer, project_id?, repo_id?}` | `LayerDoc{path: PathBuf, value: Value, text: String, trusted: Option<bool>}` | |
| `settings_set` | `{layer, project_id?, repo_id?, path: String, value: Value}` | `EffectiveSettings` | |
| `settings_reset` | `{layer, project_id?, repo_id?, path}` | `EffectiveSettings` | |
| `settings_validate` | `{layer, text: String}` | `Vec<ValidationIssue{path, message, line?, col?}>` | |
| `settings_write_raw` | `{layer, project_id?, repo_id?, text}` | `EffectiveSettings` (validate first; refuse on error) | |
| `settings_open_file` | `{layer, project_id?, repo_id?}` | `SessionInfo` (opens `$EDITOR`/nvim pane on the file) | |
| `template_preview` | `{path, template}` | `TemplatePreview{text, git_ok?}` | live example under a template setting (#479): rendered for the sample ticket KEL-214 like Start work; `git_ok` only for `worktree.branch_template` |
| `repo_trust` | `{project_id, repo_id, trust: bool, sha256?}` | `TrustInfo{path, hash, trusted}` | `sha256` (of the text the user reviewed) is required to trust; `conflict` when the file changed since |
| `secret_set` | `{secret_ref: String, value: String}` | `()` (value never echoed/logged) | `commands/secrets.rs` (L4) |
| `secret_delete` | `{secret_ref}` | `()` | |
| `secret_backends_status` | `{}` | `Vec<SecretBackendStatus{backend, available, detail}>` | |
| `secret_unlock` | `{passphrase: String, create: bool}` | `()` (unlocks `file:` refs for this run; passphrase zeroized, never logged) | |
| `oauth_device_start` | `{kind: AccountKind, base_url, secret_ref}` | `OAuthDevicePrompt{user_code, verification_uri, expires_in}` (device code kept in core; client id from `oauth.client_ids`; `secret_ref` must be `keyring:`/`file:`) | |
| `oauth_device_finish` | `{user_code}` | `()` once approved and stored (grant with the tokens at `<secret_ref>.oauth`, access token copy at `secret_ref`); `timeout` expired, `cancelled` denied or cancelled | |
| `oauth_device_cancel` | `{user_code}` | `()`; stops that sign-in's polling (the wizard closed) | |
| `account_test` | `{account_id}` | `AccountTestResult{ok, user: Option<User>, error: Option<KeltaError>}` | `commands/settings.rs` (L4) via CoreApi |
| **projects** | | | `commands/project.rs` (L3) |
| `project_list` | `{}` | `Vec<ProjectInfo>` | |
| `project_detect` | `{path}` | `ProjectDraft{suggested_id, name, repos, code_host_hints, tracker_hints}` | |
| `project_create` | `{draft: ProjectDraft}` | `ProjectInfo` (writes `projects/<id>.toml` through kelta-config) | |
| `project_update` | `{id, patch: ProjectPatch}` | `ProjectInfo` | |
| `project_remove` | `{id, kill_sessions: bool}` | `()` (config file moved to `projects/.trash/`) | |
| `project_open` | `{id}` / `project_close` `{id, kill_sessions}` / `project_activate` `{id}` | `ProjectInfo` | |
| `project_reorder` | `{ids: Vec<ProjectId>}` | `()` | |
| **layout** | | | `commands/layout.rs` (L3) |
| `layout_get` | `{project_id}` | `Layout` | |
| `layout_save` | `{layout: Layout}` | `{rev: u64}` | |
| **sessions** | | | `commands/session.rs` (L3) |
| `session_spawn` | `{req: SpawnRequest}` | `SessionInfo` | |
| `session_spawn_template` | `{project_id, template_id, ctx: TemplateCtx, placement: Placement}` | `Vec<SessionInfo>` (+ layout update event) | |
| `session_attach` | `{id, cols, rows, channel: Channel<InvokeResponseBody>}` | `AttachInfo` — spawns Dormant sessions (lazy restore) | |
| `session_detach` | `{id, generation}` | `()` | |
| `session_write` | `{id, data: Uint8Array}` | `()` (fire-and-forget from UI). Wire: raw `Uint8Array` body + header `x-kelta-session-id` (preferred), or JSON `{id, data}` with `data` as number array, index-keyed object or string | |
| `session_resize` | `{id, cols, rows}` | `()` | |
| `session_ack` | `{id, generation, bytes: u32}` | `()` | |
| `session_kill` | `{id, force: bool}` | `()` | |
| `session_restart` | `{id}` | `SessionInfo` | |
| `session_rename` | `{id, name}` | `SessionInfo` | |
| `session_list` | `{project_id?}` | `Vec<SessionInfo>` | |
| `session_mark_seen` | `{id}` | `()` | |
| `session_link` | `{id, work_item_id?: WorkItemId, ticket?: TicketRef}` | `SessionInfo` | |
| `session_text_tail` | `{id, max_lines}` | `String` | Dormant: on-disk history log first (§9.6), else the tail stored at quit |
| `session_history_search` | `{project_id, session_id?, query, limit}` | `Vec<HistoryHit{session_id, line}>` | on-disk history log (§9.6) of one session or every session of the project; case-insensitive substring |
| `claude_runs_list` | `{id}` | `Vec<ClaudeRun>` | the session's Claude `Workflow` runs as core last read them (§7.7) |
| `claude_run_tail` | `{id, run_id, agent_id}` | `Vec<TranscriptLine{kind, text}>` | last 40 lines (prose, tool calls, results) of one agent's transcript, read from its last 256 KiB |
| `terminal_set_palette` | `{palette: TerminalPalette}` | `()` (pushed on theme change) | |
| **tickets** | | | `commands/tracker.rs` (L3) |
| `tracker_list` | `{scope, source_id?, view_id?, who?: Who, cursor?: Cursor, refresh: bool}` (`source_id`/`view_id` are the pane's raw picks, resolved once in core: `TicketPage.scope`/`view_id` say what the page lists and the UI renders that; `who` overrides `view.who` and has its own cache key; no `view_id` = union of the project's views, deduped by account + key) | `TicketPage{items: Vec<TicketItem{ticket, project_ids, view_ids, work_item_id?, prs: Vec<PrLink>, caps: TrackerCaps}>, next: Option<Cursor>, stale: bool, errors: Vec<AccountError>, people: Vec<Person>, scope: Scope, view_id?}` (`people`: who the page, the scope's cached Team list and the cached review lists mention, identities merged) | |
| `tracker_get` | `{ticket: TicketRef}` | `TicketDetail` (with `prs` and the account's `caps`) | |
| `tracker_columns` | `{source_id}` | `Vec<Column>` | |
| `tracker_fields` | `{source_id}` | `Vec<SourceField{id, label}>` (`Tracker::fields` of each view of the source, deduped by id: the picker for `Source.fields`, #351; Jira field ids, GitHub project field names) | |
| `tracker_transitions` | `{ticket}` | `Vec<Transition>` | one call per ticket; the UI's StatusPicker matches several tickets by target status name |
| `tracker_transition` | `{ticket, transition_id, fields?: Value}` | `Ticket` (`NeedsFields` error carries `detail.fields`) | |
| `tracker_move` | `{ticket, column_id, source_id?}` (no `source_id`: the board of the project bound to the ticket's account) | `Ticket` (resolves column → transition; `Conflict` + candidates if ambiguous) | |
| `tracker_comment` | `{ticket, markdown}` | `()` | |
| `tracker_assign` | `{ticket, assignee: Assignee /*Me|User{id}|None*/}` | `Ticket` | |
| `tracker_assignable_users` | `{ticket, query}` | `Vec<User>` (assignee picker; `Unsupported` if the provider cannot list) | |
| `tracker_priorities` | `{ticket}` | `Vec<String>` (priority names, highest first; `Unsupported` where priorities cannot be set) | |
| `tracker_set_priority` | `{ticket, priority}` | `Ticket` | |
| `tracker_set_blocked` | `{ticket, blocked}` | `Ticket` (sets or clears the source's blocked marker: a transition for `status`, else the provider's `set_blocked`; `Unsupported` when it cannot) | |
| `tracker_refine` | `{ticket, project_id?}` | `String` (Markdown proposal of a one-shot `claude -p` in the project's primary repo: acceptance criteria, open questions, sub-tasks; MCP `get_ticket` resolves to the ticket for that run) | |
| `tracker_sources` | `{account_id, query}` | `Vec<SourceHit>` (source picker; `Unsupported` if the provider cannot list) | |
| `workflows_list` | `{ticket}` | `Vec<WorkflowInfo>` (`name`, `label`, `description`, `path`, `source`: `repo`, `home` or `config`; MCP "Launch from Kelta") | |
| `workflow_run` | `{ticket, workflow}` | `WorkflowStart` (`session`, `running`: claims the ticket, then a background Claude session; the live run already holding it with `running: true`; `Conflict` when a work item holds it) | |
| `workflow_run_on` | `{project_id, workflow, review}` | `WorkflowStart` (a `[[workflows]]` entry, `on = "pr"` on `review` or `on = "global"` with `null`; one run per PR, #355) | |
| `tracker_search` | `{scope, text}` | `Vec<TicketItem>` (palette; searched by each tracker) | |
| `next_up_list` | `{}` | `NextUp{items: Vec<NextUpItem{project_id, ticket, rank?, snoozed_until?}>, seen: Vec<String>}` (my Next up list and snoozes, and the `account:key` of the tickets I have seen, #145; SQLite `next_up`, `seen_tickets`, never a tracker call) | |
| `next_up_put` | `{item: NextUpItem}` | `()` (adds, reorders by fractional `rank` or snoozes; `rank` null = snoozed only) | |
| `next_up_remove` | `{ticket}` | `()` | |
| `ticket_seen` | `{tickets: Vec<TicketRef>}` | `()` (clears their `New` badge) | |
| `ticket_step_set` | `{ticket, step_id?}` | `TicketStep` (manual step move, #463, FLOW §4.9: `null` = the step after the current one; forward runs the step's transition and comment, back runs nothing; `NotFound` without a work item, `InvalidArgument` for an unknown step or past the last) | |
| `tags_get` | `{}` | `LocalTags{tags: Vec<LocalTag{name, color?}>, tickets: Map<"account:key", Vec<name>>, prs: Map<"account:repo#number", Vec<name>>}` (my local tags, #349; `ui_state` key `tags`, never sent to a tracker or code host) | |
| `tags_set` | `{tags: LocalTags}` | `LocalTags` (replaces the whole set; returned as stored: names trimmed, unknown tags dropped) | |
| **reviews** | | | `commands/review.rs` (L3) |
| `review_list` | `{scope, kind: ReviewKind, refresh: bool}` | `ReviewPage{items: Vec<ReviewItem{review, project_ids}>, stale, errors}` | |
| `review_get` | `{review: ReviewRef}` | `ReviewDetail` | |
| `review_approve` | `{review, head_sha}` | `()` (`Conflict` if head moved) | |
| `review_comment` | `{review, body}` | `()` | |
| `review_request_changes` | `{review, body}` | `()` | publishes my pending line comments too (as do `review_approve` / `review_comment`) |
| `review_nudge` | `{review, who, comment?}` | `()` | my PR: re-request `who` (its `waiting_on`; `comment` null, `CodeHost::rerequest_review`; GitHub drops then re-adds a pending request so it notifies again) or post `comment`; `InvalidArgument` within 24 h of the last nudge (`nudges`) |
| `review_set_draft` | `{review, draft}` | `()` | my PR: mark ready for review (`draft` false) or back to draft (`CodeHost::set_draft`: GitHub GraphQL, GitLab `Draft:` / Gitea `WIP:` title prefix, Bitbucket `draft`); `Unsupported` on a plugin host |
| **work** | | | `commands/work.rs` (L6) |
| `work_plan` | `{project_id, source: WorkSource /*Ticket{ticket}|Review{review}|Branch{name, task?, repo?}*/, source_id?}` (`source_id`: the ticket's source from its row; none = guessed from the account) | `StartWorkPlan` (§SPEC 3.1) | Branch with a task (New work item): `Conflict` when the branch exists or has an item |
| `work_start` | `{plan: StartWorkPlan}` | `WorkItem` (progress via `work.updated`) | |
| `work_list` | `{project_id?}` | `Vec<WorkItem>` | |
| `work_resume` | `{id}` | `WorkItem` | |
| `work_retry_step` | `{id, step}` | `WorkItem` | `step` = a saga step id (re-run) or `skip:<step>` (mark skipped, continue) |
| `work_create_pr` | `{id, draft: PrDraft}` | `WorkItem` | Ship, `origin = ui` (clears `review_due`); MCP `create_pr` is `origin = mcp` (does not). `Conflict` while the item's Claude is `Working`/`NeedsInput` ("Claude is working in this worktree. Ship when it stops.", ui only), with no commits ahead of `<remote>/<base>` ("No commits ahead of main."), after a merge/close, or while another operation holds the item ("Claude is shipping this item." when it is Claude's MCP ship) |
| `work_pr_draft` | `{id}` | `PrDraft` | the title / body / draft Ship would use (prefills the dialog) |
| `work_finish` | `{id, opts: FinishOpts{remove_worktree, delete_branch, force, transition_to?}}` | `WorkItem` | for `Merged`/`PrClosed` items only an explicit `transition_to` moves the ticket; a clean merged branch is deleted with `-D` (squash merges); the worktree is removed only when Kelta created it (`worktree_created`): one adopted at start stays with its ignored files, and its branch, still checked out there, is kept ("Branch kept" toast). Finished items are pruned at startup 30 days after their last write |
| `work_finish_merged` | `{ids}` | `FinishMergedReport{finished: Vec<WorkItem>, skipped: Vec<SkippedItem{id, reason}>}` | finishes the listed items still `Merged` with clean worktrees (remove worktree + delete branch); dirty / unpushed ones and those waiting for a Done choice are skipped |
| `work_check_prs` | `{}` | `()` | one `CodeHost::get` per unfinished item whose PR is missing from the authored open list; also run at startup (§8.4) |
| `work_link` | `{id, ticket: TicketRef, apply_side_effects: bool}` | `WorkItem` | scratch (Branch) items only; becomes Ticket-kind, branch never renamed; side effects = `work.on_start`, plus `work.on_pr` when a PR exists (then `pr_title_needs_key`) |
| `work_create_ticket` | `{id, view_id, title, body_md, apply_side_effects: bool}` | `WorkItem` | Create ticket (FLOW §4.3 step 4): `Tracker::create` in the project tracker's view `view_id`, then `work_link` |
| `work_status` | `{id}` | `GitStatus{ahead, behind, dirty, unpushed, diverged, remote_new, files, insertions, deletions, missing}` (on demand, no fetch; ahead/behind and diffstat against `<remote>/<base>`, never the branch's upstream; diffstat from the merge base to the working tree, untracked files count in `files`; `missing` = worktree deleted outside Kelta; re-reads a recorded rebase). `diverged` = own rewrite: the recorded `remote_sha` is still the remote tip, is in `pre_head` and not in HEAD. `remote_new` = commits on `<remote>/<branch>` in neither HEAD nor `pre_head` | |
| `work_status_all` | `{}` | `Map<WorkItemId, GitStatus>` for every unfinished item: one `git fetch <remote>` per repo first, at most every 5 min (UI: startup, window focus, Now open) | |
| `work_diff` | `{id, delta?: bool, from?: String, path?: String}` | `SessionInfo`: the review diff session in the item's worktree (editor with `editor.review_args`, `{range}` = `<remote>/<base>`; empty → a shell running `git diff $(git merge-base <base> HEAD)`). `delta`: `{range}` = `<reviewed>..<last>` (shell: `git diff <reviewed> <last>`), `Invalid` when nothing changed since the last review. `from` (review items, the reviewed PR head): `{range}` = `<from>..HEAD`, or `git range-diff <from>...HEAD` in a shell after a force push. `path`: the same diff at that file (`{range}` and the shell line get `-- <path>`; a path outside `[A-Za-z0-9._/+@-]` opens the whole diff). The UI places it split down, zoomed, in the work tab | |
| `work_delta_files` | `{id}` | `Vec<DeltaFile{path, insertions, deletions, test, generated}>`: per-file numstat of `refs/kelta/wi/<id>/reviewed..last` (renames keep the new path, binaries count 0; `generated` by the delta chip's rules), empty before the first `Stop` or once reviewed. The Story panel's Changes lists it | |
| `work_mark_reviewed` | `{id}` | `WorkItem` (clears `review_due` and `delta`; `refs/kelta/wi/<id>/reviewed` = `last`) | |
| `work_set_note` | `{id, note: Option<String>}` | `WorkItem` (`next_note`; blank clears) | |
| `work_left` | `{id}` | `WorkItem` (`left_at` = now; the return strip's clock) | |
| `work_seen` | `{id}` | `WorkItem` (clears `claude_replied`: the item's tab is shown in the focused window, #489) | |
| `work_start_now` | `{id}` | `WorkItem` | a `Queued` item starts now, over `claude.max_live` (#141) |
| `work_queue_front` | `{id}` | `WorkItem` | the `Queued` item starts next |
| `work_start_claude` | `{id}` | `WorkItem` | a Claude for an item with none alive: resumes its conversation (else a fresh one), split into its work tab |
| `work_notes` | `{id}` | `ReviewNotes{worktree, notes: Vec<ReviewNote>, since: Option<ReviewDelta>}` | review notes of the item (#133, SQLite `notes`), with `+N/−M since feedback` |
| `work_note_resolve` | `{id, note: i64}` | `ReviewNotes` | |
| `work_notes_send` | `{id}` | `ReviewNotes` | the open notes in one message to the item's Claude (held while it works, as `work_send`) |
| `ticket_steps` | `{}` | `Vec<TicketStep>` | every stored ticket step (#462, SQLite `ticket_steps`; `ticket.id` empty): the stage track's place |
| `work_arm_merge` | `{id, method: MergeMethod /*squash|merge|rebase*/}` | `WorkItem` (`auto_finish`) | host auto-merge (GitHub `enablePullRequestAutoMerge`, GitLab `merge_when_pipeline_succeeds`); refusal = host message. On `pr.merged` the item runs Finish once; a dirty/unpushed worktree is kept and the item flagged (`Merged.detail`) |
| `work_disarm_merge` | `{id}` | `WorkItem` | |
| `work_park` | `{id}` | `WorkItem` (`parked_at`, `nvim_kept`) | #142: kill the item's Claude and nvim (SIGHUP → 2 s → SIGKILL), sessions back to Dormant so the next attach resumes them (`claude --resume`, nvim `-S`); worktree and conversation untouched, `worktree.teardown` runs when set. An nvim with unsaved buffers (`getbufinfo({'bufmodified':1})` over RPC) or that cannot be checked keeps running. `Conflict` while Claude works. `claude.auto_park_after_mins` parks a done + seen Claude after that long without input (one-shot per session, cancelled by input) |
| `work_send` | `{id, prompt, files: Vec<SendFile{name, content}>, threads?: Vec<String>, feedback_at?: String}` | `WorkItem` | files (`name.md`, not `ticket.md`/`context.md`) go to the item's private Claude run dir, never the worktree; `prompt` is rendered (`{file}`, `{pr.url}`, `{onto}`…) then delivered by `deliver_prompt`: pasted (bracketed + Enter) into a live Claude whose hook status is `Done`/`WaitingUser`, held in the journal until its next `Stop` while it is `Working`/`NeedsInput` (hooks; a second send is appended, core toasts the hold), or passed to `claude --resume <uuid> -- <prompt>` (dead / closed tab / dormant; the fresh-Claude fallback, `--session-id <new uuid>`, keeps it). Never refused for a busy Claude: a live session Kelta cannot paste into (status not from hooks) gets the prompt on the clipboard (`CoreApi::clipboard_copy`) with the toast "This Claude isn't connected to Kelta: the feedback is copied, paste it when Claude is ready." `threads` → `WorkItem.sent_threads` and `feedback_at` → `WorkItem.feedback_at` (the later of old and new, as instants), stored at send time (also when held) |
| `work_feedback` | `{id}` | `Feedback{threads, reviews, failed_checks, reviewers}` (`CodeHost::feedback` of the item's PR) | |
| `work_rerequest_review` | `{id}` | `Vec<String>` (logins asked again) | |
| `work_resolve_sent_threads` | `{id}` | `WorkItem` (`sent_threads` cleared) | |
| `work_rebase` | `{id, op: RebaseOp{kind: start{onto: base\|remote_branch, no_fetch}\|continue\|abort}}` | `WorkItem` with `rebase: Option<RebaseState{onto, pre_head, remote_sha, conflicts, step, total}>` | start refuses `Conflict{claude_busy}` and `Dirty{files}`; fetches base (+ branch when pushed), `Network{reason: fetch_failed}` (retry with `no_fetch`); `total > 0` = stopped; kept with `total = 0` only while a force push is pending (dropped for a never-pushed branch) |
| `work_push` | `{id, force}` | `WorkItem` (`rebase` cleared) | plain `git push -u` or, only when `diverged`, `git push --force-with-lease=<branch>:<remote_sha> --force-if-includes`, in a visible transient pane; refused while Claude works. Failures are diagnosed by a fetch: `Conflict{reason: non_fast_forward}` / `Conflict{reason: lease_rejected}`, never retried, never a plain `--force` |
| `editor_open` | `{target: EditorTarget /*Session{id}|WorkItem{id}*/, path, line?}` | `()` | `commands/editor.rs` (L6) |
| `editor_send_selection` | `{editor_session, claude_session}` | `()` | `commands/editor.rs` (L6) |
| `editor_quickfix` | `{target: EditorTarget, files}` | `()`; nvim (RPC) only, `setqflist` with the files (relative to the editor cwd) at line 1 | `commands/editor.rs` (L6) |
| `fs_exists` | `{paths}` | `Vec<bool>` per absolute path (relative = `false`); terminal file links | `commands/editor.rs` (L6) |
| **tools / plugins / triggers** | | | `commands/tool.rs`, `plugin.rs`, `trigger.rs` (L8) |
| `tool_list` | `{project_id}` | `Vec<ToolInfo{id, label, icon, kind: Pty|Web, installed: Option<bool>, source: Layer|Plugin}>` | |
| `tool_check` | `{tool_id}` | `ToolCheck{installed, version?, install_hint?}` | |
| `tool_open` | `{project_id, tool_id, ctx: TemplateCtx, placement}` | `ToolHandle /*Pty{session_id}|Web{instance_id, url, embed: EmbedMode}*/` | |
| `tool_close` | `{instance_id}` | `()` | |
| `plugin_list` | `{}` | `Vec<PluginInfo{id, name, version, enabled, permissions, granted, problems}>` | |
| `plugin_inspect` | `{source: String /*dir|git url|tar path*/}` | `PluginInstallPreview{manifest, permissions, sha256, warnings}` | |
| `plugin_install` | `{source, sha256, grant: Vec<String>}` | `PluginInfo` | |
| `plugin_uninstall` / `plugin_enable` | `{id}` / `{id, enabled}` | `()` | |
| `plugin_grant` | `{id, permissions: Vec<String>}` | `PluginInfo` | |
| `plugin_screen_open` | `{plugin_id, screen_id, project_id?, params}` | `{instance_id, url}` | |
| `plugin_screen_close` | `{instance_id}` | `()` | |
| `plugin_call` | `{instance_id, method: PluginMethod, params: Value}` | `Value` (permission-gated, §11.3) | |
| `command_run` | `{command_id, ctx: TemplateCtx}` | `()` (plugin/config `commands`) | |
| `trigger_list` | `{project_id?}` | `Vec<TriggerInfo{id, origin, on, enabled}>` | |
| `trigger_test` | `{trigger_id, payload: Value}` | `TriggerRun` | |
| `trigger_log` | `{limit}` | `Vec<TriggerRun{ts, trigger_id, event, ok, detail, depth}>` | |

### 6.1 Terminal channel frames (`session_attach` channel)

First byte = tag. Little-endian.

| Tag | Name | Payload | UI action |
|---|---|---|---|
| `0x01` | Data | raw PTY bytes | `term.write(bytes, () => ack(n))` |
| `0x02` | Snapshot | ANSI repaint (§9.3) | `term.reset()` then write, ack |
| `0x03` | Exit | `i32` code (`-1` = signal) | show exit banner |
| `0x04` | Keyboard | `u8` kitty keyboard flags of the active screen | key encoding (§7.4); sent when the flags change, and after a Snapshot when non-zero (a Snapshot resets them to 0); not acked |

Acks are batched per animation frame (`session_ack` with summed bytes). A frame of a stale `generation` is ignored by the UI; a stale ack is ignored by Rust.

### 6.2 UiEvent (single `Channel<UiEvent>`; serde tag `type`)

```ts
type UiEvent =
 | {type:'session.updated', session: SessionInfo}
 | {type:'session.removed', id: SessionId}
 | {type:'attention.changed', project_id: ProjectId, level: Attention, needs_input_count: number, total_needs_input: number}
 | {type:'project.updated', project: ProjectInfo} | {type:'project.removed', id: ProjectId}
 | {type:'layout.changed', project_id: ProjectId, layout: Layout}            // backend-initiated changes, and another window's layout_save (#342)
 | {type:'tickets.changed', scope: Scope} | {type:'reviews.changed', scope: Scope, new_keys: ReviewRef[]}
 | {type:'work.updated', work: WorkItem}
 | {type:'ticket.step', step: TicketStep}                                                  // a ticket entered a pipeline step (#461), relayed from bus ticket.step (#462)
 | {type:'settings.changed', layers: Layer[], paths: string[], requires_restart: string[]}
 | {type:'account.status', account_id: AccountId, status: 'ok'|'needs_auth'|'rate_limited'|'offline'|'error', detail?: string}
 | {type:'toast', toast: Toast /*{level:'info'|'warn'|'error', text, action?: {label, command: ActionId|string, args?}}*/}
 | {type:'plugin.event', instance_id: ScreenInstanceId, name: string, payload: unknown}   // relayed bus events granted to a screen
 | {type:'ctl.command', cmd: CtlCommand}                                                    // toggle, palette, focus-project…
 | {type:'ui.open', request: OpenPaneRequest, project_id: ProjectId}                       // backend asks UI to open a pane
 | {type:'workflow.updated', run: ClaudeRun}                                                // a session's Claude Workflow run changed (§7.7)
 | {type:'app.quit_requested', live: number, busy: string[]}                                // quit with sessions keltad would keep: ask keep / stop, then app_quit (§7.5)
 | {type:'outside.updated', session: OutsideSession}                                      // a Claude session Kelta did not start appeared or changed (#345)
 | {type:'outside.removed', id: string}                                                     // it ended or its process is gone
```

Each window has its own channel (§2 "Detached windows"). One Tauri event exists outside this channel: `kelta://update` (no payload) at the end of each update check; the banner then calls `app_update_status`.

### 6.3 Internal bus (`BusEvent`) — catalogue in PLUGINS.md §6

`BusEvent { name: String, ts: String, project_id: Option<ProjectId>, session_id: Option<SessionId>, work_item_id: Option<WorkItemId>, payload: Value, chain: TriggerChain{depth: u8, origin_triggers: Vec<String>} }` on a `tokio::sync::broadcast` (capacity 1024; lagged subscribers get a `lagged` marker and resync). The trigger engine (`PluginHost::start`) is one subscriber. Core itself handles three events synchronously on publish: `app.focus_changed` (published by the desktop shell on window focus; scheduler intervals), `work.updated` (published by kelta-work; relayed as `UiEvent::WorkUpdated`) and `ticket.step` (published by kelta-work's pipeline; relayed as `UiEvent::TicketStep`).

---

## 7. Session / PTY lifecycle

### 7.1 Login environment (once per app start, background)

1. `$SHELL` (fallback `/bin/zsh` macOS, `/bin/bash` Linux). Run `$SHELL -l -i -c '<probe>'` where probe prints `\0__KELTA_ENV_BEGIN__\0`, then `env -0`, then `\0__KELTA_ENV_END__\0` (external `env`, `printf` works in sh/zsh/bash/fish; nushell gets `^env -0`). stdin `/dev/null`, no controlling tty, timeout 3 s.
2. On failure: retry `$SHELL -l -c`; then macOS `/usr/libexec/path_helper -s` + inherited env; Linux inherited env. "Inherited" is `LoginEnv::inherited()`: Kelta's own environment without the AppImage runtime's variables (§7.2).
3. Parse only between sentinels (rc files may print noise). Cache in memory (`LoginEnv`); `diagnostics_run` shows the source.

### 7.2 Spawn

- `program` resolved with `which::which_in(program, LoginEnv.PATH, cwd)` → absolute path; missing → `NotFound` with install hint (tools) or onboarding link (claude/nvim).
- Plain shells: `$SHELL` with `-l` (login). Programs: exec'd directly (no shell wrapper).
- Env = LoginEnv ⊕ settings `terminal.env` ⊕ project `env` ⊕ request env ⊕ Kelta vars: `TERM=xterm-256color`, `COLORTERM=truecolor`, `TERM_PROGRAM=kelta`, `TERM_PROGRAM_VERSION`, `KELTA_SESSION_ID`, `KELTA_PROJECT_ID`, `KELTA_SOCK=<runtime>/ctl.sock`, `KELTA_HOOK_TOKEN` (per-session 128-bit hex), `KELTA_TICKET` (if linked), `KELTA_MCP_TOKEN` (Claude only). `TERMINFO`/`LANG` passthrough; if `LANG` unset → `en_US.UTF-8`.
- New session + process group (`setsid`), controlling tty = PTY. Kill = signal to the process group.
- **Every non-PTY child** (git, `gh`, `ps`, tmux, the login-env probe, web-tool servers, plugin `run` / `exec.run`, KPP providers, keltad) starts from `kelta_proto::term::command()` (`async_command()` for tokio): `env_clear()` then `LoginEnv::inherited()`, so no child inherits what an AppImage runtime set up for Kelta itself (#358). `strip_appimage_env` restores saved originals (`APPIMAGE_ORIGINAL_<VAR>`, `<VAR>_ORIG`), removes entries naming the mount or the `.AppImage` file from path lists (`PATH`, `XDG_DATA_DIRS`, `LD_LIBRARY_PATH`, …), drops a variable left empty, and drops the launch-only ones (`APPDIR`, `APPIMAGE`, `OWD`, `ARGV0`, `*_APPIMAGE`, `DESKTOPINTEGRATION`, `GIO_LAUNCHED_DESKTOP_FILE*`); without `APPDIR` it does nothing. `clippy.toml` bans `std::process::Command::new` and `tokio::process::Command::new` everywhere else (§13); the one allowlisted exception is `kelta-ctl statusline`, which runs the user's statusline in Claude's environment.

### 7.3 Reader loop (L1, per session thread)

```
loop:
  timeout = processor.sync_timeout() deadline (DEC 2026) or infinite
  poll(master_fd, POLLIN, timeout)
  if timeout fired: processor.stop_sync(&mut term)        // flush synchronized-update buffer
  n = read(master, buf[64 KiB])  (EIO/0 → exit path)
  lock(term); processor.advance(&mut term, &buf[..n]); collect Term events; unlock
  Term events: PtyWrite(reply) → master.write (query responses), ColorRequest → palette reply,
               Title/ResetTitle, Bell, ClipboardStore/Load, (OSC 7 cwd and OSC 9/777 via a pre-scan of the chunk)
  if attached view and !paused: if inflight < HIGH(256 KiB) send Data frame, inflight += n
                                 else paused = true (keep parsing, drop bytes for view)
  scrollback accounting → memory budget (§9.5)
exit: waitpid WNOHANG every 10 ms up to 2 s (HANGUP_GRACE), then SIGKILL the child's process group and wait,
      emit Exited (from a drop guard, so a panic in exit still sends it), close fds
```
- `ack(gen, n)`: `inflight -= n`; if `paused && inflight < LOW(64 KiB)` → send Snapshot, `paused = false`.
- **Ack watchdog:** while `inflight > 0`, a one-shot 5 s deadline; expiry emits `TerminalEvent::AckTimeout` → core considers the webview hung (§12.4). Disarmed when `inflight == 0`.
- The child is never blocked by a slow view.
- Input: `write()` on the master fd from the IPC thread (non-blocking fd; EAGAIN → per-session queue drained by the reader on POLLOUT).
- Resize: model `Term::resize` then `master.resize` (TIOCSWINSZ → SIGWINCH). Hidden sessions keep their size. On attach with a different size: resize first, then snapshot (after reflow).

### 7.4 Query responder (single responder = Rust model)

- Model answers: DA1 (`CSI c`), DA2 (`CSI > c`), DSR 5/6 (`CSI n`), DECRQM (`CSI ? Ps $ p`, `CSI Ps $ p`), XTWINOPS 18 (`CSI 18 t`), OSC 4/10/11/12 `?` (from the palette pushed by `terminal_set_palette`, re-pushed on theme change), plus anything else alacritty_terminal 0.26 answers. L1 records the exact answered set in `docs/contracts/terminal-queries.md` (L1-owned file) with tests.
- xterm.js swallows exactly that set via `term.parser.registerCsiHandler` / `registerOscHandler` returning `true` (constant list in `ui/src/lib/gen/terminal_queries.ts`, generated from `kelta_proto::term::SWALLOWED_QUERIES`). Queries the model does not answer (e.g. XTVERSION if unsupported) are **not** swallowed.
- Kitty (D7): with `terminal.keyboard_protocol = "kitty"` the model keeps the `CSI > u` / `CSI < u` / `CSI = u` stacks (one per screen) and answers `CSI ? u`; flag changes go to the view as Keyboard frames. With `legacy` it ignores them and does not answer; apps fall back via DA1. Pushes beyond alacritty's 4096-deep stack are dropped (alacritty 0.26 would panic evicting from the title stack).

### 7.5 Lifecycle and persistence

| Event | Behaviour |
|---|---|
| Spawn | `Live`, status `Starting` → `Running` on first output. |
| Pane hidden / project switch / UI reload / webview crash | Session keeps running; view re-attaches with Snapshot. |
| Window closed, `close_behavior=background` | Webview destroyed; sessions run; attention → dock badge / notifications. |
| Process exit | `Exited{code}`; `close_on_exit` applies; else banner "Exited (code) — Enter restart, x close". |
| App quit | keltad (`terminal.session_host = daemon`): restorable sessions keep running; the rest below applies to the others. Cmd+Q / menu Quit / Dock Quit (`applicationShouldTerminate:`) and a window close that quits first apply `app.quit_sessions` (`Core::quit_plan`): `ask` sends `app.quit_requested` (raising the window) and the UI's quit dialog answers with `app_quit {stop}`; `stop` quits as without keltad (SIGHUP → 2 s → SIGKILL, Dormant rows per `restore_mode`) and then sends keltad `shutdown` (SIGKILL to what is left, socket and pid file removed, exit). The updater's restart never asks and keeps. Confirm if any Claude session is `Working`/`NeedsInput`. nvim sessions get RPC `:wall \| mksession! <data>/sessions/<sid>.vim`. All persisted as `Dormant` with `RestorePolicy`; SIGHUP → 2 s → SIGKILL. |
| Next start | Layout restored; sessions still running in keltad are re-adopted `Live` (no respawn); other `Dormant` sessions spawn **when their pane is first attached** (`app.restore_mode = lazy`; `eager`, `none` available): Claude `claude --resume <uuid>` (if resume is refused: a work item's Claude starts fresh with a new `--session-id`, other Claudes fall back to `--continue`), nvim `-S <file> --listen <sock>`, shell in last OSC 7 cwd, tools relaunched. A dimmed "restored" separator shows the persisted text tail (≤ 200 lines, `sessions.text_tail`). |

### 7.6 Claude status machine (pure fn in kelta-server `hooks::map`)

| Hook (`hook_event_name` / `notification_type`) | Status | Attention |
|---|---|---|
| SessionStart | Running (hooks_active = true) | — |
| UserPromptSubmit | Working | Activity |
| PermissionRequest; Notification `permission_prompt` \| `elicitation_dialog` \| `agent_needs_input` | NeedsInput | NeedsInput |
| Notification `idle_prompt` | WaitingUser | NeedsInput if unseen |
| Stop | Done (preview = `last_assistant_message`, 200 chars) | Done if pane not visible. A work item's Claude: kelta-work sets `review_due`/`claude_replied` and notifies "KEY ready to review" / "KEY: Claude replied" (core skips its generic "finished" notification); the tab and rail `done` lamp of a work item comes from `review_due` |
| StopFailure | Error | Error |
| SessionEnd / PTY exit | Exited | — |
| PostToolUse (matcher `*`, every tool) | `StatusChange.status = Unknown`: NeedsInput → Working (the permission prompt was answered, #278), else unchanged; `Edit\|Write\|MultiEdit`: `file_edited = tool_input.file_path` → bus `claude.file_edited` | Activity if pane not visible, on NeedsInput → Working |
| PostToolUse `Workflow`, SubagentStart, SubagentStop | unchanged; feed the workflow view (§7.7) | — |
| PostToolUse `TodoWrite` / `TaskCreate` / `TaskUpdate`; SessionStart `clear` | unchanged; `ClaudeMeta.tasks` (#406, `status::apply_tasks` via `session_claude_runs_hook`): `TodoWrite` replaces the list (`tool_input.todos[].content/status`), `TaskCreate` appends `tool_input.subject` as pending (id from `tool_response.task.id`), `TaskUpdate` sets `tool_input.status` / `subject` of task `taskId` (`deleted` removes it), `clear` empties it; unknown shapes are ignored. Published by `session.updated`, persisted in `sessions.tasks_json`, kept by a restore or restart | — |

**Hooks-inactive fallback:** no SessionStart within 10 s of spawn (one-shot) → `hooks_active=false`, status source `Heuristic`: output → Working; BEL / OSC 9 / OSC 777 → NeedsInput; 3 s quiet after output (one-shot armed only by output) → Done. Hooks or not, input written to a Claude pane in NeedsInput (the keystroke answering a permission prompt; no hook fires for it) → Working, attention cleared; WaitingUser (`idle_prompt`) waits for UserPromptSubmit. Pane header shows "status hooks inactive · Fix" → Diagnostics.

**Open `ask` question (#356):** while `ClaudeMeta.question` is set, attention stays NeedsInput through Stop / Done and `session_mark_seen` (not through Error or Exited). It is cleared by the first typed input that reaches the PTY (`session_write` succeeded and the bytes do not start with ESC, so terminal replies and focus reports do not count; #367: a failed write keeps the question).

### 7.7 Claude workflow runs (#272, `kelta-core::claude_runs`)

A Claude Code `Workflow` run inside a Kelta session is read from Claude's own files (undocumented, observed with Claude Code in 2026-10) under the session dir, the hook's `transcript_path` minus `.jsonl` (`~/.claude/projects/<slug>/<uuid>/`): `workflows/scripts/<name>-<run>.js` (written at launch), `subagents/workflows/<run>/journal.jsonl` (`{type: started, agentId, label, phase}` and `{type: result, agentId}` per agent), `subagents/workflows/<run>/agent-<id>.meta.json` (`description`, `workflowPhase`), `agent-<id>.jsonl` (the agent transcript) and `workflows/<run>.json` (final record: `workflowName`, `status`, `phases`, `durationMs`, `totalTokens`, `totalToolCalls`, `result`, `logs`; written only when the run ends).

Hooks drive it, no watcher: SubagentStart / SubagentStop fire for workflow agents (checked 2026-10-11), so Kelta registers them, and the PostToolUse matcher (`*`) includes `Workflow`. `CoreApi::session_claude_runs_hook` gets every hook of the session: the `Workflow` PostToolUse names a new run (the first `wf_…` in `tool_response`); a Subagent hook names its agent's run (`agent_transcript_path`'s directory, else the run dir holding `agent-<agent_id>.*`), and SubagentStop marks the agent done before the journal does. Each hook then re-reads every unfinished run of the session (the final record lands after the last agent stops, before Claude's next turn and its hooks) and emits `workflow.updated` when the `ClaudeRun` changed. A run is `running` until its final record exists, then takes its `status` (`completed`, `killed`, …). Phases are the final record's titles, then the agents' phases in first-seen order (`Other` without one); agents come from the journal, then the metas it lacks. Every field is optional, unparsable lines and files are skipped, ids that reach a path are checked (`[A-Za-z0-9_-]`, ≤ 64). Runs live in memory per session (`SessionEntry::claude_runs`), so a Kelta restart forgets them; sessions Kelta did not start are not seen.

UI: the context bar shows `⚙ <workflow> · <phase with a running agent>` (or the final status) for the focused session's latest run; it opens the `claude_runs` pane (#392), split right of the session's terminal in its tab (opening it again focuses it): the work items the session claimed (ticket, branch, worktree, PR), then each run with its phases and agents (✓ done, ● running, ■ stopped in a finished run), totals, logs and result. Selecting an agent reads its transcript tail (`claude_run_tail`), again on each update of its run; no polling, so a long agent's tail moves only when a hook updates the run.

---

## 8. Integrations

### 8.1 Tracker trait

```rust
#[async_trait] pub trait Tracker: Send + Sync {
  fn kind(&self) -> TrackerKind;                       // Jira | Redmine | GithubIssues | GitlabIssues | GiteaIssues | Linear
  fn caps(&self) -> TrackerCaps;                       // board_columns, assign, comment, transitions_need_fetch, projects_v2; core copies the ticket account's caps onto every TicketItem / TicketDetail so the UI dims actions the tracker cannot do
  async fn me(&self) -> Result<User, KeltaError>;
  async fn list(&self, view: &TrackerView, cursor: Option<Cursor>) -> Result<Page<Ticket>, KeltaError>;  // honours view.who and view.current_iteration
  async fn sources(&self, query: &str) -> Result<Vec<SourceHit>, KeltaError>;  // boards/projects/filters/teams/repos as ready TrackerViews; default Unsupported (no account status change)
  async fn search(&self, view: &TrackerView, text: &str) -> Result<Vec<Ticket>, KeltaError>;  // first page of `view` matching `text`, by the provider (palette); default Unsupported
  async fn fields(&self, view: &TrackerView) -> Result<Vec<SourceField>, KeltaError>;  // fields the view's tickets can show (`Source.fields`, #351); default Unsupported
  async fn get(&self, t: &TicketRef) -> Result<TicketDetail, KeltaError>;  // body_md + body_html (sanitized), last 20 comments
  async fn columns(&self, b: &TrackerBinding) -> Result<Vec<Column>, KeltaError>;
  async fn transitions(&self, t: &TicketRef) -> Result<Vec<Transition>, KeltaError>;
  async fn transition(&self, t: &TicketRef, transition_id: &str, fields: Option<Value>) -> Result<Ticket, KeltaError>;
  async fn comment(&self, t: &TicketRef, markdown: &str) -> Result<(), KeltaError>;
  async fn assign(&self, t: &TicketRef, who: Assignee) -> Result<Ticket, KeltaError>;
  fn browser_url(&self, t: &TicketRef) -> String;
  fn branch_key(&self, t: &TicketRef) -> String;       // "SHOP-123" | "4567" | "gh-12" | "gl-12"
  async fn assignable_users(&self, t: &TicketRef, query: &str) -> Result<Vec<User>, KeltaError>;  // assignee picker; default Unsupported (plugins)
  async fn priorities(&self, t: &TicketRef) -> Result<Vec<String>, KeltaError>;  // highest first: jira editmeta, redmine enumeration, linear fixed, github Projects v2 `Priority` field, gitlab `priority::` labels; default Unsupported (gitea, plugins)
  async fn set_priority(&self, t: &TicketRef, priority: &str) -> Result<Ticket, KeltaError>;  // one of `priorities` by name; gitlab swaps the `priority::` label
  async fn set_blocked(&self, t: &TicketRef, marker: &BlockedMarker, blocked: bool) -> Result<Ticket, KeltaError>;  // jira flag or label, github/gitlab/gitea label; default Unsupported (core moves `status` markers itself)
  async fn set_body(&self, t: &TicketRef, body_md: &str) -> Result<(), KeltaError>;  // MCP update_ticket_body; default Unsupported
  async fn close(&self, t: &TicketRef) -> Result<Ticket, KeltaError>;  // MCP close_ticket, where closing is apart from the status (GitHub issues); default Unsupported
  async fn create(&self, project: &TrackerView, title: &str, body_md: &str) -> Result<Ticket, KeltaError>;  // new ticket where the view points (jira jql project, redmine project_id, github repo, gitlab/gitea project, linear team); default Unsupported
}
pub struct TicketRef { account: AccountId, key: String, id: String }
pub struct Ticket { r#ref: TicketRef, title, url, status: Status, kind: Option<String>, assignee: Option<User>,
  labels: Vec<String>, priority: Option<String>, updated_at: String, project_hint: Option<String>,
  priority_rank: Option<u8> /*0 = highest*/, status_since: Option<String> /*RFC 3339; falls back to updated_at*/,
  sprint: Option<Sprint{id, name, active, ends_at: Option<String>}> /*Jira sprint, Linear cycle, GitHub Projects iteration, GitLab iteration|milestone, Redmine version*/,
  estimate: Option<String>, due: Option<String>,
  blocked: bool /*the source's `blocked` marker (#348): providers set the jira flag / linear blocked-by, kelta-core resolves the rest*/ }
pub struct TicketDetail { ticket: Ticket, body_md: String, body_html: String, body_format: BodyFormat /*Adf|JiraWiki|Textile|Markdown*/,
  comments: Vec<Comment{author, created_at, body_html}>, parent: Option<TicketRef>,
  prs: Vec<PrLink>, caps: TrackerCaps /*both filled by kelta-core, providers leave them empty*/ }
pub struct PrLink { url, account: Option<AccountId> /*code-host account when the repo is bound: opens the review detail, else browser only*/, repo, number: u64,
  title, branch, state: PrState, draft: bool, ci: CiState, review: Option<ReviewDecision>, source: PrSource /*WorkItem|KeyMatch*/ }
pub struct Status { id, name, category: StatusCategory /*Todo|InProgress|InReview|Done|Unknown*/ }
pub struct Transition { id, name, to: Status, needs_fields: bool }
pub struct Column { id, name, category: StatusCategory, order: u32, match_names: Vec<String> }
pub struct SourceHit { kind: String, label, detail: Option<String>, view: TrackerView /* core sets view.account */ }
pub enum Who { Mine, Unassigned, Anyone }          // snake_case; TrackerView.who None = legacy provider fields
// TrackerView gains who: Option<Who>, current_iteration: bool (default false), account: Option<AccountId> (None = binding.account)
pub enum Cursor { Offset(u32), Token(String), Page(u32), After(String) }
pub struct Page<T> { items: Vec<T>, next: Option<Cursor> }
```

| Provider | Auth | List | Move |
|---|---|---|---|
| Jira Cloud | Basic `email:api_token` | `POST /rest/api/3/search/jql` with explicit `fields`, `nextPageToken: null` first; stop on missing token **or** empty page, cap 20 pages | `GET`/`POST …/transitions`; match `to.statusCategory.key` or name; never hard-code ids; 400 `errors` → `NeedsFields` |
| Jira DC/Server | Bearer PAT | `POST /rest/api/2/search` (`startAt`) | same, v2; assign by `name` |
| Flavor | `GET /rest/api/2/serverInfo` → `deploymentType`, cached per account | | |
| Redmine | `X-Redmine-API-Key` header | `/issues.json?assigned_to_id=me&status_id=open&sort=updated_on:desc&limit=100` (+ `project_id`, `query_id`) | `PUT /issues/{id}.json {issue:{status_id}}` restricted to `include=allowed_statuses`; 422 surfaced; poll ≥ 60 s |
| GitHub Issues | gh-cli / keyring PAT / env | `GET /issues?filter=assigned&state=open` (drop items with `pull_request`) or per-repo; ETag | no project: open/closed; Projects v2: Status single-select field/option ids resolved **by name** at runtime, cached; `updateProjectV2ItemFieldValue` |
| Gitea Issues | `Authorization: Bearer` (keyring/env/command) | `/repos/issues/search?type=issues&assigned=true` or `/repos/:o/:r/issues` | open / closed only: Close / Reopen via `PATCH {state}` |
| GitLab Issues | `PRIVATE-TOKEN` (keyring/glab-cli/env/command) | `/api/v4/issues?scope=assigned_to_me&state=opened` or `/projects/:id/issues` | scoped labels `<scope>::<value>` (default `workflow`): `PUT add_labels` + explicit `remove_labels` of same-scope labels; Done = `state_event=close` |
| Linear | personal API key, raw `Authorization` header (`auth = "bearer"` for OAuth) | `POST https://api.linear.app/graphql` `issues(filter, first: 50, after, orderBy: updatedAt)` (assignee `isMe`, team key, project name, labels); `pageInfo.endCursor`, cap 20 pages | `issueUpdate(stateId)` with state ids read at runtime from the issue's team `states` (matched by name, never hard-coded); rate limit = HTTP 400 + `RATELIMITED` → `RateLimited` |

ADF → Markdown: tolerant recursive walker (unknown nodes render children, never fail). Comments to Jira Cloud = ADF paragraphs (one per line). Redmine Textile shown preformatted unless account `text_format = "markdown"`. Linear descriptions and comments are Markdown (same sanitizer as GitHub/GitLab).

### 8.2 CodeHost trait

```rust
#[async_trait] pub trait CodeHost: Send + Sync {
  fn kind(&self) -> CodeHostKind;                      // Github | Gitlab | Bitbucket | Gitea
  async fn me(&self) -> Result<User, KeltaError>;
  async fn changed_since_last(&self) -> Result<bool, KeltaError>;  // cheap gate; default Ok(true)
  async fn list_reviews(&self, q: &ReviewQuery) -> Result<Vec<Review>, KeltaError>;  // kind + include_team + include_drafts
  async fn list_open(&self, repos: &[String], include_drafts: bool) -> Result<Vec<Review>, KeltaError>;  // every open PR of the repos (ReviewKind::Team); default Unsupported
  async fn get(&self, r: &ReviewRef) -> Result<ReviewDetail, KeltaError>;  // ReviewDetail.state: PrState Open|Merged|Closed
  async fn approve(&self, r: &ReviewRef, head_sha: &str) -> Result<(), KeltaError>;
  async fn comment(&self, r: &ReviewRef, body: &str) -> Result<(), KeltaError>;
  async fn request_changes(&self, r: &ReviewRef, body: &str) -> Result<(), KeltaError>;
  // Line comment (new-side `line` of `path`) on MY pending review, created on first use. approve /
  // comment / request_changes publish the pending review with the decision (GitHub: review events
  // endpoint; GitLab: draft_notes + bulk_publish). Default: Unsupported.
  async fn add_pending_comment(&self, r: &ReviewRef, path: &str, line: u32, body: &str) -> Result<(), KeltaError>;
  async fn post_note(&self, r: &ReviewRef, body: &str) -> Result<(), KeltaError>;  // plain comment that leaves my pending review unpublished; default = comment
  async fn create(&self, d: &PrCreate) -> Result<Review, KeltaError>;
  // default Unsupported for the three below
  async fn update_title(&self, r: &ReviewRef, title: &str) -> Result<(), KeltaError>;  // key prefix after Link to ticket
  async fn arm_auto_merge(&self, r: &ReviewRef, method: MergeMethod) -> Result<(), KeltaError>;  // work_arm_merge; + disarm_auto_merge(r)
  async fn set_draft(&self, r: &ReviewRef, draft: bool) -> Result<(), KeltaError>;  // review_set_draft
  async fn find_for_branch(&self, repo: &str, branch: &str) -> Result<Option<Review>, KeltaError>;
  fn fetch_refspec(&self, r: &ReviewRef, local_branch: &str) -> String; // pull/N/head:… | merge-requests/N/head:… | <source branch>:… (Bitbucket)
  fn repo_from_remote(&self, url: &str) -> Option<String>;
  // Fix with Claude (default: Unsupported)
  async fn feedback(&self, r: &ReviewRef) -> Result<Feedback, KeltaError>;          // unresolved threads (+ last comment time), reviews with a body (+ submitted time), failed checks (+ legacy commit statuses) + 80-line cleaned log tail
  async fn rerequest_review(&self, r: &ReviewRef, who: &[String]) -> Result<Vec<String>, KeltaError>; // `who`, or (empty) everyone who reviewed, minus me
  async fn resolve_threads(&self, r: &ReviewRef, ids: &[String]) -> Result<(), KeltaError>;
}
// Review.reviewed_head = commit of my last submitted review (ReviewRequested kind). With head_sha it gives
// pending / "Updated since your review" / "Reviewed". GitHub ReviewRequested lists add a second search
// `is:pr is:open reviewed-by:@me` (user-review-requested drops a PR once I review) and keep only the PRs
// whose head moved since my review. ReviewDetail.pending_comments = line comments waiting in my pending review.
pub struct ReviewRef { account: AccountId, repo: String, number: u64 }
pub struct Review { r#ref, title, url, author: User, draft: bool, head_sha, source_branch, target_branch,
  ci: CiState /*Success|Failure|Pending|Error|None*/, failed_check_names: Vec<String> /*GitHub rollup contexts; empty on GitLab*/, decision: Option<ReviewDecision /*Approved|ChangesRequested|ReviewRequired*/>,
  my_state: Option<MyReviewState /*Pending|Approved|ChangesRequested|Commented*/>, mergeable: Option<bool>,
  labels: Vec<String>, kind: ReviewKind /*ReviewRequested|Authored*/, updated_at: String, linked_tickets: Vec<String>,
  additions: Option<u32>, deletions: Option<u32>, decision_head: Option<String> /*head the latest decisive review was left on*/,
  requested_at: Option<String> /*when my review was requested; authored: waiting since*/, blocking: bool /*I am the last required reviewer*/,
  waiting_on: Vec<String> /*authored: reviewers it waits on*/, nudged_at: Option<String> /*my last nudge, core-filled*/,
  unresolved_threads: Option<u32>, last_comment_at: Option<String> /*authored, GitHub/GitLab: unresolved threads; newest last comment not mine among them*/ }
pub struct Feedback { threads: Vec<FeedbackThread{id, author, path?, line?, body_md, url, last_comment_at? /*None when my comment is last*/}>,
  reviews: Vec<FeedbackReview{author, state?, body_md, submitted_at?}>, failed_checks: Vec<FailedCheck{name, url?, log_tail?}>, reviewers: Vec<String> }
```

- GitHub: list = one GraphQL request with aliased searches `is:pr is:open review-requested:@me archived:false` (`user-review-requested:@me` if `reviews.include_team_requests=false`) and `is:pr is:open author:@me` (the authored one alone also spreads a `Threads` fragment: `id reviewThreads{isResolved}`; the last comments, `comments(last:1)` per thread, come from a second `nodes(ids:)` query only for the PRs with open threads whose `(updatedAt, unresolved count)` moved since the last poll, so each search stays under 10 points for 50 PRs); small nested `first:`. Gate: `GET /notifications?participating=true` + `If-None-Match` only for classic/gh tokens (fine-grained → no gate). Actions REST: `POST /repos/{o}/{r}/pulls/{n}/reviews` with `commit_id`. GHE via `base_url`.
- GitLab: `GET /merge_requests?scope=all&state=opened&reviewer_username=<me>` and `scope=created_by_me`, `updated_after`; `draft=` if `/version` ≥ 16 else `wip=`; approve `POST …/approve {sha}` (409 → `Conflict`); request changes = note (+ optional unapprove). Gate: `/todos?state=pending&action=review_requested` count+max id. Authored MRs: `GET …/discussions` only when `(updated_at, user_notes_count)` moved since the last poll (cached in `GitlabHost`, like `diff_sizes`).
- Gitea/Forgejo: `GET /api/v1/repos/issues/search?type=pulls&state=open&review_requested=true` (or `created=true`) returns issue-shaped rows, each expanded with `GET …/pulls/{n}` (branches, head sha); pagination follows `Link: rel="next"`. Approve / request changes = `POST …/pulls/{n}/reviews {event, commit_id}`; comment = issue comment; draft = `draft` or a `WIP:` title. Bearer token auth, `base_url` required. No change gate (always `true`).
- Bitbucket Cloud: the cross-workspace endpoints were retired in April 2026, so lists walk `GET /user/workspaces`: my PRs from `/workspaces/{ws}/pullrequests/{me}`, review requests from `reviewers.uuid` queries over the workspace's 50 most recently updated member repositories; `next` URLs are followed. Auth is `Basic <email>:<API token>` (app passwords were retired in 2026) or Bearer for access tokens. Request changes = comment + `POST …/request-changes`; approve compares the PR's current head with the reviewed sha first (`Conflict` if moved); no PR refs exist, so "review locally" fetches the source branch (no forks). List rows carry no CI (detail rolls up `/statuses`). No change gate.
- `linked_tickets`: regex `reviews.ticket_key_regex` over branch + title.
- Ticket to PR join (`feeds::ticket_prs`, no extra request): core fills `TicketItem.prs` and `TicketDetail.prs` from data already polled. The work item's `pr_url` comes first (`PrSource::WorkItem`; its state follows the work item when merged or closed), then every polled review whose `linked_tickets` name the ticket (`PrSource::KeyMatch`, matched with the same key rules as the Reviews pane; a bare Redmine number only matches in the code-host repos of the ticket's projects). `PrLink.account` is set when the repo is bound to a code-host account: the UI then opens Kelta's review detail (`p`), else the browser. `P` is always the browser.
- Feedback (FLOW §4.2). GitHub: one GraphQL query (`reviewThreads(first:100)` filtered on `isResolved` client side, since the connection has no such argument; `latestReviews` with a body; `viewer`), check runs and legacy commit statuses (`/commits/{sha}/status`, failed ones listed with no log) of the head (REST, together) and, for failed GitHub Actions runs (5 max), the job log tail. `log_tail` strips ANSI CSI/OSC sequences and GitLab `section_start:`/`section_end:` markers, keeps the text after the last `\r` of a line, drops GitHub's line timestamp, cuts a GitHub log after its last `##[error]` line and keeps the last 80 lines. Re-request = `POST …/requested_reviewers` with the latest reviewers; resolve = `resolveReviewThread` per id. GitLab: unresolved resolvable discussions (system notes skipped), failed jobs of the head pipeline + `/jobs/:id/trace` tail; re-request = `/request_review @…` quick action note (GitLab 17); resolve = `PUT …/discussions/:id?resolved=true`. A 403 / offline read maps to the FLOW §6 wording ("GitHub refused the review threads (403: token lacks `pull_requests:read`).", GitLab: `read_api`).
- GitLab `decision = ChangesRequested` (and `decision_head = sha`) when `blocking_discussions_resolved = false`, `detailed_merge_status = discussions_not_resolved`, or a reviewer is in `requested_changes` state (`GET …/reviewers`, GitLab 17+). GitHub `decision_head` = commit of the latest APPROVED / CHANGES_REQUESTED review.

### 8.3 HttpCtx (kelta-http)

One shared `reqwest::Client` (20 s timeout, pool idle 30 s, UA `kelta/<ver>`). Per-account `HttpCtx`: semaphore (4 concurrent), honours `Retry-After`, `x-ratelimit-*`, `RateLimit-*`, `X-RateLimit-*`; exponential backoff with jitter (max 10 min; secondary limit without header ≥ 60 s); ETag LRU (256, exact URL); 401 → `NeedsAuth` (account paused, banner); offline → `Network` (scheduler pauses the account until focus/manual refresh).

### 8.4 Scheduler, aggregation, caches (kelta-core)

- One task, `BinaryHeap<(Instant, SubscriptionKey)>`; `SubscriptionKey = (AccountId, QueryKey)`. Subscriptions exist only while (a) a visible pane needs that query, or (b) a notification rule needs it (`notifications.review_requested`, `ci_failed_mine` → review subscriptions of open projects + unbound accounts). Identical queries across projects share one subscription.
- Interval: `polling.focused_secs` (focused) / `polling.background_secs` (unfocused; 0 = off) / off when window closed unless `polling.when_closed = "background"`; ±10 % jitter; floor `polling.min_secs` (Redmine 60).
- Refresh triggers: view open, project switch, window focus when data older than focused interval, after any write, manual (`R`).
- Startup renders from `provider_cache` (SQLite) — no network before first paint.
- Aggregation: `Scope::All` fans out over accounts referenced by open projects **plus all configured accounts** for reviews; de-dup by ref; items tagged with matching project ids; unmatched → `project_ids = []` (UI "Other" group).
- `seen_reviews(account, repo, number, head_sha, first_seen)`: first poll after start/account creation fills silently; new key (or new head after my review) → `pr.review_requested`.
- Authored PRs (FLOW §3.6): the authored query always includes drafts (`reviews.include_drafts` filters review requests only). Authored is also subscribed for every code-host account bound to a project with an unfinished work item whose PR is open, whatever panes are visible and notifications are on (core tracks those items from `work.updated`). An authored PR that leaves the open list gets one `get`: merged → `pr.merged`, closed → `pr.closed`. At startup and on `work_check_prs` (Now / Inbox open), every unfinished work item whose `pr_url` is missing from the authored open list gets the same `get`, so a merge while Kelta was closed lands as `Merged` on the next launch. Each PR's end is published once per process.
- Branch join: an authored PR whose `(repo, source_branch)` is an `Active` work item's repo binding and branch becomes that item's PR (`pr_url`, `PrOpen`, `work.updated`, `work.on_pr` once), e.g. when Claude ran `gh pr create`.
- kelta-work listens to `pr.merged` / `pr.closed` (idempotent per item): `Merged` plus the guarded `on_merge` move (SETTINGS `[work] on_merge`), or `PrClosed`. The move never guesses between Done statuses.

### 8.5 Claude IDE bridge (kelta-server `ide`, `claude.ide_bridge`, off by default)

Kelta acts as the IDE of the Claude sessions it spawns, with the protocol of the VS Code / JetBrains extensions and claudecode.nvim (no official spec; reverse-engineered, see coder/claudecode.nvim `PROTOCOL.md`):
- **One bridge per Claude session**: an axum WebSocket listener on `127.0.0.1:<random>` and the lock file `<claude dir>/ide/<port>.lock` = `{pid, workspaceFolders: [session cwd], ideName: "Kelta", transport: "ws", authToken}`. `<claude dir>` is resolved from the session's own environment: `CLAUDE_CONFIG_DIR`, else `$HOME/.claude`. The session gets `CLAUDE_CODE_SSE_PORT=<port>` and `ENABLE_IDE_INTEGRATION=true`, so Claude connects to its own bridge. A per-session port (not one shared server) is what lets one lock file carry one token and route to one session's editor.
- **Lifecycle**: opened in `spawn_with` before the pty spawns; closed (listener aborted, lock removed) on spawn failure, session exit/removal (`Server::unregister_session`), restart (replaced) and app quit (`Server::ide_close_all`; the process exits without destructors). Each open first removes *stale Kelta locks* in that dir: `ideName == "Kelta"` and `pid` no longer alive. Locks of other IDEs, of live Kelta processes and unparsable files are never touched. Sessions kept by keltad across a quit lose their bridge until restarted.
- **Transport**: MCP JSON-RPC 2.0, one message per text frame (`initialize`, `tools/list`, `tools/call`, `ping`; notifications ignored). Each request runs in its own task so a pending `openDiff` does not block `close_tab`.
- **Tools** (routed to the session's editor: its work item's editor, else a live editor session of its project — same rule as the MCP `open_in_editor`):
  - `openFile {filePath, startText?}` → `CoreApi::editor_open` (every editor preset with an open mode); `startText` → its first line.
  - `openDiff {old_file_path, new_file_path, new_file_contents, tab_name}` → the proposal is staged in `<runtime>/ide/<random>/<file name>` (0700 dir, 0600 file) and shown with `CoreApi::editor_diff` (nvim RPC: new tab, `:vertical diffsplit`). The call stays pending until Claude sends `close_tab`/`closeAllDiffTabs` or disconnects, then the diff tab is closed, the staging dir deleted and `DIFF_REJECTED` returned (claudecode.nvim semantics; the edit itself is accepted or rejected in Claude's terminal prompt). Saving the proposal in nvim does not answer `FILE_SAVED`. Without an nvim editor the call fails at once and Claude falls back to its own diff.
  - `getWorkspaceFolders` → the session cwd. `getCurrentSelection`/`getLatestSelection` → `{success: false}`, `getOpenEditors` → `{tabs: []}`, `getDiagnostics` → `[]`: Kelta tracks neither nvim buffers nor LSP diagnostics (the editor's "Send selection" action covers selections). No `selection_changed`/`at_mentioned` notifications.

---

## 9. Terminal frontend contract (L2)

### 9.1 View pool
- `TerminalView` = one xterm instance bound to one session. Pool keyed by session id; capacity `terminal.max_live_views` (default 2, min 1, max 12; 4 → 2 for gate G1) **plus** currently visible panes. Hidden views beyond capacity: `dispose()` + `session_detach` (LRU).
- Re-show of a pooled view: no snapshot needed (still attached). Re-show of a disposed view: new xterm + `session_attach` → Snapshot.

### 9.2 Rendering
- `terminal.renderer`: `auto` (WebGL on macOS, DOM on Linux) | `webgl` | `dom`. WebGL addon is dynamically imported; `onContextLoss` → dispose + DOM; hard cap 8 concurrent WebGL contexts (others use DOM).
- Linux first-run probe: 2 s rAF scroll test; if WebGL ≥ 55 fps and DOM < 45 fps, toast offers switching (result stored in the webview's `localStorage`, `ui/src/lib/terminal/probe.ts`, not in settings).
- Cursor blink only on the focused pane and only if `terminal.cursor_blink=true` (default false). Motion only to explain (DESIGN §8, #398): CSS transitions and keyframes or Svelte `transition:`/`animate:` directives, `transform`/`opacity` only, no JS timers or rAF loops. Nothing loops at idle: a loader (spinner, load bar) is mounted only while its operation is in flight, a pulse runs a bounded number of times. `app.animations` (`auto` = `reduced` in safe graphics mode) and `prefers-reduced-motion` (always off) scale it.

### 9.3 Snapshot encoder (L1) — required coverage (golden-tested with recorded nvim, Claude, lazygit, htop streams)
Alt-screen state with main scrollback (`?1049`), last N history lines (`terminal.view_scrollback`, default 1000), visible grid with SGR runs incl. colon underline styles (`4:3` undercurl), underline colour (`58`), truecolor/256, OSC 8 hyperlinks, wide + combining chars, DECSTBM + origin mode, saved cursor (DECSC), G0/G1 charsets (DEC line drawing), cursor position/shape (DECSCUSR)/visibility (`?25`), modes DECCKM (`?1`), DECKPAM, bracketed paste `?2004`, mouse `?1000/1002/1003` + encodings `?1005/1006/1015`, focus `?1004`, sync `?2026` (always emitted off), title (OSC 2), cwd (OSC 7). Budget: ≤ 150 KB and ≤ 15 ms for 200×60 + 1000 lines.

### 9.4 Keyboard routing
`attachCustomKeyEventHandler`: Kelta chords (from `keys.*`, matched on the layout character for letters and punctuation, `KeyboardEvent.code` otherwise, + modifiers) and the prefix key return `false` (consumed); everything else reaches xterm → `onData` → `session_write`. Per-kind remaps (`terminal.shift_enter`): Claude default `esc-cr` (`\x1b\r`), others `passthrough`. Never send unsolicited CSI-u. macOS `Option as Meta` via `macOptionIsMeta` (+ left/right-only handling in the key handler). Default webview shortcuts (reload, zoom, find, context menu) are disabled.

### 9.5 Memory budget for scrollback (L1)
Scrollback per kind (`terminal.scrollback`: shell 3000, claude 3000, editor 500, tool 500, setup 1000). Global cap `terminal.memory_cap_mb` (default 160): computed from `history_lines × cols × 24 B` per session, updated on line growth (counter, event-driven). On exceed: shrink history of least-recently-viewed sessions to 500 lines (never below), oldest first; the host emits `TerminalEvent::MemoryCapReached` once and core raises a warning toast. Snapshots carry `terminal.view_scrollback` history lines (`TerminalLimits.view_scrollback`).

### 9.6 On-disk history log (L1)
Output older than the in-memory scrollback stays searchable and restorable without RAM growth (`terminal.history_log`, default on).
- **Capture:** the model's `Handler` wrapper copies screen rows as plain text (ANSI stripped, wrapped rows joined, trailing blanks trimmed) just before alacritty rotates them into the primary history: linefeed/auto-wrap at the bottom of a top-anchored scroll region, `CSI S`, delete-lines at row 0, `ED 2`. The alternate screen and regions below the top never reach history and are not logged. At reader exit the remaining screen rows are appended. Not captured: rows pushed into history by a resize reflow.
- **Write path:** the reader appends to a per-session buffer under the session lock and hands it to one `kelta-history` writer thread (`try_send` on a bounded queue) past 64 KiB, when its `poll` goes idle, and at exit (exit waits for the write so quit cannot lose it). No timer. A full queue keeps the text for the next flush up to 1 MiB, then drops it behind a `[kelta: … not saved]` marker line. Other messages (caps, delete, sync) that find the queue full go through one `kelta-history-send` thread in order; while any wait there, later ones (appends included) follow them, so a deferred delete never lands after a newer append.
- **Files:** `<data>/history/<id>.log` rotates to `<id>.1.log` past half of `terminal.history_log_mb` (default 16); past `terminal.history_log_total_mb` (default 512) the oldest files (mtime) are deleted. Session ids that are not `[A-Za-z0-9_-]{1,128}` get no file. A crash can only cut the last line; the first append after a start terminates it, readers treat it as a line.
- **Read path:** `history_tail` (Dormant `session_text_tail`; a respawn under the same id first feeds the last `view_scrollback` lines into the new model's scrollback above a cleared screen, before logging starts), `history_search` (case-insensitive substring, newest `limit` per session; buffered lines are flushed first, the visible screen is not searched). Both wait for pending writes (a Sync message); `history_tail` shares one 2 s deadline between that wait and the read, which runs on one `kelta-history-read` thread (a hung disk holds that thread, callers get an empty tail), so a stalled writer costs keltad's connection loop at most 2 s; `history_search` scans on the caller's thread. A rotation during a read can skip or repeat lines near the boundary. `history_delete` runs when core deletes the session row (close, quit without restore, startup prune).

---

## 10. Storage (SQLite `<data>/kelta.db`, WAL, migrations in kelta-core)

```
schema_version(v)
projects_open(project_id PK, ord, active)              -- open set + order; config lives in TOML
layouts(project_id PK, json, rev, updated_at)
sessions(id PK, project_id, kind_json, spec_json, name, work_item_id, restore_json, cwd, lifecycle, text_tail, updated_at,
         tasks_json /*v18*/)
work_items(id PK, project_id, kind, ticket_json, review_json, repo_id, worktree, branch, base, claude_uuid, nvim_socket,
           tab_id, pr_url, state_json, session_ids_json /*'[]'*/, created_at, updated_at,
           review_due, claude_replied /*v2*/, title, pr_title_needs_key /*v3*/, sent_threads_json, rebase_json /*v4*/,
           claude_at /*v5*/, claude_message, delta_json, next_note, left_at /*v6*/, port_base /*v7*/, cost_usd /*v8*/,
           auto_finish /*v9*/, parked_at, nvim_kept /*v12*/, run_json /*v14*/, ticket_source /*v15, backfilled by v17*/,
           feedback_at /*v16*/)
work_steps(work_item_id, step, status /*pending|running|done|failed|skipped*/, detail, updated_at, PK(work_item_id, step))
seen_reviews(account, repo, number, head_sha, first_seen, reviewed_sha /*v6*/, PK(account, repo, number))
provider_cache(key PK, etag, body_json, fetched_at)
plugin_grants(plugin_id, permission, granted_at, manifest_sha256, PK(plugin_id, permission))
plugin_kv(plugin_id, key, value, PK(plugin_id, key))      -- screens' kv.* (PLUGINS §7); value = JSON text
repo_trust(path PK, sha256, trusted_at)
trigger_log(id PK, ts, trigger_id, event, ok, detail, depth)   -- capped 1000 rows
ui_state(key PK, value)                                   -- small Kelta-local JSON values, keys below
notes(id PK, work_item, path, line_start, line_end, body, source, ext_ref, state, sent_at)   -- review notes, #133 (v10)
nudges(repo, number, at, PK(repo, number))                -- my last nudge per PR (one per 24 h, `review_nudge`, v11)
next_up(ticket_key PK, project, ticket_json, rank, snoozed_until)   -- my Next up list and snoozes, #145 (v13)
seen_tickets(ticket_key PK, at)                           -- tickets I have seen (the `New` badge, v13)
ticket_steps(ticket_key PK, source_id, step_id, entered_at, entered_by)   -- where each ticket stands in its pipeline, #461 (v19)
```
Migrations: `crates/kelta-core/src/store/migrations.rs`, one per schema version (19 today, `kelta_proto::store::SCHEMA_VERSION`), each in its own transaction, recorded in `schema_version`.

`ui_state` keys: `tags` (`LocalTags`, #349), `start_options` (the last picked start options, a JSON map `PlanOptions.key` → values, #357), `claude.permission_mode` (the last mode a Claude session switched to, #277). Window geometry is in JSON files (§2.1); the WebGL probe result and the onboarding flag (`kelta.onboarding.done`) are in the webview's `localStorage`.

All writes go through the single sqlite thread. Startup reads (open projects, layouts, provider_cache) happen before window creation.

---

## 11. Security model

### 11.1 Local surfaces
- ctl socket: 0600 in a 0700 dir owned by the user; peer uid must equal ours (`SO_PEERCRED` / macOS `getpeereid`, §2); hook frames must carry the session's `KELTA_HOOK_TOKEN` (constant-time compare). Other ctl commands, `outside_hook` included, are allowed for same-uid peers. keltad's socket checks the peer uid on both ends the same way.
- HTTP server: binds `127.0.0.1` only; `/mcp/<sid>` and `/hook/<sid>` require `Authorization: Bearer <per-session token>`, `/mcp` the install token (closed while Kelta's MCP is not installed in Claude Code); the per-instance web-tool proxy listeners (kelta-plugins) serve `/proxy/<instance>/…` with an unguessable instance path segment (128-bit), only proxy to the tool's own loopback origin and require `Host: 127.0.0.1:<listener port>`; `Host` header must be `127.0.0.1:<port>` (DNS-rebinding guard).
- Per-session runtime files 0600; never inside the worktree (no repo pollution).
- Claude IDE bridge (§8.5, opt-in `claude.ide_bridge`; off → no listener, no lock file, no env). Threat model: other local users, browsers (DNS rebinding / cross-site WebSocket), other same-user processes. Controls:
  - binds `127.0.0.1` only, random port, one listener per Claude session, stopped with the session;
  - handshake requires `x-claude-code-ide-authorization: <128-bit random token>` (uuid v4, OS CSPRNG), compared in constant time (`auth::ct_eq`); missing/wrong → 401 before the upgrade; the token is never logged;
  - any `Origin` header → 403: browsers always send one on WebSocket handshakes, Claude Code does not, so a web page cannot reach the bridge even with DNS rebinding (the `Host` header is not pinned because Claude may dial `localhost`);
  - the lock file holding the token is written via a fresh `create_new` 0600 temp file (never follows an existing file or symlink) then renamed; the `ide` dir is created 0700; locks are removed on session end and quit, and startup cleanup removes only Kelta locks whose pid is dead, never other IDEs' files;
  - residual risk: any process of the same user can read the lock file and drive the tools — the same trust level as the VS Code/JetBrains integrations, and that process could already run `nvim --server` or edit files directly. Tools can open a file or show a diff in the user's editor and read nothing back except the session cwd; `openDiff` writes only into Kelta's 0700 runtime dir, never the target file. Requests are unbounded in count per connection but each is answered or parked (pending diffs are freed on disconnect).
- Tokens, secrets and tokenized URLs (ISL) are never logged (`tracing` field redaction helper in proto).

### 11.2 Webview
- CSP (tauri.conf): `default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline'; img-src 'self' data: https:; font-src 'self'; connect-src ipc: http://ipc.localhost; frame-src kelta-plugin: http://127.0.0.1:* http://localhost:*; object-src 'none'; base-uri 'none'; form-action 'none'`.
- Tauri app manifest declares every command (`build.rs` from `commands/names.rs`); capability `capabilities/main.json` grants them only to the windows `main` and `detached-*` (#342) and the local origin; no `remote` URLs. Plugin and tool iframes get nothing.
- Ticket/PR HTML is sanitized in Rust (ammonia: no scripts, no styles, no iframes, links `rel=noopener` and opened via `open_external`).

### 11.3 Plugins
- Plugin screens: `<iframe sandbox="allow-scripts allow-forms">` (no `allow-same-origin`, no popups, no top-navigation) loading `kelta-plugin://<id>/<path>`; the scheme handler serves only canonicalized files inside the plugin dir and adds `Content-Security-Policy: default-src 'self' kelta-plugin://<id>; script-src kelta-plugin://<id>; style-src kelta-plugin://<id> 'unsafe-inline'; img-src kelta-plugin://<id> data:; connect-src 'none'; frame-ancestors 'self'`.
- Bridge: host creates a `MessageChannel` per instance, transfers `port2` only after verifying `event.source === iframe.contentWindow`; every call → `plugin_call(instance_id, method, params)`; Rust checks the plugin's grants (SQLite) and the method's required permission (PLUGINS.md §4). `net:<host>` is enforced in Rust (`http_fetch`), exact host or `*.domain` glob, https only (http only for `127.0.0.1`/`localhost` when granted explicitly). Grants are re-prompted when a manifest update adds permissions.
- **Security gate S1:** a sandboxed plugin iframe and a localhost tool iframe cannot reach `window.__TAURI_INTERNALS__`, `window.parent` DOM, or invoke any command, on WKWebView and WebKitGTK: automated on WebKitGTK by `scripts/sandbox-check.sh`, manual on WKWebView (BUILD_PLAN §6.1). Iframes can post to `window.webkit.messageHandlers.ipc`; Tauri's per-launch invoke key (main frame only) is what rejects those messages.
- Declarative tools/triggers from repo-local `.kelta/config.toml` that execute anything (`command`, `run`, `setup`, `http`, session template shell leaves, `env`, editor `review_args`, `claude.append_system_prompt`) are **inert until trusted**: trust = SHA-256 of the file content stored in `repo_trust`; any change → untrusted again (banner).
- Provider plugins (KPP, PLUGINS §9): run only with the `provider` grant (checked on every call), get the account's settings and resolved secret per call and nothing else (no inherited environment, so `env:` secret refs of other accounts stay out of reach), cannot answer for another account (refs are rewritten to the calling account) and their HTML is re-sanitized in Rust.
- Trigger engine: recursion guard (depth ≤ 4, a trigger never re-fires within a chain it started), per-trigger rate limit (10 runs / 60 s), `send_keys` requires `allow_send_keys = true` on the trigger and is rate-limited (1 / 2 s per session).

---

## 12. Error handling

### 12.1 Rust
- Library crates: `thiserror` enums mapped into `KeltaError` at crate boundary; never `unwrap()` outside tests (clippy `unwrap_used = deny` in lib crates).
- Panics: each PTY reader thread and each spawned task is wrapped (`catch_unwind` / `JoinHandle` supervision); a panic marks that session `Error` / account `error` and logs; never aborts the app.
- Provider errors are per account: one failing account never empties an aggregated view (`errors: Vec<AccountError>` alongside items).
- Work saga: each step journaled in `work_steps`; failure → `WorkState::Failed{step}` + toast with **Retry** / **Skip** / **Open in browser**; tracker side effects never roll back git work.

### 12.2 UI
- Every pane has explicit states: loading (skeleton only after 150 ms), empty (with the next action), error (message + retry + "Open settings"/"Open diagnostics" where relevant), stale (cached data + "updated 5 min ago").
- Toasts for async failures; never modal except destructive confirmations (kill running Claude, force-remove worktree, uninstall plugin, quit with working sessions).

### 12.3 Config errors
Parse/validation error → keep last good config, toast `file:line:col message`, Settings shows the issue list. Startup with invalid global config → defaults + banner.

### 12.4 Webview crash / hang
- WebContent termination (Linux `web-process-terminated`, macOS `webViewWebContentProcessDidTerminate`) → reload; views re-attach.
- `AckTimeout` on any session while the window is visible ⇒ hung UI ⇒ reload with `?safe=1` (plugin screens not auto-remounted) + toast "A plugin screen stopped responding and was closed".
- **Launch crash guard (Linux):** `<data>/launch-guard` written in `pre_init`, removed on `app_ready`. If present at next launch → start once with `linux.graphics` safe profile + banner; twice → print `kelta --safe-graphics` and diagnostics hint to stderr.

---

## 13. Performance contract (hard)

Reference workload `bench/fixtures/3p10s`: 3 open projects, 10 live sessions (3 `fake-claude`, 3 `nvim --clean`, 2 bash, 1 `tui-sim`, 1 lazygit-like `tui-sim --alt`), ~500 lines each, 4 visible panes, tickets + reviews lists loaded from fixtures, idle 30 s. Children are **excluded** from Kelta's numbers and reported separately.

| Metric | Budget | Mechanism | Measurement (`kelta-bench`) |
|---|---|---|---|
| Idle footprint (core + WebKit helpers) | ≤ 220 MB macOS (sum `phys_footprint`), ≤ 250 MB Linux Mesa (sum PSS); NVIDIA proprietary: informational, +30 % allowed | §9.1 pool, bounded scrollback, Svelte, no web fonts, lazy chunks, lazy HTTP server, `M_ARENA_MAX=2`, WebKitGTK `CacheModel::DocumentViewer`, iframes destroyed when hidden | Linux `/proc/<pid>/smaps_rollup` Pss over kelta + `WebKit*` descendants; macOS `proc_pid_rusage(RUSAGE_INFO_V4).ri_phys_footprint` of kelta + `com.apple.WebKit.*` started after launch |
| Core process alone | ≤ 60 MB | | same |
| Marginal hidden session (500 lines) | ≤ 2 MB | alacritty lazy rows, no xterm | delta when +1 session |
| Background mode (window closed) | ≤ 60 MB | webview destroyed | same probe |
| Cold start → restored layout interactive | ≤ 400 ms macOS M1, ≤ 700 ms Linux | config + sqlite read before window; Dormant not spawned; caches render first; heavy views dynamic-imported | process start → `app_ready` (first rAF after mount), median of 5 |
| Project switch → interactive | ≤ 50 ms warm (pooled), ≤ 120 ms cold (snapshot) | pool + snapshot | in-app marks |
| Keydown → PTY write | ≤ 2 ms p99 macOS, ≤ 3 ms Linux | direct invoke, no batching | UI timestamp vs Rust write timestamp (bench build) |
| Echo → painted | ≤ 1 frame p95 (WebGL), ≤ 2 frames (Linux DOM) | binary channel, one frame per read | `cat` echo probe + rAF |
| Claude-like redraw (G2) | UI frame p95 ≤ 25 ms Linux DOM | flow control | `tui-sim --ink` full-screen redraw 30 Hz |
| Flood `cat` 200 MB visible | UI frame < 50 ms throughout; webview mem delta < 30 MB; in-flight ≤ 256 KiB/session | ack watermark + snapshot catch-up | scenario `flood` |
| Idle CPU | ≤ 0.2 % avg, ≤ 1 wakeup/s over 120 s unfocused | scheduler is sole timer owner; no JS intervals | `/proc/<pid>/task/*/status` ctxt switches, `ri_pkg_idle_wkups`; utime+stime |
| Bundle | initial JS ≤ 200 KB gz; each lazy chunk ≤ 120 KB gz; dmg ≤ 20 MB; deb ≤ 25 MB | dynamic imports | `bench/bundle-size.mjs`, artifact sizes |

**Enforcement lints:** `clippy.toml` `disallowed-methods` (workspace-wide, `disallowed_methods = "deny"`): `tokio::time::interval`, `tokio::time::interval_at`, `std::thread::sleep`, `std::thread::spawn` (allowed only in `kelta-term::reader`, `kelta-term::history`, `kelta-term::daemon` (keltad connection threads), `kelta-core::scheduler`, `kelta-core::store` and tests via `#[allow]` + `// allowlisted:`), `std::process::exit`; `reqwest::Client::new` / `Client::builder` / `ClientBuilder::new` / `reqwest::get` (reqwest is built without a TLS provider: use `kelta_http::client_builder`, #191); `std::process::Command::new` / `tokio::process::Command::new` (use `kelta_proto::term::command` / `async_command`, §7.2, #358). `tokio::time::sleep` is not lint-banned: it is allowed only as an event-armed one-shot marked `// one-shot: <reason>` (BUILD_PLAN §1.5); `scripts/check-no-timers.sh` fails on `setInterval` and recursive `requestAnimationFrame` in `ui/src` outside `ui/src/lib/terminal/raf.ts`.

---

## 14. Platform specifics (summary; details in SPEC.md §8)

- Linux: Tauri → WebKitGTK `webkit2gtk-4.1` (GTK3). `pre_init` (before any GTK init, single-threaded): read `[linux.graphics]` with a minimal TOML parse; NVIDIA detect (`/proc/driver/nvidia/version` or `/sys/module/nvidia_drm`) → `WEBKIT_DISABLE_DMABUF_RENDERER=1` + `__NV_DISABLE_EXPLICIT_SYNC=1`; no `/dev/dri/renderD*` (VM, container) → `WEBKIT_DISABLE_COMPOSITING_MODE=1` (software GL compositing costs ~4× per frame, gate G2); toggles for `WEBKIT_DISABLE_COMPOSITING_MODE`, `GDK_BACKEND=x11`; `--safe-graphics` sets all; never set `GTK_IM_MODULE`. `mallopt(M_ARENA_MAX, 2)`. app_id/WM_CLASS `dev.kelta.Kelta`. `window.decorations=auto`: `none` if `HYPRLAND_INSTANCE_SIGNATURE` or `SWAYSOCK`, else `native`. No in-app global shortcuts: `kelta-ctl toggle` bound in compositor; raise uses `XDG_ACTIVATION_TOKEN` when provided.
- macOS: WKWebView, WebGL default, Cmd is the app modifier, custom app menu replaces Tauri's default (Cmd+H/M/Q intentional; Cmd+W reaches the keymap as `pane.close`, Window → Close Window is Cmd+Shift+W, since menu key equivalents win over the webview), dock badge = sessions needing input, window close → background mode by default, `/tmp/kelta-<uid>` runtime dir, menu labels translated (`kelta_proto::i18n`). Signing and notarization of builds: CONTRIBUTING.md ("CI").

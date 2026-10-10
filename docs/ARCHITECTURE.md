# Kelta v0.1 — Architecture (contract)

Status: **frozen contract** for the parallel build. Changes go through the merge owner (see BUILD_PLAN.md §5).
Companion docs: SPEC.md (product), SETTINGS.md (config schema), PLUGINS.md (extensibility), BUILD_PLAN.md (lanes).

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

All versions exact (`=x.y.z` in Cargo, no `^`/`~` in package.json). Verified against crates.io / npm on 2026-10-09. The scaffold re-verifies with `cargo info` / `npm view` and fails if any pin does not resolve.

### 1.1 Rust (toolchain `1.99.0`, edition 2024, `rust-toolchain.toml` with `rustfmt`, `clippy`)

| Crate | Version | Features / notes | Used by |
|---|---|---|---|
| tauri | 2.12.2 | default features; no `unstable`, no `devtools` in release | desktop |
| tauri-build | 2.7.1 | app manifest lists every command (explicit permissions) | desktop |
| tauri-plugin-notification | 2.5.1 | | desktop |
| tauri-plugin-opener | 2.7.0 | open URLs/files externally | desktop |
| tauri-plugin-single-instance | 2.5.2 | forwards argv of 2nd launch | desktop |
| tokio | 1.53.2 | rt-multi-thread, macros, net, process, sync, time, io-util, fs, signal | all async crates |
| portable-pty | 0.9.0 | spawn/openpty only, behind `PtyBackend` trait; rustix fallback | kelta-term |
| alacritty_terminal | 0.26.0 | `default-features = false` (no serde); headless `Term`, `vte::ansi::Processor` (vte 0.15 transitive) | kelta-term |
| rustix | 1.1.5 | pty, process, event (poll), net (peer creds), termios, fs | term, server, work, platform |
| libc | 0.2.190 | `mallopt(M_ARENA_MAX)`, `malloc_trim` (glibc only) | desktop/platform |
| reqwest | 0.13.5 | `default-features = false`, features `rustls, json, gzip, http2, query, system-proxy` | kelta-http |
| axum | 0.8.9 | lazy loopback HTTP server (MCP, http hooks, web proxy) | kelta-server, kelta-plugins |
| hyper | 1.12.0 | proxy client | kelta-plugins |
| hyper-util / http-body-util | 0.1.21 / 0.1.5 | `client-legacy, tokio, http1`: hyper 1.x client + `TokioIo` (already in the graph via axum) | kelta-plugins |
| tokio-tungstenite | 0.30.0 | proxy WebSocket passthrough; axum 0.8.9 `ws` brings its own 0.29, so both are in the lockfile: never mix axum `WebSocket` types with 0.30 types | kelta-plugins |
| rmpv | 1.3.1 | hand-rolled msgpack-RPC client for nvim (`nvim-rs` not used: stale) | kelta-work |
| rusqlite | 0.40.2 | `bundled`; WAL; `PRAGMA cache_size=-2000` | kelta-core |
| toml | 1.1.8 | | kelta-config, kelta-plugins |
| toml_edit | 0.25.17 | comment-preserving writes | kelta-config |
| schemars | 1.2.2 | settings JSON Schema | kelta-proto, xtask |
| jsonschema | 0.58.6 | `default-features = false` (no remote `$ref` resolution); validate settings + plugin manifests + plugin settings fragments | kelta-config, kelta-plugins |
| serde / serde_json | 1.0.229 / 1.0.151 | | all |
| notify / notify-debouncer-full | 8.2.0 / 0.7.0 | watch config dirs (FSEvents / inotify, no polling) | kelta-config |
| keyring-core | 1.0.0 | | kelta-secrets |
| apple-native-keyring-store | 1.0.2 | feature `keychain` (macOS only target dep) | kelta-secrets |
| zbus-secret-service-keyring-store | 1.0.1 | feature `rt-tokio-crypto-rust` (Linux only) | kelta-secrets |
| zbus | 5.19.0 | `default-features = false`, feature `tokio`; Linux: notification daemon / Secret Service probes | kelta-core, kelta-secrets, desktop |
| arboard | 3.6.1 | `default-features = false`, feature `wayland-data-control` (no image data); CLIPBOARD + PRIMARY | kelta-core |
| pulldown-cmark / ammonia | 0.13.4 / 4.2.1 | markdown → sanitized HTML | kelta-providers-common (in kelta-http crate `markdown` module) |
| uuid | 1.27.0 | v4, v7, serde | proto |
| regex / globset | 1.13.1 / 0.4.20 | | many |
| directories / which | 6.0.0 / 8.0.6 | | config, work, plugins |
| thiserror / anyhow | 2.0.21 / 1.0.104 | anyhow only in binaries/tests | all |
| tracing / tracing-subscriber / tracing-appender | 0.1.44 / 0.3.23 / 0.2.5 | file log, size-capped, `info` default, no stdout in release | all |
| ts-rs | 12.0.1 | export DTOs to `ui/src/lib/gen/` | proto |
| async-trait | 0.1.92 | object-safe provider/core traits | proto + impls |
| base64 / sha2 / time / bytes / parking_lot / futures / semver / mime_guess | 0.23.1 / 0.11.0 / 0.3.55 / 1.12.1 / 0.12.5 / 0.3.34 / 1.0.28 / 2.0.5 | `semver`: plugins, config, work | various |
| objc2 / objc2-foundation / objc2-web-kit | 0.6.5 / 0.3.2 / 0.3.2 | macOS only: WebContent-terminated delegate + bench WebContent pid | desktop (L10) |
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
| @xterm/xterm | 6.0.0 | exact |
| @xterm/addon-fit / -unicode11 / -web-links | 0.11.0 / 0.9.0 / 0.12.0 | eager |
| @xterm/addon-webgl / -search | 0.19.0 / 0.16.0 | dynamic import |
| vitest / jsdom / @testing-library/svelte | 5.0.3 / 30.1.2 / 5.4.2 | |
| @xterm/headless | 6.0.0 | dev only: parser-hook tests (query swallowing) |
| @playwright/test | 1.64.0 | UI e2e against Vite dev server + IPC mock |
| eslint / typescript-eslint / eslint-plugin-svelte | 10.12.0 / 8.71.1 / 3.23.1 | |
| prettier / prettier-plugin-svelte | 3.9.9 / 4.1.1 | |
| @eslint/js / globals | 10.0.1 / 17.12.0 | root devDependencies for the flat `eslint.config.js` (the lint toolchain, `svelte` and `typescript` are also declared at the root) |
| @types/node | 22.20.5 | ui, `vite.config.ts` and Node-side test helpers |

`jsdom 30.1.2` declares `engines.node ^22.22.2`; it runs on Node 22.16+, and `.npmrc` sets `engine-strict=false`. `packageManager` is `pnpm@9.12.1`.

**Not used:** `@xterm/addon-canvas` (dead), `@xterm/addon-serialize` (Rust snapshots), `@xterm/addon-clipboard` (OSC 52 handled by the Rust model), any CSS framework, icon font, web font, markdown lib, state lib. Icons: one inline SVG sprite.

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
- **Control socket:** `<runtime>/ctl.sock` (mode 0600, dir 0700, owner verified; peer uid checked with `SO_PEERCRED` / `getpeereid`). Line-delimited JSON (§7.3).
- **`kelta-ctl start --task "<text>" [--project <id>]`** sends `CtlCommand::StartTask{task, project}`: core plans `WorkSource::Branch{name: "", task}` for the project (default: active), runs the start-work saga without a sheet and focuses the project. Same branch rule and refusals as New work item (`⇧⌘N`); a ticket key and `--task` together are a usage error.
- **Lazy HTTP server:** axum on `127.0.0.1:<random>`; starts on first need (first Claude session with `claude.mcp = true` or `claude.hook_transport = "http"`); stops when its consumer count reaches 0 (event-driven, no idle timer). Core and kelta-work each hold a consumer per Claude session they spawn (refcounted, so double counting is harmless). MCP is a hand-rolled stateless Streamable-HTTP subset (`POST` → `application/json`, `GET`/`DELETE` → 405, no `Mcp-Session-Id`): rmcp's transport keeps sessions alive with periodic SSE pings, which the no-periodic-timer rule (§13) forbids. MCP tools: `get_ticket`, `transition_ticket`, `add_ticket_comment`, `open_in_editor`, `create_pr`, `list_review_requests`, `get_review_feedback` (Markdown of `CoreApi::work_feedback` for the session's work item: unresolved threads with `path:line`, review summaries, failed checks with their log tail; the layout of the Fix with Claude sheet's `feedback.md`), `add_review_comment{path, line, body}` (line comment on the pending review of the PR under review in the session's review-kind work item, via `CodeHost::add_pending_comment`; published by the user's a / c / m in the review detail), `notify` (PLUGINS §8).
- **Web-tool proxy:** kelta-plugins serves the proxy (`proxy::ensure_listener`) on its own loopback listener per proxied tool instance (started on first open, stopped with the instance), so each web tool keeps a distinct origin and needs no kelta-server port.
- **Session daemon (`keltad`):** `<runtime>/keltad.sock` (0600 in the 0700 runtime dir, peer uid checked on both ends). Core launches it from the stable copy `<data>/bin/<version>/keltad` (`keltad --socket <path> --history <data>/history`, stderr → `<logs>/keltad.log`); it binds, forks into its own session, and the launcher connects once the parent exits. Messages: `u32 len` + `u32 json_len` + JSON head + raw bytes (input, frames). Each session's events go to the client that spawned or last adopted it. With `session_host = daemon` the on-disk history log (§9.6) lives in keltad, written only when `--history` is given. Core closes every session it removes (`kill` on the exited session), so keltad drops its model and route. On quit, sessions core would restore stay running (no SIGHUP); the others are killed and closed. On start, core `adopt`s every persisted session keltad still runs (Live, same hook token) before any Dormant respawn and kills the rest. keltad exits 30 s after it has no client and no running session (one-shot grace armed by the disconnect / exit, no polling).
- **Bundling:** `kelta-ctl` and `keltad` are added as `bundle.externalBin` only in the release config overlay `packaging/tauri.release.json` (`tauri build --config …`), so dev builds and `cargo clippy` never require the sidecar to exist.
- **Background mode:** closing the window with `window.close_behavior = background` destroys the webview (WebKit processes exit) while core + PTYs keep running; `kelta`, Dock click or `kelta-ctl toggle` recreates it and views re-attach from snapshots.

### 2.1 Filesystem locations

| Kind | Linux | macOS |
|---|---|---|
| config | `$XDG_CONFIG_HOME/kelta` (`~/.config/kelta`) | `~/.config/kelta` (honours `XDG_CONFIG_HOME`) |
| data (db, plugins, bin, logs) | `$XDG_DATA_HOME/kelta` (`~/.local/share/kelta`) | `~/Library/Application Support/dev.kelta.Kelta` |
| state db | `<data>/kelta.db` | same |
| terminal history logs | `<data>/history/` (0700): `<session id>.log` + `<session id>.1.log` (0600), §9.6 | same |
| logs | `$XDG_STATE_HOME/kelta/logs/kelta.log` (5 MB × 2) | `~/Library/Logs/Kelta/kelta.log` |
| stable CLI copy | `<data>/bin/<version>/{kelta-ctl,keltad}` + `<data>/bin/current` symlink | same |
| runtime | `$XDG_RUNTIME_DIR/kelta` (fallback `/tmp/kelta-<uid>`) | `/tmp/kelta-<uid>` |
| per-session runtime | `<runtime>/s/<sid8>/` (0700): `claude-settings.json` (0600), `mcp.json` (0600), `ticket.md`, `context.md`, `nvim.sock` | same |
| Claude IDE bridge (§8.5) | `<claude dir>/ide/<port>.lock` (0600, dir created 0700) per Claude session; `openDiff` proposals in `<runtime>/ide/<random>/` (0700) while shown | same |

`KELTA_RUNTIME_DIR` (both OSes) replaces the runtime dir and makes the instance skip the single-instance handshake, so a bench or test instance never talks to the user's running Kelta.

`<sid8>` = last 8 hex chars of the session uuid (the random tail of the v7 uuid; its first 32 bits are the millisecond clock), reserved by core under one lock (collision-checked). kelta-work names the runtime dirs of the sessions it spawns (Claude, editor) itself — `<runtime>/s/<key8>/`, recorded in its saga journal and `WorkItem.nvim_socket` — so core never assumes `<runtime>/s/<sid8>/` for work-item sessions. All socket paths are asserted < 100 bytes at startup.

---

## 3. Crates and module map

```
crates/kelta-proto      [scaffold]  ids, DTOs, IPC request/response types, UiEvent/BusEvent, settings structs (+Default, JsonSchema),
                                    plugin manifest/tool/trigger types, hook payloads, frame consts, ActionId catalog,
                                    service traits (§4), KeltaError, `testing` feature: FakeCore, FakeTerminalHost, FakeTracker,
                                    FakeCodeHost, FakeSecrets, FakeSettings, fixtures
crates/kelta-term       [L1]  TerminalHost impl: PtyBackend (portable-pty | rustix), login-env exec, reader threads, alacritty model,
                              query responder, snapshot encoder, flow control, scrollback memory budget;
                              `daemon`: the `keltad` binary and the `DaemonTerminalHost` client
crates/kelta-config     [L4]  layered load/merge/provenance, validation, toml_edit writes, hot reload, repo trust check helpers,
                              project files CRUD, early `linux.graphics` reader
crates/kelta-secrets    [L4]  SecretRef resolution chain, keyring stores, backend status
crates/kelta-http       [L5]  shared reqwest client, HttpCtx (retry, backoff, rate limits, ETag LRU), markdown→HTML
                              (scaffold ships a functional baseline: plain send, no retry)
crates/kelta-trackers   [L5]  Jira Cloud + DC, Redmine, GitHub Issues (+Projects v2), GitLab Issues, Gitea/Forgejo Issues, Linear; ADF walker
crates/kelta-codehosts  [L5]  GitHub (GraphQL list, REST actions, notifications gate), GitLab (REST), Bitbucket Cloud (REST 2.0), Gitea/Forgejo (REST v1)
crates/kelta-work       [L6]  git CLI ops, templates (branch/path/slug), work saga (journaled), Claude launcher, editor adapters
                              (nvim msgpack-RPC, vim keys, emacsclient, helix, external), review_start
crates/kelta-server     [L7]  ctl socket server, hook ingestion + status machine, lazy axum server, MCP endpoint, http hooks
crates/kelta-ctl        [L7]  CLI binary
crates/kelta-plugins    [L8]  manifest load/validate/install, registry, grants, plugin_call gate, kelta-plugin:// handler, tools
                              registry + web-tool lifecycle + proxy, trigger engine, KPP provider processes (`kpp`,
                              depends on kelta-http as a provider host)
crates/kelta-core       [L3]  AppState composition, ProjectRegistry, SessionRegistry, Layout store, Store (sqlite + migrations),
                              EventBus, Attention, Scheduler, ProviderRegistry + aggregation + caches + seen_reviews,
                              Notifier, clipboard, implements CoreApi
apps/desktop/src-tauri  [scaffold: Cargo.toml, build.rs, tauri.conf.json, capabilities/, main.rs, lib.rs, commands/mod.rs]
   src/commands/<domain>.rs  [owner per §6 table]
   src/platform/**           [L10] pre_init (linux graphics env, NVIDIA detect, mallopt, crash guard), diagnostics probes
   src/window/**             [L10] window setup, decorations, menu, background mode, UiBridge impl (bridge.rs: events fan-out,
                             badge, notifications), webview crash + hang reload
xtask                   [scaffold] `codegen` (ts-rs + schema), `codegen --check`
bench                   [L10] kelta-bench
ui/                     Svelte app (ownership in BUILD_PLAN)
packages/plugin-sdk     [L8] @kelta/plugin-sdk
```

Dependency rule: every lane crate depends only on `kelta-proto` (+ `kelta-http` for providers/core). `kelta-core` depends on all crates and wires them; only `apps/desktop` depends on Tauri. No crate other than desktop links Tauri; no crate links GTK/WebKit.

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
  async fn layout_open(&self, project: &ProjectId, req: OpenPaneRequest) -> Result<PaneRef, KeltaError>; // new tab / split / focus
  // projects & settings
  fn project(&self, id: &ProjectId) -> Option<ProjectInfo>;
  fn settings(&self, project: Option<&ProjectId>) -> Arc<Settings>;
  // providers
  async fn tracker_for(&self, account: &AccountId) -> Result<Arc<dyn Tracker>, KeltaError>;
  async fn code_host_for(&self, account: &AccountId) -> Result<Arc<dyn CodeHost>, KeltaError>;
  async fn review_list(&self, scope: Scope, kind: ReviewKind) -> Result<Vec<ReviewItem>, KeltaError>;
  // tracker writes with core-owned side effects (cache invalidation, ticket.transitioned / ticket.commented);
  // `session` = the acting session (MCP tools), added as event context
  async fn ticket_transition(&self, ticket: &TicketRef, transition_id: &str, fields: Option<Value>, session: Option<&SessionId>) -> Result<Ticket, KeltaError>;
  async fn ticket_comment(&self, ticket: &TicketRef, markdown: &str, session: Option<&SessionId>) -> Result<(), KeltaError>;
  // work & editor (core delegates to kelta-work)
  async fn work_for_session(&self, id: &SessionId) -> Option<WorkItem>;
  async fn work_create_pr(&self, id: &WorkItemId, draft: PrDraft, origin: ShipOrigin /*Ui|Mcp*/) -> Result<WorkItem, KeltaError>;
  async fn editor_open(&self, target: EditorTarget, path: &Path, line: Option<u32>) -> Result<(), KeltaError>;
  async fn editor_diff(&self, target: EditorTarget, old: &Path, proposed: &Path, close: bool) -> Result<(), KeltaError>; // IDE bridge openDiff, nvim RPC only (§8.5)
  // tools/plugins/bus/ui
  async fn tool_open(&self, project: &ProjectId, tool: &ToolId, ctx: TemplateCtx, placement: Placement) -> Result<ToolHandle, KeltaError>;
  fn publish(&self, ev: BusEvent);
  fn subscribe(&self) -> tokio::sync::broadcast::Receiver<BusEvent>;
  async fn notify(&self, n: Notification) -> Result<(), KeltaError>;
  fn toast(&self, t: Toast);
  async fn http_fetch(&self, req: ProxiedRequest) -> Result<ProxiedResponse, KeltaError>; // plugin net: (allowlist checked by caller); never follows redirects (own reqwest client in core)
  async fn ctl(&self, cmd: CtlCommand) -> Result<serde_json::Value, KeltaError>;            // dispatch of ctl socket commands
}

// ---- UI bridge (impl: apps/desktop window/bridge.rs, L10) consumed by kelta-core --------------
pub trait UiBridge: Send + Sync {
  fn emit(&self, ev: UiEvent);                          // fan-out to all events_subscribe channels; dropped if none
  fn set_badge(&self, needs_input: u32);                // macOS dock badge; Linux no-op
  fn request_attention(&self);                          // Linux urgency hint / macOS bounce (informational)
  fn notify(&self, n: Notification) -> Result<(), KeltaError>;  // tauri-plugin-notification
  fn window_state(&self) -> WindowState;                // {exists, visible, focused}
  fn reload_webview(&self, safe: bool);                 // hang/crash recovery (§12.4)
  fn webview_pids(&self) -> Vec<u32> { Vec::new() }     // WebKit helper pids for perf_snapshot (macOS)
}
// Services owned by other lanes that core calls (constructor signatures frozen in scaffold stubs):
// (exact signatures: BUILD_PLAN §2.2)
//   kelta_work::WorkService::new(core: Weak<dyn CoreApi>, store: Arc<dyn WorkStore>, dirs: Dirs) ; methods mirror §6 work_* commands (+ for_session)
//       core calls set_host(Arc<dyn kelta_work::WorkHost>) at boot: {ensure_http, release_http} over Server, run_blocking over PluginHost;
//       claude_restore_request(session, fallback) builds the argv when core restores a Dormant work-item Claude session
//   kelta_server::Server::new(core: Weak<dyn CoreApi>, dirs: Dirs) ; start_ctl(), ensure_http() -> port, register_session(sid, hook_token, mcp_token)
//   kelta_plugins::PluginHost::new(core: Weak<dyn CoreApi>, dirs: Dirs, grants: Arc<dyn GrantStore>) ; tools, triggers, screens,
//       uri::handle(host: &PluginHost, req) (the handler needs the plugin directories)
//       core calls wire(Wiring{ui, settings, settings_writer}) at boot and start() (trigger engine's own bus subscription)
//       once the runtime is up; triggers run without a UI
//   kelta_config::ConfigService::load(dirs: &Dirs, overrides: RuntimeOverrides) ; impl SettingsSource ; watch(on_change) ; set_plugin_schemas
//   kelta_secrets::Secrets::new(settings: Arc<dyn SettingsSource>) ; impl SecretResolver
//   kelta_term::PtyTerminalHost::new(env: LoginEnv, limits: TerminalLimits) ; impl TerminalHost ; kelta_term::resolve_login_env(timeout)
```

`WorkStore`, `GrantStore`, `TrustStore` are small traits in proto implemented by kelta-core over SQLite (fakes in `testing`).
`PluginSettingsSource { fn fragments(&self) -> Vec<(PluginId, serde_json::Value /*flat schema*/)> }` is implemented by `PluginHost` and handed by core to `ConfigService::set_plugin_schemas` (Plugin-defaults layer + validation of `plugins.<id>`). `Dirs { config, data, state, logs, runtime, bin }` is defined and resolved in proto (`Dirs::resolve(env, overrides)`, per §2.1) so every crate shares it.

---

## 5. Domain model (kelta-proto, all `Serialize + Deserialize + TS`)

```rust
pub struct ProjectId(String);   // slug [a-z0-9-]{1,40}; "home" reserved; "inbox" reserved pseudo id
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
pub enum RestorePolicy { None, Relaunch, ShellInCwd, ClaudeResume{ uuid: String }, Editor{ session_file: Option<PathBuf> } }
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
pub struct ClaudeMeta { session_uuid: String, model: Option<String>, preview: Option<String>, files_touched: Vec<PathBuf>, hooks_active: bool }
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
  rebase: Option<RebaseState{onto, pre_head, remote_sha: Option<String>, conflicts: Vec<PathBuf>, step: u32, total: u32}> }
pub enum WorkSource { Ticket{ ticket }, Review{ review }, Branch{ name, task: Option<String>, repo: Option<String> } } // Branch with empty name: name from work.scratch_branch_template + task slug; task is the {task} of claude.prompt_templates.standalone; no tracker steps
// review_due / claude_replied (FLOW §2.3): set only by kelta-work from a real `Stop` hook of the item's
// Claude (each `Stop` snapshots the whole working tree — tracked plus untracked non-ignored, through a
// temporary index, HEAD/index/stash untouched — to `refs/kelta/wi/<id>/last`; a non-empty
// `git diff <reviewed> <last>` (`refs/kelta/wi/<id>/reviewed`, else the merge base with <base>) → review_due
// + `delta` (ReviewDelta chip), else claude_replied; never for review checkouts; hooks of one session apply
// in order; `claude_message` keeps the full last message). Both refs are deleted by Finish and the startup prune, cleared by `UserPromptSubmit`; review_due also by a UI `work_create_pr` (`origin = ui`; MCP `create_pr` is `origin = mcp`), Finish and
// `work_mark_reviewed`; claude_replied by Finish. Store writes: `save` never writes these two (it keeps the
// stored values); `update(id, |w| ..)` re-loads under a write lock held only around load-modify-save and is
// their only writer. Long operations (`create_pr`, `finish`) end with `update` of their own fields.
// claude_uuid follows the session id of any hook of the item's Claude (/clear, in-Claude /resume).
pub enum WorkState { Planned, Starting, Active, PrOpen, Merged{ detail: Option<String> /*"choose Done status" | failed move*/ },
  PrClosed /*closed unmerged*/, Finished, Failed{ step: String, message: String } }
```

### 5.1 Layout model

```rust
pub struct Layout { project_id: ProjectId, tabs: Vec<Tab>, active_tab: Option<TabId>, rev: u64 }
pub struct Tab { id: TabId, title: String, work_item_id: Option<WorkItemId>, root: LayoutNode,
  focused_pane: Option<PaneId>, zoomed_pane: Option<PaneId> }
pub enum LayoutNode { Split{ dir: SplitDir /*Row|Column*/, ratios: Vec<f32> /*sum 1.0, each ≥ 0.05*/, children: Vec<LayoutNode> },
                      Pane{ id: PaneId, content: PaneContent } }
pub enum PaneContent {                       // serde tag = "kind"
  Terminal{ session_id: SessionId }, Web{ tool_instance_id: ToolInstanceId },
  PluginScreen{ plugin_id: PluginId, screen_id: String, instance_id: ScreenInstanceId, params: serde_json::Value },
  Tickets{ scope: Scope, view_id: Option<String>, mode: TicketsMode /*List|Board*/, who: Option<Who>, group: Option<TicketGroupBy> /*Flow|Status|Priority|Sprint|Assignee|Source|None; None = Flow*/, sort: Option<TicketSort> /*Priority|Updated|Age|Key; None = Priority*/, person: Option<String> /*assignee id, client-side*/ }, TicketDetail{ ticket: TicketRef },
  Reviews{ scope: Scope }, ReviewDetail{ review: ReviewRef }, Inbox, WorkItem{ id: WorkItemId },
  Settings{ section: Option<String> }, Diagnostics, Welcome, Empty }
pub struct OpenPaneRequest { content: PaneContent, placement: Placement, focus: bool, tab_title: Option<String>, work_item_id: Option<WorkItemId> }
pub enum Placement { NewTab, SplitRight, SplitDown, ReplaceFocused, Focused /*focus existing pane with same content if any*/ }
```

- Window = ProjectRail (Inbox, open projects, Home, "+") + active project workspace (TabBar + LayoutNode tree) + StatusBar.
- Only the active project workspace is mounted. A session is shown in **at most one pane**; opening it elsewhere moves it.
- Layout is saved by the UI via `layout_save` debounced 500 ms after a change (timer exists only while dirty) with optimistic `rev` (stale rev → `Conflict`, UI refetches).
- Project switch never touches processes; detached xterm instances go to the LRU (§9.4).

---

## 6. IPC command catalogue (Tauri commands)

Conventions: every command is `async`, takes one argument object (TS: `invoke('<name>', { ... })`), returns `Result<T, KeltaError>`. TS wrappers in `ui/src/lib/ipc/commands.ts` (scaffold, generated names + typed signatures), mocks in `ui/src/lib/ipc/mock.ts`. Binary payloads (`session_write` data, frames) use `Uint8Array` / `tauri::ipc::Response` / `Channel<InvokeResponseBody>`.

Wire format (frozen by the scaffold, checked by the fixture round-trips): enums serialize in `snake_case` (`ErrorCode` → `"not_found"`, `SessionStatus` → `"needs_input"`), except `PermissionMode` (Claude's camelCase values), `ClaudeEffort` (lowercase), `Osc52`/`ShiftEnter` (kebab-case), `PluginMethod` (`"tickets.list"`) and `Permission` (its string form). Tagged unions: `UiEvent`, `SessionKind`, `LayoutNode` tag `type`; `PaneContent`, `Scope`, `RestorePolicy`, `WorkState`, `WorkSource`, `ToolHandle`, `EditorTarget`, `Assignee` tag `kind`; `Cursor` adjacently tagged `{kind, value}`; `ActionDef` tag `action`; `CtlCommand` tag `cmd`. `Option` fields serialize as `null` (TS `T | null`; missing keys read as `None`); `u64`/`i64` are TS `number`. Small result DTOs not spelled out below (`SubscribeResult{sub_id}`, `LayoutSaveResult{rev}`, `ScreenOpenResult{instance_id, url}`, `PaneRef`, `ProjectPatch`, …) live in `kelta-proto`. The TS wrappers are camelCase functions taking the snake_case argument object (`projectClose({id, kill_sessions})`). In the scaffold stubs every command returns `Unsupported` except `app_info` and `events_subscribe`.

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
| **settings** | | | `commands/settings.rs` (L4) |
| `settings_schema` | `{}` | `serde_json::Value` (flattened JSON Schema incl. `plugins.<id>`) | |
| `settings_effective` | `{project_id?}` | `EffectiveSettings{value: Value, sources: BTreeMap<String /*dotted path*/, Layer>}` | |
| `settings_layer_get` | `{layer: Layer, project_id?, repo_id?}` | `LayerDoc{path: PathBuf, value: Value, text: String, trusted: Option<bool>}` | |
| `settings_set` | `{layer, project_id?, repo_id?, path: String, value: Value}` | `EffectiveSettings` | |
| `settings_reset` | `{layer, project_id?, repo_id?, path}` | `EffectiveSettings` | |
| `settings_validate` | `{layer, text: String}` | `Vec<ValidationIssue{path, message, line?, col?}>` | |
| `settings_write_raw` | `{layer, project_id?, repo_id?, text}` | `EffectiveSettings` (validate first; refuse on error) | |
| `settings_open_file` | `{layer, project_id?, repo_id?}` | `SessionInfo` (opens `$EDITOR`/nvim pane on the file) | |
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
| `terminal_set_palette` | `{palette: TerminalPalette}` | `()` (pushed on theme change) | |
| **tickets** | | | `commands/tracker.rs` (L3) |
| `tracker_list` | `{scope, view_id?, who?: Who, cursor?: Cursor, refresh: bool}` (`who` overrides `view.who` and has its own cache key; no `view_id` = union of the project's views, deduped by account + key) | `TicketPage{items: Vec<TicketItem{ticket, project_ids, view_ids, work_item_id?, prs: Vec<PrLink>, caps: TrackerCaps}>, next: Option<Cursor>, stale: bool, errors: Vec<AccountError>}` | |
| `tracker_get` | `{ticket: TicketRef}` | `TicketDetail` (with `prs` and the account's `caps`) | |
| `tracker_columns` | `{project_id}` | `Vec<Column>` | |
| `tracker_transitions` | `{ticket}` | `Vec<Transition>` | one call per ticket; the UI's StatusPicker matches several tickets by target status name |
| `tracker_transition` | `{ticket, transition_id, fields?: Value}` | `Ticket` (`NeedsFields` error carries `detail.fields`) | |
| `tracker_move` | `{ticket, column_id, project_id?}` (no `project_id`: the project whose binding or view account is the ticket's) | `Ticket` (resolves column → transition; `Conflict` + candidates if ambiguous) | |
| `tracker_comment` | `{ticket, markdown}` | `()` | |
| `tracker_assign` | `{ticket, assignee: Assignee /*Me|User{id}|None*/}` | `Ticket` | |
| `tracker_sources` | `{account_id, query}` | `Vec<SourceHit>` (source picker; `Unsupported` if the provider cannot list) | |
| `tracker_search` | `{scope, text}` | `Vec<TicketItem>` (palette) | |
| **reviews** | | | `commands/review.rs` (L3) |
| `review_list` | `{scope, kind: ReviewKind, refresh: bool}` | `ReviewPage{items: Vec<ReviewItem{review, project_ids}>, stale, errors}` | |
| `review_get` | `{review: ReviewRef}` | `ReviewDetail` | |
| `review_approve` | `{review, head_sha}` | `()` (`Conflict` if head moved) | |
| `review_comment` | `{review, body}` | `()` | |
| `review_request_changes` | `{review, body}` | `()` | publishes my pending line comments too (as do `review_approve` / `review_comment`) |
| **work** | | | `commands/work.rs` (L6) |
| `work_plan` | `{project_id, source: WorkSource /*Ticket{ticket}|Review{review}|Branch{name, task?, repo?}*/}` | `StartWorkPlan` (§SPEC 3.1) | Branch with a task (New work item): `Conflict` when the branch exists or has an item |
| `work_start` | `{plan: StartWorkPlan}` | `WorkItem` (progress via `work.updated`) | |
| `work_list` | `{project_id?}` | `Vec<WorkItem>` | |
| `work_resume` | `{id}` | `WorkItem` | |
| `work_retry_step` | `{id, step}` | `WorkItem` | `step` = a saga step id (re-run) or `skip:<step>` (mark skipped, continue) |
| `work_create_pr` | `{id, draft: PrDraft}` | `WorkItem` | Ship, `origin = ui` (clears `review_due`); MCP `create_pr` is `origin = mcp` (does not). `Conflict` while the item's Claude is `Working`/`NeedsInput` ("Claude is working in this worktree. Ship when it stops.", ui only), with no commits ahead of `<remote>/<base>` ("No commits ahead of main."), after a merge/close, or while another operation holds the item ("Claude is shipping this item." when it is Claude's MCP ship) |
| `work_pr_draft` | `{id}` | `PrDraft` | the title / body / draft Ship would use (prefills the dialog) |
| `work_finish` | `{id, opts: FinishOpts{remove_worktree, delete_branch, force, transition_to?}}` | `WorkItem` | for `Merged`/`PrClosed` items only an explicit `transition_to` moves the ticket; a clean merged branch is deleted with `-D` (squash merges) |
| `work_finish_merged` | `{ids}` | `FinishMergedReport{finished: Vec<WorkItem>, skipped: Vec<SkippedItem{id, reason}>}` | finishes the listed items still `Merged` with clean worktrees (remove worktree + delete branch); dirty / unpushed ones and those waiting for a Done choice are skipped |
| `work_check_prs` | `{}` | `()` | one `CodeHost::get` per unfinished item whose PR is missing from the authored open list; also run at startup (§8.4) |
| `work_link` | `{id, ticket: TicketRef, apply_side_effects: bool}` | `WorkItem` | scratch (Branch) items only; becomes Ticket-kind, branch never renamed; side effects = `work.on_start`, plus `work.on_pr` when a PR exists (then `pr_title_needs_key`) |
| `work_status` | `{id}` | `GitStatus{ahead, behind, dirty, unpushed, diverged, remote_new, files, insertions, deletions, missing}` (on demand, no fetch; ahead/behind and diffstat against `<remote>/<base>`, never the branch's upstream; diffstat from the merge base to the working tree, untracked files count in `files`; `missing` = worktree deleted outside Kelta; re-reads a recorded rebase). `diverged` = own rewrite: the recorded `remote_sha` is still the remote tip, is in `pre_head` and not in HEAD. `remote_new` = commits on `<remote>/<branch>` in neither HEAD nor `pre_head` | |
| `work_status_all` | `{}` | `Map<WorkItemId, GitStatus>` for every unfinished item: one `git fetch <remote>` per repo first, at most every 5 min (UI: startup, window focus, Now open) | |
| `work_diff` | `{id, delta?: bool, from?: String}` | `SessionInfo`: the review diff session in the item's worktree (editor with `editor.review_args`, `{range}` = `<remote>/<base>`; empty → a shell running `git diff $(git merge-base <base> HEAD)`). `delta`: `{range}` = `<reviewed>..<last>` (shell: `git diff <reviewed> <last>`), `Invalid` when nothing changed since the last review. `from` (review items, the reviewed PR head): `{range}` = `<from>..HEAD`, or `git range-diff <from>...HEAD` in a shell after a force push. The UI places it split down, zoomed, in the work tab | |
| `work_mark_reviewed` | `{id}` | `WorkItem` (clears `review_due` and `delta`; `refs/kelta/wi/<id>/reviewed` = `last`) | |
| `work_set_note` | `{id, note: Option<String>}` | `WorkItem` (`next_note`; blank clears) | |
| `work_left` | `{id}` | `WorkItem` (`left_at` = now; the return strip's clock) | |
| `work_arm_merge` | `{id, method: MergeMethod /*squash|merge|rebase*/}` | `WorkItem` (`auto_finish`) | host auto-merge (GitHub `enablePullRequestAutoMerge`, GitLab `merge_when_pipeline_succeeds`); refusal = host message. On `pr.merged` the item runs Finish once; a dirty/unpushed worktree is kept and the item flagged (`Merged.detail`) |
| `work_disarm_merge` | `{id}` | `WorkItem` | |
| `work_send` | `{id, prompt, files: Vec<SendFile{name, content}>, threads?: Vec<String>}` | `WorkItem` | files (`name.md`, not `ticket.md`/`context.md`) go to the item's private Claude run dir, never the worktree; `prompt` is rendered (`{file}`, `{pr.url}`, `{onto}`…) then pasted (bracketed + Enter) into a live Claude whose hook status is `Done`/`WaitingUser`, or passed to `claude --resume <uuid> -- <prompt>` (dead / closed tab / dormant; the `--continue` fallback keeps it). `Conflict{reason: claude_busy}` while `Working`/`NeedsInput`/`Running`, `Conflict{reason: hooks_inactive}` when the live session's status is not from hooks. `threads` → `WorkItem.sent_threads` |
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
 | {type:'layout.changed', project_id: ProjectId, layout: Layout}            // only for backend-initiated changes
 | {type:'tickets.changed', scope: Scope} | {type:'reviews.changed', scope: Scope, new_keys: ReviewRef[]}
 | {type:'work.updated', work: WorkItem}
 | {type:'settings.changed', layers: Layer[], paths: string[], requires_restart: string[]}
 | {type:'account.status', account_id: AccountId, status: 'ok'|'needs_auth'|'rate_limited'|'offline'|'error', detail?: string}
 | {type:'toast', toast: Toast /*{level:'info'|'warn'|'error', text, action?: {label, command: ActionId|string, args?}}*/}
 | {type:'plugin.event', instance_id: ScreenInstanceId, name: string, payload: unknown}   // relayed bus events granted to a screen
 | {type:'ctl.command', cmd: CtlCommand}                                                    // toggle, palette, focus-project…
 | {type:'ui.open', request: OpenPaneRequest, project_id: ProjectId}                       // backend asks UI to open a pane
```

### 6.3 Internal bus (`BusEvent`) — catalogue in PLUGINS.md §6

`BusEvent { name: String, ts: String, project_id: Option<ProjectId>, session_id: Option<SessionId>, work_item_id: Option<WorkItemId>, payload: Value, chain: TriggerChain{depth: u8, origin_triggers: Vec<String>} }` on a `tokio::sync::broadcast` (capacity 1024; lagged subscribers get a `lagged` marker and resync). The trigger engine (`PluginHost::start`) is one subscriber. Core itself handles two events synchronously on publish: `app.focus_changed` (published by the desktop shell on window focus; scheduler intervals) and `work.updated` (published by kelta-work; relayed as `UiEvent::WorkUpdated`).

---

## 7. Session / PTY lifecycle

### 7.1 Login environment (once per app start, background)

1. `$SHELL` (fallback `/bin/zsh` macOS, `/bin/bash` Linux). Run `$SHELL -l -i -c '<probe>'` where probe prints `\0__KELTA_ENV_BEGIN__\0`, then `env -0`, then `\0__KELTA_ENV_END__\0` (external `env`, `printf` works in sh/zsh/bash/fish; nushell gets `^env -0`). stdin `/dev/null`, no controlling tty, timeout 3 s.
2. On failure: retry `$SHELL -l -c`; then macOS `/usr/libexec/path_helper -s` + inherited env; Linux inherited env.
3. Parse only between sentinels (rc files may print noise). Cache in memory (`LoginEnv`); `diagnostics_run` shows the source.

### 7.2 Spawn

- `program` resolved with `which::which_in(program, LoginEnv.PATH, cwd)` → absolute path; missing → `NotFound` with install hint (tools) or onboarding link (claude/nvim).
- Plain shells: `$SHELL` with `-l` (login). Programs: exec'd directly (no shell wrapper).
- Env = LoginEnv ⊕ settings `terminal.env` ⊕ project `env` ⊕ request env ⊕ Kelta vars: `TERM=xterm-256color`, `COLORTERM=truecolor`, `TERM_PROGRAM=kelta`, `TERM_PROGRAM_VERSION`, `KELTA_SESSION_ID`, `KELTA_PROJECT_ID`, `KELTA_SOCK=<runtime>/ctl.sock`, `KELTA_HOOK_TOKEN` (per-session 128-bit hex), `KELTA_TICKET` (if linked), `KELTA_MCP_TOKEN` (Claude only). `TERMINFO`/`LANG` passthrough; if `LANG` unset → `en_US.UTF-8`.
- New session + process group (`setsid`), controlling tty = PTY. Kill = signal to the process group.

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
exit: waitpid (WNOHANG loop + blocking wait), emit Exited, close fds
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
| App quit | keltad (`terminal.session_host = daemon`): restorable sessions keep running, no confirmation; the rest below applies to the others. Confirm if any Claude session is `Working`/`NeedsInput`. nvim sessions get RPC `:wall \| mksession! <data>/sessions/<sid>.vim`. All persisted as `Dormant` with `RestorePolicy`; SIGHUP → 2 s → SIGKILL. |
| Next start | Layout restored; sessions still running in keltad are re-adopted `Live` (no respawn); other `Dormant` sessions spawn **when their pane is first attached** (`app.restore_mode = lazy`; `eager`, `none` available): Claude `claude --resume <uuid>` (fallback `--continue` if resume is refused), nvim `-S <file> --listen <sock>`, shell in last OSC 7 cwd, tools relaunched. A dimmed "restored" separator shows the persisted text tail (≤ 200 lines, `sessions.text_tail`). |

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
| PostToolUse `Edit\|Write\|MultiEdit` | unchanged (`StatusChange.status = Unknown`); `file_edited = tool_input.file_path` → bus `claude.file_edited` | — |

**Hooks-inactive fallback:** no SessionStart within 10 s of spawn (one-shot) → `hooks_active=false`, status source `Heuristic`: output → Working; BEL / OSC 9 / OSC 777 → NeedsInput; 3 s quiet after output (one-shot armed only by output) → Done. Pane header shows "status hooks inactive · Fix" → Diagnostics.

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
  async fn get(&self, t: &TicketRef) -> Result<TicketDetail, KeltaError>;  // body_md + body_html (sanitized), last 20 comments
  async fn columns(&self, b: &TrackerBinding) -> Result<Vec<Column>, KeltaError>;
  async fn transitions(&self, t: &TicketRef) -> Result<Vec<Transition>, KeltaError>;
  async fn transition(&self, t: &TicketRef, transition_id: &str, fields: Option<Value>) -> Result<Ticket, KeltaError>;
  async fn comment(&self, t: &TicketRef, markdown: &str) -> Result<(), KeltaError>;
  async fn assign(&self, t: &TicketRef, who: Assignee) -> Result<Ticket, KeltaError>;
  fn browser_url(&self, t: &TicketRef) -> String;
  fn branch_key(&self, t: &TicketRef) -> String;       // "SHOP-123" | "4567" | "gh-12" | "gl-12"
}
pub struct TicketRef { account: AccountId, key: String, id: String }
pub struct Ticket { r#ref: TicketRef, title, url, status: Status, kind: Option<String>, assignee: Option<User>,
  labels: Vec<String>, priority: Option<String>, updated_at: String, project_hint: Option<String>,
  priority_rank: Option<u8> /*0 = highest*/, status_since: Option<String> /*RFC 3339; falls back to updated_at*/,
  sprint: Option<Sprint{id, name, active, ends_at: Option<String>}> /*Jira sprint, Linear cycle, GitHub Projects iteration, GitLab iteration|milestone, Redmine version*/,
  estimate: Option<String>, due: Option<String> }
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

| Provider (v0.1) | Auth | List | Move |
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
  async fn get(&self, r: &ReviewRef) -> Result<ReviewDetail, KeltaError>;  // ReviewDetail.state: PrState Open|Merged|Closed
  async fn approve(&self, r: &ReviewRef, head_sha: &str) -> Result<(), KeltaError>;
  async fn comment(&self, r: &ReviewRef, body: &str) -> Result<(), KeltaError>;
  async fn request_changes(&self, r: &ReviewRef, body: &str) -> Result<(), KeltaError>;
  // Line comment (new-side `line` of `path`) on MY pending review, created on first use. approve /
  // comment / request_changes publish the pending review with the decision (GitHub: review events
  // endpoint; GitLab: draft_notes + bulk_publish). Default: Unsupported.
  async fn add_pending_comment(&self, r: &ReviewRef, path: &str, line: u32, body: &str) -> Result<(), KeltaError>;
  async fn create(&self, d: &PrCreate) -> Result<Review, KeltaError>;
  async fn find_for_branch(&self, repo: &str, branch: &str) -> Result<Option<Review>, KeltaError>;
  fn fetch_refspec(&self, r: &ReviewRef, local_branch: &str) -> String; // pull/N/head:… | merge-requests/N/head:… | <source branch>:… (Bitbucket)
  fn repo_from_remote(&self, url: &str) -> Option<String>;
  // Fix with Claude (default: Unsupported)
  async fn feedback(&self, r: &ReviewRef) -> Result<Feedback, KeltaError>;          // unresolved threads, reviews with a body, failed checks + 40-line log tail
  async fn rerequest_review(&self, r: &ReviewRef) -> Result<Vec<String>, KeltaError>; // everyone who reviewed, minus me
  async fn resolve_threads(&self, r: &ReviewRef, ids: &[String]) -> Result<(), KeltaError>;
}
// Review.reviewed_head = commit of my last submitted review (ReviewRequested kind). With head_sha it gives
// pending / "Updated since your review" / "Reviewed". GitHub ReviewRequested lists add a second search
// `is:pr is:open reviewed-by:@me` (user-review-requested drops a PR once I review) and keep only the PRs
// whose head moved since my review. ReviewDetail.pending_comments = line comments waiting in my pending review.
pub struct ReviewRef { account: AccountId, repo: String, number: u64 }
pub struct Review { r#ref, title, url, author: User, draft: bool, head_sha, source_branch, target_branch,
  ci: CiState /*Success|Failure|Pending|Error|None*/, decision: Option<ReviewDecision /*Approved|ChangesRequested|ReviewRequired*/>,
  my_state: Option<MyReviewState /*Pending|Approved|ChangesRequested|Commented*/>, mergeable: Option<bool>,
  labels: Vec<String>, kind: ReviewKind /*ReviewRequested|Authored*/, updated_at: String, linked_tickets: Vec<String>,
  additions: Option<u32>, deletions: Option<u32>, decision_head: Option<String> /*head the latest decisive review was left on*/,
  requested_at: Option<String> /*when my review was requested*/, blocking: bool /*I am the last required reviewer*/ }
pub struct Feedback { threads: Vec<FeedbackThread{id, author, path?, line?, body_md, url}>,
  reviews: Vec<FeedbackReview{author, state?, body_md}>, failed_checks: Vec<FailedCheck{name, url?, log_tail?}>, reviewers: Vec<String> }
```

- GitHub: list = one GraphQL request with aliased searches `is:pr is:open review-requested:@me archived:false` (`user-review-requested:@me` if `reviews.include_team_requests=false`) and `is:pr is:open author:@me`; small nested `first:`. Gate: `GET /notifications?participating=true` + `If-None-Match` only for classic/gh tokens (fine-grained → no gate). Actions REST: `POST /repos/{o}/{r}/pulls/{n}/reviews` with `commit_id`. GHE via `base_url`.
- GitLab: `GET /merge_requests?scope=all&state=opened&reviewer_username=<me>` and `scope=created_by_me`, `updated_after`; `draft=` if `/version` ≥ 16 else `wip=`; approve `POST …/approve {sha}` (409 → `Conflict`); request changes = note (+ optional unapprove). Gate: `/todos?state=pending&action=review_requested` count+max id.
- Gitea/Forgejo: `GET /api/v1/repos/issues/search?type=pulls&state=open&review_requested=true` (or `created=true`) returns issue-shaped rows, each expanded with `GET …/pulls/{n}` (branches, head sha); pagination follows `Link: rel="next"`. Approve / request changes = `POST …/pulls/{n}/reviews {event, commit_id}`; comment = issue comment; draft = `draft` or a `WIP:` title. Bearer token auth, `base_url` required. No change gate (always `true`).
- Bitbucket Cloud: the cross-workspace endpoints were retired in April 2026, so lists walk `GET /user/workspaces`: my PRs from `/workspaces/{ws}/pullrequests/{me}`, review requests from `reviewers.uuid` queries over the workspace's 50 most recently updated member repositories; `next` URLs are followed. Auth is `Basic <email>:<API token>` (app passwords were retired in 2026) or Bearer for access tokens. Request changes = comment + `POST …/request-changes`; approve compares the PR's current head with the reviewed sha first (`Conflict` if moved); no PR refs exist, so "review locally" fetches the source branch (no forks). List rows carry no CI (detail rolls up `/statuses`). No change gate.
- `linked_tickets`: regex `reviews.ticket_key_regex` over branch + title.
- Ticket to PR join (`feeds::ticket_prs`, no extra request): core fills `TicketItem.prs` and `TicketDetail.prs` from data already polled. The work item's `pr_url` comes first (`PrSource::WorkItem`; its state follows the work item when merged or closed), then every polled review whose `linked_tickets` name the ticket (`PrSource::KeyMatch`, matched with the same key rules as the Reviews pane; a bare Redmine number only matches in the code-host repos of the ticket's projects). `PrLink.account` is set when the repo is bound to a code-host account: the UI then opens Kelta's review detail (`p`), else the browser. `P` is always the browser.
- Feedback (FLOW §4.2). GitHub: one GraphQL query (`reviewThreads(first:100)` filtered on `isResolved` client side, since the connection has no such argument; `latestReviews` with a body; `viewer`), check runs of the head (REST) and, for failed GitHub Actions runs (5 max), the job log tail. Re-request = `POST …/requested_reviewers` with the latest reviewers; resolve = `resolveReviewThread` per id. GitLab: unresolved resolvable discussions (system notes skipped), failed jobs of the head pipeline + `/jobs/:id/trace` tail; re-request = `/request_review @…` quick action note (GitLab 17); resolve = `PUT …/discussions/:id?resolved=true`. A 403 / offline read maps to the FLOW §6 wording ("GitHub refused the review threads (403: token lacks `pull_requests:read`).", GitLab: `read_api`).
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
- Linux first-run probe: 2 s rAF scroll test; if WebGL ≥ 55 fps and DOM < 45 fps, toast offers switching (result stored in SQLite, not settings).
- Cursor blink only on the focused pane and only if `terminal.cursor_blink=true` (default false). No CSS animations at idle; spinners only while an operation is in flight; respects `prefers-reduced-motion`.

### 9.3 Snapshot encoder (L1) — required coverage (golden-tested with recorded nvim, Claude, lazygit, htop streams)
Alt-screen state with main scrollback (`?1049`), last N history lines (`terminal.view_scrollback`, default 1000), visible grid with SGR runs incl. colon underline styles (`4:3` undercurl), underline colour (`58`), truecolor/256, OSC 8 hyperlinks, wide + combining chars, DECSTBM + origin mode, saved cursor (DECSC), G0/G1 charsets (DEC line drawing), cursor position/shape (DECSCUSR)/visibility (`?25`), modes DECCKM (`?1`), DECKPAM, bracketed paste `?2004`, mouse `?1000/1002/1003` + encodings `?1005/1006/1015`, focus `?1004`, sync `?2026` (always emitted off), title (OSC 2), cwd (OSC 7). Budget: ≤ 150 KB and ≤ 15 ms for 200×60 + 1000 lines.

### 9.4 Keyboard routing
`attachCustomKeyEventHandler`: Kelta chords (from `keys.*`, matched on `KeyboardEvent.code` + modifiers) and the prefix key return `false` (consumed); everything else reaches xterm → `onData` → `session_write`. Per-kind remaps (`terminal.shift_enter`): Claude default `esc-cr` (`\x1b\r`), others `passthrough`. Never send unsolicited CSI-u. macOS `Option as Meta` via `macOptionIsMeta` (+ left/right-only handling in the key handler). Default webview shortcuts (reload, zoom, find, context menu) are disabled.

### 9.5 Memory budget for scrollback (L1)
Scrollback per kind (`terminal.scrollback`: shell 3000, claude 3000, editor 500, tool 500, setup 1000). Global cap `terminal.memory_cap_mb` (default 160): computed from `history_lines × cols × 24 B` per session, updated on line growth (counter, event-driven). On exceed: shrink history of least-recently-viewed sessions to 500 lines (never below), oldest first; the host emits `TerminalEvent::MemoryCapReached` once and core raises a warning toast. Snapshots carry `terminal.view_scrollback` history lines (`TerminalLimits.view_scrollback`).

### 9.6 On-disk history log (L1)
Output older than the in-memory scrollback stays searchable and restorable without RAM growth (`terminal.history_log`, default on).
- **Capture:** the model's `Handler` wrapper copies screen rows as plain text (ANSI stripped, wrapped rows joined, trailing blanks trimmed) just before alacritty rotates them into the primary history: linefeed/auto-wrap at the bottom of a top-anchored scroll region, `CSI S`, delete-lines at row 0, `ED 2`. The alternate screen and regions below the top never reach history and are not logged. At reader exit the remaining screen rows are appended. Not captured: rows pushed into history by a resize reflow.
- **Write path:** the reader appends to a per-session buffer under the session lock and hands it to one `kelta-history` writer thread (`try_send` on a bounded queue) past 64 KiB, when its `poll` goes idle, and at exit (exit waits for the write so quit cannot lose it). No timer. A full queue keeps the text for the next flush up to 1 MiB, then drops it behind a `[kelta: … not saved]` marker line.
- **Files:** `<data>/history/<id>.log` rotates to `<id>.1.log` past half of `terminal.history_log_mb` (default 16); past `terminal.history_log_total_mb` (default 512) the oldest files (mtime) are deleted. Session ids that are not `[A-Za-z0-9_-]{1,128}` get no file. A crash can only cut the last line; the first append after a start terminates it, readers treat it as a line.
- **Read path:** `history_tail` (Dormant `session_text_tail`; a respawn under the same id first feeds the last `view_scrollback` lines into the new model's scrollback above a cleared screen, before logging starts), `history_search` (case-insensitive substring, newest `limit` per session; buffered lines are flushed first, the visible screen is not searched). Both run on the writer thread, after pending writes. `history_delete` runs when core deletes the session row (close, quit without restore, startup prune).

---

## 10. Storage (SQLite `<data>/kelta.db`, WAL, migrations in kelta-core)

```
schema_version(v)
projects_open(project_id PK, ord, active)              -- open set + order; config lives in TOML
layouts(project_id PK, json, rev, updated_at)
sessions(id PK, project_id, kind_json, spec_json, name, work_item_id, restore_json, cwd, lifecycle, text_tail, updated_at)
work_items(id PK, project_id, kind, ticket_json, review_json, repo_id, worktree, branch, base, claude_uuid, nvim_socket,
           tab_id, pr_url, state_json, session_ids_json /*'[]'*/, created_at, updated_at,
           review_due /*v2*/, claude_replied /*v2*/)
work_steps(work_item_id, step, status /*pending|running|done|failed|skipped*/, detail, updated_at, PK(work_item_id, step))
seen_reviews(account, repo, number, head_sha, first_seen, PK(account, repo, number))
provider_cache(key PK, etag, body_json, fetched_at)
plugin_grants(plugin_id, permission, granted_at, manifest_sha256, PK(plugin_id, permission))
plugin_kv(plugin_id, key, value, PK(plugin_id, key))      -- screens' kv.* (PLUGINS §7); value = JSON text
repo_trust(path PK, sha256, trusted_at)
trigger_log(id PK, ts, trigger_id, event, ok, detail, depth)   -- capped 1000 rows
ui_state(key PK, value)                                   -- webgl probe result, onboarding done, window geometry
```
All writes go through the single sqlite thread. Startup reads (open projects, layouts, provider_cache) happen before window creation.

---

## 11. Security model

### 11.1 Local surfaces
- ctl socket: 0600 in a 0700 dir owned by the user; peer uid must equal ours; hook frames must carry the session's `KELTA_HOOK_TOKEN` (constant-time compare). Other ctl commands are allowed for same-uid peers.
- HTTP server: binds `127.0.0.1` only; `/mcp/<sid>` and `/hook/<sid>` require `Authorization: Bearer <per-session token>`; the per-instance web-tool proxy listeners (kelta-plugins) serve `/proxy/<instance>/…` with an unguessable instance path segment (128-bit), only proxy to the tool's own loopback origin and require `Host: 127.0.0.1:<listener port>`; `Host` header must be `127.0.0.1:<port>` (DNS-rebinding guard).
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
- Tauri app manifest declares every command; capability `capabilities/main.json` grants them only to window label `main` and local origin; no `remote` URLs.
- Ticket/PR HTML is sanitized in Rust (ammonia: no scripts, no styles, no iframes, links `rel=noopener` and opened via `open_external`).

### 11.3 Plugins
- Plugin screens: `<iframe sandbox="allow-scripts allow-forms">` (no `allow-same-origin`, no popups, no top-navigation) loading `kelta-plugin://<id>/<path>`; the scheme handler serves only canonicalized files inside the plugin dir and adds `Content-Security-Policy: default-src 'self' kelta-plugin://<id>; script-src kelta-plugin://<id>; style-src kelta-plugin://<id> 'unsafe-inline'; img-src kelta-plugin://<id> data:; connect-src 'none'; frame-ancestors 'self'`.
- Bridge: host creates a `MessageChannel` per instance, transfers `port2` only after verifying `event.source === iframe.contentWindow`; every call → `plugin_call(instance_id, method, params)`; Rust checks the plugin's grants (SQLite) and the method's required permission (PLUGINS.md §4). `net:<host>` is enforced in Rust (`http_fetch`), exact host or `*.domain` glob, https only (http only for `127.0.0.1`/`localhost` when granted explicitly). Grants are re-prompted when a manifest update adds permissions.
- **Security gate S1:** a sandboxed plugin iframe and a localhost tool iframe cannot reach `window.__TAURI_INTERNALS__`, `window.parent` DOM, or invoke any command, on WKWebView and WebKitGTK: automated on WebKitGTK by `scripts/sandbox-check.sh`, manual on WKWebView (BUILD_PLAN §6.1). Iframes can post to `window.webkit.messageHandlers.ipc`; Tauri's per-launch invoke key (main frame only) is what rejects those messages.
- Declarative tools/triggers from repo-local `.kelta/config.toml` that execute anything (`command`, `run`, `setup`, `http`) are **inert until trusted**: trust = SHA-256 of the file content stored in `repo_trust`; any change → untrusted again (banner).
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
| Bundle | initial JS ≤ 200 KB gz; each lazy chunk ≤ 120 KB gz; dmg ≤ 20 MB; deb ≤ 15 MB | dynamic imports | `bench/bundle-size.mjs`, artifact sizes |

**Enforcement lints (scaffold):** `clippy.toml` `disallowed-methods` (workspace-wide, BUILD_PLAN §2.6): `tokio::time::interval`, `tokio::time::interval_at`, `std::thread::sleep`, `std::thread::spawn` (allowed only in `kelta-term::reader`, `kelta-core::scheduler`, `kelta-core::store` and tests via `#[allow]` + `// allowlisted:`), `std::process::exit`. `tokio::time::sleep` is not lint-banned: it is allowed only as an event-armed one-shot marked `// one-shot: <reason>` (BUILD_PLAN §1.5); `scripts/check-no-timers.sh` fails on `setInterval` and recursive `requestAnimationFrame` in `ui/src` outside `ui/src/lib/terminal/raf.ts`.

---

## 14. Platform specifics (summary; details in SPEC.md §8)

- Linux: Tauri → WebKitGTK `webkit2gtk-4.1` (GTK3). `pre_init` (before any GTK init, single-threaded): read `[linux.graphics]` with a minimal TOML parse; NVIDIA detect (`/proc/driver/nvidia/version` or `/sys/module/nvidia_drm`) → `WEBKIT_DISABLE_DMABUF_RENDERER=1` + `__NV_DISABLE_EXPLICIT_SYNC=1`; no `/dev/dri/renderD*` (VM, container) → `WEBKIT_DISABLE_COMPOSITING_MODE=1` (software GL compositing costs ~4× per frame, gate G2); toggles for `WEBKIT_DISABLE_COMPOSITING_MODE`, `GDK_BACKEND=x11`; `--safe-graphics` sets all; never set `GTK_IM_MODULE`. `mallopt(M_ARENA_MAX, 2)`. app_id/WM_CLASS `dev.kelta.Kelta`. `window.decorations=auto`: `none` if `HYPRLAND_INSTANCE_SIGNATURE` or `SWAYSOCK`, else `native`. No in-app global shortcuts: `kelta-ctl toggle` bound in compositor; raise uses `XDG_ACTIVATION_TOKEN` when provided.
- macOS: WKWebView, WebGL default, Cmd is the app modifier, custom app menu replaces Tauri's default (Cmd+W/H/M/Q intentional), dock badge = sessions needing input, window close → background mode by default, `/tmp/kelta-<uid>` runtime dir, Developer ID signing + notarization in release CI.

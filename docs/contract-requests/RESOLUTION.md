# Contract requests — resolution (v0.1 integration)

Decisions by the contract owner after all lanes (L1-L10) merged. **Accepted** items were applied in
the shared code (kelta-proto + generated TS, core wiring, desktop shell, UI) and the lane-local
workaround they required was removed, so each concern has one path. **Rejected** items keep the
documented lane behaviour. Docs (ARCHITECTURE, PLUGINS, SETTINGS, SPEC) were updated to match.

S0 (scaffold) items were already resolved at the S0d gate (see `S0.md`); nothing new there.

## L1 — kelta-term

| # | Request | Decision |
|---|---------|----------|
| 1 | Swallow `XTWINOPS_CHARS` (`CSI 18 t`) | **Accepted.** Added to `SWALLOWED_QUERIES` (→ `terminal_queries.ts`); contract row flipped to `yes`; the queries test now asserts the answered set equals the swallowed set. |
| 2 | Toast when the scrollback memory cap trims sessions | **Accepted.** `TerminalEvent::MemoryCapReached { cap_mb }`, emitted once per host on a trimmed session; core raises a warning toast. |
| 3 | `terminal.view_scrollback` reaches the encoder | **Accepted.** `TerminalLimits.view_scrollback` (0 = default 1000), filled by `from_settings`, used by snapshots. |
| 4 | `TerminalHost::close(id)` | **Rejected.** `kill` on an exited session already closes it (frees model and memory share); documented on `TerminalHost::kill`. No second path. |
| — | Throughput / concurrent-fill RSS notes | Recorded, no contract change. The Linux `memory_and_leaks` budget miss (MERGE.md) stays open for L1. |

## L2 — UI shell

No requests. The scaffold `NotImplemented` stub component (and its preloaded chunk) is deleted: no
lane uses it any more.

## L3 — core

| # | Request | Decision |
|---|---------|----------|
| 1 | `work_items.session_ids_json` column | **Accepted** (doc): added to ARCHITECTURE §10. |
| 2 | `sid8` collisions with uuid v7 | **Accepted.** `SessionId::sid8()` is now the **last** 8 hex chars (the random tail of the v7 uuid). Core's "retry with a v4 uuid" workaround is removed; the per-run sid8 reservation stays. |
| 3 | Additive core API (`CoreDeps`, `start_with`, …) | Accepted as is. |
| 4 | `reqwest` in kelta-core for no-redirect `http_fetch` | **Accepted** (the dependency, already in the graph). A separate `Policy::none()` client is a few lines; a shared `HttpClient::no_redirect()` would add API for one caller. Documented on `CoreApi::http_fetch`. |
| 5 | sid8 reservation never freed | Accepted (recorded `shortcut:`). |

## L4 — settings UI

No requests.

## L5 — HTTP, trackers, code hosts

| # | Request | Decision |
|---|---------|----------|
| 1 | Core sets `reviews.ticket_key_regex` | **Accepted.** Core calls `kelta_codehosts::set_ticket_key_regex` at boot and on every settings reload (invalid pattern → warning, previous kept). Documented in SETTINGS. |
| 2 | Scheduler pauses on `RateLimited { retry_after_ms }` | Already implemented by L3's scheduler; no change. |
| 3 | Recreate providers on account change | Already implemented (`ProviderRegistry::invalidate_changed`); no change. |
| 4 | Wire details (transition ids, keys, cursors, error details) | Recorded; no change. |
| 5 | Shared tracker view keys | **Accepted** (doc): SETTINGS §3 lists `status` / `assigned_to` as shared keys. |

## L6 — work orchestration

| # | Request | Decision |
|---|---------|----------|
| 1 | Wire `WorkHost` (HTTP port + blocking triggers) | **Accepted.** Core implements `kelta_work::WorkHost` over `Server` and `PluginHost` (weak reference, no cycle) and calls `set_host` at boot. MCP config and trigger vetoes/patches now work in the app. |
| 2 | Session id known before spawn | **Accepted.** `SpawnRequest.id: Option<SessionId>`: core spawns under that id (must be a uuid and unused, else `InvalidArgument` / `Conflict`). kelta-work spawns Claude under the id its files name; the post-spawn file rewrite and the `KELTA_SESSION_ID` env override are removed. FakeCore honours the id too. |
| 3 | `work.updated` reaches the UI | **Accepted.** `bus::WORK_UPDATED` is now a catalogued bus event (proto; PLUGINS §6). Core relays it synchronously on publish as `UiEvent::WorkUpdated`. kelta-work's local constant is removed. |
| 4 | `sid8` collisions | Same fix as L3 #2. Work-item sessions keep kelta-work's own run dirs (`<runtime>/s/<key8>/`, journaled); documented in ARCHITECTURE §2.1. |
| 5 | Restore Dormant work-item Claude sessions | **Accepted.** Core's `restore_session` asks `WorkService::claude_restore_request` for ClaudeResume sessions with a work item (regenerated files, `--resume` / `--continue` fallback). |
| 6 | `EditorMeta.socket` from `--listen` | Already done by core at spawn; no change. |
| 7 | Skip a failed step | **Accepted** as `work_retry_step { step: "skip:<step>" }` (no new command). Documented in ARCHITECTURE §6 and on the TS `Commands` map; the UI shows Skip next to Retry (start sheet and work item pane); the mock implements it. |
| 8 | `CoreApi::projects()` for worktree prune | **Rejected** for v0.1: pruning the repos of known work items covers worktrees Kelta created. |
| 9 | `Review.state` (merged) | **Rejected** for v0.1: the `merge-tree` check handles squash/rebase merges; a host-reported state needs changes in both code hosts. |

## L7 — server, hooks, MCP, kelta-ctl

| # | Request | Decision |
|---|---------|----------|
| 1 | Drop `rmcp` | **Accepted.** Removed from the workspace and kelta-server; ARCHITECTURE §1.1/§2 describe the hand-rolled transport. |
| 2-3, 5-9, 11-12 | Additive `http_port`, peer uid via tokio, ctl/hook behaviour | Recorded; no change. |
| 4 | Shared Claude settings generator | **Accepted.** `kelta_proto::hooks::claude_settings(hook_command, &ClaudeSettings, http)` is the single generator; kelta-work delegates to it and kelta-server's end-to-end test uses it (its hand-written copy is removed). |
| 10 | `CoreApi::ticket_transition` / `ticket_comment` | **Accepted.** Both take `session: Option<&SessionId>` (event context). Core reuses `tracker_transition` / `tracker_comment` (cache invalidation, events). The MCP tools call them; their local `publish` helper and extra `Tracker::get` are removed. FakeCore mirrors core. |
| — | Core's local copy of `hooks::map` | Removed: `kelta_server::hooks::map` is the only mapping (ctl `hook` dispatch). |

## L8 — plugins, tools, triggers

| # | Request | Decision |
|---|---------|----------|
| 1-2 | Start and wire the plugin host in core | **Accepted.** Core wires `PluginHost` (`Wiring { ui, settings, settings_writer }`) and syncs plugin settings schemas at boot, and calls `start()` once the runtime is up, so triggers run without a UI. The desktop's lazy `plugin_host()` wrapper is removed (commands use `core.plugins()`, schemas via `Core::sync_plugin_schemas`). This also fixes a double delivery: core's bus forwarder called `on_event` while the host's own subscription delivered the same event. The trigger engine's subscription is now the only path (it also delivers the `lagged` marker). |
| 3 | `tool_status` command | **Rejected** for v0.1: the `kelta.tool_handle` / `kelta.tool_exited` `plugin.event` relay is the contract; after a restart the tool is genuinely not running. |
| 4 | Stop the tool when its pane closes | **Rejected**: the UI authors layouts, so the web pane's destroy check (`on_close` lifecycle) stays the single owner. |
| 5 | `keep_alive` across tab switches | **Rejected**: contract narrowed (PLUGINS §7, SPEC §2): `keep_alive` holds while the pane stays mounted. |
| 6 | Per-instance proxy listener vs kelta-server `/proxy` | **Per-instance listener kept** (distinct origin per web tool). kelta-server no longer mounts `/proxy` and no longer depends on kelta-plugins. Docs updated. |
| 7 | Tool keybindings dispatch `tools.open` | Done by L2 (`lib/keys`); no change. |
| 8 | Mock `controls.override` | **Rejected**: the e2e harness wrapper works. |
| Dev 1 | Screen handshake on `kelta:ready` | **Accepted** (doc): PLUGINS §7. |
| Dev 7 | `tool_check` without project id | **Rejected** for v0.1: the UI only checks after a failed `tool_open`, so a wrong hint is cosmetic. |

## L9 — work views

| # | Request | Decision |
|---|---------|----------|
| 1 | Skip a failed step | See L6 #7 (Skip buttons shown). |
| 2 | Review list filters | **Rejected**: drafts and repo filter client-side; team requests follow `reviews.include_team_requests` (SPEC §3.3 updated). |
| 3-4 | Error detail shapes, board per project | Recorded; no change. |
| 5 | Production build has no lazy views | Resolved by L2 (the shell imports the registry). |
| 6 | Toast actions dispatched | Done by L2 (`ToastHost` dispatches `action.command`). |

## L10 — packaging, window, bench

| # | Request | Decision |
|---|---------|----------|
| 1 | `enableGTKAppId` | **Accepted.** In `tauri.conf.json`; removed from the release overlay. |
| 2 | `app_ready` clears the guard and marks the bench | **Accepted.** The `app_ready` command calls `platform::launch_succeeded()` and `bench::app_ready()`. The `on_page_load(Finished)` stand-in and core's duplicate `launch-guard` removal are gone. |
| 3 | `.gitignore` for `binaries/` | Done in L10 review round 1. |
| 4 | Custom decorations | **Accepted.** `AppInfo.decorations` (what the window was built with); the shell renders `WindowChrome` (drag strip + 8 resize handles via `startResizeDragging`) when it is `custom`; capability `main` grants `core:window:allow-start-dragging` / `allow-start-resize-dragging`. |
| 5 | Bench marks from L1/L2/L3 | Deferred (bench instrumentation, not a contract): only `app_ready_ms` is written by the app today. |
| 6 | `custom.bench.close_window` reaches the bridge | **Accepted.** Core forwards `custom.bench.*` emits to the UI bridge; the bridge acts only with `KELTA_BENCH_MARKS` and never wakes the window for other emits. |
| 7 | XDG activation token on `toggle` | **Rejected** for v0.1 (needs GTK startup-id plumbing; `set_focus` works on X11 and most compositors). The `shortcut:` comment stays. |
| 8 | Window focus on the bus | **Accepted.** The desktop publishes `app.focus_changed {focused}` through `CoreApi::publish`; core handles it on publish (scheduler intervals) — previously nothing published it. |
| 9-11 | Baseline, CI validation, unverified builds | Recorded; no change. |

## Leftover stubs

No `not implemented:` path remains in `crates/`, `apps/` or `ui/src`. The unused scaffold helpers
(`commands::not_implemented`, `NotImplemented.svelte`) were deleted. `KeltaError::not_implemented`
stays in kelta-proto only as the error fixture.

## Still open (not contract changes)

- MERGE.md: two tests red in the Linux Docker image (L1 `memory_and_leaks` RSS under glibc, L6
  `edit_checktime_selection_mksession` with the image's nvim). `scripts/qa.sh` (macOS) is green.

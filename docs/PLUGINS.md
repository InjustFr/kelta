# Kelta v0.1 — Extensibility: tools, triggers, plugins (contract)

Three tiers, none of which keeps a background runtime alive (zero memory when unused):

1. **Tools** — declarative PTY commands (lazydocker, lazygit, k9s) or web tools (sl web / ISL, dashboards).
2. **Triggers** — declarative `event → match → actions` rules (Kelta's hooks), including reactions to Claude Code hooks.
3. **Plugins** — a directory with `kelta-plugin.toml` contributing tools, triggers, commands, keybindings, session templates, settings and **custom screens** (sandboxed iframes talking to a permission-checked host API).

Tools, triggers, commands and session templates use the **same schema** in `config.toml`, project files, repo-local `.kelta/config.toml` (trust-gated) and plugin manifests. Types live in `kelta-proto::ext` (`ToolDef`, `TriggerDef`, `ActionDef`, `CommandDef`, `PluginManifest`, `ScreenDef`, `PluginMethod`, `Permission`), JSON Schemas in `schema/tool.schema.json`, `schema/trigger.schema.json`, `schema/plugin-manifest.schema.json` (generated, CI drift-checked).

**Provider plugins (KPP, §9):** a plugin can also ship a process speaking JSON-RPC 2.0 over stdio that serves **tracker or code-host accounts** through the same `Tracker`/`CodeHost` traits as the built-in providers, spawned on first use only. v0.2 (designed, not built): WASM logic plugins.

---

## 1. Placeholders

As in SETTINGS.md §6: `{project.id|name|root}`, `{repo.id|path|name}`, `{worktree}`, `{branch}`, `{base}`, `{ticket.key|title|url|file}`, `{pr.url|number|head|base|title}`, `{session.id|name|cwd}`, `{sid8}`, `{run}`, `{port}`, `{config_dir}`, `{data_dir}`, `{plugin.dir}`, `{settings.<key>}` (plugin's own namespace), `{event.<path>}`, `{payload.<path>}` (triggers). Filters `|slug`, `|shell`, `|json`; fallback `{a|b}`. Unknown placeholders are load-time errors. Values are substituted **per argv element** (no shell parsing).

## 2. Tools

```rust
pub struct ToolDef {
  pub id: String,                      // [a-z0-9-]; plugin tools are namespaced "<plugin>/<id>"
  pub label: String, pub icon: Option<String>, pub description: Option<String>,
  pub kind: ToolKind,                  // Pty | Web | External (detached launch, no pane)
  // pty
  pub command: Option<String>, pub args: Vec<String>, pub cwd: Option<String>, pub env: BTreeMap<String, String>,
  pub close_on_exit: CloseOnExit,      // never | on_success | always (default never → exit banner + Relaunch)
  pub scrollback: Option<u32>,
  // web
  pub url: Option<String>,             // static URL (no server)
  pub start: Option<WebStart>,         // server launcher
  pub embed: EmbedMode,                // auto | iframe | proxy | external (default: web.embed_default)
  pub url_is_secret: bool,             // never logged / shown in chrome (default true when start is set)
  pub lifecycle: WebLifecycle,         // on_close (default) | on_project_close | never
  pub keep_alive: bool,                // keep iframe while its pane is mounted but hidden (default web.keep_alive)
  // common
  pub check: Option<Vec<String>>,      // argv run by tool_check; exit 0 = installed
  pub install_hint: Option<String>,
  pub placement: Placement,            // new_tab (default) | split_right | split_down
  pub keybinding: Option<Chord>,
  pub autostart: bool,                 // open when its project opens (default false)
  pub enabled: bool,                   // default true
}
pub struct WebStart { pub command: String, pub args: Vec<String>, pub cwd: Option<String>, pub env: BTreeMap<String,String>,
  pub ready: Ready,                    // { stdout_json = "url" } | { stdout_regex = "http://\\S+" } | { port_open = true }
  pub ready_timeout_ms: u64,           // default 10000
  pub stop: StopSpec }                 // { signal = "TERM", grace_ms = 3000 } | { command = [...] }
```

Behaviour (kelta-plugins `tools`):
- **PTY tool** → `CoreApi::session_spawn(kind = Tool{tool_id})`, cwd/args/env expanded; a normal session (attention, restore = Relaunch).
- **Web tool** → server spawned with `tokio::process` (not a PTY; stdout/stderr captured into a 64 KiB ring for the log view); readiness read from the stdout stream (no polling; `port_open` tries `connect()` after each stdout line and on backoff 100/200/400 ms … ≤ `ready_timeout_ms`); `{port}` is a free loopback port reserved by Kelta. Then `embed`:
  - `auto`: HEAD probe; no `X-Frame-Options` and no `frame-ancestors` → `iframe`, else `proxy`.
  - `proxy`: `http://127.0.0.1:<listener-port>/proxy/<128-bit instance>/…` reverse proxy to the tool's loopback origin, served by kelta-plugins on its own loopback listener per instance (a distinct origin per web tool; started on first open, stopped with the instance); strips `X-Frame-Options` and CSP `frame-ancestors`; passes WebSocket upgrades; rewrites `Location` headers to the proxy prefix.
  - `external`: opens the system browser (url via `open_external`).
- Server process killed per `lifecycle` (process group, `stop` spec). Exit → pane shows exit code + log tail + **Relaunch**.

### 2.1 Example: lazydocker (PTY)
```toml
[[tools]]
id = "lazydocker"
label = "Docker"
icon = "container"
kind = "pty"
command = "lazydocker"
cwd = "{repo.path|project.root}"
check = ["lazydocker", "--version"]
install_hint = "brew install lazydocker  |  go install github.com/jesseduffield/lazydocker@latest"
close_on_exit = "never"
scrollback = 500
placement = "split_down"
```

### 2.2 Example: lazygit opening files in the tab's nvim
```toml
[[tools]]
id = "lazygit"
label = "Git"
kind = "pty"
command = "lazygit"
cwd = "{worktree|repo.path}"
env = { LG_CONFIG_FILE = "{data_dir}/lazygit-kelta.yml" }   # Kelta generates it: os.editAtLine → "kelta-ctl editor-open {{filename}}:{{line}}"
check = ["lazygit", "--version"]
```

### 2.3 Example: Sapling ISL (`sl web`, web tool)
```toml
[[tools]]
id = "isl"
label = "Sapling ISL"
icon = "branch"
kind = "web"
start = { command = "sl", args = ["web", "--no-open", "--foreground", "--json", "--port", "{port}", "--cwd", "{repo.path}"],
          ready = { stdout_json = "url" }, ready_timeout_ms = 10000, stop = { signal = "TERM", grace_ms = 3000 } }
embed = "auto"
url_is_secret = true          # the URL carries ISL's token
lifecycle = "on_close"
check = ["sl", "--version"]
install_hint = "https://sapling-scm.com/docs/introduction/installation"
```

### 2.4 Example: static dashboard
```toml
[[tools]]
id = "grafana"
kind = "web"
url = "http://localhost:3000/d/abc"
embed = "auto"
```

---

## 3. Triggers (hooks)

```rust
pub struct TriggerDef {
  pub id: String, pub description: Option<String>,
  pub on: String,                       // event name or glob ("pr.*", "custom.*")
  pub r#match: BTreeMap<String, Matcher>, // dotted path into {event, payload, project, session, ticket, pr, app}
  pub r#do: Vec<ActionDef>,             // run in order; trigger stops on first failing action unless continue_on_error
  pub blocking: bool,                   // only honoured on *.before_* events (30 s cap)
  pub debounce_ms: Option<u64>,         // one-shot, armed by the event
  pub continue_on_error: bool,
  pub allow_send_keys: bool,            // required for send_keys actions
  pub enabled: bool,
}
pub enum Matcher { Str(String) /* exact; "glob:…" ; "re:…" ; "!…" negation */, Bool(bool), Num(f64), AnyOf(Vec<Matcher>) }
```

- No expression language: a payload path matches if its value equals / globs / regexes the matcher; a list = any-of; `!` prefix negates. Missing path → no match.
- Triggers run in parallel; actions within a trigger sequentially (async). Each run is logged (`trigger_log`, last 1000) with depth and outcome.
- **Guards:** chain depth ≤ 4 (events emitted by actions carry `chain`); a trigger never fires twice within one chain; rate limit 10 runs / 60 s per trigger (excess logged as `rate_limited`); `send_keys` needs `allow_send_keys = true` and is limited to 1 per 2 s per session.
- **Blocking pre-events** (`ticket.before_start`, `work.before_finish`, `pr.before_create`): a `run` action with non-zero exit or stdout JSON `{"veto": "reason"}` aborts the operation with that reason; `{"patch": {...}}` may patch the plan (v0.1: only `branch`, `template_id`, `claude.prompt`).
- Origins: global, project, repo-local (trusted only), plugin (`<plugin>/<id>`, needs the plugin's permissions for its actions).

### 3.1 Action types (`ActionDef`, tag `action`)

| Action | Fields | Effect | Plugin permission |
|---|---|---|---|
| `notify` | `title`, `body?`, `urgency? (low\|normal\|critical)` | desktop notification (respects quiet hours) | `notify` |
| `toast` | `text`, `level?` | in-app toast | — |
| `run` | `command`, `args?`, `cwd?`, `env?`, `stdin? = "event"\|"none"`, `timeout_ms? = 60000`, `show? = none\|toast_on_error\|pane` | background exec (no shell; `sh -c` must be explicit); output to trigger log; `pane` runs it in a visible session instead | `exec:<command>` |
| `spawn_session` | `template? \| command+args`, `cwd?`, `placement?`, `focus? = false` | new session in the event's project | `sessions.spawn` |
| `send_keys` | `session = "{event.session_id}"\|"claude"\|"editor"`, `text`, `bracketed? = true` | writes into a PTY | `terminal.write` |
| `open_tool` | `tool`, `placement?` | `tool_open` | `sessions.spawn` |
| `open_screen` | `plugin?`, `screen`, `params?`, `placement?` | opens a plugin screen | `ui.open` |
| `start_work` | `ticket = "{ticket.key}"`, `project?` | opens StartWorkPlan (preview unless `work.plan_preview=false`) | `tickets.read`,`sessions.spawn` |
| `transition_ticket` | `to_category?` \| `to_name?` | via provider transitions | `tickets.write` |
| `comment_ticket` | `body` | Markdown comment | `tickets.write` |
| `assign_ticket` | `to = "me"\|"none"` | | `tickets.write` |
| `http` | `url`, `method? = POST`, `headers?`, `body? (Template; default event JSON)`, `secret_headers? = {Header = SecretRef}`, `timeout_ms? = 10000` | outbound request | `net:<host>` |
| `focus` | `project?`, `session?` | focuses window/project/session | `ui.open` |
| `set_attention` | `session`, `level` | | `sessions.write` |
| `prompt` | `text`, `yes = [ActionDef]`, `no? = [ActionDef]` | in-app confirm toast with buttons | — |
| `command` | `id` | run a contributed command | (that command's) |

### 3.2 Examples

```toml
# Notify when Claude needs me in a background project
[[triggers]]
id = "claude-needs-me"
on = "session.status_changed"
match = { "payload.status" = ["needs_input", "waiting_user"], "session.visible" = false }
do = [ { action = "notify", title = "{project.name}: {session.name}", body = "{payload.preview}", urgency = "critical" } ]

# Start the DB when work starts on a shop ticket, then show lazydocker
[[triggers]]
id = "start-db"
on = "ticket.started"
match = { "project.id" = "shop" }
do = [
  { action = "run", command = "docker", args = ["compose", "up", "-d", "db"], cwd = "{worktree}", show = "toast_on_error" },
  { action = "open_tool", tool = "lazydocker", placement = "split_down" },
]

# Veto starting work when the main checkout is dirty
[[triggers]]
id = "guard-dirty-main"
on = "ticket.before_start"
blocking = true
do = [ { action = "run", command = "sh", args = ["-c", "git -C {repo.path|shell} diff --quiet || echo '{\"veto\":\"main checkout is dirty\"}'"] } ]

# React to a raw Claude Code hook: run clippy after Claude edits Rust files
[[triggers]]
id = "clippy-after-edit"
on = "claude.file_edited"
match = { "payload.path" = "glob:*.rs" }
debounce_ms = 1500
do = [ { action = "run", command = "cargo", args = ["clippy", "-q"], cwd = "{session.cwd}", show = "toast_on_error" } ]

# Merge → done + offer cleanup
[[triggers]]
id = "done-on-merge"
on = "pr.merged"
match = { "payload.linked_tickets" = "re:.+" }
do = [
  { action = "transition_ticket", to_category = "done" },
  { action = "prompt", text = "Remove worktree for {ticket.key}?", yes = [ { action = "command", id = "kelta.work.finish" } ] },
]
```

External events: `kelta-ctl emit custom.deploy_finished --json '{"env":"staging"}'` publishes `custom.deploy_finished` (only `custom.*` names accepted from the CLI).

---

## 4. Plugin manifest (`kelta-plugin.toml`; `.json` accepted)

Location: `<data>/plugins/<id>/` (Linux `~/.local/share/kelta/plugins`, macOS `~/Library/Application Support/dev.kelta.Kelta/plugins`), plus `plugins.dev_paths` (global settings only). Install: Settings → Plugins → Install (dir, git URL + optional tag, `.tar.gz`), or `kelta-ctl plugin install <src>`; both go through `plugin_inspect` → permission dialog (plain-language permissions, manifest SHA-256, warnings such as "this plugin can run `kubectl`") → `plugin_install` (copies into the data dir; git → shallow clone of the tag). No registry/marketplace in v0.1.

| Field | Type | Required | Notes |
|---|---|---|---|
| `id` | str `[a-z0-9-]{3,40}`, not starting with `kelta` | yes | |
| `name`, `version` (semver), `description`, `author`, `license` | str | yes | |
| `homepage` | str | no | |
| `kelta_api` | semver req | yes | host API `0.1.0`; incompatible → not loaded, shown with reason |
| `platforms` | list(`macos`\|`linux`) | no (all) | |
| `permissions` | list<Permission> | no | §5 |
| `activation` | list(`onStartup`\|`onCommand:<id>`\|`onScreen:<id>`\|`onEvent:<glob>`\|`onProjectOpen`) | no | v0.1: controls when triggers subscribe and screens are offered; nothing runs before activation |
| `[[contributes.tools]]` | ToolDef | | namespaced `<plugin>/<id>` |
| `[[contributes.triggers]]` | TriggerDef | | namespaced |
| `[[contributes.commands]]` | `{id, title, do: [ActionDef], when?: "terminal"\|"ticket"\|"review"\|"always"}` | | palette entries |
| `[[contributes.keybindings]]` | `{command, key: Chord}` | | user bindings win; conflicts reported |
| `[[contributes.session_templates]]` | session template | | |
| `[[contributes.screens]]` | ScreenDef | | §4.1 |
| `[[contributes.ticket_actions]]`, `[[contributes.review_actions]]` | `{id, title, do: [ActionDef]}` | | buttons on detail panes |
| `[contributes.settings]` | `{schema = "settings.schema.json"}` | | flat JSON Schema mounted at `plugins.<id>`, validated, rendered in Settings |
| `[provider]` | ProviderDef | | process (KPP) tracker / code-host provider, §9; needs the `provider` permission |

### 4.1 ScreenDef
`{ id, title, icon?, entry: "dist/index.html", scope: "project"|"global", placement: ["tab","pane"], keep_alive = false, min_width? }`. Served at `kelta-plugin://<plugin-id>/<entry>`.

### 4.2 Full example: custom screen plugin

```
sprint-burndown/
├─ kelta-plugin.toml
├─ settings.schema.json
└─ dist/index.html, dist/app.js, dist/style.css
```
```toml
id = "sprint-burndown"
name = "Sprint Burndown"
version = "0.2.0"
kelta_api = "^0.1"
description = "Burndown chart of the active sprint for the current project"
author = "Jane Doe"
license = "MIT"
platforms = ["macos", "linux"]
permissions = ["tickets.read", "settings.read", "events:ticket.*", "notify"]
activation = ["onScreen:burndown", "onCommand:sprint-burndown.open"]

[[contributes.screens]]
id = "burndown"
title = "Burndown"
icon = "chart"
entry = "dist/index.html"
scope = "project"
placement = ["tab", "pane"]

[[contributes.commands]]
id = "sprint-burndown.open"
title = "Burndown: open"
do = [ { action = "open_screen", screen = "burndown", placement = "new_tab" } ]

[[contributes.keybindings]]
command = "sprint-burndown.open"
key = "mod+b"               # macOS Cmd+B; Linux Ctrl+Shift+B

[[contributes.triggers]]
id = "sprint-done-toast"
on = "ticket.transitioned"
match = { "payload.to.category" = "done" }
do = [ { action = "toast", text = "{ticket.key} done" } ]

[contributes.settings]
schema = "settings.schema.json"   # e.g. { "type":"object", "properties": { "view_id": { "type":"string", "default":"sprint" } } }
```
`dist/app.js`:
```js
import { connect } from './kelta-sdk.js';            // vendored copy of @kelta/plugin-sdk (≈3 KB ESM)
const kelta = await connect();                        // waits for kelta:init, receives the MessagePort
const { view_id } = await kelta.settings.get();
const page = await kelta.tickets.list({ scope: 'project', view_id });
render(page.items, kelta.theme);                      // theme tokens as CSS variables
kelta.events.on('ticket.transitioned', () => refresh());
```

### 4.3 Example: tools-pack plugin (declarative only, no screen)
```toml
id = "tools-pack"
name = "Tools pack"
version = "0.1.0"
kelta_api = "^0.1"
description = "k9s and btop as Kelta tools"
author = "Kelta contributors"
license = "MIT"
permissions = ["sessions.spawn", "exec:k9s", "exec:btop"]

[[contributes.tools]]
id = "k9s"
label = "Kubernetes"
kind = "pty"
command = "k9s"
args = ["--readonly"]
check = ["k9s", "version"]

[[contributes.tools]]
id = "btop"
label = "btop"
kind = "pty"
command = "btop"
```

---

## 5. Permissions

Checked in Rust for every `plugin_call` (screens) and every action of a plugin-contributed trigger/command/tool. Grants live in SQLite `plugin_grants` keyed by manifest hash; a manifest update adding permissions disables the new capabilities until re-granted.

| Permission | Grants |
|---|---|
| `projects.read` | project list/info of the screen's project (or all for `scope = global`) |
| `tickets.read` / `tickets.write` | list/get/columns/transitions / transition, comment, assign |
| `prs.read` / `prs.write` | review list/get / approve, comment, request changes |
| `sessions.read` / `sessions.spawn` / `sessions.write` | list sessions / spawn sessions & tools / set attention |
| `terminal.write` | send text into sessions (`send_keys`) |
| `events:<glob>` | receive bus events matching the glob (e.g. `events:ticket.*`) |
| `settings.read` | read own namespace always allowed; this adds non-secret effective settings |
| `ui.open` | open panes/screens, focus |
| `notify` | desktop notifications |
| `clipboard.write` | write clipboard |
| `storage` | own key-value store (`kv.*`, §7): JSON values, 64 KiB per value, 1 MiB per plugin |
| `exec:<command>` | run that argv[0] (basename match) in `run` actions |
| `net:<host>` | `http_fetch` / `http` action to that host (exact or `*.domain`), https only; `net:127.0.0.1` / `net:localhost` explicit |
| `provider` | run the `[provider]` program for the accounts that name the plugin, handing it their settings and secret (§9); its own network access is its business |

Secrets are never readable by plugins in v0.1 (no `secret.get`). Plugin screens have no direct network (`connect-src 'none'`), no browser storage (use `kv.*`), no Tauri IPC.

## 6. Events catalogue (BusEvent names; payload JSON)

Every event carries `{name, ts, project_id?, session_id?, work_item_id?, payload, chain}`. Matcher roots: `event.*` (the envelope), `payload.*`, plus resolved context `project.{id,name}`, `session.{id,name,kind,cwd,visible,status}`, `ticket.{key,title,url,provider}`, `pr.{url,number,repo,head,base}`, `app.{focused}`.

| Event | Payload |
|---|---|
| `app.started` | `{version}` |
| `app.focus_changed` | `{focused: bool}` |
| `project.opened` / `project.activated` / `project.closed` | `{project_id}` |
| `session.spawned` | `{session: SessionInfo}` |
| `session.status_changed` | `{status, previous, source: "hook"\|"heuristic", preview?}` |
| `session.exited` | `{code?, signal?}` |
| `session.bell` | `{}` |
| `session.title_changed` | `{title}` |
| `claude.hook` | `{event: "<hook_event_name>", matcher_value?, payload: <raw Claude hook JSON minus transcript contents>}` |
| `claude.file_edited` | `{path, tool: "Edit"\|"Write"\|"MultiEdit"}` |
| `ticket.before_start` (blocking) | `{plan: StartWorkPlan}` |
| `ticket.started` | `{ticket: TicketRef, worktree, branch, work_item_id}` |
| `ticket.transitioned` | `{ticket, from: Status, to: Status}` |
| `ticket.commented` / `ticket.assigned` | `{ticket, ...}` |
| `worktree.created` / `worktree.removed` | `{path, branch, repo_id}` |
| `work.before_finish` (blocking) / `work.finished` | `{work_item_id, opts}` |
| `work.updated` | `{work: WorkItem}` (any item or saga step change; core relays it to the UI) |
| `pr.before_create` (blocking) / `pr.created` | `{draft}` / `{review: Review}` |
| `pr.review_requested` | `{review: Review}` (new key only) |
| `pr.updated` | `{review, changes: ["ci","decision","head"]}` |
| `pr.ci_changed` | `{review, state, previous}` |
| `pr.approved` / `pr.changes_requested` / `pr.merged` | `{review, linked_tickets}` |
| `tool.opened` / `tool.exited` | `{tool_id, instance_id, code?}` |
| `settings.changed` | `{paths, layers}` |
| `custom.*` | arbitrary (from `kelta-ctl emit`) |

## 7. Host API for custom screens

Transport: the SDK's `connect()` posts `{type:"kelta:ready"}` to the parent; the host answers with `postMessage({type:"kelta:init", api:"0.1", instance, plugin, project, params, theme}, "*", [port2])`, only if `event.source === iframe.contentWindow` (answering `kelta:ready` instead of the iframe `load` event avoids racing the screen's listener). Each iframe creation opens a fresh screen instance (`plugin_screen_open`), closed when the iframe is destroyed. Over the port: request `{id, method, params}` → response `{id, result}` | `{id, error: {code, message}}`; host-pushed `{type:"event", name, payload}`, `{type:"theme", tokens}`, `{type:"params", params}`, `{type:"visibility", visible}`.

`PluginMethod` (enum in proto; method → required permission):

| Method | Params → Result | Permission |
|---|---|---|
| `app.info` | `{}` → `{version, platform, theme}` | — |
| `projects.current` / `projects.list` | `{}` → `ProjectInfo` / `[ProjectInfo]` | `projects.read` |
| `tickets.list` | `{scope: "project"\|"all", view_id?, cursor?}` → `TicketPage` | `tickets.read` |
| `tickets.get` / `tickets.transitions` / `tickets.columns` | `{ticket}` / `{ticket}` / `{}` | `tickets.read` |
| `tickets.transition` / `tickets.comment` / `tickets.assign` | `{ticket, transition_id}` / `{ticket, markdown}` / `{ticket, assignee}` | `tickets.write` |
| `reviews.list` / `reviews.get` | `{scope, kind}` / `{review}` | `prs.read` |
| `reviews.approve` / `reviews.comment` / `reviews.request_changes` | | `prs.write` |
| `sessions.list` | `{project?}` → `[SessionInfo]` | `sessions.read` |
| `sessions.spawn` | `{template? \| command, args?, cwd?, placement?}` → `SessionInfo` | `sessions.spawn` (+ `exec:<cmd>` for command) |
| `sessions.send_text` | `{session_id, text, bracketed?}` | `terminal.write` |
| `tools.open` | `{tool_id, placement?}` | `sessions.spawn` |
| `events.subscribe` / `events.unsubscribe` | `{names: [glob]}` | `events:<glob>` covering each |
| `settings.get` | `{}` → own namespace values (+ effective non-secret settings with `settings.read`) | — / `settings.read` |
| `settings.set` | `{key, value}` (own namespace, Global layer) | — |
| `net.fetch` | `{url, method?, headers?, body?}` → `{status, headers, body (text or base64)}`; 5 MB cap | `net:<host>` |
| `ui.toast` / `ui.open_screen` / `ui.focus` | | — / `ui.open` / `ui.open` |
| `notify.send` | `{title, body}` | `notify` |
| `clipboard.write` | `{text}` | `clipboard.write` |
| `kv.get` / `kv.set` / `kv.delete` / `kv.list` | `{key}` → JSON or `null` / `{key, value}` / `{key}` / `{}` → `[key]` (sorted) | `storage` |

`kv.*` is the plugin's own store in SQLite `plugin_kv`, namespaced by the calling screen's plugin id (a plugin can never name another's), shared by all its screens and projects, kept across updates and deleted on uninstall. Keys are 1–256 bytes; a value is any JSON up to 64 KiB serialized; keys + values of one plugin are capped at 1 MiB. Over a cap → `InvalidArgument` and nothing is written.

Error codes = `ErrorCode` (ARCHITECTURE §4); denied → `PermissionDenied` with the missing permission in `detail`.

SDK (`packages/plugin-sdk`, published as `@kelta/plugin-sdk`, MIT, ≈3 KB ESM, no deps): `connect(): Promise<Kelta>`; `kelta.call(method, params)`; typed helpers `kelta.tickets.*`, `kelta.reviews.*`, `kelta.sessions.*`, `kelta.tools.open`, `kelta.events.on(name, cb)`, `kelta.settings.get/set`, `kelta.kv.get/set/delete/list`, `kelta.fetch(url, init)`, `kelta.ui.*`, `kelta.notify`, `kelta.theme` (CSS variable map, also applied to `:root`), `kelta.onVisibility(cb)`.

Lifecycle: iframe created when the screen pane becomes visible, destroyed when hidden (unless `keep_alive`, which is listed with its memory cost in Settings → Performance). `keep_alive` holds while the pane stays mounted (zoomed away, inbox overlay); switching tab or project unmounts the pane and destroys the iframe in v0.1. A screen that blocks the UI thread is detected by the core's ack watchdog (ARCHITECTURE §12.4) → webview reloaded in safe mode, screen closed, toast names the plugin.

## 8. Claude Code hooks generated by Kelta

Per Claude session, Kelta writes `<runtime>/s/<sid8>/claude-settings.json` (0600; work-item sessions use kelta-work's own run dir, ARCHITECTURE §2.1) and passes it with `--settings` (it **adds** to the user's hooks). The document is built by `kelta_proto::hooks::claude_settings(hook_command, &ClaudeSettings, http)`, shared by kelta-work and kelta-server's end-to-end test:

```json
{ "hooks": {
  "SessionStart":      [ { "hooks": [ { "type": "command", "command": "'<data>/bin/current/kelta-ctl' hook", "async": true, "timeout": 5 } ] } ],
  "UserPromptSubmit":  [ { "hooks": [ { "type": "command", "command": "'<data>/bin/current/kelta-ctl' hook", "async": true, "timeout": 5 } ] } ],
  "PermissionRequest": [ { "hooks": [ { "type": "command", "command": "'<data>/bin/current/kelta-ctl' hook", "async": true, "timeout": 5 } ] } ],
  "Notification":      [ { "matcher": "permission_prompt|idle_prompt|elicitation_dialog|agent_needs_input",
                           "hooks": [ { "type": "command", "command": "'<data>/bin/current/kelta-ctl' hook", "async": true, "timeout": 5 } ] } ],
  "PostToolUse":       [ { "matcher": "Edit|Write|MultiEdit",
                           "hooks": [ { "type": "command", "command": "'<data>/bin/current/kelta-ctl' hook", "async": true, "timeout": 5 } ] } ],
  "Stop":              [ { "hooks": [ { "type": "command", "command": "'<data>/bin/current/kelta-ctl' hook", "async": true, "timeout": 5 } ] } ],
  "StopFailure":       [ { "hooks": [ { "type": "command", "command": "'<data>/bin/current/kelta-ctl' hook", "async": true, "timeout": 5 } ] } ],
  "SessionEnd":        [ { "hooks": [ { "type": "command", "command": "'<data>/bin/current/kelta-ctl' hook", "timeout": 5 } ] } ]
} }
```
- The path is POSIX single-quoted (macOS data dir contains a space). `claude.hook_transport = "http"` replaces every event except SessionStart with `{"type":"http","url":"http://127.0.0.1:<port>/hook/<sid>","headers":{"Authorization":"Bearer ${KELTA_HOOK_TOKEN}"},"allowedEnvVars":["KELTA_HOOK_TOKEN"],"timeout":2}`.
- `kelta-ctl hook` reads stdin (cap 1 MiB), reads `KELTA_SESSION_ID`, `KELTA_HOOK_TOKEN`, `KELTA_SOCK`, writes one line `{"v":1,"cmd":"hook","session":…,"token":…,"payload":{…}}` to the ctl socket, **always exits 0**, < 5 ms.
- `claude.extra_hooks` is merged in. A user's `disableAllHooks` disables these too → hooks-inactive heuristics (ARCHITECTURE §7.6).
- `mcp.json`: `{"mcpServers":{"kelta":{"type":"http","url":"http://127.0.0.1:<port>/mcp/<sid>","headers":{"Authorization":"Bearer ${KELTA_MCP_TOKEN}"}}}}`. MCP tools: `get_ticket()`, `transition_ticket({to})`, `add_ticket_comment({markdown})`, `open_in_editor({path, line?})`, `create_pr({title?, body?, draft?})`, `list_review_requests()`, `add_review_comment({path, line, body})`, `notify({message})`; ticket tools return an error text "no ticket linked" for unlinked sessions. `add_review_comment` adds a line comment (new-side `line` of `path`) to the pending review of the PR under review in the session's review-kind work item (`CodeHost::add_pending_comment`); nothing is published until the user submits in Kelta (`a` / `c` / `m`); error text "no pull request under review in this session" otherwise. `transition_ticket` and `add_ticket_comment` go through `CoreApi::ticket_transition` / `ticket_comment` (core invalidates the tickets cache and publishes `ticket.transitioned` / `ticket.commented` with the session as context).
- **Gate C1 (verified with `claude` 2.1.295, macOS):** `KELTA_REAL_CLAUDE=1 cargo test -p kelta-core --test real_claude -- --nocapture` runs the launcher through a real `Core` (PTY, ctl socket, installed `kelta-ctl`, loopback MCP) in a Kelta worktree, in print mode via `claude.extra_args = ["-p"]`, haiku/low effort, two model runs: SessionStart → UserPromptSubmit → PermissionRequest (fires in print mode too) → Stop → SessionEnd gives Running → Working → NeedsInput → Done → Exited; `--session-id` is honoured; `mcp__kelta__notify` is callable; `session_restart` on the same session id rebuilds the work item's restore argv (`--resume <uuid>`, files regenerated) and resumes the conversation in the worktree (`source: "resume"`), with `mcp__kelta__notify` still reaching that session; a project `.claude/settings.json` hook still runs next to Kelta's (`--settings` adds) and `~/.claude/settings.json` is untouched. Interactive sessions in a repo Claude does not trust stop at its workspace-trust dialog and hooks start only once it is accepted (a worktree inherits the trust of its main repository); the hooks-inactive fallback covers the gap and the first SessionStart switches back to hooks. Shift+Enter: Claude's own `/terminal-setup` binds it to ESC CR (`\u001b\r`), what Kelta sends for Claude sessions; the live keystroke is checked by hand.

## 9. Provider plugins (KPP)

A plugin with a `[provider]` table serves tracker **or** code-host accounts from its own process. Kelta talks to it through `kelta_plugins::kpp` (`KppTracker` / `KppCodeHost`), which implement the same `Tracker` / `CodeHost` traits as the built-in providers, so the inbox, boards, work saga, MCP tools and reviews work unchanged. Example: `examples/plugins/json-tracker` (a Node script, tickets in a JSON file).

```toml
permissions = ["provider"]

[provider]
kind = "tracker"                         # tracker | codehost
command = "node"                         # program name (login-shell PATH) or a path inside the plugin dir
args = ["tracker.cjs"]                   # runs with the plugin dir as cwd
timeout_ms = 30000                       # per call (default)
caps = { comment = true, assign = true } # TrackerCaps (board_columns, assign, comment, transitions_need_fetch, projects_v2)
browser_url = "{web_url}/browse/{key}"   # tracker browser_url (default "{web_url}/{key}")
branch_key = "{key}"                     # tracker branch_key (default "{key}")
fetch_refspec = "pull/{number}/head:{branch}" # code host (this default)
```

The synchronous trait methods (`kind`, `caps`, `browser_url`, `branch_key`, `fetch_refspec`) are answered from these fields without a round trip. Placeholders: `{base_url}`, `{web_url}` (falls back to `base_url`), `{key}`, `{id}` (tickets), `{repo}`, `{number}`, `{branch}` (reviews). `kind()` is `plugin` for both. `repo_from_remote` returns nothing in v0.1 (no core caller).

**Accounts** point at the plugin (SETTINGS §2): `[accounts.demo] kind = "plugin_tracker" | "plugin_codehost", plugin = "json-tracker"`, plus any `base_url`, `web_url`, `user`, `email`, `secret`… the plugin wants. `TrackerFactory` / `CodeHostFactory` are wrapped by `KppFactory`, which builds plugin accounts and passes every other kind through.

### 9.1 Transport

JSON-RPC 2.0 over the process's stdin/stdout, **one message per line** (UTF-8 JSON, `\n`-terminated, no embedded newlines; a JSON encoder never emits a raw newline inside a string, so no escaping rule is needed). Chosen over LSP-style `Content-Length` framing because every language reads lines with its standard library (`readline`, `sys.stdin`, `bufio.Scanner`) and a human can drive a provider from a terminal. Kelta sends requests with integer ids and may have several in flight; the plugin answers each with the same id, in any order. Lines without an id (notifications) are ignored. stderr is free-form logging: Kelta keeps the last 64 KiB and attaches its tail to crash errors.

A line that is not JSON or exceeds 8 MiB is a protocol violation: Kelta kills the process and fails the pending calls (`upstream`).

### 9.2 Lifecycle

- One process per plugin, spawned on the **first call** (nothing runs at startup or for unused accounts), in its own process group, with the plugin dir as cwd and a minimal environment (`PATH` of the login shell, `HOME`, `USER`, `LANG`, `TMPDIR`; nothing else is inherited).
- Every call requires the plugin to be installed, enabled and granted `provider`; otherwise `permission_denied` / `not_found` / `invalid_argument`, and no process is started.
- Per-call timeout (`timeout_ms`) → `timeout`; the process is kept (one slow call does not kill the others).
- Crash (stdout closed) → the in-flight calls fail with `upstream`; the next call respawns it, at once after a first crash, then after 0.5 s, 1 s, 2 s … ≤ 30 s while it keeps crashing (calls in that window fail with `retry_after_ms`); one successful call resets the backoff. No timer is armed: the restart is driven by the next call.
- Killed (process group, SIGKILL) on disable, uninstall, grant change, manifest update (the next call starts the new version) and quit.
- No idle exit: an idle provider costs its own memory only, and an idle timer would break the "no timers" rule (ARCHITECTURE D11). Keep providers small. A provider may exit by itself when idle: the next call respawns it at once (only exits with no successful call in between back off).

### 9.3 Methods

Every request's `params` carries, besides the method's own fields:

| Field | Value |
|---|---|
| `account_id` | the Kelta account id (`"demo"`) |
| `account` | the account table (`AccountConfig` JSON: `kind`, `plugin`, `base_url`, `web_url`, `user`, `email`, `auth`, `secret` (the reference, not the value), …) |
| `secret` | the secret resolved by Kelta's secret chain (keychain, encrypted file, `env:`, `command:`, `gh-cli` …), or `null` when the account has none. The plugin never reads Kelta's secret store. |

Methods mirror the trait methods; shapes are the generated types (`ui/src/lib/gen/*.ts`, fixtures in `crates/kelta-proto/fixtures/*.json`):

| Method | Params | Result |
|---|---|---|
| `tracker.me` | `{}` | `User` |
| `tracker.list` | `{view: TrackerView, cursor: Cursor \| null}` | `{items: [Ticket], next: Cursor \| null}` |
| `tracker.get` | `{ticket: TicketRef}` | `TicketDetail` (`body_html` may be `""`: Kelta renders `body_md`) |
| `tracker.columns` | `{binding: TrackerBinding}` | `[Column]` (`order` = 0..n) |
| `tracker.transitions` | `{ticket}` | `[Transition]` |
| `tracker.transition` | `{ticket, transition_id, fields: JSON \| null}` | `Ticket` |
| `tracker.comment` | `{ticket, markdown}` | `null` (not called unless `caps.comment`) |
| `tracker.assign` | `{ticket, who: Assignee}` | `Ticket` (not called unless `caps.assign`) |
| `codehost.me` | `{}` | `User` |
| `codehost.changed_since_last` | `{}` | `bool` (optional: method not found = `true`) |
| `codehost.list_reviews` | `{query: ReviewQuery}` | `[Review]` |
| `codehost.get` | `{review: ReviewRef}` | `ReviewDetail` |
| `codehost.approve` | `{review, head_sha}` | `null` |
| `codehost.comment` / `codehost.request_changes` | `{review, body}` | `null` |
| `codehost.add_pending_comment` | `{review, path, line, body}` | `null` (optional) |
| `codehost.create` | `{pr: PrCreate}` | `Review` |
| `codehost.find_for_branch` | `{repo, branch}` | `Review \| null` |

Kelta does not trust the results: every `TicketRef` / `ReviewRef` it gets back is rewritten to the calling account, HTML (`body_html`, comments) is sanitized again in Rust (D12), comments are capped at 20, and a result that does not match the type fails the call (`upstream`, "bad result").

### 9.4 Errors

A JSON-RPC `error` maps to `ErrorCode` (ARCHITECTURE §4): `data.code` (an `ErrorCode` string such as `"needs_auth"`, `"not_found"`, `"conflict"`) wins; else `data.status` (the upstream HTTP status) is mapped like the built-in providers (401 → `needs_auth`, 403 → `permission_denied`, 404 → `not_found`, 409 → `conflict`, 422 → `invalid_argument`, 429 → `rate_limited`, …); else `-32601` (method not found) → `unsupported`; anything else → `upstream`. `message` (≤ 300 chars kept) is shown to the user. `needs_auth` also drops Kelta's cached secret, as for built-in providers.

### 9.5 Conformance suite

`cargo run -p xtask -- kpp-check <plugin dir> [--account '<json>'] [--secret <value>]` starts the provider and runs the **same contract** the built-in providers pass in their tests (`kelta_proto::testing::conformance`: identity, list shape and unique keys, account on every ref, sanitized detail, discovered transitions, column order, writes per caps; for code hosts both review lists, detail, approve / comment / request changes, create, find, refspec). The contract writes, so point the provider at test data (`--account '{"base_url":"/tmp/tickets.json"}'`). On failure it prints the broken rule and the provider's stderr tail. `xtask`'s own test runs it on a copy of `examples/plugins/json-tracker`.

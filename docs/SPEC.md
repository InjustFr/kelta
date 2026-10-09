# Kelta v0.1 — Product spec

Kelta lets a developer take a ticket from their tracker (Jira, Redmine, GitHub Issues, GitLab Issues, Linear) and build it in a git worktree with **Claude Code and a terminal editor (nvim) side by side** in built-in terminals; review the PRs/MRs they are asked to review; move tickets; run tools (lazydocker, lazygit, sl web) inside the app; work on **several projects in one window**; extend everything with tools, triggers and plugin screens; configure everything through settings. Open source (MIT). macOS 13+ and Ubuntu 24.04+ (Hyprland, Sway, GNOME). It must be **fast and small** (ARCHITECTURE §13).

Glossary: **Project** = named set of local repos + tracker binding + code-host bindings + defaults. **Session** = one PTY process (Claude, nvim, shell, tool). **Tab** = split layout of panes within a project. **WorkItem** = ticket/review/branch ↔ worktree ↔ branch ↔ sessions ↔ PR link. **Inbox** = aggregated view across all projects.

---

## 1. Window and navigation

```
┌──┬───────────────────────────────────────────────────────────────┐
│IN│ [SHOP-142 Rate limit ●] [Shell] [Board] [lazydocker]   +      │  ← TabBar (active project)
│──│┌ WorkItem header: SHOP-142 · In Progress ▾ · feat/SHOP-142-… ↑2↓0 · Create PR · Ticket · Finish ┐
│S●││ claude (Working)              │ nvim                         │
│A │├───────────────────────────────┤                              │
│H ││ …                             │                              │
│+ │└───────────────────────────────┴──────────────────────────────┘
└──┴ StatusBar: project · branch · session status · hooks ok · 142 MB ┘
```

- **ProjectRail** (48 px, left): Inbox (badge = review requests + needs-input count across projects), open projects (icon/letter, colour, attention dot: red needs input, amber error, green done-unseen, blue activity), Home (built-in project rooted at `~`, for unrelated work; no tracker), `+` (open/create project). Drag to reorder; right-click: close, settings, remove.
- **Switching projects** never stops or pauses sessions; attention keeps updating on the rail. Target ≤ 50 ms warm.
- **TabBar**: tabs per project (ticket tabs titled `KEY title`, plain tabs, view tabs). Tab dot = max attention of its sessions. Middle-click closes (confirm if a live process remains).
- **Panes** contain: terminal, web tool, plugin screen, Tickets (list/board), Ticket detail, Reviews, Review detail, Inbox, WorkItem, Settings, Diagnostics, Welcome. Drag the gutter to resize; zoom a pane; close a pane (terminal sessions keep running as **background sessions** listed in the palette and the project's session list unless killed).
- **Command palette** (fuzzy): actions, projects, sessions across all projects (with status), tickets (cached), PRs, tools, plugin commands, settings sections.
- **Project switcher**: fuzzy list of configured projects (open or not); Enter opens/activates.
- **StatusBar**: active project, branch of the focused session's cwd (read on focus, no polling), focused session status, hooks health, optional perf HUD (Kelta memory, separately from child processes).

## 2. Projects (multi-project)

- Create from folder: `project_detect` infers repos (git roots), remotes → code-host hints (github.com / gitlab host), tracker hints (Jira key pattern in recent branch names, `.github` presence). The "New project" sheet lets the user edit id, name, colour, repos (primary + others), tracker account + view(s), code-host bindings per repo, worktree root/branch template/base, env, setup commands, default session template. Writes `~/.config/kelta/projects/<id>.toml`.
- Several projects are open at once; open set and order persist. `kelta-ctl open <path>` opens/creates and focuses.
- Per-project groupings: tabs, layouts, sessions, work items, tools, triggers, settings overrides, tickets/reviews views.
- Aggregated views: **Inbox** = "My tickets" (all open projects' tracker views, grouped by project) + "Review requests" (all configured code-host accounts, grouped by project, unmatched items in **Other**) + "My PRs" + "Needs input" (sessions across projects).
- Close project: sessions keep running unless "Close and stop sessions" is chosen. Remove project: moves config to `projects/.trash/`.

## 3. Core flows

### 3.1 Start work on a ticket
Entry points: Tickets/Board/Inbox (select + `Enter`→detail, **Start work** button or `Mod+Enter`), palette "Start work on…", `kelta-ctl start SHOP-142 [--project shop]`, trigger action `start_work`.

1. `work_plan` returns a **StartWorkPlan**, shown in a sheet (skipped when `work.plan_preview = false`, except when a choice is required):
   ```
   StartWorkPlan { project_id, source: Ticket{ticket}, repo_id (picker if >1; remembered per ticket; repo_rules),
     base: "main", branch: "feat/SHOP-142-rate-limit-login" (template; validated with git check-ref-format),
     branch_exists: Option<{has_worktree, choice: Reuse|Suffix}> , worktree_path, template_id: "claude+editor",
     claude: {profile: "default", model, effort, permission_mode, prompt (rendered, editable)},
     side_effects: {assign_me: bool, transition_to: Option<target>, comment: Option<text>, run_setup: bool},
     existing: Option<WorkItemId>  // → sheet becomes "Resume"
   }
   ```
2. **Start** → `work_start` runs the journaled saga (each step idempotent; progress checklist in the sheet/toast; `Retry`/`Skip` per failed step):
   1. `ticket.before_start` (blocking triggers may veto with a message).
   2. Fetch ticket; write `<runtime>/s/<sid8>/ticket.md` (key, title, URL, status, labels, Markdown body, last 10 comments).
   3. `git fetch <remote> <base>` (20 s timeout; offline = warning), `git worktree add -b <branch> <path> <remote>/<base>` (or reuse); copy `worktree.include` globs (and `.worktreeinclude` patterns); add `.kelta/` to `.git/info/exclude`.
   4. Generate Claude files: `claude-settings.json` (hooks, ARCH §7.6), `mcp.json`, `context.md`.
   5. Create tab `KEY title` with the template layout (default Claude | nvim, 50/50). If `worktree.setup` is non-empty: a visible `setup` pane below; when `worktree.setup_blocking=true`, Claude waits for exit 0 (failure → Retry / Continue anyway).
   6. Spawn editor (`nvim --listen <runtime>/s/<sid8>/nvim.sock .`) and Claude:
      `claude --session-id <uuid> -n "SHOP-142 Rate-limit login" --model <m> --effort <e> --permission-mode <p> --settings <run>/claude-settings.json --mcp-config <run>/mcp.json --allowedTools "mcp__kelta__*" --append-system-prompt-file <run>/context.md "<prompt>"` (cwd = worktree).
   7. Tracker side effects **after** sessions are up, in parallel, each non-fatal: assign me; transition to `status_map.start` (resolved by category or name from `transitions()`; `NeedsFields` → small form built from the field errors, or "Open in browser"); optional comment.
   8. Persist WorkItem; emit `ticket.started`, `worktree.created`; ticket card shows a "local work" badge.
3. **While working:** Claude status in pane header/tab/rail; notifications when Claude needs input or finishes while its pane is hidden or the window unfocused. Claude edits → nvim `:checktime` (`editor.follow_claude_edits = reload`; `open` also opens the file without stealing focus). `Mod+Shift+L` in nvim sends the visual selection to Claude as `@path#Lx-y` (bracketed paste when enabled). Claude can call MCP tools `get_ticket`, `transition_ticket`, `add_ticket_comment`, `open_in_editor`, `create_pr`, `list_review_requests`, `notify`.
4. **WorkItem header:** ticket key + status (click → Move to…), branch, ahead/behind (computed when the tab gains focus), buttons **Create PR**, **Open ticket**, **Open in browser**, **Finish**.
5. **Create PR:** `git push -u origin <branch>` in a transient visible pane (credential prompts work) → if a PR already exists for the branch (Claude may have used `gh`), link it; else create via CodeHost with `work.pr.title_template` (`{key}: {title}`), `body_template` (ticket link, `Closes #n` for GitHub issues), `draft` → transition ticket to `status_map.review`, optional comment with PR URL → `pr.created`.
6. **Finish** (offered when PR merged or ticket Done, or manually): kill sessions (confirm if Claude is working), `git worktree remove` (refused if dirty or unpushed unless **Force**), optionally delete local branch, transition to `status_map.done` if configured, mark WorkItem finished. `git worktree prune` runs at app start.
7. **Resume:** opening the ticket again or the WorkItem from the palette focuses or recreates its tab; Dormant sessions respawn on show (`claude --resume <uuid>`, fallback `--continue`).

### 3.2 Standalone session (no ticket)
`Mod+T` → picker: template (Shell, Claude, nvim, Claude+nvim, any tool, plugin templates), project (active, any open, Home), cwd (project repos, existing worktrees, recent dirs, browse), placement (new tab / split right / split down). Spawned immediately (no worktree, no tracker call). Claude sessions still get hooks + MCP (`get_ticket` returns "no ticket linked"). "New branch workspace" = start-work flow with a typed name instead of a ticket (tracker steps skipped). Palette "Link session to ticket…" sets the link (metadata only; Claude receives the ticket path as a message if confirmed).

### 3.3 PR / MR review list and actions
- **Reviews** pane per project (repos bound to the project) or aggregated (Inbox, all accounts, "Other" for unbound repos). Sections: *Review requested*, *My PRs*. Row: title, repo/project chip, author, draft, CI (✓ ✗ ● –), decision, my state, size (+/−), age, linked ticket keys. Filters: include drafts, repo (team requests follow `reviews.include_team_requests`).
- **Detail:** sanitized description, reviewers, checks summary, file list (+/− counts). Actions: **Approve** (sends the shown `head_sha`; stale → "PR changed, refresh"), **Comment**, **Request changes** (GitLab: comment + optional unapprove), **Open in browser**, **Review locally**.
- **Review locally** (`work_plan` with `Review` source → same sheet): fetch `pull/<n>/head:kelta/pr-<n>` (GitHub) / `merge-requests/<iid>/head:kelta/mr-<iid>` (GitLab) → worktree `<root>/review-<n>` → template `review` (Claude with profile `review`, `--permission-mode plan`, prompt from `claude.prompt_templates.review`; nvim with `editor.review_args`, e.g. `-c "DiffviewOpen origin/{base}...HEAD"`; shell with `git diff --stat origin/<base>...`).
- New review requests detected via `seen_reviews` (never on the first poll after start) → desktop notification (toggle) + rail/Inbox badge.

### 3.4 Move ticket status
- **Board:** columns from `tracker_columns` (project `tracker.columns` override; default by status category: To do / In progress / In review / Done). Drag a card (or select + `m`) → `tracker_move`: resolves column → transition by category or names; several candidates → small picker; none → toast "No transition to <column> — Open in browser"; `NeedsFields` → field form or browser. Optimistic move with rollback on error.
- **Detail:** "Move to…" menu filled from `tracker_transitions`; assign (Me / none), comment box (Markdown).
- **Palette:** "Move SHOP-142 to…".
- Provider semantics: Jira transitions; Redmine `allowed_statuses`; GitHub Projects v2 Status (or open/closed); GitLab scoped labels + close/reopen; Linear workflow states of the ticket's team.

### 3.5 Open a tool
Tools are opened from the palette ("Open tool: lazydocker"), the `+` menu of the TabBar, a tool's keybinding, or a trigger. `tool_open` → PTY tools become sessions (`kind = Tool`, cwd per tool template, exit banner with **Relaunch**, `close_on_exit` option); web tools start their server (if any), wait for readiness from stdout (no polling), then open a Web pane. Embed `auto`: iframe; if a HEAD probe sees `X-Frame-Options`/`frame-ancestors` → local proxy that strips them (WebSocket passthrough); failure → **Open in browser** button. Web tool processes stop when the pane/tab closes (`lifecycle`), or with the project. Missing binary → empty state with `install_hint` and "Check again".

### 3.6 Plugin screens
Plugins contribute screens opened from palette/commands/keybindings/triggers into a tab or pane. A screen is a sandboxed iframe created when shown and destroyed when hidden (unless `keep_alive`, listed in Settings → Performance, which keeps it while its pane stays mounted; a tab or project switch still destroys it). Install from directory / git URL / tarball shows the manifest, requested permissions in plain words and the SHA-256; grants stored in app state; updates requesting new permissions re-prompt. Disabled plugins contribute nothing.

---

## 4. Keyboard shortcuts

Principles: **never** steal plain Ctrl+letter, Alt/Meta+anything, Ctrl+Alt chords, Shift+Tab, Ctrl+Space, Ctrl+\\ or Super chords from the terminal on Linux. On Linux, `Mod` = **Ctrl+Shift** (in legacy encoding Ctrl+Shift+letter = Ctrl+letter, so nothing is lost). On macOS `Mod` = **Cmd** (never reaches the PTY except Cmd+C/V handled by Kelta). A configurable **app-prefix key** (tmux-style, default `ctrl+shift+space`, `keys.prefix`; `"off"` disables) followed by one key within `keys.prefix_timeout_ms` (1000) reaches every action. All bindings are rebindable in `[keys.bindings]`; the keybinding editor flags conflicts with the reserved list and with other bindings. Matching uses physical keys (`KeyboardEvent.code`) so layouts don't break digits.

| Action id | macOS | Linux | Prefix then |
|---|---|---|---|
| `palette.open` | Cmd+K | Ctrl+Shift+K | `:` |
| `project.switcher` | Cmd+P | Ctrl+Shift+P | `p` |
| `project.goto.1..9` | Cmd+1..9 | Ctrl+Shift+1..9 | `1..9` |
| `inbox.open` | Cmd+0 | Ctrl+Shift+0 | `0` |
| `project.next` / `project.prev` | Cmd+Ctrl+] / [ | Ctrl+Shift+PageDown / PageUp | `)` / `(` |
| `tab.next` / `tab.prev` | Cmd+Shift+] / [ | Ctrl+Shift+] / [ | `n` / `N` |
| `session.new` | Cmd+T | Ctrl+Shift+T | `c` |
| `pane.split_right` / `pane.split_down` | Cmd+D / Cmd+Shift+D | Ctrl+Shift+E / Ctrl+Shift+O | `%` / `"` |
| `pane.focus_left/down/up/right` | Cmd+Opt+←↓↑→ | (prefix only) | `h j k l` / arrows |
| `pane.zoom` | Cmd+Shift+Enter | Ctrl+Shift+Z | `z` |
| `pane.close` | Cmd+W | Ctrl+Shift+W | `x` |
| `tickets.open` | Cmd+Shift+J | Ctrl+Shift+J | `t` |
| `reviews.open` | Cmd+Shift+R | Ctrl+Shift+R | `r` |
| `attention.next` (next session needing input, any project) | Cmd+Shift+U | Ctrl+Shift+U | `u` |
| `work.start` (ticket views only, not in terminals) | Cmd+Enter | Ctrl+Enter | `s` |
| `work.new` (New work item: task, `wip/` branch, Claude; FLOW §4.3) | Cmd+Shift+N | Ctrl+Shift+N | `w` |
| `work.link` (Link to ticket… for the focused scratch item; palette only) | — | — | — |
| `editor.send_selection` (editor pane) | Cmd+Shift+L | Ctrl+Shift+L | `@` |
| `terminal.search` | Cmd+F | Ctrl+Shift+F | `/` |
| `terminal.copy` / `terminal.paste` | Cmd+C / Cmd+V | Ctrl+Shift+C / Ctrl+Shift+V | `[` / `]` |
| `settings.open` | Cmd+, | Ctrl+Shift+, | `,` |
| `window.toggle` (global) | via `kelta-ctl toggle` bound in the compositor/system | | |

In-view single keys (only when a list/board has focus, never in terminals): `j/k` move, `Enter` open, `/` filter, `R` refresh, `m` move ticket, `a` assign me, `c` comment, `o` open in browser, `s` start work.

Terminal key handling: Shift+Enter in Claude sessions sends `ESC CR` (newline in Claude Code); passthrough elsewhere. macOS Option-as-Meta: `both` default (`left`, `right`, `none`). Copy-on-select off (Linux: selection always goes to PRIMARY; middle-click pastes PRIMARY). Paste uses bracketed paste when the app enabled `?2004`; multi-line paste into a shell prompt without bracketed paste asks for confirmation (`terminal.confirm_multiline_paste`).

## 5. Empty, loading and error states (each pane must implement)

| Situation | UI |
|---|---|
| First run | Welcome/onboarding: detects `claude`, `nvim`, `git`, `gh`, `glab`, versions (Claude ≥ `claude.min_version`), secret backend and notification daemon; buttons "Create project from folder", "Add account". |
| No projects | Home project only + "Create project from folder". |
| Project without tracker | Tickets pane: "No tracker bound — Bind a tracker" (opens project settings). |
| No accounts | Accounts wizard CTA. |
| Account `NeedsAuth` (401) | Banner on affected panes + rail warning; "Re-authenticate" opens account settings; polling paused. |
| Rate-limited | Stale data + "rate limited, retrying at hh:mm". |
| Offline | Stale data + "offline"; manual retry. |
| Empty ticket list | "Nothing assigned to you in <view>" + switch view. |
| Empty review list | "No review requests." |
| Tool not installed | Install hint + "Check again". |
| Claude/nvim missing | Pane shows "`claude` not found in your login PATH" + Diagnostics link. |
| Hooks inactive | Pane header badge "status hooks inactive · Fix". |
| Session exited | Banner with code, Enter = restart, `x` = close. |
| Worktree dirty on Finish | Dialog listing files; "Force remove" (destructive style) / Cancel. |
| Repo-local config untrusted | Banner "This repo's .kelta/config.toml wants to run commands — Review & trust". |
| Invalid config | Toast `file:line:col` + Settings issue list; last good config kept. |
| Secret Service missing (Linux) | Accounts wizard explains and offers `command:`/`env:` sources. |
| Notification daemon missing | Settings → Notifications warning; in-app toasts still work. |
| Web tool blocked from framing and proxy fails | "Open in browser". |
| Plugin screen error | Inline error with plugin id, "Reload screen", "Disable plugin". |

## 6. v0.1 scope

**In v0.1:** everything above; trackers Jira Cloud + Jira Data Center (basic), Redmine, GitHub Issues (+ Projects v2 Status), GitLab Issues, Linear; code hosts GitHub (incl. GHE) and GitLab (incl. self-managed); editors nvim (RPC), vim (keys), helix (launch only), emacs (emacsclient), external GUI editors (VS Code/Zed/JetBrains launched outside); tools tier, triggers tier, plugin manifests with commands/tools/triggers/screens/settings/keybindings; MCP server; deb + AppImage + dmg (signed/notarized on tag); docs.

**Later (designed, not built):** v0.2 — `keltad` session daemon (sessions survive quit), kitty keyboard protocol (xterm 6.1), process (KPP) provider plugins with a conformance suite, plugin KV storage API for screens, child-webview embed mode, Claude IDE WebSocket bridge, on-disk scrollback history log, Tauri updater (AppImage/macOS), AUR + Homebrew cask publishing, encrypted-file secret backend, OAuth/device flows, Bitbucket/Gitea, WASM logic plugins, rpm. Windows: out of scope.

## 7. Notifications

Desktop notifications (`tauri-plugin-notification`) fire only when the related pane is not visible or the window is unfocused (`notifications.only_when_unfocused`), outside `quiet_hours`: Claude needs input, Claude done, new review request, CI failed on my PR, my PR approved / changes requested, bell in a background project. Click → focus window, project, tab and pane (macOS: focus app if click routing unsupported). Dock badge (macOS) / urgency hint (Linux) = number of sessions needing input.

## 8. Platform behaviour

- **Linux:** Ubuntu 24.04+ primary (deb), AppImage best-effort, Arch/Fedora best-effort. Graphics section in settings (auto NVIDIA workaround, dmabuf, compositing, explicit sync, X11 backend) applied before webview start (restart required); `kelta --safe-graphics` and a "Kelta (safe graphics)" desktop action; automatic safe-mode retry after a failed launch. Decorations `auto` (none on Hyprland/Sway, native on GNOME), `native`, `none`, `custom` (app-drawn drag strip and resize handles). Global toggle via compositor binding:
  - Hyprland: `bind = SUPER, K, exec, kelta-ctl toggle`, `windowrulev2 = workspace 2, class:^(dev.kelta.Kelta)$`
  - Sway: `bindsym $mod+k exec kelta-ctl toggle`, `for_window [app_id="dev.kelta.Kelta"] …`
  - GNOME: Settings → Keyboard → Custom Shortcuts → `kelta-ctl toggle`.
  Fractional scaling may blur text (GTK3 renders at integer scale) — documented. IME: Kelta never sets `GTK_IM_MODULE`; fcitx5/ibus in QA matrix.
- **macOS:** Cmd modifier, custom app menu, Option-as-Meta setting, closing the window keeps sessions running (background mode), Dock click reopens, login PATH resolved for Dock launches.
- `kelta-ctl` commands: `toggle`, `palette`, `open <path>`, `focus-project <id>`, `start <ticket key|url> [--project]`, `start --task "<text>" [--project <id>]` (scratch work item, same saga as New work item), `new --template <id> [--cwd <dir>] [--project <id>]`, `emit <custom.event> --json '{…}'`, `trust <repo path>`, `editor-open <file>[:line]`, `hook` (internal), `version`.

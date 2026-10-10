# Kelta v0.1 — Product spec

Kelta lets a developer take a ticket from their tracker (Jira, Redmine, GitHub Issues, GitLab Issues, Gitea Issues, Linear) and build it in a git worktree with **Claude Code and a terminal editor (nvim) side by side** in built-in terminals; review the PRs/MRs they are asked to review; move tickets; run tools (lazydocker, lazygit, sl web) inside the app; work on **several projects in one window**; extend everything with tools, triggers and plugin screens; configure everything through settings. Open source (MIT). macOS 13+ and Ubuntu 24.04+ (Hyprland, Sway, GNOME). It must be **fast and small** (ARCHITECTURE §13).

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
- **Quitting Kelta** leaves the sessions running in the `keltad` session daemon (`terminal.session_host = "daemon"`); the next start re-attaches every live one where it was (shells, Claude, nvim keep their process and scrollback). Sessions that ended meanwhile restore as below.
- Close project: sessions keep running unless "Close and stop sessions" is chosen. Remove project: moves config to `projects/.trash/`.

## 3. Core flows

### 3.1 Start work on a ticket
Entry points: Tickets/Board/Inbox (select + `Enter`→detail, **Start work** button or `Mod+Enter`; in the Tickets list `s` opens the plan sheet and `S` starts without it), palette "Start work on…", `kelta-ctl start SHOP-142 [--project shop]`, trigger action `start_work`.

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
3. **While working:** Claude status in pane header/tab/rail; notifications when Claude needs input or finishes while its pane is hidden or the window unfocused. Claude edits → nvim `:checktime` (`editor.follow_claude_edits = reload`; `open` also opens the file without stealing focus). `Mod+Shift+L` in nvim sends the visual selection to Claude as `@path#Lx-y` (bracketed paste when enabled). Claude can call MCP tools `get_ticket`, `transition_ticket`, `add_ticket_comment`, `open_in_editor`, `create_pr`, `list_review_requests`, `get_review_feedback`, `add_review_comment`, `notify`.
4. **WorkItem header:** ticket key + status (click → Move to…), branch, ahead/behind (computed when the tab gains focus), buttons **Create PR**, **Open ticket**, **Open in browser**, **Finish**.
5. **Create PR:** `git push -u origin <branch>` in a transient visible pane (credential prompts work) → if a PR already exists for the branch (Claude may have used `gh`), link it; else create via CodeHost with `work.pr.title_template` (`{key}: {title}`), `body_template` (ticket link, `Closes #n` for GitHub issues), `draft` → transition ticket to `status_map.review`, optional comment with PR URL → `pr.created`.
6. **Finish** (offered when PR merged or ticket Done, or manually): kill sessions (confirm if Claude is working), `git worktree remove` (refused if dirty or unpushed unless **Force**), optionally delete local branch, transition to `status_map.done` if configured, mark WorkItem finished. `git worktree prune` runs at app start.
7. **Resume:** opening the ticket again or the WorkItem from the palette focuses or recreates its tab; Dormant sessions respawn on show (`claude --resume <uuid>`, fallback `--continue`).

### 3.2 Standalone session (no ticket)
`Mod+T` → picker: template (Shell, Claude, nvim, Claude+nvim, any tool, plugin templates), project (active, any open, Home), cwd (project repos, existing worktrees, recent dirs, browse), placement (new tab / split right / split down). Spawned immediately (no worktree, no tracker call). Claude sessions still get hooks + MCP (`get_ticket` returns "no ticket linked"). "New branch workspace" = start-work flow with a typed name instead of a ticket (tracker steps skipped). Palette "Link session to ticket…" sets the link (metadata only; Claude receives the ticket path as a message if confirmed).

### 3.3 PR / MR review list and actions
- **Reviews** pane per project (repos bound to the project) or aggregated (Inbox, all accounts, "Other" for unbound repos). Sections: *Review requested*, *My PRs*. Row: title, repo/project chip, author, draft, CI (✓ ✗ ● –), decision, my state, size (+/−), age, linked ticket keys. Filters: include drafts, repo (team requests follow `reviews.include_team_requests`).
- **Detail:** sanitized description, reviewers, checks summary, file list (+/− counts). Actions: **Approve** (sends the shown `head_sha`; stale → "PR changed, refresh"), **Comment**, **Request changes** (GitLab: comment + optional unapprove), **Open in browser**, **Review locally**.
- **Review locally** (`work_plan` with `Review` source → same sheet): fetch `pull/<n>/head:kelta/pr-<n>` (GitHub) / `merge-requests/<iid>/head:kelta/mr-<iid>` (GitLab) / `pull/<n>/head:kelta/pr-<n>` (Gitea) / the source branch (Bitbucket Cloud, which exposes no PR refs) → worktree `<root>/review-<n>` → template `review` (Claude with profile `review`, `--permission-mode plan`, prompt from `claude.prompt_templates.review`; nvim with `editor.review_args`, e.g. `-c "DiffviewOpen origin/{base}...HEAD"`; shell with `git diff --stat origin/<base>...`).
- Rows refresh on window focus through the list query (one request, also brings back PRs pushed after my review, badge "Updated since your review"), never through `get` on each row.
- New review requests detected via `seen_reviews` (never on the first poll after start) → desktop notification (toggle) + rail/Inbox badge.

### 3.4 Move ticket status
- **Status picker:** one component (`StatusPicker`) for every place a status changes: list `m`, the status chip on any row, the detail header, the work bar and the palette. Fed by `tracker_transitions` (cached per ticket and current status for the focused polling interval, dropped when the ticket is written): the ticket's own workflow as a line of native status names, then its legal transitions numbered `1`-`9` (`m` `2` moves), fuzzy filter, a needs-fields form, and on a conflict or tracker error the tracker's message with Open in browser.
- **Several tickets:** `x` marks the current ticket, `Shift+j` / `Shift+k` extend, `m` moves them all. The picker offers the transitions every marked ticket can take, matched by target status name (transition ids differ between workflows); a ticket already in that status is skipped.
- **Board:** columns from `tracker_columns` (project `tracker.columns` override; default by status category: To do / In progress / In review / Done). Drag a card (or select + `m`) → `tracker_move`: resolves column → transition by category or names; several candidates → small picker; none → toast "No transition to <column> — Open in browser"; `NeedsFields` → field form or browser. Optimistic move with rollback on error.
- **Detail:** the status chip opens the picker; assign (Me / none), comment box (Markdown). Layout and actions in §3.4b.
- **Palette:** "Move SHOP-142 to…".
- Provider semantics: Jira transitions; Redmine `allowed_statuses`; GitHub Projects v2 Status (or open/closed); GitLab scoped labels + close/reopen; Linear workflow states of the ticket's team; Gitea open/closed.

### 3.4a Choose ticket sources and who
- A **source** is a tracker view: a Jira board or filter, a Redmine project or query, a GitHub repo or Projects v2 board, a GitLab project, a Linear team, a Gitea repo. A project binds any number of them, each optionally from another account (`views[].account`; none = the binding account), so one project can mix Jira and GitHub tickets. Board mode and `tracker_columns` stay tied to the binding account.
- **Add a source** from the Tickets toolbar (source menu, `v`, then "Add source…") or Settings > Projects > Tracker: pick an account, search, press `Enter` on a hit (`tracker_sources` lists boards, projects, filters, teams, repos). A provider without discovery says so and offers the TOML.
- The Tickets pane, Now and the palette with no source chosen show **every source of the project**, one first page each, deduped by account and ticket key; Load more pages every source at once (one cursor per source). The source menu narrows the pane to one.
- Ticket search (palette, pickers: `tracker_search`) asks each source's tracker (`Tracker::search`: Jira `text ~`, Linear title/description, GitHub/GitLab/Gitea search, Redmine `subject ~`), not the cached page; a tracker without search, or a failed search, filters its cached first page.
- **Who** tabs: *Mine*, *Unassigned*, *Anyone* (keys `1` `2` `3`). The choice is saved with the pane. "Team" is Anyone grouped by assignee.
- **Current iteration** (per source toggle) keeps the open sprint (Jira), active cycle (Linear), current iteration (GitHub Projects v2), started milestone (GitLab) or the project's next open version (Redmine). Gitea has none.
- Statuses show the tracker's own names. The category (To do, In progress, In review, Done) only orders and colours them, so Backlog, Triage, QA or Blocked appear as they are. Group by Status, Assignee, Source or None (`g`); Done starts collapsed.
- The **person** control narrows the list to one assignee (client-side, from the assignees of the loaded first page per source; choosing one implies Anyone). Saved with the pane (`PaneContent::Tickets.person`).
- Group by (saved with the pane): **Flow** (default), Status, Priority, Sprint, Assignee, Source, None. Flow: *Doing* (work item active or in-progress status), *Waiting* (Claude needs input, review requested or required, CI red, or a status named blocked / on hold / waiting), *Ready* (to do, not started), *Backlog* (backlog / triage / icebox or an unknown status), *Done* (last 7 days only: a source with no status filter also fetches the first page of its closed tickets and keeps those done in the last 7 days). Sort within groups (saved): Priority, Updated, Age in status, Key.
- Each ticket carries `priority_rank` (0 = highest), `status_since` (RFC 3339; Jira `statuscategorychangedate`, Linear `startedAt`, other providers fall back to `updated_at`), `sprint`, `estimate` and `due`. A ticket not done shows an age badge from 7 days in its status, in place of the updated time (text tint only, `--k-warn` from 14, `--k-danger` from 21). The Doing header turns `--k-warn` above `tickets.wip_limit` (default 3); nothing is blocked.
- Sprint: a chip on every row unless grouped by sprint, quick filter `f` `s` (the active sprint, client-side), and the per-source `current_iteration` stays as the server-side limit.
- A row shows the linked pull request and its CI state: the work item's PR first, then any polled review whose `linked_tickets` name the ticket (`TicketItem.prs`). `p` opens it in Kelta's review detail when the repo is bound to a code host account, else the browser; `P` always the browser; several PRs open a small picker.

### 3.4b Ticket detail and split view
- In the Tickets pane (list, 720px or wider) the selected ticket's detail shows in a column on the right. `Space` toggles it, `Enter` focuses it, `Esc` returns to the list, `Shift+Enter` opens the standalone detail pane (also what `Enter` does in a narrow pane). Both use the same `TicketDetail` body.
- Top to bottom: key, title, status chip (status picker), action bar, meta grid (assignee, priority, sprint, estimate, due, labels, updated), pull requests (title, state, CI, review, branch), description (task-list checkboxes read-only), comments (last 20, compose box, `Mod+Enter` posts).
- Action bar: Move `m`, Start / Resume work `s`, Open PR `p`, Assign `a`, Comment `c`, Copy branch `y`, Open in browser `o`. Each carries its key; an action that cannot run stays visible and dimmed with its reason as the tooltip and as the toast when its key is pressed: the tracker lacks the capability (`TrackerCaps.assign`, `comment`, carried on `TicketItem.caps` / `TicketDetail.caps`), no PR is linked, no branch yet. The selected or hovered row (and a row with focus inside) shows a compact Move / PR / Start.
- Sub-tasks (`TicketDetail.children`: Jira subtasks, Linear children, GitHub sub-issues, GitLab child items, Redmine children; Gitea has none) list under the pull requests with their status chip. `Enter` on one opens its detail, `s` starts work on it (`S` with no sheet).
- Not in the detail yet: refine, lifecycle lamp, assigning someone else, editing priority.

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
| `inbox.open` (label "Open Now") | Cmd+0 | Ctrl+Shift+0 | `0` |
| `project.next` / `project.prev` | Cmd+Ctrl+] / [ | Ctrl+Shift+PageDown / PageUp | `)` / `(` |
| `tab.next` / `tab.prev` | Cmd+Shift+] / [ | Ctrl+Shift+] / [ | `n` / `N` |
| `session.new` | Cmd+T | Ctrl+Shift+T | `c` |
| `pane.split_right` / `pane.split_down` | Cmd+D / Cmd+Shift+D | Ctrl+Shift+E / Ctrl+Shift+O | `%` / `"` |
| `pane.focus_left/down/up/right` | Cmd+Opt+←↓↑→ | (prefix only) | `h j k l` / arrows |
| `pane.zoom` | Cmd+Shift+Enter | Ctrl+Shift+Z | `z` |
| `pane.close` | Cmd+W | Ctrl+Shift+W | `x` |
| `tickets.open` | Cmd+Shift+B | Ctrl+Shift+B | `t` |
| `reviews.open` | Cmd+Shift+R | Ctrl+Shift+R | `r` |
| `attention.next` / `attention.prev` ("Next waiting", Mod+J: any project, cycling, in priority bands: Claude needs input, error or rate-limited, ready for review, feedback or red CI on my PRs, review requests blocking first; oldest first within a band; HUD `2/7 · needs input · SHOP-142`, or `nothing waiting` with `Enter` opening Up next / Tickets) | Cmd+J / Cmd+Shift+J | Ctrl+Shift+J / (prefix only) | `u` / `U` |
| `attention.peek` ("Peek waiting session": popover with the last 15 lines, a detected permission menu as `1`/`2`/`3` buttons and a reply field; `Tab` next needs-input session, `Mod+Enter` go to it, `Esc` back; also on hovering a rail row, a tab lamp or a Now row) | Cmd+Shift+Y | Ctrl+Shift+Y | `y` |
| `nav.back` / `nav.forward` (jumplist of every focus change, 100 places) | Cmd+Ctrl+← / → | (prefix only) | `-` / `=` |
| `work.menu` (work menu of the focused tab's work item, from any pane) | Cmd+. | Ctrl+Shift+. | `.` |
| `work.next`, `work.review_diff`, `work.ship`, `work.mark_reviewed`, `work.fix`, `work.rebase`, `work.rebase_continue`, `work.rebase_abort`, `work.conflicts`, `work.skip_step`, `work.go_claude`, `work.link`, `work.create_ticket`, `work.open_ticket`, `work.open_pr`, `work.finish`, `work.park` (palette "Work: …", focused item), `work.finish_merged` | unbound | unbound | |
| `work.start` (ticket views only, not in terminals) | Cmd+Enter | Ctrl+Enter | `s` |
| `work.new` (New work item: task, `wip/` branch, Claude; FLOW §4.3) | Cmd+Shift+N | Ctrl+Shift+N | `w` |
| `work.link` (Link to ticket… for the focused scratch item; palette only) | — | — | — |
| `work.create_ticket` (Create ticket… for the focused scratch item: file it in the project tracker, then link it; palette only) | — | — | — |
| `editor.send_selection` (editor pane) | Cmd+Shift+L | Ctrl+Shift+L | `@` |
| `editor.quickfix_claude` (palette "Quickfix: files Claude touched": the focused tab's Claude `files_touched` become its nvim's quickfix list, `]q` / `[q`) | unbound | unbound | |
| `terminal.search` | Cmd+F | Ctrl+Shift+F | `/` |
| `terminal.copy` / `terminal.paste` | Cmd+C / Cmd+V | Ctrl+Shift+C / Ctrl+Shift+V | `[` / `]` |
| `settings.open` | Cmd+, | Ctrl+Shift+, | `,` |
| `window.toggle` (global) | via `kelta-ctl toggle` bound in the compositor/system | | |

In-view single keys (only when a list/board has focus, never in terminals): `j/k` move, `Enter` open, `/` filter, `R` refresh, `m` move ticket, `a` assign me, `c` comment, `o` open in browser, `s` start work.

Terminal key handling: programs that enable the kitty keyboard protocol get kitty-encoded keys (`terminal.keyboard_protocol`, default `kitty`; Shift+Enter → `CSI 13;2u`). Otherwise Shift+Enter in Claude sessions sends `ESC CR` (newline in Claude Code); passthrough elsewhere. macOS Option-as-Meta: `both` default (`left`, `right`, `none`). Copy-on-select off (Linux: selection always goes to PRIMARY; middle-click pastes PRIMARY). Paste uses bracketed paste when the app enabled `?2004`; multi-line paste into a shell prompt without bracketed paste asks for confirmation (`terminal.confirm_multiline_paste`).

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
| Secret Service missing (Linux) | Accounts wizard explains and offers the encrypted file (`file:`), `command:` and `env:` sources. |
| Notification daemon missing | Settings → Notifications warning; in-app toasts still work. |
| Web tool blocked from framing and proxy fails | "Open in browser". |
| Plugin screen error | Inline error with plugin id, "Reload screen", "Disable plugin". |

## 6. v0.1 scope

**In v0.1:** everything above; trackers Jira Cloud + Jira Data Center (basic), Redmine, GitHub Issues (+ Projects v2 Status), GitLab Issues, Gitea/Forgejo Issues, Linear; GitHub and GitLab sign-in with a pasted token or the OAuth device flow (user-registered OAuth apps, `oauth.client_ids`); code hosts GitHub (incl. GHE), GitLab (incl. self-managed), Bitbucket Cloud and Gitea/Forgejo; editors nvim (RPC), vim (keys), helix (launch only), emacs (emacsclient), external GUI editors (VS Code/Zed/JetBrains launched outside); tools tier, triggers tier, plugin manifests with commands/tools/triggers/screens/settings/keybindings; MCP server; Claude IDE bridge (opt-in `claude.ide_bridge`: openFile/openDiff in the editor pane); process (KPP) provider plugins with a conformance suite (`kelta-plugin.toml` `[provider]`, PLUGINS §9); deb + AppImage + dmg (signed/notarized on tag); docs.

**Later (designed, not built):** v0.2 — child-webview embed mode, Tauri updater (AppImage/macOS), AUR + Homebrew cask publishing, OAuth for the other providers, WASM logic plugins, rpm. Windows: out of scope.

## 7. Notifications

Desktop notifications (`tauri-plugin-notification`) fire only when the related pane is not visible or the window is unfocused (`notifications.only_when_unfocused`), outside `quiet_hours`: Claude needs input, Claude done, new review request, CI failed on my PR, my PR approved / changes requested, bell in a background project. Click → focus window, project, tab and pane (macOS: focus app if click routing unsupported). Dock badge (macOS) / urgency hint (Linux) = number of sessions needing input.

## 8. Platform behaviour

- **Linux:** Ubuntu 24.04+ primary (deb), AppImage best-effort, Arch/Fedora best-effort. Graphics section in settings (auto NVIDIA workaround, dmabuf, compositing, explicit sync, X11 backend) applied before webview start (restart required); `kelta --safe-graphics` and a "Kelta (safe graphics)" desktop action; automatic safe-mode retry after a failed launch. Decorations `auto` (none on Hyprland/Sway, native on GNOME), `native`, `none`, `custom` (app-drawn drag strip and resize handles). Global toggle via compositor binding:
  - Hyprland: `bind = SUPER, K, exec, kelta-ctl toggle`, `windowrulev2 = workspace 2, class:^(dev.kelta.Kelta)$`
  - Sway: `bindsym $mod+k exec kelta-ctl toggle`, `for_window [app_id="dev.kelta.Kelta"] …`
  - GNOME: Settings → Keyboard → Custom Shortcuts → `kelta-ctl toggle`.
  Fractional scaling may blur text (GTK3 renders at integer scale) — documented. IME: Kelta never sets `GTK_IM_MODULE`; fcitx5/ibus in QA matrix.
- **macOS:** Cmd modifier, custom app menu, Option-as-Meta setting, closing the window keeps sessions running (background mode), Dock click reopens, login PATH resolved for Dock launches.
- `kelta-ctl` commands: `toggle`, `palette`, `next` (`attention.next`), `back` (`nav.back`), `open <path>`, `focus-project <id>`, `start <ticket key|url> [--project]`, `start --task "<text>" [--project <id>]` (scratch work item, same saga as New work item), `new --template <id> [--cwd <dir>] [--project <id>]`, `emit <custom.event> --json '{…}'`, `trust <repo path>`, `editor-open <file>[:line]`, `hook` (internal), `version`.

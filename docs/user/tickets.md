# Working with tickets

The Tickets pane is a list, not a board: one row per ticket, grouped by flow (what you are doing, what waits on
someone, what is ready), with the keyboard doing most of the work. A detail column on the right follows the
selection. Board mode stays available with the list/board toggle.

**Tickets** on the rail (`Cmd+Shift+B`) shows this list across every source, outside any project: Mine is the
default, and its tab, source, grouping, sort and person come back after a restart.
Opening a ticket, its PR or its work from there switches to the ticket's project. The board needs one project's
columns: palette **Open project board** opens the active project's Tickets tab as a board (the `+` menu of the tab
bar opens the project's own Tickets tab).

## Sources

A source is a board tickets come from: a Jira board or filter, a Redmine project or query, a GitHub repo or
Projects v2 board, a GitLab project, a Linear team, a Gitea repo. A source can hold several views, from different
accounts, so one board can show Jira and GitHub tickets together.

- Sources live in your global settings (Settings > Sources), not in each project: one `[[sources]]` entry holds
  the account, the views, the columns and the status mapping, and lists the projects it feeds (none is fine). A
  board shared by several projects is set up once. Settings > Projects shows which sources feed each project.
  Boards from older versions moved out of the project files on first launch (a copy of each project file is kept
  in `projects/.trash/`).
- The Tickets view lists every source by default: the rail's Tickets shows all of them, also those feeding no
  project; a project's Tickets shows the sources that feed it (first page of each, duplicates removed). **Load
  more** pages every source that has more. Each row shows its source when several are listed, and Group by Source
  works everywhere.
- `v` (or the source button) narrows the list to one source; a source with several views lists them indented
  under it, to narrow to one view. The pane remembers the choice. With a single source that has a single view
  there is nothing to choose: no button, and `v` does nothing.
- Board mode needs one source (the list narrowed to one, or a scope with only one): it uses that source's
  columns. Otherwise the List/Board switch is hidden.
- The palette and the ticket pickers search the trackers themselves (Jira `text ~`, Linear title and description,
  GitHub / GitLab / Gitea search, Redmine subject), so they find tickets past the first page. A tracker that cannot
  search matches the loaded first page instead; a typed key is also looked up directly.
- With no source yet, Tickets and Now say "Add a ticket source: pick a board". **Pick a board** (also "Add
  source..." in the `v` menu and Settings > Sources) opens a sheet: choose the account, type to search, press
  `Enter` on a result. It creates a source labelled after the board, feeding the project you opened it from; the
  toast's action opens Settings > Sources to choose its projects. **Add view...** on a source adds a view to it.
  Providers that cannot list sources tell you so; edit the TOML instead ([views schema](../SETTINGS.md)).

## Mine, Unassigned, Team

Keys `1`, `2`, `3` switch the tabs; the pane remembers the choice. Counts show once loaded. Team is every open
ticket of your sources; group it by Assignee (`Shift+g`) for a per-person view.

The person menu next to the tabs narrows the list to one team person. Kelta builds the team from tickets and pull
requests it already loaded (assignees, reporters, PR authors and requested reviewers), and joins a tracker user to a
code host user only when it can prove they are the same: you, the same login on the same host, or the same email.
Otherwise the person stays apart and is marked "(tracker only)". Choosing a person switches to Team and is remembered
with the pane; "Any person" clears it. Picking a tab clears it too.

A row whose pull request waits on someone shows `waiting on Anna · 26h` (`+2` when several; hover for who and why)
next to the PR chip. Reviews and Now show the same chip. A ticket that only waits on its assignee shows no chip:
the avatar already says it.

## Current iteration

Each view can be limited to the current iteration (Settings > Sources, or `current_iteration = true`):

| Provider | Meaning |
|---|---|
| Jira | open sprint |
| Linear | active cycle |
| GitHub Projects v2 | current iteration |
| GitLab | started milestone (no Premium iterations needed) |
| Redmine | the project's next open version by due date, else the open version without a date |
| Gitea | not available |

## Keys

| Key | Action |
|---|---|
| `j` `k` | move the selection |
| `Enter` | with the detail column shown, focus it; hidden, a ticket with work runs its next step (as in Now), others open the detail (on a group header: expand or collapse) |
| `g` | open the detail: focus the detail column, or its own pane in a narrow pane |
| `Shift+Enter` | open the detail as its own pane |
| `Space` | show or hide the detail column |
| `Esc` | from the detail column back to the list; in the list, clear the multi-selection |
| `/` | filter |
| `1` `2` `3` | Mine, Unassigned, Team |
| `v` | choose sources |
| `Shift+g` | cycle the grouping |
| `f` `s` | only the current sprint (press again to clear) |
| `x` | add or remove the ticket from the selection |
| `Shift+j` `Shift+k` | extend the selection down or up |
| `m` | move status (all selected tickets, when there is a selection) |
| `s` | start or resume work (shows the plan sheet) |
| `S` | start work without the sheet |
| `p` | open the ticket's pull request in Kelta's review detail (the browser when the code host is not bound) |
| `P` | open the pull request in the browser |
| `a` `A` | assign to me, unassign |
| `u` `!` | assign to someone else, change the priority (in the detail) |
| `c` | comment |
| `r` | refine with Claude: proposed acceptance criteria, open questions and sub-tasks to post, copy or discard; kept criteria go into Claude's first prompt when you start work (in the detail) |
| `y` | copy the branch name (in the detail) |
| `o` | open in the tracker |
| `R` | refresh |

## Grouping and sorting

`Shift+g` cycles the grouping; the pane remembers it, and the sort, with its other settings.

| Group by | Groups |
|---|---|
| Flow (default) | **Doing**: work item active, or an in-progress status. **Waiting**: Claude needs you, a review is requested or required, CI is red, or the status name says blocked or on hold. **Ready**: to do and not started. **Backlog**: backlog, triage or icebox statuses, and statuses Kelta cannot place. **Done**: finished in the last 7 days (a source with no status filter also fetches its closed tickets for this); older ones are left out. |
| Status | the tracker's own status names, in progress first, Done collapsed |
| Priority | the provider's priorities, highest first, then no priority |
| Sprint | active sprints, other sprints, then no sprint |
| Assignee, Source, None | as named |

Within a group the sort is Priority (then most recently updated), Updated, Age in status (longest first) or Key.
Each ticket carries the provider's priority, how long it has been in its status, its sprint, estimate and due date.
GitHub and Gitea have no native priority: they rank from `priority:` style labels, and GitHub reads priority, sprint and estimate from Projects v2 fields.

- **Age badge**: a ticket not done shows `7d`, `14d`, `21d`... in its row after a week in the same status, in place of
  the last-update time; the text warms at 14 days and turns red at 21. Providers that do not date a status change fall back to the last update.
- **WIP limit**: when more tickets are in Doing than `tickets.wip_limit` (default 3, [Settings](../SETTINGS.md)) the
  group header says so in the warning colour. It only warns; nothing is blocked.
- **Sprint**: rows show the sprint name unless you group by sprint. `f` then `s` keeps the active sprint only (a quick
  filter, separate from the per-source current iteration below).

## The detail column

Wider than about 720px, the pane splits: the list on the left, the selected ticket on the right. `Space` hides
or shows it, `Enter` moves the keys into it and `Esc` gives them back. `Shift+Enter` opens it in its own pane,
which is also what `g` does in a narrow pane.

## Next step

A ticket with work under way shows the work's phase in its row, with its lamp (Claude working, Claude needs
you, To review, Changes requested, Checks failed, New comments, In review, Merged...): the state Now shows, from the same signals. With the
detail column hidden (or a narrow pane), `Enter` runs the phase's next action, as `Enter` does in Now (the key hints name it); `g` opens
the detail instead.

From top to bottom: key, title, status (a button: it opens the status picker), the action bar, a grid with
assignee, priority, sprint, estimate, due date, labels and last update, the pull requests, the description (task
lists are shown, not editable) and the last 20 comments with a box to add one (`Cmd+Enter` posts).

The action bar lists every action with its key: Move `m`, Start or Resume work `s`, Open PR `p`, Assign `a`,
Comment `c`, Copy branch `y`, Open in browser `o`. An action the tracker cannot do (a tracker without assignment
or comments), or that has nothing to act on (no pull request yet, no branch yet), stays visible but dimmed; hover
it or press its key to read why. The selected or hovered row shows the same actions in small form (Move, pull
request, Start work).

## Pull requests

A row shows the pull request linked to the ticket as `#12` (`!12` on GitLab) with one lamp for CI and review:
halo dot changes requested, diamond CI failed, ring CI running, dot approved (or passed with no review needed),
small dot waiting for review. Hover it for both states. A ticket links to the pull requests of its work item and to any pull request in your
review lists whose branch or title carries the ticket key. The detail lists each with its title, state, CI,
review state and branch. `p` opens it in Kelta's review detail when its repository is bound to a code host
account, else in the browser; `P` always opens the browser. With several pull requests a small menu asks which.

## Moving status

`m` opens the status picker: the ticket's workflow as a line of the tracker's own status names, then the
transitions it allows, numbered. Type to filter; press a digit to move at once (`m` `2`). If the tracker needs
more fields, Kelta asks for them or offers the browser; a refused move shows the tracker's message and Open in
browser. Only moves the tracker allows are listed.

The same picker opens from the status chip on any row, the status in the detail, the work bar and the command
palette ("Move SHOP-142 to...").

To move several tickets, `x` marks the current one, `Shift+j` and `Shift+k` extend the selection, then `m`. The
picker lists only the statuses every selected ticket can reach, matched by status name, and moves them all.
Dragging a card on the board and `Shift+Left` / `Shift+Right` still move by column.

## Starting a session

`s` opens the plan sheet (branch, repo, template) and starts on `Enter`; `S` skips the sheet. Kelta creates the
worktree, opens Claude and nvim, assigns the ticket to you and moves it to in progress (see `status_map`). Starting
again resumes, in whichever project the work item lives. The row then shows the pull request and its CI state once
one exists.

The session runs in a project you can pick: the sheet's first field is **Project** when several open projects have a
repository. Kelta picks, first match wins:

1. the project of the ticket's unfinished work item (Resume);
2. the project you chose (the sheet's Project field, `kelta-ctl start --project`, a trigger, a sub-task's parent work
   item);
3. the first route of the ticket's source matching its component, label or type, with the route's repository;
4. the project you last used for a ticket of that source;
5. the source's project, or its first one when it feeds several;
6. the active project.

When the pick is only a guess (5 with several projects, or 6), the sheet always shows (even with `S`), names the
source and remembers your pick for its next ticket. Switching project re-plans the branch, worktree and side effects
for it, and a route never overrides it. Batch start has one Project field: "Usual project for each ticket" starts a
routed ticket in its route's project and skips a ticket with no usual project (start it with `s`), or pick one project
for all.

## Your workflow

Statuses are shown as the tracker names them; Kelta only uses the category (to do, in progress, in review, done)
to order and colour them.

- **Kanban**: Mine, group by Status or Flow. Pull the next ticket with `m`, start it with `s`.
- **Scrum / Scrumban**: `f` `s` for the current sprint (or current iteration on the source); Team grouped by Assignee for stand-up, or by Sprint to see the carry-over.
- **Shape Up**: add the cycle's project or label as the source; custom states (Betting, Building) show as they are.
- **Support triage**: Unassigned, group by Status; `a` takes a ticket, `m` routes it.

## Tickets from any Claude session

Kelta's MCP server gives Claude its ticket tools. Sessions Kelta starts always get them; to use them from a
`claude` you start yourself (in a terminal, or a subagent of one), install Kelta in Claude Code:

- At launch, when Kelta finds Claude Code (`claude.binary` in your login PATH) and the `kelta` MCP server is not
  registered, a toast asks "Claude Code detected. Let your Claude sessions use Kelta's ticket tools?".
  **Install** registers it at user scope (`claude mcp add --scope user --transport http kelta
  http://127.0.0.1:<port>/mcp` with a token of its own), so the next `claude`, in any directory, lists the `kelta`
  tools. Dismissing the toast asks again at the next launch; **Never** stops asking (`claude.mcp_offer = false`).
- Settings > Claude shows whether it is installed, with **Install** / **Remove**. Remove unregisters it and revokes
  its token.
- When Kelta's port changes, Kelta updates the registration at launch without asking. Kelta must be running for
  the tools to answer.

The tools work on any ticket of your Kelta projects by key:

| Tool | Does |
|---|---|
| `list_tickets` | the tickets of every open project (or `project`, `view`), filtered by `status`, `assignee`, `label`; from Kelta's cache, `refresh` to fetch again. Parallel agents share one fetch |
| `get_ticket_by_key` | description, comments, links, status and the description's revision |
| `transition_ticket_by_key` | move to a status, transition or category (`in_progress`, `done`...) |
| `comment_ticket_by_key` | add a Markdown comment |
| `update_ticket_body` | replace the description (e.g. tick acceptance boxes); fails without writing when it changed since the revision you read (GitHub) |
| `close_ticket` | close, with an optional comment first (GitHub issues) |
| `list_review_requests` | pull requests awaiting your review |

`claude.mcp_board_writes = false` keeps the board tools read-only (`list_tickets`, `get_ticket_by_key`). A ticket
the lists do not show needs its `account` (the tracker account id) besides its key.

# Working with tickets

The Tickets pane is a list, not a board: one row per ticket, grouped on Mine by what you can start next
([To start](#grouping-and-sorting)) and elsewhere by the tracker's status (what the rest of the team sees), with the
keyboard doing most of the work. Each row carries the Queue's stage track, from Ticket to Deploy. A
reading pane on the right follows the selection and opens on where the ticket stands. Board mode stays available
with the list/board toggle.

**Tickets** in the sidebar (`Cmd+Shift+B`) shows this list across every source, outside any project: Mine is the
default, and its tab, source, grouping, sort and person come back after a restart. **All projects** in the top bar
narrows it to one project. Opening a ticket, its PR or its work from there switches to the ticket's project. The
board needs one project's columns: palette **Open project board** opens Tickets scoped to the active project, as a
board.

## Sources

A source is a board tickets come from: a Jira board or filter, a Redmine project or query, a GitHub repo or
Projects v2 board, a GitLab project, a Linear team, a Gitea repo. A source can hold several views, from different
accounts, so one board can show Jira and GitHub tickets together.

- Sources live in your global settings (Settings > Sources), not in each project: one `[[sources]]` entry holds
  the account, the views, the columns and the steps of its tickets, and lists the projects it feeds (none is fine). A
  board shared by several projects is set up once. Settings > Projects shows which sources feed each project.
  Boards from older versions moved out of the project files on first launch (a copy of each project file is kept
  in `projects/.trash/`).
- The Tickets view lists every source by default: under All projects it shows all of them, also those feeding no
  project; scoped to a project it shows the sources that feed it (first page of each, duplicates removed). **Load
  more** pages every source that has more. Each row shows its source when several are listed, and Group by Source
  works everywhere.
- `f` `v` (or the source button) narrows the list to one source; a source with several views lists them indented
  under it, to narrow to one view. The pane remembers the choice. With a single source that has a single view
  there is nothing to choose: no button, and `f` `v` does nothing.
- Board mode needs one source (the list narrowed to one, or a scope with only one): it uses that source's
  columns. Otherwise the List/Board switch is hidden.
- The palette and the ticket pickers search the trackers themselves (Jira `text ~`, Linear title and description,
  GitHub / GitLab / Gitea search, Redmine subject), so they find tickets past the first page. A tracker that cannot
  search matches the loaded first page instead; a typed key is also looked up directly.
- With no source yet, Tickets and the Queue say "Add a ticket source: pick a board". **Pick a board** (also "Add
  source..." in the `f` `v` menu and Settings > Sources) opens a sheet: choose the account, type to search, press
  `Enter` on a result. It creates a source labelled after the board, feeding the project you opened it from; the
  toast's action opens Settings > Sources to choose its projects. **Add view...** on a source adds a view to it.
  Providers that cannot list sources tell you so; edit the TOML instead ([views schema](../SETTINGS.md)).

### Per-source settings

Settings > Sources has one card per source. Besides the account, views, columns and projects:

- **Jira board and statuses.** Under each Jira view, pick a board: its filter narrows the view (ANDed with the
  view's JQL) and its columns become the view's columns. "Any board" turns it off when the view has a query of
  its own. **Statuses** lists only the ones you switch on; none on lists every status the query returns.
- **Status colours.** Give a status name a colour (the same picker as a project colour). A status without one
  keeps the colour of its category. The chip on rows and in the detail uses it.
- **Ticket fields.** Add a tracker field Kelta has no column for (a Jira custom field, a GitHub Projects v2
  field). Each field with a value shows as a `label: value` chip on the row, and as a line of the detail. An
  empty label shows the value alone. Settings lists the fields of the tracker to pick from; an id the tracker
  does not have shows nothing and is flagged *Unknown field*. A Jira field Kelta already shows (assignee,
  status, priority, sprint...) is not repeated as a chip.
- **Blocked.** Says how the tracker marks a ticket blocked (`blocked` in the TOML): `status`, `flag`,
  `relation` or `{ label = "<name>" }`. Unset, Jira uses its flag, Linear a "blocked by" relation and the others
  the status name. See [blocked tickets](#blocked-tickets).

## Mine, Unassigned, Team

Keys `1`, `2`, `3` switch the tabs; the pane remembers the choice. Counts show once loaded. Team is every open
ticket of your sources; group it by Assignee (`Shift+g`) for a per-person view.

The person menu next to the tabs narrows the list to one team person. Kelta builds the team from tickets and pull
requests it already loaded (assignees, reporters, PR authors and requested reviewers), and joins a tracker user to a
code host user only when it can prove they are the same: you, the same login on the same host, or the same email.
Otherwise the person stays apart and is marked "(tracker only)". Choosing a person switches to Team and is remembered
with the pane; "Any person" clears it. Picking a tab clears it too.

A row whose pull request waits on someone shows `waiting on Anna · 26h` (`+2` when several; hover for who and why)
next to the PR chip. Pull requests and the Queue show the same chip. A ticket that only waits on its assignee shows no chip:
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

A letter means the same in the Tickets list, the Queue and Pull requests: `Enter` the next step, `s` work on it, `v` view
changes, `o` open on the host, `m` move the ticket, `c` comment, `b` mark for a batch start, `Space` show or hide the
side panel, `/` filter, `?` every key, `Cmd+Shift+R` refresh.

| Key | Action |
|---|---|
| `j` `k` | move the selection |
| `Enter` | the next step, as in the Queue: a ticket with work runs its phase's next action, another starts work (the plan sheet); on a group header, expand or collapse |
| `g` | open the detail: focus the detail column, or its own pane in a narrow pane |
| `Shift+Enter` | open the detail as its own pane |
| `Space` | show or hide the detail column |
| `Esc` | from the detail column back to the list; in the list, clear the multi-selection |
| `/` | filter |
| `1` `2` `3` | Mine, Unassigned, Team |
| `f` `v` | choose sources |
| `Shift+g` | cycle the grouping (To start, Status, Flow, Priority, Sprint, Assignee, Source, Parent, None) |
| `f` `s` | only the current sprint (press again to clear) |
| `x` | add or remove the ticket from the selection |
| `b` | mark the ticket for a batch start, in the list and on the board (`Cmd+Enter` starts the marked tickets) |
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
| `b` (in the detail) | block or unblock the ticket ( [blocked tickets](#blocked-tickets)) |
| `t` | put a local tag on the ticket, or take it off ([tags](#local-tags)) |
| `w` | run a workflow on the ticket, in the background ([workflows](claude-code.md#workflows)) |
| `y` | copy the branch name (in the detail) |
| `o` | open in the tracker |
| `Cmd+Shift+R` (`Ctrl+Shift+R` on Linux) | refresh (outside a list or detail, it opens Pull requests) |
| `Cmd+.` (`Ctrl+.` on Linux) | the selected ticket's actions in a menu, its next step first (`Enter` runs it); in the detail, its More menu |

## Grouping and sorting

`Shift+g` cycles the grouping; the pane remembers it, and the sort, with its other settings. The **View** menu
in the toolbar holds the grouping, the sort, the person filter and the Current sprint toggle; a dot on it says a
person or the sprint is narrowing the list. `?` lists every key of the pane.

| Group by | Groups |
|---|---|
| To start (default on Mine) | **Up next**: the tickets of your Next up list in the Queue, in your order. **Ready to start**: no work in Kelta yet, whatever the tracker status. **In the queue**: started in Kelta; the rows are dimmed but keep their stage track, their next step lives in the Queue. **Blocked**: [blocked](#blocked-tickets), or the status name says blocked or on hold. **Backlog**: backlog, triage or icebox statuses, and statuses Kelta cannot place. **Done in the last 7 days**: collapsed; older ones are left out. |
| Status (default elsewhere) | the tracker's own status names, in the order of the source's board columns, Done collapsed |
| Flow | **Doing**: work item active, or an in-progress status. **Waiting**: Claude needs you, a review is requested or required, CI is red, the ticket is [blocked](#blocked-tickets), or the status name says blocked or on hold. **Ready**: to do and not started. **Backlog**: backlog, triage or icebox statuses, and statuses Kelta cannot place. **Done**: finished in the last 7 days (a source with no status filter also fetches its closed tickets for this); older ones are left out. |
| Priority | the provider's priorities, highest first, then no priority |
| Sprint | active sprints, other sprints, then no sprint |
| Assignee, Source, None | as named |
| Parent | one group per epic or parent issue (Jira parent, GitHub parent issue, GitLab epic or parent work item, Linear parent issue else project, Redmine parent issue), its key and title as header; tickets without a parent last. Outside this grouping a row shows its epic's name under the title, and the detail shows it beside the key (`Enter` on it opens it). |

Within a group the sort is Priority (then most recently updated), Updated, Age in status (longest first) or Key.
Each ticket carries the provider's priority, how long it has been in its status, its sprint, estimate and due date.
GitHub and Gitea have no native priority: they rank from `priority:` style labels, and GitHub reads priority, sprint and estimate from Projects v2 fields.

- **In status**: the last column says how many days the ticket has been in its status; past 14 days it stands out in
  the warning colour. `—` when the tracker does not date the status change, or the ticket is done. The last-update
  time is in the row's tooltip.
- **WIP limit**: when more tickets are in Doing than `tickets.wip_limit` (default 3, [Settings](../SETTINGS.md)) the
  group header says so in the warning colour. It only warns; nothing is blocked.
- **Sprint**: the row's tooltip names the sprint unless you group by sprint. `f` then `s` keeps the active sprint only (a quick
  filter, separate from the per-source current iteration below).

## The row

Key, then the title on up to two lines with the epic's name and the pull request's state under it (`#2291 checks
failed`, `No PR found`), then the stage track and the days in status. The status is the group's, and the actions
live in the reading pane: rows carry no buttons.

The stage track is the Queue's: Ticket, Claude, PR, Review, Merge, Deploy. The line runs to the furthest stage reached,
the mark sits where the ticket is now and its shape says whose move it is (filled: yours, ring: waiting on others,
diamond: something broke, a small dot once done). It follows the ticket's work item as the Queue does, else its linked pull
request. A ticket in a status its source lists as waiting for a deploy, with no branch or pull request found, gets a
dotted line labelled "per the tracker": Kelta did not see the stages before. Hover the row for where it stands.

`Enter` on a ticket with work under way runs the phase's next action, as `Enter` does in the Queue (the row's
button and `?`'s key panel name it); `g` opens the reading pane instead.

## The reading pane

Wider than about 720px (down to a 900px window), the pane splits: the list on the left, the selected ticket on the
right. `Space` hides or shows it, `g` moves the keys into it and `Esc` gives them back. `Shift+Enter` opens it in
its own pane, which is also what `g` does in a narrow pane. When the filter hides the selected ticket, the first match
is selected, so the pane never shows a hidden row.

From top to bottom: key, epic and title; the status (a button: it opens the status picker), type and points, and one
line on the situation (*Waiting for a deploy. Nothing for you to do.*); the action bar, and under it one sentence on
what Start work will do, from the plan the sheet opens on (*Start work creates branch feat/SHOP-142-rate-limit-login in
api, moves the ticket to in progress, assigns it to you, then briefs Claude next to Neovim.*), or for a ticket with
work, its stage and next step (*PR stage. Next: Push.*); the latest comment, then what
the spec still leaves open (a section headed Open questions, Missing, To clarify, Questions or TBD); the pull
requests; the description as sections you can fold, one per heading (a raw link to a ticket reads as a chip with its
key and title, acceptance criteria written Given / When / Then are laid out as such, task lists are shown, not
editable); the fields that have a value (assignee, priority, sprint, due date, the source's fields, labels, last
update); and the last 20 comments with a box to add one (`Cmd+Enter` posts).

The action bar shows Start or Resume work `s`, always the filled button (when it is blocked it says why), then Move `m` and
Comment `c`; Open PR `p` joins them only when the ticket has a pull request. More (`Cmd+.`, `Ctrl+.` on Linux) holds
Assign to me `a` (not on a ticket already yours), Unassign `A`, Block ticket `b`, Refine with Claude `r`, Run workflow
`w`, Copy branch `y`, Open in browser `o` and Tags `t`. Hover a button for its key; every key works without the bar. An
action the tracker cannot do (a tracker without assignment or comments), or that has nothing to act on (no branch yet),
stays visible but dimmed; hover it or press its key to read why.

## Pull requests

A row names the pull request linked to the ticket under its title, with its number (`#12`, `!12` on GitLab) and
state: draft, review required, approved, changes requested, merged, closed, or checks failed (in the danger colour).
A ticket links to the pull requests of its work item and to any pull request in your
review lists whose branch or title carries the ticket key. The detail lists each with its title, state, CI,
review state and branch. `p` opens it in Kelta's review detail when its repository is bound to a code host
account, else in the browser; `P` always opens the browser. With several pull requests a small menu asks which.

## Blocked tickets

A ticket can be marked blocked without changing its status. A blocked ticket shows a Blocked chip, goes to
Waiting in the Flow grouping and sorts last in its group. In the Queue it leaves Next up, New for you and Up next for the Blocked
section, with its work items unless they need you (Claude needs you, Ready for review, Fix); unblocked, it comes
back on the next refresh.

`b` in the detail (or **Block ticket** under More) blocks or unblocks. What it does depends on the source's
`blocked` setting:

| Setting | Marker | Block / Unblock |
|---|---|---|
| `flag` (Jira default) | the Jira flag | sets or clears the flag |
| `relation` (Linear default) | a "blocked by" an open issue | off: set it in Linear |
| `status` (default elsewhere) | a status named blocked, on hold, waiting or impediment | moves to a blocked status, and back to in progress (else to do) |
| `{ label = "name" }` | a label | adds or removes the label (Jira, GitHub, GitLab, Gitea); off on Linear and Redmine |

When Kelta cannot set the marker, the action stays dimmed and its tooltip says why. A status name that reads
blocked puts the ticket in Waiting whatever the setting.

## Local tags

Tags are reminders only you see, on tickets and on pull requests. They stay in Kelta: nothing is sent to the
tracker or the code host.

- Create them in Settings > Tags: a name and a colour. Renaming keeps the tag on its items; deleting removes it
  everywhere, after a confirmation.
- `t` on a ticket (or **Tags** under More) opens a menu to tick the tags it carries. With no tag yet it takes
  you to Settings > Tags.
- A tag shows as a chip on the row, in the detail and in the Queue. The toolbar has one chip per tag in the list, with its count; click it to
  keep only those tickets, click again to clear.
- Pull requests has the same `t` key and filter ([pull requests](reviews.md#local-tags)). The MCP tool `list_tickets` takes
  a `tag` argument.

## Moving status

`m` opens the status picker: the ticket's workflow as a line of the tracker's own status names, then the
transitions it allows, numbered. Type to filter; press a digit to move at once (`m` `2`). If the tracker needs
more fields, Kelta asks for them or offers the browser; a refused move shows the tracker's message and Open in
browser. Only moves the tracker allows are listed.

The same picker opens from the status in the reading pane, the Story panel's Ticket section and the command palette ("Move SHOP-142
to...").

To move several tickets, `x` marks the current one, `Shift+j` and `Shift+k` extend the selection, then `m`. The
picker lists only the statuses every selected ticket can reach, matched by status name, and moves them all.
Dragging a card on the board and `Shift+Left` / `Shift+Right` still move by column.

## Starting a session

`s` opens the plan sheet and starts on `Enter`; `S` skips the sheet. The sheet shows the project, repository, base
branch and branch, Claude's model, effort, permission mode and prompt, the options below, and the side effects
(assign, move, comment, run setup). Kelta creates the worktree, opens Claude and nvim, assigns the ticket to you and
moves it to in progress (see `work.on_start` and `status_map`). Starting again resumes, in whichever project the
work item lives. The row then shows the pull request and its CI state once one exists.

A source route (or the session template) can declare **options** for the ticket, such as `--auto` or a mode flag.
The sheet shows them under the prompt: one value is a switch, several values are a choice (or none). The picks
replace `{options}` in the Claude prompt, or follow it when the prompt has no `{options}`. Kelta remembers the last
picks for that route or template and starts a sheet-less `S` with them. The TOML is in
[Settings](../SETTINGS.md).

No ticket? **New work item** (`Cmd+Shift+N`, `Ctrl+Shift+N` on Linux, or `kelta-ctl start --task "..."`) starts
Claude and nvim from a task text, on a `wip/` branch (`work.scratch_branch_template`).

When other work is still unfinished (work items, or your tickets in progress or in review), starting a new ticket
first lists it, closest to done first: pick one to go finish it, or **Start anyway** (`Enter`) for the plan.
**Don't ask again** turns it off (`work.finish_first`).

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

- **Kanban**: Mine, grouped by To start, or by Status for the stand-up. Pull the next ticket with `m`, start it with `s`.
- **Scrum / Scrumban**: `f` `s` for the current sprint (or current iteration on the source); Team grouped by Assignee for stand-up, or by Sprint to see the carry-over.
- **Shape Up**: add the cycle's project or label as the source; custom states (Betting, Building) show as they are.
- **Support triage**: Unassigned, group by Status; `a` takes a ticket, `m` routes it.
- **Hand a ticket to a workflow**: `w` lists your workflows and runs one on the ticket in the background
  ([workflows](claude-code.md#workflows)). The row shows the workflow and its phase (`kelta-ticket-pass · Starting`)
  as it reports progress; the toast's **Open** shows the session and **Peek** its last lines. A ticket someone is
  already working on is refused.

## Tickets from any Claude session

Kelta's MCP server gives Claude its ticket tools. Sessions Kelta starts always get them; to use them from a
`claude` you start yourself (in a terminal, or a subagent of one), install Kelta in Claude Code:

- At launch, when Kelta finds Claude Code (`claude.binary` in your login PATH) and the `kelta` MCP server is not
  registered, a toast asks "Claude Code detected. Let your Claude sessions use Kelta's ticket tools?".
  **Install** registers it at user scope (the `kelta` entry of `mcpServers` in `~/.claude.json`, pointing at
  `http://127.0.0.1:<port>/mcp` with a token of its own), so the next `claude`, in any directory, lists the `kelta`
  tools. Dismissing the toast asks again at the next launch; **Never** stops asking (`claude.mcp_offer = false`).
- Settings > Claude shows whether it is installed, with **Install** / **Remove**. Remove unregisters it and revokes
  its token.
- When Kelta's port changes, Kelta updates the registration at launch without asking. Kelta must be running for
  the tools to answer.

The tools work on any ticket of your Kelta projects by key:

| Tool | Does |
|---|---|
| `list_tickets` | the tickets of every open project (or `project`, `view`), filtered by `status`, `assignee`, `label`, your local `tag` or `free` (not blocked, not claimed or started); each row says `blocked` and its `work_item`; from Kelta's cache, `refresh` to fetch again. Parallel agents share one fetch |
| `get_ticket_by_key` | description, comments, links, status and the description's revision |
| `transition_ticket_by_key` | move to a status, transition or category (`in_progress`, `done`...) |
| `comment_ticket_by_key` | add a Markdown comment |
| `update_ticket_body` | replace the description (e.g. tick acceptance boxes); fails without writing when it changed since the revision you read (GitHub) |
| `close_ticket` | close, with an optional comment first (GitHub issues) |
| `move_ticket_step` | move to a step of its pipeline: forward runs that step's status move and comment, backward only corrects where Kelta has it |
| `run_workflow` | start a workflow in a background Claude tab: with `key` a ticket run (claims the ticket; a run already live on it comes back `running: true`), else the `global` workflow of `project`; refused `Busy` once live Claude sessions reach `claude.max_live` |
| `list_review_requests` | pull requests awaiting your review |

`claude.mcp_board_writes = false` keeps the board tools read-only (`list_tickets`, `get_ticket_by_key`) and turns off `run_workflow`. A ticket
the lists do not show needs its `account` (the tracker account id) besides its key.

Sessions Kelta starts get more tools (the linked ticket, the pull request, `ask`, `notify`...), and Claude can
set Kelta up: see [Claude](claude-code.md).

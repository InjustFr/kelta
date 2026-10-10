# Working with tickets

The Tickets pane is a list, not a board: one row per ticket, grouped by flow (what you are doing, what waits on
someone, what is ready), with the keyboard doing most of the work. A detail column on the right follows the
selection. Board mode stays available with the list/board toggle.

## Sources

A source is a place tickets come from: a Jira board or filter, a Redmine project or query, a GitHub repo or
Projects v2 board, a GitLab project, a Linear team, a Gitea repo. A project can have several, from different
accounts, so one project can show Jira and GitHub tickets together.

- The pane shows every source of the project by default (first page of each, duplicates removed).
- `v` opens the source menu: "All sources", one entry per source, and "Add source...".
- **Add source** (also Settings > Projects > Tracker) opens a sheet: choose the account, type to search, press
  `Enter` on a result. Providers that cannot list sources tell you so; edit the TOML instead
  ([views schema](../SETTINGS.md)).

## Mine, Unassigned, Anyone

Keys `1`, `2`, `3` switch the tabs; the pane remembers the choice. Counts show once loaded. For a team view use
Anyone and group by Assignee (`Shift+g`).

The person menu next to the tabs narrows the list to one assignee (it lists the assignees of the loaded tickets,
so it only knows people on the first page of each source). Choosing a person switches to Anyone and is remembered
with the pane; "Any person" clears it. Picking a tab clears it too.

## Current iteration

Each source can be limited to the current iteration (Settings > Projects > Tracker, or `current_iteration = true`):

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
| `1` `2` `3` | Mine, Unassigned, Anyone |
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
| `c` | comment |
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
you, To review, Changes requested, Checks failed, In review, Merged...): the state Now shows, from the same signals. With the
detail column hidden (or a narrow pane), `Enter` runs the phase's next action, as `Enter` does in Now; `g` opens
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
again resumes. The row then shows the pull request and its CI state once one exists.

## Your workflow

Statuses are shown as the tracker names them; Kelta only uses the category (to do, in progress, in review, done)
to order and colour them.

- **Kanban**: Mine, group by Status or Flow. Pull the next ticket with `m`, start it with `s`.
- **Scrum / Scrumban**: `f` `s` for the current sprint (or current iteration on the source); Anyone grouped by Assignee for stand-up, or by Sprint to see the carry-over.
- **Shape Up**: add the cycle's project or label as the source; custom states (Betting, Building) show as they are.
- **Support triage**: Unassigned, group by Status; `a` takes a ticket, `m` routes it.

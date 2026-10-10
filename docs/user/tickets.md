# Working with tickets

The Tickets pane is a list, not a board: one row per ticket, grouped by status, with the keyboard doing most of
the work. Board mode stays available with the list/board toggle.

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
Anyone and group by Assignee (`g`).

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
| `Enter` | open the detail (on a group header: expand or collapse) |
| `/` | filter |
| `1` `2` `3` | Mine, Unassigned, Anyone |
| `v` | choose sources |
| `g` | group by Status, Assignee, Source, None |
| `m` | move status |
| `s` | start or resume work (shows the plan sheet) |
| `S` | start work without the sheet |
| `a` `A` | assign to me, unassign |
| `c` | comment |
| `o` | open in the tracker |
| `R` | refresh |

## Moving status

`m` shows the ticket's workflow as a line of the tracker's own status names, then the transitions it allows,
numbered. Press the digit: `m` `2` moves it. If the tracker needs more fields, Kelta asks for them or offers the
browser. Only moves the tracker allows are listed.

## Starting a session

`s` opens the plan sheet (branch, repo, template) and starts on `Enter`; `S` skips the sheet. Kelta creates the
worktree, opens Claude and nvim, assigns the ticket to you and moves it to in progress (see `status_map`). Starting
again resumes. The row then shows the pull request and its CI state once one exists.

## Your workflow

Statuses are shown as the tracker names them; Kelta only uses the category (to do, in progress, in review, done)
to order and colour them.

- **Kanban**: Mine, group by Status. Pull the next ticket with `m`, start it with `s`.
- **Scrum / Scrumban**: turn on current iteration for the sprint source; Anyone grouped by Assignee for stand-up.
- **Shape Up**: add the cycle's project or label as the source; custom states (Betting, Building) show as they are.
- **Support triage**: Unassigned, group by Status; `a` takes a ticket, `m` routes it.

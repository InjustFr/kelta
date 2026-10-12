# Reviews

**Reviews** on the rail (`Cmd+Shift+R`, `Ctrl+Shift+R` on Linux) lists pull requests (merge requests on GitLab)
across every open project, next to your tickets. A project's own Reviews tab shows only that project. Pull
requests come from the code host account bound to each repository: GitHub, GitLab, Bitbucket Cloud and
Gitea/Forgejo ([Bitbucket](bitbucket.md), [Gitea](gitea.md)).

## Tabs

`1`, `2` and `3` switch the tabs.

| Tab | Lists |
|---|---|
| For me | review requests addressed to you, or to a team you are in (`reviews.include_team_requests`). Drafts are left out unless `reviews.include_drafts` is on |
| Team | pull requests of the repositories your ticket sources reach. In a project's tab it is offered only when a source feeds that project. `G` groups the rows by their linked ticket |
| Mine | your own open pull requests and those assigned to you (GitHub, GitLab, Gitea; Bitbucket has no assignees), drafts included. The ones waiting on you come first: changes requested, a thread waiting for your answer, or red CI |

Each row shows the number and title, the CI lamp, a draft or new badge, the review state, the linked tickets (a
ticket key in the branch or title, `reviews.ticket_key_regex`), the repository, author, size and age. A row
whose pull request waits on someone shows `waiting on Anna · 26h` (`+2` for several); it turns amber after
`reviews.sla_hours` (24).

Palette **Open tickets and reviews side by side** opens a tab in the active project with your tickets on the
left and For me over Mine on the right; run it again to come back to that tab.

A detail column on the right follows the selection, as in [Tickets](tickets.md#the-detail-column): `Space` hides
or shows it, `Enter` moves the keys into it, `Esc` gives them back, `Shift+Enter` opens it as its own pane.

## Keys

In the list:

| Key | Action |
|---|---|
| `j` `k` | move the selection |
| `Enter` | focus the detail |
| `Shift+Enter` | open the detail as its own pane |
| `Space` | show or hide the detail column |
| `/` | filter by title, repository, number, author, branch or ticket |
| `1` `2` `3` | For me, Team, Mine |
| `G` | on Team, group by ticket |
| `s` | review locally |
| `o` | open on the code host |
| `n` | on Mine, nudge the reviewers (below) |
| `M` | on Mine, merge when ready (needs a work item for the pull request) |
| `d` | on Mine, mark ready for review, or convert back to a draft |
| `t` | put a local tag on the pull request |
| `w` | run a workflow on the pull request |
| `R` | refresh |

In the detail of someone else's pull request: `a` approve, `c` request changes, `m` comment, `s` review
locally, `o` open on the host, `t` tags, `w` workflow, `R` refresh. On your own pull request the detail offers
no approval or comment: `f` fixes it with Claude, `g` goes to its work item and `d` toggles the draft state
(below).

Approving is pinned to the commit you are looking at. If the head moved since you opened the pull request,
nothing is approved and the pane asks you to refresh.

## Review locally

**Review locally** (`s`) checks the pull request out in a worktree and opens the `review` session template:
Claude with the review profile (plan mode: it does not edit) and a prompt from `claude.prompt_templates.review`,
nvim, and a diff summary. Only the source branch is fetched, so a pull request from a fork cannot be reviewed
locally on Bitbucket.

## Your own pull requests

- **Fix with Claude** (`f` in the detail) opens a sheet with the unresolved review threads, review summaries and
  failed checks, and an editable prompt. `Cmd+Enter` sends them to the work item's Claude, which resumes its
  conversation (when Claude is working, it gets them when it stops).
- **Draft.** `d` marks a draft ready for review, or converts a pull request back to a draft. Marking ready asks
  first: the code host then asks the required reviewers to review it. The same action is in the work menu
  (**Mark ready / Convert to draft**) and in the Now row of a draft. It works on GitHub, GitLab, Bitbucket and
  Gitea; a plugin code host may not support it, and the action says so.
- **Nudge.** `n` offers **Re-request review** (asks the reviewers again) and **Comment ping** (posts
  `reviews.nudge_template`, `{reviewers} friendly ping: this is waiting on your review.` by default). Each
  works once per 24 hours per pull request.
- **Merge when ready** (`M`) offers `work.pr.merge_method` (squash by default).

## Local tags

`t` puts one of your tags on a pull request, and the toolbar has one chip per tag to filter the list. Tags are
reminders kept in Kelta, created in Settings > Tags. See [local tags](tickets.md#local-tags).

## Workflows

`w` lists the `[[workflows]]` of your config with `on = "pr"` and runs one on the pull request in the
background ([workflows](claude-code.md#workflows)).

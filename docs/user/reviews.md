# Pull requests

**Pull requests** in the sidebar (`Cmd+Shift+R`, `Ctrl+Shift+R` on Linux) lists pull requests (merge requests on
GitLab) across every open project, next to your tickets. `All projects ▾` in the top bar narrows it to one
project. Pull requests come from the code host account bound to each repository: GitHub, GitLab, Bitbucket Cloud
and Gitea/Forgejo ([Bitbucket](bitbucket.md), [Gitea](gitea.md)).

## Sections

One list, in sections by what you can do about each pull request:

| Section | Lists |
|---|---|
| Asked of you | review requests addressed to you, or to a team you are in (`reviews.include_team_requests`). One line when nobody asked. Drafts are left out unless `reviews.include_drafts` is on |
| Team, needs a reviewer | the open pull requests of the repositories your ticket sources reach that are neither yours nor asked of you, drafts left out. In a project's tab only when a source feeds that project |
| Team drafts | the team's drafts, folded until you open the section |
| Yours, as reviewers see them | your own open pull requests and those assigned to you (GitHub, GitLab, Gitea; Bitbucket has no assignees), drafts included |

A pull request whose base branch is another listed pull request's branch follows it, one step in under an elbow:
the list reads in merge order, and the row says "merges into #2287" (the base says "base of a stack of 3").

Each row shows the number and title, then the checks when they fail or run, the review state ("No review yet,
nobody requested", "1 approval, 2 reviewers still requested") or Draft, and the author. Its change map is a bar
as long as the pull request, linear up to 2k changed lines and cut past that, split into code, tests, config
and other by file path, with "1.3k lines, 49 files" under it (GitHub reads the first 100 files of a pull
request, GitLab the whole diff; Bitbucket and Gitea give no paths, so their bar is one block). The age is how
long the pull request has been open, amber past 14 days on the team's. Rows under Yours are dimmed: their next
step (fix, merge) waits for you in the Queue.

Palette **Open tickets and pull requests side by side** opens a tab in the active project with your tickets on
the left and Pull requests on the right; run it again to come back to that tab.

The selected pull request's brief sits beside the list (under it in a narrow window): title, author, how long
it has been open, checks, branch into base; **Review locally** (**Go to work item** on yours, outlined rather
than filled: the Queue holds that step) and the next actions, the rest behind **N more actions** (`.`), then one
sentence on what Review locally does; the review status with each reviewer's state; **Before
merging**, the description's section headed "Before merging", "Deploy", "Migrations" or "Rollout" (put one in
your pull request template); the change map, the description and the files grouped by area, biggest first.
`Enter` moves the keys into it, `Esc` gives them back, `Shift+Enter` opens it as its own pane.

## Keys

In the list:

| Key | Action |
|---|---|
| `j` `k` | move the selection |
| `Enter` | focus the detail |
| `Shift+Enter` | open the detail as its own pane |
| `Space` | focus the detail |
| `/` | filter by title, repository, number, author, branch or ticket |
| `s` | review locally |
| `v` | view the changes: someone else's pull request, the whole diff of its local review (review it locally first); yours, the changes since your last look |
| `c` | comment on someone else's pull request |
| `o` | open on the code host |
| `n` | on yours, nudge the reviewers (below) |
| `M` | on yours, merge when ready (needs a work item for the pull request) |
| `d` | on yours, mark ready for review, or convert back to a draft |
| `t` | put a local tag on the pull request |
| `w` | run a workflow on the pull request |
| `Cmd+Shift+R` (`Ctrl+Shift+R` on Linux) | refresh (outside a list or detail, it opens Reviews) |
| `Cmd+.` (`Ctrl+.` on Linux) | the selected pull request's actions in a menu, Open review first (`Enter` runs it); in the detail, its own actions |

In the detail of someone else's pull request: `a` approve, `c` comment, `Shift+C` request changes, `s` review
locally, `v` view the changes, `o` open on the host, `t` tags, `w` workflow, `Cmd+Shift+R` refresh, `.` every
action. A letter means the same here as in the Queue and Tickets (`c` comments everywhere). On your own pull request the detail offers
no approval or comment: `f` fixes it with Claude, `g` goes to its work item and `d` toggles the draft state
(below).

Approving is pinned to the commit you are looking at. If the head moved since you opened the pull request,
nothing is approved and the pane asks you to refresh.

## Review locally

**Review locally** (`s`) checks the pull request out in its own worktree and opens the `review` session
template: Claude with the review profile (plan mode: it does not edit) and a prompt from
`claude.prompt_templates.review`, next to the diff in nvim. Claude's line comments go into your pending review,
which `a`, `c` or `m` publishes with your decision. Only the source branch is fetched, so a pull request from a fork cannot be reviewed
locally on Bitbucket.

## Your own pull requests

- **Fix with Claude** (`f` in the detail) opens a sheet with the unresolved review threads, review summaries and
  failed checks, and an editable prompt. `Cmd+Enter` sends them to the work item's Claude, which resumes its
  conversation (when Claude is working, it gets them when it stops).
- **Draft.** `d` marks a draft ready for review, or converts a pull request back to a draft. Marking ready asks
  first: the code host then asks the required reviewers to review it. The same action is in the work menu
  (**Mark ready / Convert to draft**) and in the Queue row of a draft. It works on GitHub, GitLab, Bitbucket and
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

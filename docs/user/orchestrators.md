# Orchestrators

An orchestrator is a Claude that picks tickets, builds them and hands them off, with Kelta showing where each one
stands. Kelta has no orchestrator of its own: you write one as a slash command or a prompt, and it talks to Kelta
through MCP tools. This page is the whole contract. A working example is in
[`examples/orchestrator/`](../../examples/orchestrator/).

## Where an orchestrator runs

| Where | How it starts | Notes |
|---|---|---|
| A `⌘T` Claude | you type the slash command | The simplest: it is a Kelta session, so it has every tool below. |
| A `[[workflows]]` run | `w`, the palette, a trigger, or MCP `run_workflow` | `on = "ticket"` (the run claims the ticket), `"pr"` or `"global"` (no ticket: the orchestrator picks). See [Workflows](claude-code.md#workflows). |
| A cron job | `kelta-ctl emit custom.morning --project shop` | A trigger on `custom.morning` runs a `global` workflow in `shop`. Only `custom.*` events can be emitted. |

A `claude` you start in another terminal reaches the board, claim and `link_pr` tools once Kelta is installed in
Claude Code ([setup](tickets.md#tickets-from-any-claude-session)), but not `ask`, `notify` or `create_pr`: those
belong to sessions Kelta starts. `claude.mcp` must be on (it is by default); `move_ticket_step` and `run_workflow`
also need `claude.mcp_board_writes`.

## The contract, in order

Every tool name and field below is the one Kelta's MCP server lists. Errors start with their code
(`Conflict: …`, `InvalidArgument: …`, `Busy: …`), so a run can branch on it.

1. **Find work.** `list_tickets{free: true}` returns the tickets that are not blocked and have no unfinished work
   item, from Kelta's cache (first page of each list; add `project`, `view`, `status`, `assignee`, `label`, `tag`
   or `refresh`). Each row has `key`, `account`, `blocked` and `work_item`.
2. **Claim, once the worktree exists.** Make the worktree and branch yourself (`git worktree add`), then
   `claim_ticket{key, worktree, branch, run: {id, name}}`.
   - `worktree` must be a worktree of one of the project's repos, `branch` a valid branch name (default: the
     worktree's checkout). Kelta never creates, removes or deletes them.
   - `run.name` is shown on the ticket (`build-ticket · Implement`); `run.id` tells the lanes of one session apart.
   - `account` (and `project`) are needed only for a ticket the lists do not show.
   - A ticket with live work answers `Conflict: KEY is held by session … (run)`: pick another. A claim that is
     orphaned (its session is gone), blocked or failed is taken over.
   - The claim does not move the ticket on the board.
3. **Report each phase.** `report_progress{key, phase, state, note?, sha?}`. `phase` is free text, `state` is
   one of the following; an omitted `note` or `sha` keeps the last one.

   | `state` | Meaning | Where it shows |
   |---|---|---|
   | `running` | working | the ticket's row and detail (`build-ticket · Implement`), the item's lamp, the Queue's In flight, the sidebar, the claiming terminal's list of claims |
   | `blocked` | needs you; put the reason in `note` | amber lamp; Queue's Claude needs you with the note |
   | `failed` | gave up; `note` says why | red lamp; same Queue section |
   | `done` | handed over (step 6) | the run's phase row disappears; the item reads like any other (its PR, its review) |

   A claim answers only to the session that made it; if that session is gone, any caller's report takes the claim
   over. Reporting a state or phase that did not change publishes nothing.
4. **Ask and notify.** `ask{question, options, body_md?}` (1 to 9 options) shows buttons on the ticket row, in the
   Queue and in Peek, and returns at once: end your turn, the chosen option arrives as your next message.
   `notify{message}` sends a desktop notification. Both exist only on sessions Kelta starts.
5. **Open the PR.** `create_pr{title?, body?, draft?}` pushes the work branch and opens the PR of the session's
   work item, so it needs the session to be in the item (see Adoption). Without it, push and open the PR
   yourself, then `link_pr{key, pr, status?}` (`pr`: a URL, or a number only one of your open PRs has). Either
   enters the ticket's `pr` step, which runs the pipeline's transition and comment.
6. **Hand off**, one of two ways:
   - `report_progress state=done`: the run ends but the item stays open. It goes to review with its PR and
     carries the conversation that did the work (Resume, review feedback and Fix with Claude reach it).
   - `release_ticket{key, outcome: "merged" | "abandoned", sha?}`: the item is finished. Kelta leaves the worktree
     and branch alone and does not touch the ticket; `abandoned` leaves the worktree on disk. Use it when the
     run itself merged or gave up.

Both publish `run.state_changed` with `state = done` (a `release_ticket` of a claim already `done` publishes
nothing).

## Adoption

A claim can make the claiming session the item's Claude, so the item's tab, sidebar rows, `open_in_editor`,
`get_ticket` and review feedback all follow the conversation that did the work.

- The first claim adopts: a live Kelta Claude that is in no unfinished work item becomes the Claude of the item it
  claims, whatever started it (`⌘T`, a `pr` or `global` run, a trigger's run, a ticket run). The session keeps
  its cwd, also after a restart.
- A session is in at most one live item. A claim made while it is in one, and a re-claim of a ticket it holds,
  never adopt.
- A session that claims several tickets passes `adopt: false` on each, so its own conversation does not become the
  first ticket's Claude.
- The lanes of one session pass their own `run.id`: lanes with different ids conflict on a ticket, the same id
  re-claims. Without ids a session's re-claim of its own ticket is also accepted.
- A re-claim by the holder returns its item, never `Conflict`. Naming `worktree` or `branch` re-points the item;
  omitting them keeps its worktree, branch and base. It replaces the item's `run`, so repeat `run: {id, name}`.
  A ticket in no Kelta list can be re-claimed by `key` alone.
- A ticket run (`w`, a trigger or `run_workflow`) claims its ticket and its Claude is adopted already. Its command
  can claim again with the worktree it made; `args.json`'s `run` field (`{args}` in the prompt) is the run id to repeat.
- A ticket whose item is unfinished cannot be claimed by another session, also after `state=done`: end it with
  `release_ticket`, or finish the item, first.

## Moving the ticket

Claiming and reporting never move the ticket on the board. To move it so that the pipeline's transition and
comment run, use `move_ticket_step{key, account?, step}`. `step` is a step id of the ticket's pipeline
(default: `implementing`, `pr`, `review`, `merge`, `done`). Forward runs that step's transition and comment once;
backward is a correction that runs nothing; a failed transition keeps the step and says so, and moving to it
again retries. It needs an unfinished work item of the ticket, so call it before `release_ticket`. Kelta enters
`pr`, `review` and `merge` itself on the PR's signals ([FLOW §4.9](../FLOW.md)); `transition_ticket_by_key` is the
tool for a plain status move that skips the pipeline.

## Fan-out

One orchestrator, one run per ticket: `run_workflow{workflow, key, account?}`. Each call starts a Claude in its
own background tab; it claims the ticket under the run id `<workflow>-<session>` and is adopted by it. The reply
is `{session_id, name, running}`.

- `running: true` means the ticket already has a live run; nothing was started.
- `claude.max_live` (default 4, counting every live Claude, the orchestrator's included; `0` is no cap) is the
  limit: past it the call fails `Busy: N of M Claude slots in use` and nothing is queued. Stop or retry later,
  for example when a `run.state_changed` trigger fires. `w`, the palette and triggers are not capped.
- Parallel calls are counted one at a time, so a burst cannot overbook the slots.
- Without `key`, `run_workflow` starts the `global` workflow of `project` (default: the caller's), which is how
  an orchestrator starts a sub-orchestrator.
- A fan-out orchestrator claims nothing itself. If it also claims tickets to build them in its own session, it
  passes `adopt: false`.

## Trigger recipes

Triggers go in `config.toml` (`[[triggers]]`, [PLUGINS §3](../PLUGINS.md)). Events an orchestrator cares about:
`run.state_changed` (`payload.state`, `previous`, `phase`, `note`, `work`; once per change of a claim's state)
and `ticket.step` (`payload.step_id`).

```toml
# A run is blocked or failed: tell me once.
[[triggers]]
id = "run-needs-me"
on = "run.state_changed"
match = { "payload.state" = ["blocked", "failed"] }
do = [{ action = "notify", title = "{ticket.key}: run {payload.state}", body = "{payload.note}", urgency = "critical" }]

# Keep the board moving: when a run is done, start one more global run (put your own guard command first).
[[triggers]]
id = "next-on-done"
on = "run.state_changed"
match = { "payload.state" = "done" }
do = [{ action = "run_workflow", workflow = "fan-out" }]

# A ticket reached review: start your own `triage` ticket workflow on it (the ticket must hold no unfinished item).
[[triggers]]
id = "triage-on-review"
on = "ticket.step"
match = { "payload.step_id" = "review" }
do = [{ action = "run_workflow", workflow = "triage", ticket = "{ticket.key}" }]
```

A trigger is not capped by `claude.max_live`: a chain like `next-on-done` stops only when the global workflow
finds no free ticket, so make it do so. A `ticket` workflow started on a ticket whose last run reported `done`
fails `Conflict` while that item is unfinished; start a `global` one, or `release_ticket` first.

## An example

[`examples/orchestrator/`](../../examples/orchestrator/) has the `[[workflows]]` entries (`build-ticket` on a
ticket, `fan-out` global), four triggers (a cron event, run blocked or failed, run done, ticket in review) and two
slash commands: `/build-ticket` (picks a free ticket when given none, makes the worktree, claims, reports each
phase, opens the PR, hands off) and `/fan-out` (one `build-ticket` run per free ticket). The config is checked by a
unit test, so it parses.

See also [Tickets from any Claude session](tickets.md#tickets-from-any-claude-session), [Workflows](claude-code.md#workflows),
[FLOW §4.8](../FLOW.md) and [PLUGINS §3.2](../PLUGINS.md).

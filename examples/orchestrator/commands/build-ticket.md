---
description: Build one Kelta ticket in its own worktree and hand it off (no argument: pick a free one)
---

Arguments: `$ARGUMENTS`. Either empty, or `KEY ARGS_JSON` (a Kelta ticket run: its claim already exists and
`ARGS_JSON` is the path of a file whose `run` field is the run id).

1. Pick. With a key, use it. Without: read `echo $KELTA_PROJECT_ID $KELTA_SESSION_ID`, call
   `list_tickets{free: true, status: "todo", project: <KELTA_PROJECT_ID>}` (without `project` it lists every open
   project, and a ticket of another project cannot be claimed from this checkout), take the first row and keep its
   `key` and `account`. Nothing free: call `notify` and stop.
2. Worktree. In the project's main checkout run
   `git worktree add ../wt-<key slug> -b feat/<key slug> origin/main`.
3. Claim, once the worktree exists: `claim_ticket{key, account, worktree, branch, run: {id, name: "build-ticket"}}`.
   `id` is the `run` of `ARGS_JSON` for a ticket run (this re-points the claim Kelta made at the worktree; it
   replaces the whole run, so repeat `id` and `name`), else `"build-ticket-<KELTA_SESSION_ID>"`: unique per
   session and repeated on every re-claim (a fixed id would let another session's claim take this one over). On `Conflict`, remove the
   worktree and pick another ticket (or stop for a given key).
4. For each phase (Implement, Test, PR) call `report_progress{key, phase, state: "running"}` when it starts.
   Work in the worktree: read the ticket with `get_ticket_by_key`, implement, run the tests.
5. A decision only the user can take: `ask{question, options}`, then end your turn; the answer is your next
   message. Stuck: `report_progress{key, phase, state: "blocked", note}`, call `notify`, stop.
6. Push and open the PR with `create_pr{title, body}`. Kelta moves the ticket to its `pr` step.
7. Hand off: `report_progress{key, phase: "Hand-off", state: "done", sha}`. The item stays open for review and
   keeps this conversation.

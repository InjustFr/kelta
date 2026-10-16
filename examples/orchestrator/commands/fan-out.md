---
description: Start one build-ticket run per free Kelta ticket
---

Start at most `$ARGUMENTS` runs (3 when empty).

1. `list_tickets{free: true, status: "todo"}`.
2. For each of the first rows, call `run_workflow{workflow: "build-ticket", key, account}`. Do not claim the
   tickets: each run claims its own.
   - `running: true` in the reply: that ticket already has a live run, go on with the next.
   - `Busy: N of M Claude slots in use`: stop, nothing is queued.
3. Call `notify` with the keys started and the number of free tickets left.

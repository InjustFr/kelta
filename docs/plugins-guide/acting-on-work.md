# Act on a ticket, a PR or a branch

This chapter builds [`examples/plugins/branch-script`](../../examples/plugins/branch-script): a "Run script on
this branch" action on work items and tickets. It opens a screen that shows the branch, worktree, ticket and
PR, runs a script of your choice in the worktree, and keeps the last 10 runs. The reference for every field is
[PLUGINS.md](../PLUGINS.md) (sections 1, 3.1, 4, 5 and 7).

```
branch-script/
├─ kelta-plugin.toml
├─ settings.schema.json      # `script`: the command line to run (default `echo`)
├─ run.sh                    # the only program the plugin may run
└─ dist/index.html, dist/app.js, dist/style.css, dist/kelta-sdk.js
```

Try it: `kelta-ctl plugin install examples/plugins/branch-script`, open a work item, press ⌘. and pick "Run
script on this branch". Out of the box it runs `echo <branch> <worktree>`.

## 1. What Kelta passes to an action

An action is a list of `ActionDef` steps whose strings are templates. What they can use depends on where the
user ran it:

| Subject | Placeholders |
|---|---|
| Work item (work menu, ⌘.) | `{work.id}`, `{work.port}`, `{branch}`, `{base}`, `{worktree}`, `{project.*}`, `{repo.*}`, plus `{ticket.*}` when the item has a ticket |
| Ticket (ticket detail) | `{ticket.key\|title\|url\|provider}`, plus `{work.*}`, `{branch}`, `{worktree}` when the ticket has a work item |
| PR (review detail) | `{pr.url\|number\|head\|base\|title\|repo}` |

A placeholder without a value expands to an empty string (`{a|b}` falls back to `b`), so a ticket that has no
work item yet gets `work = ""`. A placeholder Kelta does not know is an error when the plugin loads.

The run uses the ticket's work item's project, else the ticket's or PR's first project, the focused pane's,
then the active project. A work action runs in the work item's project. An action that opens a screen, a tool
or a session shows that project first.

## 2. Adding actions to tickets, PRs and work items

```toml
[[contributes.work_actions]]
id = "run-on-work"
title = "Run script on this branch"
do = [{ action = "open_screen", screen = "run", params = { work = "{work.id}", ticket = "{ticket.key}" } }]

[[contributes.ticket_actions]]
id = "run-on-ticket"
title = "Run script on this branch"
do = [{ action = "open_screen", screen = "run", params = { work = "{work.id}", ticket = "{ticket.key}" } }]
```

Work actions come after a separator at the end of the work menu (⌘.); ticket actions after the built-in
buttons of the ticket detail (one button, or a `Plugins ▾` menu when there are several).
`[[contributes.review_actions]]` does the same on the review detail with `{pr.*}`. Each one is also a palette
command, `branch-script/run-on-work`, offered only while that kind of subject is focused.

## 3. Opening your screen for that subject

```toml
[[contributes.screens]]
id = "run"
title = "Branch script"
entry = "dist/index.html"
scope = "project"
placement = ["tab", "pane"]
```

`open_screen` expands every string in `params` and hands the result to the screen as `kelta.params`. When the
same screen is shown again for another subject, the host pushes the new params:

```js
const kelta = await connect();
await load(kelta); // reads kelta.params: { work: "wi-…", ticket: "SHOP-142" }
kelta.onParams(() => void load(kelta));
```

The `open_screen` action needs `ui.open`. Every screen also gets an `Open Branch script` palette entry: the user
opens it, so it needs no grant at all, and the screen gets no params. Handle that case (the example says
"Open this screen from a work item or a ticket").

## 4. Reading tickets and work items

A work item holds the branch, worktree, base, linked ticket and PR URL. The example reads it by id, or finds
the ticket's item in the project's list:

```js
async function findWork(kelta, params) {
  if (params?.work) return kelta.work.get(params.work);
  if (params?.ticket) {
    const items = await kelta.work.list();
    return items.find((w) => w.ticket?.key === params.ticket) ?? null;
  }
  return null;
}
```

`work.*` needs `work.read`, and only sees the screen's project (a global screen can pass `project: null`).
Tickets and PRs have their own methods, not used by the example:

```js
const page = await kelta.tickets.list(); // Kelta's Tickets list: items carry work_item_id and prs (tickets.read)
const detail = await kelta.tickets.get(ticketRef); // with prs and caps (tickets.read)
const mine = await kelta.reviews.list({ kind: 'authored' }); // your own PRs (prs.read)
```

## 5. Running a script and showing its result

```js
const MAX_RUNS = 10;
const w = item; // `load()` can replace `item` mid-run: keep the one this run started on
const { script = 'echo' } = await kelta.settings.get();
const res = await kelta.exec('./run.sh', [script, w.branch, w.worktree], { cwd: w.worktree });
text($('status'), `Exit ${res.code}`);
const output = (res.stdout + res.stderr).slice(-2000); // a kv value is capped at 64 KiB
const runs = (await kelta.kv.get('runs')) ?? [];
runs.unshift({ at: new Date().toLocaleString(), branch: w.branch, code: res.code, output });
await kelta.kv.set('runs', runs.slice(0, MAX_RUNS));
```

`exec.run` is headless and has no shell: `command` is an argv[0], and `args` are passed as they are. A path
such as `./run.sh` must be a file inside the plugin directory; a bare name is looked up on the login shell's
`PATH`, so a Kelta launched from the Dock still finds Homebrew tools. `cwd` defaults to the plugin directory;
pass the worktree. A non-zero exit is a result (`{code, stdout, stderr}`), not an error; stdout and stderr are
capped at 64 KiB each, and the call fails with `timeout` after `timeout_ms` (default 60 s). `run.sh` runs the
`script` setting with the branch and worktree as arguments, and falls back to `echo` when the setting is
empty (`settings.get` then returns `""`, which the JS default does not replace):

```sh
script=${1:-echo}
shift
exec sh -c "$script \"\$@\"" run.sh "$@"
```

`kv.*` values are capped at 64 KiB, so the example keeps the last 2000 characters of each run. The example
also keeps a `running` flag so that `load()`, called again on `work.updated`, leaves the Run button disabled
until the current run ends.

To show a program the user watches or types into (a deploy log, a REPL), open it as a tool instead: declare a
`kind = "pty"` tool and run `{ action = "open_tool", tool = "<plugin>/<id>" }` or `kelta.tools.open(id)`. It
runs in a terminal pane and needs `sessions.spawn`.

## 6. Reacting to events

```js
kelta.events.on('work.updated', (payload) => {
  if (payload?.work?.id === item?.id) void load(kelta);
});
kelta.events.on('pr.merged', () => void load(kelta));
```

Each name needs an `events:<glob>` permission covering it. `work.updated` carries `{work}` on every item or
step change; `pr.merged` carries `{review, linked_tickets}`. The full list is PLUGINS.md section 6.

## 7. The permissions to request

```toml
permissions = ["work.read", "exec:./run.sh", "storage", "events:work.updated", "events:pr.merged", "ui.open"]
```

| Permission | Why |
|---|---|
| `work.read` | `work.get` / `work.list`: they expose branch names and worktree paths |
| `exec:./run.sh` | `exec.run ./run.sh` (a path is matched exactly as written) |
| `storage` | `kv.get` / `kv.set` for the last runs |
| `events:work.updated`, `events:pr.merged` | the two subscriptions, by exact name rather than `events:*` |
| `ui.open` | the actions' `open_screen` step (the `Open Branch script` palette entry needs none) |

Remove one and the matching call fails with `permission_denied`, naming the missing permission. The
integration test `crates/kelta-plugins/tests/it/branch_script.rs` asserts this exact list.

## Calling a protected API

To call a token-protected service (a QA deploy endpoint, say), mark the token setting secret and name it in
`secret_headers`. Kelta resolves the secret and adds the header; the plugin never sees the value.

```json
{
  "type": "object",
  "properties": {
    "qa_token": { "type": "string", "title": "QA token", "x-kelta-secret": true }
  }
}
```

```js
const res = await kelta.fetch('https://qa.example.com/deploy', {
  method: 'POST',
  body: JSON.stringify({ branch: item.branch }),
  secret_headers: { Authorization: { setting: 'qa_token', format: 'Bearer {secret}' } },
});
```

This needs `net:qa.example.com`. The user picks the SecretRef (`keyring:`, `env:`, `command:`) in Settings →
Plugins; while it is unset the call fails with `needs_auth`. The `http` action takes the same `secret_headers`
(PLUGINS.md section 5.1).

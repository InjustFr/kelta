# Claude

How Kelta works with Claude Code. Kelta needs `claude` in your login PATH; Settings > Diagnostics checks it.

## Set up with Claude

On a fresh install the welcome sheet offers **Set up with Claude**; the command palette has the same action at
any time. Kelta registers its MCP server in Claude Code if that is not done yet, then opens a Claude session in
Home with a guided script. Claude looks at your code folders, remotes and CLIs (`gh`, `glab`, `nvim`), then asks
what it needs: code host and tracker accounts, which repositories are projects, a ticket source, your editor.
It writes settings through the same validation as the Settings pane.

Tokens never go through Claude. An account's `secret` is a reference (`gh-cli`, `glab-cli`, `env:VAR`,
`keyring:name`...). When you have none, Claude asks Kelta to ask you, and you type the token in Kelta, which
stores it in the keyring. Claude then tests the account and summarises what it did.

You can do all of it by hand: see [getting started](getting-started.md#first-launch).

## Status on every pane

Kelta starts Claude with hooks that report its state: working, needs you, done. The state shows on the pane, its
tab, the project in the rail and in Now. If Kelta cannot tell (the hooks are not installed in that session), the
pane shows **Live status off** with a **Fix** button that opens Diagnostics.

## Tickets from any Claude session

Kelta's MCP server gives Claude its ticket tools, also in a `claude` you start in another terminal: see
[tickets from any Claude session](tickets.md#tickets-from-any-claude-session).

## Questions from Claude

In a session Kelta starts, Claude has an `ask` tool: a question with up to nine fixed answers and an optional
Markdown body. Kelta shows one button per answer on the session's row in Now and in Peek (`Cmd+Shift+Y`,
`Ctrl+Shift+Y` on Linux). A click types the answer into the session and presses Enter, so Claude gets it as your
next message. Typing something else works too. While the session shows one of Claude's own prompts (a permission
request), the buttons are off until you answer that prompt, so the click cannot answer it by mistake.

The session counts as needing you while a question is open, so it appears under **Claude needs you**, like one
that waits on a permission prompt.

## Sessions started outside Kelta

A `claude` you start in another terminal can show in Kelta too, with its state. Install Kelta's hook relay once:

- At launch, when Claude Code is found and the relay is not installed, a toast asks "Show the Claude sessions
  you start outside Kelta in Now?". **Install** adds Kelta's hooks to Claude Code's user settings
  (`~/.claude/settings.json`, or `$CLAUDE_CONFIG_DIR/settings.json`); **Never** stops asking
  (`claude.hooks_offer = false`).
- Settings > Claude > **Claude sessions started outside Kelta** installs or removes it any time. Remove drops only
  Kelta's hooks.

The relay is `kelta-ctl hook --outside`. It reports a session only when Kelta runs, never fails Claude, and does
nothing inside a Kelta pane (those report as usual).

Such sessions are listed under **Elsewhere** in the rail and in Now, dimmed, with their state. One that needs you
moves up to **Claude needs you**. If the session has a ticket, its row shows the state too.

`Enter` or `g` on one (or a click in the rail) jumps to it:

- if it runs in tmux (the default tmux server), Kelta selects its window and pane, and switches the client to it;
- Kelta then raises the terminal window that owns it: on Hyprland and Sway the window of the closest parent
  process, on macOS the terminal app (not one window of it).
- When it cannot, Peek opens and says where the session runs.

## Claude in your own terminal

By default Claude runs in a Kelta pane. A session template can run it elsewhere with `claude_host`:

| `claude_host` | Claude runs |
|---|---|
| `builtin` (default) | in its Kelta pane |
| `terminal` | in a window of your terminal, opened by `claude.terminal_command` |
| `multiplexer` | in a new window of the tmux session `kelta-<repo>` (created detached when missing) |

```toml
[claude]
# {command} is the script Kelta writes: it sets the session's hook and MCP variables and runs claude
terminal_command = ["ghostty", "--working-directory={cwd}", "-e", "{shell}", "-ic", "{command}"]

[[session_templates]]
id = "claude-terminal"
label = "Claude in my terminal"
claude_host = "terminal"
layout = { session = "claude" }
```

The other placeholders are `{cwd}` and `{shell}`. Without `terminal_command`, Kelta uses `open -na Terminal
{command}` on macOS and `x-terminal-emulator -e {command}` on Linux.

Kelta still creates the worktree, `ticket.md`, the hooks and the MCP config, and the Claude pane shows a banner with
a **Focus** button that jumps to the external window (as above). Status, Resume, park and Finish work as with a
built-in pane, and closing the Kelta pane ends that Claude. Sending text into Claude from Kelta (notes, queued
prompts) needs the built-in pane: Kelta copies the prompt to the clipboard instead.

## Workflows

A workflow is a task for Claude that runs in the background in its own tab. Kelta lists two kinds.

**Saved workflows**: the `.claude/workflows/*.js` files of the project's main repository, then
`~/.claude/workflows/`. On a ticket, `w` runs one: Kelta claims the ticket for it, starts Claude in a new tab and
tells it to run that workflow on the ticket. The run reports its phase, and the ticket's row shows it.

The rail's **Workflows** section lists the current run of each Claude session: its phase while it runs, **Needs
input** when Claude asks you something, then how it ended. Click a row for the session's runs.

**Workflows from your config**: a prompt or a slash command, in `config.toml`:

```toml
[[workflows]]
id = "describe-pr"
label = "Describe PR"
prompt = "/describe-pr {pr.url}"
on = "pr"            # "ticket" (default), "pr" or "global"
```

`on` says where it is offered:

| `on` | Offered | Placeholders in `prompt` |
|---|---|---|
| `ticket` | `w` on a ticket; the run claims the ticket | `{ticket.key}`, `{ticket.title}`, `{ticket.url}`, `{file}` (the ticket as Markdown) |
| `pr` | `w` on a row of Reviews or in the review detail | `{pr.number}`, `{pr.url}`, `{pr.head}`, `{pr.base}`, `{pr.repo}`, `{pr.title}`, `{ticket.key}` (the linked ticket, else empty) |
| `global` | the command palette, as "Run <label>", with no ticket | none |

`{workflow}` (the workflow's id) works in all three. A second run on the same ticket or pull request opens the running one instead of starting another. `cwd` defaults
to the pull request's worktree or repository for `pr`, else the project's primary repository. A saved `.js`
workflow with the same name wins over a config one. Set `enabled = false` to hide one. The keys are in
[Settings](../SETTINGS.md).

When a run starts, a toast offers **Open** (shows the session) and **Peek** (its last lines). `[[workflows]]` is a
global setting: a project or repository file cannot define one.

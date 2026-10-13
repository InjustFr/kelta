# Getting started

## Install

See "Install" in the [README](../../README.md#install): a dmg on macOS, an AppImage or a deb on Linux, from the
stable releases or the nightly builds. A stable install stays on the stable channel and a nightly on the nightly
one; each updates itself from its own channel (a deb never does). The macOS app is not notarized, so the first
launch needs one extra step, described in the README.

`kelta-ctl` is installed next to the app and copied to `<data>/bin/current` so scripts and compositor bindings
have a stable path.

## First launch

On a fresh install a welcome sheet checks your machine (`claude`, `nvim`, `git`, `gh`, `glab`, keyring,
notification daemon) and offers the first steps:

- **Set up with Claude**: Claude Code finds your repositories and asks for the rest
  ([details](claude-code.md#set-up-with-claude)). The command palette has the same action later.
- **Create a project from a folder**, then **Add an account**. The steps below do the same by hand.

1. Open a project: **Add project** in the sidebar, or `kelta-ctl open ~/code/shop`. Type the folder or pick it
   with **Browse...**; Kelta detects the repositories, remotes and tracker. A folder holding several repositories
   becomes one project.
2. Add an account (Settings > Accounts) for your tracker and code host. Tokens go to the OS keyring; if you have
   none, use `env:` or `command:` references ([keyring setup](keyring-and-notifications.md)). GitHub and GitLab can
   also reuse your `gh` / `glab` login or sign in with the browser ([oauth](oauth.md)).
3. Pick a ticket source: **Pick a board** in Tickets or in the Queue, or **Set up with Claude**
   ([working with tickets](tickets.md#sources)).

While your Queue is empty and your first item has not shipped yet, it shows **Your loop** instead of a blank list:
the stage track and five steps, **Add a project**, **Connect a ticket source**, **Start work on a ticket** (`s`),
**Review what Claude changed** (`Enter`) and **Ship** (`p`), each checked off from what Kelta finds. The current step
is highlighted, with its button. The last three are [the loop](#the-loop). Once your first item has shipped, the panel never shows again.

Settings > Diagnostics checks the things Kelta depends on: graphics, WebKitGTK, notification daemon, keyring,
`claude`, `nvim`, `git`, `gh`, `glab`, hook health and socket paths.

### Accounts

| Provider | Tracker | Code host | Token |
|---|---|---|---|
| Jira (Cloud, Data Center) | yes | no | Cloud: e-mail and API token. Data Center: personal access token |
| Redmine | yes | no | API key |
| GitHub (also Enterprise Server) | issues, Projects v2 | yes | `gh` login, a token, or browser sign-in |
| GitLab (also self-managed) | yes | yes | `glab` login, a token, or browser sign-in |
| Linear | yes | no | personal API key ([Linear](linear.md)) |
| Gitea, Forgejo, Codeberg | issues | yes | access token ([Gitea](gitea.md)) |
| Bitbucket Cloud | no | yes | Atlassian API token ([Bitbucket](bitbucket.md)) |

## The loop

Kelta has three places, listed at the top of the sidebar: the **Queue**, where all your work waits; **Tickets** and
**Pull requests**, the lists you pick work from; and, for each piece of work you start, its own tab with Claude and
nvim, the desk. One piece of work (a branch, a worktree, a Claude conversation, at most one pull request) is a
**work item**. The loop goes:

1. **Pick a ticket.** Open **Tickets** (`Cmd+Shift+B`, `Ctrl+Shift+B` on Linux). It lists your tickets from every
   open project (Mine); `2` and `3` switch to Unassigned and Team. Select one and press `s` (**Start work**).
   Kelta creates a worktree and a branch in the ticket's project, moves the ticket to In progress and opens Claude,
   already briefed on the ticket, next to nvim. No ticket? `Shift+Cmd+N` starts a work item from one sentence.
2. **Let Claude work.** The work item's tab is the desk. Its header names the item and says, in one sentence,
   whose move it is and why (`Your move: To review, +212 −3` once Claude stops); the stage track under it runs Ticket, Claude, PR, Review,
   Merge, Deploy. The **Story panel** on the right (`Cmd+I`) tells the item's story in that order: since you left,
   the ticket and its acceptance criteria, Claude's tasks and last message, the changes file by file, the pull
   request, the start log. You can go elsewhere: the item keeps its place in the sidebar.
3. **Come back when it needs you.** When Claude stops, asks or finishes, the item moves to **Your move** in the
   Queue (`Cmd+0`). `Cmd+J` goes straight to the next thing that needs you, wherever it is. A Claude that asks a
   question can be answered from the Queue: `1`–`9` pick an option, or type in **Reply to Claude**.
4. **Review.** `Enter` runs the item's next step, here **Review changes**: the diff of what Claude changed since you
   last looked, in nvim. `Shift+R` marks it reviewed; send fixes back to Claude from the Story panel's Changes.
5. **Ship.** `p` pushes the branch and opens the pull request; the ticket moves to review. The item now waits on
   others (reviewers, checks) in the Queue, and comes back to Your move when a review asks for changes or checks
   fail. Once merged, **Finish** removes the worktree.

Pull requests you are asked to review are in **Pull requests** (`Cmd+Shift+R`): read, approve, or review one
locally in a worktree with Claude ([pull requests](reviews.md)).

### Five keys

| Key | What it does |
|---|---|
| `Enter` | the next step of what is selected, in every list; on the desk, `Cmd+.` `Enter` |
| `Cmd+.` | the actions of whatever is selected (a row, a detail, the work item on the desk), what can run now first |
| `Cmd+K` | find anything: recent places, work items, sessions, tickets, pull requests, actions, settings. `#`, `!`, `>` and `,` narrow it to tickets, pull requests, commands or settings |
| `Cmd+J` | the next thing that needs you (`Shift+Cmd+J` back) |
| `Cmd+0` | the Queue, and from the Queue back where you were |

On Linux, `Ctrl` replaces `Cmd`. The prefix key (`Ctrl+Shift+Space`, then one key) reaches every action; pause after
it and a panel lists the keys that follow (`Esc` closes it, `?` opens the full list).

## The Queue

The **Queue** lists all your work, across projects, in three zones by whose move it is:

- **Your move**: Claude needs you, ready for review, next up, new for you, fix, review requests, ship and clean up,
  up next.
- **Claude working**: work whose Claude is working, background runs included: back in minutes.
- **Waiting on others**: reviewers, checks and deploys: in flight, elsewhere, blocked, then waiting to deploy
  (collapsed: tickets in a status their source lists as waiting for a deployment, set in Settings > Sources).

The header sums them up: "9 your move, 2 Claude working, 2 waiting on others". Each row has a stage track: the line
runs to the furthest stage reached and the mark sits where the item is now, filled when it is your move, a ring while
it waits, a diamond when something broke. The row's next step is a button that runs it. The selected item's brief
sits beside the list (under it in a narrow pane): its next action, the two likely after it, the rest behind "more
actions" (`.`), Claude's last message and the facts.

`j` `k` move, `Enter` runs the next step, `g` goes to its place without acting, `/` filters, `Cmd+Shift+R` refreshes,
`?` lists every key. A letter means the same in the Queue, Tickets and Pull requests: `s` work on it (start a ticket,
go to a work item's tab, review a pull request locally), `v` view changes (`Shift+V` the whole diff), `o` open on
the host, `m` move the ticket, `c` comment, `e` expand Claude's message, `Shift+R` mark reviewed. `Esc` clears the
filter, then goes back.

## The sidebar

The sidebar on the left lists your work: the **Queue**, **Tickets** and **Pull requests**, then each open project
with its work items, in the order you started them, so a row never moves under you. `Cmd+1` to `Cmd+9` jump to the
first nine. Every other tab of a project (shells, diagnostics, details) and its background sessions fold into one
**n more** row; click it to see them. A row whose Claude needs you is lit. **Settings** sits at the bottom.

- **Width.** Drag the edge of the sidebar to resize it (200 to 480 px). With the edge focused, the arrow keys move
  it by 16 px (64 with Shift).
- **Collapse.** `Cmd+B` (`Ctrl+B` on Linux), the button at the bottom of the sidebar, or `Enter` on the edge
  folds it into an icon column. It starts folded when the window is narrower than 1400 px, and folds while the
  Queue is shown (the Queue lists the same work); open it by hand and it stays open until you leave the Queue.
  The width and the state are remembered.
- A tab running a Claude workflow shows the run's phase under its row ([details](claude-code.md#workflows)).
- **Other Claude sessions** lists Claude sessions started outside Kelta, once the relay is installed
  ([details](claude-code.md#sessions-started-outside-kelta)).
- Tools (lazygit, lazydocker...) are not in the sidebar: their key or the palette opens them in the tab you are on
  ([tools](tools.md)).

## The top bar

Across the top of the window: `‹` `›` go back and forward through the places you visited, the search field opens
the palette (`Cmd+K`), **All projects** narrows the Queue, Tickets and Pull requests to one project, the **needs
you** chip counts the Claudes blocked on you (a click goes to one; `Cmd+J` does the same), and **+ New** starts a
work item or a session.

The needs you chip is the one thing in Kelta that calls you. A Claude that finished does not send a desktop
notification by default (it can wait in the Queue), and a toast closes when you leave the view it came from. To
work without interruptions, **focus mode** (`Shift+Cmd+F`) hides the sidebar and the Story panel and holds the
rest until you leave it ([focus mode](keyring-and-notifications.md#focus-mode)).

## Projects

- **Add project** opens the New project sheet; `kelta-ctl open <path>` does the same from a terminal.
- A project's menu in the sidebar (right-click, or the button on its name) has **Close project** (its sessions keep
  running), **Close and stop sessions** and **Remove project...** (its file moves to `projects/.trash`).
- A closed project leaves the sidebar. Bring it back with **Open project...** under the project list (shown while a
  closed project exists), the palette action **Open project**, or the switcher (`Cmd+P`, `Ctrl+P`), which
  lists closed projects as "not open".

## Panes and windows

- Drag a pane by its header onto the side of another pane to split beside or below it, onto its middle to swap the
  two, onto a row in the sidebar to move it there, or onto the project's name in the sidebar to give it a tab of
  its own. A pane stays in its project.
- Drag a pane, or a row of the sidebar, out of the Kelta window to open it in its own window. The same is **Move to
  new window** in the pane's or the tab's menu, and the palette action **Move pane to new window**. In that window
  the menu says **Move back to main window**. A detached window shows one tab; its sessions keep running.
- Settings (`Cmd+,`) opens over the window; `Esc` or `Cmd+,` again closes it onto where you were.

## Keyboard on Linux

Linux shortcuts are the macOS ones with `Cmd` replaced by `Ctrl`: `Ctrl+K` palette, `Ctrl+D` / `Ctrl+Shift+D`
split, `Ctrl+W` close pane, `Ctrl+Alt+arrows` move between panes. Inside a terminal a plain `Ctrl+letter` belongs to
the shell (`Ctrl+D` still ends it, `Ctrl+W` deletes a word), so the same actions also answer to `Ctrl+Shift+letter`
(`Ctrl+Shift+K`, `Ctrl+Shift+W`...), and the prefix key (`Ctrl+Shift+Space`, then one key) reaches every action.
Pause after the prefix and a panel lists the keys that follow (`Esc` closes it, `?` opens the full list);
`keys.prefix_hint_ms` sets the pause, `0` turns the panel off.
Copy and paste in a terminal are `Ctrl+Shift+C` / `Ctrl+Shift+V`. If you never use those shell keys, set
`keys.linux_terminal = "kelta_first"` (Settings > Keys) and Kelta's `Ctrl+letter` wins in terminals too. Your
desktop may already take `Ctrl+Alt+arrows` for workspaces: rebind either side in Settings > Keys.

## Language

The interface is available in English, French and German: Settings > General > Language (`app.language`:
`system`, `en`, `fr` or `de`). `system` follows the OS language, and English when it is none of the three. It
applies at once. Ticket and pull request content, terminal output and the keys of the config files stay as they
are.

## Where things live

| What | Linux | macOS |
|---|---|---|
| Settings (`config.toml`, `projects/`) | `~/.config/kelta/` (`$XDG_CONFIG_HOME`) | `~/.config/kelta/` (`$XDG_CONFIG_HOME`) |
| Data (state, `secrets.enc`, `bin/current`) | `~/.local/share/kelta/` | `~/Library/Application Support/dev.kelta.Kelta/` |
| Logs (`kelta.log`, `keltad.log`) | `~/.local/state/kelta/logs/` | `~/Library/Logs/Kelta/` |

Settings > Edit TOML edits the file of the layer you are on. `KELTA_LOG=debug` raises the log level for one run.
To report a bug, see [reporting bugs](../../CONTRIBUTING.md#reporting-bugs-and-requesting-features).

## Window behaviour

- macOS: closing the window keeps every session running (background mode). Click the Dock icon to come back.
- Linux: closing the window quits. Set `window.close_behavior = "background"` to keep sessions running; then
  `kelta`, a second launch or `kelta-ctl toggle` reopens the window.
- Quitting (Cmd+Q, Dock > Quit, or closing the window when it quits) while sessions run asks whether to **keep them running**
  in the background (they are back, same processes, on the next launch) or **stop all sessions**. Tick "Don't ask
  again" to remember the answer; change it in Settings > General (`app.quit_sessions`).
- `window.decorations`: `auto` (no title bar on Hyprland and Sway), `native`, `none`, `custom`. Restart to apply.
- `window.restore_geometry` restores the size and position of the last session.

## Command line

`kelta [--safe-graphics] [--config-dir DIR] [--set key=value] [PATH...]`. Starting `kelta` while it runs forwards
the arguments to the running instance and raises its window.

`kelta-ctl`: `toggle`, `palette`, `next`, `back`, `open <path>`, `focus-project <id>`, `start <ticket> [--project]`,
`start --task <text> [--project]` (a work item with no ticket), `new --template <id> [--cwd] [--project]`,
`emit <custom.event>`, `trust <repo>`, `editor-open <file>[:line]`, `plugin install <src>`, `version`.

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

1. Open a project: **Add project** in the project rail, or `kelta-ctl open ~/code/shop`. Type the folder or pick it
   with **Browse...**; Kelta detects the repositories, remotes and tracker. A folder holding several repositories
   becomes one project.
2. Add an account (Settings > Accounts) for your tracker and code host. Tokens go to the OS keyring; if you have
   none, use `env:` or `command:` references ([keyring setup](keyring-and-notifications.md)). GitHub and GitLab can
   also reuse your `gh` / `glab` login or sign in with the browser ([oauth](oauth.md)).
3. Click **Tickets** at the top of the rail (`Cmd+Shift+B`). It shows your tickets from every open project (Mine);
   `2` and `3` switch to Unassigned and Team ([working with tickets](tickets.md)). Select one and press
   **Start work**: Kelta creates a worktree in the ticket's project and opens Claude and nvim side by side. **Queue**
   and **Reviews**, just below, show what waits on you and the PRs to review, across projects too
   ([reviews](reviews.md)).

While your Queue is empty, it shows **Your loop** instead of a blank list: the stage track and the five steps Kelta
works in, each checked off from what it finds. The current step is highlighted, with its button.

1. **Add a project.**
2. **Connect a ticket source**: **Pick a board**, or **Set up with Claude**.
3. **Start work on a ticket** (`s`): Claude works in its own worktree.
4. **Review what Claude changed** (`Enter`).
5. **Ship** (`p`): the pull request opens.

No tickets? `Shift+Cmd+N` starts a branch and a Claude from one sentence. Once your first item has shipped, the
panel never shows again.

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

## The rail

The rail on the left holds Search, **Tickets**, **Queue** and **Reviews**, your open projects with their tabs, the
Tools, and Settings.

- **Width.** Drag the edge of the rail to resize it (200 to 480 px). With the edge focused, the arrow keys move
  it by 16 px (64 with Shift).
- **Collapse.** `Cmd+B` (`Ctrl+B` on Linux), the button at the bottom of the rail, or `Enter` on the edge
  folds the rail into an icon column. It starts folded when the window is narrower than 1100 px. The width and
  the state are remembered. In a narrow rail the shortcut hints hide before a label is cut.
- A tab running a Claude workflow shows the run's phase under its row ([details](claude-code.md#workflows)).
- **Elsewhere** lists Claude sessions started outside Kelta, once the relay is installed
  ([details](claude-code.md#sessions-started-outside-kelta)).

## Projects

- **Add project** opens the New project sheet; `kelta-ctl open <path>` does the same from a terminal.
- A project's menu in the rail (right-click, or the button on its row) has **Close project** (its sessions keep
  running), **Close and stop sessions** and **Remove project...** (its file moves to `projects/.trash`).
- A closed project leaves the rail. Bring it back with **Open project...** under the project list (shown while a
  closed project exists), the palette action **Open project**, or the switcher (`Cmd+P`, `Ctrl+P`), which
  lists closed projects as "not open".

## Panes and windows

- Drag a pane by its header onto the side of another pane to split beside or below it, onto its middle to swap the
  two, onto a tab in the rail to move it there, or onto the project's name in the rail to give it a tab of its
  own. A pane stays in its project.
- Drag a pane, or a tab in the rail, out of the Kelta window to open it in its own window. The same is **Move to
  new window** in the pane's or the tab's menu, and the palette action **Move pane to new window**. In that window
  the menu says **Move back to main window**. A detached window shows one tab; its sessions keep running.
- Settings (`Cmd+,`) opens the tab you already have instead of a new one.

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

## Queue

The **Queue** is where all your work waits, across projects, in one list split in three zones by whose move it is:
**Your move** (Claude needs you, ready for review, next up, new for you, fix, review requests, ship and clean up, up
next), **Claude working** (work whose Claude is working, background runs included: back in minutes) and **Waiting on
others** (reviewers, checks and deploys: in flight, elsewhere, blocked, then waiting to deploy, collapsed: tickets in a
status their source lists as waiting for a deployment, set in Settings → Sources). The header sums them up: "9 your
move, 2 Claude working, 2 waiting on others". Each row has a stage track, Ticket, Claude, PR, Review, Merge, Deploy:
the line runs to the furthest stage reached and the mark sits where the item is now, filled when it is your move, a
ring while it waits, a diamond when something broke. The row's next step is a button that runs it. The selected
item's brief sits beside the list (under it in a narrow pane): its next action, the two likely after it, the rest
behind "more actions" (`.`), Claude's last message and the facts. `j` `k` move, `Enter` runs the next step, `g` goes
to its place without acting, `/` filters, `R` refreshes, `?` lists every key. `Cmd+.` (`Ctrl+.` on Linux) works on any
selection: in the Queue, Tickets and Reviews it opens the selected row's actions in a menu, the next step first, so
`Cmd+.` `Enter` runs it; on a work tab, the work menu; with nothing selected, the palette.

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

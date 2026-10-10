# Tools

A tool is a button that opens a program in Kelta: a terminal program like lazygit in its own pane, or an app
like Fork beside Kelta. Tools show in the tool strip at the right of the tab bar, in the `+` menu, and in the
palette ("Open tool: lazygit").

## Add one in Settings

Tools are yours, not a project's: they live in your global settings and work in every project. Settings >
Tools > **Add tool** is at the Global level (a Project level shows the list and a **Go to Global** button):

- **Terminal**: a name and a command line, e.g. `lazydocker` or `k9s --context "my ctx"`. The command line
  is split like a shell would (quotes and backslashes), but it is not run through a shell: no pipes, `&&` or
  `$VAR`.
- **App** (macOS): a name and the application, e.g. `Fork`. Kelta runs `open -a Fork .`, so the app opens on
  the current folder (turn "Open the current folder in it" off to just start the app).
- **Key** (optional): focus the field and press a shortcut. If an action, a reserved key or another tool
  already uses it, the form says so: the tool's key will not work until you pick another one (or rebind the
  action in Settings > Keys). The tool list shows the same warning on each tool whose key is taken.

The id comes from the name. When a tool with that id already exists (yours or a plugin's), the new one gets
`-2`, `-3`…, so it never silently replaces another tool. Environment variables, an install
check, the working folder and web tools are set in the full editor below the form, or with **Edit TOML**.

## In the config file

The form writes a plain `[[tools]]` entry to `config.toml`. A project file or a repo's `.kelta/config.toml`
cannot hold tools: Kelta reports it (tools that used to be in project files were moved to `config.toml` once,
with a copy of each project file in `projects/.trash/`).

```toml
[[tools]]
id = "lazydocker"
label = "lazydocker"
command = "lazydocker"

[[tools]]
id = "lazygit"
label = "lazygit"
icon = "branch"
command = "lazygit"
keybinding = "mod+g"            # Cmd+G on macOS, Ctrl+Shift+G on Linux
check = ["lazygit", "--version"]
install_hint = "brew install lazygit"

[[tools]]
id = "k9s"
label = "k9s"
command = "k9s"
args = ["--context", "my ctx"]

[[tools]]
id = "fork"
label = "Fork"
kind = "external"               # launched beside Kelta, never tracked or stopped
command = "open"
args = ["-a", "Fork", "."]
```

A tool runs in the work item's worktree when there is one, else in the project root. Set `cwd` to change it;
placeholders work in `cwd`, `command` and `args`, and `|` picks the first one that has a value:
`cwd = "{worktree|project.root}"`, `args = ["--path", "{repo.path}"]`.

## Where a tool opens

A tool opens where you are: in the project you are looking at, in the worktree of the focused work item; from
Home, Tickets, Now or Reviews, in Home (in `~`). A tool that takes a project, because a field uses
`{project.*}` or `{repo.*}`, asks which project when there is none. [Sapling ISL](sapling-isl.md) is one:
its `start` args hold `--cwd {repo.path}`, so it needs a project's repo.

Opened from Tickets, it shows a project picker ("Open Sapling ISL for project…"): pick `shop` to switch to it
and open ISL there, or press `Esc` to open nothing. Inside `shop` it opens there without asking. The palette
also has "Open Sapling ISL for project…", which always asks, even inside a project. A tool with no project
placeholder, like lazydocker, never asks.

Other fields: `placement` (`split_right` by default, `split_down`, `new_tab`, `replace_focused`),
`close_on_exit` (`never`, `on_success`, `always`), `env`, `description`. Web tools (`kind = "web"`) are
covered in [Sapling ISL](sapling-isl.md).

## Opening a tool that is already running

A terminal tool runs once per work item: clicking its button (or pressing its key) while it runs in the
current work item focuses that pane instead of opening a second copy, and the button shows as pressed. In
another work item it opens a new one there. When the program is not installed, the pane says so with its
install hint and a **Check again** button; Settings > Tools > **Check** runs the same check.

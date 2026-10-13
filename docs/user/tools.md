# Tools

A tool opens a program from Kelta: a terminal program like lazygit over the tab you are working in, or an
app like Fork beside Kelta. Tools are a short trip away from your work, not places in the sidebar: open one
with its key (`Cmd+Alt+1..9` on macOS opens the first nine, in config order) or from the palette (`Cmd+K`,
"Open lazygit").

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
scope = "project"               # one per project, whatever the work item

[[tools]]
id = "lazygit"
label = "lazygit"
icon = "git-branch"              # a Lucide icon name or 1-2 characters
command = "lazygit"
keybinding = "mod+g"            # Cmd+G on macOS, Ctrl+G on Linux (a focused terminal keeps Ctrl+G)
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

`icon` is any [Lucide](https://lucide.dev) icon name (`git-branch`, `container`, `terminal`, `file-code`, ...) or one
or two characters. Without one, a well-known tool gets its icon (lazydocker, lazygit, a
shell, an editor) and any other shows the first two letters of its label.

A tool runs in the work item's worktree when there is one, else in the project root. Set `cwd` to change it;
placeholders work in `cwd`, `command` and `args`, and `|` picks the first one that has a value:
`cwd = "{worktree|project.root}"`, `args = ["--path", "{repo.path}"]`.

## Where a tool opens

A terminal tool opens in the tab you are on, as a pane that fills the tab (zoomed), in the tab's worktree
when it is a work item's tab, else in the project folder. Press its key again, or `Esc` `Esc`, to get back
to the tab as it was: the tool's pane closes but the program keeps running, and its key brings it back where
you left it. There is no tool tab and nothing to return from.

It opens where you are: in the project you are looking at, with the focused work item; from Home, Tickets,
Queue or Pull requests, in Home (in `~`). A tool that takes a project, because a field uses
`{project.*}` or `{repo.*}`, asks which project when there is none. [Sapling ISL](sapling-isl.md) is one:
its `start` args hold `--cwd {repo.path}`, so it needs a project's repo.

Opened from Tickets, it shows a project picker ("Open Sapling ISL for project…"): pick `shop` to switch to it
and open ISL there, or press `Esc` to open nothing. Inside `shop` it opens there without asking. The palette
also has "Open Sapling ISL for project…", which always asks, even inside a project. A tool with no project
placeholder, like lazydocker, never asks.

Other fields: `placement` for web tools (`split_right` by default, `split_down`, `new_tab`, `replace_focused`;
a terminal tool always opens zoomed in the current tab), `close_on_exit` (`never`, `on_success`, `always`),
`env`, `description`. Web tools (`kind = "web"`) are
covered in [Sapling ISL](sapling-isl.md).

## Opening a tool that is already running

`scope` says how many copies a terminal tool has. By default (`work`) it runs once per work item: its key
in the same work item brings that copy back (wherever it was hidden) instead of starting a second one, and
in another work item it starts a new one there. `project` and `user` run it once per project (Settings >
Tools: **One instance**); its key brings that copy into the tab you are on. A web tool keeps its own place:
`project` as its own tab per project, `user` as one tab in Home; pressing its key again while it is in front
takes you back where you were, the Queue included, and a web tool that takes a project asks for another
project instead. When the program is not installed, the pane says so with its install hint and a **Check
again** button; Settings > Tools > **Check** runs the same check.

A tab made only of tools from an earlier version stays a tab under the project's "n more" row; the tool's
key moves its program into the tab you are on and the empty tab closes.

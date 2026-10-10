# Sapling ISL (`sl web`)

Sapling's Interactive Smartlog runs as a web tool in a Kelta pane. Kelta starts `sl web` per open pane on a
free loopback port, waits for its JSON line, shows it next to your terminals and stops it when the pane closes.

1. Install Sapling: `brew install sapling` (macOS) or see https://sapling-scm.com/docs/introduction/installation.
   ISL also needs Node.js on your login shell's `PATH`. Check with `sl --version` (it must say "Sapling":
   the `sl` steam-locomotive package uses the same name).
2. Add the tool (Settings > Tools, or in `config.toml` / a project file). It works in Sapling and in plain git
   repos:

```toml
[[tools]]
id = "isl"
label = "Sapling ISL"
icon = "branch"
kind = "web"
start = { command = "sl", args = ["web", "--no-open", "--foreground", "--json", "--port", "{port}", "--cwd", "{repo.path}"],
          ready = { stdout_json = "url" }, ready_timeout_ms = 10000, stop = { signal = "TERM", grace_ms = 3000 } }
embed = "auto"
url_is_secret = true          # the URL carries ISL's access token; Kelta never logs it
check = ["sl", "--version"]
install_hint = "https://sapling-scm.com/docs/introduction/installation"
```

Why these flags: `--port {port}` takes the port Kelta reserved, `--no-open` keeps ISL from opening a browser
or its own window, `--foreground` keeps the server a child of Kelta (so closing the pane stops it), and
`--json` prints the ready line with the tokenized URL. `--cwd {repo.path}` points ISL at the project's
primary repo; use `{worktree|repo.path}` to follow the current worktree instead.

`embed = "auto"` shows ISL in a plain iframe (it sends no frame-blocking headers). `embed = "proxy"` also works:
Kelta's per-tool loopback proxy serves the page and passes ISL's WebSocket through. Both are covered by the
`gate_w1_*` tests in `crates/kelta-plugins/tests/it/tools.rs` against a real `sl web`.

If the pane shows "exited before it was ready", the log under the error usually says `node` was not found
(fix your login shell's `PATH`) or that the folder is not a repository.

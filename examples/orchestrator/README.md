# Orchestrator example

An orchestrator for Kelta: two `[[workflows]]`, four triggers and two slash commands. Read
[docs/user/orchestrators.md](../../docs/user/orchestrators.md) first.

- `config.toml`: merge into `~/.config/kelta/config.toml`. `build-ticket` is a ticket workflow, `fan-out` a
  global one; the triggers notify when a run is blocked, failed or done and when a ticket enters review, and a
  cron line starts `fan-out`.
- `commands/build-ticket.md`: pick (or take) a ticket, make its worktree, claim it, report each phase, open the
  PR and hand off. Run it as `/build-ticket` in a `⌘T` Claude to let it pick a free ticket.
- `commands/fan-out.md`: one `build-ticket` run per free ticket, within `claude.max_live`.

Copy `commands/*.md` to `~/.claude/commands/`. The config is checked by `example_orchestrator_config_parses`
(`crates/kelta-proto/src/settings.rs`).

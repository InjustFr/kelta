# Contributing

Design documents live in `docs/`: `SPEC.md` (product), `ARCHITECTURE.md` (crates, contracts, performance
budgets), `SETTINGS.md`, `PLUGINS.md`, `BUILD_PLAN.md` (how the work is split).

## Setup

```sh
pnpm install --frozen-lockfile
pnpm dev:mock            # UI in the browser against mock IPC, no Rust needed
pnpm dev                 # Vite dev server on 127.0.0.1:5173, for `cargo run -p kelta-desktop`
```

Toolchain: Rust 1.99 (`rust-toolchain.toml`), Node 22, pnpm 9. Generated files (`ui/src/lib/gen`,
`schema/`, `crates/kelta-proto/fixtures`) come from `cargo run -p xtask -- codegen`; never edit them by hand.

## Contracts

`kelta-proto` freezes the shared types and traits, `ui/src/lib/ipc/commands.ts` the IPC wrappers. If a
contract is missing or wrong, do not patch it from your area: work around it locally and write the
request down in `docs/contract-requests/<lane>.md`.

Performance rules (ARCHITECTURE section 13) are part of the contract: no idle timers, intervals or polling
loops, no ad hoc threads outside the allowlisted modules (`clippy.toml`, `scripts/check-no-timers.sh`).

## Secrets in development

Use `env:` references for tokens while developing, for example in `config.toml`:

```toml
secret = "env:JIRA_TOKEN"
```

On macOS every rebuild changes the code signature, so Keychain asks for permission again on each run of a
`keyring:` secret. `env:` and `command:` references never prompt. Never commit tokens or `.env` files.

## QA

Before every pull request (see CI below):

```sh
bash scripts/qa.sh      # or: bash scripts/ci-local.sh, which also runs e2e and cargo-deny
```

It runs the UI build, `svelte-check`, ESLint, vitest, the timer check, `cargo fmt --check`, clippy with
`-D warnings`, `cargo test --workspace` and the codegen drift check. The Playwright suite on mock IPC runs with
`pnpm e2e`.

### Linux check in Docker

`scripts/linux-check.sh` repeats the checks on Ubuntu 24.04 (`docker/ubuntu-build.Dockerfile`), which is the
primary Linux target. Run it when you touch `platform/`, `window/`, packaging or anything with `cfg(target_os)`.

## CI

GitHub Actions is the confirmation, not the test bench. Do not push to see whether it works.

1. Verify locally with `bash scripts/ci-local.sh`: the local mirror of `ci.yml` (`scripts/qa.sh`, Playwright
   e2e, cargo-deny). It also runs `scripts/linux-check.sh` when the diff against `origin/main` touches Linux
   code (`--linux` forces it, `--base <ref>` changes the base).
2. Open the PR when it is ready. Draft PRs do not run CI; marking one ready does. A PR runs only the jobs for
   what it changes (Rust, UI, packaging), and docs-only PRs run nothing. There is no CI on pushes to `main`
   or to branches.
3. Label a PR `perf` to run the benchmarks (bundle budgets and the bench dry run) on it. The real memory/CPU
   runs are manual: Actions, bench, Run workflow.
4. Nightly builds run once a day (03:17 UTC), and only when `main` moved since the last one. Tags `v*` build
   the release.

## Packaging and benchmarks

- `bash packaging/build.sh` builds the installers with `packaging/tauri.release.json`. It needs the
  `kelta-ctl` sidecar, which the script builds.
- `cargo run -p kelta-bench -- --scenario idle-3p10s --dry-run` checks the harness against fixtures. Real runs
  need a release build of the app (`--app target/release/kelta`) and, on macOS, no running Kelta (the runtime
  socket path is shared).
- `node bench/bundle-size.mjs ui/dist` checks the JavaScript budgets.

## Commits

Conventional commits (`feat(scope): ...`). The nightly workflow publishes `main` once a day.

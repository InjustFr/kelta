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
bash scripts/ci-local.sh          # what the diff against origin/main affects; --full runs everything
bash scripts/qa.sh [all|ui|rust] [package...]   # the QA steps alone (default: everything)
```

`qa.sh` runs the UI build, `svelte-check`, ESLint, vitest, the timer check, the gate self-test
(`scripts/affected_test.py`), `cargo fmt --check`, clippy with `-D warnings`, the Rust tests (the codegen
drift check is the xtask test `generated_files_are_up_to_date`). With `all` the UI checks run in the background during the Rust steps and their log prints when
they end. Packages limit the Rust steps to those workspace packages. Rust tests run with
[cargo-nextest](https://nexte.st) (`brew install cargo-nextest`) when it is installed, with one retry: a test
that passes only on its retry is printed as `FLAKY` (fix it, do not ignore it); doctests still run with
`cargo test --doc`. Without nextest the gate falls back to `cargo test`. The Playwright suite on mock IPC runs
with `pnpm e2e`.

`ci-local.sh` picks the steps from the diff against `--base` (default `origin/main`, plus uncommitted and
untracked files), see `scripts/affected.py`:

- Rust steps on the workspace packages owning the changed files plus every package that depends on them.
  `Cargo.lock`, the root `Cargo.toml`, `.cargo/`, `rust-toolchain*`, `rustfmt.toml`, `clippy.toml`,
  `deny.toml`, `ui/src/lib/gen/` and any path outside a package that is not known to be Rust-free (`schema/`,
  `examples/`, `fixtures/` data, ...) run every crate. Docs, `*.md`, `ui/`, `packages/`, `packaging/` and
  `docker/` run none.
- The UI half when `ui/`, `packages/`, `package.json`, `pnpm-*.yaml` or `eslint.config.js` changed; e2e when
  the UI, `apps/desktop/` or `kelta-proto` changed; cargo-deny when `Cargo.lock` or `deny.toml` changed.
- Everything when `scripts/` changed, with `--full`, or when the detection fails. Release tags and the
  nightly use `--full`.

Each run writes its own log under `${TMPDIR}/kelta-gate/logs/`; the summary shows the
time of each step and the log path. The e2e suite runs alongside `qa.sh` (it uses the Vite dev server, not
`ui/dist`) and writes its own `-e2e.log`, printed when it ends. `.config/nextest.toml` starts the slowest
tests first so they do not become the tail of the run; give a new test that takes several seconds a priority
there.

Integration tests live in one binary per crate (`crates/<name>/tests/it/main.rs`, one `mod` per file), which
keeps `target/` small and links fast. Run a subset with a module filter:
`cargo test -p kelta-core --test it daemon::`. The RSS and thread-leak checks in `kelta-term`
(`--test history`, `--test resources`) stay separate binaries because they measure the whole process.

### Linux check in Docker

`scripts/linux-check.sh` repeats the checks on Ubuntu 24.04 (`docker/ubuntu-build.Dockerfile`), which is the
primary Linux target. Run it when you touch `platform/`, `window/`, packaging or anything with `cfg(target_os)`.
`scripts/linux-gui-check.sh` runs the release app under headless sway in the same image and measures the gate G2
latency budgets with kelta-bench (slow; run it when you touch the terminal path or Linux graphics). Its timings
follow the host load, so measure on an idle machine.

## CI

GitHub Actions is the confirmation, not the test bench. Do not push to see whether it works.

1. Verify locally with `bash scripts/ci-local.sh`: the local mirror of `ci.yml` (`scripts/qa.sh`, Playwright
   e2e, cargo-deny) for what the change affects (see QA). `scripts/linux-check.sh` runs only with `--linux` or
   `--full` (`--base <ref>` changes the base).
2. Open the PR when it is ready. Draft PRs do not run CI; marking one ready does. A PR runs only the jobs for
   what it changes (Rust, UI, packaging), and docs-only PRs run nothing. There is no CI on pushes to `main`
   or to branches.
3. Label a PR `perf` to run the benchmarks (bundle budgets and the bench dry run) on it. The real memory/CPU
   runs are manual: Actions, bench, Run workflow.
4. Nightly builds are local: `bash scripts/nightly-local.sh` (clean tree, needs `gh` and Docker) builds the
   macOS dmg here and the Linux deb + AppImage (arm64, and amd64 under QEMU) in Docker, moves the `nightly`
   tag to HEAD and uploads to the prerelease. It runs `bash scripts/ci-local.sh --full` first. Tags `v*` build
   the release: run `bash scripts/ci-local.sh --full` on the commit before you tag it.

## Packaging and benchmarks

- `bash packaging/build.sh` builds the installers with `packaging/tauri.release.json`. It needs the
  `kelta-ctl` sidecar, which the script builds.
- `cargo run -p kelta-bench -- --scenario idle-3p10s --dry-run` checks the harness against fixtures. Real runs
  need a release build embedding the UI (`pnpm --filter @kelta/ui run build`, then
  `cargo build --release -p kelta-desktop --features tauri/custom-protocol` and
  `cargo build --release -p kelta-bench -p tui-sim`). The harness runs the app in a temp HOME with its own
  `KELTA_RUNTIME_DIR`, so a Kelta you are using is never touched.
- `node bench/bundle-size.mjs ui/dist` checks the JavaScript budgets.

## Commits

Conventional commits (`feat(scope): ...`).

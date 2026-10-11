# Contributing

Design documents live in `docs/`: `SPEC.md` (product), `ARCHITECTURE.md` (crates, contracts, performance
budgets), `SETTINGS.md`, `PLUGINS.md`, `BUILD_PLAN.md` (how the work is split).

## Reporting bugs and requesting features

Look first: search the [existing issues](https://github.com/InjustFr/kelta/issues?q=is%3Aissue) (open and closed),
the [README](README.md) and the user docs in `docs/user/`. Settings > Diagnostics checks the usual suspects
(graphics, WebKitGTK, keyring, notification daemon). If an issue already covers it, add your details there.

Then open a [new issue](https://github.com/InjustFr/kelta/issues/new/choose) and pick **Bug report** or **Feature
request**. Security problems never go in a public issue: see [SECURITY.md](SECURITY.md).

A good bug report has:

- the version: Settings > Diagnostics > Version (also in Settings > General > Updates); nightlies end in
  `-nightly.<timestamp>`;
- the OS and its version, the desktop or compositor on Linux, and the install type (dmg, AppImage, deb, source);
- the providers involved (Jira, GitHub, Claude Code, ...);
- numbered steps that reproduce it, what you expected and what happened;
- the relevant log lines. `kelta.log` and `keltad.log` live in `~/Library/Logs/Kelta/` on macOS and in
  `~/.local/state/kelta/logs/` (`$XDG_STATE_HOME/kelta/logs/`) on Linux. `KELTA_LOG=debug` gives more detail.
  Settings > Diagnostics > Copy report adds the environment. Remove tokens, secrets and private ticket content first.

A good feature request says what you are trying to do and what gets in the way, before the proposal.

After filing: the issue gets the `bug` or `enhancement` label, is triaged (questions, more labels, or closed as a
duplicate or out of scope) and, once accepted, goes on the project board, where it moves from Todo to Done.

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
with `pnpm e2e`. The README screenshots (`docs/images/readme/*.webp`) are recaptured from it with
`cd ui && README_SHOTS=1 pnpm exec playwright test readme-shots` (needs `cwebp`: `brew install webp`).

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
there. It also stops a test still running after 90 s: it fails as `TIMEOUT` with its name.

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
   what it changes (Rust, UI, packaging), on Linux only, and docs-only PRs run nothing. Every push to `main`
   runs every job on Linux and macOS: ticket passes merge after `ci-local.sh`, and this run confirms on the
   runners. There is no CI on pushes to other branches. The Linux jobs (ci, nightly, release) and
   `scripts/linux-check.sh` run in the same image, `ghcr.io/injustfr/kelta-ci` (`docker/ubuntu-build.Dockerfile`:
   system packages, pinned Neovim, Rust, nextest), which `.github/workflows/ci-image.yml` publishes when the
   Dockerfile changes on `main`; macOS CI installs the same pinned Neovim (`scripts/install-nvim.sh`).
3. Label a PR `perf` to run the benchmarks (bundle budgets and the bench dry run) on it. The real memory/CPU
   runs are manual: Actions, bench, Run workflow.
4. Nightly builds: the `nightly` workflow runs every night at 03:00 UTC (and on a manual run). It picks the
   newest `main` commit with a green `ci` push run (or its `sha` input), skips when the `nightly` tag is already
   on it or past it, builds the
   Apple silicon dmg + updater bundle and the deb + AppImage for amd64 and arm64 on native runners, and
   publishes them with `scripts/publish-nightly.sh`. The same publish by hand, from a Mac:
   `bash scripts/nightly-local.sh` (clean tree, needs `gh` and Docker) builds the
   macOS dmg and signed updater bundle here (and, with `--linux`, the deb + AppImage for arm64 in Docker;
   `--amd64` adds amd64, built under QEMU and the slowest step), moves the `nightly` tag to HEAD and uploads
   to the prerelease, `latest.json` after the other assets. A nightly without `--amd64` drops the amd64
   assets and the `linux-x86_64` entry of `latest.json`, so installed amd64 AppImages stop updating. It runs
   `bash scripts/ci-local.sh --full` first, and refuses to run without the updater key (see Nightly builds).
5. Releases: run `bash scripts/ci-local.sh --full` on `main`, then Actions, release, Run workflow on `main`
   (tick `dry_run` first to see the version and build without publishing). semantic-release (`.releaserc.json`)
   picks the version from the commits since the last `vX.Y.Z` tag (see Commits), writes it everywhere
   (`scripts/set-version.py`), prepends the notes to `CHANGELOG.md`, pushes `chore(release): X.Y.Z` and the tag
   to `main`. The same run builds that commit (deb + AppImage for amd64 and arm64, Apple silicon dmg ad-hoc signed
   unless the `APPLE_*` secrets are set, updater bundles signed like the nightly), publishes the GitHub release
   with them, `latest.json` and `SHA256SUMS` (installed release builds update from it; the notes are the new
   `CHANGELOG.md` section) and runs `nightly` on the release commit. A failed or cancelled build leaves the tag
   without a release: run release again with `tag` set to it (`vX.Y.Z`).

## Nightly builds

Installed copies update from the nightly's `latest.json`, verified with a Tauri (minisign) key pair. This is not
Apple signing. One-time setup on the Mac that publishes:

```sh
ui/node_modules/.bin/tauri signer generate -w ~/.tauri/kelta-updater.key       # choose a password
security add-generic-password -a "$USER" -s kelta-updater -w            # prompts; login Keychain
```

Put the content of `~/.tauri/kelta-updater.key.pub` in `packaging/tauri.release.json` under
`plugins.updater.pubkey` (replacing the placeholder) and commit it. Back up the private key and its password in a
password manager: losing either strands every installed build until a manual reinstall. `nightly-local.sh` reads
the key from `${KELTA_UPDATER_KEY:-~/.tauri/kelta-updater.key}` and the password from the Keychain, and stops
before building when either is missing or the pubkey is still the placeholder. `packaging/build.sh` without
`TAURI_SIGNING_PRIVATE_KEY` still builds unsigned installers (no update artifacts) from source. The `nightly` and
`release` workflows read the same key and password from the repository secrets `TAURI_SIGNING_PRIVATE_KEY` and
`TAURI_SIGNING_PRIVATE_KEY_PASSWORD`, passed to the build steps only:

```sh
printf %s "$(cat ~/.tauri/kelta-updater.key)" | gh secret set TAURI_SIGNING_PRIVATE_KEY
printf %s "$(security find-generic-password -s kelta-updater -w)" | gh secret set TAURI_SIGNING_PRIVATE_KEY_PASSWORD
```

## Packaging and benchmarks

- `bash packaging/build.sh` builds the installers with `packaging/tauri.release.json`. It needs the
  `kelta-ctl` sidecar, which the script builds. It logs each phase's wall time (`[build.sh] ...`) and cargo's
  `--timings` report lands in `target/cargo-timings/` (the `nightly` run uploads the macOS one).
- `cargo run -p kelta-bench -- --scenario idle-3p10s --dry-run` checks the harness against fixtures. Real runs
  need a release build embedding the UI (`pnpm --filter @kelta/ui run build`, then
  `cargo build --release -p kelta-desktop --features tauri/custom-protocol` and
  `cargo build --release -p kelta-bench -p tui-sim`). The harness runs the app in a temp HOME with its own
  `KELTA_RUNTIME_DIR`, so a Kelta you are using is never touched.
- `node bench/bundle-size.mjs ui/dist` checks the JavaScript budgets.

## Commits

Conventional commits (`feat(scope): ...`). They make the releases: `fix:` and `perf:` give a patch, `feat:` a
minor, `!` after the type or a `BREAKING CHANGE:` footer a major, and their subjects are the release notes. Other
types (`chore`, `docs`, `ci`, ...) release nothing and stay out of the notes.

`CHANGELOG.md` belongs to semantic-release: do not edit it or keep an `[Unreleased]` section. To get a change
into the notes, write its commit subject for the user.

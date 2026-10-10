
## L10 merge (#97)
- No integration fixes: merge was conflict-free and `scripts/qa.sh` passed unchanged.

## L3 merge
- Merge was conflict-free and `scripts/qa.sh` (macOS) passed unchanged.
- `scripts/linux-check.sh` (Docker, ubuntu:24.04) was run for the first time at this merge. It did
  not compile on Linux before L3. Fixes, all in already-merged L10 paths:
  - `bench/src/sample.rs`: `bail` import was only used in the macOS module, so it was an unused import
    on Linux (`-D warnings`). The call site now uses `anyhow::bail!`.
  - `bench/src/run.rs`: `App::before` is only read by the macOS sampler. Dead-code is now allowed on
    the field for non-macOS targets.
  - `apps/desktop/src-tauri/src/platform/probes.rs`: `webkit2gtk::functions::*_version` does not exist
    in webkit2gtk 2.0.2. The probe now calls `webkit2gtk::ffi::webkit_get_{major,minor,micro}_version`.
- Still red in Docker, both unrelated to L3 and present on main before this merge (requests to the owners):
  - L1 `crates/kelta-term/tests/resources.rs` `memory_and_leaks`: 10 idle sessions add ~21060 KiB
    RSS under glibc, against the 20 MB budget (macOS is within budget). The budget was not relaxed.
  - L6 `crates/kelta-work/tests/it/nvim_rpc.rs:99` `edit_checktime_selection_mksession`: selection
    text assertion fails with the nvim in the CI image.
  - `kelta-core` (L3) tests all pass in Docker.

//! Launch crash guard (ARCHITECTURE §12.4): `<data>/launch-guard` is written in `pre_init` and
//! removed on `app_ready`. The file holds the number of consecutive launches that never became
//! ready. State machine (pure): see [`decide`].

use std::path::{Path, PathBuf};

/// File name under the data dir.
pub const GUARD_FILE: &str = "launch-guard";

/// What `pre_init` does for this launch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GuardDecision {
    /// Start with the safe graphics profile.
    pub safe: bool,
    /// Second consecutive failure: print the `--safe-graphics` / diagnostics hint to stderr.
    pub hint: bool,
    /// Value to persist for this launch.
    pub write: u32,
}

/// `prior` = failed (never ready) launches recorded before this one.
///
/// - 0: normal launch.
/// - 1: the previous launch failed: start once in the safe profile.
/// - 2 or more: safe profile again and print the hint.
pub fn decide(prior: u32) -> GuardDecision {
    GuardDecision { safe: prior >= 1, hint: prior >= 2, write: prior.saturating_add(1) }
}

/// Parses the file content (garbage counts as one failure: the file exists).
pub fn parse(content: &str) -> u32 {
    content.trim().parse::<u32>().unwrap_or(1).max(1)
}

pub fn path(data_dir: &Path) -> PathBuf {
    data_dir.join(GUARD_FILE)
}

/// Reads the prior failure count (0 when the file is absent).
pub fn read_prior(data_dir: &Path) -> u32 {
    std::fs::read_to_string(path(data_dir)).map(|s| parse(&s)).unwrap_or(0)
}

/// Records this launch. Errors are ignored: the guard is best-effort.
pub fn write(data_dir: &Path, count: u32) {
    let _ = std::fs::create_dir_all(data_dir);
    let _ = std::fs::write(path(data_dir), count.to_string());
}

/// Called on `app_ready`: the launch succeeded.
pub fn clear(data_dir: &Path) {
    let _ = std::fs::remove_file(path(data_dir));
}

/// Full cycle used by `pre_init`: read, decide, persist.
pub fn begin(data_dir: &Path) -> GuardDecision {
    let d = decide(read_prior(data_dir));
    write(data_dir, d.write);
    d
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn state_machine() {
        assert_eq!(decide(0), GuardDecision { safe: false, hint: false, write: 1 });
        assert_eq!(decide(1), GuardDecision { safe: true, hint: false, write: 2 });
        assert_eq!(decide(2), GuardDecision { safe: true, hint: true, write: 3 });
        assert!(decide(u32::MAX).hint);
    }

    #[test]
    fn parse_is_forgiving() {
        assert_eq!(parse("2\n"), 2);
        assert_eq!(parse(""), 1);
        assert_eq!(parse("zzz"), 1);
        assert_eq!(parse("0"), 1);
    }

    #[test]
    fn failed_launch_switches_to_safe_once_then_recovers() {
        let tmp = tempfile::tempdir().unwrap();
        let d = tmp.path();
        // Launch 1: clean start, never reaches app_ready (crash).
        assert!(!begin(d).safe);
        // Launch 2: safe profile once, no hint. It succeeds: app_ready clears the file.
        let second = begin(d);
        assert!(second.safe && !second.hint);
        clear(d);
        // Launch 3: back to the normal profile.
        assert!(!begin(d).safe);
    }

    #[test]
    fn two_failures_print_hint() {
        let tmp = tempfile::tempdir().unwrap();
        begin(tmp.path());
        begin(tmp.path());
        let third = begin(tmp.path());
        assert!(third.safe && third.hint);
    }
}

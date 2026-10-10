//! On-disk history log (ARCHITECTURE §9.6) through real PTYs: a 50 MiB flood keeps memory flat and
//! the log under its caps; a session respawned under the same id gets its earlier output back.
//! One test so nothing else runs in this process while it measures RSS.

#![allow(clippy::unwrap_used, clippy::expect_used)] // test helpers outside #[test] fns

mod common;

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use common::*;
use kelta_proto::api::TerminalHost;
use kelta_proto::ids::SessionId;
use kelta_proto::term::TerminalLimits;
use kelta_term::PtyTerminalHost;
use kelta_term::backend::default_backend;

const T: Duration = Duration::from_secs(120);

fn rss() -> u64 {
    #[cfg(target_os = "macos")]
    {
        // SAFETY: zeroed proc_taskinfo is a valid out-parameter for proc_pidinfo.
        let mut info: libc::proc_taskinfo = unsafe { std::mem::zeroed() };
        let size = std::mem::size_of::<libc::proc_taskinfo>() as libc::c_int;
        // SAFETY: `info` is a writable buffer of `size` bytes.
        let n = unsafe {
            libc::proc_pidinfo(libc::getpid(), libc::PROC_PIDTASKINFO, 0, (&raw mut info).cast(), size)
        };
        assert_eq!(n, size, "proc_pidinfo");
        info.pti_resident_size
    }
    #[cfg(not(target_os = "macos"))]
    {
        let status = std::fs::read_to_string("/proc/self/status").unwrap();
        let line = status.lines().find(|l| l.starts_with("VmRSS:")).unwrap();
        line.split_whitespace().nth(1).unwrap().parse::<u64>().unwrap() * 1024
    }
}

fn dir_size(dir: &Path) -> u64 {
    std::fs::read_dir(dir).unwrap().map(|e| e.unwrap().metadata().unwrap().len()).sum()
}

fn host(dir: &Path) -> PtyTerminalHost {
    let limits = TerminalLimits {
        history_log: true,
        history_log_mb: 4,
        history_log_total_mb: 16,
        ..TerminalLimits::default()
    };
    PtyTerminalHost::with_history_dir(limits, default_backend(), dir.to_owned())
}

fn run(h: &PtyTerminalHost, id: &str, script: &str) {
    let ev = Arc::new(Events::default());
    h.spawn(spec(id, "/bin/sh", &["-c", script], 100, 30, ev.clone())).unwrap();
    ev.wait_exit(T);
}

fn flood(h: &PtyTerminalHost, dir: &Path) {
    let line = "lorem ipsum dolor sit amet consectetur 0123456789 abcdefghijklmnopqrstuvwxyz";
    // Warm up allocator pools, the writer thread and a full scrollback first.
    run(h, "warm", &format!("yes '{line}' | head -c 4194304"));
    let before = rss();
    run(h, "flood", &format!("yes '{line}' | head -c 52428800; echo FLOOD-END"));
    let after = rss();
    let added = after.saturating_sub(before);
    eprintln!("50 MiB flood: +{} KiB RSS, logs {} KiB", added / 1024, dir_size(dir) / 1024);
    assert!(added <= 16 * 1024 * 1024, "50 MiB flood added {} KiB RSS", added / 1024);
    // Per-session cap 4 MiB (two halves), global cap 16 MiB.
    let flood_bytes: u64 = ["flood.log", "flood.1.log"]
        .iter()
        .map(|f| std::fs::metadata(dir.join(f)).map_or(0, |m| m.len()))
        .sum();
    assert!(flood_bytes <= 4 * 1024 * 1024 + 128 * 1024, "flood log {flood_bytes} B");
    assert!(dir_size(dir) <= 16 * 1024 * 1024 + 128 * 1024);
    let id = SessionId::new("flood");
    // `head -c` cuts the last line short.
    assert!(h.history_tail(&id, 1).unwrap().ends_with(" FLOOD-END"));
    assert_eq!(h.history_tail(&id, 3).unwrap().lines().filter(|l| *l == line).count(), 2);
    let hits = h.history_search(&[id], "flood-end", 10).unwrap();
    assert_eq!(hits.len(), 1, "{hits:?}");
}

fn respawn_restores_and_delete_forgets(h: &PtyTerminalHost, dir: &Path) {
    run(h, "r", "seq 1 300");
    // A new host (app restart) reads what the old one wrote.
    let h2 = host(dir);
    let id = SessionId::new("r");
    assert_eq!(h2.history_tail(&id, 3).unwrap(), "298\n299\n300");
    let ev = Arc::new(Events::default());
    h2.spawn(spec("r", "/bin/sh", &["-c", "echo again; exec cat"], 100, 30, ev.clone())).unwrap();
    let text = wait_until(T, || h2.text_tail(&id, 400).ok().filter(|t| t.contains("again"))).unwrap();
    assert!(text.contains("299\n300"), "{text}");
    // Restored lines are not logged a second time.
    h2.kill(&id, kelta_proto::term::KillSignal::Kill).unwrap();
    ev.wait_exit(T);
    let hits = h2.history_search(std::slice::from_ref(&id), "300", 10).unwrap();
    assert_eq!(hits.len(), 1, "{hits:?}");
    assert!(h2.history_tail(&id, 1).unwrap().ends_with("again"));
    h2.history_delete(&id);
    assert_eq!(h2.history_tail(&id, 5).unwrap(), "");
    assert!(!dir.join("r.log").exists());
}

#[test]
fn history_log() {
    let d = tempfile::tempdir().unwrap();
    let h = host(d.path());
    flood(&h, d.path());
    respawn_restores_and_delete_forgets(&h, d.path());
}

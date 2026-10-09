//! Resource acceptance (BUILD_PLAN §4 L1): 10 idle sessions with 500 lines each add ≤ 20 MB RSS;
//! no fd or thread leak after 100 spawn/kill cycles. One test so nothing else runs in this process
//! while it measures.

#![allow(clippy::unwrap_used, clippy::expect_used)] // test helpers outside #[test] fns

mod common;

use std::sync::Arc;
use std::time::Duration;

use common::*;
use kelta_proto::api::TerminalHost;
use kelta_proto::ids::SessionId;
use kelta_proto::term::{KillSignal, LoginEnv, TerminalLimits};
use kelta_term::PtyTerminalHost;

const T: Duration = Duration::from_secs(20);

#[cfg(target_os = "macos")]
fn rss_and_threads() -> (u64, u64) {
    // SAFETY: zeroed proc_taskinfo is a valid out-parameter for proc_pidinfo.
    let mut info: libc::proc_taskinfo = unsafe { std::mem::zeroed() };
    let size = std::mem::size_of::<libc::proc_taskinfo>() as libc::c_int;
    // SAFETY: `info` is a writable buffer of `size` bytes.
    let n = unsafe {
        libc::proc_pidinfo(
            libc::getpid(),
            libc::PROC_PIDTASKINFO,
            0,
            (&raw mut info).cast::<libc::c_void>(),
            size,
        )
    };
    assert_eq!(n, size, "proc_pidinfo");
    (info.pti_resident_size, u64::try_from(info.pti_threadnum).unwrap())
}

#[cfg(not(target_os = "macos"))]
fn rss_and_threads() -> (u64, u64) {
    let status = std::fs::read_to_string("/proc/self/status").unwrap();
    let field = |name: &str| -> u64 {
        let line = status.lines().find(|l| l.starts_with(name)).unwrap();
        line.split_whitespace().nth(1).unwrap().parse().unwrap()
    };
    (field("VmRSS:") * 1024, field("Threads:"))
}

fn open_fds() -> usize {
    let dir = if cfg!(target_os = "macos") { "/dev/fd" } else { "/proc/self/fd" };
    std::fs::read_dir(dir).unwrap().count()
}

fn threads() -> u64 {
    rss_and_threads().1
}

fn idle_sessions_stay_small(h: &PtyTerminalHost) {
    // Warm up lazily initialised state (thread-locals, allocator pools, tracing) once.
    let ev = Arc::new(Events::default());
    h.spawn(spec("warm", "/bin/sh", &["-c", "echo warm"], 80, 24, ev.clone())).unwrap();
    ev.wait_exit(T);
    let (before, _) = rss_and_threads();
    let script = "i=0; while [ $i -lt 500 ]; do echo \"line $i: lorem ipsum dolor sit amet consectetur adipiscing elit\"; \
                  i=$((i+1)); done; echo END; exec cat";
    // Sessions fill one after another: ten readers growing their grids at the same instant leave
    // ~25 MB of freed-but-retained malloc regions (same live heap, measured with
    // malloc_zone_statistics), which measures the allocator, not the idle footprint.
    let mut evs = Vec::new();
    for i in 0..10 {
        let ev = Arc::new(Events::default());
        h.spawn(spec(&format!("idle{i}"), "/bin/sh", &["-c", script], 80, 24, ev.clone())).unwrap();
        evs.push(ev);
        let id = SessionId::new(format!("idle{i}"));
        wait_until(T, || h.text_tail(&id, 3).ok().filter(|t| t.contains("END")))
            .unwrap_or_else(|| panic!("idle{i} did not finish"));
    }
    let st = h.stats();
    for s in st.sessions.iter().filter(|s| s.id.as_str().starts_with("idle")) {
        assert!(s.history_lines >= 470, "{s:?}");
    }
    // Readers release the grid cache once idle.
    #[allow(clippy::disallowed_methods)] // allowlisted: let reader threads go idle
    std::thread::sleep(Duration::from_millis(300));
    let (after, _) = rss_and_threads();
    let added = after.saturating_sub(before);
    eprintln!("10 idle sessions × 500 lines: +{} KiB RSS", added / 1024);
    assert!(added <= 20 * 1024 * 1024, "10 idle sessions added {} KiB RSS", added / 1024);
    for (i, ev) in evs.iter().enumerate() {
        h.kill(&SessionId::new(format!("idle{i}")), KillSignal::Kill).unwrap();
        ev.wait_exit(T);
    }
}

fn spawn_kill_cycles_do_not_leak(h: &PtyTerminalHost) {
    let settle = |fds: usize, thr: u64| {
        wait_until(T, || {
            (open_fds() <= fds && threads() <= thr && h.stats().reader_threads == 0).then_some(())
        })
    };
    // One cycle first so one-time descriptors (e.g. /dev/ptmx clones, tty lookups) exist.
    let ev = Arc::new(Events::default());
    h.spawn(spec("cycle", "/bin/sh", &["-c", "exec sleep 30"], 80, 24, ev.clone())).unwrap();
    h.kill(&SessionId::new("cycle"), KillSignal::Kill).unwrap();
    ev.wait_exit(T);
    settle(usize::MAX, u64::MAX);
    let (fds, thr) = (open_fds(), threads());
    for i in 0..100 {
        let ev = Arc::new(Events::default());
        // Alternate between reusing one id (replacing an exited session) and fresh ids.
        let id = if i % 2 == 0 { "cycle".to_owned() } else { format!("cycle{i}") };
        h.spawn(spec(&id, "/bin/sh", &["-c", "echo up; exec sleep 30"], 80, 24, ev.clone())).unwrap();
        let sid = SessionId::new(&id);
        if i % 3 == 0 {
            let sink = Frames::default();
            h.attach(&sid, 80, 24, Box::new(sink)).unwrap();
        }
        h.kill(&sid, KillSignal::Kill).unwrap();
        ev.wait_exit(T);
    }
    let ok = settle(fds, thr);
    assert!(
        ok.is_some(),
        "leak after 100 cycles: fds {fds} → {}, threads {thr} → {}, readers {}",
        open_fds(),
        threads(),
        h.stats().reader_threads
    );
}

#[test]
fn memory_and_leaks() {
    let h = PtyTerminalHost::new(LoginEnv::inherited(), TerminalLimits::default());
    idle_sessions_stay_small(&h);
    spawn_kill_cycles_do_not_leak(&h);
}

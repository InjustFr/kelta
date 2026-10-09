//! PTY integration tests with `/bin/sh` and `tui-sim` (BUILD_PLAN §4 L1), on both backends.

#![allow(clippy::unwrap_used, clippy::expect_used)] // test helpers outside #[test] fns

mod common;

use std::sync::Arc;
use std::sync::Mutex as StdMutex;
use std::sync::atomic::{AtomicI64, Ordering};
use std::time::{Duration, Instant};

use common::*;
use kelta_proto::api::{FrameSink, TerminalHost};
use kelta_proto::ids::SessionId;
use kelta_proto::term::{
    ClipboardKind, HIGH_WATERMARK, KillSignal, LoginEnv, TerminalEvent, TerminalLimits, TerminalPalette,
};
use kelta_term::PtyTerminalHost;
use kelta_term::backend::{PortablePty, PtyBackend, RustixPty};
use kelta_term::frames::{self, Frame};
use kelta_term::model::TermModel;

const T: Duration = Duration::from_secs(20);

fn backends() -> Vec<Arc<dyn PtyBackend>> {
    vec![Arc::new(PortablePty), Arc::new(RustixPty)]
}

fn host(b: Arc<dyn PtyBackend>) -> PtyTerminalHost {
    PtyTerminalHost::with_backend(LoginEnv::inherited(), TerminalLimits::default(), b)
}

fn sh(host: &PtyTerminalHost, id: &str, script: &str) -> Arc<Events> {
    let ev = Arc::new(Events::default());
    host.spawn(spec(id, "/bin/sh", &["-c", script], 80, 24, ev.clone())).unwrap();
    ev
}

fn wait_text(host: &PtyTerminalHost, id: &str, needle: &str) -> String {
    let sid = SessionId::new(id);
    wait_until(T, || host.text_tail(&sid, 200).ok().filter(|t| t.contains(needle))).unwrap_or_else(|| {
        panic!("{needle:?} not seen; tail:\n{}", host.text_tail(&sid, 200).unwrap_or_default())
    })
}

#[test]
fn exit_codes_and_signals() {
    for b in backends() {
        let name = b.name();
        let h = host(b);
        let ev = sh(&h, "ok", "echo hi; exit 3");
        assert_eq!(ev.wait_exit(T), (Some(3), None), "{name}");
        assert!(h.text_tail(&SessionId::new("ok"), 10).unwrap().contains("hi"));
        let ev = sh(&h, "sig", "kill -TERM $$; sleep 5");
        assert_eq!(ev.wait_exit(T), (None, Some(libc::SIGTERM)), "{name}");
        // Activity is emitted on the first output.
        assert!(matches!(sh(&h, "act", "echo x").all().first(), None | Some(TerminalEvent::Activity)));
    }
}

#[test]
fn missing_program_and_duplicate_ids() {
    let h = host(Arc::new(PortablePty));
    let ev = Arc::new(Events::default());
    let e = h.spawn(spec("x", "/nonexistent/prog", &[], 80, 24, ev.clone())).unwrap_err();
    assert_eq!(e.code, kelta_proto::ErrorCode::NotFound);
    let e = h.spawn(spec("x", "bin/sh", &[], 80, 24, ev.clone())).unwrap_err();
    assert_eq!(e.code, kelta_proto::ErrorCode::InvalidArgument);
    let ev = sh(&h, "dup", "sleep 30");
    let e = h.spawn(spec("dup", "/bin/sh", &["-c", "true"], 80, 24, ev.clone())).unwrap_err();
    assert_eq!(e.code, kelta_proto::ErrorCode::Conflict);
    h.kill(&SessionId::new("dup"), KillSignal::Kill).unwrap();
    ev.wait_exit(T);
    // An exited session id can be reused.
    let ev2 = sh(&h, "dup", "exit 0");
    assert_eq!(ev2.wait_exit(T), (Some(0), None));
    assert_eq!(h.write(&SessionId::new("nope"), b"x").unwrap_err().code, kelta_proto::ErrorCode::NotFound);
}

#[test]
fn kill_reaches_the_whole_process_group() {
    for b in backends() {
        let h = host(b);
        let ev = sh(&h, "grp", "sleep 60 & sleep 60 & echo PGID=$$; wait");
        let text = wait_text(&h, "grp", "PGID=");
        let pgid: i32 = text.split("PGID=").nth(1).unwrap().lines().next().unwrap().trim().parse().unwrap();
        // SAFETY: probing our child's process group.
        assert_eq!(unsafe { libc::kill(-pgid, 0) }, 0);
        h.kill(&SessionId::new("grp"), KillSignal::Term).unwrap();
        assert_eq!(ev.wait_exit(T).1, Some(libc::SIGTERM));
        // Background members of the group are gone too.
        let gone = wait_until(T, || {
            // SAFETY: as above.
            (unsafe { libc::kill(-pgid, 0) } == -1).then_some(())
        });
        assert!(gone.is_some(), "process group {pgid} still alive");
        // Killing an exited session is a no-op.
        h.kill(&SessionId::new("grp"), KillSignal::Kill).unwrap();
    }
}

#[test]
fn hup_ignored_then_kill() {
    let h = host(Arc::new(RustixPty));
    let ev = sh(&h, "hup", "trap '' HUP; echo armed; while :; do sleep 1; done");
    wait_text(&h, "hup", "armed");
    h.kill(&SessionId::new("hup"), KillSignal::Hup).unwrap();
    assert!(wait_until(Duration::from_millis(500), || ev.exited()).is_none());
    h.kill(&SessionId::new("hup"), KillSignal::Kill).unwrap();
    assert_eq!(ev.wait_exit(T).1, Some(libc::SIGKILL));
}

#[test]
fn resize_delivers_sigwinch() {
    for b in backends() {
        let h = host(b);
        let ev =
            sh(&h, "winch", "trap 'echo WINCH $(stty size)' WINCH; echo ready; while :; do sleep 0.05; done");
        wait_text(&h, "winch", "ready");
        h.resize(&SessionId::new("winch"), 100, 30).unwrap();
        wait_text(&h, "winch", "WINCH 30 100");
        let st = h.stats();
        assert_eq!((st.sessions[0].cols, st.sessions[0].rows), (100, 30));
        assert!(h.resize(&SessionId::new("winch"), 0, 10).is_err());
        h.kill(&SessionId::new("winch"), KillSignal::Kill).unwrap();
        ev.wait_exit(T);
    }
}

#[test]
fn input_echo_and_eagain_queue() {
    for b in backends() {
        let h = host(b);
        let dir = tempfile::tempdir().unwrap();
        let out = dir.path().join("in.bin");
        let script =
            format!("stty raw -echo; echo READY; sleep 1; head -c 300000 > '{}'; echo DONE", out.display());
        let ev = sh(&h, "eagain", &script);
        wait_text(&h, "eagain", "READY");
        // The child does not read for a second: most of this is queued (EAGAIN) and drained later.
        let blob: Vec<u8> = (0..300_000u32).map(|i| b'a' + (i % 26) as u8).collect();
        let t0 = Instant::now();
        for c in blob.chunks(10_000) {
            h.write(&SessionId::new("eagain"), c).unwrap();
        }
        assert!(t0.elapsed() < Duration::from_millis(500), "write blocked");
        wait_text(&h, "eagain", "DONE");
        assert_eq!(std::fs::read(&out).unwrap(), blob);
        assert_eq!(ev.wait_exit(T), (Some(0), None));
    }
}

#[test]
fn events_title_bell_cwd_notify_clipboard() {
    let h = host(Arc::new(PortablePty));
    let ev = sh(
        &h,
        "ev",
        r"printf '\033]2;my title\007\007\033]7;file://host/tmp/a%%20b\007\033]9;hello\007\033]777;notify;T;B\033\\\033]52;c;aGk=\007'",
    );
    ev.wait_exit(T);
    let all = ev.all();
    assert_eq!(all.first(), Some(&TerminalEvent::Activity));
    for want in [
        TerminalEvent::Title("my title".into()),
        TerminalEvent::Bell,
        TerminalEvent::Cwd("/tmp/a b".into()),
        TerminalEvent::Notify { title: None, body: "hello".into() },
        TerminalEvent::Notify { title: Some("T".into()), body: "B".into() },
        TerminalEvent::ClipboardStore { kind: ClipboardKind::Clipboard, text: "hi".into() },
    ] {
        assert!(all.contains(&want), "missing {want:?} in {all:?}");
    }
    assert!(matches!(all.last(), Some(TerminalEvent::Exited { code: Some(0), .. })));
}

#[test]
fn palette_answers_osc_queries() {
    let h = host(Arc::new(PortablePty));
    h.set_palette(TerminalPalette { background: "#010203".into(), ..Default::default() });
    let ev = sh(
        &h,
        "pal",
        r"stty raw -echo; printf '\033]11;?\007'; dd bs=1 count=24 2>/dev/null | tr '\033\007' 'EB'; echo; echo END",
    );
    let text = wait_text(&h, "pal", "END");
    assert!(text.contains("E]11;rgb:0101/0202/0303B"), "{text}");
    ev.wait_exit(T);
}

#[test]
fn queries_are_answered_exactly_once() {
    let sim = tui_sim();
    for b in backends() {
        let h = host(b);
        let ev = Arc::new(Events::default());
        h.spawn(spec("q", sim.to_str().unwrap(), &["--queries", "--timeout-ms", "3000"], 80, 24, ev.clone()))
            .unwrap();
        // tui-sim exits 1 on a missing or duplicated reply.
        assert_eq!(ev.wait_exit(T), (Some(0), None), "{}", h.text_tail(&SessionId::new("q"), 20).unwrap());
        let text = h.text_tail(&SessionId::new("q"), 20).unwrap();
        for name in ["DA1: \\e[?6c", "DSR: \\e[1;1R", "OSC11: \\e]11;rgb:1e1e/1e1e/1e1e\\a"] {
            assert!(text.contains(name), "{name} missing in:\n{text}");
        }
    }
}

#[test]
fn sync_timeout_flushes_the_screen() {
    // A synchronized update that never ends: the 150 ms DEC 2026 deadline must flush it while
    // the program is still running.
    let h = host(Arc::new(PortablePty));
    let ev = sh(&h, "sync", r"printf '\033[?2026hINSIDE SYNC'; sleep 3");
    let t0 = Instant::now();
    wait_text(&h, "sync", "INSIDE SYNC");
    assert!(t0.elapsed() < Duration::from_secs(2));
    assert!(ev.exited().is_none(), "flushed by the deadline, not by the exit");
    h.kill(&SessionId::new("sync"), KillSignal::Kill).unwrap();
}

#[test]
fn ink_redraw_renders() {
    let sim = tui_sim();
    let h = host(Arc::new(PortablePty));
    let ev = Arc::new(Events::default());
    h.spawn(spec(
        "ink",
        sim.to_str().unwrap(),
        &["--ink", "--fps", "60", "--lines", "10"],
        100,
        30,
        ev.clone(),
    ))
    .unwrap();
    wait_text(&h, "ink", "Working");
    let sink = Frames::default();
    let info = h.attach(&SessionId::new("ink"), 100, 30, Box::new(sink.clone())).unwrap();
    let frames = sink.take();
    assert!(matches!(frames::decode(&frames[0]), Some(Frame::Snapshot(_))));
    // The snapshot alone reproduces a full frame.
    let Some(Frame::Snapshot(s)) = frames::decode(&frames[0]) else { unreachable!() };
    let mut m = TermModel::new(100, 30, 100);
    m.feed(s);
    assert!(m.text_tail(40).contains("Working"));
    h.ack(&SessionId::new("ink"), info.generation, frames::ack_len(&frames[0]));
    h.write(&SessionId::new("ink"), b"q").unwrap();
    assert_eq!(ev.wait_exit(T), (Some(0), None));
}

/// Simulated slow UI: records every frame, tracks in-flight bytes as the host sees them.
#[derive(Clone)]
struct SlowView {
    frames: Arc<StdMutex<Vec<Vec<u8>>>>,
    unacked: Arc<AtomicI64>,
    max_unacked: Arc<AtomicI64>,
}

impl FrameSink for SlowView {
    fn send(&mut self, frame: Vec<u8>) -> bool {
        let n = i64::from(frames::ack_len(&frame));
        let now = self.unacked.fetch_add(n, Ordering::SeqCst) + n;
        self.max_unacked.fetch_max(now, Ordering::SeqCst);
        self.frames.lock().unwrap().push(frame);
        true
    }
}

#[test]
fn flood_keeps_inflight_bounded_and_ends_with_a_snapshot() {
    let sim = tui_sim();
    let h = host(Arc::new(PortablePty));
    let ev = Arc::new(Events::default());
    let id = SessionId::new("flood");
    let view = SlowView {
        frames: Arc::default(),
        unacked: Arc::new(AtomicI64::new(0)),
        max_unacked: Arc::new(AtomicI64::new(0)),
    };
    h.spawn(spec("flood", sim.to_str().unwrap(), &["--flood", "50"], 120, 40, ev.clone())).unwrap();
    let info = h.attach(&id, 120, 40, Box::new(view.clone())).unwrap();
    let t0 = Instant::now();
    let mut seen: Vec<Vec<u8>> = Vec::new();
    let mut acked_bytes: u64 = 0;
    let mut exit_at = None;
    // Ack at most ~1 MiB/s: far slower than the child writes.
    loop {
        let batch: Vec<Vec<u8>> = std::mem::take(&mut *view.frames.lock().unwrap());
        for f in batch {
            let n = frames::ack_len(&f);
            #[allow(clippy::disallowed_methods)] // allowlisted: simulated slow renderer
            std::thread::sleep(Duration::from_micros(u64::from(n) / 2 + 200));
            view.unacked.fetch_sub(i64::from(n), Ordering::SeqCst);
            h.ack(&id, info.generation, n);
            acked_bytes += u64::from(n);
            seen.push(f);
        }
        let st = h.stats();
        assert!(st.sessions[0].inflight <= HIGH_WATERMARK);
        if ev.exited().is_some() && exit_at.is_none() {
            exit_at = Some(t0.elapsed());
        }
        if seen.last().is_some_and(|f| f[0] == kelta_proto::term::FRAME_EXIT) {
            break;
        }
        assert!(t0.elapsed() < Duration::from_secs(240), "flood did not finish");
        #[allow(clippy::disallowed_methods)] // allowlisted: test pacing
        std::thread::sleep(Duration::from_millis(2));
    }
    let child_time = exit_at.unwrap();
    let max = view.max_unacked.load(Ordering::SeqCst);
    assert!(max <= i64::from(HIGH_WATERMARK), "in-flight peaked at {max}");
    // The child was not paced by the view: it wrote 50 MiB while the view acknowledged far less.
    let stats = h.stats();
    assert!(stats.sessions[0].bytes_in >= 50 * 1024 * 1024);
    assert!(acked_bytes < 50 * 1024 * 1024 / 4, "view consumed {acked_bytes} bytes in {child_time:?}");
    // Catch-up snapshots happened, and the stream ends with Snapshot + Exit.
    let snapshots = seen.iter().filter(|f| f[0] == kelta_proto::term::FRAME_SNAPSHOT).count();
    assert!(snapshots >= 2, "only {snapshots} snapshots");
    let n = seen.len();
    assert_eq!(frames::decode(&seen[n - 1]), Some(Frame::Exit(0)));
    let Some(Frame::Snapshot(last)) = frames::decode(&seen[n - 2]) else { panic!("no final snapshot") };
    let mut m = TermModel::new(120, 40, 100);
    m.feed(last);
    assert!(m.text_tail(5).contains("flood done"), "{}", m.text_tail(5));
}

#[test]
fn attach_detach_generations_and_exit_banner() {
    let h = host(Arc::new(PortablePty));
    let ev = sh(&h, "gen", "echo first; read x; echo got $x; exit 7");
    wait_text(&h, "gen", "first");
    let id = SessionId::new("gen");
    let a = Frames::default();
    let g1 = h.attach(&id, 80, 24, Box::new(a.clone())).unwrap();
    let b = Frames::default();
    let g2 = h.attach(&id, 90, 20, Box::new(b.clone())).unwrap();
    assert!(g2.generation > g1.generation);
    assert_eq!((g2.cols, g2.rows), (90, 20));
    // Stale ack and stale detach are ignored.
    h.ack(&id, g1.generation, 1_000_000);
    h.detach(&id, g1.generation);
    assert!(h.stats().sessions[0].attached);
    assert!(h.stats().sessions[0].inflight > 0, "snapshot in flight");
    h.write(&id, b"yes\n").unwrap();
    assert_eq!(ev.wait_exit(T), (Some(7), None));
    let fb = b.take();
    assert!(matches!(frames::decode(&fb[0]), Some(Frame::Snapshot(_))));
    let tail: Vec<_> = fb.iter().rev().take(2).collect();
    // Snapshot then Exit after the child exits.
    assert_eq!(frames::decode(tail[0]), Some(Frame::Exit(7)));
    // Detach, then a re-attach after exit gets a snapshot and the exit banner.
    h.detach(&id, g2.generation);
    assert!(!h.stats().sessions[0].attached);
    let c = Frames::default();
    let g3 = h.attach(&id, 90, 20, Box::new(c.clone())).unwrap();
    let fc = c.take();
    assert_eq!(fc.len(), 2);
    let Some(Frame::Snapshot(s)) = frames::decode(&fc[0]) else { panic!() };
    assert!(String::from_utf8_lossy(s).contains("got yes"));
    assert_eq!(frames::decode(&fc[1]), Some(Frame::Exit(7)));
    assert!(g3.generation > g2.generation);
    assert!(h.write(&id, b"x").is_err(), "writing to an exited session");
}

#[test]
fn closed_sink_auto_detaches() {
    struct Closed;
    impl FrameSink for Closed {
        fn send(&mut self, _: Vec<u8>) -> bool {
            false
        }
    }
    let h = host(Arc::new(PortablePty));
    let ev = sh(&h, "closed", "sleep 30");
    let id = SessionId::new("closed");
    h.attach(&id, 80, 24, Box::new(Closed)).unwrap();
    assert!(!h.stats().sessions[0].attached);
    h.kill(&id, KillSignal::Kill).unwrap();
    ev.wait_exit(T);
}

#[test]
fn ack_watchdog_fires_once_while_bytes_are_in_flight() {
    let h = host(Arc::new(PortablePty));
    let ev = sh(&h, "wd", "echo out; sleep 30");
    let id = SessionId::new("wd");
    let sink = Frames::default();
    let info = h.attach(&id, 80, 24, Box::new(sink.clone())).unwrap();
    // Never acknowledged: AckTimeout after ~5 s, exactly once.
    let t0 = Instant::now();
    let got = wait_until(Duration::from_secs(9), || {
        ev.all().into_iter().find(|e| matches!(e, TerminalEvent::AckTimeout { .. }))
    });
    assert_eq!(got, Some(TerminalEvent::AckTimeout { generation: info.generation }));
    assert!(t0.elapsed() >= Duration::from_millis(4500), "fired after {:?}", t0.elapsed());
    #[allow(clippy::disallowed_methods)] // allowlisted: test pacing
    std::thread::sleep(Duration::from_millis(300));
    let n = ev.all().iter().filter(|e| matches!(e, TerminalEvent::AckTimeout { .. })).count();
    assert_eq!(n, 1);
    h.kill(&id, KillSignal::Kill).unwrap();
}

#[test]
fn acked_views_never_time_out() {
    let h = host(Arc::new(PortablePty));
    let ev = sh(&h, "nowd", "for i in 1 2 3; do echo $i; sleep 0.3; done; sleep 30");
    let id = SessionId::new("nowd");
    let sink = Frames::default();
    let info = h.attach(&id, 80, 24, Box::new(sink.clone())).unwrap();
    let t0 = Instant::now();
    while t0.elapsed() < Duration::from_millis(6500) {
        for f in sink.take() {
            h.ack(&id, info.generation, frames::ack_len(&f));
        }
        #[allow(clippy::disallowed_methods)] // allowlisted: test pacing
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(!ev.all().iter().any(|e| matches!(e, TerminalEvent::AckTimeout { .. })));
    h.kill(&id, KillSignal::Kill).unwrap();
}

#[test]
fn scrollback_cap_shrinks_least_recently_viewed() {
    // 1 MiB cap; 4 sessions × 3000 lines × 80 cols × 24 B ≈ 5.5 MiB → trimmed to 500 lines.
    let limits = TerminalLimits { memory_cap_mb: 1, ..TerminalLimits::default() };
    let h = PtyTerminalHost::with_backend(LoginEnv::inherited(), limits, Arc::new(PortablePty));
    let mut evs = Vec::new();
    for i in 0..4 {
        let id = format!("cap{i}");
        evs.push(sh(&h, &id, "i=0; while [ $i -lt 2500 ]; do echo line $i; i=$((i+1)); done"));
    }
    for e in &evs {
        e.wait_exit(T);
    }
    let st = h.stats();
    for s in &st.sessions {
        assert!(s.history_lines <= 2500);
        assert!(s.history_lines >= 500 || s.history_lines == 0, "{s:?}");
    }
    assert!(st.sessions.iter().any(|s| s.history_lines == 500), "{st:?}");
    let tail = h.text_tail(&SessionId::new("cap0"), 3).unwrap();
    assert!(tail.contains("line 2499"), "{tail}");
}

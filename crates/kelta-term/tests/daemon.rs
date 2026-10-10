//! keltad end to end: sessions keep running across client reconnects, the launcher starts the
//! real binary.

#![allow(clippy::unwrap_used, clippy::expect_used)] // test helpers outside #[test] fns
#![allow(clippy::disallowed_methods)] // allowlisted: tests run keltad on a thread and outwait its grace

mod common;

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use common::*;
use kelta_proto::api::TerminalHost;
use kelta_proto::ids::SessionId;
use kelta_proto::term::{KillSignal, LoginEnv, TerminalLimits};
use kelta_term::PtyTerminalHost;
use kelta_term::daemon::{self, DaemonTerminalHost};

const T: Duration = Duration::from_secs(20);

fn start_daemon(sock: &Path) -> std::thread::JoinHandle<()> {
    let l = daemon::bind(sock).unwrap();
    let host = PtyTerminalHost::new(LoginEnv::inherited(), TerminalLimits::default());
    std::thread::spawn(move || daemon::serve(l, host, Duration::from_millis(200)))
}

fn text(frames: &Frames) -> String {
    frames.take().iter().map(|f| String::from_utf8_lossy(&f[1..]).into_owned()).collect()
}

#[test]
fn attach_after_client_reconnect() {
    let tmp = tempfile::tempdir().unwrap();
    let sock = tmp.path().join("run/keltad.sock");
    let daemon = start_daemon(&sock);
    let id = SessionId::new("s1");

    let a = DaemonTerminalHost::connect(&sock).unwrap();
    let ev_a = Arc::new(Events::default());
    let mut s = spec("s1", "/bin/sh", &["-c", "echo MARK-ONE; exec cat"], 80, 24, ev_a.clone());
    s.env.insert("KELTA_HOOK_TOKEN".into(), "tok".into());
    a.spawn(s).unwrap();
    let view = Frames::default();
    a.attach(&id, 80, 24, Box::new(view.clone())).unwrap();
    let mut seen = String::new();
    wait_until(T, || {
        seen.push_str(&text(&view));
        seen.contains("MARK-ONE").then_some(())
    })
    .expect("output through keltad");
    assert!(a.stats().sessions.iter().any(|s| s.id == id && s.attached));

    // The app quits: the session keeps running in keltad.
    a.close();
    drop(a);
    let b = DaemonTerminalHost::connect(&sock).unwrap();
    assert!(b.persistent());
    let ev_b = Arc::new(Events::default());
    assert!(b.adopt(&SessionId::new("ghost"), ev_b.clone()).unwrap().is_none());
    let env = b.adopt(&id, ev_b.clone()).unwrap().expect("session still running");
    assert_eq!(env.get("KELTA_HOOK_TOKEN").map(String::as_str), Some("tok"));
    let view_b = Frames::default();
    b.attach(&id, 100, 30, Box::new(view_b.clone())).unwrap();
    let snap = text(&view_b);
    assert!(snap.contains("MARK-ONE"), "snapshot after reconnect: {snap:?}");
    b.write(&id, b"MARK-TWO\n").unwrap();
    let mut seen = String::new();
    wait_until(T, || {
        seen.push_str(&text(&view_b));
        seen.contains("MARK-TWO").then_some(())
    })
    .expect("input and output after reconnect");
    assert!(b.text_tail(&id, 10).unwrap().contains("MARK-TWO"));

    // Exit events go to the adopting client only.
    b.kill(&id, KillSignal::Kill).unwrap();
    ev_b.wait_exit(T);
    assert!(ev_a.exited().is_none());
    assert!(b.adopt(&id, ev_b.clone()).unwrap().is_none(), "exited sessions are not adopted");
    b.kill(&id, KillSignal::Kill).unwrap(); // closes it
    assert!(b.stats().sessions.is_empty());

    // No client, no session: keltad leaves after its grace.
    drop(b);
    daemon.join().unwrap();
    assert!(!sock.exists());
}

#[test]
fn running_session_keeps_keltad_alive() {
    let tmp = tempfile::tempdir().unwrap();
    let sock = tmp.path().join("run/keltad.sock");
    let daemon = start_daemon(&sock);
    let a = DaemonTerminalHost::connect(&sock).unwrap();
    a.spawn(spec("s2", "/bin/sh", &["-c", "exec sleep 60"], 80, 24, Arc::new(Events::default()))).unwrap();
    drop(a);
    std::thread::sleep(Duration::from_millis(600));
    assert!(!daemon.is_finished());
    let b = DaemonTerminalHost::connect(&sock).unwrap();
    let ev = Arc::new(Events::default());
    b.adopt(&SessionId::new("s2"), ev.clone()).unwrap().unwrap();
    b.kill(&SessionId::new("s2"), KillSignal::Kill).unwrap();
    ev.wait_exit(T);
    drop(b);
    daemon.join().unwrap();
}

#[test]
fn unreachable_keltad_is_an_adopt_error_not_a_stopped_session() {
    let tmp = tempfile::tempdir().unwrap();
    let sock = tmp.path().join("run/keltad.sock");
    let _daemon = start_daemon(&sock);
    let h = DaemonTerminalHost::connect(&sock).unwrap();
    h.close();
    std::fs::remove_file(&sock).unwrap();
    // core kills what it did not adopt: "could not ask" must not read as "not running".
    assert!(h.adopt(&SessionId::new("s4"), Arc::new(Events::default())).is_err());
}

#[test]
fn launches_the_keltad_binary() {
    let tmp = tempfile::tempdir().unwrap();
    let sock = tmp.path().join("run/keltad.sock");
    let log = tmp.path().join("logs/keltad.log");
    let exe = Path::new(env!("CARGO_BIN_EXE_keltad"));
    let h = DaemonTerminalHost::connect_or_launch(&sock, exe, &log).unwrap();
    let ev = Arc::new(Events::default());
    h.spawn(spec("s3", "/bin/sh", &["-c", "exit 7"], 80, 24, ev.clone())).unwrap();
    assert_eq!(ev.wait_exit(T), (Some(7), None));
    // A second launcher finds the running daemon.
    let again = DaemonTerminalHost::connect_or_launch(&sock, exe, &log).unwrap();
    assert!(again.stats().sessions.iter().any(|s| s.id.as_str() == "s3"));
    assert!(std::fs::read_to_string(&log).unwrap().contains("keltad started"));
    // keltad exits on its own 30 s after both clients and the exited session are gone.
}

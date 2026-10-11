//! keltad end to end: sessions keep running across client reconnects, the launcher starts the
//! real binary.

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use crate::common::*;
use kelta_proto::api::TerminalHost;
use kelta_proto::ids::SessionId;
use kelta_proto::term::{KillSignal, TerminalLimits};
use kelta_term::PtyTerminalHost;
use kelta_term::daemon::{self, DaemonTerminalHost};

const T: Duration = Duration::from_secs(20);

fn start_daemon(sock: &Path) -> std::thread::JoinHandle<()> {
    let l = daemon::bind(sock).unwrap();
    let host = PtyTerminalHost::new(TerminalLimits::default());
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
    let h = DaemonTerminalHost::connect_or_launch(&sock, exe, &log, &log.with_extension("history")).unwrap();
    let ev = Arc::new(Events::default());
    h.spawn(spec("s3", "/bin/sh", &["-c", "exit 7"], 80, 24, ev.clone())).unwrap();
    assert_eq!(ev.wait_exit(T), (Some(7), None));
    // A second launcher finds the running daemon.
    let again =
        DaemonTerminalHost::connect_or_launch(&sock, exe, &log, &log.with_extension("history")).unwrap();
    assert!(again.stats().sessions.iter().any(|s| s.id.as_str() == "s3"));
    assert!(std::fs::read_to_string(&log).unwrap().contains("keltad started"));
    // keltad exits on its own 30 s after both clients and the exited session are gone.
}

const FAKE_SOCK_ENV: &str = "KELTA_TEST_FAKE_KELTAD_SOCK";

/// Set (with `FAKE_SOCK_ENV`) when the fake keltad should write its pidfile like a current keltad.
const FAKE_PIDFILE_ENV: &str = "KELTA_TEST_FAKE_KELTAD_PIDFILE";

/// Not a test: the keltad of another protocol `refused_hello_replaces_the_old_keltad` runs as a
/// process named `keltad`. Like keltad.rs: binds, forks (the parent exits), setsid. Refuses every
/// Hello and keeps the connection open.
#[test]
#[ignore = "child process of refused_hello_replaces_the_old_keltad"]
fn fake_old_keltad() {
    use kelta_proto::error::KeltaError;
    use kelta_term::daemon::{Call, Out, decode, encode, read_msg};
    use std::io::Write;
    let Some(sock) = std::env::var_os(FAKE_SOCK_ENV) else { return };
    let l = daemon::bind(Path::new(&sock)).unwrap();
    // SAFETY: run with --test-threads=1, so no other thread exists; the child only does plain Rust.
    match unsafe { libc::fork() } {
        0 => {}
        _ => return,
    }
    // SAFETY: plain syscall.
    unsafe { libc::setsid() };
    let pid = std::process::id().to_string();
    std::fs::write(Path::new(&sock).with_extension("old"), &pid).unwrap();
    if std::env::var_os(FAKE_PIDFILE_ENV).is_some() {
        std::fs::write(daemon::pidfile(Path::new(&sock)), &pid).unwrap();
    }
    let mut open = Vec::new();
    for mut s in l.incoming().map_while(Result::ok) {
        // The test's readiness probe connects and leaves without a message.
        let Ok(Some(body)) = read_msg(&mut s) else { continue };
        let (call, _) = decode::<Call>(&body).unwrap();
        let reply =
            Out::Reply { seq: call.seq, result: Err(KeltaError::unsupported("keltad speaks protocol 0")) };
        s.write_all(&encode(&reply, &[]).unwrap()).unwrap();
        open.push(s);
    }
}

fn replaces_the_old_keltad(pidfile: bool) {
    let tmp = tempfile::tempdir().unwrap();
    let sock = tmp.path().join("run/keltad.sock");
    let log = tmp.path().join("logs/keltad.log");
    // The app only signals a process named keltad.
    let fake = tmp.path().join("keltad");
    std::os::unix::fs::symlink(std::env::current_exe().unwrap(), &fake).unwrap();
    let mut cmd = std::process::Command::new(&fake);
    cmd.args(["daemon::fake_old_keltad", "--exact", "--ignored", "--test-threads=1"])
        .env(FAKE_SOCK_ENV, &sock);
    if pidfile {
        cmd.env(FAKE_PIDFILE_ENV, "1");
    }
    assert!(cmd.stdout(std::process::Stdio::null()).status().unwrap().success());
    wait_until(T, || std::os::unix::net::UnixStream::connect(&sock).ok()).expect("fake keltad listening");
    // The listener is a forked child: peer credentials name the exited parent.
    // The child writes its pid files after the parent returned.
    let ready = || std::fs::read_to_string(daemon::pidfile(&sock)).ok().filter(|t| !t.is_empty());
    let old_pid = if pidfile {
        wait_until(T, ready).expect("pidfile").parse::<i32>().unwrap()
    } else {
        wait_until(T, || std::fs::read_to_string(sock.with_extension("old")).ok().filter(|t| !t.is_empty()))
            .expect("pid")
            .parse::<i32>()
            .unwrap()
    };

    let exe = Path::new(env!("CARGO_BIN_EXE_keltad"));
    let h = DaemonTerminalHost::connect_or_launch(&sock, exe, &log, &log.with_extension("history")).unwrap();
    // The old keltad was stopped, not left running beside the new one.
    // SAFETY: signal 0 only probes.
    assert!(wait_until(T, || (unsafe { libc::kill(old_pid, 0) } != 0).then_some(())).is_some());
    // The new keltad answers.
    let ev = Arc::new(Events::default());
    h.spawn(spec("s5", "/bin/sh", &["-c", "exit 3"], 80, 24, ev.clone())).unwrap();
    assert_eq!(ev.wait_exit(T), (Some(3), None));
    assert!(std::fs::read_to_string(&log).unwrap().contains("keltad started"));
}

#[test]
fn refused_hello_replaces_the_old_keltad() {
    replaces_the_old_keltad(true);
}

/// A keltad installed before the pidfile existed: found through the socket it holds.
#[test]
fn refused_hello_replaces_an_old_keltad_without_pidfile() {
    replaces_the_old_keltad(false);
}

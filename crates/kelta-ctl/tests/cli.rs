#![allow(clippy::unwrap_used, clippy::expect_used, clippy::disallowed_methods)]
//! kelta-ctl as a black box: `hook` never fails; other commands speak the ctl wire format.

use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::net::UnixListener;
use std::path::Path;
use std::process::{Command, Output, Stdio};

const BIN: &str = env!("CARGO_BIN_EXE_kelta-ctl");

fn tmp() -> tempfile::TempDir {
    tempfile::Builder::new().prefix("klc").tempdir_in("/tmp").unwrap()
}

fn run(args: &[&str], env: &[(&str, &Path)], extra_env: &[(&str, &str)], stdin: &[u8]) -> Output {
    let mut c = Command::new(BIN);
    c.args(args).env_remove("KELTA_SOCK").env_remove("KELTA_SESSION_ID").env_remove("KELTA_HOOK_TOKEN");
    for (k, v) in env {
        c.env(k, v);
    }
    for (k, v) in extra_env {
        c.env(k, v);
    }
    let mut child = c.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap();
    {
        let mut si = child.stdin.take().unwrap();
        // The child may stop reading early (oversized input): ignore EPIPE.
        let _ = si.write_all(stdin);
    }
    child.wait_with_output().unwrap()
}

/// A one-shot fake ctl server: accepts one connection, returns the request line, answers `reply`.
fn fake_server(sock: &Path, reply: &'static str) -> std::thread::JoinHandle<String> {
    let listener = UnixListener::bind(sock).unwrap();
    std::thread::spawn(move || {
        let (s, _) = listener.accept().unwrap();
        let mut r = BufReader::new(s.try_clone().unwrap());
        let mut line = String::new();
        r.read_line(&mut line).unwrap();
        let mut w = s;
        w.write_all(reply.as_bytes()).unwrap();
        w.write_all(b"\n").unwrap();
        line
    })
}

#[test]
fn hook_exits_zero_in_every_failure_mode() {
    let d = tmp();
    let missing = d.path().join("nope.sock");
    let env = [("KELTA_SOCK", missing.as_path())];
    let ids = [("KELTA_SESSION_ID", "s"), ("KELTA_HOOK_TOKEN", "t")];
    // no socket
    let o = run(&["hook"], &env, &ids, br#"{"hook_event_name":"Stop"}"#);
    assert!(o.status.success());
    assert!(o.stdout.is_empty());
    // no env
    assert!(run(&["hook"], &env, &[], br#"{"hook_event_name":"Stop"}"#).status.success());
    // bad input
    assert!(run(&["hook"], &env, &ids, b"{not json").status.success());
    assert!(run(&["hook"], &env, &ids, b"[1,2]").status.success());
    assert!(run(&["hook"], &env, &ids, b"").status.success());
    // oversized stdin (> 1 MiB)
    let big = vec![b' '; 2 * 1024 * 1024];
    assert!(run(&["hook"], &env, &ids, &big).status.success());
    // socket path that is not a socket
    let file = d.path().join("file");
    std::fs::write(&file, "x").unwrap();
    assert!(
        run(&["hook"], &[("KELTA_SOCK", file.as_path())], &ids, br#"{"hook_event_name":"Stop"}"#)
            .status
            .success()
    );
}

#[test]
fn hook_exits_zero_when_server_rejects_or_hangs_up() {
    let d = tmp();
    let sock = d.path().join("c.sock");
    let listener = UnixListener::bind(&sock).unwrap();
    let t = std::thread::spawn(move || {
        let (mut s, _) = listener.accept().unwrap();
        let mut buf = [0u8; 16];
        let _ = s.read(&mut buf);
        // hang up without answering
    });
    let o = run(
        &["hook"],
        &[("KELTA_SOCK", sock.as_path())],
        &[("KELTA_SESSION_ID", "s"), ("KELTA_HOOK_TOKEN", "t")],
        b"{}",
    );
    assert!(o.status.success());
    t.join().unwrap();
}

#[test]
fn hook_relays_the_payload() {
    let d = tmp();
    let sock = d.path().join("c.sock");
    let srv = fake_server(&sock, r#"{"ok":true,"result":null}"#);
    let payload =
        br#"{"hook_event_name":"Notification","notification_type":"idle_prompt","extra":{"a":[1]}}"#;
    let o = run(
        &["hook"],
        &[("KELTA_SOCK", sock.as_path())],
        &[("KELTA_SESSION_ID", "sid-1"), ("KELTA_HOOK_TOKEN", "tok")],
        payload,
    );
    assert!(o.status.success());
    let line: serde_json::Value = serde_json::from_str(&srv.join().unwrap()).unwrap();
    assert_eq!(line["v"], 1);
    assert_eq!(line["cmd"], "hook");
    assert_eq!(line["session"], "sid-1");
    assert_eq!(line["token"], "tok");
    assert_eq!(line["payload"]["notification_type"], "idle_prompt");
    assert_eq!(line["payload"]["extra"]["a"][0], 1);
}

#[test]
fn commands_print_result_or_fail() {
    let d = tmp();
    let sock = d.path().join("c.sock");
    let srv = fake_server(&sock, r#"{"ok":true,"result":{"shown":true}}"#);
    let o = run(&["toggle"], &[("KELTA_SOCK", sock.as_path())], &[], b"");
    assert!(o.status.success());
    assert_eq!(String::from_utf8_lossy(&o.stdout).trim(), r#"{"shown":true}"#);
    assert_eq!(serde_json::from_str::<serde_json::Value>(&srv.join().unwrap()).unwrap()["cmd"], "toggle");

    std::fs::remove_file(&sock).unwrap();
    let srv = fake_server(&sock, r#"{"ok":false,"error":{"code":"invalid_argument","message":"nope"}}"#);
    let o = run(&["emit", "custom.x", "--json", r#"{"n":1}"#], &[("KELTA_SOCK", sock.as_path())], &[], b"");
    assert_eq!(o.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&o.stderr).contains("invalid_argument"));
    let req: serde_json::Value = serde_json::from_str(&srv.join().unwrap()).unwrap();
    assert_eq!(req["payload"]["n"], 1);

    // not running
    let o = run(&["palette"], &[("KELTA_SOCK", d.path().join("none.sock").as_path())], &[], b"");
    assert_eq!(o.status.code(), Some(1));
    // usage errors
    assert_eq!(run(&["emit", "pr.created"], &[], &[], b"").status.code(), Some(2));
    assert_eq!(run(&["frobnicate"], &[], &[], b"").status.code(), Some(2));
}

#[test]
fn start_task_sends_a_scratch_request() {
    let d = tmp();
    let sock = d.path().join("c.sock");
    let srv = fake_server(&sock, r#"{"ok":true,"result":{"branch":"wip/fix-the-login-flake"}}"#);
    let o = run(
        &["start", "--task", "Fix the login flake\nIt fails on CI", "--project", "shop"],
        &[("KELTA_SOCK", sock.as_path())],
        &[],
        b"",
    );
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    assert!(String::from_utf8_lossy(&o.stdout).contains("wip/fix-the-login-flake"));
    let req: serde_json::Value = serde_json::from_str(&srv.join().unwrap()).unwrap();
    assert_eq!(req["cmd"], "start_task");
    assert_eq!(req["task"], "Fix the login flake\nIt fails on CI");
    assert_eq!(req["project"], "shop");
    // usage errors: empty task, task plus a ticket key
    assert_eq!(run(&["start", "--task", " "], &[], &[], b"").status.code(), Some(2));
    assert_eq!(run(&["start", "SHOP-1", "--task", "x"], &[], &[], b"").status.code(), Some(2));
    assert!(String::from_utf8_lossy(&run(&["--help"], &[], &[], b"").stdout).contains("start --task"));
}

#[test]
fn version_works_offline() {
    let d = tmp();
    let o = run(&["version"], &[("KELTA_SOCK", d.path().join("none.sock").as_path())], &[], b"");
    assert!(o.status.success());
    let v: serde_json::Value = serde_json::from_slice(&o.stdout).unwrap();
    assert_eq!(v["ctl"], env!("CARGO_PKG_VERSION"));
    assert!(v["app"].is_null());
    let o = run(&["--version"], &[], &[], b"");
    assert!(String::from_utf8_lossy(&o.stdout).starts_with("kelta-ctl "));
}

const STATUS_JSON: &[u8] =
    br#"{"session_id":"u","cost":{"total_cost_usd":1.84},"context_window":{"used_percentage":72}}"#;

#[test]
fn statusline_relays_and_keeps_the_user_output() {
    let d = tmp();
    let sock = d.path().join("c.sock");
    let listener = UnixListener::bind(&sock).unwrap();
    // statusline does not wait for an answer: only read the frame.
    let srv = std::thread::spawn(move || {
        let mut line = String::new();
        BufReader::new(listener.accept().unwrap().0).read_line(&mut line).unwrap();
        line
    });
    // The user's statusline sees Claude's JSON on stdin and its stdout is passed through unchanged.
    let user = r#"printf '\033[2mOpus\033[0m %s\n' "$(wc -c | tr -d ' ')""#;
    let ids = [("KELTA_SESSION_ID", "sid-1"), ("KELTA_HOOK_TOKEN", "tok"), ("KELTA_USER_STATUSLINE", user)];
    let o = run(&["statusline"], &[("KELTA_SOCK", sock.as_path())], &ids, STATUS_JSON);
    assert!(o.status.success());
    let direct = Command::new("/bin/sh")
        .args(["-c", user])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .and_then(|mut c| {
            c.stdin.take().unwrap().write_all(STATUS_JSON)?;
            c.wait_with_output()
        })
        .unwrap();
    assert_eq!(o.stdout, direct.stdout);
    let line: serde_json::Value = serde_json::from_str(&srv.join().unwrap()).unwrap();
    assert_eq!(line["cmd"], "hook");
    assert_eq!(line["session"], "sid-1");
    assert_eq!(line["token"], "tok");
    assert_eq!(line["payload"]["hook_event_name"], "Status");
    assert_eq!(line["payload"]["cost"]["total_cost_usd"], 1.84);
}

#[test]
fn statusline_without_kelta_or_user_command_prints_nothing() {
    let d = tmp();
    let missing = d.path().join("nope.sock");
    let o = run(&["statusline"], &[("KELTA_SOCK", missing.as_path())], &[], STATUS_JSON);
    assert!(o.status.success());
    assert!(o.stdout.is_empty());
    let o = run(&["statusline"], &[], &[("KELTA_USER_STATUSLINE", "echo mine")], b"not json");
    assert_eq!(o.stdout, b"mine\n");
}

/// Ticket #135: the relay adds < 50 ms to the user's statusline, even when Kelta never answers.
#[test]
fn statusline_round_trip_adds_under_50ms() {
    let d = tmp();
    let sock = d.path().join("c.sock");
    let listener = UnixListener::bind(&sock).unwrap();
    let srv = std::thread::spawn(move || {
        // Accept and hold every connection without answering.
        listener.incoming().take(5).map(|s| s.unwrap()).collect::<Vec<_>>()
    });
    let ids =
        [("KELTA_SESSION_ID", "s"), ("KELTA_HOOK_TOKEN", "t"), ("KELTA_USER_STATUSLINE", "cat >/dev/null")];
    let time = |f: &dyn Fn()| {
        let t = std::time::Instant::now();
        f();
        t.elapsed()
    };
    let mut via = Vec::new();
    let mut direct = Vec::new();
    for _ in 0..5 {
        via.push(time(&|| {
            assert!(
                run(&["statusline"], &[("KELTA_SOCK", sock.as_path())], &ids, STATUS_JSON).status.success()
            );
        }));
        direct.push(time(&|| {
            let mut c =
                Command::new("/bin/sh").args(["-c", "cat >/dev/null"]).stdin(Stdio::piped()).spawn().unwrap();
            c.stdin.take().unwrap().write_all(STATUS_JSON).unwrap();
            c.wait().unwrap();
        }));
    }
    srv.join().unwrap();
    let (via, direct) = (via.iter().min().unwrap(), direct.iter().min().unwrap());
    let added = via.saturating_sub(*direct);
    eprintln!("statusline: {via:?} via kelta-ctl, {direct:?} direct, {added:?} added");
    assert!(added < std::time::Duration::from_millis(50), "statusline relay added {added:?}");
}

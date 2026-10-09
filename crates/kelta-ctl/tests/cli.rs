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

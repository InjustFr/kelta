#![allow(clippy::unwrap_used, clippy::expect_used)]
//! ctl socket: permissions, token checks, frame caps, dispatch.

mod common;

use std::os::unix::fs::PermissionsExt;
use std::path::Path;

use kelta_proto::ErrorCode;
use kelta_proto::ctl::{CTL_MAX_LINE, CtlCommand, CtlResponse};
use kelta_proto::events::bus;
use kelta_proto::ids::SessionId;
use kelta_proto::model::SessionStatus;
use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixStream;

async fn send(path: &Path, line: &[u8]) -> Option<CtlResponse> {
    let mut s = UnixStream::connect(path).await.unwrap();
    // The server may close early (oversized frame); ignore write errors.
    let _ = s.write_all(line).await;
    let _ = s.write_all(b"\n").await;
    let _ = s.shutdown().await;
    let mut r = BufReader::new(s);
    let mut out = String::new();
    r.read_line(&mut out).await.ok()?;
    serde_json::from_str(&out).ok()
}

async fn send_json(path: &Path, v: Value) -> CtlResponse {
    send(path, &serde_json::to_vec(&v).unwrap()).await.expect("response")
}

fn hook_req(sid: &str, token: &str, event: &str) -> Value {
    json!({"v":1,"cmd":"hook","session":sid,"token":token,"payload":{"hook_event_name":event,"session_id":"u"}})
}

#[tokio::test]
async fn socket_and_dir_permissions() {
    let e = common::env();
    let path = e.server.start_ctl().await.unwrap();
    assert_eq!(path, e.tmp.path().join("run").join("ctl.sock"));
    let sock_mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
    let dir_mode = std::fs::metadata(path.parent().unwrap()).unwrap().permissions().mode() & 0o777;
    assert_eq!(sock_mode, 0o600);
    assert_eq!(dir_mode, 0o700);
    // Idempotent.
    assert_eq!(e.server.start_ctl().await.unwrap(), path);
}

#[tokio::test]
async fn too_open_runtime_dir_is_fixed() {
    let e = common::env();
    let run = e.tmp.path().join("run");
    std::fs::create_dir_all(&run).unwrap();
    std::fs::set_permissions(&run, std::fs::Permissions::from_mode(0o755)).unwrap();
    e.server.start_ctl().await.unwrap();
    assert_eq!(std::fs::metadata(&run).unwrap().permissions().mode() & 0o777, 0o700);
}

#[tokio::test]
async fn stale_socket_is_replaced_and_live_one_refused() {
    let e = common::env();
    let path = e.server.start_ctl().await.unwrap();
    // A second server on the same dirs sees a live socket.
    let other = kelta_server::Server::new(
        std::sync::Arc::downgrade(&e.core),
        kelta_proto::dirs::Dirs::under(e.tmp.path()),
    );
    assert_eq!(other.start_ctl().await.unwrap_err().code, ErrorCode::Conflict);
    drop(other);
    // Dropping the first server removes its socket; a stale file left behind is replaced.
    drop(e.server);
    assert!(!path.exists());
    let _stale = std::os::unix::net::UnixListener::bind(&path).unwrap();
    drop(_stale);
    let again = kelta_server::Server::new(
        std::sync::Arc::downgrade(&e.core),
        kelta_proto::dirs::Dirs::under(e.tmp.path()),
    );
    assert_eq!(again.start_ctl().await.unwrap(), path);
}

#[tokio::test]
async fn hook_requires_the_session_token() {
    let e = common::env();
    let path = e.server.start_ctl().await.unwrap();
    let sid = SessionId::new("s-1");
    e.server.register_session(&sid, "tok-123", Some("mcp-1"));

    for (s, t) in [("s-1", "wrong"), ("s-1", ""), ("s-1", "mcp-1"), ("s-2", "tok-123"), ("s-1", "tok-1234")] {
        let r = send_json(&path, hook_req(s, t, "SessionStart")).await;
        assert!(!r.ok);
        assert_eq!(r.error.unwrap().code, ErrorCode::PermissionDenied, "{s}/{t}");
    }
    assert!(e.fake.hooks().is_empty());
    assert!(e.fake.published().is_empty());

    let r = send_json(&path, hook_req("s-1", "tok-123", "SessionStart")).await;
    assert!(r.ok, "{r:?}");
    let hooks = e.fake.hooks();
    assert_eq!(hooks.len(), 1);
    assert_eq!(hooks[0].0, sid);
    assert_eq!(hooks[0].1.status, SessionStatus::Running);
    let names: Vec<String> = e.fake.published().into_iter().map(|b| b.name).collect();
    assert_eq!(names, vec![bus::CLAUDE_HOOK.to_owned()]);

    e.server.unregister_session(&sid);
    let r = send_json(&path, hook_req("s-1", "tok-123", "Stop")).await;
    assert_eq!(r.error.unwrap().code, ErrorCode::PermissionDenied);
    assert_eq!(e.fake.hooks().len(), 1);
}

#[tokio::test]
async fn file_edited_is_published() {
    let e = common::env();
    let path = e.server.start_ctl().await.unwrap();
    e.server.register_session(&SessionId::new("s"), "t", None);
    let req = json!({"v":1,"cmd":"hook","session":"s","token":"t","payload":{
        "hook_event_name":"PostToolUse","tool_name":"Write","tool_input":{"file_path":"/w/a.rs","content":"x"}}});
    assert!(send_json(&path, req).await.ok);
    let ev = e.fake.published().into_iter().find(|b| b.name == bus::CLAUDE_FILE_EDITED).unwrap();
    assert_eq!(ev.payload, json!({"path":"/w/a.rs","tool":"Write"}));
    assert_eq!(ev.session_id, Some(SessionId::new("s")));
    let hook = e.fake.published().into_iter().find(|b| b.name == bus::CLAUDE_HOOK).unwrap();
    assert_eq!(hook.payload["event"], "PostToolUse");
    assert_eq!(hook.payload["matcher_value"], "Write");
    assert_eq!(e.fake.hooks()[0].1.status, SessionStatus::Unknown);
}

#[tokio::test]
async fn oversized_frame_is_rejected() {
    let e = common::env();
    let path = e.server.start_ctl().await.unwrap();
    e.server.register_session(&SessionId::new("s"), "t", None);
    let big = "x".repeat(CTL_MAX_LINE + 10);
    let line = format!(
        r#"{{"v":1,"cmd":"hook","session":"s","token":"t","payload":{{"hook_event_name":"Stop","message":"{big}"}}}}"#
    );
    let r = send(&path, line.as_bytes()).await.expect("error response");
    assert!(!r.ok);
    assert_eq!(r.error.unwrap().code, ErrorCode::InvalidArgument);
    assert!(e.fake.hooks().is_empty());
}

#[tokio::test]
async fn malformed_and_wrong_version_requests() {
    let e = common::env();
    let path = e.server.start_ctl().await.unwrap();
    let r = send(&path, b"not json").await.unwrap();
    assert_eq!(r.error.unwrap().code, ErrorCode::InvalidArgument);
    let r = send_json(&path, json!({"v":2,"cmd":"toggle"})).await;
    assert_eq!(r.error.unwrap().code, ErrorCode::InvalidArgument);
    let r = send_json(&path, json!({"v":1,"cmd":"nope"})).await;
    assert_eq!(r.error.unwrap().code, ErrorCode::InvalidArgument);
    assert!(e.fake.ctl_commands().is_empty());
}

#[tokio::test]
async fn commands_are_dispatched_to_core() {
    let e = common::env();
    let path = e.server.start_ctl().await.unwrap();
    assert!(send_json(&path, json!({"v":1,"cmd":"toggle"})).await.ok);
    assert!(send_json(&path, json!({"v":1,"cmd":"start","ticket":"SHOP-1","project":"shop"})).await.ok);
    assert!(
        send_json(&path, json!({"v":1,"cmd":"emit","name":"custom.build","payload":{"ok":true}})).await.ok
    );
    let r = send_json(&path, json!({"v":1,"cmd":"emit","name":"pr.created","payload":{}})).await;
    assert_eq!(r.error.unwrap().code, ErrorCode::InvalidArgument);
    let r = send_json(&path, json!({"v":1,"cmd":"version"})).await;
    assert!(r.result.unwrap()["version"].is_string());
    let cmds = e.fake.ctl_commands();
    assert_eq!(cmds.len(), 3);
    assert_eq!(cmds[0], CtlCommand::Toggle);
    assert!(matches!(&cmds[2], CtlCommand::Emit { name, .. } if name == "custom.build"));
}

#[tokio::test]
async fn several_requests_per_connection() {
    let e = common::env();
    let path = e.server.start_ctl().await.unwrap();
    let mut s = UnixStream::connect(&path).await.unwrap();
    s.write_all(b"{\"v\":1,\"cmd\":\"toggle\"}\n{\"v\":1,\"cmd\":\"palette\"}\n").await.unwrap();
    let mut r = BufReader::new(s);
    for _ in 0..2 {
        let mut l = String::new();
        r.read_line(&mut l).await.unwrap();
        let resp: CtlResponse = serde_json::from_str(&l).unwrap();
        assert!(resp.ok);
    }
    assert_eq!(e.fake.ctl_commands(), vec![CtlCommand::Toggle, CtlCommand::Palette]);
}

#![allow(clippy::unwrap_used, clippy::expect_used)]
//! Claude IDE bridge: handshake (token, Origin), lock file lifecycle and stale cleanup, tool routing.

mod common;

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use futures::{SinkExt, StreamExt};
use kelta_proto::ids::{SessionId, WorkItemId};
use kelta_proto::samples;
use serde_json::{Value, json};
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::{Error as WsError, Message};

type Ws = tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

fn lock(claude: &Path, port: u16) -> PathBuf {
    claude.join("ide").join(format!("{port}.lock"))
}

async fn connect(port: u16, token: Option<&str>, origin: Option<&str>) -> Result<Ws, u16> {
    let mut req = format!("ws://127.0.0.1:{port}").into_client_request().unwrap();
    if let Some(t) = token {
        req.headers_mut().insert("x-claude-code-ide-authorization", t.parse().unwrap());
    }
    if let Some(o) = origin {
        req.headers_mut().insert("origin", o.parse().unwrap());
    }
    match tokio_tungstenite::connect_async(req).await {
        Ok((ws, _)) => Ok(ws),
        Err(WsError::Http(resp)) => Err(resp.status().as_u16()),
        Err(e) => panic!("{e}"),
    }
}

async fn send(ws: &mut Ws, msg: Value) {
    ws.send(Message::Text(msg.to_string().into())).await.unwrap();
}

async fn recv(ws: &mut Ws) -> Value {
    loop {
        if let Message::Text(t) = ws.next().await.unwrap().unwrap() {
            return serde_json::from_str(t.as_str()).unwrap();
        }
    }
}

async fn call(ws: &mut Ws, id: u64, name: &str, args: Value) -> Value {
    send(ws, json!({"jsonrpc":"2.0","id":id,"method":"tools/call","params":{"name":name,"arguments":args}}))
        .await;
    let v = recv(ws).await;
    assert_eq!(v["id"], id, "{v}");
    v["result"].clone()
}

#[tokio::test]
async fn handshake_needs_the_token_and_no_origin() {
    let e = common::env();
    let claude = e.tmp.path().join("claude");
    let sid = SessionId::new("s1");
    let port = e.server.ide_open(&sid, &claude, vec![e.tmp.path().to_path_buf()]).unwrap();

    let lock_file = lock(&claude, port);
    assert_eq!(std::fs::metadata(&lock_file).unwrap().permissions().mode() & 0o777, 0o600);
    assert_eq!(std::fs::metadata(claude.join("ide")).unwrap().permissions().mode() & 0o777, 0o700);
    let l: Value = serde_json::from_slice(&std::fs::read(&lock_file).unwrap()).unwrap();
    assert_eq!(l["ideName"], "Kelta");
    assert_eq!(l["transport"], "ws");
    assert_eq!(l["pid"], std::process::id());
    assert_eq!(l["workspaceFolders"], json!([e.tmp.path()]));
    let token = l["authToken"].as_str().unwrap().to_owned();
    assert_eq!(token.len(), 32);

    assert_eq!(connect(port, None, None).await.err(), Some(401));
    assert_eq!(connect(port, Some("0".repeat(32).as_str()), None).await.err(), Some(401));
    assert_eq!(connect(port, Some(&token), Some("https://evil.example")).await.err(), Some(403));

    let mut ws = connect(port, Some(&token), None).await.unwrap();
    send(
        &mut ws,
        json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18"}}),
    )
    .await;
    let v = recv(&mut ws).await;
    assert_eq!(v["result"]["protocolVersion"], "2025-06-18");
    // notifications get no answer; the next reply is the tools/list one
    send(&mut ws, json!({"jsonrpc":"2.0","method":"notifications/initialized"})).await;
    send(&mut ws, json!({"jsonrpc":"2.0","id":2,"method":"tools/list"})).await;
    let v = recv(&mut ws).await;
    assert_eq!(v["id"], 2);
    let names: Vec<&str> =
        v["result"]["tools"].as_array().unwrap().iter().map(|t| t["name"].as_str().unwrap()).collect();
    assert!(names.contains(&"openFile") && names.contains(&"openDiff") && names.contains(&"getDiagnostics"));
    let r = call(&mut ws, 3, "getDiagnostics", json!({})).await;
    assert_eq!(r["content"][0]["text"], "[]");
    send(&mut ws, json!({"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"rm_rf"}})).await;
    assert_eq!(recv(&mut ws).await["error"]["code"], -32602);
}

#[tokio::test]
async fn lock_file_lifecycle_and_stale_cleanup() {
    let e = common::env();
    let claude = e.tmp.path().join("claude");
    let ide = claude.join("ide");
    std::fs::create_dir_all(&ide).unwrap();
    // a pid that is certainly gone: a child we already reaped
    let mut child = std::process::Command::new("true").spawn().unwrap();
    let dead = child.id();
    child.wait().unwrap();
    let write = |name: &str, ide_name: &str, pid: u32| {
        let body = json!({"pid": pid, "ideName": ide_name, "transport": "ws", "authToken": "x"});
        std::fs::write(ide.join(name), body.to_string()).unwrap();
    };
    write("1.lock", "Kelta", dead);
    write("2.lock", "Visual Studio Code", dead);
    write("3.lock", "Kelta", std::process::id());
    std::fs::write(ide.join("4.lock"), "not json").unwrap();

    let sid = SessionId::new("s1");
    let port = e.server.ide_open(&sid, &claude, vec![]).unwrap();
    assert!(!ide.join("1.lock").exists(), "stale Kelta lock removed");
    for kept in ["2.lock", "3.lock", "4.lock"] {
        assert!(ide.join(kept).exists(), "{kept} must not be touched");
    }
    assert!(lock(&claude, port).exists());

    // Re-open replaces the previous bridge (and its lock file).
    let port2 = e.server.ide_open(&sid, &claude, vec![]).unwrap();
    assert!(!lock(&claude, port).exists());
    assert!(lock(&claude, port2).exists());

    // Session end: lock removed, listener closed.
    e.server.unregister_session(&sid);
    assert!(!lock(&claude, port2).exists());
    let mut closed = false;
    for _ in 0..100 {
        if tokio::net::TcpStream::connect(("127.0.0.1", port2)).await.is_err() {
            closed = true;
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    assert!(closed, "port {port2} still accepting after close");

    let port3 = e.server.ide_open(&SessionId::new("s2"), &claude, vec![]).unwrap();
    e.server.ide_close_all();
    assert!(!lock(&claude, port3).exists());
}

#[tokio::test]
async fn open_file_and_diff_route_to_the_session_editor() {
    let e = common::env();
    let repo = e.tmp.path().join("repo");
    std::fs::create_dir_all(repo.join("src")).unwrap();
    std::fs::write(repo.join("src/login.rs"), "fn a() {}\n\nfn login() {}\n").unwrap();
    let mut session = samples::session_info();
    session.cwd = repo.clone();
    session.work_item_id = Some(WorkItemId::new("w1"));
    let sid = session.id.clone();
    e.fake.insert_session(session);

    let claude = e.tmp.path().join("claude");
    let port = e.server.ide_open(&sid, &claude, vec![repo.clone()]).unwrap();
    let l: Value = serde_json::from_slice(&std::fs::read(lock(&claude, port)).unwrap()).unwrap();
    let mut ws = connect(port, l["authToken"].as_str(), None).await.unwrap();

    let r = call(&mut ws, 1, "openFile", json!({"filePath":"src/login.rs","startText":"fn login"})).await;
    assert_eq!(r["content"][0]["text"], format!("Opened file: {}", repo.join("src/login.rs").display()));
    let ed = e.fake.calls().into_iter().find(|c| c.method == "editor_open").unwrap();
    assert_eq!(ed.args["target"], json!({"kind":"work_item","id":"w1"}));
    assert_eq!(ed.args["path"], repo.join("src/login.rs").display().to_string());
    assert_eq!(ed.args["line"], 3);

    let r = call(&mut ws, 2, "getWorkspaceFolders", json!({})).await;
    let folders: Value = serde_json::from_str(r["content"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(folders["rootPath"], repo.display().to_string());

    // openDiff blocks until Claude closes the tab.
    let args = json!({"old_file_path": repo.join("src/login.rs"), "new_file_path": repo.join("src/login.rs"),
                      "new_file_contents": "fn login() { todo!() }\n", "tab_name": "✻ [Claude Code] login.rs"});
    send(
        &mut ws,
        json!({"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"openDiff","arguments":args}}),
    )
    .await;
    let opened = loop {
        if let Some(c) = e.fake.calls().into_iter().find(|c| c.method == "editor_diff") {
            break c;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    };
    assert_eq!(opened.args["close"], false);
    assert_eq!(opened.args["target"]["kind"], "work_item");
    let proposed = PathBuf::from(opened.args["proposed"].as_str().unwrap());
    assert!(proposed.starts_with(e.tmp.path()) && proposed.ends_with("login.rs"));
    assert_eq!(std::fs::read_to_string(&proposed).unwrap(), "fn login() { todo!() }\n");

    send(&mut ws, json!({"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"close_tab","arguments":{"tab_name":"✻ [Claude Code] login.rs"}}}))
        .await;
    let (mut closed, mut diff) = (Value::Null, Value::Null);
    for _ in 0..2 {
        let v = recv(&mut ws).await;
        if v["id"] == 4 { closed = v } else { diff = v }
    }
    assert_eq!(closed["result"]["content"][0]["text"], "TAB_CLOSED");
    assert_eq!(diff["id"], 3);
    assert_eq!(diff["result"]["content"][0]["text"], "DIFF_REJECTED");
    assert_eq!(diff["result"]["content"][1]["text"], "✻ [Claude Code] login.rs");
    let closes: Vec<_> = e.fake.calls().into_iter().filter(|c| c.method == "editor_diff").collect();
    assert_eq!(closes.len(), 2);
    assert_eq!(closes[1].args["close"], true);
    assert!(!proposed.exists(), "staged proposal removed");

    // No editor able to diff: openDiff fails at once so Claude falls back to its own prompt.
    e.fake.fail("editor_diff", kelta_proto::error::KeltaError::unsupported("no nvim"));
    let r = call(&mut ws, 5, "openDiff", args).await;
    assert_eq!(r["isError"], true);
}

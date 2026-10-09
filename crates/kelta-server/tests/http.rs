#![allow(clippy::unwrap_used, clippy::expect_used)]
//! Lazy HTTP server: refcount, Host guard, bearer checks, http hooks, MCP over HTTP.

mod common;

use std::sync::Arc;

use kelta_proto::ids::{AccountId, SessionId};
use kelta_proto::model::SessionStatus;
use kelta_proto::samples;
use kelta_proto::testing::{FakeCodeHost, FakeTracker};
use serde_json::{Value, json};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

fn client() -> reqwest::Client {
    reqwest::Client::builder().no_proxy().build().unwrap()
}

/// Raw HTTP/1.1 request (full control of the Host header); returns the status code.
async fn raw_status(port: u16, host: &str, path: &str, auth: Option<&str>, body: &str) -> u16 {
    let mut s = tokio::net::TcpStream::connect(("127.0.0.1", port)).await.unwrap();
    let auth = auth.map(|a| format!("Authorization: Bearer {a}\r\n")).unwrap_or_default();
    let req = format!(
        "POST {path} HTTP/1.1\r\nHost: {host}\r\n{auth}Content-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    s.write_all(req.as_bytes()).await.unwrap();
    let mut out = String::new();
    s.read_to_string(&mut out).await.unwrap();
    out.split_whitespace().nth(1).unwrap().parse().unwrap()
}

async fn mcp(port: u16, sid: &str, token: &str, msg: Value) -> reqwest::Response {
    client()
        .post(format!("http://127.0.0.1:{port}/mcp/{sid}"))
        .bearer_auth(token)
        .header("accept", "application/json, text/event-stream")
        .json(&msg)
        .send()
        .await
        .unwrap()
}

async fn call(port: u16, sid: &str, token: &str, name: &str, args: Value) -> (String, bool) {
    let r = mcp(
        port,
        sid,
        token,
        json!({"jsonrpc":"2.0","id":7,"method":"tools/call","params":{"name":name,"arguments":args}}),
    )
    .await;
    assert_eq!(r.status(), 200);
    let v: Value = r.json().await.unwrap();
    assert_eq!(v["id"], 7);
    let res = &v["result"];
    (res["content"][0]["text"].as_str().unwrap().to_owned(), res["isError"].as_bool().unwrap())
}

#[tokio::test]
async fn server_is_lazy_and_stops_after_last_release() {
    let e = common::env();
    assert_eq!(e.server.http_port(), None);
    let port = e.server.ensure_http().await.unwrap();
    assert_eq!(e.server.ensure_http().await.unwrap(), port);
    e.server.release_http();
    // Still one consumer: reachable.
    assert_eq!(raw_status(port, &format!("127.0.0.1:{port}"), "/hook/x", None, "{}").await, 401);
    e.server.release_http();
    assert_eq!(e.server.http_port(), None);
    let mut closed = false;
    for _ in 0..100 {
        if tokio::net::TcpStream::connect(("127.0.0.1", port)).await.is_err() {
            closed = true;
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    assert!(closed, "port {port} still accepting after the last release");
    // Restart on demand.
    let p2 = e.server.ensure_http().await.unwrap();
    assert_eq!(raw_status(p2, &format!("127.0.0.1:{p2}"), "/hook/x", None, "{}").await, 401);
    e.server.release_http();
}

#[tokio::test]
async fn host_guard_rejects_foreign_hosts() {
    let e = common::env();
    let sid = SessionId::new("s");
    e.server.register_session(&sid, "ht", Some("mt"));
    let port = e.server.ensure_http().await.unwrap();
    let body = r#"{"hook_event_name":"SessionStart"}"#;
    for host in [
        "evil.example",
        &format!("localhost:{port}"),
        &format!("evil.example:{port}"),
        "127.0.0.1",
        "127.0.0.1:1",
    ] {
        assert_eq!(raw_status(port, host, "/hook/s", Some("ht"), body).await, 403, "{host}");
        assert_eq!(
            raw_status(port, host, "/mcp/s", Some("mt"), r#"{"jsonrpc":"2.0","id":1,"method":"ping"}"#).await,
            403
        );
        assert_eq!(raw_status(port, host, "/proxy/abc/", None, "").await, 403);
    }
    assert!(e.fake.hooks().is_empty());
    assert_eq!(raw_status(port, &format!("127.0.0.1:{port}"), "/hook/s", Some("ht"), body).await, 200);
    assert_eq!(e.fake.hooks().len(), 1);
    e.server.release_http();
}

#[tokio::test]
async fn bearer_is_required_per_session() {
    let e = common::env();
    e.server.register_session(&SessionId::new("s"), "ht", Some("mt"));
    e.server.register_session(&SessionId::new("nomcp"), "ht2", None);
    let port = e.server.ensure_http().await.unwrap();
    let host = format!("127.0.0.1:{port}");
    let hook = r#"{"hook_event_name":"Stop"}"#;
    let ping = r#"{"jsonrpc":"2.0","id":1,"method":"ping"}"#;
    // hooks
    assert_eq!(raw_status(port, &host, "/hook/s", None, hook).await, 401);
    assert_eq!(raw_status(port, &host, "/hook/s", Some("mt"), hook).await, 401);
    assert_eq!(raw_status(port, &host, "/hook/other", Some("ht"), hook).await, 401);
    assert_eq!(raw_status(port, &host, "/hook/s", Some("ht"), "not json").await, 400);
    // mcp
    assert_eq!(raw_status(port, &host, "/mcp/s", None, ping).await, 401);
    assert_eq!(raw_status(port, &host, "/mcp/s", Some("ht"), ping).await, 401);
    assert_eq!(raw_status(port, &host, "/mcp/nomcp", Some("ht2"), ping).await, 401);
    assert_eq!(raw_status(port, &host, "/mcp/nomcp", Some(""), ping).await, 401);
    assert_eq!(raw_status(port, &host, "/mcp/s", Some("mt"), ping).await, 200);
    assert!(e.fake.hooks().is_empty());
    // GET (no server stream) needs the token too.
    let r = client().get(format!("http://127.0.0.1:{port}/mcp/s")).send().await.unwrap();
    assert_eq!(r.status(), 401);
    let r = client().get(format!("http://127.0.0.1:{port}/mcp/s")).bearer_auth("mt").send().await.unwrap();
    assert_eq!(r.status(), 405);
    e.server.release_http();
}

#[tokio::test]
async fn http_hook_transport_applies_status() {
    let e = common::env();
    let sid = SessionId::new("s");
    e.server.register_session(&sid, "ht", None);
    let port = e.server.ensure_http().await.unwrap();
    let payload: Value = kelta_proto::testing::fixtures::load_value("hook_stop").unwrap();
    let r = client()
        .post(format!("http://127.0.0.1:{port}/hook/s"))
        .bearer_auth("ht")
        .json(&payload)
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 200);
    let hooks = e.fake.hooks();
    assert_eq!(hooks[0].1.status, SessionStatus::Done);
    assert_eq!(hooks[0].1.preview.as_deref(), Some("Done. All tests pass."));
    e.server.release_http();
}

#[tokio::test]
async fn mcp_initialize_list_and_call() {
    let e = common::env();
    let session = samples::session_info();
    let sid = session.id.clone();
    e.fake.insert_session(session.clone());
    let tracker = Arc::new(FakeTracker::new());
    e.fake.add_tracker(samples::ticket_ref().account, tracker.clone());
    e.fake.add_code_host(AccountId::new("gh-oss"), Arc::new(FakeCodeHost::new()));
    let mut work = samples::work_item();
    work.pr_url = Some("https://github.com/acme/shop/pull/9".into());
    e.fake.add_work_item(work.clone());
    e.server.register_session(&sid, "ht", Some("mt"));
    let port = e.server.ensure_http().await.unwrap();

    // initialize
    let r = mcp(
        port,
        sid.as_str(),
        "mt",
        json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{
        "protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"claude-code","version":"2"}}}),
    )
    .await;
    assert_eq!(r.status(), 200);
    assert!(r.headers()["content-type"].to_str().unwrap().starts_with("application/json"));
    let v: Value = r.json().await.unwrap();
    assert_eq!(v["result"]["protocolVersion"], "2025-06-18");
    assert_eq!(v["result"]["serverInfo"]["name"], "kelta");
    assert!(v["result"]["capabilities"]["tools"].is_object());

    // initialized notification → 202
    let r =
        mcp(port, sid.as_str(), "mt", json!({"jsonrpc":"2.0","method":"notifications/initialized"})).await;
    assert_eq!(r.status(), 202);

    // tools/list
    let v: Value = mcp(port, sid.as_str(), "mt", json!({"jsonrpc":"2.0","id":2,"method":"tools/list"}))
        .await
        .json()
        .await
        .unwrap();
    let names: Vec<&str> =
        v["result"]["tools"].as_array().unwrap().iter().map(|t| t["name"].as_str().unwrap()).collect();
    assert_eq!(
        names,
        [
            "get_ticket",
            "transition_ticket",
            "add_ticket_comment",
            "open_in_editor",
            "create_pr",
            "list_review_requests",
            "notify"
        ]
    );
    for t in v["result"]["tools"].as_array().unwrap() {
        assert_eq!(t["inputSchema"]["type"], "object");
    }

    // unknown method / tool
    let v: Value = mcp(port, sid.as_str(), "mt", json!({"jsonrpc":"2.0","id":3,"method":"nope"}))
        .await
        .json()
        .await
        .unwrap();
    assert_eq!(v["error"]["code"], -32601);
    let v: Value = mcp(
        port,
        sid.as_str(),
        "mt",
        json!({"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"rm_rf"}}),
    )
    .await
    .json()
    .await
    .unwrap();
    assert_eq!(v["error"]["code"], -32602);

    // tools/call
    let (text, err) = call(port, sid.as_str(), "mt", "get_ticket", json!({})).await;
    assert!(!err, "{text}");
    assert!(text.contains("SHOP-142") && text.contains("Rate-limit login"), "{text}");

    let (text, err) = call(port, sid.as_str(), "mt", "transition_ticket", json!({"to":"in_review"})).await;
    assert!(!err, "{text}");
    assert_eq!(tracker.ticket("SHOP-142").unwrap().ticket.status.name, "In Review");
    let (text, err) = call(port, sid.as_str(), "mt", "transition_ticket", json!({"to":"Nowhere"})).await;
    assert!(err && text.contains("no transition"), "{text}");

    let (_, err) =
        call(port, sid.as_str(), "mt", "add_ticket_comment", json!({"markdown":"Fixed in **abc**"})).await;
    assert!(!err);
    assert!(tracker.calls().iter().any(|c| c.starts_with("comment")), "{:?}", tracker.calls());
    let evs = e.fake.published();
    let moved = evs.iter().find(|ev| ev.name == "ticket.transitioned").expect("ticket.transitioned");
    assert_eq!(moved.payload["ticket"]["key"], "SHOP-142");
    assert_eq!(moved.payload["to"]["name"], "In Review");
    assert!(moved.payload["from"]["name"].is_string(), "{:?}", moved.payload);
    assert_eq!(moved.session_id.as_ref(), Some(&sid));
    let commented = evs.iter().find(|ev| ev.name == "ticket.commented").expect("ticket.commented");
    assert_eq!(commented.payload["ticket"]["key"], "SHOP-142");

    let (_, err) =
        call(port, sid.as_str(), "mt", "open_in_editor", json!({"path":"src/login.rs","line":12})).await;
    assert!(!err);
    let ed = e.fake.calls().into_iter().find(|c| c.method == "editor_open").unwrap();
    assert_eq!(ed.args["path"], session.cwd.join("src/login.rs").display().to_string());
    assert_eq!(ed.args["line"], 12);
    assert_eq!(ed.args["target"]["kind"], "work_item");

    let (text, err) =
        call(port, sid.as_str(), "mt", "create_pr", json!({"title":"Rate limit","draft":true})).await;
    assert!(!err && text.contains("pull/9"), "{text}");
    let pr = e.fake.calls().into_iter().find(|c| c.method == "work_create_pr").unwrap();
    assert_eq!(pr.args["draft"]["title"], "Rate limit");
    assert_eq!(pr.args["draft"]["draft"], true);

    let (text, err) = call(port, sid.as_str(), "mt", "list_review_requests", json!({})).await;
    assert!(!err, "{text}");
    assert!(e.fake.call_names().contains(&"review_list"));

    let (_, err) = call(port, sid.as_str(), "mt", "notify", json!({"message":"Tests are green"})).await;
    assert!(!err);
    let n = e.fake.notifications();
    assert_eq!(n[0].body.as_deref(), Some("Tests are green"));
    assert_eq!(n[0].session_id.as_ref(), Some(&sid));

    let (text, err) = call(port, sid.as_str(), "mt", "notify", json!({})).await;
    assert!(err && text.contains("message"));
    e.server.release_http();
}

#[tokio::test]
async fn ticket_tools_without_link() {
    let e = common::env();
    let sid = SessionId::new("plain");
    e.server.register_session(&sid, "ht", Some("mt"));
    let port = e.server.ensure_http().await.unwrap();
    for (tool, args) in [
        ("get_ticket", json!({})),
        ("transition_ticket", json!({"to":"done"})),
        ("add_ticket_comment", json!({"markdown":"x"})),
    ] {
        let (text, err) = call(port, "plain", "mt", tool, args).await;
        assert!(err, "{tool}");
        assert_eq!(text, "no ticket linked");
    }
    let (_, err) = call(port, "plain", "mt", "create_pr", json!({})).await;
    assert!(err);
    e.server.release_http();
}

#[tokio::test]
async fn proxy_is_mounted() {
    let e = common::env();
    let port = e.server.ensure_http().await.unwrap();
    let r = client().get(format!("http://127.0.0.1:{port}/proxy/0123456789abcdef/")).send().await.unwrap();
    // Reaches the kelta-plugins router (501 while it is a stub) past the Host guard.
    assert_ne!(r.status(), 403);
    if r.status() == 501 {
        assert!(r.text().await.unwrap().contains("proxy"));
    }
    e.server.release_http();
}

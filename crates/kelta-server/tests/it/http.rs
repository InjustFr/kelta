//! Lazy HTTP server: refcount, Host guard, bearer checks, http hooks, MCP over HTTP.

use crate::common;

use std::sync::Arc;

use kelta_proto::ids::{AccountId, SessionId};
use kelta_proto::model::SessionStatus;
use kelta_proto::samples;
use kelta_proto_testing::{FakeCodeHost, FakeTracker};
use serde_json::{Value, json};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

fn client() -> reqwest::Client {
    kelta_http::client_builder().no_proxy().build().unwrap()
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
    // A half-sent request must not keep the listener alive (#293).
    let mut half = tokio::net::TcpStream::connect(("127.0.0.1", port)).await.unwrap();
    half.write_all(b"POST /hook/x HTTP/1.1\r\nHost: 127.0.0.1").await.unwrap();
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

/// #499: the server binds the previous run's port again when it is free, else any free port.
#[tokio::test]
async fn ensure_rebinds_the_preferred_port_when_free() {
    let e = common::env();
    let free = std::net::TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let port = free.local_addr().unwrap().port();
    drop(free);
    assert_eq!(e.server.ensure_http_at(Some(port)).await.unwrap(), port);
    assert_eq!(raw_status(port, &format!("127.0.0.1:{port}"), "/hook/x", None, "{}").await, 401);
    e.server.release_http();
    // Taken: another port.
    let taken = std::net::TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let busy = taken.local_addr().unwrap().port();
    assert_ne!(e.server.ensure_http_at(Some(busy)).await.unwrap(), busy);
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
    let payload: Value = kelta_proto_testing::fixtures::load_value("hook_stop").unwrap();
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
            "get_review_feedback",
            "add_review_comment",
            "notify",
            "ask",
            "list_tickets",
            "get_ticket_by_key",
            "transition_ticket_by_key",
            "comment_ticket_by_key",
            "update_ticket_body",
            "close_ticket",
            "move_ticket_step",
            "claim_ticket",
            "report_progress",
            "release_ticket",
            "run_workflow",
            "link_pr",
            "get_setup",
            "add_project",
            "set_setting",
            "test_account",
            "request_token"
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
    assert_eq!(pr.args["origin"], "mcp", "Claude's ship leaves review_due set");

    let (text, err) = call(port, sid.as_str(), "mt", "list_review_requests", json!({})).await;
    assert!(!err, "{text}");
    assert!(e.fake.call_names().contains(&"review_list"));

    e.fake.respond("work_feedback", serde_json::to_value(kelta_proto::samples::feedback()).unwrap());
    let (text, err) = call(port, sid.as_str(), "mt", "get_review_feedback", json!({})).await;
    assert!(!err, "{text}");
    assert!(text.contains("src/login.rs:42") && text.contains("ci / test"), "{text}");
    assert!(e.fake.call_names().contains(&"work_feedback"));

    let (_, err) = call(port, sid.as_str(), "mt", "notify", json!({"message":"Tests are green"})).await;
    assert!(!err);
    let n = e.fake.notifications();
    assert_eq!(n[0].body.as_deref(), Some("Tests are green"));
    assert_eq!(n[0].session_id.as_ref(), Some(&sid));

    let (text, err) = call(port, sid.as_str(), "mt", "notify", json!({})).await;
    assert!(err && text.contains("message"));

    let ask = json!({"question":"Which one?","options":["A","B","C"],"body_md":"**why**"});
    let (text, err) = call(port, sid.as_str(), "mt", "ask", ask).await;
    assert!(!err && text.contains("End your turn"), "{text}");
    let q = e.fake.sessions()[0].claude.clone().unwrap().question.unwrap();
    assert_eq!(q.options, ["A", "B", "C"]);
    let asked = e.fake.calls().into_iter().find(|c| c.method == "session_ask").unwrap();
    assert_eq!(asked.args["body_md"], "**why**");
    for options in [json!([]), json!([""]), json!(["a\rb"]), json!([1])] {
        let (text, err) =
            call(port, sid.as_str(), "mt", "ask", json!({"question":"?","options":options})).await;
        assert!(err && text.contains("options"), "{text}");
    }
    e.server.release_http();
}

#[tokio::test]
async fn add_review_comment_goes_to_the_pending_review_of_the_session_pr() {
    let e = common::env();
    let session = samples::session_info();
    let sid = session.id.clone();
    e.fake.insert_session(session);
    let review = samples::review();
    let host = Arc::new(FakeCodeHost::with_reviews(vec![review.clone()]));
    e.fake.add_code_host(review.r#ref.account.clone(), host.clone());
    let mut work = samples::work_item();
    let repo = tempfile::tempdir().unwrap();
    let git = |args: &[&str]| {
        let ok = std::process::Command::new("git").arg("-C").arg(repo.path()).args(args).status().unwrap();
        assert!(ok.success());
    };
    git(&["init", "-q"]);
    git(&["-c", "user.name=t", "-c", "user.email=t@t", "commit", "-q", "--allow-empty", "-m", "x"]);
    work.worktree = repo.path().to_path_buf();
    work.review = Some(review.r#ref.clone());
    e.fake.add_work_item(work);
    e.server.register_session(&sid, "ht", Some("mt"));
    let port = e.server.ensure_http().await.unwrap();

    // checkout behind the PR head: refused, nothing posted
    let args = json!({"path": "src/prices.rs", "line": 12, "body": "stale"});
    let (text, err) = call(port, sid.as_str(), "mt", "add_review_comment", args).await;
    assert!(err && text.contains("behind"), "{text}");
    assert!(host.pending_comments().is_empty());
    let head = std::process::Command::new("git")
        .arg("-C")
        .arg(repo.path())
        .args(["rev-parse", "HEAD"])
        .output()
        .unwrap();
    host.set_head(&review.r#ref, String::from_utf8(head.stdout).unwrap().trim());

    let args = json!({"path": "src/prices.rs", "line": 12, "body": "cache never expires"});
    let (text, err) = call(port, sid.as_str(), "mt", "add_review_comment", args).await;
    assert!(!err && text.contains("Pending comment"), "{text}");
    let got = host.pending_comments();
    assert_eq!((got[0].0.clone(), got[0].1.as_str(), got[0].2), (review.r#ref, "src/prices.rs", 12));

    let bad = json!({"path": "a.rs", "line": 0, "body": "x"});
    let (text, err) = call(port, sid.as_str(), "mt", "add_review_comment", bad).await;
    assert!(err && text.contains("line"), "{text}");
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
    let args = json!({"path": "a.rs", "line": 1, "body": "x"});
    let (text, err) = call(port, "plain", "mt", "add_review_comment", args).await;
    assert!(err && text.contains("under review"), "{text}");
    e.server.release_http();
}

async fn global(port: u16, token: &str, msg: Value) -> Value {
    let r = client()
        .post(format!("http://127.0.0.1:{port}/mcp"))
        .bearer_auth(token)
        .json(&msg)
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 200);
    r.json().await.unwrap()
}

async fn global_call(port: u16, name: &str, args: Value) -> (String, bool) {
    let v = global(
        port,
        "it",
        json!({"jsonrpc":"2.0","id":7,"method":"tools/call","params":{"name":name,"arguments":args}}),
    )
    .await;
    let res = &v["result"];
    (
        res["content"][0]["text"].as_str().unwrap_or_default().to_owned(),
        res["isError"].as_bool().unwrap_or(true),
    )
}

async fn global_tools(port: u16) -> Vec<String> {
    let v = global(port, "it", json!({"jsonrpc":"2.0","id":2,"method":"tools/list"})).await;
    v["result"]["tools"].as_array().unwrap().iter().map(|t| t["name"].as_str().unwrap().to_owned()).collect()
}

#[tokio::test]
async fn global_mcp_takes_the_install_token_only() {
    let e = common::env();
    e.server.register_session(&SessionId::new("s"), "ht", Some("mt"));
    let port = e.server.ensure_http().await.unwrap();
    let host = format!("127.0.0.1:{port}");
    let ping = r#"{"jsonrpc":"2.0","id":1,"method":"ping"}"#;
    // Closed until installed.
    assert_eq!(raw_status(port, &host, "/mcp", Some("it"), ping).await, 401);
    e.server.set_install_token(Some("it"));
    for bad in [None, Some("wrong"), Some("mt"), Some("ht"), Some("")] {
        assert_eq!(raw_status(port, &host, "/mcp", bad, ping).await, 401, "{bad:?}");
    }
    assert_eq!(raw_status(port, &host, "/mcp", Some("it"), ping).await, 200);
    assert_eq!(raw_status(port, &host, "/mcp/s", Some("it"), ping).await, 401);
    // Remove revokes it.
    e.server.set_install_token(None);
    assert_eq!(raw_status(port, &host, "/mcp", Some("it"), ping).await, 401);
    e.server.release_http();
}

#[tokio::test]
async fn global_mcp_serves_the_board_tools() {
    let e = common::env();
    let tracker = Arc::new(FakeTracker::new());
    e.fake.add_tracker(samples::ticket_ref().account, tracker.clone());
    e.server.set_install_token(Some("it"));
    let port = e.server.ensure_http().await.unwrap();
    let account = samples::ticket_ref().account.to_string();

    // No session tools without a session.
    assert_eq!(
        global_tools(port).await,
        [
            "list_review_requests",
            "list_tickets",
            "get_ticket_by_key",
            "transition_ticket_by_key",
            "comment_ticket_by_key",
            "update_ticket_body",
            "close_ticket",
            "move_ticket_step",
            "claim_ticket",
            "report_progress",
            "release_ticket",
            "run_workflow",
            "link_pr",
            "get_setup",
            "add_project",
            "set_setting",
            "test_account",
            "request_token"
        ]
    );
    let v = global(
        port,
        "it",
        json!({"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"get_ticket"}}),
    )
    .await;
    assert_eq!(v["error"]["code"], -32602);
    let (text, err) = global_call(port, "list_review_requests", json!({})).await;
    assert!(!err, "{text}");

    // A stale revision writes nothing.
    let (text, err) =
        global_call(port, "get_ticket_by_key", json!({"key":"SHOP-141","account":account})).await;
    assert!(!err, "{text}");
    let rev = text.lines().find_map(|l| l.strip_prefix("Revision: ")).unwrap().to_owned();
    // A key without its account: found in Kelta's lists by key, never through the enriched list.
    let r141 = kelta_proto::tracker::TicketRef {
        account: samples::ticket_ref().account,
        key: "SHOP-141".into(),
        id: "SHOP-141".into(),
    };
    let item = kelta_proto::tracker::TicketItem {
        ticket: kelta_proto::tracker::Ticket { r#ref: r141, ..samples::ticket() },
        ..Default::default()
    };
    e.fake.respond("ticket_by_key", serde_json::to_value(Some(item)).unwrap());
    let (text, err) = global_call(port, "get_ticket_by_key", json!({"key":"SHOP-141"})).await;
    assert!(!err && text.contains(&format!("Revision: {rev}")), "{text}");
    assert!(!e.fake.call_names().contains(&"ticket_list"), "{:?}", e.fake.call_names());
    let (text, err) = global_call(
        port,
        "update_ticket_body",
        json!({"key":"SHOP-141","account":account,"body":"- [x] done","revision":"0000000000000000"}),
    )
    .await;
    assert!(err && text.starts_with("conflict"), "{text}");
    assert!(!tracker.calls().iter().any(|c| c.starts_with("set_body")));
    assert_eq!(tracker.ticket("SHOP-141").unwrap().body_md, "Body of SHOP-141");
    let (text, err) = global_call(
        port,
        "update_ticket_body",
        json!({"key":"SHOP-141","account":account,"body":"- [x] done","revision":rev}),
    )
    .await;
    assert!(!err, "{text}");
    assert_eq!(tracker.ticket("SHOP-141").unwrap().body_md, "- [x] done");

    let (text, err) = global_call(
        port,
        "transition_ticket_by_key",
        json!({"key":"SHOP-141","account":account,"to":"in_progress"}),
    )
    .await;
    assert!(!err && text == "SHOP-141 is now In Progress.", "{text}");
    let (text, err) =
        global_call(port, "close_ticket", json!({"key":"SHOP-141","account":account,"comment":"Merged"}))
            .await;
    assert!(!err && text == "SHOP-141 is now Done.", "{text}");
    assert!(tracker.calls().contains(&"comment:SHOP-141".to_owned()));

    // Read-only board tools when board writes are off.
    let mut s = kelta_proto::settings::Settings::defaults();
    s.claude.mcp_board_writes = false;
    e.fake.set_settings(s);
    assert_eq!(
        global_tools(port).await,
        [
            "list_review_requests",
            "list_tickets",
            "get_ticket_by_key",
            "claim_ticket",
            "report_progress",
            "release_ticket",
            "link_pr",
            "get_setup",
            "add_project",
            "set_setting",
            "test_account",
            "request_token"
        ]
    );
    let v = global(
        port,
        "it",
        json!({"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"close_ticket","arguments":{"key":"SHOP-142"}}}),
    )
    .await;
    assert_eq!(v["error"]["code"], -32602);
    e.server.release_http();
}

#[tokio::test]
async fn a_holder_re_claims_an_unlisted_ticket_by_key() {
    let e = common::env();
    e.server.set_install_token(Some("it"));
    let port = e.server.ensure_http().await.unwrap();
    let held = |account: &str, state| {
        let mut w = samples::work_item();
        w.ticket = Some(kelta_proto::tracker::TicketRef {
            account: AccountId::new(account),
            key: "NEW-1".into(),
            id: "9001".into(),
        });
        w.state = state;
        w
    };
    let args = json!({"key":"NEW-1","worktree":"/wt/new-1","branch":"feat/new-1"});

    // No open item: the account is still asked for.
    e.fake.add_work_item(held("gh", kelta_proto::model::WorkState::Finished));
    let (text, err) = global_call(port, "claim_ticket", args.clone()).await;
    assert!(err && text.contains("pass its `account`"), "{text}");

    // The holder's item gives the full ref and its project; the re-claim names the new worktree.
    e.fake.add_work_item(held("gh", kelta_proto::model::WorkState::Active));
    e.fake.respond("work_claim", serde_json::to_value(samples::work_item()).unwrap());
    let (text, err) = global_call(port, "claim_ticket", args.clone()).await;
    assert!(!err && text.starts_with("Claimed NEW-1"), "{text}");
    let call = e.fake.calls().into_iter().find(|c| c.method == "work_claim").unwrap();
    assert_eq!(call.args["ticket"]["id"], "9001");
    assert_eq!(call.args["ticket"]["account"], "gh");
    assert_eq!(call.args["project"], "shop");
    assert_eq!(call.args["worktree"], "/wt/new-1");

    // Open items of two accounts: ask for the account, which then picks one.
    e.fake.add_work_item(held("other", kelta_proto::model::WorkState::Active));
    let (text, err) = global_call(port, "claim_ticket", args.clone()).await;
    assert!(err && text.contains("pass its `account`"), "{text}");
    let (_, err) = global_call(port, "claim_ticket", json!({"key":"NEW-1","account":"other"})).await;
    assert!(!err);
    e.server.release_http();
}

#[tokio::test]
async fn list_tickets_filters_by_local_tag() {
    let e = common::env();
    e.server.set_install_token(Some("it"));
    let port = e.server.ensure_http().await.unwrap();
    let item = |key: &str| {
        let mut ticket = samples::ticket();
        ticket.r#ref.key = key.into();
        json!({ "ticket": ticket, "project_ids": [] })
    };
    e.fake.respond(
        "ticket_list",
        json!({ "items": [item("A-1"), item("A-2")], "next": null, "stale": false, "errors": [] }),
    );
    let account = samples::ticket().r#ref.account;
    e.fake.respond(
        "local_tags",
        json!({ "tags": [{ "name": "Retest", "color": null }], "tickets": { format!("{account}:A-2"): ["Retest"] }, "prs": {} }),
    );
    let keys = |text: String| -> Vec<String> {
        let v: Value = serde_json::from_str(&text).unwrap();
        v["tickets"].as_array().unwrap().iter().map(|t| t["key"].as_str().unwrap().to_owned()).collect()
    };
    let (text, err) = global_call(port, "list_tickets", json!({ "tag": "retest" })).await;
    assert!(!err, "{text}");
    assert_eq!(keys(text), ["A-2"]);
    let (text, _) = global_call(port, "list_tickets", json!({})).await;
    assert_eq!(keys(text), ["A-1", "A-2"]);
    e.server.release_http();
}

#[tokio::test]
async fn list_tickets_free_hides_claimed_and_blocked() {
    let e = common::env();
    e.server.set_install_token(Some("it"));
    let port = e.server.ensure_http().await.unwrap();
    let item = |key: &str, blocked: bool, work: Option<&str>| {
        let mut ticket = samples::ticket();
        ticket.r#ref.key = key.into();
        ticket.blocked = blocked;
        json!({ "ticket": ticket, "project_ids": [], "work_item_id": work })
    };
    e.fake.respond(
        "ticket_list",
        json!({
            "items": [item("F-1", false, None), item("F-2", true, None), item("F-3", false, Some("w-3"))],
            "next": null, "stale": false, "errors": []
        }),
    );
    let rows = |text: String| -> Vec<Value> {
        serde_json::from_str::<Value>(&text).unwrap()["tickets"].as_array().unwrap().clone()
    };
    let (text, err) = global_call(port, "list_tickets", json!({ "free": true })).await;
    assert!(!err, "{text}");
    let free = rows(text);
    assert_eq!(free.len(), 1);
    assert_eq!((free[0]["key"].as_str(), free[0]["blocked"].as_bool()), (Some("F-1"), Some(false)));
    assert!(free[0]["work_item"].is_null());
    let (text, _) = global_call(port, "list_tickets", json!({})).await;
    let all = rows(text);
    assert_eq!((all[1]["blocked"].as_bool(), all[2]["work_item"].as_str()), (Some(true), Some("w-3")));
    e.server.release_http();
}

#[tokio::test]
async fn link_pr_resolves_my_pr_and_passes_conflicts_on() {
    let e = common::env();
    e.server.set_install_token(Some("it"));
    let port = e.server.ensure_http().await.unwrap();
    let account = samples::ticket_ref().account;
    let mut pr = samples::review();
    pr.kind = kelta_proto::codehost::ReviewKind::Authored;
    let url = pr.url.clone();
    let number = pr.r#ref.number;
    e.fake.respond(
        "review_list",
        json!([{ "review": pr, "project_ids": [samples::project_info().id], "waiting": [] }]),
    );
    let mut w = samples::work_item();
    w.branch = pr.source_branch.clone();
    e.fake.respond("work_link_pr", json!([w, true]));
    let link =
        |pr: Value| global_call(port, "link_pr", json!({ "key": "SHOP-151", "account": account, "pr": pr }));

    let (text, err) = link(json!(url)).await;
    assert!(!err && text.starts_with(&format!("Linked SHOP-151 to {url}: work item {}", w.id)), "{text}");
    let call = e.fake.calls().into_iter().rfind(|c| c.method == "work_link_pr").unwrap();
    assert_eq!(
        (call.args["pr"].as_str(), call.args["ticket"]["key"].as_str()),
        (Some(url.as_str()), Some("SHOP-151"))
    );
    assert_eq!(call.args["project"], json!(samples::project_info().id));
    let (text, err) = link(json!(format!("#{number}"))).await;
    assert!(!err, "by number: {text}");

    e.fake.respond("work_link_pr", json!([w, false]));
    let (text, err) = link(json!(url)).await;
    assert!(!err && text.starts_with("Linked already: no change. SHOP-151"), "{text}");
    assert!(text.contains(&w.id.to_string()), "{text}");

    let (text, err) = link(json!("https://github.com/acme/shop-api/pull/1")).await;
    assert!(err && text.starts_with("NotFound:"), "{text}");
    e.fake.fail("work_link_pr", kelta_proto::error::KeltaError::conflict("SHOP-151 is held by work item w1"));
    let (text, err) = link(json!(url)).await;
    assert!(err && text == "Conflict: SHOP-151 is held by work item w1", "{text}");
    e.server.release_http();
}

#[tokio::test]
async fn link_pr_in_a_session_uses_the_session_project() {
    let e = common::env();
    let session = samples::session_info();
    let (sid, mine) = (session.id.clone(), session.project_id.clone());
    e.fake.insert_session(session);
    e.server.register_session(&sid, "ht", Some("mt"));
    let port = e.server.ensure_http().await.unwrap();
    let account = samples::ticket_ref().account;
    let mut pr = samples::review();
    pr.kind = kelta_proto::codehost::ReviewKind::Authored;
    let url = pr.url.clone();
    e.fake.respond("review_list", json!([{ "review": pr, "project_ids": ["other", mine], "waiting": [] }]));
    e.fake.respond("work_link_pr", json!([samples::work_item(), true]));
    let args = json!({ "key": "SHOP-151", "account": account, "pr": url });
    let (text, err) = call(port, sid.as_str(), "mt", "link_pr", args).await;
    assert!(!err, "{text}");
    let c = e.fake.calls().into_iter().rfind(|c| c.method == "work_link_pr").unwrap();
    assert_eq!(c.args["project"], json!(mine));
    e.server.release_http();
}

#[tokio::test]
async fn link_pr_moves_the_ticket_to_the_status_given() {
    let e = common::env();
    let tracker = Arc::new(FakeTracker::new());
    e.fake.add_tracker(samples::ticket_ref().account, tracker.clone());
    e.fake.add_project(samples::project_info());
    e.server.set_install_token(Some("it"));
    let port = e.server.ensure_http().await.unwrap();
    let account = samples::ticket_ref().account;
    let mut pr = samples::review();
    pr.kind = kelta_proto::codehost::ReviewKind::Authored;
    let url = pr.url.clone();
    e.fake.respond(
        "review_list",
        json!([{ "review": pr, "project_ids": [samples::project_info().id], "waiting": [] }]),
    );
    // A repeat call on a linked PR returns the same item: the fake answers every call alike.
    e.fake.respond("work_link_pr", json!([samples::work_item(), true]));
    let link = |status: Option<&str>| {
        let mut args = json!({ "key": "SHOP-141", "account": account, "pr": url });
        if let Some(s) = status {
            args["status"] = json!(s);
        }
        global_call(port, "link_pr", args)
    };
    let status = || tracker.ticket("SHOP-141").unwrap().ticket.status.name;
    let moves = || tracker.calls().iter().filter(|c| c.starts_with("transition:")).count();

    let (text, err) = link(None).await;
    assert!(!err && text.starts_with("Linked SHOP-141"), "{text}");
    assert_eq!((status().as_str(), moves()), ("To Do", 0), "no status: the ticket stays");

    // A literal status name goes to the tracker transition as is.
    let (text, err) = link(Some("In Progress")).await;
    assert!(!err && text.ends_with("SHOP-141 is now In Progress."), "{text}");
    assert!(tracker.calls().contains(&"transition:SHOP-141:t3".to_owned()));

    // A failed move keeps the link and warns; the retry moves it.
    tracker.fail_next(kelta_proto::error::KeltaError::upstream("tracker down"));
    let (text, err) = link(Some("review")).await;
    assert!(!err && text.starts_with("Linked SHOP-141"), "{text}");
    assert!(text.contains("Warning: SHOP-141 not moved to \"review\": tracker down"), "{text}");
    assert_eq!(status(), "In Progress");

    // `review` is the project's status_map.review ("In Review").
    let (text, err) = link(Some("review")).await;
    assert!(!err && text.ends_with("SHOP-141 is now In Review."), "{text}");
    assert_eq!(e.fake.calls().iter().filter(|c| c.method == "work_link_pr").count(), 4);

    // Board writes off: `status` is refused before anything is linked or moved.
    let mut s = kelta_proto::settings::Settings::defaults();
    s.claude.mcp_board_writes = false;
    e.fake.set_settings(s);
    let before = (moves(), status());
    let (text, err) = link(Some("Done")).await;
    assert!(err && text.starts_with("InvalidArgument: `status` needs claude.mcp_board_writes"), "{text}");
    assert_eq!((moves(), status()), before);
    assert_eq!(e.fake.calls().iter().filter(|c| c.method == "work_link_pr").count(), 4);
    let (text, err) = link(None).await;
    assert!(!err && text.starts_with("Linked SHOP-141"), "{text}");
    e.server.release_http();
}

#[tokio::test]
async fn move_ticket_step_passes_the_step_and_names_an_unknown_one() {
    let e = common::env();
    e.server.set_install_token(Some("it"));
    let port = e.server.ensure_http().await.unwrap();
    let account = samples::ticket_ref().account;
    let row = kelta_proto::model::TicketStep {
        ticket: samples::ticket_ref(),
        source_id: None,
        step_id: "po".into(),
        entered_at: String::new(),
        by: "manual".into(),
    };
    e.fake.respond("ticket_step_set", serde_json::to_value(row).unwrap());
    let go = |args: Value| global_call(port, "move_ticket_step", args);

    let (text, err) = go(json!({ "key": "SHOP-141", "account": account, "step": "po" })).await;
    assert!(!err && text == "SHOP-141 is now at step po.", "{text}");
    let call = e.fake.calls().into_iter().rfind(|c| c.method == "ticket_step_set").unwrap();
    assert_eq!(
        (call.args["ticket"]["key"].as_str(), call.args["step_id"].as_str()),
        (Some("SHOP-141"), Some("po"))
    );

    let unknown = "unknown step `nope`; the steps are implementing, pr, po";
    e.fake.fail("ticket_step_set", kelta_proto::error::KeltaError::invalid(unknown));
    let (text, err) = go(json!({ "key": "SHOP-141", "account": account, "step": "nope" })).await;
    assert!(err && text == format!("InvalidArgument: {unknown}"), "{text}");
    let (text, err) = go(json!({ "key": "SHOP-141", "account": account })).await;
    assert!(err && text.contains("missing argument `step`"), "{text}");
    e.server.release_http();
}

#[tokio::test]
async fn global_mcp_sets_kelta_up() {
    let e = common::env();
    e.server.set_install_token(Some("it"));
    let port = e.server.ensure_http().await.unwrap();
    let setup = || async {
        let (text, err) = global_call(port, "get_setup", json!({})).await;
        assert!(!err, "{text}");
        serde_json::from_str::<Value>(&text).unwrap()
    };

    // A fresh install: everything is missing.
    assert!(setup().await["missing"][0].as_str().unwrap().starts_with("No project"));

    let mut s = kelta_proto::settings::Settings::defaults();
    let jira =
        serde_json::from_value(json!({"kind": "jira", "base_url": "https://acme.atlassian.net"})).unwrap();
    s.accounts.insert(AccountId::new("jira"), jira);
    e.fake.set_settings(s);
    let mut p = samples::project_info();
    p.tracker = None;
    e.fake.add_project(p.clone());
    let v = setup().await;
    let missing: Vec<&str> = v["missing"].as_array().unwrap().iter().filter_map(Value::as_str).collect();
    assert!(missing.contains(&"Account jira has no secret reference."), "{missing:?}");
    assert!(missing.contains(&"Account jira feeds no ticket source."), "{missing:?}");
    assert!(missing.contains(&format!("Project {} has no ticket source.", p.id).as_str()), "{missing:?}");
    assert_eq!(v["projects"][0]["id"], json!(p.id));

    // Writes go through core's validated settings write, on the layer asked for.
    let (text, err) =
        global_call(port, "set_setting", json!({"path": "accounts.jira.secret", "value": "keyring:jira"}))
            .await;
    assert!(!err, "{text}");
    let (_, err) =
        global_call(port, "set_setting", json!({"path": "x", "value": 1, "layer": "project"})).await;
    assert!(err, "the project layer needs a project");
    let args = json!({"path": "worktree.setup", "value": [], "layer": "project", "project": p.id});
    let (_, err) = global_call(port, "set_setting", args).await;
    assert!(!err);
    let writes: Vec<Value> =
        e.fake.calls().into_iter().filter(|c| c.method == "settings_write").map(|c| c.args).collect();
    assert_eq!(
        writes[0],
        json!({"layer": "global", "project": null, "path": "accounts.jira.secret", "value": "keyring:jira"})
    );
    assert_eq!((&writes[1]["layer"], &writes[1]["project"]), (&json!("project"), &json!(p.id)));
    e.fake.fail(
        "settings_write",
        kelta_proto::error::KeltaError::invalid("accounts.jira.secret: expected a reference"),
    );
    let (text, err) =
        global_call(port, "set_setting", json!({"path": "accounts.jira.secret", "value": "x"})).await;
    assert!(err && text.starts_with("InvalidArgument:"), "{text}");

    // The token is typed in Kelta: a toast opening Settings → Accounts.
    let (_, err) = global_call(port, "request_token", json!({"account": "nope"})).await;
    assert!(err);
    let (_, err) = global_call(port, "request_token", json!({"account": "jira"})).await;
    assert!(!err);
    let action = e.fake.toasts().pop().unwrap().action.unwrap();
    assert_eq!(
        (action.command.as_str(), action.args),
        ("settings.open", Some(json!({"section": "accounts"})))
    );

    e.fake.respond("account_test", json!({"ok": true, "user": samples::user(), "error": null}));
    let (text, err) = global_call(port, "test_account", json!({"account": "jira"})).await;
    assert!(!err && text.starts_with("jira: signed in as"), "{text}");
    e.server.release_http();
}

#[tokio::test]
async fn run_workflow_tool_starts_a_ticket_run_once() {
    let e = common::env();
    let session = samples::session_info();
    let sid = session.id.clone();
    e.fake.insert_session(session.clone());
    e.server.register_session(&sid, "ht", Some("mt"));
    e.server.set_install_token(Some("it"));
    let port = e.server.ensure_http().await.unwrap();
    let account = samples::ticket_ref().account;
    let args = json!({ "workflow": "kelta-ticket-pass", "key": "SHOP-141", "account": account });
    let (text, err) = call(port, sid.as_str(), "mt", "run_workflow", args.clone()).await;
    assert!(!err, "{text}");
    let v: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(
        (&v["session_id"], &v["name"], &v["running"]),
        (&json!(session.id), &json!(session.name), &json!(false))
    );
    let starts = || e.fake.calls().into_iter().filter(|c| c.method == "workflow_start").collect::<Vec<_>>();
    let c = starts().pop().unwrap();
    assert_eq!(
        (c.args["workflow"].as_str(), c.args["ticket"]["key"].as_str()),
        (Some("kelta-ticket-pass"), Some("SHOP-141"))
    );
    // A live run on the ticket comes back (core's workflow_run dedupes; the fake answers as it does).
    e.fake.respond("workflow_start", json!({ "session": session, "running": true }));
    let (text, err) = call(port, sid.as_str(), "mt", "run_workflow", args).await;
    assert!(!err && serde_json::from_str::<Value>(&text).unwrap()["running"] == true, "{text}");
    // No key: the global entry in the session's project, else in `project`.
    let (_, err) = call(port, sid.as_str(), "mt", "run_workflow", json!({ "workflow": "daily" })).await;
    assert!(!err);
    let c = starts().pop().unwrap();
    assert_eq!(c.args["project"], json!(session.project_id));
    assert!(c.args["ticket"].is_null());
    let (_, err) =
        global_call(port, "run_workflow", json!({ "workflow": "daily", "project": "other" })).await;
    assert!(!err);
    assert_eq!(starts().pop().unwrap().args["project"], "other");
    let (text, err) = global_call(port, "run_workflow", json!({ "workflow": "daily" })).await;
    assert!(err && text.starts_with("InvalidArgument:"), "{text}");
    assert_eq!(starts().len(), 4);
    e.server.release_http();
}

#[tokio::test]
async fn run_workflow_tool_refuses_over_max_live() {
    let e = common::env();
    e.fake.insert_session(samples::session_info()); // one live Claude
    e.server.set_install_token(Some("it"));
    let port = e.server.ensure_http().await.unwrap();
    let mut s = kelta_proto::settings::Settings::defaults();
    s.claude.max_live = 1;
    e.fake.set_settings(s.clone());
    let args = json!({ "workflow": "daily", "project": "shop" });
    let (text, err) = global_call(port, "run_workflow", args.clone()).await;
    assert!(err && text == "Busy: 1 of 1 Claude slots in use", "{text}");
    assert!(!e.fake.call_names().contains(&"workflow_start"), "nothing started");
    s.claude.max_live = 0; // 0 = no cap
    e.fake.set_settings(s.clone());
    let (text, err) = global_call(port, "run_workflow", args.clone()).await;
    assert!(!err, "{text}");
    // Hidden and refused with board writes off.
    s.claude.mcp_board_writes = false;
    e.fake.set_settings(s);
    assert!(!global_tools(port).await.iter().any(|t| t == "run_workflow"));
    let (_, err) = global_call(port, "run_workflow", args).await;
    assert!(err);
    e.server.release_http();
}

#[tokio::test]
async fn run_workflow_returns_a_live_run_at_the_cap() {
    let e = common::env();
    let session = samples::session_info(); // the run's live Claude fills the only slot
    e.fake.insert_session(session.clone());
    let run = kelta_proto::model::RunClaim { session_id: Some(session.id.clone()), ..Default::default() }; // state Running
    e.fake.add_work_item(kelta_proto::model::WorkItem { run: Some(run), ..samples::work_item() }); // holds SHOP-142
    e.server.set_install_token(Some("it"));
    let port = e.server.ensure_http().await.unwrap();
    let mut s = kelta_proto::settings::Settings::defaults();
    s.claude.max_live = 1;
    e.fake.set_settings(s);
    let account = samples::ticket_ref().account;
    let (text, err) = global_call(
        port,
        "run_workflow",
        json!({ "workflow": "kelta-ticket-pass", "key": "SHOP-142", "account": account }),
    )
    .await;
    assert!(!err, "{text}");
    let v: Value = serde_json::from_str(&text).unwrap();
    assert_eq!((&v["session_id"], &v["running"]), (&json!(session.id), &json!(true)));
    assert!(!e.fake.call_names().contains(&"workflow_start"), "nothing started");
    // Another ticket still meets the cap.
    let (text, err) = global_call(
        port,
        "run_workflow",
        json!({ "workflow": "kelta-ticket-pass", "key": "SHOP-999", "account": account }),
    )
    .await;
    assert!(err && text == "Busy: 1 of 1 Claude slots in use", "{text}");
    e.server.release_http();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn parallel_run_workflow_calls_respect_max_live() {
    let e = common::env();
    // One live Claude whose id differs from the one the fake's start inserts (samples::session_info()).
    e.fake.insert_session(kelta_proto::model::SessionInfo {
        id: SessionId::new("other"),
        ..samples::session_info()
    });
    e.server.set_install_token(Some("it"));
    let port = e.server.ensure_http().await.unwrap();
    let mut s = kelta_proto::settings::Settings::defaults();
    s.claude.max_live = 2; // one slot left
    e.fake.set_settings(s);
    let args = json!({ "workflow": "daily", "project": "shop" });
    let (a, b) = tokio::join!(
        global_call(port, "run_workflow", args.clone()),
        global_call(port, "run_workflow", args)
    );
    let mut errs: Vec<_> = [a, b].into_iter().filter(|(_, err)| *err).map(|(t, _)| t).collect();
    assert_eq!(errs.pop().as_deref(), Some("Busy: 2 of 2 Claude slots in use"));
    assert!(errs.is_empty(), "one run starts");
    assert_eq!(e.fake.call_names().iter().filter(|m| **m == "workflow_start").count(), 1);
    e.server.release_http();
}

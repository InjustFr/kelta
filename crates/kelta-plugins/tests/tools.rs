//! Tools: registry merge + namespacing, `tool_check`, PTY tools through `CoreApi::session_spawn`,
//! web tools (free port, stdout readiness, timeout, early exit, port_open, proxy embed, lifecycle).

#![allow(clippy::unwrap_used, clippy::expect_used)] // helpers outside #[test] fns unwrap too

mod common;

use std::time::Duration;

use kelta_proto::ErrorCode;
use kelta_proto::events::bus;
use kelta_proto::ext::{EmbedMode, Ready, StopSpec, ToolDef, ToolHandle, ToolKind, ToolSource, WebStart};
use kelta_proto::ids::{PluginId, ProjectId, ToolId};
use kelta_proto::model::{PaneContent, Placement, SessionKind, TemplateCtx};
use kelta_proto::settings::Layer;

/// Fake web server run from this very test binary (`fake_server_main`), configured by env vars.
#[test]
fn fake_server_main() {
    let Ok(port) = std::env::var("KELTA_FAKE_SERVER_PORT") else { return };
    let quiet = std::env::var("KELTA_FAKE_SERVER_QUIET").is_ok();
    let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
    rt.block_on(async move {
        let l = tokio::net::TcpListener::bind(format!("127.0.0.1:{port}")).await.unwrap();
        if !quiet {
            println!("{{\"url\":\"http://127.0.0.1:{port}/?token=s3cret\",\"port\":{port}}}");
        }
        use std::io::Write;
        let _ = std::io::stdout().flush();
        use axum::extract::ws::{Message, WebSocketUpgrade};
        use axum::http::{StatusCode, header};
        let app = axum::Router::new()
            .route(
                "/",
                axum::routing::get(|| async {
                    (
                        [
                            (header::X_FRAME_OPTIONS, "SAMEORIGIN"),
                            (header::CONTENT_SECURITY_POLICY, "default-src 'self'; frame-ancestors 'self'"),
                        ],
                        "fake tool",
                    )
                }),
            )
            .route(
                "/login",
                axum::routing::get(move || async move {
                    (
                        [(header::LOCATION, format!("http://127.0.0.1:{port}/?token=s3cret"))],
                        StatusCode::FOUND,
                    )
                }),
            )
            .route(
                "/ws",
                axum::routing::get(|ws: WebSocketUpgrade| async move {
                    ws.on_upgrade(|mut socket| async move {
                        while let Some(Ok(Message::Text(t))) = socket.recv().await {
                            let _ = socket.send(Message::Text(format!("echo:{t}").into())).await;
                        }
                    })
                }),
            );
        axum::serve(l, app).await.unwrap();
    });
}

fn web_tool(id: &str, args_env: &[(&str, &str)], ready: Ready, timeout_ms: u64) -> ToolDef {
    let exe = std::env::current_exe().unwrap().display().to_string();
    ToolDef {
        id: id.into(),
        label: id.into(),
        kind: ToolKind::Web,
        start: Some(WebStart {
            command: exe,
            args: vec![
                "fake_server_main".into(),
                "--exact".into(),
                "--nocapture".into(),
                "--test-threads=1".into(),
            ],
            cwd: None,
            env: args_env.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect(),
            ready,
            ready_timeout_ms: timeout_ms,
            stop: StopSpec::default(),
        }),
        ..Default::default()
    }
}

fn sh_tool(id: &str, script: &str, timeout_ms: u64) -> ToolDef {
    ToolDef {
        id: id.into(),
        kind: ToolKind::Web,
        embed: Some(EmbedMode::Iframe),
        start: Some(WebStart {
            command: "sh".into(),
            args: vec!["-c".into(), script.into()],
            cwd: None,
            env: Default::default(),
            ready: Ready::StdoutJson("url".into()),
            ready_timeout_ms: timeout_ms,
            stop: StopSpec::default(),
        }),
        ..Default::default()
    }
}

fn shop() -> ProjectId {
    ProjectId::new("shop")
}

async fn port_closed(port: u16) -> bool {
    common::wait_for(|| std::net::TcpStream::connect(("127.0.0.1", port)).is_err()).await
}

#[tokio::test]
async fn registry_merges_config_and_namespaced_plugin_tools() {
    let env = common::Env::new().with_settings(|s| {
        s.tools.push(ToolDef {
            id: "lazydocker".into(),
            label: "Docker".into(),
            command: Some("lazydocker".into()),
            ..Default::default()
        });
        s.tools.push(ToolDef {
            id: "off".into(),
            command: Some("x".into()),
            enabled: false,
            ..Default::default()
        });
    });
    let src = common::example("tools-pack");
    let preview = env.host.inspect(src.to_str().unwrap()).await.unwrap();
    env.host.install(src.to_str().unwrap(), &preview.sha256, vec![]).await.unwrap();
    let tools = env.host.tools(&shop()).await.unwrap();
    let ids: Vec<&str> = tools.iter().map(|t| t.id.as_str()).collect();
    assert_eq!(ids, ["lazydocker", "tools-pack/k9s", "tools-pack/btop"]);
    assert_eq!(tools[0].source, ToolSource::Layer { layer: Layer::Global });
    assert_eq!(tools[1].source, ToolSource::Plugin { plugin_id: PluginId::new("tools-pack") });
    assert_eq!(tools[0].installed, None);
}

#[tokio::test]
async fn tool_check_runs_the_check_argv() {
    let env = common::Env::new().with_settings(|s| {
        s.tools.push(ToolDef {
            id: "ok".into(),
            command: Some("x".into()),
            check: Some(vec!["sh".into(), "-c".into(), "echo v1.2.3".into()]),
            ..Default::default()
        });
        s.tools.push(ToolDef {
            id: "missing".into(),
            command: Some("x".into()),
            check: Some(vec!["false".into()]),
            install_hint: Some("brew install x".into()),
            ..Default::default()
        });
        s.tools.push(ToolDef {
            id: "nobin".into(),
            command: Some("definitely-not-a-binary-kelta".into()),
            ..Default::default()
        });
    });
    let c = env.host.tool_check(&ToolId::new("ok")).await.unwrap();
    assert!(c.installed);
    assert_eq!(c.version.as_deref(), Some("v1.2.3"));
    let c = env.host.tool_check(&ToolId::new("missing")).await.unwrap();
    assert!(!c.installed);
    assert_eq!(c.install_hint.as_deref(), Some("brew install x"));
    assert!(!env.host.tool_check(&ToolId::new("nobin")).await.unwrap().installed);
    let tools = env.host.tools(&shop()).await.unwrap();
    assert_eq!(tools.iter().find(|t| t.id.as_str() == "ok").unwrap().installed, Some(true));
    assert_eq!(env.host.tool_check(&ToolId::new("nope")).await.unwrap_err().code, ErrorCode::NotFound);
}

#[tokio::test]
async fn pty_tools_spawn_sessions_with_expanded_templates() {
    let env = common::Env::new().with_settings(|s| {
        s.tools.push(ToolDef {
            id: "lazygit".into(),
            label: "Git".into(),
            command: Some("lazygit".into()),
            args: vec!["--path".into(), "{repo.path}".into()],
            cwd: Some("{worktree|repo.path}".into()),
            env: [("LG_CONFIG_FILE".to_owned(), "{data_dir}/lazygit-kelta.yml".to_owned())].into(),
            ..Default::default()
        });
    });
    let h = env
        .host
        .tool_open(&shop(), &ToolId::new("lazygit"), TemplateCtx::default(), Placement::SplitDown)
        .await
        .unwrap();
    let ToolHandle::Pty { session_id } = h else { panic!("expected a pty handle") };
    let repo = env.tmp.path().join("repo");
    let spawn = env.core.calls().into_iter().find(|c| c.method == "session_spawn").unwrap().args;
    assert_eq!(spawn["program"], "lazygit");
    assert_eq!(spawn["args"], serde_json::json!(["--path", repo.display().to_string()]));
    assert_eq!(spawn["cwd"], repo.display().to_string());
    assert!(spawn["env"]["LG_CONFIG_FILE"].as_str().unwrap().ends_with("data/lazygit-kelta.yml"));
    assert_eq!(spawn["restore"]["kind"], "relaunch");
    let s = env.core.sessions().into_iter().find(|s| s.id == session_id).unwrap();
    assert_eq!(s.kind, SessionKind::Tool { tool_id: ToolId::new("lazygit") });
    let (_, req) = env.core.opened().pop().unwrap();
    assert_eq!(req.placement, Placement::SplitDown);
    assert!(matches!(req.content, PaneContent::Terminal { .. }));
    assert!(env.core.published().iter().any(|e| e.name == bus::TOOL_OPENED));
}

#[tokio::test]
async fn plugin_tools_need_exec_and_spawn_grants() {
    let env = common::Env::new();
    let src = common::example("tools-pack");
    let preview = env.host.inspect(src.to_str().unwrap()).await.unwrap();
    env.host.install(src.to_str().unwrap(), &preview.sha256, vec!["sessions.spawn".into()]).await.unwrap();
    let k9s = ToolId::new("tools-pack/k9s");
    let e = env.host.tool_open(&shop(), &k9s, TemplateCtx::default(), Placement::NewTab).await.unwrap_err();
    assert_eq!(e.code, ErrorCode::PermissionDenied);
    assert_eq!(e.detail.unwrap()["permission"], "exec:k9s");
    env.host
        .grant(&PluginId::new("tools-pack"), vec!["sessions.spawn".into(), "exec:k9s".into()])
        .await
        .unwrap();
    assert!(env.host.tool_open(&shop(), &k9s, TemplateCtx::default(), Placement::NewTab).await.is_ok());
    // A disabled plugin's tools are gone, even after the picker listed them.
    env.host.tools(&shop()).await.unwrap();
    env.host.enable(&PluginId::new("tools-pack"), false).await.unwrap();
    let e = env.host.tool_open(&shop(), &k9s, TemplateCtx::default(), Placement::NewTab).await.unwrap_err();
    assert_eq!(e.code, ErrorCode::NotFound);
    assert_eq!(env.host.tool_check(&k9s).await.unwrap_err().code, ErrorCode::NotFound);
}

#[tokio::test]
async fn web_tool_ready_from_stdout_json_is_proxied_and_killed_on_close() {
    let env = common::Env::new().with_settings(|s| {
        s.tools.push(web_tool(
            "fake",
            &[("KELTA_FAKE_SERVER_PORT", "{port}")],
            Ready::StdoutJson("url".into()),
            10_000,
        ));
    });
    let h = env
        .host
        .tool_open(&shop(), &ToolId::new("fake"), TemplateCtx::default(), Placement::NewTab)
        .await
        .unwrap();
    let ToolHandle::Web { instance_id, url, embed } = h else { panic!("expected a web handle") };
    assert_eq!(embed, EmbedMode::Proxy, "X-Frame-Options → proxy");
    assert!(url.contains(&format!("/proxy/{instance_id}/?token=s3cret")), "{url}");
    let opened = env.core.opened();
    assert!(
        matches!(&opened.last().unwrap().1.content, PaneContent::Web { tool_instance_id } if tool_instance_id == &instance_id)
    );
    // The proxied page is served without the frame-blocking header.
    let port: u16 = url.split(':').nth(2).unwrap().split('/').next().unwrap().parse().unwrap();
    let path = url.splitn(4, '/').nth(3).unwrap();
    let mut s = tokio::net::TcpStream::connect(("127.0.0.1", port)).await.unwrap();
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    s.write_all(
        format!("GET /{path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nConnection: close\r\n\r\n").as_bytes(),
    )
    .await
    .unwrap();
    let mut body = String::new();
    s.read_to_string(&mut body).await.unwrap();
    assert!(body.starts_with("HTTP/1.1 200"), "{body}");
    assert!(!body.to_ascii_lowercase().contains("x-frame-options"));
    assert!(body.ends_with("fake tool"));
    assert_eq!(env.host.web_tools_running(), 1);
    let pids = env.host.web_tool_pids();
    assert_eq!(pids.len(), 1);

    env.host.tool_close(&instance_id).await.unwrap();
    assert_eq!(env.host.web_tools_running(), 0);
    assert!(!kelta_plugins::proxy::is_registered(instance_id.as_str()));
    let pid = rustix::process::Pid::from_raw(pids[0] as i32).unwrap();
    assert!(common::wait_for(|| rustix::process::test_kill_process(pid).is_err()).await, "server killed");
}

#[tokio::test]
async fn web_tool_port_open_readiness_and_process_group_kill() {
    let env = common::Env::new().with_settings(|s| {
        let mut t = web_tool(
            "quiet",
            &[("KELTA_FAKE_SERVER_PORT", "{port}"), ("KELTA_FAKE_SERVER_QUIET", "1")],
            Ready::PortOpen(true),
            10_000,
        );
        t.embed = Some(EmbedMode::Iframe);
        s.tools.push(t);
    });
    let h = env
        .host
        .tool_open(&shop(), &ToolId::new("quiet"), TemplateCtx::default(), Placement::NewTab)
        .await
        .unwrap();
    let ToolHandle::Web { instance_id, url, embed } = h else { panic!() };
    assert_eq!(embed, EmbedMode::Iframe);
    let port: u16 = url.trim_end_matches('/').rsplit(':').next().unwrap().parse().unwrap();
    assert!(std::net::TcpStream::connect(("127.0.0.1", port)).is_ok());
    env.host.tool_close(&instance_id).await.unwrap();
    assert!(port_closed(port).await, "the server process group is stopped");
}

#[tokio::test]
async fn web_tool_readiness_timeout_and_early_exit() {
    let env = common::Env::new().with_settings(|s| {
        s.tools.push(sh_tool("slow", "echo starting; exec sleep 30", 300));
        s.tools.push(sh_tool("crash", "echo boom >&2; exit 3", 5_000));
    });
    let e = env
        .host
        .tool_open(&shop(), &ToolId::new("slow"), TemplateCtx::default(), Placement::NewTab)
        .await
        .unwrap_err();
    assert_eq!(e.code, ErrorCode::Timeout);
    assert!(e.detail.unwrap()["log"].as_str().unwrap().contains("starting"));
    let e = env
        .host
        .tool_open(&shop(), &ToolId::new("crash"), TemplateCtx::default(), Placement::NewTab)
        .await
        .unwrap_err();
    assert_eq!(e.code, ErrorCode::Upstream);
    assert!(e.message.contains("code 3"), "{}", e.message);
    assert!(e.detail.unwrap()["log"].as_str().unwrap().contains("boom"));
    assert_eq!(env.host.web_tools_running(), 0);
}

#[tokio::test]
async fn web_tool_exit_after_ready_publishes_tool_exited() {
    let env = common::Env::new().with_settings(|s| {
        s.tools.push(sh_tool("short", r#"echo '{"url":"http://127.0.0.1:9/"}'; sleep 0.2; exit 4"#, 5_000));
    });
    let h = env
        .host
        .tool_open(&shop(), &ToolId::new("short"), TemplateCtx::default(), Placement::NewTab)
        .await
        .unwrap();
    assert!(matches!(h, ToolHandle::Web { embed: EmbedMode::Iframe, .. }));
    let core = env.core.clone();
    assert!(
        common::wait_for(|| core
            .published()
            .iter()
            .any(|e| e.name == bus::TOOL_EXITED && e.payload["code"] == 4))
        .await
    );
    assert_eq!(env.host.web_tools_running(), 0);
}

#[tokio::test]
async fn project_close_stops_project_web_tools() {
    let env = common::Env::new().with_settings(|s| {
        s.tools.push(sh_tool("srv", r#"echo '{"url":"http://127.0.0.1:9/"}'; exec sleep 30"#, 5_000));
    });
    env.host
        .tool_open(&shop(), &ToolId::new("srv"), TemplateCtx::default(), Placement::NewTab)
        .await
        .unwrap();
    assert_eq!(env.host.web_tools_running(), 1);
    let ev =
        kelta_proto::events::BusEvent::new(bus::PROJECT_CLOSED, serde_json::json!({ "project_id": "shop" }))
            .with_project(shop());
    env.host.on_event(&ev).await;
    assert_eq!(env.host.web_tools_running(), 0);
    tokio::time::sleep(Duration::from_millis(1)).await;
}

// ---- Gate W1 (BUILD_PLAN §6): web tool embedding from both webview origins ----------------------

/// The page holding the tool's iframe: `tauri://localhost` (macOS), `http://tauri.localhost` (Linux).
const WEBVIEW_ORIGINS: [&str; 2] = ["tauri://localhost", "http://tauri.localhost"];

/// Raw `GET` (no client dependency) to `host` (`localhost` resolves to `::1` and/or `127.0.0.1`).
async fn http_get(host: &str, port: u16, path: &str, extra_headers: &str) -> String {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let mut s = tokio::net::TcpStream::connect((host, port)).await.unwrap();
    s.write_all(
        format!("GET {path} HTTP/1.1\r\nHost: {host}:{port}\r\n{extra_headers}Connection: close\r\n\r\n")
            .as_bytes(),
    )
    .await
    .unwrap();
    let mut out = String::new();
    s.read_to_string(&mut out).await.unwrap();
    out
}

/// `http://host:port/path?q` → (host, port, `/path?q`).
fn split_url(url: &str) -> (String, u16, String) {
    let rest = url.strip_prefix("http://").unwrap();
    let (authority, path) = rest.split_at(rest.find('/').unwrap());
    let (host, port) = authority.rsplit_once(':').unwrap();
    (host.to_owned(), port.parse().unwrap(), path.to_owned())
}

/// Open a WebSocket the way a page framed at `http://host:port` does (path-absolute, own Origin),
/// send `msg` and return the first text reply.
async fn ws_roundtrip(host: &str, port: u16, path: &str, msg: &str) -> String {
    use futures::{SinkExt, StreamExt};
    use tokio_tungstenite::tungstenite::{Message, client::IntoClientRequest};
    let mut req = format!("ws://{host}:{port}{path}").into_client_request().unwrap();
    req.headers_mut().insert("origin", format!("http://{host}:{port}").parse().unwrap());
    let stream = tokio::net::TcpStream::connect((host, port)).await.unwrap();
    let (mut ws, resp) = tokio_tungstenite::client_async(req, stream).await.unwrap();
    assert_eq!(resp.status(), 101);
    ws.send(Message::Text(msg.into())).await.unwrap();
    let reply = tokio::time::timeout(Duration::from_secs(15), async {
        loop {
            match ws.next().await.unwrap().unwrap() {
                Message::Text(t) => return t.as_str().to_owned(),
                _ => continue,
            }
        }
    })
    .await
    .expect("websocket reply");
    let _ = ws.close(None).await;
    reply
}

/// The framed page as the webview loads it: frame-blocking headers must be gone for either origin.
async fn assert_frameable(host: &str, port: u16, path: &str) -> String {
    let mut body = String::new();
    for origin in WEBVIEW_ORIGINS {
        let resp =
            http_get(host, port, path, &format!("Referer: {origin}/\r\nSec-Fetch-Dest: iframe\r\n")).await;
        assert!(resp.starts_with("HTTP/1.1 200"), "{origin}: {resp}");
        let lower = resp.to_ascii_lowercase();
        assert!(!lower.contains("x-frame-options"), "{origin}: {resp}");
        assert!(!lower.contains("frame-ancestors"), "{origin}: {resp}");
        body = resp;
    }
    body
}

#[tokio::test]
async fn gate_w1_frame_blocking_tool_is_proxied_for_both_webview_origins() {
    let env = common::Env::new().with_settings(|s| {
        s.tools.push(web_tool(
            "fake",
            &[("KELTA_FAKE_SERVER_PORT", "{port}")],
            Ready::StdoutJson("url".into()),
            10_000,
        ));
    });
    let h =
        env.host.tool_open(&shop(), &ToolId::new("fake"), TemplateCtx::default(), Placement::NewTab).await;
    let ToolHandle::Web { instance_id, url, embed } = h.unwrap() else { panic!("expected a web handle") };
    assert_eq!(embed, EmbedMode::Proxy, "X-Frame-Options + frame-ancestors → auto picks the proxy");
    let (host, port, path) = split_url(&url);
    assert_eq!(host, "127.0.0.1");
    let page = assert_frameable(&host, port, &path).await;
    assert!(page.to_ascii_lowercase().contains("content-security-policy: default-src 'self'"), "{page}");
    assert!(page.ends_with("fake tool"));
    for origin in WEBVIEW_ORIGINS {
        // Kelta's own page (webview origin) without the instance id never reaches the tool.
        let resp = http_get(&host, port, "/", &format!("Origin: {origin}\r\n")).await;
        assert!(resp.starts_with("HTTP/1.1 404"), "{origin}: {resp}");
    }
    let resp = http_get(&host, port, &format!("/proxy/{instance_id}/login"), "").await;
    assert!(resp.contains(&format!("location: /proxy/{instance_id}/?token=s3cret")), "{resp}");
    assert_eq!(ws_roundtrip(&host, port, "/ws", "hi").await, "echo:hi");
    assert_eq!(ws_roundtrip(&host, port, &format!("/proxy/{instance_id}/ws"), "ho").await, "echo:ho");
    env.host.tool_close(&instance_id).await.unwrap();
}

/// Real Sapling ISL (`sl web`) with the documented tool definition, in a temp git repo: readiness,
/// auto embed (ISL sends no frame-blocking header → plain iframe), and the same page + WebSocket
/// through the proxy. Skipped when Sapling is not installed.
#[tokio::test]
async fn gate_w1_sapling_isl_embeds_via_iframe_and_proxy() {
    let sapling = std::process::Command::new("sl")
        .arg("--version")
        .output()
        .is_ok_and(|o| o.status.success() && String::from_utf8_lossy(&o.stdout).contains("Sapling"));
    if !sapling {
        eprintln!("skipped: Sapling `sl` is not installed (https://sapling-scm.com)");
        return;
    }
    let env = common::Env::new().with_settings(|s| {
        s.tools.push(kelta_proto::samples::isl_tool());
        let mut proxied = kelta_proto::samples::isl_tool();
        proxied.id = "isl-proxy".into();
        proxied.embed = Some(EmbedMode::Proxy);
        s.tools.push(proxied);
    });
    let repo = env.tmp.path().join("repo");
    for args in [
        &["init", "-q"][..],
        &["-c", "user.name=k", "-c", "user.email=k@k", "commit", "-q", "--allow-empty", "-m", "init"],
    ] {
        assert!(std::process::Command::new("git").args(args).current_dir(&repo).status().unwrap().success());
    }
    // ISL's own wire format (its client serializes objects with `__rpcType`).
    let heartbeat = r#"{"__rpcType":"object","type":"heartbeat","id":"isl-connection"}"#;

    let h = env.host.tool_open(&shop(), &ToolId::new("isl"), TemplateCtx::default(), Placement::NewTab).await;
    let ToolHandle::Web { instance_id, url, embed } = h.unwrap() else { panic!("expected a web handle") };
    assert_eq!(embed, EmbedMode::Iframe, "ISL is frameable as is: {url}");
    let (host, port, path) = split_url(&url);
    assert!(path.contains("token="), "{url}");
    let query = path.split_once('?').unwrap().1.to_owned();
    assert!(assert_frameable(&host, port, &path).await.contains("Interactive Smartlog"));
    assert!(ws_roundtrip(&host, port, &format!("/ws?{query}"), heartbeat).await.contains("heartbeat"));
    let pids = env.host.web_tool_pids();
    env.host.tool_close(&instance_id).await.unwrap();
    let pid = rustix::process::Pid::from_raw(pids[0] as i32).unwrap();
    assert!(common::wait_for(|| rustix::process::test_kill_process(pid).is_err()).await, "sl web stopped");

    let h = env
        .host
        .tool_open(&shop(), &ToolId::new("isl-proxy"), TemplateCtx::default(), Placement::NewTab)
        .await;
    let ToolHandle::Web { instance_id, url, embed } = h.unwrap() else { panic!("expected a web handle") };
    assert_eq!(embed, EmbedMode::Proxy);
    let (host, port, path) = split_url(&url);
    assert!(path.starts_with(&format!("/proxy/{instance_id}/?token=")), "{url}");
    let page = assert_frameable(&host, port, &path).await;
    assert!(page.contains("Interactive Smartlog"));
    // Relative assets resolve under the proxy prefix; ISL's socket is path-absolute (`/ws`).
    let asset = page.split("src=\"./").nth(1).unwrap().split('"').next().unwrap();
    let resp = http_get(&host, port, &format!("/proxy/{instance_id}/{asset}"), "").await;
    assert!(resp.starts_with("HTTP/1.1 200"), "{resp}");
    let query = path.split_once('?').unwrap().1;
    assert!(ws_roundtrip(&host, port, &format!("/ws?{query}"), heartbeat).await.contains("heartbeat"));
    env.host.tool_close(&instance_id).await.unwrap();
}

/// A Dock-launched app has a bare PATH: web tools must be found and run through the login PATH,
/// with the project env (regression for `launch_env` / `check_tool`).
#[tokio::test]
async fn web_tool_found_and_launched_through_login_path_with_project_env() {
    use std::os::unix::fs::PermissionsExt;
    let env = common::Env::new().with_settings(|s| {
        let mut t = sh_tool("fake", "", 5_000);
        let start = t.start.as_mut().unwrap();
        start.command = "kelta-fake-tool".into();
        start.args.clear();
        s.tools.push(t);
        s.env.insert("KELTA_PROJECT_VAR".into(), "proj".into());
    });
    let bin_dir = env.tmp.path().join("login-bin");
    std::fs::create_dir_all(&bin_dir).unwrap();
    let bin = bin_dir.join("kelta-fake-tool");
    let script = "#!/bin/sh\necho \"{\\\"url\\\":\\\"http://127.0.0.1:9/?v=$KELTA_PROJECT_VAR&p=$PATH\\\"}\"\nexec /bin/sleep 30\n";
    std::fs::write(&bin, script).unwrap();
    std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755)).unwrap();
    let id = ToolId::new("fake");
    assert!(!env.host.tool_check(&id).await.unwrap().installed, "not on the dev PATH");

    env.core.respond("login_path", serde_json::json!(bin_dir));
    assert!(env.host.tool_check(&id).await.unwrap().installed, "found through the login PATH");
    let h = env.host.tool_open(&shop(), &id, TemplateCtx::default(), Placement::NewTab).await.unwrap();
    let ToolHandle::Web { instance_id, url, .. } = h else { panic!("expected a web handle") };
    assert!(url.contains(&format!("?v=proj&p={}", bin_dir.display())), "{url}");
    env.host.tool_close(&instance_id).await.unwrap();
}

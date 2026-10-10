//! Reverse proxy against local axum test servers: frame-blocking headers stripped, Location
//! rewritten, WebSocket echo passed through, loopback-only upstreams, Host guard on the own listener.

use axum::Router;
use axum::extract::ws::{Message, WebSocketUpgrade};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Redirect};
use axum::routing::get;
use futures::{SinkExt, StreamExt};
use kelta_plugins::proxy;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

async fn serve(app: Router) -> u16 {
    let l = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
    let port = l.local_addr().unwrap().port();
    tokio::spawn(async move { axum::serve(l, app).await.unwrap() });
    port
}

fn upstream() -> Router {
    Router::new()
        .route(
            "/page",
            get(|| async {
                (
                    [
                        (header::X_FRAME_OPTIONS, "DENY"),
                        (header::CONTENT_SECURITY_POLICY, "default-src 'self'; frame-ancestors 'none'"),
                        (header::CONTENT_TYPE, "text/html"),
                    ],
                    "<html>tool</html>",
                )
            }),
        )
        .route(
            "/only-fa",
            get(|| async { ([(header::CONTENT_SECURITY_POLICY, "frame-ancestors 'self'")], "x") }),
        )
        .route("/login", get(|| async { Redirect::to("/dashboard?x=1") }))
        .route(
            "/abs",
            get(|| async { ([(header::LOCATION, "http://127.0.0.1:1/abs-target")], StatusCode::FOUND) }),
        )
        .route(
            "/echo-headers",
            get(|headers: HeaderMap| async move {
                format!("host={:?} origin={:?}", headers.get(header::HOST), headers.get(header::ORIGIN))
            }),
        )
        .route(
            "/ws",
            get(|ws: WebSocketUpgrade| async move {
                ws.on_upgrade(|mut socket| async move {
                    while let Some(Ok(msg)) = socket.recv().await {
                        if let Message::Text(t) = msg {
                            let _ = socket.send(Message::Text(format!("echo:{t}").into())).await;
                        }
                    }
                })
            }),
        )
}

const INST: &str = "0123456789abcdef0123456789abcdef";

async fn setup(inst: &str) -> (u16, u16) {
    let up = serve(upstream()).await;
    proxy::register(inst, &format!("http://127.0.0.1:{up}/")).unwrap();
    let px = proxy::ensure_listener(inst).await.unwrap();
    (up, px)
}

async fn raw_get(port: u16, path: &str, host: &str) -> (u16, String) {
    let mut s = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
    s.write_all(format!("GET {path} HTTP/1.1\r\nHost: {host}\r\nOrigin: http://127.0.0.1:{port}\r\nConnection: close\r\n\r\n").as_bytes())
        .await
        .unwrap();
    let mut buf = String::new();
    s.read_to_string(&mut buf).await.unwrap();
    let status = buf.split(' ').nth(1).and_then(|c| c.parse().ok()).unwrap_or(0);
    (status, buf)
}

#[tokio::test]
async fn strips_frame_blocking_headers_and_rewrites_location() {
    let inst = "1123456789abcdef0123456789abcdef";
    let (up, px) = setup(inst).await;
    let host = format!("127.0.0.1:{px}");
    let (status, resp) = raw_get(px, &format!("/proxy/{inst}/page"), &host).await;
    assert_eq!(status, 200);
    let lower = resp.to_ascii_lowercase();
    assert!(!lower.contains("x-frame-options"), "{resp}");
    assert!(!lower.contains("frame-ancestors"), "{resp}");
    assert!(lower.contains("content-security-policy: default-src 'self'"), "{resp}");
    assert!(resp.ends_with("<html>tool</html>"));

    let (_, resp) = raw_get(px, &format!("/proxy/{inst}/only-fa"), &host).await;
    assert!(!resp.to_ascii_lowercase().contains("content-security-policy"), "{resp}");

    let (status, resp) = raw_get(px, &format!("/proxy/{inst}/login"), &host).await;
    assert_eq!(status, 303);
    assert!(resp.contains(&format!("location: /proxy/{inst}/dashboard?x=1")), "{resp}");

    // Host and Origin are rewritten to the upstream.
    let (_, resp) = raw_get(px, &format!("/proxy/{inst}/echo-headers"), &host).await;
    assert!(resp.contains(&format!("host=Some(\"127.0.0.1:{up}\")")), "{resp}");
    assert!(resp.contains(&format!("origin=Some(\"http://127.0.0.1:{up}\")")), "{resp}");
    proxy::unregister(inst);
}

#[tokio::test]
async fn passes_websocket_traffic() {
    let inst = "2123456789abcdef0123456789abcdef";
    let (_, px) = setup(inst).await;
    let url = format!("ws://127.0.0.1:{px}/proxy/{inst}/ws");
    let (mut ws, resp) = tokio_tungstenite::connect_async(url).await.unwrap();
    assert_eq!(resp.status(), 101);
    for i in 0..3 {
        ws.send(tokio_tungstenite::tungstenite::Message::Text(format!("hello {i}").into())).await.unwrap();
        let reply = ws.next().await.unwrap().unwrap();
        assert_eq!(reply.into_text().unwrap().as_str(), format!("echo:hello {i}"));
    }
    ws.close(None).await.unwrap();
    proxy::unregister(inst);
}

#[tokio::test]
async fn refuses_non_loopback_upstreams() {
    assert!(proxy::register(INST, "http://example.com/").is_err());
    assert!(proxy::register(INST, "https://127.0.0.1:443/").is_err());
    assert!(proxy::register("not-an-instance", "http://127.0.0.1:1/").is_err());
}

#[tokio::test]
async fn own_listener_guards_host_and_stops_when_idle() {
    let inst = "3123456789abcdef0123456789abcdef";
    let up = serve(upstream()).await;
    proxy::register(inst, &format!("http://127.0.0.1:{up}/")).unwrap();
    let port = proxy::ensure_listener(inst).await.unwrap();
    assert_eq!(proxy::ensure_listener(inst).await.unwrap(), port);
    // Each instance gets its own listener (origin), which serves only that instance.
    let other = "4123456789abcdef0123456789abcdef";
    proxy::register(other, &format!("http://127.0.0.1:{up}/")).unwrap();
    let other_port = proxy::ensure_listener(other).await.unwrap();
    assert_ne!(other_port, port);
    let (status, _) = raw_get(port, &format!("/proxy/{other}/page"), &format!("127.0.0.1:{port}")).await;
    assert_eq!(status, 404, "another instance is unreachable from this origin");
    proxy::unregister(other);
    let (status, _) = raw_get(port, &format!("/proxy/{inst}/page"), &format!("127.0.0.1:{port}")).await;
    assert_eq!(status, 200);
    let (status, _) = raw_get(port, &format!("/proxy/{inst}/page"), "evil.example:80").await;
    assert_eq!(status, 421, "DNS-rebinding guard");
    // Path-absolute subresources of the proxied page are routed by Referer.
    let mut s = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
    s.write_all(
        format!("GET /page HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nReferer: http://127.0.0.1:{port}/proxy/{inst}/\r\nConnection: close\r\n\r\n")
            .as_bytes(),
    )
    .await
    .unwrap();
    let mut buf = String::new();
    s.read_to_string(&mut buf).await.unwrap();
    assert!(buf.starts_with("HTTP/1.1 200"), "{buf}");
    // A foreign page without the instance id gets nothing (no single-instance fallback for it).
    let mut s = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
    s.write_all(
        format!("GET /page HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nOrigin: http://evil.example\r\nConnection: close\r\n\r\n")
            .as_bytes(),
    )
    .await
    .unwrap();
    let mut buf = String::new();
    s.read_to_string(&mut buf).await.unwrap();
    assert!(buf.starts_with("HTTP/1.1 404"), "{buf}");
    proxy::unregister(inst);
    // The aborted accept task drops its socket shortly after.
    let mut closed = false;
    for _ in 0..200 {
        if TcpStream::connect(("127.0.0.1", port)).await.is_err() {
            closed = true;
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    assert!(closed, "listener stops on unregister");
}

#[tokio::test]
async fn head_probe_detects_framing_headers() {
    let up = serve(upstream()).await;
    assert!(proxy::probe_http(&format!("http://127.0.0.1:{up}/page")).await.unwrap());
    assert!(!proxy::probe_http(&format!("http://127.0.0.1:{up}/echo-headers")).await.unwrap());
    let _ = IntoResponse::into_response(());
}

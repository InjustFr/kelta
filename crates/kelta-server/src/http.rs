//! Lazy loopback HTTP server (ARCHITECTURE §2, §11.1): axum on `127.0.0.1:<random>`, started by
//! the first consumer and stopped when the consumer count drops to 0 (no idle timer).
//!
//! Routes: `POST /hook/<sid>` (http hook transport, hook token), `POST /mcp/<sid>` (MCP, MCP token),
//! `POST /mcp` (MCP for Claude sessions Kelta did not spawn, install token).
//! Every request must carry `Host: 127.0.0.1:<port>`. (Web-tool proxies run on their own loopback
//! listener per instance, in kelta-plugins.)

use std::sync::{Arc, Weak};

use axum::Router;
use axum::body::Bytes;
use axum::extract::{DefaultBodyLimit, Path, Request, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::post;
use kelta_proto::api::CoreApi;
use kelta_proto::ctl::CTL_MAX_LINE;
use kelta_proto::error::KeltaError;
use kelta_proto::hooks::HookPayload;
use kelta_proto::ids::SessionId;
use parking_lot::Mutex;
use tokio::sync::oneshot;

use crate::auth::{TokenKind, TokenTable, bearer};
use crate::{mcp, serve};

/// Request body cap for `/hook` and `/mcp`.
const BODY_LIMIT: usize = CTL_MAX_LINE;

struct Running {
    port: u16,
    shutdown: oneshot::Sender<()>,
}

/// Refcounted server state.
#[derive(Default)]
pub(crate) struct HttpState {
    refs: usize,
    running: Option<Running>,
}

struct Ctx {
    core: Weak<dyn CoreApi>,
    tokens: Arc<TokenTable>,
    /// Exact expected `Host` value (`127.0.0.1:<port>`).
    host: String,
}

pub(crate) fn port(state: &Mutex<HttpState>) -> Option<u16> {
    state.lock().running.as_ref().map(|r| r.port)
}

/// Take a consumer reference, starting the server if it is not running (on `preferred` when free).
pub(crate) fn ensure(
    state: &Mutex<HttpState>,
    core: Weak<dyn CoreApi>,
    tokens: Arc<TokenTable>,
    preferred: Option<u16>,
) -> Result<u16, KeltaError> {
    let mut st = state.lock();
    if let Some(port) = st.running.as_ref().map(|r| r.port) {
        st.refs += 1;
        return Ok(port);
    }
    let std_listener = match preferred.and_then(|p| std::net::TcpListener::bind(("127.0.0.1", p)).ok()) {
        Some(l) => l,
        None => std::net::TcpListener::bind(("127.0.0.1", 0))?,
    };
    std_listener.set_nonblocking(true)?;
    let port = std_listener.local_addr()?.port();
    let listener = tokio::net::TcpListener::from_std(std_listener)?;
    let ctx = Arc::new(Ctx { core, tokens, host: format!("127.0.0.1:{port}") });
    let app = router(ctx);
    let (tx, rx) = oneshot::channel::<()>();
    tokio::spawn(async move {
        let shutdown = async move {
            let _ = rx.await;
        };
        serve::serve(listener, app, serve::MAX_CONNS, serve::REQUEST_TIMEOUT, shutdown).await;
        tracing::debug!(port, "http server stopped");
    });
    tracing::debug!(port, "http server started");
    st.running = Some(Running { port, shutdown: tx });
    st.refs = 1;
    Ok(port)
}

/// Drop a consumer reference; stop the server at 0.
pub(crate) fn release(state: &Mutex<HttpState>) {
    let mut st = state.lock();
    st.refs = st.refs.saturating_sub(1);
    if st.refs == 0
        && let Some(r) = st.running.take()
    {
        let _ = r.shutdown.send(());
    }
}

/// Stop regardless of the refcount (server dropped).
pub(crate) fn stop(state: &Mutex<HttpState>) {
    let mut st = state.lock();
    st.refs = 0;
    if let Some(r) = st.running.take() {
        let _ = r.shutdown.send(());
    }
}

fn router(ctx: Arc<Ctx>) -> Router {
    Router::new()
        .route("/hook/{sid}", post(hook))
        .route("/mcp/{sid}", post(mcp_post).get(mcp_no_stream).delete(mcp_no_stream))
        .route("/mcp", post(mcp_global_post).get(mcp_global_no_stream).delete(mcp_global_no_stream))
        .layer(DefaultBodyLimit::max(BODY_LIMIT))
        .with_state(ctx.clone())
        .layer(middleware::from_fn_with_state(ctx, host_guard))
}

/// DNS-rebinding guard: `Host` (or the HTTP/2 authority) must be exactly `127.0.0.1:<port>`.
async fn host_guard(State(ctx): State<Arc<Ctx>>, req: Request, next: Next) -> Response {
    let host = match req.headers().get(header::HOST) {
        Some(h) => h.to_str().ok().map(str::to_owned),
        None => req.uri().authority().map(|a| a.as_str().to_owned()),
    };
    if host.as_deref() != Some(ctx.host.as_str()) {
        tracing::warn!("http: request with an unexpected Host header rejected");
        return (StatusCode::FORBIDDEN, "forbidden").into_response();
    }
    // A stalled body would hold a connection slot forever (#293).
    let (parts, body) = req.into_parts();
    let body =
        match tokio::time::timeout(serve::REQUEST_TIMEOUT, axum::body::to_bytes(body, BODY_LIMIT)).await {
            Ok(Ok(b)) => b,
            Ok(Err(_)) => return (StatusCode::PAYLOAD_TOO_LARGE, "request body too large").into_response(),
            Err(_) => return (StatusCode::REQUEST_TIMEOUT, "request body timeout").into_response(),
        };
    next.run(Request::from_parts(parts, body.into())).await
}

fn presented(headers: &HeaderMap) -> Option<&str> {
    bearer(headers.get(header::AUTHORIZATION).and_then(|v| v.to_str().ok()))
}

fn authorized(ctx: &Ctx, sid: &SessionId, kind: TokenKind, headers: &HeaderMap) -> bool {
    presented(headers).is_some_and(|t| ctx.tokens.check(sid, kind, t))
}

fn unauthorized() -> Response {
    (StatusCode::UNAUTHORIZED, [(header::WWW_AUTHENTICATE, "Bearer")], "unauthorized").into_response()
}

fn core_gone() -> Response {
    (StatusCode::SERVICE_UNAVAILABLE, "kelta is shutting down").into_response()
}

async fn hook(
    State(ctx): State<Arc<Ctx>>,
    Path(sid): Path<String>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let sid = SessionId::new(sid);
    if !authorized(&ctx, &sid, TokenKind::Hook, &headers) {
        tracing::warn!(session = %sid, "http: hook with an invalid token rejected");
        return unauthorized();
    }
    let payload: HookPayload = match serde_json::from_slice(&body) {
        Ok(p) => p,
        Err(e) => return (StatusCode::BAD_REQUEST, format!("bad hook payload: {e}")).into_response(),
    };
    let Some(core) = ctx.core.upgrade() else { return core_gone() };
    if let Err(e) = crate::hooks::ingest(&core, &sid, payload).await {
        tracing::warn!(session = %sid, error = %e, "http: hook ingestion failed");
    }
    // Empty 200: no decision for Claude.
    StatusCode::OK.into_response()
}

async fn mcp_post(
    State(ctx): State<Arc<Ctx>>,
    Path(sid): Path<String>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let sid = SessionId::new(sid);
    if !authorized(&ctx, &sid, TokenKind::Mcp, &headers) {
        tracing::warn!(session = %sid, "http: MCP request with an invalid token rejected");
        return unauthorized();
    }
    mcp_reply(&ctx, Some(&sid), &body).await
}

async fn mcp_global_post(State(ctx): State<Arc<Ctx>>, headers: HeaderMap, body: Bytes) -> Response {
    if !presented(&headers).is_some_and(|t| ctx.tokens.check_global(t)) {
        tracing::warn!("http: global MCP request with an invalid token rejected");
        return unauthorized();
    }
    mcp_reply(&ctx, None, &body).await
}

async fn mcp_reply(ctx: &Ctx, sid: Option<&SessionId>, body: &[u8]) -> Response {
    let Some(core) = ctx.core.upgrade() else { return core_gone() };
    match mcp::handle_body(&core, sid, body).await {
        mcp::Reply::Json(v) => json_response(StatusCode::OK, &v),
        mcp::Reply::Accepted => StatusCode::ACCEPTED.into_response(),
        mcp::Reply::BadRequest(v) => json_response(StatusCode::BAD_REQUEST, &v),
    }
}

/// Stateless server: no server-initiated SSE stream and no session to delete.
async fn mcp_no_stream(State(ctx): State<Arc<Ctx>>, Path(sid): Path<String>, headers: HeaderMap) -> Response {
    let sid = SessionId::new(sid);
    if !authorized(&ctx, &sid, TokenKind::Mcp, &headers) {
        return unauthorized();
    }
    (StatusCode::METHOD_NOT_ALLOWED, [(header::ALLOW, "POST")]).into_response()
}

async fn mcp_global_no_stream(State(ctx): State<Arc<Ctx>>, headers: HeaderMap) -> Response {
    if !presented(&headers).is_some_and(|t| ctx.tokens.check_global(t)) {
        return unauthorized();
    }
    (StatusCode::METHOD_NOT_ALLOWED, [(header::ALLOW, "POST")]).into_response()
}

fn json_response(status: StatusCode, v: &serde_json::Value) -> Response {
    let body = serde_json::to_vec(v).unwrap_or_default();
    (status, [(header::CONTENT_TYPE, "application/json")], body).into_response()
}

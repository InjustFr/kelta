//! Header-stripping reverse proxy for web tools (PLUGINS §2, ARCHITECTURE D14/§11.1).
//!
//! `/proxy/<128-bit instance>/…` → the tool's own loopback origin. Strips `X-Frame-Options` and the
//! CSP `frame-ancestors` directive, passes WebSocket upgrades through (raw byte copy after the 101),
//! rewrites `Location` to the proxy prefix and only ever talks to loopback upstreams.
//!
//! [`router`] can be mounted by kelta-server (at `/proxy` or `/proxy/`); kelta-plugins also serves
//! it from its own lazy loopback listener ([`ensure_listener`]) while at least one proxied instance
//! exists, so web tools work without knowing kelta-server's port (see `docs/contract-requests/L8.md`).

use std::collections::HashMap;
use std::sync::LazyLock;

use axum::Router;
use axum::body::Body;
use axum::extract::{Request, State};
use axum::http::{HeaderMap, HeaderName, HeaderValue, Method, StatusCode, Uri, header};
use axum::response::{IntoResponse, Response};
use bytes::Bytes;
use http_body_util::Empty;
use hyper_util::client::legacy::Client;
use hyper_util::client::legacy::connect::HttpConnector;
use hyper_util::rt::{TokioExecutor, TokioIo};
use kelta_proto::error::KeltaError;
use parking_lot::{Mutex, RwLock};
use tokio::net::TcpListener;

use crate::util::is_loopback_host;

/// Upstream of one proxied instance.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Upstream {
    /// `127.0.0.1:4312`.
    pub authority: String,
    /// `http://127.0.0.1:4312`.
    pub origin: String,
}

impl Upstream {
    /// Parse a loopback `http://` URL. Anything else is refused.
    pub fn from_url(url: &str) -> Result<Self, KeltaError> {
        let uri: Uri =
            url.parse().map_err(|_| KeltaError::invalid(format!("invalid upstream URL `{url}`")))?;
        if uri.scheme_str() != Some("http") {
            return Err(KeltaError::invalid("the web proxy only supports http:// upstreams"));
        }
        let host = uri.host().ok_or_else(|| KeltaError::invalid("upstream URL without host"))?;
        if !is_loopback_host(host) {
            return Err(KeltaError::invalid(format!(
                "the web proxy only proxies loopback hosts, not `{host}`"
            )));
        }
        let port = uri.port_u16().unwrap_or(80);
        let host =
            if host.contains(':') && !host.starts_with('[') { format!("[{host}]") } else { host.to_owned() };
        let authority = format!("{host}:{port}");
        Ok(Self { origin: format!("http://{authority}"), authority })
    }
}

static REGISTRY: LazyLock<RwLock<HashMap<String, Upstream>>> = LazyLock::new(|| RwLock::new(HashMap::new()));

static CLIENT: LazyLock<Client<HttpConnector, Body>> =
    LazyLock::new(|| Client::builder(TokioExecutor::new()).build_http());

/// Own listener: `(port, accept task)`.
static LISTENER: LazyLock<Mutex<Option<(u16, tokio::task::AbortHandle)>>> =
    LazyLock::new(|| Mutex::new(None));

/// Register `instance` → `upstream_url` (loopback only).
pub fn register(instance: &str, upstream_url: &str) -> Result<Upstream, KeltaError> {
    if !is_instance_id(instance) {
        return Err(KeltaError::invalid("proxy instance ids are 32 hex characters"));
    }
    let up = Upstream::from_url(upstream_url)?;
    REGISTRY.write().insert(instance.to_owned(), up.clone());
    Ok(up)
}

/// Forget `instance`; stops the own listener when nothing is proxied any more.
pub fn unregister(instance: &str) {
    let empty = {
        let mut r = REGISTRY.write();
        r.remove(instance);
        r.is_empty()
    };
    if empty && let Some((_, task)) = LISTENER.lock().take() {
        task.abort();
    }
}

pub fn is_registered(instance: &str) -> bool {
    REGISTRY.read().contains_key(instance)
}

/// `/proxy/<instance>` + the path and query of `upstream_url`.
pub fn proxy_path(instance: &str, upstream_url: &str) -> String {
    let rest = upstream_url
        .parse::<Uri>()
        .ok()
        .and_then(|u| u.path_and_query().map(|pq| pq.as_str().to_owned()))
        .unwrap_or_else(|| "/".into());
    let rest = if rest.starts_with('/') { rest } else { format!("/{rest}") };
    format!("/proxy/{instance}{rest}")
}

/// Port of the own loopback listener, started on demand (event-driven: first proxied instance).
pub async fn ensure_listener() -> Result<u16, KeltaError> {
    if let Some((port, _)) = LISTENER.lock().as_ref() {
        return Ok(*port);
    }
    let listener = TcpListener::bind(("127.0.0.1", 0))
        .await
        .map_err(|e| KeltaError::internal(format!("proxy listener: {e}")))?;
    let port =
        listener.local_addr().map_err(|e| KeltaError::internal(format!("proxy listener: {e}")))?.port();
    let app = router_with(Cfg { port: Some(port) });
    let task = tokio::spawn(async move {
        if let Err(e) = axum::serve(listener, app).await {
            tracing::warn!(error = %e, "web proxy listener stopped");
        }
    });
    let mut slot = LISTENER.lock();
    if let Some((existing, _)) = slot.as_ref() {
        // Lost a race: keep the first listener.
        task.abort();
        return Ok(*existing);
    }
    *slot = Some((port, task.abort_handle()));
    Ok(port)
}

/// Port of the own listener if running.
pub fn listener_port() -> Option<u16> {
    LISTENER.lock().as_ref().map(|(p, _)| *p)
}

#[derive(Debug, Clone, Copy)]
struct Cfg {
    /// Own listener: the `Host` header must be `127.0.0.1:<port>`/`localhost:<port>` (DNS rebinding).
    port: Option<u16>,
}

/// The proxy router (mountable by kelta-server; requests are `/<instance>/…` or `/proxy/<instance>/…`).
pub fn router() -> Router {
    router_with(Cfg { port: None })
}

fn router_with(cfg: Cfg) -> Router {
    Router::new().fallback(handle).with_state(cfg)
}

fn is_instance_id(s: &str) -> bool {
    s.len() == 32 && s.bytes().all(|b| b.is_ascii_hexdigit())
}

/// Split `/proxy/<id>/rest` or `/<id>/rest` into `(id, /rest)`.
fn split_instance(path: &str) -> Option<(String, String)> {
    let p = path.strip_prefix("/proxy").filter(|r| r.starts_with('/')).unwrap_or(path);
    let p = p.strip_prefix('/')?;
    let (id, rest) = match p.find('/') {
        Some(i) => (&p[..i], &p[i..]),
        None => (p, "/"),
    };
    is_instance_id(id).then(|| (id.to_ascii_lowercase(), rest.to_owned()))
}

fn error(status: StatusCode, msg: &str) -> Response {
    (status, [(header::CONTENT_TYPE, "text/plain; charset=utf-8")], msg.to_owned()).into_response()
}

/// Resolve which instance a request targets. Falls back to the `Referer` (path-absolute subresource
/// URLs of the proxied page) and, when exactly one instance is proxied, to that one.
fn resolve(req: &Request) -> Option<(String, Upstream, String)> {
    let path = req.uri().path();
    let query = req.uri().query().map(|q| format!("?{q}")).unwrap_or_default();
    let reg = REGISTRY.read();
    if let Some((id, rest)) = split_instance(path)
        && let Some(up) = reg.get(&id)
    {
        return Some((id, up.clone(), format!("{rest}{query}")));
    }
    let from_referer = req
        .headers()
        .get(header::REFERER)
        .and_then(|v| v.to_str().ok())
        .and_then(|r| r.parse::<Uri>().ok())
        .and_then(|u| split_instance(u.path()))
        .and_then(|(id, _)| reg.get(&id).map(|up| (id, up.clone())));
    let fallback = from_referer.or_else(|| {
        (reg.len() == 1).then(|| reg.iter().next().map(|(k, v)| (k.clone(), v.clone()))).flatten()
    })?;
    Some((fallback.0, fallback.1, format!("{path}{query}")))
}

fn host_ok(cfg: Cfg, headers: &HeaderMap) -> bool {
    let Some(port) = cfg.port else { return true };
    let Some(host) = headers.get(header::HOST).and_then(|v| v.to_str().ok()) else { return false };
    host == format!("127.0.0.1:{port}") || host == format!("localhost:{port}")
}

async fn handle(State(cfg): State<Cfg>, req: Request) -> Response {
    if !host_ok(cfg, req.headers()) {
        return error(StatusCode::MISDIRECTED_REQUEST, "unexpected Host header");
    }
    let Some((id, upstream, rest)) = resolve(&req) else {
        return error(StatusCode::NOT_FOUND, "unknown web tool instance");
    };
    let result = if is_upgrade(req.headers()) {
        websocket(req, &id, &upstream, &rest).await
    } else {
        forward(req, &id, &upstream, &rest).await
    };
    result.unwrap_or_else(|e| error(StatusCode::BAD_GATEWAY, &format!("web tool unreachable: {}", e.message)))
}

fn is_upgrade(h: &HeaderMap) -> bool {
    let upgrade = h
        .get(header::UPGRADE)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v.eq_ignore_ascii_case("websocket"));
    let conn = h
        .get(header::CONNECTION)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v.split(',').any(|t| t.trim().eq_ignore_ascii_case("upgrade")));
    upgrade && conn
}

const HOP_BY_HOP: &[&str] = &[
    "connection",
    "keep-alive",
    "proxy-authenticate",
    "proxy-authorization",
    "proxy-connection",
    "te",
    "trailer",
    "transfer-encoding",
    "upgrade",
];

fn request_headers(src: &HeaderMap, upstream: &Upstream, keep_upgrade: bool) -> HeaderMap {
    let mut out = HeaderMap::new();
    for (k, v) in src {
        let name = k.as_str();
        if (!keep_upgrade || !matches!(name, "connection" | "upgrade")) && HOP_BY_HOP.contains(&name) {
            continue;
        }
        if name == "host" {
            continue;
        }
        if name == "origin" {
            if let Ok(o) = HeaderValue::from_str(&upstream.origin) {
                out.append(header::ORIGIN, o);
            }
            continue;
        }
        out.append(k.clone(), v.clone());
    }
    if let Ok(h) = HeaderValue::from_str(&upstream.authority) {
        out.insert(header::HOST, h);
    }
    out
}

/// Remove `frame-ancestors` from a CSP value; `None` when nothing is left.
pub fn strip_frame_ancestors(csp: &str) -> Option<String> {
    let kept: Vec<&str> = csp
        .split(';')
        .map(str::trim)
        .filter(|d| !d.is_empty())
        .filter(|d| !d.to_ascii_lowercase().starts_with("frame-ancestors"))
        .collect();
    (!kept.is_empty()).then(|| kept.join("; "))
}

fn rewrite_location(loc: &str, id: &str, upstream: &Upstream) -> String {
    let prefix = format!("/proxy/{id}");
    let port = upstream.authority.rsplit(':').next().unwrap_or("");
    for origin in
        [upstream.origin.clone(), format!("http://localhost:{port}"), format!("http://127.0.0.1:{port}")]
    {
        if let Some(rest) = loc.strip_prefix(&origin) {
            return if rest.is_empty() { format!("{prefix}/") } else { format!("{prefix}{rest}") };
        }
    }
    if loc.starts_with('/') && !loc.starts_with("//") {
        return format!("{prefix}{loc}");
    }
    loc.to_owned()
}

/// Response header filter: drops hop-by-hop, `X-Frame-Options`, CSP `frame-ancestors`; rewrites `Location`.
pub fn response_headers(src: &HeaderMap, id: &str, upstream: &Upstream, keep_upgrade: bool) -> HeaderMap {
    let mut out = HeaderMap::new();
    for (k, v) in src {
        let name = k.as_str();
        if (!keep_upgrade || !matches!(name, "connection" | "upgrade")) && HOP_BY_HOP.contains(&name) {
            continue;
        }
        if name == "x-frame-options" {
            continue;
        }
        if name == "content-security-policy" || name == "content-security-policy-report-only" {
            if let Some(kept) = v.to_str().ok().and_then(strip_frame_ancestors)
                && let Ok(val) = HeaderValue::from_str(&kept)
            {
                out.append(k.clone(), val);
            }
            continue;
        }
        if name == "location" {
            if let Some(val) = v
                .to_str()
                .ok()
                .map(|l| rewrite_location(l, id, upstream))
                .and_then(|l| HeaderValue::from_str(&l).ok())
            {
                out.append(header::LOCATION, val);
            }
            continue;
        }
        out.append(k.clone(), v.clone());
    }
    out
}

async fn forward(req: Request, id: &str, upstream: &Upstream, rest: &str) -> Result<Response, KeltaError> {
    let (parts, body) = req.into_parts();
    let uri: Uri = format!("{}{}", upstream.origin, rest)
        .parse()
        .map_err(|_| KeltaError::invalid("invalid proxied path"))?;
    let mut out = axum::http::Request::builder()
        .method(parts.method.clone())
        .uri(uri)
        .body(body)
        .map_err(|e| KeltaError::internal(e.to_string()))?;
    *out.headers_mut() = request_headers(&parts.headers, upstream, false);
    let resp = CLIENT.request(out).await.map_err(|e| KeltaError::network(e.to_string()))?;
    let (rparts, rbody) = resp.into_parts();
    let mut response = Response::new(Body::new(rbody));
    *response.status_mut() = rparts.status;
    *response.headers_mut() = response_headers(&rparts.headers, id, upstream, false);
    Ok(response)
}

async fn websocket(
    mut req: Request,
    id: &str,
    upstream: &Upstream,
    rest: &str,
) -> Result<Response, KeltaError> {
    let client_upgrade = hyper::upgrade::on(&mut req);
    let stream = tokio::net::TcpStream::connect(&upstream.authority)
        .await
        .map_err(|e| KeltaError::network(format!("connect {}: {e}", upstream.authority)))?;
    let (mut sender, conn) = hyper::client::conn::http1::handshake::<_, Empty<Bytes>>(TokioIo::new(stream))
        .await
        .map_err(|e| KeltaError::network(e.to_string()))?;
    tokio::spawn(async move {
        let _ = conn.with_upgrades().await;
    });
    let mut up_req = axum::http::Request::builder()
        .method(Method::GET)
        .uri(rest)
        .body(Empty::<Bytes>::new())
        .map_err(|e| KeltaError::internal(e.to_string()))?;
    *up_req.headers_mut() = request_headers(req.headers(), upstream, true);
    let mut up_resp = sender.send_request(up_req).await.map_err(|e| KeltaError::network(e.to_string()))?;
    let status = up_resp.status();
    let headers =
        response_headers(up_resp.headers(), id, upstream, status == StatusCode::SWITCHING_PROTOCOLS);
    if status != StatusCode::SWITCHING_PROTOCOLS {
        let mut response = Response::new(Body::new(up_resp.into_body()));
        *response.status_mut() = status;
        *response.headers_mut() = headers;
        return Ok(response);
    }
    let upstream_upgrade = hyper::upgrade::on(&mut up_resp);
    tokio::spawn(async move {
        let (Ok(client), Ok(server)) = tokio::join!(client_upgrade, upstream_upgrade) else { return };
        let mut client = TokioIo::new(client);
        let mut server = TokioIo::new(server);
        let _ = tokio::io::copy_bidirectional(&mut client, &mut server).await;
    });
    let mut response = Response::new(Body::empty());
    *response.status_mut() = StatusCode::SWITCHING_PROTOCOLS;
    *response.headers_mut() = headers;
    Ok(response)
}

/// Header names a HEAD probe looks at (embed `auto`).
pub fn blocks_framing(headers: &HeaderMap) -> bool {
    headers.contains_key(HeaderName::from_static("x-frame-options"))
        || headers
            .get_all(header::CONTENT_SECURITY_POLICY)
            .iter()
            .filter_map(|v| v.to_str().ok())
            .any(|v| v.to_ascii_lowercase().contains("frame-ancestors"))
}

/// HEAD probe of a loopback/plain-http URL (2 s cap). `Ok(true)` = framing is blocked.
pub async fn probe_http(url: &str) -> Result<bool, KeltaError> {
    let uri: Uri = url.parse().map_err(|_| KeltaError::invalid(format!("invalid URL `{url}`")))?;
    let req = axum::http::Request::builder()
        .method(Method::HEAD)
        .uri(uri)
        .body(Body::empty())
        .map_err(|e| KeltaError::internal(e.to_string()))?;
    // one-shot: probe deadline armed by tool_open
    let resp = tokio::time::timeout(std::time::Duration::from_secs(2), CLIENT.request(req))
        .await
        .map_err(|_| KeltaError::timeout("HEAD probe timed out"))?
        .map_err(|e| KeltaError::network(e.to_string()))?;
    Ok(blocks_framing(resp.headers()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn upstreams_are_loopback_only() {
        assert!(Upstream::from_url("http://127.0.0.1:3011/x").is_ok());
        assert!(Upstream::from_url("http://localhost:3011/").is_ok());
        assert!(Upstream::from_url("http://example.com/").is_err());
        assert!(Upstream::from_url("https://127.0.0.1/").is_err());
        assert_eq!(Upstream::from_url("http://[::1]:9/").unwrap().authority, "[::1]:9");
    }

    #[test]
    fn paths_split() {
        let id = "0123456789abcdef0123456789abcdef";
        assert_eq!(split_instance(&format!("/proxy/{id}/a/b")), Some((id.into(), "/a/b".into())));
        assert_eq!(split_instance(&format!("/{id}")), Some((id.into(), "/".into())));
        assert_eq!(split_instance("/proxy/nothex/a"), None);
        assert_eq!(proxy_path(id, "http://127.0.0.1:1/x?token=1"), format!("/proxy/{id}/x?token=1"));
    }

    #[test]
    fn csp_and_location() {
        assert_eq!(
            strip_frame_ancestors("default-src 'self'; frame-ancestors 'none'; img-src *").as_deref(),
            Some("default-src 'self'; img-src *")
        );
        assert_eq!(strip_frame_ancestors("frame-ancestors 'none'"), None);
        let up = Upstream::from_url("http://127.0.0.1:4000/").unwrap();
        assert_eq!(rewrite_location("http://127.0.0.1:4000/login", "ab", &up), "/proxy/ab/login");
        assert_eq!(rewrite_location("http://localhost:4000", "ab", &up), "/proxy/ab/");
        assert_eq!(rewrite_location("/x?y", "ab", &up), "/proxy/ab/x?y");
        assert_eq!(rewrite_location("https://other/x", "ab", &up), "https://other/x");
    }
}

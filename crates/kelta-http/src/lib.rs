//! # kelta-http
//!
//! Shared `reqwest` client, per-account [`HttpCtx`], Markdown → sanitized HTML, the
//! [`provider::ProviderFactory`] trait and provider authentication helpers (ARCHITECTURE §8.3).
//!
//! [`HttpCtx`] adds, on top of the plain client: a per-account concurrency semaphore, `Retry-After`
//! and rate-limit header handling, exponential backoff with jitter, an exact-URL ETag LRU,
//! `401 → NeedsAuth` and `offline → Network`. Nothing here owns a timer: waits are one-shot sleeps
//! armed by a rate-limit response (`// one-shot:` comments in `ctx.rs`).

use std::collections::BTreeMap;
use std::time::Duration;

pub mod auth;
mod ctx;
pub mod graphql;
pub mod markdown;
pub mod oauth;
pub mod provider;
pub mod util;

pub use auth::{AuthScheme, Authed};
pub use ctx::{HttpCtx, retry_after_ms, status_error};
pub use provider::ProviderFactory;

/// `kelta/<version>`.
pub fn default_user_agent() -> String {
    format!("kelta/{}", env!("CARGO_PKG_VERSION"))
}

/// One shared client for the whole app (20 s timeout, pool idle 30 s).
#[derive(Clone)]
pub struct HttpClient {
    inner: reqwest::Client,
}

impl HttpClient {
    pub fn new(user_agent: &str) -> Self {
        Self::with_timeout(user_agent, Some(Duration::from_secs(20)))
    }

    /// Like [`HttpClient::new`] with an explicit request timeout. `None` = no client-side timeout
    /// and no connection pooling, so the client arms no timer at all (tests that run under paused
    /// tokio time would otherwise see the clock auto-advance to reqwest's own timers).
    pub fn with_timeout(user_agent: &str, timeout: Option<Duration>) -> Self {
        let mut b = reqwest::Client::builder()
            .user_agent(user_agent)
            .pool_idle_timeout(Duration::from_secs(30))
            // reqwest strips only Authorization on cross-host redirects, not PRIVATE-TOKEN etc.
            .redirect(reqwest::redirect::Policy::custom(|a| {
                if a.previous().last().is_some_and(|p| p.host_str() == a.url().host_str()) {
                    a.follow()
                } else {
                    a.stop()
                }
            }));
        match timeout {
            Some(t) => b = b.timeout(t),
            None => b = b.pool_max_idle_per_host(0),
        }
        let inner = b.build().unwrap_or_else(|e| {
            tracing::warn!(error = %e, "http client builder failed; using defaults");
            reqwest::Client::new()
        });
        Self { inner }
    }

    pub fn reqwest(&self) -> &reqwest::Client {
        &self.inner
    }
}

/// Per-account limits.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HttpPolicy {
    /// Concurrent requests per account.
    pub max_concurrent: usize,
    /// Upper bound for exponential backoff.
    pub max_backoff: Duration,
    /// Backoff applied to secondary rate limits without a header.
    pub secondary_backoff: Duration,
    /// ETag LRU capacity (exact URL).
    pub etag_capacity: usize,
}

impl Default for HttpPolicy {
    fn default() -> Self {
        Self {
            max_concurrent: 4,
            max_backoff: Duration::from_secs(600),
            secondary_backoff: Duration::from_secs(60),
            etag_capacity: 256,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Method {
    Get,
    Post,
    Put,
    Patch,
    Delete,
    Head,
}

impl Method {
    fn to_reqwest(self) -> reqwest::Method {
        match self {
            Self::Get => reqwest::Method::GET,
            Self::Post => reqwest::Method::POST,
            Self::Put => reqwest::Method::PUT,
            Self::Patch => reqwest::Method::PATCH,
            Self::Delete => reqwest::Method::DELETE,
            Self::Head => reqwest::Method::HEAD,
        }
    }

    /// Safe to repeat after a transport failure or a 5xx.
    pub fn is_idempotent(self) -> bool {
        matches!(self, Self::Get | Self::Head | Self::Put | Self::Delete)
    }
}

/// A provider request (decoupled from reqwest so the transport can evolve).
#[derive(Clone, PartialEq)]
pub struct HttpRequest {
    pub method: Method,
    pub url: String,
    pub headers: Vec<(String, String)>,
    pub query: Vec<(String, String)>,
    pub json: Option<serde_json::Value>,
    /// Use the ETag cache for this GET.
    pub etag: bool,
}

/// Header names whose values never appear in `Debug` output.
fn is_secret_header(name: &str) -> bool {
    let n = name.to_ascii_lowercase();
    n == "authorization" || n == "private-token" || n == "x-redmine-api-key" || n.contains("token")
}

impl std::fmt::Debug for HttpRequest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let headers: Vec<(&str, &str)> = self
            .headers
            .iter()
            .map(|(k, v)| (k.as_str(), if is_secret_header(k) { "[redacted]" } else { v.as_str() }))
            .collect();
        f.debug_struct("HttpRequest")
            .field("method", &self.method)
            .field("url", &kelta_proto::redact::redact_url(&self.url))
            .field("headers", &headers)
            .field("query", &self.query.iter().map(|(k, _)| k.as_str()).collect::<Vec<_>>())
            .field("json", &self.json.is_some())
            .field("etag", &self.etag)
            .finish()
    }
}

impl HttpRequest {
    pub fn new(method: Method, url: impl Into<String>) -> Self {
        Self { method, url: url.into(), headers: Vec::new(), query: Vec::new(), json: None, etag: false }
    }
    pub fn get(url: impl Into<String>) -> Self {
        Self::new(Method::Get, url)
    }
    pub fn post(url: impl Into<String>) -> Self {
        Self::new(Method::Post, url)
    }
    pub fn put(url: impl Into<String>) -> Self {
        Self::new(Method::Put, url)
    }
    pub fn patch(url: impl Into<String>) -> Self {
        Self::new(Method::Patch, url)
    }
    pub fn delete(url: impl Into<String>) -> Self {
        Self::new(Method::Delete, url)
    }
    pub fn header(mut self, k: impl Into<String>, v: impl Into<String>) -> Self {
        self.headers.push((k.into(), v.into()));
        self
    }
    pub fn bearer(self, token: &str) -> Self {
        self.header("Authorization", format!("Bearer {token}"))
    }
    pub fn basic(self, user: &str, password: &str) -> Self {
        use base64::Engine as _;
        let enc = base64::engine::general_purpose::STANDARD.encode(format!("{user}:{password}"));
        self.header("Authorization", format!("Basic {enc}"))
    }
    pub fn query(mut self, k: impl Into<String>, v: impl Into<String>) -> Self {
        self.query.push((k.into(), v.into()));
        self
    }
    pub fn json(mut self, body: serde_json::Value) -> Self {
        self.json = Some(body);
        self
    }
    pub fn with_etag(mut self) -> Self {
        self.etag = true;
        self
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct HttpResponse<T> {
    /// Wire status (`304` when served from the ETag cache).
    pub status: u16,
    /// Lowercased header names.
    pub headers: BTreeMap<String, String>,
    pub body: T,
    pub etag: Option<String>,
    /// 304 answered from the ETag cache: `body` is the cached body of the previous 200.
    pub not_modified: bool,
}

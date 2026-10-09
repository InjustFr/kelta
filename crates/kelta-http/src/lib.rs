//! # kelta-http
//!
//! Shared `reqwest` client, per-account [`HttpCtx`], Markdown → sanitized HTML, and the
//! [`provider::ProviderFactory`] trait (ARCHITECTURE §8.3).
//!
//! Scaffold FUNCTIONAL BASELINE: plain send with status → `KeltaError` mapping and a concurrency
//! semaphore; no retry, backoff or ETag cache yet (L5 upgrades behind the same API).

use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;

use kelta_proto::error::{ErrorCode, KeltaError};
use kelta_proto::ids::AccountId;
use serde::de::DeserializeOwned;
use tokio::sync::Semaphore;

pub mod markdown;
pub mod provider;

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
        let inner = reqwest::Client::builder()
            .user_agent(user_agent)
            .timeout(Duration::from_secs(20))
            .pool_idle_timeout(Duration::from_secs(30))
            .build()
            .unwrap_or_else(|e| {
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
}

/// A provider request (decoupled from reqwest so the transport can evolve).
#[derive(Debug, Clone, PartialEq)]
pub struct HttpRequest {
    pub method: Method,
    pub url: String,
    pub headers: Vec<(String, String)>,
    pub query: Vec<(String, String)>,
    pub json: Option<serde_json::Value>,
    /// Use the ETag cache for this GET (L5).
    pub etag: bool,
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
    pub status: u16,
    /// Lowercased header names.
    pub headers: BTreeMap<String, String>,
    pub body: T,
    pub etag: Option<String>,
    /// 304 served from the ETag cache (L5).
    pub not_modified: bool,
}

/// Per-account context: shared client + semaphore (+ rate-limit state in L5).
#[derive(Clone)]
pub struct HttpCtx {
    client: HttpClient,
    account_id: AccountId,
    policy: HttpPolicy,
    permits: Arc<Semaphore>,
}

impl HttpCtx {
    pub fn new(client: HttpClient, account_id: AccountId, policy: HttpPolicy) -> Self {
        let permits = Arc::new(Semaphore::new(policy.max_concurrent.max(1)));
        Self { client, account_id, policy, permits }
    }

    pub fn account_id(&self) -> &AccountId {
        &self.account_id
    }

    pub fn policy(&self) -> &HttpPolicy {
        &self.policy
    }

    pub fn client(&self) -> &HttpClient {
        &self.client
    }

    /// Send and deserialize a JSON body. Empty bodies deserialize from `null`.
    pub async fn send_json<T: DeserializeOwned>(
        &self,
        req: HttpRequest,
    ) -> Result<HttpResponse<T>, KeltaError> {
        let raw = self.send_text(req).await?;
        let body: T = if raw.body.trim().is_empty() {
            serde_json::from_value(serde_json::Value::Null)
        } else {
            serde_json::from_str(&raw.body)
        }
        .map_err(|e| KeltaError::upstream(format!("invalid JSON from {}: {e}", self.account_id)))?;
        Ok(HttpResponse {
            status: raw.status,
            headers: raw.headers,
            body,
            etag: raw.etag,
            not_modified: raw.not_modified,
        })
    }

    /// Send and return the body as text. Non-2xx statuses become errors.
    pub async fn send_text(&self, req: HttpRequest) -> Result<HttpResponse<String>, KeltaError> {
        let _permit =
            self.permits.acquire().await.map_err(|_| KeltaError::cancelled("http context closed"))?;
        let mut rb = self.client.inner.request(req.method.to_reqwest(), &req.url);
        for (k, v) in &req.headers {
            rb = rb.header(k, v);
        }
        if !req.query.is_empty() {
            rb = rb.query(&req.query);
        }
        if let Some(j) = &req.json {
            rb = rb.json(j);
        }
        let resp = rb.send().await.map_err(map_reqwest_error)?;
        let status = resp.status().as_u16();
        let headers: BTreeMap<String, String> = resp
            .headers()
            .iter()
            .filter_map(|(k, v)| v.to_str().ok().map(|v| (k.as_str().to_ascii_lowercase(), v.to_owned())))
            .collect();
        let body = resp.text().await.map_err(map_reqwest_error)?;
        if !(200..300).contains(&status) {
            return Err(status_error(status, &headers, &body));
        }
        let etag = headers.get("etag").cloned();
        Ok(HttpResponse { status, headers, body, etag, not_modified: false })
    }
}

fn map_reqwest_error(e: reqwest::Error) -> KeltaError {
    if e.is_timeout() {
        KeltaError::timeout(format!("request timed out: {}", short(&e)))
    } else if e.is_connect() || e.is_request() {
        KeltaError::network(format!("network error: {}", short(&e)))
    } else if e.is_decode() || e.is_body() {
        KeltaError::upstream(format!("bad response body: {}", short(&e)))
    } else {
        KeltaError::network(short(&e))
    }
}

/// Error text without the URL (may carry tokens).
fn short(e: &reqwest::Error) -> String {
    let mut s = e.to_string();
    if let Some(u) = e.url() {
        s = s.replace(u.as_str(), &kelta_proto::redact::redact_url(u.as_str()));
    }
    s
}

/// `Retry-After` (seconds or HTTP date — only seconds supported in the baseline) in ms.
pub fn retry_after_ms(headers: &BTreeMap<String, String>) -> Option<u64> {
    headers.get("retry-after").and_then(|v| v.trim().parse::<u64>().ok()).map(|s| s * 1000)
}

/// Map a non-2xx status to a `KeltaError`.
pub fn status_error(status: u16, headers: &BTreeMap<String, String>, body: &str) -> KeltaError {
    let snippet: String = body.chars().take(300).collect();
    let code = match status {
        401 => ErrorCode::NeedsAuth,
        403 if headers.get("x-ratelimit-remaining").map(String::as_str) == Some("0") => {
            ErrorCode::RateLimited
        }
        403 => ErrorCode::PermissionDenied,
        404 => ErrorCode::NotFound,
        409 | 412 => ErrorCode::Conflict,
        400 | 422 => ErrorCode::InvalidArgument,
        429 => ErrorCode::RateLimited,
        408 | 504 => ErrorCode::Timeout,
        _ => ErrorCode::Upstream,
    };
    let mut e = KeltaError::new(code, format!("HTTP {status}: {snippet}"))
        .with_detail(serde_json::json!({ "status": status }));
    if code == ErrorCode::RateLimited {
        e.retry_after_ms = retry_after_ms(headers);
    }
    e
}

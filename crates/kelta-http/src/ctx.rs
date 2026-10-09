//! `HttpCtx`: per-account semaphore, rate-limit handling, backoff with jitter, ETag LRU.
//!
//! Waiting policy (ARCHITECTURE §8.3): a rate-limit answer records `blocked_until` for the whole
//! account. Short waits (<= [`INLINE_WAIT_MAX`]) are slept inline and the request is retried;
//! longer ones fail fast with `RateLimited { retry_after_ms }` so the scheduler can pause the
//! account instead of holding a request open for minutes.

use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;
use std::time::Duration;

use kelta_proto::error::{ErrorCode, KeltaError};
use kelta_proto::ids::AccountId;
use parking_lot::Mutex;
use serde::de::DeserializeOwned;
use tokio::sync::Semaphore;
use tokio::time::Instant;

use crate::{HttpClient, HttpPolicy, HttpRequest, HttpResponse, Method};

/// Longest wait slept inside a request; anything longer is reported to the caller.
const INLINE_WAIT_MAX: Duration = Duration::from_secs(5);
/// Retries after the first attempt (rate limits, 502/503/504, timeouts of idempotent requests).
const MAX_RETRIES: u32 = 3;
/// First exponential-backoff step (without jitter).
const BACKOFF_BASE: Duration = Duration::from_millis(250);
/// Stored error bodies are cut to this many bytes.
const ERROR_BODY_MAX: usize = 4096;

struct EtagEntry {
    etag: String,
    body: String,
    tick: u64,
}

/// Exact-URL ETag cache with least-recently-used eviction (capacity <= a few hundred, so eviction
/// scans the map instead of keeping a linked list).
struct EtagCache {
    map: HashMap<String, EtagEntry>,
    tick: u64,
    capacity: usize,
}

impl EtagCache {
    fn new(capacity: usize) -> Self {
        Self { map: HashMap::new(), tick: 0, capacity: capacity.max(1) }
    }

    fn get(&mut self, key: &str) -> Option<(String, String)> {
        self.tick += 1;
        let tick = self.tick;
        self.map.get_mut(key).map(|e| {
            e.tick = tick;
            (e.etag.clone(), e.body.clone())
        })
    }

    fn put(&mut self, key: String, etag: String, body: String) {
        self.tick += 1;
        if !self.map.contains_key(&key)
            && self.map.len() >= self.capacity
            && let Some(oldest) = self.map.iter().min_by_key(|(_, e)| e.tick).map(|(k, _)| k.clone())
        {
            self.map.remove(&oldest);
        }
        self.map.insert(key, EtagEntry { etag, body, tick: self.tick });
    }
}

struct Shared {
    permits: Semaphore,
    blocked_until: Mutex<Option<Instant>>,
    etags: Mutex<EtagCache>,
}

/// Per-account context: shared client + semaphore + rate-limit state + ETag cache. Cheap to clone;
/// clones share the state.
#[derive(Clone)]
pub struct HttpCtx {
    client: HttpClient,
    account_id: AccountId,
    policy: HttpPolicy,
    shared: Arc<Shared>,
}

/// Result of one wire attempt.
enum Attempt {
    Done(HttpResponse<String>),
    Transport(KeltaError),
    Status { status: u16, headers: BTreeMap<String, String>, body: String },
}

impl HttpCtx {
    pub fn new(client: HttpClient, account_id: AccountId, policy: HttpPolicy) -> Self {
        let shared = Arc::new(Shared {
            permits: Semaphore::new(policy.max_concurrent.max(1)),
            blocked_until: Mutex::new(None),
            etags: Mutex::new(EtagCache::new(policy.etag_capacity)),
        });
        Self { client, account_id, policy, shared }
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

    /// Time left before the account may send again (set by a rate-limit answer).
    pub fn blocked_for(&self) -> Option<Duration> {
        let until = (*self.shared.blocked_until.lock())?;
        let left = until.saturating_duration_since(Instant::now());
        (!left.is_zero()).then_some(left)
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

    /// Send and return the body as text. Non-2xx statuses become errors; rate limits and transient
    /// failures are retried (see the module docs).
    pub async fn send_text(&self, req: HttpRequest) -> Result<HttpResponse<String>, KeltaError> {
        let cache_key = (req.etag && req.method == Method::Get).then(|| cache_key(&req));
        let mut attempt = 0u32;
        loop {
            self.wait_unblocked().await?;
            match self.attempt(&req, cache_key.as_deref()).await {
                Attempt::Done(resp) => return Ok(resp),
                Attempt::Transport(err) => {
                    if err.code == ErrorCode::Timeout && req.method.is_idempotent() && attempt < MAX_RETRIES {
                        let wait = self.backoff(attempt);
                        if wait <= INLINE_WAIT_MAX {
                            attempt += 1;
                            sleep_one_shot(wait).await;
                            continue;
                        }
                    }
                    return Err(err);
                }
                Attempt::Status { status, headers, body } => {
                    if let Some(wait) = self.classify_wait(req.method, status, &headers, &body, attempt) {
                        if wait.rate_limit {
                            self.block_for(wait.dur);
                        }
                        if attempt < MAX_RETRIES && wait.dur <= INLINE_WAIT_MAX {
                            attempt += 1;
                            sleep_one_shot(wait.dur).await;
                            continue;
                        }
                    }
                    return Err(status_error(status, &headers, &body));
                }
            }
        }
    }

    /// Sleep out a short account-wide block; fail fast with `RateLimited` for a long one.
    async fn wait_unblocked(&self) -> Result<(), KeltaError> {
        let Some(left) = self.blocked_for() else { return Ok(()) };
        if left <= INLINE_WAIT_MAX {
            sleep_one_shot(left).await;
            return Ok(());
        }
        Err(KeltaError::rate_limited(
            format!("{} is rate limited; retry in {} s", self.account_id, left.as_secs()),
            Some(left.as_millis() as u64),
        ))
    }

    fn block_for(&self, dur: Duration) {
        if dur.is_zero() {
            return;
        }
        let until = Instant::now() + dur;
        let mut g = self.shared.blocked_until.lock();
        if g.is_none_or(|cur| until > cur) {
            *g = Some(until);
        }
    }

    /// Exponential backoff with +-25 % jitter, capped at `policy.max_backoff`.
    fn backoff(&self, attempt: u32) -> Duration {
        let factor = 1u32 << attempt.min(16);
        let raw = BACKOFF_BASE.saturating_mul(factor).min(self.policy.max_backoff);
        raw.mul_f64(0.75 + 0.5 * jitter_unit())
    }

    /// How long to wait before retrying this status, if it is a retryable one.
    fn classify_wait(
        &self,
        method: Method,
        status: u16,
        headers: &BTreeMap<String, String>,
        body: &str,
        attempt: u32,
    ) -> Option<Wait> {
        let max = self.policy.max_backoff;
        let hinted = wait_hint(headers).map(|d| d.min(max));
        if status == 429 || is_rate_limited(status, headers, body) {
            let dur = match hinted {
                Some(d) => d,
                None if is_secondary(body) || status == 403 => self.policy.secondary_backoff.min(max),
                None => self.backoff(attempt),
            };
            return Some(Wait { dur, rate_limit: true });
        }
        if status == 503
            && let Some(d) = hinted
        {
            return Some(Wait { dur: d, rate_limit: false });
        }
        if matches!(status, 502..=504) && method.is_idempotent() {
            return Some(Wait { dur: self.backoff(attempt), rate_limit: false });
        }
        None
    }

    async fn attempt(&self, req: &HttpRequest, cache_key: Option<&str>) -> Attempt {
        let Ok(_permit) = self.shared.permits.acquire().await else {
            return Attempt::Transport(KeltaError::cancelled("http context closed"));
        };
        let mut rb = self.client.reqwest().request(req.method.to_reqwest(), &req.url);
        for (k, v) in &req.headers {
            rb = rb.header(k, v);
        }
        if !req.query.is_empty() {
            rb = rb.query(&req.query);
        }
        if let Some(j) = &req.json {
            rb = rb.json(j);
        }
        let cached = cache_key.and_then(|k| self.shared.etags.lock().get(k));
        if let Some((etag, _)) = &cached
            && !req.headers.iter().any(|(k, _)| k.eq_ignore_ascii_case("if-none-match"))
        {
            rb = rb.header("If-None-Match", etag);
        }
        let resp = match rb.send().await {
            Ok(r) => r,
            Err(e) => return Attempt::Transport(map_reqwest_error(e)),
        };
        let status = resp.status().as_u16();
        let headers: BTreeMap<String, String> = resp
            .headers()
            .iter()
            .filter_map(|(k, v)| v.to_str().ok().map(|v| (k.as_str().to_ascii_lowercase(), v.to_owned())))
            .collect();
        let body = match resp.text().await {
            Ok(b) => b,
            Err(e) => return Attempt::Transport(map_reqwest_error(e)),
        };
        if status == 304
            && let Some((etag, cached_body)) = cached
        {
            return Attempt::Done(HttpResponse {
                status,
                headers,
                body: cached_body,
                etag: Some(etag),
                not_modified: true,
            });
        }
        if !(200..300).contains(&status) {
            return Attempt::Status { status, headers, body };
        }
        self.note_rate_limit(&headers);
        let etag = headers.get("etag").cloned();
        if let (Some(key), Some(tag)) = (cache_key, &etag) {
            self.shared.etags.lock().put(key.to_owned(), tag.clone(), body.clone());
        }
        Attempt::Done(HttpResponse { status, headers, body, etag, not_modified: false })
    }

    /// Proactive primary-limit handling: remaining == 0 blocks the account until the reset.
    fn note_rate_limit(&self, headers: &BTreeMap<String, String>) {
        let remaining = header_u64(headers, &["x-ratelimit-remaining", "ratelimit-remaining"]);
        if remaining == Some(0)
            && let Some(reset) = reset_wait(headers)
        {
            self.block_for(reset.min(self.policy.max_backoff));
        }
    }
}

struct Wait {
    dur: Duration,
    rate_limit: bool,
}

fn cache_key(req: &HttpRequest) -> String {
    let mut k = req.url.clone();
    for (i, (a, b)) in req.query.iter().enumerate() {
        k.push(if i == 0 { '?' } else { '&' });
        k.push_str(a);
        k.push('=');
        k.push_str(b);
    }
    k
}

async fn sleep_one_shot(d: Duration) {
    if d.is_zero() {
        return;
    }
    // one-shot: armed by a rate-limit / transient-failure response, bounded by INLINE_WAIT_MAX.
    tokio::time::sleep(d).await;
}

/// Uniform in `[0, 1)` from the std hasher's per-instance random keys (no `rand` dependency).
fn jitter_unit() -> f64 {
    use std::hash::{BuildHasher, RandomState};
    (RandomState::new().hash_one(0u8) >> 11) as f64 / (1u64 << 53) as f64
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

fn header_u64(headers: &BTreeMap<String, String>, names: &[&str]) -> Option<u64> {
    names.iter().find_map(|n| headers.get(*n)).and_then(|v| v.trim().parse::<u64>().ok())
}

/// Wait implied by `x-ratelimit-reset` / `ratelimit-reset`: an epoch (GitHub, GitLab) or a number
/// of seconds (IETF draft); anything above 10^9 is treated as an epoch.
fn reset_wait(headers: &BTreeMap<String, String>) -> Option<Duration> {
    let v = header_u64(headers, &["x-ratelimit-reset", "ratelimit-reset"])?;
    if v > 1_000_000_000 {
        let now = time::OffsetDateTime::now_utc().unix_timestamp().max(0) as u64;
        Some(Duration::from_secs(v.saturating_sub(now)))
    } else {
        Some(Duration::from_secs(v))
    }
}

/// `Retry-After` (delta seconds or HTTP date) as a duration.
fn retry_after(headers: &BTreeMap<String, String>) -> Option<Duration> {
    let v = headers.get("retry-after")?.trim();
    if let Ok(secs) = v.parse::<u64>() {
        return Some(Duration::from_secs(secs));
    }
    let at = time::OffsetDateTime::parse(v, &time::format_description::well_known::Rfc2822).ok()?;
    let delta = (at - time::OffsetDateTime::now_utc()).whole_seconds();
    Some(Duration::from_secs(delta.max(0) as u64))
}

/// Best wait hint in the response headers: `Retry-After`, else the rate-limit reset.
fn wait_hint(headers: &BTreeMap<String, String>) -> Option<Duration> {
    retry_after(headers).or_else(|| {
        // Only a drained window makes the reset header a wait.
        let drained = header_u64(headers, &["x-ratelimit-remaining", "ratelimit-remaining"]) == Some(0);
        drained.then(|| reset_wait(headers)).flatten()
    })
}

fn is_secondary(body: &str) -> bool {
    let b = body.to_ascii_lowercase();
    b.contains("secondary rate limit") || b.contains("abuse detection")
}

/// 403 that is really a rate limit (GitHub primary: remaining 0; secondary: message).
fn is_rate_limited(status: u16, headers: &BTreeMap<String, String>, body: &str) -> bool {
    status == 403
        && (header_u64(headers, &["x-ratelimit-remaining", "ratelimit-remaining"]) == Some(0)
            || is_secondary(body))
}

/// `Retry-After` in milliseconds (delta seconds or HTTP date).
pub fn retry_after_ms(headers: &BTreeMap<String, String>) -> Option<u64> {
    retry_after(headers).map(|d| d.as_millis() as u64)
}

/// Map a non-2xx status to a `KeltaError`. The (truncated) response body travels in
/// `detail.body` so providers can parse structured errors (Jira `errors`, Redmine 422).
pub fn status_error(status: u16, headers: &BTreeMap<String, String>, body: &str) -> KeltaError {
    let snippet: String = body.chars().take(300).collect();
    let code = match status {
        401 => ErrorCode::NeedsAuth,
        403 if is_rate_limited(status, headers, body) => ErrorCode::RateLimited,
        403 => ErrorCode::PermissionDenied,
        404 => ErrorCode::NotFound,
        409 | 412 => ErrorCode::Conflict,
        400 | 422 => ErrorCode::InvalidArgument,
        429 => ErrorCode::RateLimited,
        408 | 504 => ErrorCode::Timeout,
        _ => ErrorCode::Upstream,
    };
    let mut cut = body.len().min(ERROR_BODY_MAX);
    while !body.is_char_boundary(cut) {
        cut -= 1;
    }
    let mut e = KeltaError::new(code, format!("HTTP {status}: {snippet}"))
        .with_detail(serde_json::json!({ "status": status, "body": &body[..cut] }));
    if code == ErrorCode::RateLimited {
        e.retry_after_ms = wait_hint(headers).map(|d| d.as_millis() as u64);
    }
    e
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn etag_cache_evicts_least_recently_used() {
        let mut c = EtagCache::new(2);
        c.put("a".into(), "1".into(), "A".into());
        c.put("b".into(), "2".into(), "B".into());
        assert!(c.get("a").is_some()); // a is now fresher than b
        c.put("c".into(), "3".into(), "C".into());
        assert!(c.get("b").is_none());
        assert!(c.get("a").is_some());
        assert!(c.get("c").is_some());
    }

    #[test]
    fn jitter_is_a_unit_fraction() {
        for _ in 0..100 {
            let j = jitter_unit();
            assert!((0.0..1.0).contains(&j));
        }
    }

    #[test]
    fn retry_after_forms() {
        let mut h = BTreeMap::new();
        h.insert("retry-after".to_owned(), "7".to_owned());
        assert_eq!(retry_after_ms(&h), Some(7000));
        h.insert("retry-after".to_owned(), "Wed, 21 Oct 2015 07:28:00 GMT".to_owned());
        assert_eq!(retry_after_ms(&h), Some(0)); // in the past
    }

    #[test]
    fn reset_header_is_epoch_or_delta() {
        let mut h = BTreeMap::new();
        h.insert("x-ratelimit-remaining".to_owned(), "0".to_owned());
        h.insert("x-ratelimit-reset".to_owned(), "30".to_owned());
        assert_eq!(wait_hint(&h), Some(Duration::from_secs(30)));
        let in_20s = time::OffsetDateTime::now_utc().unix_timestamp() + 20;
        h.insert("x-ratelimit-reset".to_owned(), in_20s.to_string());
        let w = wait_hint(&h).unwrap();
        assert!(w >= Duration::from_secs(18) && w <= Duration::from_secs(20));
        h.insert("x-ratelimit-remaining".to_owned(), "12".to_owned());
        assert_eq!(wait_hint(&h), None);
    }

    #[test]
    fn debug_never_prints_tokens() {
        let r = HttpRequest::get("https://h/x?token=zzz").bearer("s3cret").header("PRIVATE-TOKEN", "glpat-1");
        let s = format!("{r:?}");
        assert!(!s.contains("s3cret") && !s.contains("glpat") && !s.contains("zzz"), "{s}");
    }
}

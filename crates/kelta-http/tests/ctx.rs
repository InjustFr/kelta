//! `HttpCtx` behaviour against a local wiremock server: Retry-After under paused time, backoff,
//! rate-limit headers, ETag reuse, 401, offline, concurrency. No real network.

use std::time::Duration;

use kelta_http::{HttpClient, HttpCtx, HttpPolicy, HttpRequest};
use kelta_proto::error::ErrorCode;
use kelta_proto::ids::AccountId;
use tokio::time::Instant;
use wiremock::matchers::{header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

/// No client-side timeout: under paused time the runtime would otherwise auto-advance the clock to
/// reqwest's own timeout while the (real) socket I/O is still in flight.
fn ctx_with(policy: HttpPolicy) -> HttpCtx {
    HttpCtx::new(HttpClient::with_timeout("kelta-test", None), AccountId::new("acc"), policy)
}

fn ctx() -> HttpCtx {
    ctx_with(HttpPolicy::default())
}

#[tokio::test(start_paused = true)]
async fn retry_after_is_honoured_then_request_succeeds() {
    let server = MockServer::start().await;
    Mock::given(path("/x"))
        .respond_with(ResponseTemplate::new(429).insert_header("retry-after", "2"))
        .up_to_n_times(1)
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(path("/x"))
        .respond_with(ResponseTemplate::new(200).set_body_string("ok"))
        .expect(1)
        .mount(&server)
        .await;
    let t0 = Instant::now();
    let r = ctx().send_text(HttpRequest::get(format!("{}/x", server.uri()))).await.unwrap();
    assert_eq!(r.body, "ok");
    let waited = t0.elapsed();
    assert!(waited >= Duration::from_secs(2), "waited only {waited:?}");
    assert!(waited < Duration::from_secs(3), "waited too long: {waited:?}");
}

#[tokio::test(start_paused = true)]
async fn long_retry_after_fails_fast_and_blocks_the_account() {
    let server = MockServer::start().await;
    Mock::given(path("/x"))
        .respond_with(ResponseTemplate::new(429).insert_header("retry-after", "120"))
        .expect(1)
        .mount(&server)
        .await;
    let c = ctx();
    let url = format!("{}/x", server.uri());
    let e = c.send_text(HttpRequest::get(&url)).await.unwrap_err();
    assert_eq!(e.code, ErrorCode::RateLimited);
    assert_eq!(e.retry_after_ms, Some(120_000));
    let t0 = Instant::now();
    // The second call never reaches the server (expect(1) above) and does not sleep.
    let e2 = c.send_text(HttpRequest::get(&url)).await.unwrap_err();
    assert_eq!(e2.code, ErrorCode::RateLimited);
    assert!(e2.retry_after_ms.unwrap() <= 120_000);
    assert!(t0.elapsed() < Duration::from_secs(1));
    assert!(c.blocked_for().is_some());
    // After the window the account is usable again.
    tokio::time::advance(Duration::from_secs(121)).await;
    assert!(c.blocked_for().is_none());
}

#[tokio::test(start_paused = true)]
async fn retries_are_capped() {
    let server = MockServer::start().await;
    Mock::given(path("/x"))
        .respond_with(ResponseTemplate::new(429).insert_header("retry-after", "1"))
        .expect(4) // first attempt + 3 retries
        .mount(&server)
        .await;
    let e = ctx().send_text(HttpRequest::get(format!("{}/x", server.uri()))).await.unwrap_err();
    assert_eq!(e.code, ErrorCode::RateLimited);
}

#[tokio::test(start_paused = true)]
async fn primary_rate_limit_403_waits_for_reset_header() {
    let server = MockServer::start().await;
    let reset = time::OffsetDateTime::now_utc().unix_timestamp() + 3;
    Mock::given(path("/x"))
        .respond_with(
            ResponseTemplate::new(403)
                .insert_header("x-ratelimit-remaining", "0")
                .insert_header("x-ratelimit-reset", reset.to_string().as_str()),
        )
        .up_to_n_times(1)
        .mount(&server)
        .await;
    Mock::given(path("/x"))
        .respond_with(ResponseTemplate::new(200).set_body_string("ok"))
        .mount(&server)
        .await;
    let t0 = Instant::now();
    let r = ctx().send_text(HttpRequest::get(format!("{}/x", server.uri()))).await.unwrap();
    assert_eq!(r.body, "ok");
    let waited = t0.elapsed();
    assert!(waited >= Duration::from_secs(1) && waited <= Duration::from_secs(5), "{waited:?}");
}

#[tokio::test(start_paused = true)]
async fn secondary_rate_limit_without_header_backs_off_at_least_a_minute() {
    let server = MockServer::start().await;
    Mock::given(path("/x"))
        .respond_with(ResponseTemplate::new(403).set_body_string("You have exceeded a secondary rate limit."))
        .expect(1)
        .mount(&server)
        .await;
    let e = ctx().send_text(HttpRequest::get(format!("{}/x", server.uri()))).await.unwrap_err();
    assert_eq!(e.code, ErrorCode::RateLimited);
    assert!(e.retry_after_ms.is_none() || e.retry_after_ms >= Some(60_000));
}

#[tokio::test(start_paused = true)]
async fn drained_window_on_a_2xx_blocks_following_calls() {
    let server = MockServer::start().await;
    let reset = time::OffsetDateTime::now_utc().unix_timestamp() + 90;
    Mock::given(path("/x"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("x-ratelimit-remaining", "0")
                .insert_header("x-ratelimit-reset", reset.to_string().as_str()),
        )
        .expect(1)
        .mount(&server)
        .await;
    let c = ctx();
    let url = format!("{}/x", server.uri());
    c.send_text(HttpRequest::get(&url)).await.unwrap();
    let e = c.send_text(HttpRequest::get(&url)).await.unwrap_err();
    assert_eq!(e.code, ErrorCode::RateLimited);
    assert!(e.retry_after_ms.unwrap() > 60_000);
}

#[tokio::test(start_paused = true)]
async fn transient_502_is_retried_with_jittered_backoff_for_get_only() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/g"))
        .respond_with(ResponseTemplate::new(502))
        .up_to_n_times(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/g"))
        .respond_with(ResponseTemplate::new(200).set_body_string("g"))
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/p"))
        .respond_with(ResponseTemplate::new(502))
        .expect(1)
        .mount(&server)
        .await;
    let c = ctx();
    let t0 = Instant::now();
    assert_eq!(c.send_text(HttpRequest::get(format!("{}/g", server.uri()))).await.unwrap().body, "g");
    let waited = t0.elapsed();
    assert!(waited >= Duration::from_millis(180) && waited <= Duration::from_millis(330), "{waited:?}");
    let e = c.send_text(HttpRequest::post(format!("{}/p", server.uri()))).await.unwrap_err();
    assert_eq!(e.code, ErrorCode::Upstream);
}

#[tokio::test]
async fn etag_is_sent_and_304_reuses_the_cached_body() {
    let server = MockServer::start().await;
    Mock::given(path("/l"))
        .and(header("if-none-match", "\"v1\""))
        .respond_with(ResponseTemplate::new(304).insert_header("x-poll-interval", "60"))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(path("/l"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("etag", "\"v1\"")
                .set_body_json(serde_json::json!([1, 2])),
        )
        .expect(1)
        .mount(&server)
        .await;
    let c = ctx();
    let url = format!("{}/l", server.uri());
    let a = c.send_json::<Vec<u32>>(HttpRequest::get(&url).with_etag()).await.unwrap();
    assert!(!a.not_modified);
    assert_eq!(a.status, 200);
    let b = c.send_json::<Vec<u32>>(HttpRequest::get(&url).with_etag()).await.unwrap();
    assert!(b.not_modified);
    assert_eq!(b.status, 304);
    assert_eq!(b.body, vec![1, 2]);
    assert_eq!(b.etag.as_deref(), Some("\"v1\""));
    assert_eq!(b.headers.get("x-poll-interval").map(String::as_str), Some("60"));
}

#[tokio::test]
async fn etag_cache_is_keyed_by_exact_url_and_query() {
    let server = MockServer::start().await;
    Mock::given(path("/l"))
        .respond_with(ResponseTemplate::new(200).insert_header("etag", "\"v1\"").set_body_string("x"))
        .expect(2)
        .mount(&server)
        .await;
    let c = ctx();
    let url = format!("{}/l", server.uri());
    c.send_text(HttpRequest::get(&url).query("page", "1").with_etag()).await.unwrap();
    // A different query is a different cache entry: no If-None-Match, so the server answers 200.
    let r = c.send_text(HttpRequest::get(&url).query("page", "2").with_etag()).await.unwrap();
    assert!(!r.not_modified);
}

#[tokio::test]
async fn requests_without_etag_flag_never_send_if_none_match() {
    let server = MockServer::start().await;
    Mock::given(path("/l"))
        .and(header("if-none-match", "\"v1\""))
        .respond_with(ResponseTemplate::new(500))
        .mount(&server)
        .await;
    Mock::given(path("/l"))
        .respond_with(ResponseTemplate::new(200).insert_header("etag", "\"v1\"").set_body_string("x"))
        .mount(&server)
        .await;
    let c = ctx();
    let url = format!("{}/l", server.uri());
    c.send_text(HttpRequest::get(&url)).await.unwrap();
    c.send_text(HttpRequest::get(&url)).await.unwrap();
}

#[tokio::test]
async fn unauthorized_is_needs_auth_without_retry() {
    let server = MockServer::start().await;
    Mock::given(path("/x"))
        .respond_with(ResponseTemplate::new(401).set_body_string("nope"))
        .expect(1)
        .mount(&server)
        .await;
    let e = ctx().send_text(HttpRequest::get(format!("{}/x", server.uri()))).await.unwrap_err();
    assert_eq!(e.code, ErrorCode::NeedsAuth);
    assert_eq!(e.detail.as_ref().and_then(|d| d["body"].as_str()), Some("nope"));
}

#[tokio::test]
async fn offline_is_network_without_retry() {
    let t0 = std::time::Instant::now();
    let e = ctx().send_text(HttpRequest::get("http://127.0.0.1:1/")).await.unwrap_err();
    assert_eq!(e.code, ErrorCode::Network, "{e:?}");
    assert!(t0.elapsed() < Duration::from_secs(2));
}

#[tokio::test]
async fn semaphore_serialises_requests_per_account() {
    let server = MockServer::start().await;
    Mock::given(path("/x"))
        .respond_with(ResponseTemplate::new(200).set_delay(Duration::from_millis(150)))
        .mount(&server)
        .await;
    let c = ctx_with(HttpPolicy { max_concurrent: 1, ..HttpPolicy::default() });
    let url = format!("{}/x", server.uri());
    let t0 = std::time::Instant::now();
    let (a, b) = tokio::join!(c.send_text(HttpRequest::get(&url)), c.send_text(HttpRequest::get(&url)));
    a.unwrap();
    b.unwrap();
    assert!(t0.elapsed() >= Duration::from_millis(300), "{:?}", t0.elapsed());
    // With 2 permits the same pair overlaps.
    let c2 = ctx_with(HttpPolicy { max_concurrent: 2, ..HttpPolicy::default() });
    let t1 = std::time::Instant::now();
    let (a, b) = tokio::join!(c2.send_text(HttpRequest::get(&url)), c2.send_text(HttpRequest::get(&url)));
    a.unwrap();
    b.unwrap();
    assert!(t1.elapsed() < Duration::from_millis(290), "{:?}", t1.elapsed());
}

#[tokio::test]
async fn error_bodies_travel_in_detail() {
    let server = MockServer::start().await;
    Mock::given(path("/x"))
        .respond_with(
            ResponseTemplate::new(400)
                .set_body_json(serde_json::json!({"errors": {"resolution": "required"}})),
        )
        .mount(&server)
        .await;
    let e = ctx().send_text(HttpRequest::post(format!("{}/x", server.uri()))).await.unwrap_err();
    assert_eq!(e.code, ErrorCode::InvalidArgument);
    let body = e.detail.unwrap()["body"].as_str().unwrap().to_owned();
    assert!(body.contains("resolution"));
}

#[tokio::test]
async fn a_304_for_a_caller_supplied_validator_is_reported_as_unchanged() {
    let server = MockServer::start().await;
    Mock::given(path("/x")).respond_with(ResponseTemplate::new(304)).mount(&server).await;
    let r = ctx()
        .send_text(HttpRequest::get(format!("{}/x", server.uri())).header("If-None-Match", "\"mine\""))
        .await
        .unwrap();
    assert!(r.not_modified);
    assert_eq!(r.status, 304);
    assert!(r.body.is_empty());
}

#[tokio::test]
async fn cross_host_redirect_is_not_followed() {
    let (a, b) = (MockServer::start().await, MockServer::start().await);
    Mock::given(path("/x"))
        .respond_with(ResponseTemplate::new(302).insert_header("location", format!("{}/y", b.uri())))
        .mount(&a)
        .await;
    // 127.0.0.1 vs localhost: same IP, different host string
    let from = a.uri().replace("127.0.0.1", "localhost");
    let r = ctx().send_text(HttpRequest::get(format!("{from}/x")).header("PRIVATE-TOKEN", "t")).await;
    assert!(r.is_err());
    assert!(b.received_requests().await.unwrap().is_empty());
}

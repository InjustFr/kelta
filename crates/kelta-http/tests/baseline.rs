//! Baseline behaviour of `HttpCtx` against a local wiremock server (no real network).

use kelta_http::{HttpClient, HttpCtx, HttpPolicy, HttpRequest};
use kelta_proto::error::ErrorCode;
use kelta_proto::ids::AccountId;
use wiremock::matchers::{header, method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn ctx() -> HttpCtx {
    HttpCtx::new(HttpClient::new("kelta-test"), AccountId::new("acc"), HttpPolicy::default())
}

#[tokio::test]
async fn send_json_ok_with_query_and_auth() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/issues"))
        .and(query_param("state", "open"))
        .and(header("authorization", "Bearer t0k"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("etag", "\"abc\"")
                .set_body_json(serde_json::json!([{"id": 1}])),
        )
        .mount(&server)
        .await;
    let r = ctx()
        .send_json::<serde_json::Value>(
            HttpRequest::get(format!("{}/issues", server.uri())).query("state", "open").bearer("t0k"),
        )
        .await
        .unwrap();
    assert_eq!(r.status, 200);
    assert_eq!(r.body[0]["id"], 1);
    assert_eq!(r.etag.as_deref(), Some("\"abc\""));
}

#[tokio::test]
async fn status_mapping() {
    let server = MockServer::start().await;
    for (p, status) in [("/a", 401u16), ("/b", 404), ("/c", 409), ("/d", 500)] {
        Mock::given(path(p)).respond_with(ResponseTemplate::new(status)).mount(&server).await;
    }
    Mock::given(path("/e"))
        .respond_with(ResponseTemplate::new(429).insert_header("retry-after", "7"))
        .mount(&server)
        .await;
    let c = ctx();
    let code = |p: &'static str| {
        let c = c.clone();
        let url = format!("{}{p}", server.uri());
        async move { c.send_text(HttpRequest::get(url)).await.unwrap_err() }
    };
    assert_eq!(code("/a").await.code, ErrorCode::NeedsAuth);
    assert_eq!(code("/b").await.code, ErrorCode::NotFound);
    assert_eq!(code("/c").await.code, ErrorCode::Conflict);
    assert_eq!(code("/d").await.code, ErrorCode::Upstream);
    let e = code("/e").await;
    assert_eq!(e.code, ErrorCode::RateLimited);
    assert_eq!(e.retry_after_ms, Some(7000));
}

#[tokio::test]
async fn connection_refused_is_network() {
    let e = ctx().send_text(HttpRequest::get("http://127.0.0.1:1/")).await.unwrap_err();
    assert!(matches!(e.code, ErrorCode::Network | ErrorCode::Timeout), "{e:?}");
}

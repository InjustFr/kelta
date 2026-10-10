//! Device flow (RFC 8628) and token refresh against wiremock, under paused time. Captured logs
//! must never contain a token, refresh token or device code.
#![allow(clippy::unwrap_used)]

use kelta_http::oauth::{device_finish, device_start, grant_ref};
use kelta_http::{AuthScheme, Authed, HttpClient, HttpCtx, HttpPolicy, HttpRequest};
use kelta_proto::api::SecretResolver;
use kelta_proto::error::ErrorCode;
use kelta_proto::ids::AccountId;
use kelta_proto::secret::{SecretCtx, SecretRef};
use kelta_proto::settings::AccountKind;
use kelta_proto::testing::FakeSecrets;
use serde_json::json;
use tokio::time::Instant;
use wiremock::matchers::{body_string_contains, header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

#[path = "../../kelta-secrets/tests/support/capture.rs"]
mod capture;

const SECRETS: [&str; 5] = ["gho_ACCESS", "glpat_NEW", "REFRESH_OLD", "REFRESH_NEW", "DEVICE_CODE_XYZ"];

fn client() -> HttpClient {
    HttpClient::with_timeout("kelta-test", None)
}

fn assert_no_secret(log: &str) {
    assert!(log.contains("oauth"), "the capture saw nothing:\n{log}");
    for s in SECRETS {
        assert!(!log.contains(s), "`{s}` leaked into the logs:\n{log}");
    }
}

async fn stored(s: &FakeSecrets, r: &str) -> String {
    s.resolve(&SecretRef::new(r), &SecretCtx::default()).await.unwrap().expose().to_owned()
}

async fn mount_device(server: &MockServer, url: &str) {
    Mock::given(method("POST"))
        .and(path(url))
        .and(body_string_contains("client_id=cid"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "device_code": "DEVICE_CODE_XYZ", "user_code": "ABCD-1234",
            "verification_uri": "https://example.test/device", "expires_in": 900, "interval": 5
        })))
        .expect(1)
        .mount(server)
        .await;
}

/// One token-endpoint answer, used once, in mount order.
async fn answer(server: &MockServer, url: &str, status: u16, body: serde_json::Value) {
    Mock::given(method("POST"))
        .and(path(url))
        .respond_with(ResponseTemplate::new(status).set_body_json(body))
        .up_to_n_times(1)
        .expect(1)
        .mount(server)
        .await;
}

#[tokio::test(start_paused = true)]
async fn github_device_flow_pending_slow_down_success() {
    let log = capture::install();
    let server = MockServer::start().await;
    mount_device(&server, "/login/device/code").await;
    Mock::given(path("/login/oauth/access_token"))
        .and(header("accept", "application/json"))
        .and(body_string_contains("grant_type=urn%3Aietf%3Aparams%3Aoauth%3Agrant-type%3Adevice_code"))
        .and(body_string_contains("device_code=DEVICE_CODE_XYZ"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"error": "authorization_pending"})))
        .up_to_n_times(1)
        .expect(1)
        .mount(&server)
        .await;
    // GitHub answers slow_down with the new interval
    answer(&server, "/login/oauth/access_token", 200, json!({"error": "slow_down", "interval": 10})).await;
    answer(
        &server,
        "/login/oauth/access_token",
        200,
        json!({"access_token": "gho_ACCESS", "token_type": "bearer", "scope": "repo"}),
    )
    .await;

    let http = client();
    let auth =
        device_start(&http, AccountKind::Github, &format!("{}/api/v3", server.uri()), "cid").await.unwrap();
    assert_eq!(auth.user_code, "ABCD-1234");
    assert!(!format!("{auth:?}").contains("DEVICE_CODE_XYZ"));
    let secrets = FakeSecrets::new();
    let t0 = Instant::now();
    device_finish(&http, &*secrets, &SecretRef::new("keyring:gh"), auth).await.unwrap();
    // 5 s, 5 s, then 10 s after slow_down
    assert_eq!(t0.elapsed().as_secs(), 20);
    assert_eq!(stored(&secrets, "keyring:gh").await, "gho_ACCESS");
    let grant: serde_json::Value = serde_json::from_str(&stored(&secrets, "keyring:gh.oauth").await).unwrap();
    assert_eq!(grant["expires_at"], json!(null), "GitHub OAuth App tokens do not expire");
    assert_no_secret(&log.contents());
}

#[tokio::test(start_paused = true)]
async fn gitlab_device_flow_stores_refresh_token_and_expiry() {
    let log = capture::install();
    let server = MockServer::start().await;
    mount_device(&server, "/oauth/authorize_device").await;
    answer(&server, "/oauth/token", 400, json!({"error": "authorization_pending"})).await;
    // a deploy's 502 page mid-poll does not end the sign-in
    Mock::given(path("/oauth/token"))
        .respond_with(ResponseTemplate::new(502).set_body_string("<html>502 Bad Gateway</html>"))
        .up_to_n_times(1)
        .expect(1)
        .mount(&server)
        .await;
    answer(
        &server,
        "/oauth/token",
        200,
        json!({"access_token": "glpat_NEW", "refresh_token": "REFRESH_NEW", "expires_in": 7200, "token_type": "Bearer"}),
    )
    .await;
    let http = client();
    let auth = device_start(&http, AccountKind::Gitlab, &server.uri(), "cid").await.unwrap();
    let secrets = FakeSecrets::new();
    device_finish(&http, &*secrets, &SecretRef::new("keyring:gl"), auth).await.unwrap();
    assert_eq!(stored(&secrets, "keyring:gl").await, "glpat_NEW");
    let grant: serde_json::Value = serde_json::from_str(&stored(&secrets, "keyring:gl.oauth").await).unwrap();
    assert_eq!(grant["refresh_token"], "REFRESH_NEW");
    assert_eq!(grant["token_url"], format!("{}/oauth/token", server.uri()));
    assert!(grant["expires_at"].as_i64().unwrap() > time::OffsetDateTime::now_utc().unix_timestamp() + 7000);
    assert_no_secret(&log.contents());
}

#[tokio::test(start_paused = true)]
async fn expired_and_denied_end_the_flow() {
    for (error, code) in [("expired_token", ErrorCode::Timeout), ("access_denied", ErrorCode::Cancelled)] {
        let server = MockServer::start().await;
        mount_device(&server, "/oauth/authorize_device").await;
        answer(&server, "/oauth/token", 400, json!({"error": error})).await;
        let http = client();
        let auth = device_start(&http, AccountKind::Gitlab, &server.uri(), "cid").await.unwrap();
        let secrets = FakeSecrets::new();
        let e = device_finish(&http, &*secrets, &SecretRef::new("keyring:gl"), auth).await.unwrap_err();
        assert_eq!(e.code, code, "{error}");
        assert!(secrets.resolve(&SecretRef::new("keyring:gl"), &SecretCtx::default()).await.is_err());
    }
}

#[tokio::test(start_paused = true)]
async fn a_non_json_4xx_from_the_token_url_ends_the_flow_at_once() {
    let server = MockServer::start().await;
    mount_device(&server, "/oauth/authorize_device").await;
    Mock::given(path("/oauth/token"))
        .respond_with(ResponseTemplate::new(404).set_body_string("<html>Not Found</html>"))
        .expect(1)
        .mount(&server)
        .await;
    let http = client();
    let auth = device_start(&http, AccountKind::Gitlab, &server.uri(), "cid").await.unwrap();
    let e =
        device_finish(&http, &*FakeSecrets::new(), &SecretRef::new("keyring:gl"), auth).await.unwrap_err();
    assert_eq!(e.code, ErrorCode::InvalidArgument, "{}", e.message);
}

#[tokio::test(start_paused = true)]
async fn code_expiry_is_enforced_locally() {
    let server = MockServer::start().await;
    Mock::given(path("/oauth/authorize_device"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "device_code": "D", "user_code": "U", "verification_uri": "https://x", "expires_in": 12, "interval": 5
        })))
        .mount(&server)
        .await;
    Mock::given(path("/oauth/token"))
        .respond_with(ResponseTemplate::new(400).set_body_json(json!({"error": "authorization_pending"})))
        .expect(2)
        .mount(&server)
        .await;
    let http = client();
    let auth = device_start(&http, AccountKind::Gitlab, &server.uri(), "cid").await.unwrap();
    let e =
        device_finish(&http, &*FakeSecrets::new(), &SecretRef::new("keyring:gl"), auth).await.unwrap_err();
    assert_eq!(e.code, ErrorCode::Timeout);
}

fn oauth_authed(server: &MockServer, secrets: std::sync::Arc<FakeSecrets>) -> Authed {
    let ctx = HttpCtx::new(client(), AccountId::new("gl"), HttpPolicy::default());
    Authed::new(ctx, secrets, Some(SecretRef::new("keyring:gl")), Some(&server.uri()), AuthScheme::OAuth)
}

fn grant(server: &MockServer, expires_in: i64) -> String {
    json!({
        "client_id": "cid", "token_url": format!("{}/oauth/token", server.uri()),
        "refresh_token": "REFRESH_OLD",
        "expires_at": time::OffsetDateTime::now_utc().unix_timestamp() + expires_in,
    })
    .to_string()
}

#[tokio::test]
async fn expiring_token_is_refreshed_before_use() {
    let log = capture::install();
    let server = MockServer::start().await;
    Mock::given(path("/oauth/token"))
        .and(body_string_contains("grant_type=refresh_token"))
        .and(body_string_contains("refresh_token=REFRESH_OLD"))
        .and(body_string_contains("client_id=cid"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "access_token": "glpat_NEW", "refresh_token": "REFRESH_NEW", "expires_in": 7200
        })))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(path("/api/v4/user"))
        .and(header("authorization", "Bearer glpat_NEW"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"id": 1})))
        .expect(2)
        .mount(&server)
        .await;
    let secrets =
        FakeSecrets::with(&[("keyring:gl", "glpat_OLD"), ("keyring:gl.oauth", &grant(&server, 30))]);
    let a = oauth_authed(&server, secrets.clone());
    let url = format!("{}/api/v4/user", server.uri());
    a.send_json::<serde_json::Value>(HttpRequest::get(&url)).await.unwrap();
    // the second request uses the stored token: no second refresh (expect(1) above)
    a.send_json::<serde_json::Value>(HttpRequest::get(&url)).await.unwrap();
    assert_eq!(stored(&secrets, "keyring:gl").await, "glpat_NEW");
    let g: serde_json::Value =
        serde_json::from_str(&stored(&secrets, grant_ref(&SecretRef::new("keyring:gl")).as_str()).await)
            .unwrap();
    assert_eq!(g["refresh_token"], "REFRESH_NEW");
    assert_no_secret(&log.contents());
}

#[tokio::test]
async fn refresh_keeps_the_rotated_refresh_token_when_the_token_copy_cannot_be_written() {
    let server = MockServer::start().await;
    Mock::given(path("/oauth/token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "access_token": "glpat_NEW", "refresh_token": "REFRESH_NEW", "expires_in": 7200
        })))
        .expect(1)
        .mount(&server)
        .await;
    let secrets =
        FakeSecrets::with(&[("keyring:gl", "glpat_OLD"), ("keyring:gl.oauth", &grant(&server, -10))]);
    secrets.fail_set("keyring:gl");
    let a = oauth_authed(&server, secrets.clone());
    assert!(a.prepare(HttpRequest::get("http://h/")).await.is_err());
    assert!(stored(&secrets, "keyring:gl.oauth").await.contains("REFRESH_NEW"));
    // the grant alone carries the new token: no second refresh (expect(1) above)
    let req = a.prepare(HttpRequest::get("http://h/")).await.unwrap();
    assert!(req.headers.contains(&("Authorization".to_owned(), "Bearer glpat_NEW".to_owned())));
}

#[tokio::test]
async fn fresh_token_is_used_without_refresh() {
    let server = MockServer::start().await;
    Mock::given(path("/oauth/token")).respond_with(ResponseTemplate::new(500)).expect(0).mount(&server).await;
    let secrets =
        FakeSecrets::with(&[("keyring:gl", "glpat_OLD"), ("keyring:gl.oauth", &grant(&server, 3600))]);
    let req = oauth_authed(&server, secrets).prepare(HttpRequest::get("http://h/")).await.unwrap();
    assert!(req.headers.contains(&("Authorization".to_owned(), "Bearer glpat_OLD".to_owned())));
}

#[tokio::test]
async fn failed_refresh_is_needs_auth() {
    let log = capture::install();
    let server = MockServer::start().await;
    Mock::given(path("/oauth/token"))
        .respond_with(ResponseTemplate::new(400).set_body_json(json!({"error": "invalid_grant"})))
        .expect(1)
        .mount(&server)
        .await;
    let secrets =
        FakeSecrets::with(&[("keyring:gl", "glpat_OLD"), ("keyring:gl.oauth", &grant(&server, -10))]);
    let e = oauth_authed(&server, secrets).prepare(HttpRequest::get("http://h/")).await.unwrap_err();
    assert_eq!(e.code, ErrorCode::NeedsAuth);
    assert!(e.message.contains("sign in again"), "{}", e.message);
    assert_no_secret(&log.contents());
}

#[tokio::test]
async fn refresh_5xx_keeps_the_account_signed_in() {
    let server = MockServer::start().await;
    Mock::given(path("/oauth/token"))
        .respond_with(ResponseTemplate::new(502).set_body_string("<html>502 Bad Gateway</html>"))
        .expect(1)
        .mount(&server)
        .await;
    let secrets =
        FakeSecrets::with(&[("keyring:gl", "glpat_OLD"), ("keyring:gl.oauth", &grant(&server, -10))]);
    let e = oauth_authed(&server, secrets.clone()).prepare(HttpRequest::get("http://h/")).await.unwrap_err();
    assert_eq!(e.code, ErrorCode::Upstream);
    assert!(stored(&secrets, "keyring:gl.oauth").await.contains("REFRESH_OLD"));
}

#[tokio::test]
async fn start_surfaces_oauth_errors() {
    let server = MockServer::start().await;
    Mock::given(path("/login/device/code"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"error": "device_flow_disabled"})))
        .mount(&server)
        .await;
    let e = device_start(&client(), AccountKind::Github, &server.uri(), "cid").await.unwrap_err();
    assert!(e.message.contains("enable Device Flow"), "{}", e.message);
    let e = device_start(&client(), AccountKind::Jira, &server.uri(), "cid").await.unwrap_err();
    assert_eq!(e.code, ErrorCode::InvalidArgument);
}

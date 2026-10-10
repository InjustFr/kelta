//! Browser sign-in: closing the wizard (`oauth_device_cancel`) stops the device-flow polling.

use std::time::Duration;

use crate::common::*;
use kelta_proto::ErrorCode;
use kelta_proto::secret::SecretRef;
use kelta_proto::settings::{AccountKind, Settings};
use serde_json::json;
use wiremock::matchers::path;
use wiremock::{Mock, MockServer, ResponseTemplate};

#[tokio::test]
async fn cancel_ends_a_pending_sign_in() {
    let server = MockServer::start().await;
    Mock::given(path("/oauth/authorize_device"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "device_code": "D", "user_code": "WDJB-MJHT", "verification_uri": "https://x", "interval": 1
        })))
        .mount(&server)
        .await;
    Mock::given(path("/oauth/token"))
        .respond_with(ResponseTemplate::new(400).set_body_json(json!({"error": "authorization_pending"})))
        .mount(&server)
        .await;
    let tmp = tempfile::tempdir().unwrap();
    let mut s = Settings::defaults();
    s.oauth.client_ids.insert("127.0.0.1".into(), "cid".into());
    let h = start(tmp.path(), s, Vec::new());
    let p = h
        .core
        .oauth_device_start(AccountKind::Gitlab, &server.uri(), SecretRef::new("keyring:gl"))
        .await
        .unwrap();

    let core = h.core.clone();
    let code = p.user_code.clone();
    let finish = tokio::spawn(async move { core.oauth_device_finish(&code).await });
    // let it reach the polling loop (first poll after the 1 s interval), then close the wizard
    let polled = async {
        while !server.received_requests().await.unwrap().iter().any(|r| r.url.path() == "/oauth/token") {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    };
    tokio::time::timeout(Duration::from_secs(5), polled).await.expect("device flow never polled");
    h.core.oauth_device_cancel(&p.user_code);
    let e = tokio::time::timeout(Duration::from_secs(2), finish).await.unwrap().unwrap().unwrap_err();
    assert_eq!(e.code, ErrorCode::Cancelled);
}

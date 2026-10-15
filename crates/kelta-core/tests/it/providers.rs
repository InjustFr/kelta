//! ProviderRegistry: the tracker and code host of one account share one `HttpCtx`.

use std::collections::BTreeMap;
use std::sync::Arc;

use kelta_core::providers::ProviderRegistry;
use kelta_http::{HttpClient, HttpCtx, HttpRequest, ProviderFactory};
use kelta_proto::api::{CodeHost, SecretResolver, Tracker};
use kelta_proto::error::{ErrorCode, KeltaError};
use kelta_proto::ids::AccountId;
use kelta_proto::settings::{AccountConfig, AccountKind};
use kelta_proto_testing::FakeSecrets;
use parking_lot::Mutex;
use wiremock::{Mock, MockServer, ResponseTemplate};

use crate::common::account;

/// Keeps the ctx each provider was built with (and builds nothing).
#[derive(Default)]
struct Recorder(Mutex<Vec<HttpCtx>>);

impl ProviderFactory for Recorder {
    fn tracker(
        &self,
        _: &AccountConfig,
        http: HttpCtx,
        _: Arc<dyn SecretResolver>,
    ) -> Result<Arc<dyn Tracker>, KeltaError> {
        self.0.lock().push(http);
        Err(KeltaError::unsupported("recorder"))
    }

    fn code_host(
        &self,
        _: &AccountConfig,
        http: HttpCtx,
        _: Arc<dyn SecretResolver>,
    ) -> Result<Arc<dyn CodeHost>, KeltaError> {
        self.0.lock().push(http);
        Err(KeltaError::unsupported("recorder"))
    }
}

#[tokio::test]
async fn a_secondary_limit_on_one_side_blocks_the_other() {
    let server = MockServer::start().await;
    Mock::given(wiremock::matchers::any())
        .respond_with(ResponseTemplate::new(403).set_body_string("You have exceeded a secondary rate limit"))
        .mount(&server)
        .await;
    let rec = Arc::new(Recorder::default());
    let reg = ProviderRegistry::new(
        HttpClient::with_timeout("kelta-test", None),
        rec.clone(),
        rec.clone(),
        FakeSecrets::new(),
    );
    let id = AccountId::new("github-work");
    let accounts = BTreeMap::from([(id.clone(), account(AccountKind::Github))]);
    assert!(reg.tracker(&id, &accounts).is_err());
    assert!(reg.code_host(&id, &accounts).is_err());
    let [tracker, host] = &rec.0.lock().clone()[..] else { panic!("two providers built") };
    let e = tracker.send_text(HttpRequest::get(format!("{}/issues", server.uri()))).await.unwrap_err();
    assert_eq!(e.code, ErrorCode::RateLimited);
    assert!(host.blocked_for().is_some(), "the code host waits out the tracker's 403 too");
}

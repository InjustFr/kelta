//! Shared test helpers: fixtures, accounts, wiremock mounting. No real network.
#![allow(dead_code, clippy::expect_used, clippy::unwrap_used)]

use std::path::PathBuf;
use std::sync::Arc;

use kelta_codehosts::CodeHostFactory;
use kelta_http::{HttpClient, HttpCtx, HttpPolicy, ProviderFactory};
use kelta_proto::api::CodeHost;
use kelta_proto::codehost::{ReviewKind, ReviewQuery, ReviewRef};
use kelta_proto::ids::AccountId;
use kelta_proto::settings::AccountConfig;
use kelta_proto::testing::FakeSecrets;
use serde_json::{Value, json};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

pub const TOKEN: &str = "tok-123";

pub fn fixture_text(name: &str) -> String {
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("fixtures").join(name);
    std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("fixture {name}: {e}"))
}

pub fn fixture(name: &str) -> Value {
    serde_json::from_str(&fixture_text(name)).unwrap_or_else(|e| panic!("fixture {name}: {e}"))
}

pub fn secrets_with(token: &str) -> Arc<FakeSecrets> {
    FakeSecrets::with(&[("env:TOK", token)])
}

pub fn http(id: &str) -> HttpCtx {
    http_with(id, HttpPolicy::default())
}

pub fn http_with(id: &str, policy: HttpPolicy) -> HttpCtx {
    HttpCtx::new(HttpClient::with_timeout("kelta-test", None), AccountId::new(id), policy)
}

pub fn account(kind: &str, base_url: &str, extra: Value) -> AccountConfig {
    let mut v = json!({"kind": kind, "base_url": base_url, "secret": "env:TOK"});
    if let (Some(o), Some(e)) = (v.as_object_mut(), extra.as_object()) {
        for (k, val) in e {
            o.insert(k.clone(), val.clone());
        }
    }
    serde_json::from_value(v).expect("account config")
}

pub fn host_with(id: &str, kind: &str, base_url: &str, token: &str) -> (Arc<dyn CodeHost>, Arc<FakeSecrets>) {
    let secrets = secrets_with(token);
    (host_over(http(id), kind, base_url, secrets.clone()), secrets)
}

pub fn host_over(http: HttpCtx, kind: &str, base_url: &str, secrets: Arc<FakeSecrets>) -> Arc<dyn CodeHost> {
    CodeHostFactory
        .code_host(&account(kind, base_url, json!({"email": "louis@acme.test"})), http, secrets)
        .expect("code host")
}

pub fn host(id: &str, kind: &str, base_url: &str) -> Arc<dyn CodeHost> {
    host_with(id, kind, base_url, TOKEN).0
}

pub fn rref(account: &str, repo: &str, number: u64) -> ReviewRef {
    ReviewRef { account: AccountId::new(account), repo: repo.into(), number }
}

pub fn query(kind: ReviewKind, include_team: bool, include_drafts: bool) -> ReviewQuery {
    ReviewQuery { kind, include_team, include_drafts }
}

pub async fn mount(server: &MockServer, m: &str, p: &str, status: u16, fixture_name: &str) {
    Mock::given(method(m))
        .and(path(p))
        .respond_with(
            ResponseTemplate::new(status)
                .set_body_string(fixture_text(fixture_name))
                .insert_header("content-type", "application/json"),
        )
        .mount(server)
        .await;
}

pub async fn bodies(server: &MockServer, m: &str, p: &str) -> Vec<Value> {
    server
        .received_requests()
        .await
        .unwrap_or_default()
        .into_iter()
        .filter(|r| r.method.as_str() == m && r.url.path() == p)
        .map(|r| serde_json::from_slice(&r.body).unwrap_or(Value::Null))
        .collect()
}

pub async fn count(server: &MockServer, m: &str, p: &str) -> usize {
    bodies(server, m, p).await.len()
}

/// Query strings of the requests for `method path`.
pub async fn queries(server: &MockServer, m: &str, p: &str) -> Vec<String> {
    server
        .received_requests()
        .await
        .unwrap_or_default()
        .into_iter()
        .filter(|r| r.method.as_str() == m && r.url.path() == p)
        .map(|r| r.url.query().unwrap_or("").to_owned())
        .collect()
}

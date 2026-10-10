//! Shared test helpers: fixtures, accounts, wiremock mounting. No real network.
#![allow(dead_code, clippy::expect_used, clippy::unwrap_used)]

use std::path::PathBuf;
use std::sync::Arc;

use kelta_http::{HttpClient, HttpCtx, HttpPolicy, ProviderFactory};
use kelta_proto::api::Tracker;
use kelta_proto::ids::AccountId;
use kelta_proto::settings::{AccountConfig, TrackerView};
use kelta_proto::testing::FakeSecrets;
use kelta_proto::tracker::TicketRef;
use kelta_trackers::TrackerFactory;
use serde_json::{Value, json};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

pub const TOKEN: &str = "tok-123";

pub fn fixture_path(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("fixtures").join(name)
}

pub fn fixture_text(name: &str) -> String {
    std::fs::read_to_string(fixture_path(name)).unwrap_or_else(|e| panic!("fixture {name}: {e}"))
}

pub fn fixture(name: &str) -> Value {
    serde_json::from_str(&fixture_text(name)).unwrap_or_else(|e| panic!("fixture {name}: {e}"))
}

pub fn secrets() -> Arc<FakeSecrets> {
    FakeSecrets::with(&[("env:TOK", TOKEN)])
}

/// A context without client-side timers (safe under paused time) for account `id`.
pub fn http(id: &str) -> HttpCtx {
    HttpCtx::new(HttpClient::with_timeout("kelta-test", None), AccountId::new(id), HttpPolicy::default())
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

pub fn tracker_with(id: &str, acc: &AccountConfig, secrets: Arc<FakeSecrets>) -> Arc<dyn Tracker> {
    TrackerFactory.tracker(acc, http(id), secrets).expect("tracker")
}

pub fn tracker(id: &str, kind: &str, base_url: &str, extra: Value) -> Arc<dyn Tracker> {
    tracker_with(id, &account(kind, base_url, extra), secrets())
}

pub fn tref(account: &str, key: &str, id: &str) -> TicketRef {
    TicketRef { account: AccountId::new(account), key: key.into(), id: id.into() }
}

pub fn view(id: &str) -> TrackerView {
    TrackerView { id: id.into(), label: id.into(), ..TrackerView::default() }
}

/// Mount `fixture` as the JSON answer of `method path` (any query).
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

/// Bodies (parsed) of the requests the server received for `method path`.
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

/// Mount Linear's single `/graphql` endpoint: each operation is told apart by its query text.
pub async fn linear_mocks(server: &MockServer) {
    use wiremock::matchers::body_string_contains;
    for (needle, fixture_name) in [
        ("viewer", "linear/viewer.json"),
        ("issues(filter", "linear/issues_p1.json"),
        ("comments(last", "linear/issue.json"),
        ("states(first", "linear/transitions.json"),
        ("workflowStates", "linear/workflow_states.json"),
        ("issueUpdate", "linear/issue_updated.json"),
        ("commentCreate", "linear/comment_created.json"),
    ] {
        Mock::given(method("POST"))
            .and(path("/graphql"))
            .and(body_string_contains(needle))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_string(fixture_text(fixture_name))
                    .insert_header("content-type", "application/json"),
            )
            .mount(server)
            .await;
    }
}

//! Process (KPP) provider runtime: crash/restart with backoff, per-call timeout, malformed output,
//! error mapping, per-call secret, account ownership, grants and uninstall.
#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use std::path::{Path, PathBuf};
use std::sync::Arc;

use kelta_http::{HttpClient, HttpCtx, HttpPolicy, ProviderFactory};
use kelta_plugins::kpp::{KppCodeHost, KppFactory, KppProcess, KppTracker, Source};
use kelta_proto::api::{CodeHost, SecretResolver, Tracker};
use kelta_proto::codehost::CodeHostKind;
use kelta_proto::error::{ErrorCode, KeltaError};
use kelta_proto::ext::ProviderDef;
use kelta_proto::ids::{AccountId, PluginId};
use kelta_proto::settings::{AccountConfig, TrackerView};
use kelta_proto::testing::FakeSecrets;
use kelta_proto::testing::conformance::{CodeHostCase, code_host_contract};

fn node() -> bool {
    let ok = std::process::Command::new("node").arg("--version").output().is_ok_and(|o| o.status.success());
    if !ok {
        eprintln!("skipped: node not found");
    }
    ok
}

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/kpp")
}

fn def(timeout_ms: u64) -> ProviderDef {
    toml::from_str(&format!(
        "kind = \"tracker\"\ncommand = \"node\"\nargs = [\"fake.cjs\"]\ntimeout_ms = {timeout_ms}"
    ))
    .unwrap()
}

fn account(user: &str) -> AccountConfig {
    serde_json::from_value(serde_json::json!({
        "kind": "plugin_tracker", "plugin": "fake", "user": user, "secret": "env:TOK",
    }))
    .unwrap()
}

fn tracker(p: &Arc<KppProcess>, user: &str, secrets: &Arc<FakeSecrets>) -> KppTracker {
    let secrets: Arc<dyn SecretResolver> = secrets.clone();
    KppTracker::new(Source::Direct(p.clone()), def(0), AccountId::new("acc"), account(user), secrets)
}

fn process(timeout_ms: u64) -> Arc<KppProcess> {
    Arc::new(KppProcess::new(&def(timeout_ms), &fixtures(), "sha", None))
}

fn alive(pid: u32) -> bool {
    rustix::process::test_kill_process(rustix::process::Pid::from_raw(pid as i32).unwrap()).is_ok()
}

#[tokio::test]
async fn crash_restarts_with_backoff_and_keeps_stderr() {
    if !node() {
        return;
    }
    let s = FakeSecrets::with(&[("env:TOK", "tok-1")]);
    let p = process(5000);
    let ok = tracker(&p, "ok", &s);
    let crash = tracker(&p, "crash", &s);

    let me = ok.me().await.unwrap();
    assert_eq!(me.name, "tok-1", "the secret is passed per call");
    let first = me.id.clone();
    assert_eq!(p.spawns(), 1, "spawned lazily on first use");

    let e = crash.me().await.unwrap_err();
    assert_eq!(e.code, ErrorCode::Upstream, "{e:?}");
    // the first crash restarts at once
    let second = ok.me().await.unwrap().id;
    assert_ne!(first, second);
    assert_eq!(p.spawns(), 2);
    assert!(p.log_tail(10).contains("fake provider started"), "stderr captured");

    // two crashes in a row: the next restart waits
    crash.me().await.unwrap_err();
    ok.me().await.unwrap();
    crash.me().await.unwrap_err();
    // crash count was reset by the success above: restart at once, then crash again
    crash.me().await.unwrap_err();
    let e = ok.me().await.unwrap_err();
    assert_eq!(e.code, ErrorCode::Upstream);
    assert!(e.retry_after_ms.is_some_and(|ms| ms <= 500), "{e:?}");
    tokio::time::sleep(std::time::Duration::from_millis(600)).await;
    ok.me().await.unwrap();
}

#[tokio::test]
async fn timeout_fails_the_call_but_keeps_a_process_that_answers_others() {
    if !node() {
        return;
    }
    let s = FakeSecrets::with(&[("env:TOK", "t")]);
    let p = process(500);
    let pid = tracker(&p, "ok", &s).me().await.unwrap().id;
    let hang = tracker(&p, "hang", &s);
    let ok = tracker(&p, "ok", &s);
    let (e, other) = tokio::join!(hang.me(), async {
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        ok.me().await
    });
    assert_eq!(e.unwrap_err().code, ErrorCode::Timeout);
    assert_eq!(other.unwrap().id, pid, "answered during the slow call");
    assert_eq!(tracker(&p, "ok", &s).me().await.unwrap().id, pid, "slow, not hung: kept");
}

#[tokio::test]
async fn a_silent_timeout_kills_the_hung_process() {
    if !node() {
        return;
    }
    let s = FakeSecrets::with(&[("env:TOK", "t")]);
    let p = process(300);
    let pid = tracker(&p, "ok", &s).me().await.unwrap().id;
    let e = tracker(&p, "hang", &s).me().await.unwrap_err();
    assert_eq!(e.code, ErrorCode::Timeout);
    assert!(common::wait_for(|| !alive(pid.parse().unwrap())).await, "hung process killed");
    assert_ne!(tracker(&p, "ok", &s).me().await.unwrap().id, pid, "respawned");
}

#[tokio::test]
async fn the_code_host_adapter_passes_the_contract() {
    if !node() {
        return;
    }
    let def: ProviderDef =
        toml::from_str("kind = \"codehost\"\ncommand = \"node\"\nargs = [\"fake.cjs\"]").unwrap();
    let p = Arc::new(KppProcess::new(&def, &fixtures(), "sha", None));
    let secrets: Arc<dyn SecretResolver> = FakeSecrets::with(&[("env:TOK", "t")]);
    let h = KppCodeHost::new(Source::Direct(p), def, AccountId::new("acc"), account("ok"), secrets);
    // the fake claims another account and sends a <script>: the adapter owns refs and sanitizes
    let case = CodeHostCase {
        kind: CodeHostKind::Plugin,
        account_id: "acc".into(),
        repo: None,
        refspec: Some("pull/{n}/head".into()),
    };
    code_host_contract(&h, &case).await.unwrap();
    assert!(h.changed_since_last().await.unwrap(), "unsupported = assume a change");
}

#[tokio::test]
async fn malformed_output_kills_and_respawns() {
    if !node() {
        return;
    }
    let s = FakeSecrets::with(&[("env:TOK", "t")]);
    let p = process(5000);
    let pid = tracker(&p, "ok", &s).me().await.unwrap().id;
    let e = tracker(&p, "garbage", &s).me().await.unwrap_err();
    assert_eq!(e.code, ErrorCode::Upstream);
    assert!(e.message.contains("malformed"), "{e:?}");
    assert!(common::wait_for(|| !alive(pid.parse().unwrap())).await, "killed");
    assert_ne!(tracker(&p, "ok", &s).me().await.unwrap().id, pid);
}

#[tokio::test]
async fn errors_map_to_error_codes_and_accounts_are_owned() {
    if !node() {
        return;
    }
    let s = FakeSecrets::with(&[("env:TOK", "t")]);
    let p = process(5000);
    let e = tracker(&p, "denied", &s).me().await.unwrap_err();
    assert_eq!(e.code, ErrorCode::NeedsAuth);
    assert_eq!(s.invalidated(), vec!["env:TOK".to_owned()], "401 drops the cached secret");
    let ticket = serde_json::from_str(r#"{"account":"acc","key":"X-1","id":"1"}"#).unwrap();
    let e = tracker(&p, "ok", &s).transitions(&ticket).await.unwrap_err();
    assert_eq!(e.code, ErrorCode::Unsupported, "unknown method");
    let view = TrackerView { id: "v".into(), label: "v".into(), ..TrackerView::default() };
    let page = tracker(&p, "ok", &s).list(&view, None).await.unwrap();
    assert_eq!(page.items[0].r#ref.account.as_str(), "acc", "a plugin cannot speak for another account");
}

struct NoBuiltins;

impl ProviderFactory for NoBuiltins {
    fn tracker(
        &self,
        _: &AccountConfig,
        _: HttpCtx,
        _: Arc<dyn SecretResolver>,
    ) -> Result<Arc<dyn Tracker>, KeltaError> {
        Err(KeltaError::unsupported("builtin"))
    }
    fn code_host(
        &self,
        _: &AccountConfig,
        _: HttpCtx,
        _: Arc<dyn SecretResolver>,
    ) -> Result<Arc<dyn CodeHost>, KeltaError> {
        Err(KeltaError::unsupported("builtin"))
    }
}

#[tokio::test]
async fn the_factory_needs_the_grant_and_uninstall_kills_the_process() {
    if !node() {
        return;
    }
    let env = common::Env::new();
    let fake = std::fs::read_to_string(fixtures().join("fake.cjs")).unwrap();
    let manifest = common::manifest(
        "fake",
        &["provider"],
        "[provider]\nkind = \"tracker\"\ncommand = \"node\"\nargs = [\"fake.cjs\"]\n",
    );
    env.write_plugin("fake", &manifest, &[("fake.cjs", &fake)]);
    let factory = KppFactory::new(Arc::new(NoBuiltins), Arc::downgrade(&env.host));
    let http = HttpCtx::new(HttpClient::new("t"), AccountId::new("acc"), HttpPolicy::default());
    let secrets: Arc<dyn SecretResolver> = FakeSecrets::with(&[("env:TOK", "t")]);
    let t = factory.tracker(&account("ok"), http.clone(), secrets.clone()).unwrap();
    assert!(factory.code_host(&account("ok"), http, secrets).is_err(), "not a code host plugin");

    let e = t.me().await.unwrap_err();
    assert_eq!(e.code, ErrorCode::PermissionDenied, "no grant, no process: {e:?}");
    let id = PluginId::new("fake");
    env.host.grant(&id, vec!["provider".into()]).await.unwrap();
    let pid: u32 = t.me().await.unwrap().id.parse().unwrap();
    assert!(alive(pid));

    env.host.uninstall(&id).await.unwrap();
    assert!(common::wait_for(|| !alive(pid)).await, "uninstall kills the provider");
    assert_eq!(t.me().await.unwrap_err().code, ErrorCode::NotFound);
}

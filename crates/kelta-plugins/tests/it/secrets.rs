//! `secret_headers` (PLUGINS §5): a plugin names its own `x-kelta-secret` setting, the host resolves
//! it and injects the header; user config gives a SecretRef. FakeCore records the outgoing headers
//! at the `CoreApi::http_fetch` boundary (it has no network).

use crate::common;

use std::sync::Arc;

use kelta_plugins::Wiring;
use kelta_proto::ErrorCode;
use kelta_proto::api::{CoreApi, SecretResolver};
use kelta_proto::events::BusEvent;
use kelta_proto::ext::{ActionDef, CallOrigin, PluginMethod, SecretHeader, TriggerDef};
use kelta_proto::ids::{PluginId, ProjectId};
use kelta_proto::testing::FakeSecrets;
use serde_json::{Value, json};

const SCHEMA: &str = r#"{"type":"object","properties":{
    "qa_token":{"type":"string","title":"QA token","x-kelta-secret":true},
    "unset_token":{"type":"string","title":"Other token","x-kelta-secret":true,"default":"gh-cli"},
    "url":{"type":"string"}}}"#;

const PLUGIN: &str = r#"
[[contributes.screens]]
id = "main"
title = "Main"
entry = "index.html"

[[contributes.triggers]]
id = "deploy"
on = "custom.deploy"
do = [{ action = "http", url = "https://qa.example.com/run", secret_headers = { Authorization = { setting = "qa_token", format = "Bearer {secret}" }, "X-Token" = "qa_token" } }]

[contributes.settings]
schema = "s.json"
"#;

async fn setup() -> (common::Env, Arc<FakeSecrets>) {
    let env = common::Env::new().with_settings(|s| {
        s.plugins.settings.insert(
            "qa-hub".into(),
            json!({ "qa_token": "env:QA_TOKEN", "url": "keyring:not-a-secret-setting" }),
        );
    });
    let secrets =
        FakeSecrets::with(&[("env:QA_TOKEN", "t0k"), ("gh-cli", "gh-secret"), ("env:TOK", "cfg-tok")]);
    env.host.wire(Wiring { secrets: Some(secrets.clone() as Arc<dyn SecretResolver>), ..Default::default() });
    env.write_plugin(
        "qa-hub",
        &format!("{}{PLUGIN}", common::manifest("qa-hub", &["net:qa.example.com"], "")),
        &[("index.html", "x"), ("s.json", SCHEMA)],
    );
    env.host.grant(&PluginId::new("qa-hub"), vec!["net:qa.example.com".into()]).await.unwrap();
    env.core
        .respond("http_fetch", json!({ "status": 200, "headers": {}, "body": "ok", "body_base64": false }));
    (env, secrets)
}

fn sent_headers(env: &common::Env) -> Value {
    env.core.calls().into_iter().rev().find(|c| c.method == "http_fetch").unwrap().args["headers"].clone()
}

#[tokio::test]
async fn plugin_http_action_sends_the_resolved_secret() {
    let (env, _) = setup().await;
    let ev = BusEvent::new("custom.deploy", json!({})).with_project(ProjectId::new("shop"));
    env.host.on_event(&ev).await;
    env.host.settle().await;
    let run = env
        .host
        .trigger_log(10)
        .await
        .unwrap()
        .into_iter()
        .find(|r| r.trigger_id == "qa-hub/deploy")
        .unwrap();
    assert!(run.ok, "{run:?}");
    let h = sent_headers(&env);
    assert_eq!(h["Authorization"], "Bearer t0k");
    assert_eq!(h["X-Token"], "t0k");
    assert!(!format!("{run:?}").contains("t0k"), "secret in the trigger log");
}

#[tokio::test]
async fn net_fetch_injects_secrets_and_rejects_non_secret_settings_and_raw_refs() {
    let (env, secrets) = setup().await;
    let s = env
        .host
        .screen_open(&PluginId::new("qa-hub"), "main", Some(&ProjectId::new("shop")), Value::Null)
        .await
        .unwrap();
    let o = CallOrigin::Screen { instance_id: s.instance_id.clone() };
    let fetch = |sh: Value| {
        let (host, i, o) = (env.host.clone(), s.instance_id.clone(), o.clone());
        async move {
            host.call(
                &i,
                PluginMethod::NetFetch,
                json!({ "url": "https://qa.example.com/x", "secret_headers": sh }),
                o,
            )
            .await
        }
    };
    let resp = fetch(json!({ "Authorization": { "setting": "qa_token", "format": "Bearer {secret}" } }))
        .await
        .unwrap();
    assert_eq!(sent_headers(&env)["Authorization"], "Bearer t0k");
    assert!(!resp.to_string().contains("t0k"), "{resp}");

    for bad in [json!({ "A": "url" }), json!({ "A": "gh-cli" }), json!({ "A": { "setting": "nope" } })] {
        let e = fetch(bad.clone()).await.unwrap_err();
        assert_eq!(e.code, ErrorCode::InvalidArgument, "{bad}: {e:?}");
    }
    assert!(!secrets.resolved().contains(&"gh-cli".to_owned()));

    let e = fetch(json!({ "A": "unset_token" })).await.unwrap_err();
    assert_eq!(e.code, ErrorCode::NeedsAuth, "{e:?}");
    assert!(e.message.contains("set \"Other token\" in Settings"), "{e:?}");
    assert!(!secrets.resolved().contains(&"gh-cli".to_owned()), "a schema default chose its own ref");
    let got = env.host.call(&s.instance_id, PluginMethod::SettingsGet, json!({}), o.clone()).await.unwrap();
    assert!(got.get("unset_token").is_none(), "unset secret must not read as set: {got}");

    // A 401 drops the cached secret.
    env.core.respond("http_fetch", json!({ "status": 401, "headers": {}, "body": "", "body_base64": false }));
    fetch(json!({ "A": "qa_token" })).await.unwrap();
    assert_eq!(secrets.invalidated(), ["env:QA_TOKEN"]);
}

#[tokio::test]
async fn user_config_trigger_resolves_a_secret_ref() {
    let (env, _) = setup().await;
    let http = ActionDef::Http {
        url: "https://ci.example.com/hook".into(),
        method: None,
        headers: Default::default(),
        body: Some("{}".into()),
        secret_headers: [("Authorization".to_owned(), SecretHeader::Name("env:TOK".into()))].into(),
        timeout_ms: None,
    };
    env.core.set_settings({
        let mut s = (*env.core.settings(None)).clone();
        s.triggers = vec![TriggerDef {
            id: "hook".into(),
            on: "custom.x".into(),
            r#do: vec![http],
            ..Default::default()
        }];
        s
    });
    env.host.refresh();
    let run = env.host.trigger_test("hook", json!({})).await.unwrap();
    assert!(run.ok, "{run:?}");
    assert_eq!(sent_headers(&env)["Authorization"], "cfg-tok");
}

//! Permission matrix (PLUGINS §5/§7): every `PluginMethod` without its permission is
//! `PermissionDenied`; with the permission granted it is not.

mod common;

use kelta_proto::ErrorCode;
use kelta_proto::ext::{CallOrigin, MethodPermission, PluginMethod};
use kelta_proto::ids::{PluginId, ProjectId};
use serde_json::{Value, json};

const ALL_PERMS: &[&str] = &[
    "projects.read",
    "tickets.read",
    "tickets.write",
    "prs.read",
    "prs.write",
    "sessions.read",
    "sessions.spawn",
    "sessions.write",
    "terminal.write",
    "events:session.*",
    "settings.read",
    "ui.open",
    "notify",
    "clipboard.write",
    "exec:htop",
    "net:api.example.com",
];

const SCREEN: &str = "\n[[contributes.screens]]\nid = \"main\"\ntitle = \"Main\"\nentry = \"index.html\"\n";

fn params() -> Value {
    json!({
        "ticket": { "account": "jira-acme", "key": "SHOP-1", "id": "1" },
        "transition_id": "t1", "markdown": "hi", "assignee": { "kind": "me" },
        "review": { "account": "github-work", "repo": "acme/shop-api", "number": 1 },
        "head_sha": "abc", "body": "b",
        "command": "htop", "session_id": "s-1", "text": "ls",
        "tool_id": "nope", "names": ["session.*"], "url": "https://api.example.com/x",
        "screen_id": "main", "title": "t", "key": "k", "value": 1,
    })
}

async fn setup(granted: &[&str]) -> (common::Env, kelta_proto::ids::ScreenInstanceId) {
    let env = common::Env::new();
    env.write_plugin(
        "matrix",
        &format!("{}{SCREEN}", common::manifest("matrix", ALL_PERMS, "")),
        &[("index.html", "<p>hi</p>")],
    );
    let id = PluginId::new("matrix");
    let perms: Vec<String> = granted.iter().map(|s| s.to_string()).collect();
    env.host.grant(&id, perms).await.unwrap();
    let screen = env.host.screen_open(&id, "main", Some(&ProjectId::new("shop")), Value::Null).await.unwrap();
    (env, screen.instance_id)
}

fn origin(i: &kelta_proto::ids::ScreenInstanceId) -> CallOrigin {
    CallOrigin::Screen { instance_id: i.clone() }
}

#[tokio::test]
async fn every_method_without_its_permission_is_denied() {
    let (env, instance) = setup(&[]).await;
    for &method in PluginMethod::ALL {
        let res = env.host.call(&instance, method, params(), origin(&instance)).await;
        let needs =
            method.required_permission() != MethodPermission::None && method != PluginMethod::SettingsGet;
        match res {
            Err(e) if e.code == ErrorCode::PermissionDenied => {
                assert!(needs, "{method:?} needs no permission but was denied: {e:?}");
                assert!(
                    e.detail.as_ref().and_then(|d| d.get("permission")).is_some(),
                    "{method:?}: detail.permission"
                );
            }
            other => {
                assert!(!needs, "{method:?} must be PermissionDenied without its permission, got {other:?}")
            }
        }
    }
}

#[tokio::test]
async fn every_method_with_its_permission_passes_the_gate() {
    let (env, instance) = setup(ALL_PERMS).await;
    for &method in PluginMethod::ALL {
        let res = env.host.call(&instance, method, params(), origin(&instance)).await;
        if let Err(e) = &res {
            assert_ne!(e.code, ErrorCode::PermissionDenied, "{method:?} denied although granted: {e:?}");
        }
    }
}

#[tokio::test]
async fn dynamic_permissions_check_their_argument() {
    let (env, instance) = setup(ALL_PERMS).await;
    let call = |method, p: Value| {
        let host = env.host.clone();
        let instance = instance.clone();
        async move { host.call(&instance, method, p, origin(&instance)).await }
    };
    let e = call(PluginMethod::SessionsSpawn, json!({ "command": "/bin/rm" })).await.unwrap_err();
    assert_eq!(
        (e.code, e.detail.unwrap()["permission"].clone()),
        (ErrorCode::PermissionDenied, json!("exec:rm"))
    );
    let e = call(PluginMethod::NetFetch, json!({ "url": "https://evil.example.org/" })).await.unwrap_err();
    assert_eq!(e.code, ErrorCode::PermissionDenied);
    let e = call(PluginMethod::NetFetch, json!({ "url": "http://api.example.com/" })).await.unwrap_err();
    assert_eq!(e.code, ErrorCode::PermissionDenied, "http to a non-loopback host is refused");
    let e = call(PluginMethod::EventsSubscribe, json!({ "names": ["pr.*"] })).await.unwrap_err();
    assert_eq!(
        (e.code, e.detail.unwrap()["permission"].clone()),
        (ErrorCode::PermissionDenied, json!("events:pr.*"))
    );
    assert!(call(PluginMethod::EventsSubscribe, json!({ "names": ["session.bell"] })).await.is_ok());
    // net.fetch reaches CoreApi::http_fetch once allowed (FakeCore has no network).
    let e = call(PluginMethod::NetFetch, json!({ "url": "https://api.example.com/x" })).await.unwrap_err();
    assert_eq!(e.code, ErrorCode::Network);
    assert!(env.core.call_names().contains(&"http_fetch"));
}

#[tokio::test]
async fn settings_get_returns_defaults_and_effective_only_with_settings_read() {
    let schema = r#"{"type":"object","properties":{"greeting":{"type":"string","default":"Hello"}}}"#;
    let env = common::Env::new();
    let m = format!(
        "{}{SCREEN}\n[contributes.settings]\nschema = \"s.json\"\n",
        common::manifest("cfg", &["settings.read"], "")
    );
    env.write_plugin("cfg", &m, &[("index.html", "x"), ("s.json", schema)]);
    let id = PluginId::new("cfg");
    let s = env.host.screen_open(&id, "main", None, Value::Null).await.unwrap();
    let v = env
        .host
        .call(&s.instance_id, PluginMethod::SettingsGet, json!({}), origin(&s.instance_id))
        .await
        .unwrap();
    assert_eq!(v, json!({ "greeting": "Hello" }));
    env.host.grant(&id, vec!["settings.read".into()]).await.unwrap();
    let v = env
        .host
        .call(&s.instance_id, PluginMethod::SettingsGet, json!({}), origin(&s.instance_id))
        .await
        .unwrap();
    assert!(v.get("$effective").is_some());
    let fragments = kelta_proto::api::PluginSettingsSource::fragments(&*env.host);
    assert_eq!(fragments.len(), 1);
    assert_eq!(fragments[0].0, id);
}

#[tokio::test]
async fn manifest_update_adding_permissions_needs_a_new_grant() {
    let (env, _) = setup(&[]).await;
    let id = PluginId::new("matrix");
    let v1 = format!("{}{SCREEN}", common::manifest("matrix", &["projects.read"], ""));
    env.write_plugin("matrix", &v1, &[]);
    env.host.grant(&id, vec!["projects.read".into()]).await.unwrap();
    let v2 = format!("{}{SCREEN}", common::manifest("matrix", &["projects.read", "sessions.read"], ""));
    env.write_plugin("matrix", &v2, &[]);
    let s = env.host.screen_open(&id, "main", Some(&ProjectId::new("shop")), Value::Null).await.unwrap();
    let o = origin(&s.instance_id);
    assert!(env.host.call(&s.instance_id, PluginMethod::ProjectsCurrent, json!({}), o.clone()).await.is_ok());
    let e = env.host.call(&s.instance_id, PluginMethod::SessionsList, json!({}), o).await.unwrap_err();
    assert_eq!(e.code, ErrorCode::PermissionDenied);
    let info = env.host.plugins().await.unwrap().into_iter().find(|p| p.id == id).unwrap();
    assert_eq!(info.granted, vec!["projects.read"]);
    assert!(info.problems.iter().any(|p| p.contains("not granted: sessions.read")), "{:?}", info.problems);
}

#[tokio::test]
async fn disabled_plugins_cannot_open_screens_or_call() {
    let (env, instance) = setup(ALL_PERMS).await;
    let id = PluginId::new("matrix");
    env.host.enable(&id, false).await.unwrap();
    assert!(env.host.screen_open(&id, "main", None, Value::Null).await.is_err());
    let e = env.host.call(&instance, PluginMethod::AppInfo, json!({}), origin(&instance)).await.unwrap_err();
    assert_eq!(e.code, ErrorCode::NotFound, "screens of a disabled plugin are closed");
    env.host.enable(&id, true).await.unwrap();
    assert!(env.host.screen_open(&id, "main", None, Value::Null).await.is_ok());
}

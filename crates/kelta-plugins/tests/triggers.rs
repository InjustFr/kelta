//! Trigger engine: matchers (glob / regex / negation / any-of), recursion guard (depth ≤ 4, no
//! re-fire within a chain), rate limit, `send_keys` gate, blocking veto/patch, debounce under paused
//! time, `trigger_test`, `prompt`, plugin permissions.

#![allow(clippy::unwrap_used, clippy::expect_used)] // helpers outside #[test] fns unwrap too

mod common;

use std::collections::BTreeMap;
use std::time::Duration;

use kelta_proto::api::CoreApi;
use kelta_proto::events::{BusEvent, bus};
use kelta_proto::ext::{ActionDef, BlockingOutcome, Matcher, ToolDef, TriggerDef, TriggerOrigin};
use kelta_proto::ids::{PluginId, ProjectId, ToolId};
use kelta_proto::model::{Placement, SessionKind, SpawnRequest, TemplateCtx};
use serde_json::json;

fn toast(text: &str) -> ActionDef {
    ActionDef::Toast { text: text.into(), level: None }
}

fn trigger(id: &str, on: &str, matchers: &[(&str, Matcher)], actions: Vec<ActionDef>) -> TriggerDef {
    TriggerDef {
        id: id.into(),
        on: on.into(),
        r#match: matchers.iter().map(|(k, m)| (k.to_string(), m.clone())).collect::<BTreeMap<_, _>>(),
        r#do: actions,
        ..Default::default()
    }
}

fn s(v: &str) -> Matcher {
    Matcher::Str(v.into())
}

fn ev(name: &str, payload: serde_json::Value) -> BusEvent {
    BusEvent::new(name, payload).with_project(ProjectId::new("shop"))
}

fn toasts(env: &common::Env) -> Vec<String> {
    env.core.toasts().into_iter().map(|t| t.text).collect()
}

#[tokio::test]
async fn matchers_select_triggers() {
    let env = common::Env::new().with_settings(|st| {
        st.triggers = vec![
            trigger(
                "glob",
                "claude.file_edited",
                &[("payload.path", s("glob:*.rs"))],
                vec![toast("glob {payload.path}")],
            ),
            trigger("re", "custom.*", &[("payload.env", s("re:^stag"))], vec![toast("re {event.name}")]),
            trigger(
                "neg",
                "session.status_changed",
                &[("payload.status", s("!done"))],
                vec![toast("neg {payload.status}")],
            ),
            trigger(
                "any",
                "session.status_changed",
                &[("payload.status", Matcher::AnyOf(vec![s("needs_input"), s("waiting_user")]))],
                vec![toast("any {payload.status}")],
            ),
            trigger(
                "missing",
                "session.status_changed",
                &[("payload.nope", s("!x"))],
                vec![toast("missing")],
            ),
            trigger(
                "project",
                "pr.*",
                &[("project.id", s("shop")), ("payload.n", Matcher::Num(2.0))],
                vec![toast("pr {project.name}")],
            ),
            trigger("disabled", "custom.*", &[], vec![toast("disabled")]).tap_disable(),
        ];
    });
    for (name, payload) in [
        ("claude.file_edited", json!({"path": "src/lib.rs"})),
        ("claude.file_edited", json!({"path": "README.md"})),
        ("custom.deploy_finished", json!({"env": "staging"})),
        ("custom.deploy_finished", json!({"env": "prod"})),
        ("session.status_changed", json!({"status": "needs_input"})),
        ("session.status_changed", json!({"status": "done"})),
        ("pr.merged", json!({"n": 2})),
    ] {
        env.host.on_event(&ev(name, payload)).await;
    }
    env.host.settle().await;
    let mut got = toasts(&env);
    got.sort();
    assert_eq!(
        got,
        ["any needs_input", "glob src/lib.rs", "neg needs_input", "pr Shop", "re custom.deploy_finished"]
    );
}

trait TapDisable {
    fn tap_disable(self) -> Self;
}

impl TapDisable for TriggerDef {
    fn tap_disable(mut self) -> Self {
        self.enabled = false;
        self
    }
}

fn pty(id: &str) -> ToolDef {
    ToolDef { id: id.into(), command: Some("true".into()), ..Default::default() }
}

#[tokio::test]
async fn self_triggering_chains_stop_at_depth_four() {
    let env = common::Env::new().with_settings(|st| {
        st.tools = (0..=6).map(|i| pty(&format!("t{i}"))).collect();
        st.triggers = (0..=5)
            .map(|i| {
                trigger(
                    &format!("c{i}"),
                    "tool.opened",
                    &[("payload.tool_id", s(&format!("t{i}")))],
                    vec![ActionDef::OpenTool { tool: format!("t{}", i + 1), placement: None }],
                )
            })
            .chain([trigger(
                "self",
                "tool.opened",
                &[("payload.tool_id", s("s"))],
                vec![ActionDef::OpenTool { tool: "s".into(), placement: None }],
            )])
            .collect();
        st.tools.push(pty("s"));
    });
    let shop = ProjectId::new("shop");
    env.host.tool_open(&shop, &ToolId::new("t0"), TemplateCtx::default(), Placement::NewTab).await.unwrap();
    let host = env.host.clone();
    let finished = common::wait_for(|| {
        futures::executor::block_on(host.trigger_log(100)).unwrap().iter().any(|r| r.trigger_id == "c5")
    })
    .await;
    assert!(finished, "the chain reached c5");
    env.host.settle().await;
    let log = env.host.trigger_log(100).await.unwrap();
    for i in 0..=4u8 {
        let r = log.iter().find(|r| r.trigger_id == format!("c{i}")).unwrap();
        assert!(r.ok, "c{i}: {r:?}");
        assert_eq!(r.depth, i);
    }
    let c5 = log.iter().find(|r| r.trigger_id == "c5").unwrap();
    assert!(!c5.ok);
    assert!(c5.detail.as_deref().unwrap().starts_with("depth_exceeded"), "{c5:?}");
    let opened: Vec<String> = env
        .core
        .published()
        .iter()
        .filter(|e| e.name == bus::TOOL_OPENED)
        .map(|e| e.payload["tool_id"].as_str().unwrap().to_owned())
        .collect();
    assert_eq!(opened, ["t0", "t1", "t2", "t3", "t4", "t5"], "t6 is never opened");

    // A trigger whose action re-emits its own event fires once per chain.
    env.host.tool_open(&shop, &ToolId::new("s"), TemplateCtx::default(), Placement::NewTab).await.unwrap();
    let host = env.host.clone();
    assert!(
        common::wait_for(|| futures::executor::block_on(host.trigger_log(100))
            .unwrap()
            .iter()
            .any(|r| r.trigger_id == "self" && r.detail.as_deref().is_some_and(|d| d.starts_with("loop"))))
        .await
    );
    env.host.settle().await;
    let runs = env
        .host
        .trigger_log(100)
        .await
        .unwrap()
        .into_iter()
        .filter(|r| r.trigger_id == "self")
        .collect::<Vec<_>>();
    assert_eq!(runs.len(), 2, "{runs:?}");
    assert_eq!(runs.iter().filter(|r| r.ok).count(), 1);
}

#[tokio::test]
async fn per_trigger_rate_limit() {
    let env = common::Env::new()
        .with_settings(|st| st.triggers = vec![trigger("ping", "custom.ping", &[], vec![toast("pong")])]);
    for n in 0..12 {
        env.host.on_event(&ev("custom.ping", json!({ "n": n }))).await;
        env.host.settle().await;
    }
    let log = env.host.trigger_log(100).await.unwrap();
    assert_eq!(log.iter().filter(|r| r.ok).count(), 10);
    let limited: Vec<_> =
        log.iter().filter(|r| r.detail.as_deref().is_some_and(|d| d.starts_with("rate_limited"))).collect();
    assert_eq!(limited.len(), 2);
    assert_eq!(toasts(&env).len(), 10);
}

#[tokio::test]
async fn duplicate_fan_in_is_ignored() {
    let env = common::Env::new()
        .with_settings(|st| st.triggers = vec![trigger("once", "custom.x", &[], vec![toast("x")])]);
    let e = ev("custom.x", json!({}));
    env.host.on_event(&e).await;
    env.host.on_event(&e).await;
    env.host.settle().await;
    assert_eq!(toasts(&env), ["x"]);
}

fn sh(script: &str) -> ActionDef {
    ActionDef::Run {
        command: "sh".into(),
        args: vec!["-c".into(), script.into()],
        cwd: None,
        env: Default::default(),
        stdin: None,
        timeout_ms: None,
        show: None,
    }
}

#[tokio::test]
async fn blocking_triggers_veto_and_patch() {
    let mut veto = trigger(
        "guard",
        "ticket.before_start",
        &[("payload.plan.branch", s("re:^main$"))],
        vec![sh(r#"echo '{"veto":"main checkout is dirty"}'"#)],
    );
    veto.blocking = true;
    let mut patch = trigger(
        "rename",
        "ticket.before_start",
        &[],
        vec![sh(
            r#"cat >/dev/null; echo '{"patch":{"branch":"feat/x","claude":{"prompt":"hi"},"worktree_path":"/evil"}}'"#,
        )],
    );
    patch.blocking = true;
    let mut fail = trigger("fails", "work.before_finish", &[], vec![sh("echo 'tests are red' >&2; exit 1")]);
    fail.blocking = true;
    let env = common::Env::new().with_settings(|st| st.triggers = vec![veto, patch, fail]);

    let out = env
        .host
        .run_blocking(&ev(bus::TICKET_BEFORE_START, json!({"plan": {"branch": "main"}})))
        .await
        .unwrap();
    assert_eq!(
        out,
        BlockingOutcome::Veto { trigger_id: "guard".into(), reason: "main checkout is dirty".into() }
    );

    let out =
        env.host.run_blocking(&ev(bus::TICKET_BEFORE_START, json!({"plan": {"branch": "x"}}))).await.unwrap();
    assert_eq!(
        out,
        BlockingOutcome::Proceed { patch: Some(json!({"branch": "feat/x", "claude.prompt": "hi"})) }
    );

    let out = env.host.run_blocking(&ev(bus::WORK_BEFORE_FINISH, json!({}))).await.unwrap();
    assert_eq!(out, BlockingOutcome::Veto { trigger_id: "fails".into(), reason: "tests are red".into() });

    // Not a blocking event: nothing runs inline.
    assert_eq!(
        env.host.run_blocking(&ev("custom.x", json!({}))).await.unwrap(),
        BlockingOutcome::Proceed { patch: None }
    );
    // Blocking triggers do not run again through the async path.
    env.host.on_event(&ev(bus::TICKET_BEFORE_START, json!({"plan": {"branch": "main"}}))).await;
    env.host.settle().await;
    assert_eq!(env.host.trigger_log(100).await.unwrap().len(), 3);
}

#[tokio::test(start_paused = true)]
async fn debounce_runs_once_after_quiet_period() {
    let mut t = trigger(
        "clippy",
        "claude.file_edited",
        &[("payload.path", s("glob:*.rs"))],
        vec![toast("lint {payload.path}")],
    );
    t.debounce_ms = Some(1500);
    let env = common::Env::new().with_settings(|st| st.triggers = vec![t]);
    for (i, f) in ["a.rs", "b.rs", "c.rs"].iter().enumerate() {
        env.host.on_event(&ev("claude.file_edited", json!({ "path": f, "i": i }))).await;
        tokio::time::advance(Duration::from_millis(500)).await;
        tokio::task::yield_now().await;
    }
    assert!(toasts(&env).is_empty(), "nothing before the quiet period");
    env.host.settle().await;
    assert_eq!(toasts(&env), ["lint c.rs"]);
    // A new burst re-arms.
    env.host.on_event(&ev("claude.file_edited", json!({ "path": "d.rs" }))).await;
    env.host.settle().await;
    assert_eq!(toasts(&env), ["lint c.rs", "lint d.rs"]);
}

#[tokio::test]
async fn send_keys_needs_opt_in_and_is_rate_limited() {
    let keys = |allow: bool, id: &str| {
        let mut t = trigger(
            id,
            "custom.keys",
            &[],
            vec![ActionDef::SendKeys {
                session: "{event.session_id}".into(),
                text: "ls".into(),
                bracketed: Some(false),
            }],
        );
        t.allow_send_keys = allow;
        t
    };
    let env = common::Env::new()
        .with_settings(|st| st.triggers = vec![keys(false, "blocked"), keys(true, "allowed")]);
    let session = env
        .core
        .session_spawn(SpawnRequest {
            project_id: ProjectId::new("shop"),
            kind: SessionKind::Shell,
            name: None,
            program: None,
            args: vec![],
            cwd: None,
            env: Default::default(),
            cols: 80,
            rows: 24,
            work_item_id: None,
            restore: Default::default(),
            close_on_exit: Default::default(),
            template_id: None,
        })
        .await
        .unwrap();
    for n in 0..2 {
        env.host.on_event(&ev("custom.keys", json!({ "n": n })).with_session(session.id.clone())).await;
        env.host.settle().await;
    }
    assert_eq!(env.core.written_text(&session.id), "ls", "one write: the second is within 2 s");
    let log = env.host.trigger_log(10).await.unwrap();
    assert!(
        log.iter()
            .filter(|r| r.trigger_id == "blocked")
            .all(|r| !r.ok && r.detail.as_deref().unwrap().contains("allow_send_keys"))
    );
    assert!(
        log.iter().any(|r| r.trigger_id == "allowed"
            && !r.ok
            && r.detail.as_deref().unwrap().contains("at most one"))
    );
}

#[tokio::test]
async fn plugin_triggers_are_permission_checked_and_listed() {
    let env = common::Env::new()
        .with_settings(|st| st.triggers = vec![trigger("cfg", "custom.*", &[], vec![toast("cfg")])]);
    let m = common::manifest(
        "notifier",
        &["notify"],
        "\n[[contributes.triggers]]\nid = \"n\"\non = \"custom.note\"\ndo = [{ action = \"notify\", title = \"hi {project.name}\" }]\n",
    );
    env.write_plugin("notifier", &m, &[]);
    let list = env.host.triggers(None).await.unwrap();
    assert_eq!(list.iter().map(|t| t.id.as_str()).collect::<Vec<_>>(), ["cfg", "notifier/n"]);
    assert_eq!(list[1].origin, TriggerOrigin::Plugin { plugin_id: PluginId::new("notifier") });

    env.host.on_event(&ev("custom.note", json!({ "a": 1 }))).await;
    env.host.settle().await;
    let r =
        env.host.trigger_log(10).await.unwrap().into_iter().find(|r| r.trigger_id == "notifier/n").unwrap();
    assert!(!r.ok && r.detail.as_deref().unwrap().contains("missing permission: notify"), "{r:?}");
    assert!(env.core.notifications().is_empty());

    env.host.grant(&PluginId::new("notifier"), vec!["notify".into()]).await.unwrap();
    env.host.on_event(&ev("custom.note", json!({ "a": 2 }))).await;
    env.host.settle().await;
    assert_eq!(env.core.notifications()[0].title, "hi Shop");
}

#[tokio::test]
async fn trigger_test_and_prompt() {
    let prompt = ActionDef::Prompt {
        text: "Remove worktree for {ticket.key}?".into(),
        yes: vec![toast("removed")],
        no: vec![],
    };
    let env = common::Env::new().with_settings(|st| {
        st.triggers =
            vec![trigger("merge", "pr.merged", &[("payload.linked_tickets", s("re:.+"))], vec![prompt])]
    });
    let run = env.host.trigger_test("merge", json!({ "linked_tickets": [] })).await.unwrap();
    assert!(!run.ok);
    assert!(run.detail.unwrap().contains("no match"));
    let run = env.host.trigger_test("merge", json!({ "linked_tickets": ["SHOP-1"], "ticket": {"account": "jira-acme", "key": "SHOP-1", "id": "1"} })).await.unwrap();
    assert!(run.ok, "{run:?}");
    let t = env.core.toasts().pop().unwrap();
    assert_eq!(t.text, "Remove worktree for SHOP-1?");
    let action = t.action.unwrap();
    assert_eq!(action.command, kelta_plugins::actions::PROMPT_UI_ACTION);
    let command_id = action.args.unwrap()["command_id"].as_str().unwrap().to_owned();
    env.host.command_run(&command_id, TemplateCtx::default()).await.unwrap();
    assert_eq!(env.core.toasts().last().unwrap().text, "removed");
    assert!(
        env.host.command_run(&command_id, TemplateCtx::default()).await.is_err(),
        "a prompt answers once"
    );
    assert!(env.host.trigger_test("nope", json!({})).await.is_err());
}

#[tokio::test]
async fn commands_run_actions_with_plugin_permissions() {
    let env = common::Env::new();
    let src = common::example("hello-screen");
    let preview = env.host.inspect(src.to_str().unwrap()).await.unwrap();
    env.host.install(src.to_str().unwrap(), &preview.sha256, vec!["projects.read".into()]).await.unwrap();
    let cmds = env.host.commands().await.unwrap();
    let open = cmds.iter().find(|c| c.id == "hello-screen.open").unwrap();
    assert_eq!(open.keybinding.as_deref(), Some("mod+shift+h"));
    let mut ctx = TemplateCtx::default();
    ctx.extra.insert("project_id".into(), "shop".into());
    let e = env.host.command_run("hello-screen.open", ctx.clone()).await.unwrap_err();
    assert_eq!(e.code, kelta_proto::ErrorCode::PermissionDenied, "{e:?}");
    env.host.grant(&PluginId::new("hello-screen"), vec!["ui.open".into()]).await.unwrap();
    env.host.command_run("hello-screen.open", ctx).await.unwrap();
    let (project, req) = env.core.opened().pop().unwrap();
    assert_eq!(project.as_str(), "shop");
    assert!(
        matches!(req.content, kelta_proto::model::PaneContent::PluginScreen { ref screen_id, .. } if screen_id == "hello")
    );
}

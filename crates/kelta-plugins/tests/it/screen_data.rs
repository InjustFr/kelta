//! Screen ticket / work-item API (PLUGINS §7): served through core paths, scoped to the screen's project.

use std::sync::Arc;

use crate::common;

use kelta_proto::ErrorCode;
use kelta_proto::events::bus;
use kelta_proto::ext::{CallOrigin, PluginMethod};
use kelta_proto::ids::{AccountId, PluginId, ProjectId, ScreenInstanceId, WorkItemId};
use kelta_proto::samples;
use kelta_proto_testing::{FakeTracker, fixtures};
use serde_json::{Value, json};

const SCREENS: &str = "\n[[contributes.screens]]\nid = \"main\"\ntitle = \"Main\"\nentry = \"index.html\"\n\
    \n[[contributes.screens]]\nid = \"global\"\ntitle = \"Global\"\nentry = \"index.html\"\nscope = \"global\"\n";

async fn env() -> common::Env {
    let env = common::Env::new();
    let perms = ["tickets.read", "tickets.write", "work.read"];
    env.write_plugin(
        "qa-deploy",
        &format!("{}{SCREENS}", common::manifest("qa-deploy", &perms, "")),
        &[("index.html", "")],
    );
    env.host.grant(&PluginId::new("qa-deploy"), perms.iter().map(|s| s.to_string()).collect()).await.unwrap();
    env
}

async fn open(env: &common::Env, screen: &str, project: Option<&str>) -> ScreenInstanceId {
    let project = project.map(ProjectId::new);
    env.host
        .screen_open(&PluginId::new("qa-deploy"), screen, project.as_ref(), Value::Null)
        .await
        .unwrap()
        .instance_id
}

async fn call(
    env: &common::Env,
    i: &ScreenInstanceId,
    m: PluginMethod,
    p: Value,
) -> Result<Value, kelta_proto::KeltaError> {
    env.host.call(i, m, p, CallOrigin::Screen { instance_id: i.clone() }).await
}

fn last_args(env: &common::Env, method: &str) -> Value {
    env.core.calls().into_iter().rev().find(|c| c.method == method).unwrap().args
}

#[tokio::test]
async fn tickets_list_is_keltas_list_scoped_to_the_screen() {
    let env = env().await;
    let page = fixtures::load_value("ticket_page").unwrap();
    env.core.respond("ticket_list", page.clone());
    let project = open(&env, "main", Some("shop")).await;
    let got = call(&env, &project, PluginMethod::TicketsList, json!({ "scope": "all" })).await.unwrap();
    assert_eq!(got, page, "work_item_id and prs come from core's list");
    assert!(got["items"][0]["work_item_id"].is_string() && got["items"][0]["prs"][0]["url"].is_string());
    assert_eq!(last_args(&env, "ticket_list")["scope"], json!({ "kind": "project", "id": "shop" }));

    let global = open(&env, "global", Some("shop")).await;
    call(&env, &global, PluginMethod::TicketsList, json!({ "view_id": "mine" })).await.unwrap();
    let a = last_args(&env, "ticket_list");
    assert_eq!(
        (a["scope"].clone(), a["view_id"].clone()),
        (json!({ "kind": "project", "id": "shop" }), json!("mine"))
    );
    call(&env, &global, PluginMethod::TicketsList, json!({ "scope": "all" })).await.unwrap();
    assert_eq!(last_args(&env, "ticket_list")["scope"], json!({ "kind": "all" }));
}

#[tokio::test]
async fn ticket_writes_go_through_core_and_publish() {
    let env = env().await;
    env.core.add_tracker(AccountId::new("jira-acme"), Arc::new(FakeTracker::new()));
    let i = open(&env, "main", Some("shop")).await;
    let ticket = json!({ "account": "jira-acme", "key": "SHOP-141", "id": "10141" });
    let t =
        call(&env, &i, PluginMethod::TicketsTransition, json!({ "ticket": ticket, "transition_id": "t5" }))
            .await
            .unwrap();
    assert_eq!(t["status"]["name"], "Done");
    call(&env, &i, PluginMethod::TicketsAssign, json!({ "ticket": ticket, "assignee": { "kind": "me" } }))
        .await
        .unwrap();
    let names: Vec<String> = env.core.published().into_iter().map(|e| e.name).collect();
    assert!(names.contains(&bus::TICKET_TRANSITIONED.to_owned()), "{names:?}");
    assert_eq!(names.iter().filter(|n| *n == bus::TICKET_ASSIGNED).count(), 1, "{names:?}");
    let core_calls = env.core.call_names();
    assert!(core_calls.contains(&"ticket_transition") && core_calls.contains(&"ticket_assign"));
}

#[tokio::test]
async fn work_items_are_scoped_and_drop_private_fields() {
    let env = env().await;
    let mine = samples::work_item();
    let foreign = kelta_proto::model::WorkItem {
        id: WorkItemId::new("wi-other"),
        project_id: ProjectId::new("other"),
        ..mine.clone()
    };
    env.core.add_work_item(mine.clone());
    env.core.add_work_item(foreign.clone());

    let project = open(&env, "main", Some("shop")).await;
    let items = call(&env, &project, PluginMethod::WorkList, json!({ "project": null })).await.unwrap();
    let items = items.as_array().unwrap();
    assert_eq!(items.len(), 1, "a project screen only sees its project");
    assert_eq!(items[0]["branch"], json!(mine.branch));
    assert_eq!(items[0]["worktree"], json!(mine.worktree));
    for k in ["claude_uuid", "nvim_socket", "claude_message", "next_note", "sent_threads"] {
        assert!(items[0].get(k).is_none(), "{k} must not reach screens");
    }
    let got = call(&env, &project, PluginMethod::WorkGet, json!({ "id": mine.id })).await.unwrap();
    assert_eq!(got["id"], json!(mine.id));
    let e = call(&env, &project, PluginMethod::WorkGet, json!({ "id": foreign.id })).await.unwrap_err();
    assert_eq!(e.code, ErrorCode::NotFound);

    let global = open(&env, "global", Some("shop")).await;
    let own = call(&env, &global, PluginMethod::WorkList, json!({})).await.unwrap();
    assert_eq!(own.as_array().unwrap().len(), 1, "default = the screen's project");
    let all = call(&env, &global, PluginMethod::WorkList, json!({ "project": null })).await.unwrap();
    assert_eq!(all.as_array().unwrap().len(), 2);
    assert!(call(&env, &global, PluginMethod::WorkGet, json!({ "id": foreign.id })).await.is_ok());
}

#[tokio::test]
async fn work_methods_need_work_read() {
    let env = common::Env::new();
    env.write_plugin(
        "nowork",
        &format!("{}{SCREENS}", common::manifest("nowork", &["work.read"], "")),
        &[("index.html", "")],
    );
    let i = env
        .host
        .screen_open(&PluginId::new("nowork"), "main", Some(&ProjectId::new("shop")), Value::Null)
        .await
        .unwrap()
        .instance_id;
    let e = call(&env, &i, PluginMethod::WorkList, json!({})).await.unwrap_err();
    assert_eq!(
        (e.code, e.detail.unwrap()["permission"].clone()),
        (ErrorCode::PermissionDenied, json!("work.read"))
    );
}

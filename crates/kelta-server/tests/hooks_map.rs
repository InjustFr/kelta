#![allow(clippy::unwrap_used, clippy::expect_used)]
//! `hooks::map` over the hook fixtures: every row of ARCHITECTURE §7.6.

use std::path::PathBuf;

use kelta_proto::hooks::HookPayload;
use kelta_proto::model::SessionStatus;
use kelta_proto::testing::fixtures;
use kelta_server::hooks::map;

#[test]
fn table_over_fixtures() {
    let rows: &[(&str, SessionStatus, &str)] = &[
        ("hook_session_start", SessionStatus::Running, "SessionStart"),
        ("hook_user_prompt_submit", SessionStatus::Working, "UserPromptSubmit"),
        ("hook_permission_request", SessionStatus::NeedsInput, "PermissionRequest"),
        ("hook_notification_permission", SessionStatus::NeedsInput, "Notification:permission_prompt"),
        ("hook_notification_idle", SessionStatus::WaitingUser, "Notification:idle_prompt"),
        ("hook_stop", SessionStatus::Done, "Stop"),
        ("hook_stop_failure", SessionStatus::Error, "StopFailure"),
        ("hook_session_end", SessionStatus::Exited, "SessionEnd"),
        ("hook_post_tool_use_edit", SessionStatus::Unknown, "PostToolUse"),
    ];
    let all: Vec<String> = fixtures::names().into_iter().filter(|n| n.starts_with("hook_")).collect();
    assert_eq!(all.len(), rows.len(), "every hook fixture has a row: {all:?}");
    for (name, status, raw) in rows {
        let p: HookPayload = fixtures::load(name).unwrap();
        let c = map(&p).unwrap_or_else(|| panic!("{name} ignored"));
        assert_eq!(c.status, *status, "{name}");
        assert_eq!(c.raw_event, *raw, "{name}");
        match *name {
            "hook_stop" => assert_eq!(c.preview.as_deref(), Some("Done. All tests pass.")),
            "hook_post_tool_use_edit" => assert_eq!(
                c.file_edited,
                Some(PathBuf::from(
                    "/home/ada/.kelta-worktrees/shop/api/SHOP-142-rate-limit-login/src/login.rs"
                ))
            ),
            _ => {
                assert_eq!(c.file_edited, None, "{name}");
            }
        }
    }
}

#[test]
fn other_notifications_and_tools_and_events_are_ignored() {
    let base: HookPayload = fixtures::load("hook_notification_idle").unwrap();
    for kind in ["elicitation_dialog", "agent_needs_input"] {
        let p = HookPayload { notification_type: Some(kind.into()), ..base.clone() };
        assert_eq!(map(&p).unwrap().status, SessionStatus::NeedsInput, "{kind}");
    }
    let p = HookPayload { notification_type: Some("auth_success".into()), ..base.clone() };
    assert!(map(&p).is_none());

    let edit: HookPayload = fixtures::load("hook_post_tool_use_edit").unwrap();
    for tool in ["Write", "MultiEdit"] {
        let p = HookPayload { tool_name: Some(tool.into()), ..edit.clone() };
        assert!(map(&p).unwrap().file_edited.is_some(), "{tool}");
    }
    let p = HookPayload { tool_name: Some("Bash".into()), ..edit.clone() };
    assert!(map(&p).is_none());

    for ev in ["PreToolUse", "SubagentStop", "PreCompact", "", "Whatever"] {
        let p = HookPayload { hook_event_name: ev.into(), ..Default::default() };
        assert!(map(&p).is_none(), "{ev}");
    }
}

#[test]
fn stop_preview_is_capped() {
    let mut p: HookPayload = fixtures::load("hook_stop").unwrap();
    p.last_assistant_message = Some("x".repeat(1000));
    let pv = map(&p).unwrap().preview.unwrap();
    assert!(pv.chars().count() <= 201);
}

//! Hook StatusChange → status / attention / notification decision tables; attention
//! max-aggregation + badge; hooks-inactive one-shot and heuristic status; ctl hook dispatch;
//! OSC 52 policy.
#![allow(clippy::unwrap_used)] // fixture helpers outside #[test] fns

mod common;

use std::time::Duration;

use common::*;
use kelta_core::notifier::should_notify;
use kelta_core::status::{Heuristic, NotifyKind, SessState, apply_heuristic, apply_hook, mark_seen};
use kelta_proto::api::CoreApi;
use kelta_proto::ctl::CtlCommand;
use kelta_proto::events::UiEvent;
use kelta_proto::ids::ProjectId;
use kelta_proto::ipc::WindowState;
use kelta_proto::model::{
    Attention, OpenPaneRequest, PaneContent, Placement, SessionKind, SessionStatus, SpawnRequest,
    StatusChange, StatusSource,
};
use kelta_proto::settings::{NotificationSettings, Osc52, Settings};
use kelta_proto::term::{ClipboardKind, TerminalEvent};

use Attention as A;
use SessionStatus as S;

fn st(status: S, attention: A, seen: bool, visible: bool) -> SessState {
    SessState { status, attention, seen, visible }
}

fn ch(status: S) -> StatusChange {
    StatusChange { status, preview: None, file_edited: None, raw_event: "t".into(), session_uuid: None }
}

#[test]
fn hook_decision_table() {
    // (previous state, incoming status) → (status, attention, seen, notify)
    let rows: Vec<(SessState, S, S, A, bool, Option<NotifyKind>)> = vec![
        (st(S::Starting, A::None, true, false), S::Running, S::Running, A::None, true, None),
        (st(S::Running, A::None, true, false), S::Working, S::Working, A::Activity, false, None),
        (st(S::Running, A::None, true, true), S::Working, S::Working, A::None, true, None),
        (st(S::NeedsInput, A::NeedsInput, false, false), S::Working, S::Working, A::Activity, false, None),
        (
            st(S::Working, A::Activity, false, false),
            S::NeedsInput,
            S::NeedsInput,
            A::NeedsInput,
            false,
            Some(NotifyKind::ClaudeNeedsInput),
        ),
        (
            st(S::Working, A::None, true, true),
            S::NeedsInput,
            S::NeedsInput,
            A::NeedsInput,
            true,
            Some(NotifyKind::ClaudeNeedsInput),
        ),
        (
            st(S::Done, A::None, false, false),
            S::WaitingUser,
            S::WaitingUser,
            A::NeedsInput,
            false,
            Some(NotifyKind::ClaudeNeedsInput),
        ),
        (st(S::Done, A::None, true, false), S::WaitingUser, S::WaitingUser, A::None, true, None),
        (
            st(S::Working, A::Activity, false, false),
            S::Done,
            S::Done,
            A::Done,
            false,
            Some(NotifyKind::ClaudeDone),
        ),
        (st(S::Working, A::None, true, true), S::Done, S::Done, A::None, true, Some(NotifyKind::ClaudeDone)),
        (st(S::Working, A::None, true, true), S::Error, S::Error, A::Error, true, None),
        (st(S::NeedsInput, A::NeedsInput, false, false), S::Exited, S::Exited, A::None, false, None),
        (st(S::Working, A::Activity, false, false), S::Unknown, S::Working, A::Activity, false, None),
    ];
    for (prev, input, status, attention, seen, notify) in rows {
        let d = apply_hook(&prev, &ch(input));
        assert_eq!(
            (d.status, d.attention, d.seen, d.notify),
            (status, attention, seen, notify),
            "{prev:?} + {input:?}"
        );
    }
    assert_eq!(apply_hook(&st(S::Starting, A::None, true, false), &ch(S::Running)).hooks_active, Some(true));
    // mark_seen keeps a pending needs-input
    assert_eq!(mark_seen(&st(S::NeedsInput, A::NeedsInput, false, true)).attention, A::NeedsInput);
    assert_eq!(mark_seen(&st(S::Done, A::Done, false, true)).attention, A::None);
    // heuristic
    assert_eq!(apply_heuristic(&st(S::Running, A::None, true, false), Heuristic::Output).status, S::Working);
    assert_eq!(apply_heuristic(&st(S::Working, A::None, true, false), Heuristic::Quiet).status, S::Done);
    assert_eq!(apply_heuristic(&st(S::Running, A::None, true, false), Heuristic::Quiet).status, S::Running);
    assert_eq!(
        apply_heuristic(&st(S::Working, A::None, true, false), Heuristic::Alert).attention,
        A::NeedsInput
    );
}

#[test]
fn notification_rules_table() {
    let n = NotificationSettings::default();
    let focused = WindowState { exists: true, visible: true, focused: true };
    let unfocused = WindowState { focused: false, ..focused };
    let closed = WindowState { exists: false, visible: false, focused: false };
    let noon = 12 * 60;
    // (kind, settings, window, pane visible) → fires
    let quiet = NotificationSettings { quiet_hours: "11:00-13:00".into(), ..n.clone() };
    let off = NotificationSettings { enabled: false, ..n.clone() };
    let no_done = NotificationSettings { claude_done: false, ..n.clone() };
    let always = NotificationSettings { only_when_unfocused: false, ..n.clone() };
    let rows = [
        (NotifyKind::ClaudeNeedsInput, &n, focused, true, false),
        (NotifyKind::ClaudeNeedsInput, &n, focused, false, true),
        (NotifyKind::ClaudeNeedsInput, &n, unfocused, true, true),
        (NotifyKind::ClaudeDone, &n, closed, false, true),
        (NotifyKind::ClaudeDone, &no_done, unfocused, false, false),
        (NotifyKind::ClaudeDone, &quiet, unfocused, false, false),
        (NotifyKind::ClaudeDone, &off, unfocused, false, false),
        (NotifyKind::ClaudeDone, &always, focused, true, true),
        (NotifyKind::BellBackground, &n, unfocused, false, false),
        (NotifyKind::ReviewRequested, &n, focused, false, true),
    ];
    for (kind, s, w, visible, fires) in rows {
        assert_eq!(should_notify(kind, s, w, visible, noon), fires, "{kind:?} {w:?} visible={visible}");
    }
}

fn claude_req() -> SpawnRequest {
    SpawnRequest {
        id: None,
        project_id: ProjectId::new("shop"),
        kind: SessionKind::Claude,
        name: Some("claude".into()),
        program: Some("claude".into()),
        args: vec![],
        cwd: None,
        env: Default::default(),
        cols: 80,
        rows: 24,
        work_item_id: None,
        restore: Default::default(),
        close_on_exit: Default::default(),
        template_id: None,
    }
}

fn attention_events(ui: &kelta_proto::testing::FakeUiBridge) -> Vec<(String, Attention, u32, u32)> {
    ui.events()
        .into_iter()
        .filter_map(|e| match e {
            UiEvent::AttentionChanged { project_id, level, needs_input_count, total_needs_input } => {
                Some((project_id.to_string(), level, needs_input_count, total_needs_input))
            }
            _ => None,
        })
        .collect()
}

#[tokio::test]
async fn hooks_drive_status_attention_badge_and_notifications() {
    let tmp = tempfile::tempdir().unwrap();
    let h = start(tmp.path(), Settings::defaults(), vec![project("shop", tmp.path())]);
    h.core.project_activate(&ProjectId::new("shop")).unwrap();
    let a = h.core.session_spawn(claude_req()).await.unwrap();
    let b = h.core.session_spawn(claude_req()).await.unwrap();
    // a is on screen, b is a background session
    h.core
        .layout_open(
            &ProjectId::new("shop"),
            OpenPaneRequest {
                content: PaneContent::Terminal { session_id: a.id.clone() },
                placement: Placement::NewTab,
                focus: true,
                tab_title: None,
                work_item_id: None,
            },
        )
        .await
        .unwrap();
    let vis = h.core.visible_sessions();
    assert!(vis.contains(&a.id) && !vis.contains(&b.id));

    h.core.session_apply_hook(&a.id, ch(S::Running)).await.unwrap();
    let info = h.core.session_get(&a.id).unwrap();
    assert_eq!((info.status, info.status_source), (S::Running, StatusSource::Hook));
    assert!(info.claude.unwrap().hooks_active);

    // b needs input while the window is focused but b is hidden → notification + badge
    h.core.session_apply_hook(&b.id, ch(S::NeedsInput)).await.unwrap();
    assert_eq!(h.core.session_get(&b.id).unwrap().attention, A::NeedsInput);
    assert_eq!(h.ui.badge(), 1);
    assert_eq!(h.ui.notifications().len(), 1);
    assert_eq!(h.ui.notifications()[0].session_id.as_ref(), Some(&b.id));
    // a done while visible + focused → no attention, no notification
    h.core
        .session_apply_hook(&a.id, StatusChange { preview: Some("All done".into()), ..ch(S::Done) })
        .await
        .unwrap();
    let ai = h.core.session_get(&a.id).unwrap();
    assert_eq!((ai.status, ai.attention), (S::Done, A::None));
    assert_eq!(ai.claude.unwrap().preview.as_deref(), Some("All done"));
    assert_eq!(h.ui.notifications().len(), 1);
    // window unfocused: a errors → attention max-aggregates to NeedsInput (b) for the project
    h.ui.set_window_state(WindowState { exists: true, visible: true, focused: false });
    h.core.session_apply_hook(&a.id, ch(S::Error)).await.unwrap();
    let last = attention_events(&h.ui).last().cloned().unwrap();
    assert_eq!(last, ("shop".into(), A::NeedsInput, 1, 1));
    // b answered → project level drops to Error, badge 0
    h.core.session_apply_hook(&b.id, ch(S::Working)).await.unwrap();
    assert_eq!(attention_events(&h.ui).last().cloned().unwrap(), ("shop".into(), A::Error, 0, 0));
    assert_eq!(h.ui.badge(), 0);
    // PostToolUse: status unchanged, file recorded
    h.core
        .session_apply_hook(
            &b.id,
            StatusChange {
                file_edited: Some("/x/src/a.rs".into()),
                raw_event: "PostToolUse:Edit".into(),
                ..ch(S::Unknown)
            },
        )
        .await
        .unwrap();
    let bi = h.core.session_get(&b.id).unwrap();
    assert_eq!(bi.status, S::Working);
    assert_eq!(bi.claude.unwrap().files_touched, vec![std::path::PathBuf::from("/x/src/a.rs")]);
    // seen clears non-needs-input attention
    h.core.session_mark_seen(&a.id).unwrap();
    assert_eq!(h.core.session_get(&a.id).unwrap().attention, A::None);
}

#[tokio::test(start_paused = true)]
async fn hooks_inactive_falls_back_to_heuristics() {
    let tmp = tempfile::tempdir().unwrap();
    let h = start(tmp.path(), Settings::defaults(), vec![project("shop", tmp.path())]);
    let s = h.core.session_spawn(claude_req()).await.unwrap();
    assert_eq!(h.core.perf_snapshot().timers_armed, 1, "the 10 s hooks one-shot");
    tokio::time::sleep(Duration::from_secs(11)).await;
    settle().await;
    let info = h.core.session_get(&s.id).unwrap();
    assert_eq!(info.status_source, StatusSource::Heuristic);
    assert_eq!(h.core.perf_snapshot().timers_armed, 0);
    // output → Working (+ a 3 s quiet one-shot), quiet → Done
    h.term.emit(&s.id, TerminalEvent::Activity);
    assert_eq!(h.core.session_get(&s.id).unwrap().status, S::Working);
    assert_eq!(h.core.perf_snapshot().timers_armed, 1);
    tokio::time::sleep(Duration::from_secs(4)).await;
    settle().await;
    assert_eq!(h.core.session_get(&s.id).unwrap().status, S::Done);
    assert_eq!(h.core.perf_snapshot().timers_armed, 0);
    // BEL → needs input
    h.term.emit(&s.id, TerminalEvent::Bell);
    assert_eq!(h.core.session_get(&s.id).unwrap().attention, A::NeedsInput);

    // a session whose SessionStart arrives in time stays hook-driven
    let t = h.core.session_spawn(claude_req()).await.unwrap();
    h.core.session_apply_hook(&t.id, ch(S::Running)).await.unwrap();
    settle().await;
    tokio::time::sleep(Duration::from_secs(11)).await;
    settle().await;
    assert_eq!(h.core.session_get(&t.id).unwrap().status_source, StatusSource::Hook);
}

#[tokio::test]
async fn ctl_hook_through_the_server() {
    let tmp = tempfile::tempdir().unwrap();
    let h = start(tmp.path(), Settings::defaults(), vec![project("shop", tmp.path())]);
    let sock = h.core.server().start_ctl().await.unwrap();
    let s = h.core.session_spawn(claude_req()).await.unwrap();
    let token = h.term.with_session(&s.id, |x| x.spec.env["KELTA_HOOK_TOKEN"].clone()).unwrap();
    let hook = |token: &str, payload: serde_json::Value| serde_json::json!({ "v": 1, "cmd": "hook", "session": s.id, "token": token, "payload": payload });
    let mut rx = h.core.subscribe();
    let bad =
        ctl_send(&sock, hook("nope", serde_json::json!({ "hook_event_name": "PermissionRequest" }))).await;
    assert_eq!(bad["ok"], false, "{bad}");
    // `/clear` starts a new Claude conversation: the resume uuid must follow it.
    let r = ctl_send(
        &sock,
        hook(
            &token,
            serde_json::json!({ "hook_event_name": "PermissionRequest", "session_id": "claude-uuid-2" }),
        ),
    )
    .await;
    assert_eq!(r["ok"], true, "{r}");
    let info = h.core.session_get(&s.id).unwrap();
    assert_eq!(info.status, S::NeedsInput);
    assert_eq!(info.claude.unwrap().session_uuid, "claude-uuid-2");
    let edit = serde_json::json!({
        "hook_event_name": "PostToolUse",
        "session_id": "claude-uuid-2",
        "tool_name": "Edit",
        "tool_input": { "file_path": "/x/a.rs" },
    });
    assert_eq!(ctl_send(&sock, hook(&token, edit)).await["ok"], true);
    h.core.publish(kelta_proto::events::BusEvent::new("custom.end", serde_json::json!({})));
    let (mut hooks, mut edits) = (Vec::new(), Vec::new());
    loop {
        let ev = rx.recv().await.unwrap();
        match ev.name.as_str() {
            "claude.hook" => hooks.push(ev.payload["event"].clone()),
            "claude.file_edited" => edits.push(ev.payload),
            "custom.end" => break,
            _ => {}
        }
    }
    assert_eq!(hooks, ["PermissionRequest", "PostToolUse"]);
    assert_eq!(edits, [serde_json::json!({ "path": "/x/a.rs", "tool": "Edit" })], "one event per edit");
    // only custom.* can be emitted
    assert!(
        h.core
            .ctl(CtlCommand::Emit { name: "session.spawned".into(), payload: serde_json::json!({}) })
            .await
            .is_err()
    );
    h.core
        .ctl(CtlCommand::Emit { name: "custom.deploy".into(), payload: serde_json::json!({"ok": 1}) })
        .await
        .unwrap();
    assert_eq!(h.core.ctl(CtlCommand::Version).await.unwrap()["version"], kelta_proto::VERSION);
}

#[tokio::test]
async fn osc52_policy() {
    let tmp = tempfile::tempdir().unwrap();
    let mut settings = Settings::defaults();
    settings.terminal.osc52 = Osc52::Write;
    let h = start(tmp.path(), settings, vec![project("shop", tmp.path())]);
    let s = h
        .core
        .session_spawn(SpawnRequest { kind: SessionKind::Shell, program: None, ..claude_req() })
        .await
        .unwrap();
    h.term
        .emit(&s.id, TerminalEvent::ClipboardStore { kind: ClipboardKind::Clipboard, text: "copied".into() });
    settle().await;
    assert_eq!(h.core.clipboard_read(ClipboardKind::Clipboard).await.unwrap(), "copied");
    // write-only: loads are not answered
    h.term.emit(&s.id, TerminalEvent::ClipboardLoad { kind: ClipboardKind::Clipboard });
    settle().await;
    assert!(h.term.written(&s.id).is_empty());
    h.cfg.update(|s| s.terminal.osc52 = Osc52::ReadWrite);
    h.term.emit(&s.id, TerminalEvent::ClipboardLoad { kind: ClipboardKind::Clipboard });
    settle().await;
    assert_eq!(h.term.written(&s.id), b"\x1b]52;c;Y29waWVk\x07".to_vec());
    h.cfg.update(|s| s.terminal.osc52 = Osc52::Off);
    h.term.emit(&s.id, TerminalEvent::ClipboardStore { kind: ClipboardKind::Clipboard, text: "nope".into() });
    settle().await;
    assert_eq!(h.core.clipboard_read(ClipboardKind::Clipboard).await.unwrap(), "copied");
}

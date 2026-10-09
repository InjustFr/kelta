//! Behaviour of the `testing` fakes other lanes build on.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;

use kelta_proto::api::*;
use kelta_proto::codehost::{ReviewKind, ReviewQuery};
use kelta_proto::error::ErrorCode;
use kelta_proto::events::{BusEvent, UiEvent};
use kelta_proto::ids::*;
use kelta_proto::model::*;
use kelta_proto::secret::{SecretCtx, SecretRef};
use kelta_proto::settings::TrackerView;
use kelta_proto::term::*;
use kelta_proto::testing::*;
use kelta_proto::tracker::{Assignee, Cursor, StatusCategory};

fn spawn_req(kind: SessionKind) -> SpawnRequest {
    SpawnRequest {
        id: None,
        project_id: ProjectId::new("shop"),
        kind,
        name: None,
        program: None,
        args: vec![],
        cwd: Some(PathBuf::from("/tmp")),
        env: BTreeMap::new(),
        cols: 80,
        rows: 24,
        work_item_id: None,
        restore: RestorePolicy::None,
        close_on_exit: CloseOnExit::Never,
        template_id: None,
    }
}

#[tokio::test]
async fn fake_core_sessions_and_bus() {
    let core = FakeCore::new();
    let mut rx = core.subscribe();
    let s = core.session_spawn(spawn_req(SessionKind::Claude)).await.unwrap();
    assert!(s.claude.is_some());
    assert_eq!(rx.recv().await.unwrap().name, "session.spawned");
    core.session_write(&s.id, b"hi").await.unwrap();
    assert_eq!(core.written_text(&s.id), "hi");
    assert_eq!(core.session_list(Some(&ProjectId::new("shop"))).len(), 1);
    core.session_apply_hook(
        &s.id,
        StatusChange {
            status: SessionStatus::Working,
            preview: None,
            file_edited: None,
            raw_event: "UserPromptSubmit".into(),
        },
    )
    .await
    .unwrap();
    assert_eq!(core.session_get(&s.id).unwrap().status, SessionStatus::Working);
    core.publish(BusEvent::new("custom.x", serde_json::json!({})));
    assert_eq!(rx.recv().await.unwrap().name, "custom.x");
    core.fail("session_spawn", kelta_proto::KeltaError::conflict("nope"));
    assert_eq!(
        core.session_spawn(spawn_req(SessionKind::Shell)).await.unwrap_err().code,
        ErrorCode::Conflict
    );
    assert!(core.call_names().contains(&"session_write"));
    core.session_kill(&s.id, true).await.unwrap();
    assert_eq!(core.session_get(&s.id).unwrap().lifecycle, Lifecycle::Exited);
    let p = core
        .layout_open(
            &ProjectId::new("shop"),
            OpenPaneRequest {
                content: PaneContent::Inbox,
                placement: Placement::NewTab,
                focus: true,
                tab_title: None,
                work_item_id: None,
            },
        )
        .await
        .unwrap();
    assert_eq!(p.pane_id.as_str(), "pane-1");
}

#[test]
fn fake_terminal_host_scripts_frames_and_events() {
    let host = FakeTerminalHost::new();
    let events = RecordingTerminalEvents::new();
    let id = SessionId::new("s1");
    host.spawn(PtySpawnSpec {
        id: id.clone(),
        program: PathBuf::from("/bin/sh"),
        args: vec![],
        cwd: PathBuf::from("/"),
        env: BTreeMap::new(),
        cols: 80,
        rows: 24,
        scrollback_lines: 1000,
        kind: SessionKind::Shell,
        events: events.clone(),
    })
    .unwrap();
    host.script(&id, vec![encode_frame(FRAME_SNAPSHOT, b"hello")]);
    let sink = RecordingSink::new();
    let info = host.attach(&id, 100, 30, Box::new(sink.clone())).unwrap();
    assert_eq!(info.generation, 1);
    assert_eq!(sink.frames()[0][0], FRAME_SNAPSHOT);
    assert!(host.push_frame(&id, encode_frame(FRAME_DATA, b"x")));
    host.write(&id, b"ls\r").unwrap();
    assert_eq!(host.written(&id), b"ls\r");
    host.emit(&id, TerminalEvent::Bell);
    host.kill(&id, KillSignal::Term).unwrap();
    let evs = events.events();
    assert_eq!(evs[0].1, TerminalEvent::Bell);
    assert!(matches!(evs[1].1, TerminalEvent::Exited { signal: Some(15), .. }));
    host.set_text_tail(&id, "a\nb\nc");
    assert_eq!(host.text_tail(&id, 2).unwrap(), "b\nc");
    assert_eq!(host.write(&SessionId::new("nope"), b"").unwrap_err().code, ErrorCode::NotFound);
}

#[tokio::test]
async fn fake_tracker_paginates_and_transitions() {
    let t = FakeTracker::new().with_page_size(2);
    let view = TrackerView { id: "mine".into(), ..TrackerView::default() };
    let p1 = t.list(&view, None).await.unwrap();
    assert_eq!(p1.items.len(), 2);
    assert_eq!(p1.next, Some(Cursor::Offset(2)));
    let p2 = t.list(&view, p1.next).await.unwrap();
    assert_eq!(p2.items.len(), 1);
    assert!(p2.next.is_none());
    let r = p1.items[0].r#ref.clone();
    let trs = t.transitions(&r).await.unwrap();
    let done = trs.iter().find(|x| x.to.category == StatusCategory::Done).unwrap();
    t.require_fields(&done.id);
    assert_eq!(t.transition(&r, &done.id, None).await.unwrap_err().code, ErrorCode::NeedsFields);
    let tk = t.transition(&r, &done.id, Some(serde_json::json!({"resolution": "Fixed"}))).await.unwrap();
    assert_eq!(tk.status.category, StatusCategory::Done);
    assert!(t.assign(&r, Assignee::None).await.unwrap().assignee.is_none());
    t.fail_next(kelta_proto::KeltaError::needs_auth("401"));
    assert_eq!(t.me().await.unwrap_err().code, ErrorCode::NeedsAuth);
}

#[tokio::test]
async fn fake_code_host_approve_checks_head() {
    let h = FakeCodeHost::new();
    let q = ReviewQuery { kind: ReviewKind::ReviewRequested, include_team: true, include_drafts: false };
    let r = h.list_reviews(&q).await.unwrap().remove(0);
    assert_eq!(h.approve(&r.r#ref, "stale").await.unwrap_err().code, ErrorCode::Conflict);
    h.approve(&r.r#ref, &r.head_sha).await.unwrap();
    assert_eq!(h.approvals().len(), 1);
    let found = h.find_for_branch("acme/shop-api", "feat/SHOP-142-rate-limit-login").await.unwrap();
    assert_eq!(found.unwrap().r#ref.number, 90);
    assert_eq!(h.repo_from_remote("git@github.com:acme/shop.git").as_deref(), Some("acme/shop"));
    assert_eq!(h.fetch_refspec(&r.r#ref, "kelta/pr-87"), "pull/87/head:kelta/pr-87");
}

#[tokio::test]
async fn fake_settings_secrets_bridge_and_stores() {
    let s = FakeSettings::from_toml("[terminal]\nmax_live_views = 2\n").unwrap();
    s.add_project_toml("[project]\nid = \"shop\"\nname = \"Shop\"\n[terminal]\nmax_live_views = 6\n")
        .unwrap();
    assert_eq!(s.effective(None).terminal.max_live_views, 2);
    assert_eq!(s.effective(Some(&ProjectId::new("shop"))).terminal.max_live_views, 6);
    assert_eq!(s.projects().len(), 1);

    let sec = FakeSecrets::with(&[("env:TOKEN", "t0k")]);
    let v = sec.resolve(&SecretRef::new("env:TOKEN"), &SecretCtx::default()).await.unwrap();
    assert_eq!(v.expose(), "t0k");
    assert!(sec.set(&SecretRef::new("env:X"), "v").await.is_err());

    let b = FakeUiBridge::new();
    b.emit(UiEvent::SessionRemoved { id: SessionId::new("x") });
    b.set_badge(3);
    assert_eq!(b.event_names(), vec!["session.removed"]);
    assert_eq!(b.badge(), 3);

    let ws = MemWorkStore::new();
    let id = WorkItemId::new("w1");
    ws.set_step(&id, "claude", StepStatus::Done, None).await.unwrap();
    ws.set_step(&id, "fetch_ticket", StepStatus::Running, None).await.unwrap();
    let steps: Vec<String> = ws.steps(&id).await.unwrap().into_iter().map(|s| s.step).collect();
    assert_eq!(steps, vec!["fetch_ticket", "claude"]);

    let g = MemGrantStore::new();
    let pid = PluginId::new("tools-pack");
    g.grant(&pid, &["notify".into()], "abc").await.unwrap();
    assert_eq!(g.grants(&pid).await.unwrap().len(), 1);
    let t = MemTrustStore::new();
    t.set_trust(std::path::Path::new("/r/.kelta/config.toml"), Some("h".into())).await.unwrap();
    assert_eq!(
        t.trusted_hash(std::path::Path::new("/r/.kelta/config.toml")).await.unwrap().as_deref(),
        Some("h")
    );

    let _shared: Arc<dyn SettingsSource> = s;
}

#[test]
fn fixtures_load() {
    let info: SessionInfo = fixtures::load("session_info").unwrap();
    assert_eq!(info.kind, SessionKind::Claude);
    assert!(fixtures::names().contains(&"ui_event_toast".to_owned()));
    assert_eq!(fixtures::load::<SessionInfo>("nope").unwrap_err().code, ErrorCode::NotFound);
}

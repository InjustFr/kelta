//! Park (#142): the item's Claude goes Dormant (process killed, conversation kept) and resumes on
//! the next attach; an nvim that cannot be checked for unsaved buffers keeps running; auto-park
//! fires only on a done + seen Claude and input cancels it.

use std::time::Duration;

use crate::common::*;
use kelta_proto::api::CoreApi;
use kelta_proto::ids::{ProjectId, SessionId};
use kelta_proto::model::{
    Lifecycle, RestorePolicy, SessionKind, SessionStatus as S, SpawnRequest, StatusChange, WorkItem,
};
use kelta_proto::samples::{CLAUDE_UUID, SID, SID2};
use kelta_proto::settings::Settings;
use kelta_proto::term::KillSignal;
use kelta_proto::testing::RecordingSink;

fn req(id: &str, kind: SessionKind, program: &str, args: &[&str], restore: RestorePolicy) -> SpawnRequest {
    SpawnRequest {
        id: Some(SessionId::new(id)),
        project_id: ProjectId::new("shop"),
        kind,
        name: None,
        program: Some(program.into()),
        args: args.iter().map(|s| (*s).to_owned()).collect(),
        cwd: None,
        env: Default::default(),
        cols: 80,
        rows: 24,
        work_item_id: Some(kelta_proto::samples::work_item().id),
        restore,
        close_on_exit: Default::default(),
        template_id: None,
    }
}

fn ch(status: S) -> StatusChange {
    StatusChange { status, preview: None, file_edited: None, raw_event: "t".into(), session_uuid: None }
}

/// The sample item (sessions SID = Claude, SID2 = nvim) in the store, its Claude spawned.
async fn seed(h: &H, root: &std::path::Path) -> WorkItem {
    let mut item = kelta_proto::samples::work_item();
    item.repo_id = "main".into();
    item.worktree = root.to_path_buf();
    let it = item.clone();
    h.core.store().call(move |c| kelta_core::store::q::work_put(c, &it)).await.unwrap();
    let claude = req(
        SID,
        SessionKind::Claude,
        "claude",
        &["--session-id", CLAUDE_UUID, "-n", "x", "do it"],
        RestorePolicy::ClaudeResume { uuid: CLAUDE_UUID.into() },
    );
    h.core.session_spawn(claude).await.unwrap();
    item
}

async fn parked_at(h: &H, item: &WorkItem) -> Option<String> {
    CoreApi::work_get(h.core.as_ref(), &item.id).await.unwrap().parked_at
}

fn lifecycle(h: &H, id: &str) -> Lifecycle {
    h.core.session_get(&SessionId::new(id)).unwrap().lifecycle
}

#[tokio::test]
async fn park_frees_claude_and_reopen_resumes_the_conversation() {
    let tmp = tempfile::tempdir().unwrap();
    let h = start(tmp.path(), Settings::defaults(), vec![project("shop", tmp.path())]);
    let item = seed(&h, tmp.path()).await;
    // An nvim whose socket does not answer cannot be checked for unsaved buffers: it is kept.
    let sock = tmp.path().join("gone.sock");
    let nvim = req(
        SID2,
        SessionKind::Editor { adapter: "nvim".into() },
        "nvim",
        &["--listen", sock.to_str().unwrap(), "."],
        RestorePolicy::Editor { session_file: None },
    );
    h.core.session_spawn(nvim).await.unwrap();
    h.core.session_apply_hook(&SessionId::new(SID), ch(S::Working)).await.unwrap();
    let busy = h.core.work_park(&item.id, false).await.unwrap_err();
    assert!(busy.message.contains("working"), "a working Claude is never parked: {}", busy.message);
    h.core.session_apply_hook(&SessionId::new(SID), ch(S::Done)).await.unwrap();

    let parked = h.core.work_park(&item.id, false).await.unwrap();
    assert!(parked.parked_at.is_some());
    let sid = SessionId::new(SID);
    assert_eq!(h.term.with_session(&sid, |s| s.killed).unwrap(), Some(KillSignal::Hup), "process killed");
    assert_eq!(lifecycle(&h, SID), Lifecycle::Dormant);
    assert_eq!(lifecycle(&h, SID2), Lifecycle::Live, "unverified nvim kept");
    assert_eq!(h.term.with_session(&SessionId::new(SID2), |s| s.killed).unwrap(), None);
    let live_claude = h
        .core
        .session_list(None)
        .iter()
        .filter(|s| s.kind == SessionKind::Claude && s.lifecycle == Lifecycle::Live)
        .count();
    assert_eq!(live_claude, 0, "the Claude slot is free");

    // Showing it again: the same conversation, resumed.
    h.core.session_attach(&sid, 80, 24, Box::new(RecordingSink::new())).await.unwrap();
    let args = h.term.with_session(&sid, |s| s.spec.args.clone()).unwrap();
    let at = args.iter().position(|a| a == "--resume").expect("--resume");
    assert_eq!(args[at + 1], CLAUDE_UUID);
    assert_eq!(lifecycle(&h, SID), Lifecycle::Live);
    for _ in 0..50 {
        settle().await;
        if parked_at(&h, &item).await.is_none() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("parked_at not cleared after the resume");
}

#[tokio::test(start_paused = true)]
async fn auto_park_only_after_done_and_seen_and_input_cancels_it() {
    let tmp = tempfile::tempdir().unwrap();
    let mut settings = Settings::defaults();
    settings.claude.auto_park_after_mins = 1;
    let h = start(tmp.path(), settings, vec![project("shop", tmp.path())]);
    let item = seed(&h, tmp.path()).await;
    let sid = SessionId::new(SID);
    let wait = || async {
        tokio::time::sleep(Duration::from_secs(61)).await;
        settle().await;
    };

    h.core.session_apply_hook(&sid, ch(S::Working)).await.unwrap();
    wait().await;
    assert_eq!(lifecycle(&h, SID), Lifecycle::Live, "never while working");

    h.core.session_apply_hook(&sid, ch(S::Done)).await.unwrap();
    wait().await;
    assert_eq!(lifecycle(&h, SID), Lifecycle::Live, "done but not seen yet");

    h.core.session_mark_seen(&sid).unwrap();
    h.core.session_apply_hook(&sid, ch(S::NeedsInput)).await.unwrap();
    wait().await;
    assert_eq!(lifecycle(&h, SID), Lifecycle::Live, "never while it needs input");

    h.core.session_apply_hook(&sid, ch(S::Done)).await.unwrap();
    h.core.session_mark_seen(&sid).unwrap();
    tokio::time::sleep(Duration::from_secs(30)).await;
    h.core.session_write(&sid, b"x").await.unwrap();
    wait().await;
    assert_eq!(lifecycle(&h, SID), Lifecycle::Live, "input cancels auto-park");

    h.core.session_apply_hook(&sid, ch(S::Working)).await.unwrap();
    h.core.session_apply_hook(&sid, ch(S::Done)).await.unwrap();
    h.core.session_mark_seen(&sid).unwrap();
    // Showing the pane sends a focus report: not input.
    h.core.session_write(&sid, b"\x1b[I").await.unwrap();
    // idle_prompt ~60 s after Stop: still parkable.
    h.core.session_apply_hook(&sid, ch(S::WaitingUser)).await.unwrap();
    wait().await;
    for _ in 0..50 {
        settle().await;
        if parked_at(&h, &item).await.is_some() {
            break;
        }
    }
    assert_eq!(lifecycle(&h, SID), Lifecycle::Dormant, "done + seen + no input → parked");
    assert!(parked_at(&h, &item).await.is_some());
}

//! Background runs claiming tickets (#235): adopt, conflict, progress, release, orphan + takeover.

use std::path::PathBuf;
use std::time::Duration;

use crate::common::{Fx, git, has_git, project};
use kelta_proto::api::WorkStore;
use kelta_proto::error::ErrorCode;
use kelta_proto::ids::{SessionId, WorkItemId};
use kelta_proto::model::{Lifecycle, RunClaim, RunState, SessionInfo, WorkState};
use kelta_proto::samples;

fn run(sid: &str) -> RunClaim {
    RunClaim {
        name: Some("kelta-ticket-pass".into()),
        run_id: Some(format!("run-{sid}")),
        session_id: Some(SessionId::new(sid)),
        phase: None,
        state: RunState::Running,
        note: None,
        sha: None,
    }
}

fn session(fx: &Fx, sid: &str, name: &str) {
    fx.core.insert_session(SessionInfo {
        id: SessionId::new(sid),
        name: name.into(),
        ..samples::session_info()
    });
}

/// A worktree the run made itself, as `kelta-ticket-pass` does.
fn run_worktree(fx: &Fx, branch: &str) -> PathBuf {
    let wt = fx.tmp.path().join("runs").join(branch.replace('/', "-"));
    git(&fx.repo, &["worktree", "add", "-q", "-b", branch, wt.to_str().unwrap()]);
    wt
}

async fn run_state(fx: &Fx, id: &WorkItemId) -> RunState {
    fx.store.get_item(id).await.unwrap().unwrap().run.unwrap().state
}

#[tokio::test]
async fn claim_adopts_the_worktree_and_conflicts_name_the_holder() {
    if !has_git() {
        return;
    }
    let fx = Fx::new();
    session(&fx, "orch-a", "kelta-a6");
    session(&fx, "orch-b", "kelta-b2");
    let w = fx.service();
    let wt = run_worktree(&fx, "feat/ticket-142");
    let (t, p) = (samples::ticket_ref(), project());

    // Two pickers race for the same ticket: exactly one wins.
    let (a, b) = tokio::join!(
        w.claim(&p, t.clone(), Some(wt.clone()), None, run("orch-a")),
        w.claim(&p, t.clone(), Some(wt.clone()), None, run("orch-b")),
    );
    assert!(a.is_ok() != b.is_ok(), "{a:?} / {b:?}");
    let (item, err) = if a.is_ok() { (a.unwrap(), b.unwrap_err()) } else { (b.unwrap(), a.unwrap_err()) };
    assert_eq!(item.branch, "feat/ticket-142", "branch read from the worktree");
    assert_eq!(item.worktree.canonicalize().unwrap(), wt.canonicalize().unwrap());
    assert_eq!(item.state, WorkState::Active);
    assert!(item.session_ids.is_empty(), "the orchestrator session is never killed by Finish");
    assert_eq!(err.code, ErrorCode::Conflict);
    let holder = if item.run.as_ref().unwrap().session_id == Some(SessionId::new("orch-a")) {
        "kelta-a6"
    } else {
        "kelta-b2"
    };
    assert_eq!(err.message, format!("SHOP-142 is held by session {holder} (kelta-ticket-pass)"));
    assert_eq!(fx.store.list_items(None).await.unwrap().len(), 1);
}

#[tokio::test]
async fn lanes_of_one_session_conflict_but_a_lane_re_claims_its_own() {
    if !has_git() {
        return;
    }
    let fx = Fx::new();
    session(&fx, "orch-a", "kelta-a6");
    let w = fx.service();
    let wt = run_worktree(&fx, "feat/ticket-142");
    let (t, p) = (samples::ticket_ref(), project());
    let lane = |id: &str| RunClaim { run_id: Some(id.into()), ..run("orch-a") };

    // Same session, different lanes: the second pick loses.
    let a = w.claim(&p, t.clone(), Some(wt.clone()), None, lane("lane-1")).await.unwrap();
    let b = w.claim(&p, t.clone(), Some(wt.clone()), None, lane("lane-2")).await.unwrap_err();
    assert_eq!(b.code, ErrorCode::Conflict);
    // Same lane (Pick then Implement): same item.
    let again = w.claim(&p, t, Some(wt), None, lane("lane-1")).await.unwrap();
    assert_eq!(again.id, a.id);
    assert_eq!(fx.store.list_items(None).await.unwrap().len(), 1);
    // A blocked lane has ended: a later lane (resumed board run) takes the claim over.
    w.report("SHOP-142", None, RunState::Blocked, None, None).await.unwrap();
    let t = samples::ticket_ref();
    let taken = w.claim(&p, t, None, None, lane("lane-3")).await.unwrap();
    assert_eq!(taken.id, a.id);
    assert_eq!(taken.run.unwrap().state, RunState::Running);
}

#[tokio::test]
async fn claim_refuses_a_path_that_is_not_a_project_worktree() {
    if !has_git() {
        return;
    }
    let fx = Fx::new();
    let w = fx.service();
    let stray = fx.tmp.path().join("stray");
    std::fs::create_dir_all(&stray).unwrap();
    let e = w.claim(&project(), samples::ticket_ref(), Some(stray), None, run("orch-a")).await.unwrap_err();
    assert_eq!(e.code, ErrorCode::InvalidArgument, "{}", e.message);
    assert!(fx.store.list_items(None).await.unwrap().is_empty());
}

#[tokio::test]
async fn progress_then_release_leaves_the_worktree_alone() {
    if !has_git() {
        return;
    }
    let fx = Fx::new();
    let w = fx.service();
    let wt = run_worktree(&fx, "feat/ticket-142");
    let item =
        w.claim(&project(), samples::ticket_ref(), Some(wt.clone()), None, run("orch-a")).await.unwrap();

    w.report(
        "SHOP-142",
        Some("Review".into()),
        RunState::Blocked,
        Some("2 unresolved review findings".into()),
        None,
    )
    .await
    .unwrap();
    let r = fx.store.get_item(&item.id).await.unwrap().unwrap().run.unwrap();
    assert_eq!(
        (r.phase.as_deref(), r.state, r.note.as_deref()),
        (Some("Review"), RunState::Blocked, Some("2 unresolved review findings"))
    );
    // `None` keeps the last phase and note.
    w.report("SHOP-142", None, RunState::Running, None, Some("abc123".into())).await.unwrap();
    let r = fx.store.get_item(&item.id).await.unwrap().unwrap().run.unwrap();
    assert_eq!(
        (r.phase.as_deref(), r.note.is_some(), r.sha.as_deref()),
        (Some("Review"), true, Some("abc123"))
    );

    // Abandoned: finished, worktree and branch kept.
    let done = w.release("SHOP-142", None).await.unwrap();
    assert_eq!(done.state, WorkState::Finished);
    assert!(wt.join("README.md").exists());
    assert_eq!(fx.worktree_count(), 2);
    assert!(git(&fx.repo, &["branch", "--list", "feat/ticket-142"]).contains("feat/ticket-142"));
    assert!(w.report("SHOP-142", None, RunState::Running, None, None).await.is_err(), "no claim left");

    // Merged: the run removed its worktree first; Finish does not trip over it.
    let item =
        w.claim(&project(), samples::ticket_ref(), Some(wt.clone()), None, run("orch-a")).await.unwrap();
    git(&fx.repo, &["worktree", "remove", wt.to_str().unwrap()]);
    let done = w.release("SHOP-142", Some("def456".into())).await.unwrap();
    assert_eq!((done.id, done.state), (item.id, WorkState::Finished));
    assert_eq!(done.run.unwrap().sha.as_deref(), Some("def456"));
}

#[tokio::test]
async fn exited_session_orphans_its_running_claims_and_a_new_claim_takes_over() {
    if !has_git() {
        return;
    }
    let fx = Fx::new();
    session(&fx, "orch-a", "kelta-a6");
    session(&fx, "orch-b", "kelta-b2");
    let w = fx.service();
    let wt = run_worktree(&fx, "feat/ticket-142");
    let item =
        w.claim(&project(), samples::ticket_ref(), Some(wt.clone()), None, run("orch-a")).await.unwrap();

    fx.core.exit_session(&SessionId::new("orch-a"), 0);
    for _ in 0..500 {
        if run_state(&fx, &item.id).await == RunState::Orphaned {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert_eq!(run_state(&fx, &item.id).await, RunState::Orphaned);

    let again = w.claim(&project(), samples::ticket_ref(), Some(wt), None, run("orch-b")).await.unwrap();
    assert_eq!(again.id, item.id, "same work item");
    let r = again.run.unwrap();
    assert_eq!((r.state, r.session_id), (RunState::Running, Some(SessionId::new("orch-b"))));
}

#[tokio::test]
async fn startup_orphans_claims_of_dormant_or_missing_sessions_but_not_live_ones() {
    if !has_git() {
        return;
    }
    let fx = Fx::new();
    session(&fx, "orch-a", "kelta-a6");
    let w = fx.service();
    let wt = run_worktree(&fx, "feat/ticket-142");
    let item = w.claim(&project(), samples::ticket_ref(), Some(wt), None, run("orch-a")).await.unwrap();

    w.startup().await.unwrap();
    assert_eq!(run_state(&fx, &item.id).await, RunState::Running, "live owner keeps its claim");

    // Kelta quit: after relaunch the owner is restored Dormant, its workflow is dead.
    fx.core.insert_session(SessionInfo {
        id: SessionId::new("orch-a"),
        lifecycle: Lifecycle::Dormant,
        ..samples::session_info()
    });
    w.startup().await.unwrap();
    assert_eq!(run_state(&fx, &item.id).await, RunState::Orphaned);
}

//! Background runs claiming tickets (#235): adopt, conflict, progress, release, orphan + takeover.
//! Liveness is computed from the owner session (#271): no sweep, `w.list` shows `orphaned`.

use std::path::PathBuf;

use crate::common::{Fx, git, has_git, project};
use crate::signals::{hook, wait_item};
use kelta_proto::api::{Tracker, WorkStore};
use kelta_proto::error::ErrorCode;
use kelta_proto::ids::{SessionId, WorkItemId};
use kelta_proto::model::{
    ClaudeMeta, Lifecycle, PaneContent, RunClaim, RunState, SessionInfo, SessionKind, WorkSource, WorkState,
};
use kelta_proto::samples;
use kelta_proto::tracker::TicketRef;
use serde_json::json;

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

fn orch_a() -> SessionId {
    SessionId::new("orch-a")
}

fn session(fx: &Fx, sid: &str, name: &str) {
    fx.core.insert_session(SessionInfo {
        id: SessionId::new(sid),
        name: name.into(),
        ..samples::session_info()
    });
}

/// The Claude of a Kelta-started run, in the repo it was started in (#491).
fn run_claude(fx: &Fx) -> SessionInfo {
    let s = SessionInfo {
        id: SessionId::new("run-s"),
        name: "kelta-ticket-pass · SHOP-142".into(),
        cwd: fx.repo.clone(),
        work_item_id: None,
        claude: Some(ClaudeMeta { session_uuid: "run-uuid".into(), ..ClaudeMeta::default() }),
        ..samples::session_info()
    };
    fx.core.insert_session(s.clone());
    s
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

/// The run as `work_list` shows it (liveness computed).
async fn shown(w: &kelta_work::WorkService, id: &WorkItemId) -> RunClaim {
    w.list(None).await.unwrap().into_iter().find(|i| &i.id == id).unwrap().run.unwrap()
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
    w.report("SHOP-142", None, RunState::Blocked, None, None, Some(&orch_a())).await.unwrap();
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
    session(&fx, "orch-a", "kelta-a6");
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
        Some(&orch_a()),
    )
    .await
    .unwrap();
    let r = fx.store.get_item(&item.id).await.unwrap().unwrap().run.unwrap();
    assert_eq!(
        (r.phase.as_deref(), r.state, r.note.as_deref()),
        (Some("Review"), RunState::Blocked, Some("2 unresolved review findings"))
    );
    // `None` keeps the last phase and note.
    w.report("SHOP-142", None, RunState::Running, None, Some("abc123".into()), Some(&orch_a()))
        .await
        .unwrap();
    let r = fx.store.get_item(&item.id).await.unwrap().unwrap().run.unwrap();
    assert_eq!(
        (r.phase.as_deref(), r.note.is_some(), r.sha.as_deref()),
        (Some("Review"), true, Some("abc123"))
    );

    // Abandoned: finished, worktree and branch kept.
    let done = w.release("SHOP-142", None, Some(&orch_a())).await.unwrap();
    assert_eq!(done.state, WorkState::Finished);
    assert!(wt.join("README.md").exists());
    assert_eq!(fx.worktree_count(), 2);
    assert!(git(&fx.repo, &["branch", "--list", "feat/ticket-142"]).contains("feat/ticket-142"));
    assert!(
        w.report("SHOP-142", None, RunState::Running, None, None, Some(&orch_a())).await.is_err(),
        "no claim left"
    );

    // Merged: the run removed its worktree first; Finish does not trip over it.
    let item =
        w.claim(&project(), samples::ticket_ref(), Some(wt.clone()), None, run("orch-a")).await.unwrap();
    git(&fx.repo, &["worktree", "remove", wt.to_str().unwrap()]);
    let done = w.release("SHOP-142", Some("def456".into()), Some(&orch_a())).await.unwrap();
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
    assert_eq!(shown(&w, &item.id).await.state, RunState::Orphaned, "computed, nothing stored");
    assert_eq!(w.get(&item.id).await.unwrap().run.unwrap().state, RunState::Orphaned, "work_get too");
    assert_eq!(run_state(&fx, &item.id).await, RunState::Running);

    let again = w.claim(&project(), samples::ticket_ref(), Some(wt), None, run("orch-b")).await.unwrap();
    assert_eq!(again.id, item.id, "same work item");
    let r = again.run.unwrap();
    assert_eq!((r.state, r.session_id), (RunState::Running, Some(SessionId::new("orch-b"))));
}

#[tokio::test]
async fn a_dormant_or_missing_owner_is_orphaned_without_a_sweep() {
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
    assert_eq!(shown(&w, &item.id).await.state, RunState::Running, "live owner keeps its claim");
    let e = w.claim(&project(), samples::ticket_ref(), Some(wt.clone()), None, run("orch-b")).await;
    assert_eq!(e.unwrap_err().code, ErrorCode::Conflict);

    // Kelta quit: after relaunch the owner is restored Dormant, its workflow is dead. No startup sweep.
    fx.core.insert_session(SessionInfo {
        id: SessionId::new("orch-a"),
        lifecycle: Lifecycle::Dormant,
        ..samples::session_info()
    });
    assert_eq!(shown(&w, &item.id).await.state, RunState::Orphaned);
    let taken = w.claim(&project(), samples::ticket_ref(), Some(wt), None, run("orch-b")).await.unwrap();
    assert_eq!((taken.id, taken.run.unwrap().session_id), (item.id.clone(), Some(SessionId::new("orch-b"))));

    // A missing owner (no session at all) can be released by anyone.
    let ghost = RunClaim { session_id: Some(SessionId::new("ghost")), ..run("orch-b") };
    w.report("SHOP-142", None, RunState::Blocked, None, None, Some(&SessionId::new("orch-b"))).await.unwrap();
    let t = samples::ticket_ref();
    w.claim(&project(), t, None, None, RunClaim { run_id: Some("r2".into()), ..ghost }).await.unwrap();
    assert_eq!(shown(&w, &item.id).await.state, RunState::Orphaned);
    let done = w.release("SHOP-142", None, Some(&SessionId::new("orch-b"))).await.unwrap();
    assert_eq!(done.state, WorkState::Finished);
}

#[tokio::test]
async fn a_non_owner_driving_a_dead_owners_claim_takes_it_over() {
    if !has_git() {
        return;
    }
    let fx = Fx::new();
    session(&fx, "orch-a", "kelta-a6");
    session(&fx, "orch-b", "kelta-b2");
    let w = fx.service();
    let wt = run_worktree(&fx, "feat/ticket-142");
    let item = w.claim(&project(), samples::ticket_ref(), Some(wt), None, run("orch-a")).await.unwrap();
    fx.core.exit_session(&orch_a(), 0);

    let b = SessionId::new("orch-b");
    w.report("SHOP-142", None, RunState::Running, None, None, Some(&b)).await.unwrap();
    let r = shown(&w, &item.id).await;
    assert_eq!((r.state, r.session_id), (RunState::Running, Some(b.clone())), "not the dead session's");
    // Now orch-b's live claim: a third session is refused, orch-b drives it.
    let c = SessionId::new("orch-c");
    let e = w.report("SHOP-142", None, RunState::Failed, None, None, Some(&c)).await.unwrap_err();
    assert_eq!(e.code, ErrorCode::Conflict);
    assert_eq!(w.release("SHOP-142", None, Some(&b)).await.unwrap().state, WorkState::Finished);
}

#[tokio::test]
async fn only_the_owner_session_reports_or_releases_its_claim() {
    if !has_git() {
        return;
    }
    let fx = Fx::new();
    session(&fx, "orch-a", "kelta-a6");
    let w = fx.service();
    let wt = run_worktree(&fx, "feat/ticket-142");
    let item = w.claim(&project(), samples::ticket_ref(), Some(wt), None, run("orch-a")).await.unwrap();

    let other = SessionId::new("orch-b");
    let e = w.report("SHOP-142", None, RunState::Failed, None, None, Some(&other)).await.unwrap_err();
    assert_eq!(e.code, ErrorCode::Conflict);
    let e = w.release("SHOP-142", None, None).await.unwrap_err();
    assert_eq!(e.code, ErrorCode::Conflict, "the global endpoint is not the owner");
    assert_eq!(run_state(&fx, &item.id).await, RunState::Running);

    w.report("SHOP-142", None, RunState::Blocked, None, None, Some(&orch_a())).await.unwrap();
    assert_eq!(w.release("SHOP-142", None, Some(&orch_a())).await.unwrap().state, WorkState::Finished);
}

#[tokio::test]
async fn claim_refuses_a_branch_git_would_not_accept() {
    if !has_git() {
        return;
    }
    let fx = Fx::new();
    let w = fx.service();
    let wt = run_worktree(&fx, "feat/ticket-142");
    for bad in ["--upload-pack=x", "a..b", "a b"] {
        let e = w
            .claim(&project(), samples::ticket_ref(), Some(wt.clone()), Some(bad.into()), run("orch-a"))
            .await
            .unwrap_err();
        assert_eq!(e.code, ErrorCode::InvalidArgument, "{bad}");
    }
    assert!(fx.store.list_items(None).await.unwrap().is_empty());
}

#[tokio::test]
async fn the_callers_claim_wins_over_an_unowned_one_and_orphans_are_open_to_all() {
    if !has_git() {
        return;
    }
    let fx = Fx::new();
    session(&fx, "orch-a", "kelta-a6");
    let w = fx.service();
    let (wt1, wt2) = (run_worktree(&fx, "feat/a"), run_worktree(&fx, "feat/b"));
    // Same key on two accounts: one claimed from the global endpoint, one by orch-a.
    let other = kelta_proto::tracker::TicketRef {
        account: kelta_proto::ids::AccountId::new("gh"),
        ..samples::ticket_ref()
    };
    let unowned = RunClaim { session_id: None, ..run("x") };
    let free = w.claim(&project(), samples::ticket_ref(), Some(wt1), None, unowned).await.unwrap();
    let mine = w.claim(&project(), other, Some(wt2), None, run("orch-a")).await.unwrap();
    let r = w.report("SHOP-142", None, RunState::Blocked, None, None, Some(&orch_a())).await.unwrap();
    assert_eq!(r.id, mine.id, "exact owner match first");
    assert_eq!(run_state(&fx, &free.id).await, RunState::Running);

    // orch-a exits: its claim is orphaned, and anyone may release it.
    w.report("SHOP-142", None, RunState::Running, None, None, Some(&orch_a())).await.unwrap();
    fx.core.exit_session(&orch_a(), 0);
    assert_eq!(shown(&w, &mine.id).await.state, RunState::Orphaned);
    w.release("SHOP-142", None, None).await.unwrap(); // the unowned one
    let done = w.release("SHOP-142", None, Some(&SessionId::new("orch-b"))).await.unwrap();
    assert_eq!((done.id, done.state), (mine.id, WorkState::Finished));
}

#[tokio::test]
async fn the_check_finishes_claims_no_live_run_will_release() {
    if !has_git() {
        return;
    }
    let fx = Fx::new();
    session(&fx, "orch-a", "kelta-a6");
    session(&fx, "orch-b", "kelta-b2");
    let w = fx.service();
    let p = project();
    let tref = |n: u32| TicketRef { key: format!("SHOP-{n}"), id: format!("10{n}"), ..samples::ticket_ref() };

    // Closed ticket, live owner: the run is still going, untouched.
    let wt_closed = run_worktree(&fx, "feat/closed");
    let closed = w.claim(&p, tref(142), Some(wt_closed.clone()), None, run("orch-a")).await.unwrap();
    fx.tracker.close(&tref(142)).await.unwrap();
    // No owner, worktree gone (ticket open).
    let wt_gone = run_worktree(&fx, "feat/gone");
    let unowned = RunClaim { session_id: None, ..run("x") };
    let gone = w.claim(&p, tref(141), Some(wt_gone.clone()), None, unowned).await.unwrap();
    git(&fx.repo, &["worktree", "remove", wt_gone.to_str().unwrap()]);
    // Orphaned on an open ticket: stays, reads orphaned.
    let wt_open = run_worktree(&fx, "feat/open");
    let open = w.claim(&p, tref(143), Some(wt_open), None, run("orch-b")).await.unwrap();
    fx.core.exit_session(&SessionId::new("orch-b"), 0);

    w.finish_stale_claims().await.unwrap();
    let state = |id: WorkItemId| {
        let store = fx.store.clone();
        async move { store.get_item(&id).await.unwrap().unwrap().state }
    };
    assert_eq!(state(closed.id.clone()).await, WorkState::Active, "live owner");
    assert_eq!(run_state(&fx, &closed.id).await, RunState::Running);
    assert_eq!(state(gone.id.clone()).await, WorkState::Finished, "worktree gone");
    assert_eq!(state(open.id.clone()).await, WorkState::Active, "open ticket");
    assert_eq!(shown(&w, &open.id).await.state, RunState::Orphaned);

    // The owner dies without releasing: its closed ticket's claim is finished, worktree and branch kept.
    fx.core.exit_session(&orch_a(), 0);
    w.finish_stale_claims().await.unwrap();
    assert_eq!(state(closed.id.clone()).await, WorkState::Finished);
    assert_eq!(run_state(&fx, &closed.id).await, RunState::Done);
    assert!(wt_closed.join("README.md").exists());
    assert!(git(&fx.repo, &["branch", "--list", "feat/closed"]).contains("feat/closed"));
    assert_eq!(state(open.id).await, WorkState::Active);
}

#[tokio::test]
async fn a_run_claude_adopted_by_its_item_opens_in_its_tab_and_release_leaves_it() {
    if !has_git() {
        return;
    }
    let fx = Fx::new();
    let s = run_claude(&fx);
    let w = fx.service();
    let item = w.claim(&project(), samples::ticket_ref(), None, None, run("run-s")).await.unwrap();
    let item = w.adopt_run(&item.id, &s).await.unwrap();
    assert_eq!(
        (item.session_ids.clone(), item.claude_uuid.as_deref()),
        (vec![s.id.clone()], Some("run-uuid"))
    );

    // The run's own workflow claims its worktree: same item, same Claude.
    let wt = run_worktree(&fx, "feat/ticket-142");
    let took = w.claim(&project(), samples::ticket_ref(), Some(wt), None, run("run-s")).await.unwrap();
    assert_eq!((&took.id, &took.session_ids), (&item.id, &vec![s.id.clone()]));

    w.resume(&item.id).await.unwrap();
    let calls = fx.core.calls();
    let spawns: Vec<_> = calls.iter().filter(|c| c.method == "session_spawn").collect();
    assert!(spawns.iter().all(|c| c.args["kind"]["type"] != "claude"), "the run's Claude is reused");
    assert!(!spawns.is_empty() && spawns.iter().all(|c| c.args["cwd"] == json!(took.worktree)));
    assert!(fx.core.opened().iter().any(|(_, r)| {
        r.content == (PaneContent::Terminal { session_id: s.id.clone() })
            && r.work_item_id.as_ref() == Some(&item.id)
    }));

    w.release("SHOP-142", None, Some(&s.id)).await.unwrap();
    let killed = fx.core.calls().iter().any(|c| c.method == "session_kill" && c.args["id"] == "run-s");
    assert!(!killed, "release_ticket comes from the run: Finish leaves it running");
}

#[tokio::test]
async fn an_adopted_claude_resumes_in_the_runs_cwd() {
    if !has_git() {
        return;
    }
    let fx = Fx::new();
    let s = run_claude(&fx);
    let w = fx.service();
    let wt = run_worktree(&fx, "feat/ticket-142");
    let item = w.claim(&project(), samples::ticket_ref(), Some(wt), None, run("run-s")).await.unwrap();
    w.adopt_run(&item.id, &s).await.unwrap();

    fx.core.exit_session(&s.id, 0);
    w.resume(&item.id).await.unwrap();
    let calls = fx.core.calls();
    let last = calls
        .iter()
        .rfind(|c| c.method == "session_spawn" && c.args["kind"]["type"] == "claude")
        .expect("Claude respawned");
    let args: Vec<String> = serde_json::from_value(last.args["args"].clone()).unwrap();
    assert_eq!(args[..2], ["--resume".to_owned(), "run-uuid".to_owned()]);
    assert_eq!(last.args["cwd"], json!(s.cwd), "the run's cwd, not the claimed worktree");
}

#[tokio::test]
async fn the_holder_session_re_claims_its_ticket_without_conflict() {
    if !has_git() {
        return;
    }
    let fx = Fx::new();
    session(&fx, "orch-a", "kelta-a6");
    let w = fx.service();
    let (t, p) = (samples::ticket_ref(), project());
    let bare = RunClaim { run_id: None, ..run("orch-a") };

    let a = w.claim(&p, t.clone(), None, None, bare.clone()).await.unwrap();
    // The second claim names its worktree: the same item, re-pointed.
    let wt = run_worktree(&fx, "feat/ticket-142");
    let b = w.claim(&p, t.clone(), Some(wt.clone()), None, bare).await.unwrap();
    assert_eq!(b.id, a.id);
    assert_eq!(b.worktree.canonicalize().unwrap(), wt.canonicalize().unwrap());
    assert_eq!(b.branch, "feat/ticket-142");
    // A run id set while the held one is missing is still the holder.
    let c = w.claim(&p, t, None, None, run("orch-a")).await.unwrap();
    assert_eq!(c.id, a.id);
    assert_eq!(fx.store.list_items(None).await.unwrap().len(), 1);
}

#[tokio::test]
async fn a_bare_re_claim_keeps_the_items_worktree() {
    if !has_git() {
        return;
    }
    let fx = Fx::new();
    session(&fx, "orch-a", "kelta-a6");
    let w = fx.service();
    let wt = run_worktree(&fx, "feat/ticket-142");
    let (t, p) = (samples::ticket_ref(), project());
    let a = w.claim(&p, t.clone(), Some(wt.clone()), None, run("orch-a")).await.unwrap();
    let b = w.claim(&p, t, None, None, run("orch-a")).await.unwrap();
    assert_eq!(b.id, a.id);
    assert_eq!(b.worktree.canonicalize().unwrap(), wt.canonicalize().unwrap());
    assert_eq!(b.branch, "feat/ticket-142");
}

#[tokio::test]
async fn an_items_own_claude_claims_its_ticket() {
    if !has_git() {
        return;
    }
    let fx = Fx::new();
    let w = fx.service();
    let mut t = samples::ticket_ref();
    t.key = "SHOP-141".into();
    let plan = w.plan(&project(), WorkSource::Ticket { ticket: t.clone() }).await.unwrap();
    let item = w.start(plan).await.unwrap();
    let claude = fx.spawned_of(|k| *k == SessionKind::Claude)[0].id.clone();

    let own = RunClaim { run_id: None, session_id: Some(claude.clone()), ..run("x") };
    let got = w.claim(&project(), t.clone(), None, None, own).await.unwrap();
    assert_eq!((&got.id, &got.worktree, &got.branch), (&item.id, &item.worktree, &item.branch));
    assert_eq!(got.run.unwrap().session_id, Some(claude));
    assert_eq!(fx.store.list_items(None).await.unwrap().len(), 1);
    // Another session still gets the Conflict.
    session(&fx, "orch-b", "kelta-b2");
    let e = w.claim(&project(), t, None, None, run("orch-b")).await.unwrap_err();
    assert_eq!(e.code, ErrorCode::Conflict);
}

#[tokio::test]
async fn hooks_follow_the_unfinished_item_of_a_reused_session() {
    if !has_git() {
        return;
    }
    let fx = Fx::new();
    let s = run_claude(&fx);
    let w = fx.service();
    let a = w.claim(&project(), samples::ticket_ref(), None, None, run("run-s")).await.unwrap();
    w.adopt_run(&a.id, &s).await.unwrap();
    w.release("SHOP-142", None, Some(&s.id)).await.unwrap();

    let next = TicketRef { key: "SHOP-143".into(), id: "10143".into(), ..samples::ticket_ref() };
    let wt = run_worktree(&fx, "feat/ticket-143");
    let b = w.claim(&project(), next, Some(wt), None, run("run-s")).await.unwrap();
    w.adopt_run(&b.id, &s).await.unwrap();
    assert_eq!(w.for_session(&s.id).await.unwrap().id, b.id);

    fx.core.publish(hook(&s.id, "SessionStart", json!({ "session_id": "after-clear", "source": "clear" })));
    wait_item(&fx, &b, |i| i.claude_uuid.as_deref() == Some("after-clear")).await;
}

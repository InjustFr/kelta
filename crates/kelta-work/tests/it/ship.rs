//! Ship preconditions and origin, merge detection (Merged + guarded Done move), Finish all merged.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use crate::common::{Fx, git, has_git, project};
use kelta_proto::api::{CoreApi, WorkStore};
use kelta_proto::codehost::{MergeMethod, PrDraft, Review};
use kelta_proto::error::{ErrorCode, KeltaError};
use kelta_proto::events::{BusEvent, bus};
use kelta_proto::model::{SessionKind, SessionStatus, ShipOrigin, WorkItem, WorkSource, WorkState};
use kelta_proto::samples;
use kelta_proto::settings::{PipelineEvent, StepDef, TransitionTarget, Trigger};
use kelta_proto::tracker::StatusCategory;
use kelta_work::WorkService;

macro_rules! need_git {
    () => {
        if !has_git() {
            eprintln!("skipping: git not found");
            return;
        }
    };
}

async fn started(fx: &Fx, w: &Arc<WorkService>, key: &str) -> WorkItem {
    let mut t = samples::ticket_ref();
    t.key = key.into();
    let plan = w.plan(&project(), WorkSource::Ticket { ticket: t }).await.unwrap();
    let item = w.start(plan).await.unwrap();
    assert_eq!(item.state, WorkState::Active, "{:?} {}", item.steps, fx.repo.display());
    item
}

fn commit(item: &WorkItem, file: &str) {
    std::fs::write(item.worktree.join(file), "x\n").unwrap();
    git(&item.worktree, &["add", file]);
    git(&item.worktree, &["commit", "-q", "-m", file]);
}

fn set_claude_status(fx: &Fx, item: &WorkItem, status: SessionStatus) {
    let mut s = fx
        .core
        .sessions()
        .into_iter()
        .find(|s| item.session_ids.contains(&s.id) && s.kind == SessionKind::Claude)
        .expect("claude session");
    s.status = status;
    fx.core.insert_session(s);
}

/// The item with a PR (as if shipped earlier).
async fn with_pr(fx: &Fx, item: &WorkItem, n: u64) -> Review {
    let mut review = samples::review();
    review.r#ref.number = n;
    review.url = format!("https://github.com/acme/shop-api/pull/{n}");
    let mut it = fx.store.get_item(&item.id).await.unwrap().unwrap();
    it.pr_url = Some(review.url.clone());
    it.state = WorkState::PrOpen;
    fx.store.put_item(&it).await.unwrap();
    review
}

async fn wait_state(fx: &Fx, item: &WorkItem, pred: impl Fn(&WorkState) -> bool) -> WorkItem {
    for _ in 0..300 {
        let it = fx.store.get_item(&item.id).await.unwrap().unwrap();
        if pred(&it.state) {
            return it;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("state never reached");
}

async fn step_of(fx: &Fx, item: &WorkItem) -> Option<String> {
    fx.store.ticket_step(item.ticket.as_ref().unwrap()).await.unwrap().map(|s| s.step_id)
}

async fn wait_step(fx: &Fx, item: &WorkItem, id: &str) {
    for _ in 0..300 {
        if step_of(fx, item).await.as_deref() == Some(id) {
            return;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("ticket never entered step {id}, at {:?}", step_of(fx, item).await);
}

fn step(id: &str, enter: Trigger, to: Option<&str>, comment: Option<&str>) -> StepDef {
    StepDef {
        id: id.into(),
        label: id.into(),
        enter: vec![enter],
        transition_to: to.map(|name| TransitionTarget::Name { name: name.into() }),
        comment: comment.map(Into::into),
    }
}

fn on(e: PipelineEvent) -> Trigger {
    Trigger::Event(e)
}

fn publish_end(fx: &Fx, name: &str, review: &Review) {
    fx.core.publish(BusEvent::new(name, serde_json::json!({ "review": review, "linked_tickets": [] })));
}

#[tokio::test]
async fn ship_refuses_without_commits_and_while_claude_works() {
    need_git!();
    let fx = Fx::new();
    let w = fx.service();
    let item = started(&fx, &w, "SHOP-141").await;
    let e = w.create_pr(&item.id, PrDraft::default(), ShipOrigin::Ui).await.unwrap_err();
    assert_eq!((e.code, e.message.as_str()), (ErrorCode::Conflict, "No commits ahead of main."));

    commit(&item, "a.txt");
    for busy in [SessionStatus::Working, SessionStatus::NeedsInput] {
        set_claude_status(&fx, &item, busy);
        let e = w.create_pr(&item.id, PrDraft::default(), ShipOrigin::Ui).await.unwrap_err();
        assert_eq!(e.message, "Claude is working in this worktree. Ship when it stops.");
    }
    assert!(fx.core.calls().iter().all(|c| c.args["name"] != "git push"), "nothing pushed");
}

#[tokio::test]
async fn create_pr_then_link_pr_is_a_no_op_also_when_the_repo_case_differs() {
    need_git!();
    use kelta_proto::api::CodeHost;
    let fx = Fx::new();
    let w = fx.service();
    let item = started(&fx, &w, "SHOP-141").await;
    commit(&item, "a.txt");
    set_claude_status(&fx, &item, SessionStatus::Working);
    let task = tokio::spawn({
        let (w, id) = (w.clone(), item.id.clone());
        async move { w.create_pr(&id, PrDraft::default(), ShipOrigin::Mcp).await }
    });
    let push = fx.wait_session(|s| s.name == "git push").await;
    git(&item.worktree, &["push", "-q", "-u", "origin", &item.branch]);
    fx.core.exit_session(&push, 0);
    let out = task.await.unwrap().unwrap();
    assert_eq!(out.state, WorkState::PrOpen);
    let binding = samples::project_info().repos[0].code_host.clone().unwrap();
    let pr = fx.host.find_for_branch(&binding.repo, &item.branch).await.unwrap().unwrap();
    assert_eq!(out.pr_url.as_deref(), Some(pr.url.as_str()));

    let t = item.ticket.clone().unwrap();
    let (same, linked) = w.link_ticket_pr(&project(), t.clone(), &pr).await.unwrap();
    assert!(!linked, "create_pr linked it already");
    assert_eq!(same.id, item.id);

    // Another URL and a repo name in other case: found by head branch in the bound repo.
    let mut upper = pr.clone();
    upper.r#ref.repo = pr.r#ref.repo.to_uppercase();
    upper.url = pr.url.replace(&pr.r#ref.repo, &upper.r#ref.repo);
    assert_ne!(upper.url, pr.url);
    let (same, linked) = w.link_ticket_pr(&project(), t, &upper).await.unwrap();
    assert!(!linked);
    assert_eq!(same.id, item.id);
    assert_eq!(w.list(None).await.unwrap().len(), 1, "no second item");
}

#[tokio::test]
async fn mcp_ship_holds_the_lock_and_names_it() {
    need_git!();
    let fx = Fx::new();
    let w = fx.service();
    let item = started(&fx, &w, "SHOP-141").await;
    commit(&item, "a.txt");
    // Claude ships while its own session is Working: allowed for origin=mcp.
    set_claude_status(&fx, &item, SessionStatus::Working);
    let task = tokio::spawn({
        let (w, id) = (w.clone(), item.id.clone());
        async move { w.create_pr(&id, PrDraft::default(), ShipOrigin::Mcp).await }
    });
    let push = fx.wait_session(|s| s.name == "git push").await;
    let e = w.create_pr(&item.id, PrDraft::default(), ShipOrigin::Ui).await.unwrap_err();
    assert_eq!((e.code, e.message.as_str()), (ErrorCode::Conflict, "Claude is shipping this item."));
    git(&item.worktree, &["push", "-q", "-u", "origin", &item.branch]);
    fx.core.exit_session(&push, 0);
    let out = task.await.unwrap().unwrap();
    assert_eq!(out.state, WorkState::PrOpen);

    // A UI ship of the same item (PR found for the branch).
    set_claude_status(&fx, &item, SessionStatus::Done);
    commit(&item, "b.txt");
    let mut it = fx.store.get_item(&item.id).await.unwrap().unwrap();
    it.state = WorkState::Active;
    fx.store.put_item(&it).await.unwrap();
    let task = tokio::spawn({
        let (w, id) = (w.clone(), item.id.clone());
        async move { w.create_pr(&id, PrDraft::default(), ShipOrigin::Ui).await }
    });
    let push = fx.wait_session(|s| s.name == "git push" && s.id != push).await;
    fx.core.exit_session(&push, 0);
    task.await.unwrap().unwrap();
}

#[tokio::test]
async fn pr_draft_prefills_the_ticket_template_and_the_draft_setting() {
    need_git!();
    let fx = Fx::new();
    fx.settings(|s| s.work.pr.draft = true);
    let w = fx.service();
    let item = started(&fx, &w, "SHOP-141").await;
    let d = w.pr_draft(&item.id).await.unwrap();
    assert_eq!(d.title.as_deref(), Some("SHOP-141: Add login form"));
    assert_eq!(d.draft, Some(true));
    assert!(d.body.is_some());
}

#[tokio::test]
async fn pr_draft_of_a_scratch_item_is_its_title_and_task() {
    need_git!();
    let fx = Fx::new();
    let w = fx.service();
    let task = "Speed up search\nCache the index between queries.";
    let src = WorkSource::Branch { name: String::new(), task: Some(task.into()), repo: None };
    let item = w.start(w.plan(&project(), src).await.unwrap()).await.unwrap();
    let d = w.pr_draft(&item.id).await.unwrap();
    assert_eq!(d.title.as_deref(), Some("Speed up search"));
    assert!(d.body.as_deref().unwrap().ends_with(task), "{:?}", d.body);
}

#[tokio::test]
async fn push_after_link_adds_the_ticket_key_to_the_pr_title_once() {
    need_git!();
    use kelta_proto::api::CodeHost;
    let fx = Fx::new();
    let w = fx.service();
    let item = started(&fx, &w, "SHOP-141").await;
    set_claude_status(&fx, &item, SessionStatus::Done);
    let binding = samples::project_info().repos[0].code_host.clone().unwrap();
    let pr = fx
        .host
        .create(&kelta_proto::codehost::PrCreate {
            repo: binding.repo.clone(),
            head: item.branch.clone(),
            base: "main".into(),
            title: "Speed up search".into(),
            body: String::new(),
            draft: false,
        })
        .await
        .unwrap();
    let mut it = fx.store.get_item(&item.id).await.unwrap().unwrap();
    (it.pr_url, it.state, it.pr_title_needs_key) = (Some(pr.url.clone()), WorkState::PrOpen, true);
    fx.store.put_item(&it).await.unwrap();

    let mut last = None;
    for file in ["a.txt", "b.txt"] {
        commit(&item, file);
        let task = tokio::spawn({
            let (w, id) = (w.clone(), item.id.clone());
            async move { w.push(&id, false).await }
        });
        let push = fx.wait_session(|s| s.name == "git push" && Some(&s.id) != last.as_ref()).await;
        last = Some(push.clone());
        git(&item.worktree, &["push", "-q", "-u", "origin", &item.branch]);
        fx.core.exit_session(&push, 0);
        let out = task.await.unwrap().unwrap();
        assert!(!out.pr_title_needs_key);
    }
    let found = fx.host.find_for_branch(&binding.repo, &item.branch).await.unwrap().unwrap();
    assert_eq!(found.title, "SHOP-141: Speed up search");
    let renames = fx.host.calls().into_iter().filter(|c| c.starts_with("update_title:")).count();
    assert_eq!(renames, 1, "the second push leaves the title alone");
}

#[tokio::test]
async fn merge_moves_the_ticket_to_done_on_the_last_open_pr_of_the_ticket() {
    need_git!();
    let fx = Fx::new();
    let w = fx.service();
    w.ensure_listener();
    let first = started(&fx, &w, "SHOP-141").await;
    let mut t = samples::ticket_ref();
    t.key = "SHOP-141".into();
    let plan =
        w.plan_with_repo(&project(), WorkSource::Ticket { ticket: t }, None, true, None).await.unwrap();
    let second = w.start(plan).await.unwrap();
    let (a, b) = (with_pr(&fx, &first, 91).await, with_pr(&fx, &second, 92).await);

    publish_end(&fx, bus::PR_MERGED, &a);
    let got = wait_state(&fx, &first, |s| matches!(s, WorkState::Merged { .. })).await;
    assert_eq!(got.state, WorkState::Merged { detail: None });
    assert_ne!(fx.tracker.ticket("SHOP-141").unwrap().ticket.status.name, "Done", "#92 is still open");
    assert_eq!(
        step_of(&fx, &first).await.as_deref(),
        Some("implementing"),
        "the first merge does not enter Done"
    );

    publish_end(&fx, bus::PR_MERGED, &b);
    wait_state(&fx, &second, |s| matches!(s, WorkState::Merged { .. })).await;
    assert_eq!(fx.tracker.ticket("SHOP-141").unwrap().ticket.status.name, "Done");
    assert_eq!(step_of(&fx, &first).await.as_deref(), Some("done"));
}

#[tokio::test]
async fn merge_moves_the_ticket_to_done_once_and_close_is_pr_closed() {
    need_git!();
    let fx = Fx::new();
    let w = fx.service();
    w.ensure_listener();
    let item = started(&fx, &w, "SHOP-141").await;
    let review = with_pr(&fx, &item, 91).await;
    publish_end(&fx, bus::PR_MERGED, &review);
    let got = wait_state(&fx, &item, |s| matches!(s, WorkState::Merged { .. })).await;
    assert_eq!(got.state, WorkState::Merged { detail: None });
    assert_eq!(fx.tracker.ticket("SHOP-141").unwrap().ticket.status.name, "Done");
    // Idempotent: a second pr.merged (check + live diff) moves nothing.
    publish_end(&fx, bus::PR_MERGED, &review);
    tokio::time::sleep(Duration::from_millis(100)).await;
    let moves = fx.tracker.calls().iter().filter(|c| c.starts_with("transition:SHOP-141:t5")).count();
    assert_eq!(moves, 1);

    let other = started(&fx, &w, "SHOP-143").await;
    let review = with_pr(&fx, &other, 92).await;
    let before = fx.tracker.ticket("SHOP-143").unwrap().ticket.status;
    assert!(w.arm_merge(&other.id, MergeMethod::Squash).await.unwrap().auto_finish);
    publish_end(&fx, bus::PR_CLOSED, &review);
    let closed = wait_state(&fx, &other, |s| *s == WorkState::PrClosed).await;
    assert!(!closed.auto_finish, "a closed PR never finishes: disarmed");
    assert_eq!(fx.tracker.ticket("SHOP-143").unwrap().ticket.status, before, "close moves nothing");
}

#[tokio::test]
async fn ambiguous_done_is_never_auto_picked() {
    need_git!();
    let fx = Fx::new();
    // The only Done transition needs fields (resolution): not unambiguous.
    fx.tracker.require_fields("t5");
    let w = fx.service();
    w.ensure_listener();
    let item = started(&fx, &w, "SHOP-141").await;
    let review = with_pr(&fx, &item, 93).await;
    assert!(w.arm_merge(&item.id, MergeMethod::Squash).await.unwrap().auto_finish);
    publish_end(&fx, bus::PR_MERGED, &review);
    let got = wait_state(&fx, &item, |s| matches!(s, WorkState::Merged { .. })).await;
    assert_eq!(got.state, WorkState::Merged { detail: Some("choose Done status".into()) });
    assert!(!got.auto_finish, "Finish waits for the dialog: disarmed");
    let texts: Vec<String> = fx.core.toasts().into_iter().map(|t| t.text).collect();
    assert!(
        texts.iter().any(|t| t == "SHOP-141 merged · auto-finish skipped: choose Done status"),
        "{texts:?}"
    );
    assert_ne!(fx.tracker.ticket("SHOP-141").unwrap().ticket.status.name, "Done");
    assert!(fx.tracker.calls().iter().all(|c| !c.starts_with("transition:SHOP-141:t5")));
}

#[tokio::test]
async fn finish_merged_finishes_listed_clean_items_and_skips_dirty_ones() {
    need_git!();
    let fx = Fx::new();
    let w = fx.service();
    w.ensure_listener();
    let clean = started(&fx, &w, "SHOP-141").await;
    let dirty = started(&fx, &w, "SHOP-143").await;
    // Merged but not listed by the dialog (merged while it was open): left alone.
    let unlisted = started(&fx, &w, "SHOP-142").await;
    for (it, n) in [(&clean, 94), (&dirty, 95), (&unlisted, 96)] {
        commit(it, "a.txt");
        git(&it.worktree, &["push", "-q", "-u", "origin", &it.branch]);
        let review = with_pr(&fx, it, n).await;
        publish_end(&fx, bus::PR_MERGED, &review);
        wait_state(&fx, it, |s| matches!(s, WorkState::Merged { .. })).await;
    }
    std::fs::write(dirty.worktree.join("wip.txt"), "wip\n").unwrap();

    let report = w.finish_merged(&[clean.id.clone(), dirty.id.clone()]).await.unwrap();
    assert_eq!(report.finished.iter().map(|w| &w.id).collect::<Vec<_>>(), vec![&clean.id]);
    assert_eq!(report.skipped.len(), 1);
    assert_eq!(report.skipped[0].id, dirty.id);
    assert!(report.skipped[0].reason.contains("uncommitted"), "{:?}", report.skipped);
    assert!(!clean.worktree.exists() && dirty.worktree.exists() && unlisted.worktree.exists());
    assert!(git(&fx.repo, &["branch", "--list", &clean.branch]).is_empty(), "merged branch deleted");
    for it in [&dirty, &unlisted] {
        assert_eq!(
            fx.store.get_item(&it.id).await.unwrap().unwrap().state,
            WorkState::Merged { detail: None }
        );
    }
}

#[tokio::test]
async fn armed_merge_finishes_once_and_keeps_a_dirty_worktree_flagged() {
    need_git!();
    let fx = Fx::new();
    let w = fx.service();
    let clean = started(&fx, &w, "SHOP-141").await;
    let dirty = started(&fx, &w, "SHOP-143").await;
    let mut reviews = Vec::new();
    for (it, n) in [(&clean, 97), (&dirty, 98)] {
        commit(it, "a.txt");
        git(&it.worktree, &["push", "-q", "-u", "origin", &it.branch]);
        reviews.push(with_pr(&fx, it, n).await);
    }
    // A refusal surfaces the host's message and arms nothing.
    fx.host.fail_next(KeltaError::upstream("auto-merge is not allowed for this repository"));
    let e = w.arm_merge(&clean.id, MergeMethod::Squash).await.unwrap_err();
    assert!(e.message.contains("auto-merge is not allowed"));
    assert!(!fx.store.get_item(&clean.id).await.unwrap().unwrap().auto_finish);
    for it in [&clean, &dirty] {
        assert!(w.arm_merge(&it.id, MergeMethod::Squash).await.unwrap().auto_finish);
    }
    assert!(!w.disarm_merge(&dirty.id).await.unwrap().auto_finish);
    assert!(w.arm_merge(&dirty.id, MergeMethod::Merge).await.unwrap().auto_finish);
    let calls = fx.host.calls();
    assert!(calls.iter().any(|c| c == "arm_auto_merge:97:Squash"), "{calls:?}");
    assert!(calls.iter().any(|c| c == "disarm_auto_merge:98"), "{calls:?}");

    std::fs::write(dirty.worktree.join("wip.txt"), "wip\n").unwrap();
    for r in &reviews {
        publish_end(&fx, bus::PR_MERGED, r);
    }
    wait_state(&fx, &clean, |s| *s == WorkState::Finished).await;
    assert!(!clean.worktree.exists());
    assert!(git(&fx.repo, &["branch", "--list", &clean.branch]).is_empty(), "merged branch deleted");
    assert_eq!(fx.tracker.ticket("SHOP-141").unwrap().ticket.status.name, "Done");
    let flagged = wait_state(&fx, &dirty, |s| matches!(s, WorkState::Merged { detail: Some(_) })).await;
    assert!(dirty.worktree.join("wip.txt").exists(), "a dirty worktree is never removed");
    assert!(!flagged.auto_finish);
    let WorkState::Merged { detail: Some(why) } = &flagged.state else { unreachable!() };
    assert!(why.starts_with("auto-finish skipped:") && why.contains("uncommitted"), "{why}");
    let texts: Vec<String> = fx.core.toasts().into_iter().map(|t| t.text).collect();
    assert!(texts.iter().any(|t| t == "SHOP-141 merged · worktree removed · ticket → Done"), "{texts:?}");

    // Idempotent across polls and a restart: nothing runs again.
    for r in &reviews {
        publish_end(&fx, bus::PR_MERGED, r);
    }
    w.startup().await.unwrap();
    tokio::time::sleep(Duration::from_millis(100)).await;
    let moves = fx.tracker.calls().iter().filter(|c| c.starts_with("transition:SHOP-141:t5")).count();
    assert_eq!(moves, 1);
    assert!(dirty.worktree.join("wip.txt").exists());
    assert_eq!(fx.store.get_item(&dirty.id).await.unwrap().unwrap().state, flagged.state);
}

#[tokio::test]
async fn startup_finishes_an_armed_item_merged_before_a_crash() {
    need_git!();
    let fx = Fx::new();
    let w = fx.service();
    let item = started(&fx, &w, "SHOP-141").await;
    commit(&item, "a.txt");
    git(&item.worktree, &["push", "-q", "-u", "origin", &item.branch]);
    with_pr(&fx, &item, 99).await;
    let mut it = fx.store.get_item(&item.id).await.unwrap().unwrap();
    it.state = WorkState::Merged { detail: None };
    it.auto_finish = true;
    fx.store.put_item(&it).await.unwrap();
    w.startup().await.unwrap();
    assert_eq!(fx.store.get_item(&item.id).await.unwrap().unwrap().state, WorkState::Finished);
    assert!(!item.worktree.exists());
}

#[tokio::test]
async fn finish_merged_keeps_a_branch_with_unpushed_commits_when_head_moved_away() {
    need_git!();
    let fx = Fx::new();
    let w = fx.service();
    let item = started(&fx, &w, "SHOP-141").await;
    commit(&item, "a.txt");
    with_pr(&fx, &item, 99).await;
    let mut it = fx.store.get_item(&item.id).await.unwrap().unwrap();
    it.state = WorkState::Merged { detail: None };
    fx.store.put_item(&it).await.unwrap();
    // Follow-up never pushed, then HEAD moves to a clean branch: HEAD alone looks safe to delete.
    commit(&item, "b.txt");
    git(&item.worktree, &["checkout", "-q", "-b", "tmp", "origin/main"]);
    let opts = kelta_proto::model::FinishOpts {
        remove_worktree: true,
        delete_branch: true,
        force: false,
        transition_to: None,
    };
    assert_eq!(w.finish(&item.id, opts).await.unwrap().state, WorkState::Finished);
    assert!(!item.worktree.exists());
    assert!(!git(&fx.repo, &["branch", "--list", &item.branch]).is_empty(), "unpushed branch kept");
    let texts: Vec<String> = fx.core.toasts().into_iter().map(|t| t.text).collect();
    assert!(texts.iter().any(|t| t.starts_with(&format!("Branch {} kept", item.branch))), "{texts:?}");
}

#[tokio::test]
async fn ticket_step_stays_pr_when_claude_works_after_an_mcp_ship() {
    need_git!();
    let fx = Fx::new();
    let w = fx.service();
    w.ensure_listener();
    let item = started(&fx, &w, "SHOP-141").await;
    assert_eq!(step_of(&fx, &item).await.as_deref(), Some("implementing"));
    commit(&item, "a.txt");
    set_claude_status(&fx, &item, SessionStatus::Working);
    let task = tokio::spawn({
        let (w, id) = (w.clone(), item.id.clone());
        async move { w.create_pr(&id, PrDraft::default(), ShipOrigin::Mcp).await }
    });
    let push = fx.wait_session(|s| s.name == "git push").await;
    git(&item.worktree, &["push", "-q", "-u", "origin", &item.branch]);
    fx.core.exit_session(&push, 0);
    task.await.unwrap().unwrap();
    assert_eq!(step_of(&fx, &item).await.as_deref(), Some("pr"));
    assert!(fx.core.published().iter().any(|e| e.name == bus::TICKET_STEP));

    let claude = fx.core.sessions().into_iter().find(|s| s.kind == SessionKind::Claude).unwrap().id;
    let payload = serde_json::json!({ "hook_event_name": "UserPromptSubmit" });
    fx.core.publish(
        BusEvent::new(
            bus::CLAUDE_HOOK,
            serde_json::json!({ "event": "UserPromptSubmit", "payload": payload }),
        )
        .with_session(claude),
    );
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert_eq!(step_of(&fx, &item).await.as_deref(), Some("pr"), "never back to implementing");
}

#[tokio::test]
async fn pr_ready_enters_review_and_runs_its_transition_once() {
    need_git!();
    let fx = Fx::new();
    fx.settings(|s| {
        s.work.pipeline = vec![
            step("implementing", on(PipelineEvent::WorkStarted), None, None),
            step("pr", on(PipelineEvent::PrOpened), None, None),
            step("review", on(PipelineEvent::PrReady), Some("In Review"), None),
            step("done", on(PipelineEvent::Merged), None, None),
        ];
    });
    let w = fx.service();
    w.ensure_listener();
    let item = started(&fx, &w, "SHOP-141").await;
    let mut review = with_pr(&fx, &item, 91).await;
    review.draft = false;
    let updated =
        || BusEvent::new(bus::PR_UPDATED, serde_json::json!({ "review": review, "changes": ["draft"] }));
    fx.core.publish(updated());
    wait_step(&fx, &item, "review").await;
    let moves = || fx.tracker.calls().iter().filter(|c| c.starts_with("transition:SHOP-141:t10")).count();
    assert_eq!(moves(), 1);
    fx.core.publish(updated());
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert_eq!((moves(), step_of(&fx, &item).await.as_deref()), (1, Some("review")));
}

#[tokio::test]
async fn a_status_trigger_enters_po_review_without_a_transition() {
    need_git!();
    let fx = Fx::new();
    let po = Trigger::Status { status: TransitionTarget::Name { name: "PO Review".into() } };
    fx.settings(|s| {
        s.work.pipeline = vec![
            step("implementing", on(PipelineEvent::WorkStarted), None, None),
            step("po", po, Some("Done"), Some("Ready for PO")),
        ];
    });
    let w = fx.service();
    let item = started(&fx, &w, "SHOP-141").await;
    let mut tk = fx.tracker.ticket("SHOP-141").unwrap().ticket;
    tk.status = samples::status("12", "PO Review", StatusCategory::InReview);
    w.ticket_statuses(std::slice::from_ref(&tk)).await;
    assert_eq!(step_of(&fx, &item).await.as_deref(), Some("po"));
    let comments = || fx.tracker.ticket("SHOP-141").unwrap().comments.len();
    assert_eq!(comments(), 1);
    assert!(comments_have(&fx, "Ready for PO"));
    assert!(fx.tracker.calls().iter().all(|c| !c.starts_with("transition:")), "the tracker is already there");
    w.ticket_statuses(std::slice::from_ref(&tk)).await;
    assert_eq!(comments(), 1);
}

#[tokio::test]
async fn a_manual_move_runs_the_entered_step_forward_and_nothing_back() {
    need_git!();
    let fx = Fx::new();
    fx.settings(|s| {
        s.work.pipeline = vec![
            step("implementing", on(PipelineEvent::WorkStarted), None, None),
            step("pr", on(PipelineEvent::PrOpened), None, None),
            step("po", on(PipelineEvent::Approved), Some("In Review"), Some("Ready for PO")),
            step("done", on(PipelineEvent::Merged), None, None),
        ];
    });
    let w = fx.service();
    let item = started(&fx, &w, "SHOP-141").await;
    let t = item.ticket.clone().unwrap();
    assert_eq!(w.move_step(&t, None).await.unwrap().step_id, "pr", "Next step");
    let row = w.move_step(&t, Some("po")).await.unwrap();
    assert_eq!((row.step_id.as_str(), row.by.as_str()), ("po", "manual"));
    let moves = || fx.tracker.calls().iter().filter(|c| c.starts_with("transition:SHOP-141:t10")).count();
    assert_eq!(moves(), 1);
    assert!(comments_have(&fx, "Ready for PO"));

    let calls = fx.tracker.calls().len();
    let published = || fx.core.published().iter().filter(|e| e.name == bus::TICKET_STEP).count();
    let before = published();
    w.move_step(&t, Some("implementing")).await.unwrap();
    assert_eq!(step_of(&fx, &item).await.as_deref(), Some("implementing"));
    assert_eq!(fx.tracker.calls().len(), calls, "a backward move runs no side effect");
    assert_eq!(published(), before + 1);

    let err = w.move_step(&t, Some("nope")).await.unwrap_err();
    assert_eq!(err.code, ErrorCode::InvalidArgument);
    assert!(err.message.contains("unknown step `nope`"), "{}", err.message);
    w.move_step(&t, Some("done")).await.unwrap();
    assert_eq!(w.move_step(&t, None).await.unwrap_err().code, ErrorCode::InvalidArgument, "past the last");
    let mut other = t.clone();
    other.key = "SHOP-999".into();
    assert_eq!(w.move_step(&other, None).await.unwrap_err().code, ErrorCode::NotFound);
    let mut it = fx.store.get_item(&item.id).await.unwrap().unwrap();
    it.state = WorkState::Finished;
    fx.store.put_item(&it).await.unwrap();
    assert_eq!(
        w.move_step(&t, None).await.unwrap_err().code,
        ErrorCode::NotFound,
        "a finished item is no work, as in the UI"
    );
}

fn comments_have(fx: &Fx, text: &str) -> bool {
    fx.tracker.ticket("SHOP-141").unwrap().comments.iter().any(|c| c.body_html.contains(text))
}

/// A PR for the ticket, not written on any item: `link_pr` joins it.
fn pr_review(n: u64) -> Review {
    let mut review = samples::review();
    review.r#ref.number = n;
    review.url = format!("https://github.com/acme/shop-api/pull/{n}");
    review
}

fn transitions(fx: &Fx, id: &str) -> usize {
    let p = format!("transition:SHOP-141:{id}");
    fx.tracker.calls().iter().filter(|c| c.starts_with(&p)).count()
}

fn started_comments(fx: &Fx) -> usize {
    fx.tracker.ticket("SHOP-141").unwrap().comments.iter().filter(|c| c.body_html.contains("Started")).count()
}

async fn another(fx: &Fx, w: &Arc<WorkService>) -> WorkItem {
    let mut t = samples::ticket_ref();
    t.key = "SHOP-141".into();
    let plan =
        w.plan_with_repo(&project(), WorkSource::Ticket { ticket: t }, None, true, None).await.unwrap();
    let item = w.start(plan).await.unwrap();
    assert_eq!(item.state, WorkState::Active, "{:?} {}", item.steps, fx.repo.display());
    item
}

fn start_pipeline(fx: &Fx) {
    fx.settings(|s| {
        s.work.pipeline = vec![
            step("implementing", on(PipelineEvent::WorkStarted), Some("In Progress"), Some("Started")),
            step("pr", on(PipelineEvent::PrOpened), Some("In Review"), None),
            step("done", on(PipelineEvent::Merged), Some("Done"), None),
        ];
    });
}

#[tokio::test]
async fn replacing_the_pipeline_never_moves_a_ticket_back() {
    need_git!();
    let fx = Fx::new();
    fx.settings(|s| {
        s.work.pipeline = vec![
            step("implementing", on(PipelineEvent::WorkStarted), None, None),
            step("pr", on(PipelineEvent::PrOpened), None, None),
            step("review", on(PipelineEvent::PrReady), None, None),
            step("done", on(PipelineEvent::Merged), None, None),
        ];
    });
    let w = fx.service();
    w.ensure_listener();
    let item = started(&fx, &w, "SHOP-141").await;
    let mut review = pr_review(91);
    w.link_pr(&item.id, &review).await.unwrap();
    wait_step(&fx, &item, "pr").await;
    review.draft = false;
    let updated = |r: &Review, c: &str| {
        BusEvent::new(bus::PR_UPDATED, serde_json::json!({ "review": r, "changes": [c] }))
    };
    fx.core.publish(updated(&review, "draft"));
    wait_step(&fx, &item, "review").await;

    fx.settings(|s| {
        s.work.pipeline = vec![
            step("start", on(PipelineEvent::WorkStarted), None, None),
            step("asked", on(PipelineEvent::ReviewRequested), None, None),
            step("shipped", on(PipelineEvent::Merged), None, None),
        ];
    });
    review.waiting_on = vec!["me".into()];
    fx.core.publish(updated(&review, "reviewers"));
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert_eq!(step_of(&fx, &item).await.as_deref(), Some("shipped"), "clamped forward, never `asked`");
}

#[tokio::test]
async fn done_then_another_pr_runs_the_start_pr_and_done_transitions_again() {
    need_git!();
    let fx = Fx::new();
    start_pipeline(&fx);
    let w = fx.service();
    w.ensure_listener();
    let first = started(&fx, &w, "SHOP-141").await;
    let a = pr_review(91);
    w.link_pr(&first.id, &a).await.unwrap();
    publish_end(&fx, bus::PR_MERGED, &a);
    wait_state(&fx, &first, |s| matches!(s, WorkState::Merged { .. })).await;
    wait_step(&fx, &first, "done").await;

    let second = another(&fx, &w).await;
    assert_eq!(step_of(&fx, &second).await.as_deref(), Some("implementing"), "the flow restarted");
    let b = pr_review(92);
    w.link_pr(&second.id, &b).await.unwrap();
    wait_step(&fx, &second, "pr").await;
    publish_end(&fx, bus::PR_MERGED, &b);
    wait_state(&fx, &second, |s| matches!(s, WorkState::Merged { .. })).await;
    wait_step(&fx, &second, "done").await;
    assert_eq!((transitions(&fx, "t3"), transitions(&fx, "t10"), transitions(&fx, "t5")), (2, 2, 2));
    assert_eq!(started_comments(&fx), 2);
}

#[tokio::test]
async fn a_sibling_start_at_the_start_step_posts_the_comment_once() {
    need_git!();
    let fx = Fx::new();
    start_pipeline(&fx);
    let w = fx.service();
    started(&fx, &w, "SHOP-141").await;
    let second = another(&fx, &w).await;
    assert_eq!(step_of(&fx, &second).await.as_deref(), Some("implementing"));
    assert_eq!((transitions(&fx, "t3"), started_comments(&fx)), (1, 1));
}

#[tokio::test]
async fn sibling_starts_at_once_post_the_start_comment_once() {
    need_git!();
    let fx = Fx::new();
    start_pipeline(&fx);
    let w = fx.service();
    let mut t = samples::ticket_ref();
    t.key = "SHOP-141".into();
    let mut a = w.plan(&project(), WorkSource::Ticket { ticket: t.clone() }).await.unwrap();
    let mut b =
        w.plan_with_repo(&project(), WorkSource::Ticket { ticket: t }, None, true, None).await.unwrap();
    a.another = true;
    b.branch = format!("{}-2", a.branch);
    b.worktree_path = PathBuf::from(format!("{}-2", a.worktree_path.display()));
    let (x, y) = tokio::join!(w.start(a), w.start(b));
    assert_eq!((x.unwrap().state, y.unwrap().state), (WorkState::Active, WorkState::Active));
    assert_eq!((transitions(&fx, "t3"), started_comments(&fx)), (1, 1));
}

#[tokio::test]
async fn a_sibling_start_never_moves_the_ticket_back() {
    need_git!();
    let fx = Fx::new();
    start_pipeline(&fx);
    let w = fx.service();
    w.ensure_listener();
    let first = started(&fx, &w, "SHOP-141").await;
    w.link_pr(&first.id, &pr_review(91)).await.unwrap();
    wait_step(&fx, &first, "pr").await;
    let second = another(&fx, &w).await;
    assert_eq!(step_of(&fx, &second).await.as_deref(), Some("pr"));
    assert_eq!((transitions(&fx, "t3"), started_comments(&fx)), (1, 1));
}

#[tokio::test]
async fn resume_after_the_pr_opened_skips_the_start_effects() {
    need_git!();
    let fx = Fx::new();
    start_pipeline(&fx);
    let w = fx.service();
    let mut t = samples::ticket_ref();
    t.key = "SHOP-141".into();
    let plan = w.plan(&project(), WorkSource::Ticket { ticket: t.clone() }).await.unwrap();
    w.set_crash_after(Some("claude"));
    w.start(plan).await.unwrap_err();
    let id = fx.store.list_items(None).await.unwrap().remove(0).id;
    fx.store
        .put_ticket_step(&kelta_proto::model::TicketStep {
            ticket: t,
            source_id: None,
            step_id: "pr".into(),
            entered_at: kelta_proto::now_rfc3339(),
            by: serde_json::to_string(&on(PipelineEvent::PrOpened)).unwrap(),
        })
        .await
        .unwrap();
    let done = fx.service().resume(&id).await.unwrap();
    assert_eq!(done.state, WorkState::Active, "{:?}", done.steps);
    assert_eq!(step_of(&fx, &done).await.as_deref(), Some("pr"));
    assert_eq!((transitions(&fx, "t3"), started_comments(&fx)), (0, 0));
}

#[tokio::test]
async fn a_manual_move_back_stays_across_an_unchanged_status_poll() {
    need_git!();
    let fx = Fx::new();
    let po = Trigger::Status { status: TransitionTarget::Name { name: "PO Review".into() } };
    fx.settings(|s| {
        s.work.pipeline = vec![
            step("implementing", on(PipelineEvent::WorkStarted), None, None),
            step("po", po, None, None),
        ];
    });
    let w = fx.service();
    let item = started(&fx, &w, "SHOP-141").await;
    let mut tk = fx.tracker.ticket("SHOP-141").unwrap().ticket;
    tk.status = samples::status("12", "PO Review", StatusCategory::InReview);
    w.ticket_statuses(std::slice::from_ref(&tk)).await;
    assert_eq!(step_of(&fx, &item).await.as_deref(), Some("po"));
    w.move_step(item.ticket.as_ref().unwrap(), Some("implementing")).await.unwrap();
    w.ticket_statuses(std::slice::from_ref(&tk)).await;
    assert_eq!(step_of(&fx, &item).await.as_deref(), Some("implementing"), "same status: no signal");
    let mut other = tk.clone();
    other.status = samples::status("10", "In Review", StatusCategory::InReview);
    w.ticket_statuses(std::slice::from_ref(&other)).await;
    assert_eq!(step_of(&fx, &item).await.as_deref(), Some("implementing"));
    w.ticket_statuses(std::slice::from_ref(&tk)).await;
    assert_eq!(step_of(&fx, &item).await.as_deref(), Some("po"), "a changed status enters it");
}

#[tokio::test]
async fn a_failed_manual_move_is_reported_and_retried() {
    need_git!();
    let fx = Fx::new();
    fx.settings(|s| {
        s.work.pipeline = vec![
            step("implementing", on(PipelineEvent::WorkStarted), None, None),
            step("po", on(PipelineEvent::Approved), Some("In Review"), None),
        ];
    });
    let w = fx.service();
    let item = started(&fx, &w, "SHOP-141").await;
    let t = item.ticket.clone().unwrap();
    fx.tracker.require_fields("t10");
    let e = w.move_step(&t, Some("po")).await.unwrap_err();
    assert!(e.message.contains("transition failed"), "{}", e.message);
    assert_eq!(step_of(&fx, &item).await.as_deref(), Some("po"), "stored anyway");
    assert!(w.move_step(&t, Some("po")).await.is_err());
    assert_eq!(transitions(&fx, "t10"), 2, "retried, no early Ok");
}

#[tokio::test]
async fn a_retry_at_the_same_step_keeps_the_trigger_that_entered_it() {
    need_git!();
    let fx = Fx::new();
    fx.settings(|s| {
        s.work.pipeline = vec![
            step("implementing", on(PipelineEvent::WorkStarted), None, None),
            step("pr", on(PipelineEvent::PrOpened), Some("In Review"), None),
        ];
    });
    let w = fx.service();
    w.ensure_listener();
    let item = started(&fx, &w, "SHOP-141").await;
    let t = item.ticket.clone().unwrap();
    fx.tracker.require_fields("t10");
    w.link_pr(&item.id, &pr_review(91)).await.unwrap();
    wait_step(&fx, &item, "pr").await;
    assert!(w.move_step(&t, Some("pr")).await.is_err());
    assert_eq!(transitions(&fx, "t10"), 2, "retried");
    let by = fx.store.ticket_step(&t).await.unwrap().unwrap().by;
    assert_eq!(by, serde_json::to_string(&on(PipelineEvent::PrOpened)).unwrap());
}

#[tokio::test]
async fn a_step_entered_by_its_status_clears_its_failed_transition() {
    need_git!();
    let fx = Fx::new();
    let po = Trigger::Status { status: TransitionTarget::Name { name: "PO Review".into() } };
    fx.settings(|s| {
        s.work.pipeline = vec![
            step("implementing", on(PipelineEvent::WorkStarted), None, None),
            step("po", po, Some("In Review"), None),
        ];
    });
    let w = fx.service();
    let item = started(&fx, &w, "SHOP-141").await;
    let t = item.ticket.clone().unwrap();
    fx.tracker.require_fields("t10");
    assert!(w.move_step(&t, Some("po")).await.is_err());
    w.move_step(&t, Some("implementing")).await.unwrap();
    let mut tk = fx.tracker.ticket("SHOP-141").unwrap().ticket;
    tk.status = samples::status("12", "PO Review", StatusCategory::InReview);
    w.ticket_statuses(std::slice::from_ref(&tk)).await;
    assert_eq!(step_of(&fx, &item).await.as_deref(), Some("po"));
    assert!(w.move_step(&t, Some("po")).await.is_ok(), "nothing left to retry");
    assert_eq!(transitions(&fx, "t10"), 1, "2 with the marker left behind");
}

#[tokio::test]
async fn next_step_without_a_stored_step_skips_the_start_effects() {
    need_git!();
    let fx = Fx::new();
    fx.settings(|s| {
        s.work.pipeline = vec![
            step("implementing", on(PipelineEvent::WorkStarted), Some("In Progress"), Some("Started")),
            step("pr", on(PipelineEvent::PrOpened), None, None),
        ];
    });
    let w = fx.service();
    let scratch =
        WorkSource::Branch { name: String::new(), task: Some("Explore caching".into()), repo: None };
    let a = w.start(w.plan(&project(), scratch).await.unwrap()).await.unwrap();
    let mut t = samples::ticket_ref();
    t.key = "SHOP-141".into();
    w.link(&a.id, t.clone(), false).await.unwrap();
    assert_eq!(w.move_step(&t, None).await.unwrap().step_id, "implementing");
    assert_eq!((transitions(&fx, "t3"), started_comments(&fx)), (0, 0));
}

#[tokio::test]
async fn a_retry_at_the_start_step_still_runs_its_effects_once() {
    need_git!();
    let fx = Fx::new();
    start_pipeline(&fx);
    let w = fx.service();
    let mut t = samples::ticket_ref();
    t.key = "SHOP-141".into();
    let mut plan = w.plan(&project(), WorkSource::Ticket { ticket: t }).await.unwrap();
    plan.side_effects.comment = Some("Started".into());
    plan.side_effects.transition_to = Some(TransitionTarget::Name { name: "Nowhere".into() });
    let item = w.start(plan).await.unwrap();
    assert_eq!(step_of(&fx, &item).await.as_deref(), Some("implementing"), "the start row is stored");
    w.retry_step(&item.id, "tracker_side_effects").await.unwrap();
    let c = fx.tracker.calls();
    let count = |p: &str| c.iter().filter(|x| x.starts_with(p)).count();
    assert_eq!((count("transitions:"), count("comment:")), (2, 1), "{c:?}");
}

#[tokio::test]
async fn moving_to_the_current_step_from_a_sibling_item_does_not_rerun_it() {
    need_git!();
    let fx = Fx::new();
    start_pipeline(&fx);
    let w = fx.service();
    w.ensure_listener();
    let first = started(&fx, &w, "SHOP-141").await;
    w.link_pr(&first.id, &pr_review(91)).await.unwrap();
    wait_step(&fx, &first, "pr").await;
    let second = another(&fx, &w).await;
    w.move_step(second.ticket.as_ref().unwrap(), Some("pr")).await.unwrap();
    assert_eq!(transitions(&fx, "t10"), 1, "no retry without a failure");
}

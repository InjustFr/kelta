//! Ship preconditions and origin, merge detection (Merged + guarded Done move), Finish all merged.

use std::sync::Arc;
use std::time::Duration;

use crate::common::{Fx, git, has_git, project};
use kelta_proto::api::{CoreApi, WorkStore};
use kelta_proto::codehost::{MergeMethod, PrDraft, Review};
use kelta_proto::error::{ErrorCode, KeltaError};
use kelta_proto::events::{BusEvent, bus};
use kelta_proto::model::{SessionKind, SessionStatus, ShipOrigin, WorkItem, WorkSource, WorkState};
use kelta_proto::samples;
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

//! Finish (dirty/unpushed refusal, force), create PR, status, git timeouts / non-interactivity.

mod common;

use std::time::{Duration, Instant};

use common::{Fx, git, has_git, project};
use kelta_proto::codehost::PrDraft;
use kelta_proto::error::ErrorCode;
use kelta_proto::model::{FinishOpts, SessionKind, WorkItem, WorkSource, WorkState};
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

async fn started(fx: &Fx, key: &str) -> (std::sync::Arc<WorkService>, WorkItem) {
    let w = fx.service();
    let mut t = samples::ticket_ref();
    t.key = key.into();
    let plan = w.plan(&project(), WorkSource::Ticket { ticket: t }).await.unwrap();
    let item = w.start(plan).await.unwrap();
    assert_eq!(item.state, WorkState::Active, "{:?}", item.steps);
    (w, item)
}

fn opts(force: bool) -> FinishOpts {
    FinishOpts { remove_worktree: true, delete_branch: true, force, transition_to: None }
}

#[tokio::test]
async fn finish_refuses_dirty_then_unpushed_then_force_removes() {
    need_git!();
    let fx = Fx::new();
    // A non-ignored include file is copied but must not count as the user's work.
    std::fs::write(fx.repo.join("local.cfg"), "x=1\n").unwrap();
    std::fs::write(fx.repo.join(".worktreeinclude"), "local.cfg\n").unwrap();
    let (w, item) = started(&fx, "SHOP-141").await;
    assert!(item.worktree.join("local.cfg").exists());

    std::fs::write(item.worktree.join("new.txt"), "wip\n").unwrap();
    let e = w.finish(&item.id, opts(false)).await.unwrap_err();
    assert_eq!(e.code, ErrorCode::Dirty);
    let detail = e.detail.clone().unwrap();
    assert_eq!(detail["files"], serde_json::json!(["new.txt"]), "{detail}");
    assert_eq!(detail["unpushed"], false);
    assert!(item.worktree.exists());
    assert!(fx.core.calls().iter().all(|c| c.method != "session_kill"), "nothing killed on refusal");

    git(&item.worktree, &["add", "new.txt"]);
    git(&item.worktree, &["commit", "-q", "-m", "wip"]);
    let e = w.finish(&item.id, opts(false)).await.unwrap_err();
    assert_eq!(e.code, ErrorCode::Dirty);
    let detail = e.detail.unwrap();
    assert_eq!(detail["files"], serde_json::json!([]));
    assert_eq!(detail["unpushed"], true);
    assert_eq!(detail["unpushed_commits"], 1);

    let done = w.finish(&item.id, opts(true)).await.unwrap();
    assert_eq!(done.state, WorkState::Finished);
    assert!(!item.worktree.exists());
    assert_eq!(fx.worktree_count(), 1);
    assert!(git(&fx.repo, &["branch", "--list", &item.branch]).is_empty(), "branch deleted");
    let killed = fx.core.calls().iter().filter(|c| c.method == "session_kill").count();
    assert_eq!(killed, 2);
    let names: Vec<String> = fx.core.published().into_iter().map(|e| e.name).collect();
    assert!(names.contains(&"worktree.removed".to_owned()) && names.contains(&"work.finished".to_owned()));
    // status_map.done → transition to Done.
    assert_eq!(fx.tracker.ticket("SHOP-141").unwrap().ticket.status.name, "Done");
    // Idempotent.
    assert_eq!(w.finish(&item.id, opts(false)).await.unwrap().state, WorkState::Finished);
}

#[tokio::test]
async fn finish_clean_pushed_without_force() {
    need_git!();
    let fx = Fx::new();
    let (w, item) = started(&fx, "SHOP-141").await;
    std::fs::write(item.worktree.join("done.txt"), "ok\n").unwrap();
    git(&item.worktree, &["add", "done.txt"]);
    git(&item.worktree, &["commit", "-q", "-m", "done"]);
    git(&item.worktree, &["push", "-q", "-u", "origin", &item.branch]);
    let st = w.status(&item.id).await.unwrap();
    assert!(!st.dirty && !st.unpushed);
    let done = w
        .finish(
            &item.id,
            FinishOpts { remove_worktree: true, delete_branch: false, force: false, transition_to: None },
        )
        .await
        .unwrap();
    assert_eq!(done.state, WorkState::Finished);
    assert!(!item.worktree.exists());
    assert!(!git(&fx.repo, &["branch", "--list", &item.branch]).is_empty(), "branch kept");
}

#[tokio::test]
async fn status_ahead_behind_dirty() {
    need_git!();
    let fx = Fx::new();
    let (w, item) = started(&fx, "SHOP-141").await;
    let st = w.status(&item.id).await.unwrap();
    assert_eq!((st.ahead, st.behind, st.dirty, st.unpushed), (0, 0, false, false));
    std::fs::write(item.worktree.join("a.txt"), "a\n").unwrap();
    git(&item.worktree, &["add", "a.txt"]);
    git(&item.worktree, &["commit", "-q", "-m", "a"]);
    std::fs::write(item.worktree.join("b.txt"), "b\n").unwrap();
    // main moves on the remote.
    std::fs::write(fx.repo.join("main.txt"), "m\n").unwrap();
    git(&fx.repo, &["add", "main.txt"]);
    git(&fx.repo, &["commit", "-q", "-m", "m"]);
    git(&fx.repo, &["push", "-q", "origin", "main"]);
    let st = w.status(&item.id).await.unwrap();
    assert_eq!((st.ahead, st.behind, st.dirty, st.unpushed), (1, 1, true, true));
}

#[tokio::test]
async fn create_pr_pushes_in_a_pane_creates_and_applies_on_pr() {
    need_git!();
    let fx = Fx::new();
    let (w, item) = started(&fx, "SHOP-141").await;
    let task = tokio::spawn({
        let (w, id) = (w.clone(), item.id.clone());
        async move { w.create_pr(&id, PrDraft::default()).await }
    });
    let push = fx.wait_session(|s| s.kind == SessionKind::Custom && s.name == "git push").await;
    let call = fx
        .core
        .calls()
        .into_iter()
        .find(|c| c.method == "session_spawn" && c.args["name"] == "git push")
        .unwrap();
    assert_eq!(call.args["args"], serde_json::json!(["push", "-u", "origin", item.branch]));
    assert_eq!(call.args["close_on_exit"], "on_success");
    // Really push (the transient session is simulated), then report success.
    git(&item.worktree, &["push", "-q", "-u", "origin", &item.branch]);
    fx.core.exit_session(&push, 0);
    let out = task.await.unwrap().unwrap();
    assert_eq!(out.state, WorkState::PrOpen);
    assert_eq!(out.pr_url.as_deref(), Some("https://github.com/acme/shop-api/pull/91"));
    let hc = fx.host.calls();
    assert!(
        hc.contains(&format!("find_for_branch:{}", item.branch)) && hc.contains(&"create".to_owned()),
        "{hc:?}"
    );
    let t = fx.tracker.ticket("SHOP-141").unwrap();
    assert_eq!(t.ticket.status.name, "In Review");
    assert!(t.comments.iter().any(|c| c.body_html.contains("PR: https://github.com/acme/shop-api/pull/91")));
    let names: Vec<String> = fx.core.published().into_iter().map(|e| e.name).collect();
    assert!(names.contains(&"pr.before_create".to_owned()) && names.contains(&"pr.created".to_owned()));
}

#[tokio::test]
async fn create_pr_links_existing_and_reports_push_failure() {
    need_git!();
    let fx = Fx::new();
    // SHOP-142's branch already has PR #90 (Claude used `gh`).
    let (w, item) = started(&fx, "SHOP-142").await;
    assert_eq!(item.branch, "feat/SHOP-142-rate-limit-login");
    let task = tokio::spawn({
        let (w, id) = (w.clone(), item.id.clone());
        async move { w.create_pr(&id, PrDraft::default()).await }
    });
    let push = fx.wait_session(|s| s.name == "git push").await;
    fx.core.exit_session(&push, 0);
    let out = task.await.unwrap().unwrap();
    assert_eq!(out.pr_url.as_deref(), Some("https://github.com/acme/shop-api/pull/90"));
    assert!(!fx.host.calls().contains(&"create".to_owned()));

    let fx = Fx::new();
    let (w, item) = started(&fx, "SHOP-141").await;
    let task = tokio::spawn({
        let (w, id) = (w.clone(), item.id.clone());
        async move { w.create_pr(&id, PrDraft::default()).await }
    });
    let push = fx.wait_session(|s| s.name == "git push").await;
    fx.core.exit_session(&push, 128);
    let e = task.await.unwrap().unwrap_err();
    assert!(e.message.contains("git push failed (exit 128)"), "{e:?}");
    assert_eq!(w.list(None).await.unwrap()[0].state, WorkState::Active);
}

#[tokio::test]
async fn git_timeout_kills_hanging_fetch() {
    need_git!();
    let fx = Fx::new();
    git(&fx.repo, &["config", "remote.origin.uploadpack", "sleep 30; git-upload-pack"]);
    let t0 = Instant::now();
    let e = kelta_work::git::fetch(&fx.repo, "origin", &["main"], Duration::from_secs(1)).await.unwrap_err();
    assert_eq!(e.code, ErrorCode::Timeout, "{e:?}");
    assert!(t0.elapsed() < Duration::from_secs(5), "{:?}", t0.elapsed());

    // The saga treats a hanging fetch as offline and still starts from the local origin/main.
    fx.settings(|s| s.worktree.fetch_timeout_secs = 1);
    let t0 = Instant::now();
    let (_w, item) = started(&fx, "SHOP-141").await;
    assert!(t0.elapsed() < Duration::from_secs(15));
    let step = item.steps.iter().find(|s| s.step == "fetch_base").unwrap();
    assert!(step.detail.as_deref().unwrap_or_default().starts_with("offline"), "{step:?}");
}

#[tokio::test]
async fn git_never_prompts() {
    need_git!();
    let fx = Fx::new();
    for url in ["https://127.0.0.1:9/acme/x.git", "ssh://git@127.0.0.1:9/acme/x.git"] {
        git(&fx.repo, &["remote", "set-url", "origin", url]);
        let t0 = Instant::now();
        let e =
            kelta_work::git::fetch(&fx.repo, "origin", &["main"], Duration::from_secs(20)).await.unwrap_err();
        assert!(matches!(e.code, ErrorCode::Network | ErrorCode::Timeout), "{url}: {e:?}");
        assert!(t0.elapsed() < Duration::from_secs(20), "{url} hung");
    }
    let env = kelta_work::git::git_env();
    assert_eq!(env["GIT_TERMINAL_PROMPT"], "0");
    assert!(env["GIT_SSH_COMMAND"].contains("BatchMode=yes"));
}

//! Finish (dirty/unpushed refusal, force), create PR, status, git timeouts / non-interactivity.

use std::time::{Duration, Instant};

use crate::common::{Fx, git, has_git, project};
use kelta_proto::codehost::PrDraft;
use kelta_proto::error::ErrorCode;
use kelta_proto::model::{FinishOpts, SessionKind, ShipOrigin, WorkItem, WorkSource, WorkState};
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

/// One commit ahead of the base (something to ship).
fn commit(item: &WorkItem, file: &str) {
    std::fs::write(item.worktree.join(file), "x\n").unwrap();
    git(&item.worktree, &["add", file]);
    git(&item.worktree, &["commit", "-q", "-m", file]);
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
    // Our untracked include copy must not need --force.
    std::fs::write(fx.repo.join("local.cfg"), "x=1\n").unwrap();
    std::fs::write(fx.repo.join(".worktreeinclude"), "local.cfg\n").unwrap();
    let (w, item) = started(&fx, "SHOP-141").await;
    assert!(item.worktree.join("local.cfg").exists());
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
    commit(&item, "a.txt");
    let task = tokio::spawn({
        let (w, id) = (w.clone(), item.id.clone());
        async move { w.create_pr(&id, PrDraft::default(), ShipOrigin::Ui).await }
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
    commit(&item, "a.txt");
    let task = tokio::spawn({
        let (w, id) = (w.clone(), item.id.clone());
        async move { w.create_pr(&id, PrDraft::default(), ShipOrigin::Ui).await }
    });
    let push = fx.wait_session(|s| s.name == "git push").await;
    fx.core.exit_session(&push, 0);
    let out = task.await.unwrap().unwrap();
    assert_eq!(out.pr_url.as_deref(), Some("https://github.com/acme/shop-api/pull/90"));
    assert!(!fx.host.calls().contains(&"create".to_owned()));

    let fx = Fx::new();
    let (w, item) = started(&fx, "SHOP-141").await;
    commit(&item, "a.txt");
    let task = tokio::spawn({
        let (w, id) = (w.clone(), item.id.clone());
        async move { w.create_pr(&id, PrDraft::default(), ShipOrigin::Ui).await }
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
    let e =
        kelta_work::git::fetch(&fx.repo, "origin", &["main"], Duration::from_millis(200)).await.unwrap_err();
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

#[tokio::test]
async fn unpushed_ignores_base_history_without_remote() {
    need_git!();
    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path().join("solo");
    std::fs::create_dir_all(&repo).unwrap();
    git(&repo, &["init", "-q", "-b", "main"]);
    git(&repo, &["-c", "user.email=a@b", "-c", "user.name=a", "commit", "-q", "--allow-empty", "-m", "one"]);
    git(&repo, &["checkout", "-q", "-b", "feat"]);
    assert_eq!(kelta_work::git::unpushed_count(&repo, "main").await.unwrap(), 0);
    git(&repo, &["-c", "user.email=a@b", "-c", "user.name=a", "commit", "-q", "--allow-empty", "-m", "two"]);
    assert_eq!(kelta_work::git::unpushed_count(&repo, "main").await.unwrap(), 1);
    // Missing base branch is ignored, not an error.
    assert_eq!(kelta_work::git::unpushed_count(&repo, "nope").await.unwrap(), 2);
}

#[tokio::test]
async fn finish_keeps_user_files_that_match_include_patterns() {
    need_git!();
    let fx = Fx::new();
    std::fs::write(fx.repo.join("local.cfg"), "x=1\n").unwrap();
    std::fs::write(fx.repo.join(".worktreeinclude"), "local.*\n").unwrap();
    let (w, item) = started(&fx, "SHOP-141").await;
    // Never copied by Kelta, same pattern: the user's file.
    std::fs::write(item.worktree.join("local.staging"), "mine\n").unwrap();
    let e = w.finish(&item.id, opts(false)).await.unwrap_err();
    assert_eq!(e.code, ErrorCode::Dirty);
    assert_eq!(e.detail.unwrap()["files"], serde_json::json!(["local.staging"]));
    assert!(item.worktree.join("local.staging").exists());
    // A copy the user edited is theirs too.
    std::fs::remove_file(item.worktree.join("local.staging")).unwrap();
    std::fs::write(item.worktree.join("local.cfg"), "x=2\n").unwrap();
    let e = w.finish(&item.id, opts(false)).await.unwrap_err();
    assert_eq!(e.detail.unwrap()["files"], serde_json::json!(["local.cfg"]));
    assert!(w.status(&item.id).await.unwrap().dirty);
}

#[tokio::test]
async fn finish_after_squash_merge_with_pruned_branch() {
    need_git!();
    let fx = Fx::new();
    let (w, item) = started(&fx, "SHOP-141").await;
    std::fs::write(item.worktree.join("a.txt"), "a\n").unwrap();
    git(&item.worktree, &["add", "a.txt"]);
    git(&item.worktree, &["commit", "-q", "-m", "a"]);
    std::fs::write(item.worktree.join("b.txt"), "b\n").unwrap();
    git(&item.worktree, &["add", "b.txt"]);
    git(&item.worktree, &["commit", "-q", "-m", "b"]);
    // Squash merge on the host: one new commit on main, head branch never on a remote ref.
    std::fs::write(fx.repo.join("a.txt"), "a\n").unwrap();
    std::fs::write(fx.repo.join("b.txt"), "b\n").unwrap();
    git(&fx.repo, &["add", "a.txt", "b.txt"]);
    git(&fx.repo, &["commit", "-q", "-m", "squash"]);
    git(&fx.repo, &["push", "-q", "origin", "main"]);
    git(&fx.repo, &["reset", "-q", "--hard", "HEAD~1"]);
    assert!(!w.status(&item.id).await.unwrap().unpushed);
    let done = w.finish(&item.id, opts(false)).await.unwrap();
    assert_eq!(done.state, WorkState::Finished);
    assert!(!item.worktree.exists());
}

#[tokio::test]
async fn scratch_pr_uses_the_item_title_and_task() {
    need_git!();
    use kelta_proto::api::CodeHost;
    let fx = Fx::new();
    let w = fx.service();
    let task = "Speed up search\nKeep the ranking the same.";
    let source = WorkSource::Branch { name: String::new(), task: Some(task.into()), repo: None };
    let item = w.start(w.plan(&project(), source).await.unwrap()).await.unwrap();
    std::fs::write(item.worktree.join("a.txt"), "x\n").unwrap();
    git(&item.worktree, &["add", "a.txt"]);
    git(&item.worktree, &["commit", "-q", "-m", "a"]);
    let pr = tokio::spawn({
        let (w, id) = (w.clone(), item.id.clone());
        async move { w.create_pr(&id, PrDraft::default(), ShipOrigin::Ui).await }
    });
    let push = fx.wait_session(|s| s.name == "git push").await;
    fx.core.exit_session(&push, 0);
    let out = pr.await.unwrap().unwrap();
    let binding = samples::project_info().repos[0].code_host.clone().unwrap();
    let review = fx.host.find_for_branch(&binding.repo, &item.branch).await.unwrap().unwrap();
    assert_eq!(review.title, "Speed up search");
    assert_eq!(out.pr_url.as_deref(), Some(review.url.as_str()));
    let detail = fx.host.get(&review.r#ref).await.unwrap();
    assert!(detail.body_html.contains("Keep the ranking the same."), "{}", detail.body_html);
    assert!(fx.tracker.calls().is_empty(), "no tracker side effects");
}

#[tokio::test]
async fn ports_env_template_and_teardown() {
    need_git!();
    let fx = Fx::new();
    fx.settings(|s| {
        s.ports.range = "47300-47319".into();
        s.worktree.env_template = ".env.kelta".into();
        s.worktree.teardown = "docker compose down".into();
    });
    // Untracked template in the main checkout; the main `.env` (SECRET=1) must not win.
    std::fs::write(fx.repo.join(".env.kelta"), "PORT={port}\nDB={port.9}\n").unwrap();
    let (w, a) = started(&fx, "SHOP-141").await;
    let (_, b) = started(&fx, "SHOP-142").await;
    let (pa, pb) = (a.port_base.unwrap(), b.port_base.unwrap());
    assert!(pa.abs_diff(pb) >= 10, "{pa} {pb}");
    assert_eq!(
        std::fs::read_to_string(a.worktree.join(".env")).unwrap(),
        format!("PORT={pa}\nDB={}\n", pa + 9)
    );
    assert_eq!(a.env()["KELTA_PORT_9"], (pa + 9).to_string());
    assert_eq!(a.env()["COMPOSE_PROJECT_NAME"], "kelta-shop-141");
    let scratch = || WorkSource::Branch { name: "wip/ports".into(), task: None, repo: None };
    let e = w.start(w.plan(&project(), scratch()).await.unwrap()).await.unwrap_err();
    assert!(e.message.contains("no free block"), "{}", e.message);

    let finish = |id: kelta_proto::ids::WorkItemId| {
        let w = w.clone();
        tokio::spawn(async move { w.finish(&id, opts(false)).await })
    };
    let teardown = |n: usize| {
        let fx = &fx;
        async move {
            for _ in 0..500 {
                let t = fx.spawned_of(|k| *k == SessionKind::Setup);
                if t.len() > n {
                    assert_eq!(t[n].name, "teardown");
                    return t[n].id.clone();
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
            panic!("no teardown");
        }
    };
    // A failed teardown stops the Finish before anything is removed.
    let task = finish(b.id.clone());
    fx.core.exit_session(&teardown(0).await, 1);
    let e = task.await.unwrap().unwrap_err();
    assert!(e.message.contains("teardown `docker compose down` failed (exit 1)"), "{}", e.message);
    assert!(b.worktree.exists());
    let task = finish(b.id.clone());
    fx.core.exit_session(&teardown(1).await, 0);
    let done = task.await.unwrap().unwrap();
    assert_eq!((done.state, done.port_base), (WorkState::Finished, None));
    // The freed block is reused.
    let c = w.start(w.plan(&project(), scratch()).await.unwrap()).await.unwrap();
    assert_eq!(c.port_base, Some(pb));
}

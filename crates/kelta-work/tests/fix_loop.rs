//! Feedback loop (work_send, feedback, re-request, resolve), own-PR resume/adoption (B2), rebase and
//! push on real temp git repos: never type into a busy Claude, never drop someone else's commits.
#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use std::path::Path;
use std::process::Command;
use std::sync::Arc;

use common::{Fx, git, has_git, project};
use kelta_proto::api::WorkStore;
use kelta_proto::codehost::ReviewKind;
use kelta_proto::error::ErrorCode;
use kelta_proto::ids::{AccountId, SessionId};
use kelta_proto::model::{
    Lifecycle, RebaseOnto, RebaseOp, SendFile, SessionKind, SessionStatus, StatusSource, WorkItem, WorkKind,
    WorkSource, WorkState,
};
use kelta_proto::samples;
use kelta_proto::testing::FakeCodeHost;
use kelta_work::WorkService;

macro_rules! need_git {
    () => {
        if !has_git() {
            eprintln!("skipping: git not found");
            return;
        }
    };
}

async fn started(fx: &Fx, key: &str) -> (Arc<WorkService>, WorkItem) {
    let w = fx.service();
    let mut t = samples::ticket_ref();
    t.key = key.into();
    let item = w.start(w.plan(&project(), WorkSource::Ticket { ticket: t }).await.unwrap()).await.unwrap();
    assert_eq!(item.state, WorkState::Active, "{:?}", item.steps);
    (w, item)
}

fn claude(fx: &Fx) -> SessionId {
    fx.spawned_of(|k| *k == SessionKind::Claude)
        .into_iter()
        .find(|s| s.lifecycle == Lifecycle::Live)
        .unwrap()
        .id
}

fn set_status(fx: &Fx, sid: &SessionId, status: SessionStatus, source: StatusSource) {
    let mut s = fx.core.sessions().into_iter().find(|s| &s.id == sid).unwrap();
    (s.status, s.status_source) = (status, source);
    fx.core.insert_session(s);
}

fn brief() -> Vec<SendFile> {
    vec![SendFile { name: "feedback.md".into(), content: "# Review feedback\n".into() }]
}

fn commit(dir: &Path, file: &str, text: &str, msg: &str) {
    std::fs::write(dir.join(file), text).unwrap();
    git(dir, &["add", file]);
    git(dir, &["commit", "-q", "-m", msg]);
}

/// A teammate's clone of the remote.
fn teammate(fx: &Fx) -> std::path::PathBuf {
    let dir = fx.tmp.path().join("teammate");
    if !dir.exists() {
        git(fx.tmp.path(), &["clone", "-q", fx.remote.to_str().unwrap(), "teammate"]);
    }
    git(&dir, &["fetch", "-q", "origin"]);
    dir
}

/// Run the transient push pane the way the terminal would, then report its exit.
async fn run_push_pane(fx: &Fx, wt: &Path) -> Vec<String> {
    let sid = fx.wait_session(|s| s.name == "git push" && s.lifecycle == Lifecycle::Live).await;
    let call = fx
        .core
        .calls()
        .into_iter()
        .rev()
        .find(|c| c.method == "session_spawn" && c.args["name"] == "git push")
        .unwrap();
    let args: Vec<String> = serde_json::from_value(call.args["args"].clone()).unwrap();
    let out =
        Command::new("git").args(&args).current_dir(wt).env("GIT_TERMINAL_PROMPT", "0").output().unwrap();
    fx.core.exit_session(&sid, out.status.code().unwrap_or(1));
    args
}

async fn push(
    fx: &Fx,
    w: &Arc<WorkService>,
    item: &WorkItem,
    force: bool,
) -> (Vec<String>, Result<WorkItem, kelta_proto::error::KeltaError>) {
    let task = tokio::spawn({
        let (w, id) = (w.clone(), item.id.clone());
        async move { w.push(&id, force).await }
    });
    let args = run_push_pane(fx, &item.worktree).await;
    (args, task.await.unwrap())
}

fn reason(e: &kelta_proto::error::KeltaError) -> &str {
    e.detail.as_ref().and_then(|d| d["reason"].as_str()).unwrap_or("")
}

// ---- work_send ---------------------------------------------------------------------------------

#[tokio::test]
async fn send_pastes_into_an_idle_claude_and_keeps_the_brief_out_of_the_worktree() {
    need_git!();
    let fx = Fx::new();
    let (w, item) = started(&fx, "SHOP-141").await;
    let sid = claude(&fx);
    set_status(&fx, &sid, SessionStatus::Done, StatusSource::Hook);
    let out = w.send(&item.id, "Read {file}\x1b[201~ now", brief(), Some(vec!["T1".into()])).await.unwrap();
    assert_eq!(out.sent_threads, vec!["T1"]);

    let settings = fx
        .core
        .calls()
        .into_iter()
        .find(|c| c.method == "session_spawn" && c.args["kind"]["type"] == "claude")
        .unwrap();
    let args: Vec<String> = serde_json::from_value(settings.args["args"].clone()).unwrap();
    let run = args
        .windows(2)
        .find(|a| a[0] == "--settings")
        .map(|a| Path::new(&a[1]).parent().unwrap().to_path_buf())
        .unwrap();
    let file = run.join("feedback.md");
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "# Review feedback\n");
    assert!(run.join("ticket.md").exists(), "next to ticket.md");
    assert_eq!(fx.core.written_text(&sid), format!("\x1b[200~Read {}[201~ now\x1b[201~\r", file.display()));
    assert_eq!(git(&item.worktree, &["status", "--porcelain"]), "", "worktree untouched");

    // WaitingUser (idle_prompt) is idle too.
    set_status(&fx, &sid, SessionStatus::WaitingUser, StatusSource::Hook);
    w.send(&item.id, "again", vec![], None).await.unwrap();
    assert!(fx.core.written_text(&sid).ends_with("\x1b[200~again\x1b[201~\r"));
    assert_eq!(w.list(None).await.unwrap()[0].sent_threads, vec!["T1"], "kept without new threads");
}

#[tokio::test]
async fn send_never_types_into_a_busy_or_permission_prompting_claude() {
    need_git!();
    let fx = Fx::new();
    let (w, item) = started(&fx, "SHOP-141").await;
    let sid = claude(&fx);
    for (status, source, why) in [
        (SessionStatus::Working, StatusSource::Hook, "claude_busy"),
        (SessionStatus::NeedsInput, StatusSource::Hook, "claude_busy"),
        (SessionStatus::Running, StatusSource::Hook, "claude_busy"),
        (SessionStatus::Done, StatusSource::Heuristic, "hooks_inactive"),
        (SessionStatus::WaitingUser, StatusSource::None, "hooks_inactive"),
    ] {
        set_status(&fx, &sid, status, source);
        let e = w.send(&item.id, "fix it", brief(), Some(vec!["T1".into()])).await.unwrap_err();
        assert_eq!((e.code, reason(&e)), (ErrorCode::Conflict, why), "{status:?}/{source:?}");
    }
    assert!(fx.core.writes().is_empty(), "nothing typed");
    assert!(w.list(None).await.unwrap()[0].sent_threads.is_empty());
    let e =
        w.send(&item.id, "x", vec![SendFile { name: "../x.md".into(), content: String::new() }], None).await;
    assert_eq!(e.unwrap_err().code, ErrorCode::InvalidArgument);
    assert_eq!(w.send(&item.id, "  ", vec![], None).await.unwrap_err().code, ErrorCode::InvalidArgument);
}

#[tokio::test]
async fn send_resumes_a_dead_claude_with_the_prompt_and_the_fallback_keeps_it() {
    need_git!();
    let fx = Fx::new();
    let (w, item) = started(&fx, "SHOP-141").await;
    fx.core.exit_session(&claude(&fx), 0);
    w.send(&item.id, "-Fix the review", brief(), None).await.unwrap();
    let claude_args = |fx: &Fx| -> Vec<Vec<String>> {
        fx.core
            .calls()
            .into_iter()
            .filter(|c| c.method == "session_spawn" && c.args["kind"]["type"] == "claude")
            .map(|c| serde_json::from_value(c.args["args"].clone()).unwrap())
            .collect()
    };
    let spawns = claude_args(&fx);
    assert_eq!(spawns.len(), 2);
    assert_eq!(spawns[1][..2], ["--resume".to_owned(), item.claude_uuid.clone().unwrap()]);
    assert_eq!(spawns[1][spawns[1].len() - 2..], ["--".to_owned(), "-Fix the review".to_owned()]);

    // `--resume` refused: the `--continue` fallback carries the same prompt, with the §6 toast.
    fx.core.exit_session(&claude(&fx), 1);
    for _ in 0..300 {
        if claude_args(&fx).len() == 3 {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    let spawns = claude_args(&fx);
    assert_eq!(spawns[2][0], "--continue");
    assert_eq!(spawns[2].last().map(String::as_str), Some("-Fix the review"));
    assert!(
        fx.core
            .toasts()
            .iter()
            .any(|t| t.text.contains("continued the latest one in this worktree with your prompt"))
    );
}

// ---- feedback, re-request, resolve -----------------------------------------------------------

#[tokio::test]
async fn feedback_rerequest_and_resolve_go_to_the_items_pr() {
    need_git!();
    let fx = Fx::new();
    let (w, item) = started(&fx, "SHOP-142").await;
    assert_eq!(w.feedback(&item.id).await.unwrap_err().code, ErrorCode::InvalidArgument, "no PR yet");
    let mut it = w.list(None).await.unwrap().remove(0);
    it.pr_url = Some("https://github.com/acme/shop-api/pull/90".into());
    it.sent_threads = vec!["T1".into(), "T2".into()];
    fx.store.put_item(&it).await.unwrap();
    assert_eq!(w.feedback(&item.id).await.unwrap(), samples::feedback());
    assert_eq!(w.rerequest_review(&item.id).await.unwrap(), vec!["bob"]);
    let out = w.resolve_sent_threads(&item.id).await.unwrap();
    assert!(out.sent_threads.is_empty());
    let calls = fx.host.calls();
    for c in ["feedback:90", "rerequest_review:90", "resolve_threads:90:T1,T2"] {
        assert!(calls.contains(&c.to_owned()), "{c} in {calls:?}");
    }
    assert_eq!(w.resolve_sent_threads(&item.id).await.unwrap_err().code, ErrorCode::InvalidArgument);
}

// ---- B2: own PRs -----------------------------------------------------------------------------

#[tokio::test]
async fn review_locally_on_my_own_pr_resumes_its_work_item() {
    need_git!();
    let fx = Fx::new();
    let (w, item) = started(&fx, "SHOP-142").await;
    let mut pr = samples::review_ref();
    pr.number = 90; // FakeCodeHost: authored, head feat/SHOP-142-rate-limit-login
    let plan = w.plan(&project(), WorkSource::Review { review: pr }).await.unwrap();
    assert_eq!(plan.existing.as_ref(), Some(&item.id));
    let before = fx.worktree_count();
    let out = w.start(plan).await.unwrap();
    assert_eq!(out.id, item.id);
    assert_eq!(fx.worktree_count(), before, "no kelta/pr-90 worktree");
    assert_eq!(w.list(None).await.unwrap().len(), 1);
}

#[tokio::test]
async fn my_pr_made_outside_kelta_is_adopted_on_its_head_branch() {
    need_git!();
    let fx = Fx::new();
    git(&fx.repo, &["checkout", "-q", "-b", "feat/outside"]);
    commit(&fx.repo, "outside.txt", "x\n", "outside work");
    git(&fx.repo, &["push", "-q", "origin", "feat/outside"]);
    let head = git(&fx.repo, &["rev-parse", "HEAD"]);
    git(&fx.repo, &["checkout", "-q", "main"]);
    git(&fx.repo, &["branch", "-q", "-D", "feat/outside"]);
    let mut r = samples::review();
    r.r#ref.number = 95;
    r.kind = ReviewKind::Authored;
    r.url = "https://github.com/acme/shop-api/pull/95".into();
    r.source_branch = "feat/outside".into();
    fx.core
        .add_code_host(AccountId::new("github-work"), Arc::new(FakeCodeHost::with_reviews(vec![r.clone()])));

    let w = fx.service();
    let plan = w.plan(&project(), WorkSource::Review { review: r.r#ref.clone() }).await.unwrap();
    assert_eq!(plan.source, WorkSource::Branch { name: "feat/outside".into() });
    assert_eq!(plan.adopt_pr.as_deref(), Some(r.url.as_str()));
    let item = w.start(plan).await.unwrap();
    assert_eq!((item.kind, item.state.clone()), (WorkKind::Branch, WorkState::PrOpen), "{:?}", item.steps);
    assert_eq!(item.pr_url.as_deref(), Some(r.url.as_str()));
    assert_eq!(git(&item.worktree, &["rev-parse", "HEAD"]), head, "checked out the PR head");
    assert_eq!(git(&item.worktree, &["rev-parse", "--abbrev-ref", "HEAD"]), "feat/outside");
}

// ---- rebase ----------------------------------------------------------------------------------

/// Item with one own commit, `main` advanced on the remote; `pushed` pushes the branch first.
async fn diverging(fx: &Fx, pushed: bool) -> (Arc<WorkService>, WorkItem) {
    let (w, item) = started(fx, "SHOP-141").await;
    commit(&item.worktree, "mine.txt", "mine\n", "my work");
    if pushed {
        git(&item.worktree, &["push", "-q", "-u", "origin", &item.branch]);
    }
    let mate = teammate(fx);
    commit(&mate, "theirs.txt", "theirs\n", "main moved");
    git(&mate, &["push", "-q", "origin", "main"]);
    set_status(fx, &claude(fx), SessionStatus::Done, StatusSource::Hook);
    (w, item)
}

fn start(onto: RebaseOnto) -> RebaseOp {
    RebaseOp::Start { onto, no_fetch: false }
}

#[tokio::test]
async fn rebase_refuses_while_claude_works_or_the_tree_is_dirty() {
    need_git!();
    let fx = Fx::new();
    let (w, item) = diverging(&fx, false).await;
    set_status(&fx, &claude(&fx), SessionStatus::Working, StatusSource::Hook);
    let e = w.rebase(&item.id, start(RebaseOnto::Base)).await.unwrap_err();
    assert_eq!((e.code, reason(&e)), (ErrorCode::Conflict, "claude_busy"));
    assert_eq!(e.message, "Claude is working in this worktree. Rebase when it stops.");
    set_status(&fx, &claude(&fx), SessionStatus::Done, StatusSource::Hook);
    std::fs::write(item.worktree.join("README.md"), "edited\n").unwrap();
    let e = w.rebase(&item.id, start(RebaseOnto::Base)).await.unwrap_err();
    assert_eq!(e.code, ErrorCode::Dirty);
    assert_eq!(e.message, "1 uncommitted file. Commit or stash them first.");
    assert_eq!(e.detail.unwrap()["files"], serde_json::json!(["README.md"]));
    assert!(w.list(None).await.unwrap()[0].rebase.is_none());
}

#[tokio::test]
async fn clean_rebase_of_a_never_pushed_branch_leaves_no_state() {
    need_git!();
    let fx = Fx::new();
    let (w, item) = diverging(&fx, false).await;
    let out = w.rebase(&item.id, start(RebaseOnto::Base)).await.unwrap();
    assert!(out.rebase.is_none());
    git(&item.worktree, &["merge-base", "--is-ancestor", "origin/main", "HEAD"]);
    let st = w.status(&item.id).await.unwrap();
    assert!(!st.diverged && st.remote_new == 0);
}

#[tokio::test]
async fn own_rewrite_force_pushes_with_a_lease_on_the_recorded_tip() {
    need_git!();
    let fx = Fx::new();
    let (w, item) = diverging(&fx, true).await;
    let tip = git(&item.worktree, &["rev-parse", "HEAD"]);
    let out = w.rebase(&item.id, start(RebaseOnto::Base)).await.unwrap();
    let st = out.rebase.clone().unwrap();
    assert_eq!((st.pre_head.as_str(), st.remote_sha.as_deref()), (tip.as_str(), Some(tip.as_str())));
    assert_eq!((st.onto.as_str(), st.total), ("origin/main", 0));
    let status = w.status(&item.id).await.unwrap();
    assert!(status.diverged && status.remote_new == 0, "{status:?}");

    // A second rebase before pushing keeps the HEAD that still holds the remote tip.
    let out = w.rebase(&item.id, start(RebaseOnto::Base)).await.unwrap();
    assert_eq!(out.rebase.unwrap().pre_head, tip);

    let (args, res) = push(&fx, &w, &item, true).await;
    assert_eq!(
        args,
        [
            "push",
            &format!("--force-with-lease={}:{tip}", item.branch),
            "--force-if-includes",
            "origin",
            &item.branch
        ]
    );
    assert!(res.unwrap().rebase.is_none());
    assert_eq!(git(&fx.remote, &["rev-parse", &item.branch]), git(&item.worktree, &["rev-parse", "HEAD"]));
}

#[tokio::test]
async fn a_moved_remote_rejects_the_lease_and_never_falls_back_to_force() {
    need_git!();
    let fx = Fx::new();
    let (w, item) = diverging(&fx, true).await;
    w.rebase(&item.id, start(RebaseOnto::Base)).await.unwrap();
    let mate = teammate(&fx);
    git(&mate, &["checkout", "-q", &item.branch]);
    commit(&mate, "theirs2.txt", "late\n", "teammate pushed");
    git(&mate, &["push", "-q", "origin", &item.branch]);
    let theirs = git(&mate, &["rev-parse", "HEAD"]);

    let (_, res) = push(&fx, &w, &item, true).await;
    let e = res.unwrap_err();
    assert_eq!((e.code, reason(&e)), (ErrorCode::Conflict, "lease_rejected"), "{e:?}");
    assert_eq!(
        e.message,
        format!("origin/{} moved since your last fetch. Someone else pushed.", item.branch)
    );
    assert_eq!(git(&fx.remote, &["rev-parse", &item.branch]), theirs, "their commit survives");
    let pushes = fx.core.sessions().into_iter().filter(|s| s.name == "git push").count();
    assert_eq!(pushes, 1, "no retry");
    // After the fetch the item shows Remote has new commits, not Force push.
    let st = w.status(&item.id).await.unwrap();
    assert!(!st.diverged && st.remote_new == 1, "{st:?}");
    let e = w.push(&item.id, true).await.unwrap_err();
    assert_eq!(reason(&e), "not_diverged");
}

#[tokio::test]
async fn suggestion_commits_and_update_branch_are_remote_new_never_force_pushed() {
    need_git!();
    let fx = Fx::new();
    let (w, item) = diverging(&fx, true).await;
    let mate = teammate(&fx);
    git(&mate, &["checkout", "-q", &item.branch]);
    git(&mate, &["merge", "-q", "--no-edit", "origin/main"]); // Update branch
    commit(&mate, "mine.txt", "mine, as suggested\n", "Apply suggestion"); // Commit suggestion
    git(&mate, &["push", "-q", "origin", &item.branch]);
    git(&fx.repo, &["fetch", "-q", "origin"]);

    let st = w.status(&item.id).await.unwrap();
    assert!(st.remote_new >= 2 && !st.diverged, "{st:?}");
    assert_eq!(reason(&w.push(&item.id, true).await.unwrap_err()), "not_diverged");

    let out = w.rebase(&item.id, start(RebaseOnto::RemoteBranch)).await.unwrap();
    assert!(out.rebase.is_none(), "fast-forward push suffices: {:?}", out.rebase);
    assert_eq!(std::fs::read_to_string(item.worktree.join("mine.txt")).unwrap(), "mine, as suggested\n");
    let st = w.status(&item.id).await.unwrap();
    assert!(st.remote_new == 0 && !st.diverged, "{st:?}");
}

#[tokio::test]
async fn rebase_onto_remote_after_a_rebase_onto_base_never_copies_base_commits() {
    need_git!();
    let fx = Fx::new();
    let (w, item) = diverging(&fx, true).await;
    w.rebase(&item.id, start(RebaseOnto::Base)).await.unwrap();
    // The force push is not done yet: a reviewer commits a suggestion on the remote branch.
    let mate = teammate(&fx);
    git(&mate, &["checkout", "-q", &item.branch]);
    commit(&mate, "mine.txt", "mine, as suggested\n", "Apply suggestion");
    git(&mate, &["push", "-q", "origin", &item.branch]);

    w.rebase(&item.id, start(RebaseOnto::RemoteBranch)).await.unwrap();
    let own = git(&item.worktree, &["log", "--format=%s", &format!("origin/{}..HEAD", item.branch)]);
    assert_eq!(own, "", "main's commits are not replayed onto the branch");
    let out = w.rebase(&item.id, start(RebaseOnto::Base)).await.unwrap();
    assert!(out.rebase.is_some_and(|r| r.total == 0), "force push pending");
    let log = git(&item.worktree, &["log", "--format=%s", "origin/main..HEAD"]);
    assert_eq!(log, "Apply suggestion\nmy work");
    assert_eq!(std::fs::read_to_string(item.worktree.join("mine.txt")).unwrap(), "mine, as suggested\n");
}

#[tokio::test]
async fn non_fast_forward_push_says_the_remote_has_commits() {
    need_git!();
    let fx = Fx::new();
    let (w, item) = diverging(&fx, true).await;
    let mate = teammate(&fx);
    git(&mate, &["checkout", "-q", &item.branch]);
    commit(&mate, "theirs2.txt", "x\n", "suggestion");
    git(&mate, &["push", "-q", "origin", &item.branch]);
    commit(&item.worktree, "more.txt", "more\n", "more work");
    let (args, res) = push(&fx, &w, &item, false).await;
    assert_eq!(args, ["push", "-u", "origin", &item.branch]);
    let e = res.unwrap_err();
    assert_eq!((reason(&e), e.message.as_str()), ("non_fast_forward", "origin has commits you do not have."));
}

#[tokio::test]
async fn conflicts_stop_the_rebase_then_continue_or_abort() {
    need_git!();
    let fx = Fx::new();
    let (w, item) = started(&fx, "SHOP-141").await;
    set_status(&fx, &claude(&fx), SessionStatus::Done, StatusSource::Hook);
    commit(&item.worktree, "README.md", "mine\n", "my readme");
    let mate = teammate(&fx);
    commit(&mate, "README.md", "theirs\n", "their readme");
    git(&mate, &["push", "-q", "origin", "main"]);
    let pre = git(&item.worktree, &["rev-parse", "HEAD"]);

    let out = w.rebase(&item.id, start(RebaseOnto::Base)).await.unwrap();
    let st = out.rebase.unwrap();
    assert_eq!(st.conflicts, vec![std::path::PathBuf::from("README.md")]);
    assert_eq!((st.step, st.total, st.pre_head.as_str()), (1, 1, pre.as_str()));
    assert_eq!(reason(&w.push(&item.id, false).await.unwrap_err()), "");
    // Continue without resolving: still stopped.
    assert_eq!(w.rebase(&item.id, RebaseOp::Continue).await.unwrap().rebase.unwrap().conflicts.len(), 1);
    // Abort: back to the pre-rebase HEAD, no state.
    assert!(w.rebase(&item.id, RebaseOp::Abort).await.unwrap().rebase.is_none());
    assert_eq!(git(&item.worktree, &["rev-parse", "HEAD"]), pre);

    // Again, resolved by hand (or by Claude): Continue finishes; never pushed → no state.
    w.rebase(&item.id, start(RebaseOnto::Base)).await.unwrap();
    std::fs::write(item.worktree.join("README.md"), "both\n").unwrap();
    git(&item.worktree, &["add", "README.md"]);
    assert!(w.rebase(&item.id, RebaseOp::Continue).await.unwrap().rebase.is_none());
    git(&item.worktree, &["merge-base", "--is-ancestor", "origin/main", "HEAD"]);
}

#[tokio::test]
async fn a_rebase_finished_outside_kelta_is_reread_on_status() {
    need_git!();
    let fx = Fx::new();
    let (w, item) = started(&fx, "SHOP-141").await;
    set_status(&fx, &claude(&fx), SessionStatus::Done, StatusSource::Hook);
    commit(&item.worktree, "README.md", "mine\n", "my readme");
    let mate = teammate(&fx);
    commit(&mate, "README.md", "theirs\n", "their readme");
    git(&mate, &["push", "-q", "origin", "main"]);
    w.rebase(&item.id, start(RebaseOnto::Base)).await.unwrap();
    // Claude resolves and continues in its terminal.
    std::fs::write(item.worktree.join("README.md"), "both\n").unwrap();
    git(&item.worktree, &["add", "README.md"]);
    git(&item.worktree, &["-c", "core.editor=true", "rebase", "--continue"]);
    w.status(&item.id).await.unwrap();
    assert!(w.list(None).await.unwrap()[0].rebase.is_none());
}

#[tokio::test]
async fn a_failed_fetch_offers_the_last_fetched_base() {
    need_git!();
    let fx = Fx::new();
    let (w, item) = diverging(&fx, false).await;
    git(&fx.repo, &["fetch", "-q", "origin"]);
    git(&fx.repo, &["remote", "set-url", "origin", "/nonexistent/remote.git"]);
    let e = w.rebase(&item.id, start(RebaseOnto::Base)).await.unwrap_err();
    assert_eq!((e.code, reason(&e)), (ErrorCode::Network, "fetch_failed"));
    assert_eq!(e.message, "Could not fetch origin/main.");
    let out = w.rebase(&item.id, RebaseOp::Start { onto: RebaseOnto::Base, no_fetch: true }).await.unwrap();
    assert!(out.rebase.is_none());
    git(&item.worktree, &["merge-base", "--is-ancestor", "origin/main", "HEAD"]);
}

#[tokio::test]
async fn claude_uuid_follows_the_conversation_after_clear() {
    need_git!();
    let fx = Fx::new();
    let (w, item) = started(&fx, "SHOP-141").await;
    // `/clear` in Claude: the next hook carries a new session_id.
    let ev = kelta_proto::events::BusEvent::new(
        kelta_proto::events::bus::CLAUDE_HOOK,
        serde_json::json!({ "event": "SessionStart", "payload": { "session_id": "after-clear" } }),
    )
    .with_session(claude(&fx));
    kelta_proto::api::CoreApi::publish(&*fx.core, ev);
    for _ in 0..300 {
        if w.list(None).await.unwrap()[0].claude_uuid.as_deref() == Some("after-clear") {
            return;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    panic!("claude_uuid still {:?}", item.claude_uuid);
}

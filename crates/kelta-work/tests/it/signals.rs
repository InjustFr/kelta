//! Work item signals and status (FLOW §2.3, §3.6): `review_due` / `claude_replied` from hooks,
//! field-level writes, `claude_uuid` from hooks (B3), `work_status_all` against `<remote>/<base>` (B4).

use std::sync::Arc;
use std::time::Duration;

use crate::common::{Fx, git, has_git, project};
use kelta_proto::api::{CoreApi, WorkStore};
use kelta_proto::codehost::PrDraft;
use kelta_proto::events::{BusEvent, bus};
use kelta_proto::ids::SessionId;
use kelta_proto::model::{FinishOpts, SessionKind, SessionStatus, WorkItem, WorkSource};
use kelta_proto::samples;
use kelta_work::WorkService;
use serde_json::json;

macro_rules! need_git {
    () => {
        if !has_git() {
            eprintln!("skipping: git not found");
            return;
        }
    };
}

fn hook(sid: &SessionId, event: &str, extra: serde_json::Value) -> BusEvent {
    let mut payload = json!({ "hook_event_name": event });
    if let (Some(p), Some(e)) = (payload.as_object_mut(), extra.as_object()) {
        p.extend(e.clone());
    }
    BusEvent::new(bus::CLAUDE_HOOK, json!({ "event": event, "payload": payload })).with_session(sid.clone())
}

async fn started(fx: &Fx) -> (Arc<WorkService>, WorkItem, SessionId) {
    let w = fx.service();
    let mut t = samples::ticket_ref();
    t.key = "SHOP-141".into();
    let plan = w.plan(&project(), WorkSource::Ticket { ticket: t }).await.unwrap();
    let item = w.start(plan).await.unwrap();
    let claude = fx.spawned_of(|k| *k == SessionKind::Claude)[0].id.clone();
    (w, item, claude)
}

/// Polls the store (test-only) until `pred` holds.
async fn wait_item(fx: &Fx, item: &WorkItem, pred: impl Fn(&WorkItem) -> bool) -> WorkItem {
    for _ in 0..500 {
        let cur = fx.store.get_item(&item.id).await.unwrap().unwrap();
        if pred(&cur) {
            return cur;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("work item never reached the expected state");
}

fn last_notification(fx: &Fx) -> String {
    fx.core.notifications().last().map(|n| n.title.clone()).unwrap_or_default()
}

#[tokio::test]
async fn hooks_set_and_clear_signals() {
    need_git!();
    let fx = Fx::new();
    let (w, item, claude) = started(&fx).await;

    // Stop without changes: Claude replied (a question), not "to review".
    fx.core.publish(hook(&claude, "Stop", json!({ "last_assistant_message": "A or B?" })));
    let cur = wait_item(&fx, &item, |w| w.claude_replied).await;
    assert!(!cur.review_due);
    assert_eq!(last_notification(&fx), "SHOP-141: Claude replied");

    // Louis answers: both cleared.
    fx.core.publish(hook(&claude, "UserPromptSubmit", json!({})));
    wait_item(&fx, &item, |w| !w.claude_replied).await;

    // Stop with changes (uncommitted): to review, with a notification.
    std::fs::write(item.worktree.join("login.rs"), "fn login() {}\n").unwrap();
    fx.core.publish(hook(&claude, "Stop", json!({})));
    let cur = wait_item(&fx, &item, |w| w.review_due).await;
    assert!(!cur.claude_replied);
    assert_eq!(last_notification(&fx), "SHOP-141 ready to review");

    // Mark reviewed clears review_due only.
    let out = w.mark_reviewed(&item.id).await.unwrap();
    assert!(!out.review_due);

    // Heuristic status and hooks of other sessions never set a flag.
    fx.core.publish(
        BusEvent::new(bus::SESSION_STATUS_CHANGED, json!({ "status": "done", "source": "heuristic" }))
            .with_session(claude.clone()),
    );
    let editor = fx.spawned_of(|k| matches!(k, SessionKind::Editor { .. }))[0].id.clone();
    fx.core.publish(hook(&editor, "Stop", json!({})));
    // B3: any hook carrying a new Claude session id moves claude_uuid (after /clear, /resume).
    fx.core.publish(hook(&claude, "SessionStart", json!({ "session_id": "after-clear", "source": "clear" })));
    let cur = wait_item(&fx, &item, |w| w.claude_uuid.as_deref() == Some("after-clear")).await;
    assert!(!cur.review_due && !cur.claude_replied);

    // Finish clears both (a new change: login.rs was reviewed).
    std::fs::write(item.worktree.join("signup.rs"), "fn signup() {}\n").unwrap();
    fx.core.publish(hook(&claude, "Stop", json!({})));
    wait_item(&fx, &item, |w| w.review_due).await;
    let opts = FinishOpts { remove_worktree: true, delete_branch: false, force: true, transition_to: None };
    let done = w.finish(&item.id, opts).await.unwrap();
    assert!(!done.review_due && !done.claude_replied);
}

#[tokio::test]
async fn claude_at_stamps_when_claude_asks_or_stops() {
    need_git!();
    let fx = Fx::new();
    let (_w, item, claude) = started(&fx).await;
    assert!(item.claude_at.is_none());
    // Idle reminders are not a question.
    fx.core.publish(hook(&claude, "Notification", json!({ "notification_type": "idle_prompt" })));
    fx.core.publish(hook(&claude, "SessionStart", json!({ "session_id": "s2" })));
    let cur = wait_item(&fx, &item, |w| w.claude_uuid.as_deref() == Some("s2")).await;
    assert!(cur.claude_at.is_none(), "hooks run in order: the idle prompt stamped nothing");
    fx.core.publish(hook(&claude, "Notification", json!({ "notification_type": "permission_prompt" })));
    let asked = wait_item(&fx, &item, |w| w.claude_at.is_some()).await.claude_at.unwrap();
    tokio::time::sleep(std::time::Duration::from_millis(1100)).await;
    fx.core.publish(hook(&claude, "Stop", json!({})));
    wait_item(&fx, &item, |w| w.claude_at.as_deref().is_some_and(|t| t > asked.as_str())).await;
}

#[tokio::test]
async fn stop_hook_during_push_survives_the_push_save() {
    need_git!();
    let fx = Fx::new();
    let (w, item, claude) = started(&fx).await;
    std::fs::write(item.worktree.join("login.rs"), "fn login() {}\n").unwrap();
    git(&item.worktree, &["add", "login.rs"]);
    git(&item.worktree, &["commit", "-q", "-m", "login"]);

    let (w2, id) = (w.clone(), item.id.clone());
    let pr = tokio::spawn(async move {
        w2.create_pr(&id, PrDraft::default(), kelta_proto::model::ShipOrigin::Mcp).await
    });
    let push = fx.wait_session(|s| s.name == "git push").await;
    fx.core.publish(hook(&claude, "Stop", json!({})));
    wait_item(&fx, &item, |w| w.review_due).await;
    fx.core.exit_session(&push, 0);

    let out = pr.await.unwrap().unwrap();
    assert!(out.pr_url.is_some());
    assert!(out.review_due, "the push's save kept the hook's review_due");
    wait_item(&fx, &item, |w| w.review_due && w.pr_url.is_some()).await;
}

#[tokio::test]
async fn status_all_compares_with_remote_base_after_first_push() {
    need_git!();
    let fx = Fx::new();
    let (w, item, _) = started(&fx).await;
    let wt = &item.worktree;

    // First push: the branch now has its own upstream.
    std::fs::write(wt.join("a.txt"), "one\ntwo\n").unwrap();
    git(wt, &["add", "a.txt"]);
    git(wt, &["commit", "-q", "-m", "a"]);
    git(wt, &["push", "-q", "-u", "origin", &item.branch]);
    // main moves on the remote.
    std::fs::write(fx.repo.join("b.txt"), "b\n").unwrap();
    git(&fx.repo, &["add", "b.txt"]);
    git(&fx.repo, &["commit", "-q", "-m", "b"]);
    git(&fx.repo, &["push", "-q", "origin", "main"]);
    git(&fx.repo, &["reset", "-q", "--hard", "HEAD~1"]);
    git(&fx.repo, &["update-ref", "refs/remotes/origin/main", "HEAD"]);

    let st = w.status_all().await.unwrap()[&item.id].clone();
    assert_eq!((st.ahead, st.behind), (1, 1), "behind is against origin/main, not the upstream (B4)");
    assert_eq!((st.files, st.insertions, st.deletions, st.dirty), (1, 2, 0, false));

    // Uncommitted work counts (merge base to working tree); within the floor, no new fetch.
    std::fs::write(wt.join("a.txt"), "one\n").unwrap();
    std::fs::write(wt.join("c.txt"), "c\n").unwrap();
    std::fs::write(fx.repo.join("d.txt"), "d\n").unwrap();
    git(&fx.repo, &["add", "d.txt"]);
    git(&fx.repo, &["commit", "-q", "-m", "d"]);
    git(&fx.repo, &["push", "-q", "origin", "HEAD:refs/heads/main", "--force"]);
    git(&fx.repo, &["update-ref", "refs/remotes/origin/main", "HEAD~1"]);
    let st = w.status_all().await.unwrap()[&item.id].clone();
    assert_eq!(st.behind, 0, "no second fetch within 5 minutes");
    assert_eq!((st.files, st.insertions, st.dirty), (2, 1, true));

    // Deleted outside Kelta: missing, not an error.
    std::fs::remove_dir_all(wt).unwrap();
    assert!(w.status_all().await.unwrap()[&item.id].missing);
}

#[tokio::test]
async fn prose_answer_after_a_commit_is_a_reply() {
    need_git!();
    let fx = Fx::new();
    let (w, item, claude) = started(&fx).await;
    std::fs::write(item.worktree.join("a.txt"), "a\n").unwrap();
    git(&item.worktree, &["add", "a.txt"]);
    git(&item.worktree, &["commit", "-q", "-m", "a"]);
    // Louis looked at the commit (never reviewed, the whole branch would still be to review).
    w.mark_reviewed(&item.id).await.unwrap();

    // "Why did you do X?": Claude answers without touching code, the branch stays ahead of base.
    fx.core.publish(hook(&claude, "UserPromptSubmit", json!({})));
    fx.core.publish(hook(&claude, "Stop", json!({})));
    let cur = wait_item(&fx, &item, |w| w.claude_replied).await;
    assert!(!cur.review_due);

    // A new commit after the next prompt is to review again.
    fx.core.publish(hook(&claude, "UserPromptSubmit", json!({})));
    wait_item(&fx, &item, |w| !w.claude_replied).await;
    std::fs::write(item.worktree.join("a.txt"), "b\n").unwrap();
    git(&item.worktree, &["commit", "-q", "-am", "b"]);
    fx.core.publish(hook(&claude, "Stop", json!({})));
    wait_item(&fx, &item, |w| w.review_due).await;
}

#[tokio::test]
async fn prompt_right_after_stop_wins() {
    need_git!();
    let fx = Fx::new();
    let (_w, item, claude) = started(&fx).await;
    std::fs::write(item.worktree.join("login.rs"), "fn login() {}\n").unwrap();

    fx.core.publish(hook(&claude, "Stop", json!({})));
    fx.core.publish(hook(&claude, "UserPromptSubmit", json!({})));
    // Hooks of one session apply in order: once this marker lands, both earlier hooks have.
    fx.core.publish(hook(&claude, "SessionStart", json!({ "session_id": "marker" })));
    let cur = wait_item(&fx, &item, |w| w.claude_uuid.as_deref() == Some("marker")).await;
    assert!(!cur.review_due && !cur.claude_replied);
}

/// Pushes a new commit to the remote's `refs/pull/87/head` (someone updated the PR).
fn move_pr_head(fx: &Fx, file: &str) {
    git(&fx.repo, &["fetch", "-q", "origin", "refs/pull/87/head"]);
    git(&fx.repo, &["checkout", "-q", "FETCH_HEAD"]);
    std::fs::write(fx.repo.join(file), "x\n").unwrap();
    git(&fx.repo, &["add", file]);
    git(&fx.repo, &["commit", "-q", "-m", file]);
    git(&fx.repo, &["push", "-q", "origin", "HEAD:refs/pull/87/head"]);
    git(&fx.repo, &["checkout", "-q", "main"]);
}

#[tokio::test]
async fn status_all_fast_forwards_a_clean_review_checkout_only() {
    need_git!();
    let fx = Fx::new();
    let w = fx.service();
    let plan = w.plan(&project(), WorkSource::Review { review: samples::review_ref() }).await.unwrap();
    let item = w.start(plan).await.unwrap();
    move_pr_head(&fx, "second.txt");
    w.status_all().await.unwrap();
    assert!(item.worktree.join("second.txt").exists(), "clean checkout follows the PR head");

    // Dirty: left alone (a fresh service, so the 5 min fetch floor does not hide the case).
    std::fs::write(item.worktree.join("feature.txt"), "my notes\n").unwrap();
    move_pr_head(&fx, "third.txt");
    fx.service().status_all().await.unwrap();
    assert!(!item.worktree.join("third.txt").exists());
    assert_eq!(std::fs::read_to_string(item.worktree.join("feature.txt")).unwrap(), "my notes\n");
}

#[tokio::test]
async fn status_all_leaves_a_review_checkout_alone_while_claude_works() {
    need_git!();
    let fx = Fx::new();
    let w = fx.service();
    let plan = w.plan(&project(), WorkSource::Review { review: samples::review_ref() }).await.unwrap();
    let item = w.start(plan).await.unwrap();
    let mut s = fx
        .core
        .sessions()
        .into_iter()
        .find(|s| item.session_ids.contains(&s.id) && s.kind == SessionKind::Claude)
        .expect("claude session");
    s.status = SessionStatus::Working;
    fx.core.insert_session(s);
    let head = git(&item.worktree, &["rev-parse", "HEAD"]);
    move_pr_head(&fx, "second.txt");
    w.status_all().await.unwrap();
    assert_eq!(git(&item.worktree, &["rev-parse", "HEAD"]), head, "HEAD stays while Claude reads it");
}

fn refs(fx: &Fx, item: &WorkItem) -> String {
    git(&fx.repo, &["for-each-ref", "--format=%(refname:short)", &format!("refs/kelta/wi/{}/", item.id)])
}

fn spawn_args(fx: &Fx) -> serde_json::Value {
    fx.core.calls().iter().rev().find(|c| c.method == "session_spawn").unwrap().args.clone()
}

#[tokio::test]
async fn ready_for_review_is_the_delta_since_the_last_look() {
    need_git!();
    let fx = Fx::new();
    let (w, item, claude) = started(&fx).await;
    let wt = item.worktree.clone();
    let last = format!("refs/kelta/wi/{}/last", item.id);
    let reviewed = format!("refs/kelta/wi/{}/reviewed", item.id);

    // Claude leaves uncommitted work: the snapshot holds it, the delta is its shape, the full message kept.
    std::fs::write(wt.join("login.rs"), "fn a() {}\nfn b() {}\n").unwrap();
    let long = "x".repeat(500);
    fx.core.publish(hook(&claude, "Stop", json!({ "last_assistant_message": long })));
    let cur = wait_item(&fx, &item, |w| w.review_due).await;
    let d = cur.delta.unwrap();
    assert_eq!((d.lines, d.files, d.tests, d.generated), (2, 1, 0, 0));
    assert_eq!(cur.claude_message.as_deref(), Some(long.as_str()), "not cut at 200 chars");
    assert_eq!(git(&wt, &["show", &format!("{last}:login.rs")]), "fn a() {}\nfn b() {}");

    // `v`: the delta range (shell without review_args), then in nvim.
    let s = w.diff(&item.id, true, None).await.unwrap();
    let base = git(&wt, &["merge-base", "origin/main", "HEAD"]);
    let tip = git(&wt, &["rev-parse", &last]);
    assert_eq!(fx.core.written_text(&s.id), format!("git diff {base} {tip}\r"));
    fx.settings(|s| s.editor.review_args = vec!["-c".into(), "DiffviewOpen {range}".into()]);
    w.diff(&item.id, true, None).await.unwrap();
    assert_eq!(spawn_args(&fx)["args"], json!(["-c", format!("DiffviewOpen {base}..{tip}")]));
    // `V`: the full diff keeps `<remote>/<base>`.
    w.diff(&item.id, false, None).await.unwrap();
    assert_eq!(spawn_args(&fx)["args"], json!(["-c", "DiffviewOpen origin/main"]));

    // `R`: reviewed = last, the row goes.
    let out = w.mark_reviewed(&item.id).await.unwrap();
    assert!(!out.review_due && out.delta.is_none());
    assert_eq!(git(&wt, &["rev-parse", &reviewed]), tip);
    let e = w.diff(&item.id, true, None).await.unwrap_err();
    assert!(e.message.contains("Nothing new"), "{}", e.message);

    // Done with an empty delta: a reply, never ready for review.
    fx.core.publish(hook(&claude, "UserPromptSubmit", json!({})));
    fx.core.publish(hook(&claude, "Stop", json!({})));
    let cur = wait_item(&fx, &item, |w| w.claude_replied).await;
    assert!(!cur.review_due && cur.delta.is_none());

    // Next round: only the new work, lockfiles counted apart, a test file counted.
    fx.core.publish(hook(&claude, "UserPromptSubmit", json!({})));
    std::fs::write(wt.join("Cargo.lock"), "a\nb\nc\n").unwrap();
    std::fs::create_dir_all(wt.join("tests")).unwrap();
    std::fs::write(wt.join("tests/login.rs"), "#[test]\nfn t() {}\n").unwrap();
    std::fs::write(wt.join(".gitattributes"), "api.gen.ts linguist-generated\n").unwrap();
    std::fs::write(wt.join("api.gen.ts"), "1\n2\n3\n4\n").unwrap();
    fx.core.publish(hook(&claude, "Stop", json!({})));
    let cur = wait_item(&fx, &item, |w| w.review_due).await;
    let d = cur.delta.unwrap();
    assert_eq!((d.lines, d.files, d.tests, d.generated), (3, 2, 1, 7));

    // Finish deletes both refs.
    assert_eq!(refs(&fx, &item).lines().count(), 2);
    let opts = FinishOpts { remove_worktree: true, delete_branch: false, force: true, transition_to: None };
    w.finish(&item.id, opts).await.unwrap();
    assert_eq!(refs(&fx, &item), "");
}

#[tokio::test]
async fn startup_prunes_refs_of_items_that_are_gone() {
    need_git!();
    let fx = Fx::new();
    let (w, item, _) = started(&fx).await;
    w.mark_reviewed(&item.id).await.unwrap();
    git(&fx.repo, &["update-ref", "refs/kelta/wi/gone/last", "HEAD"]);
    w.startup().await.unwrap();
    assert_eq!(git(&fx.repo, &["for-each-ref", "--format=%(refname)", "refs/kelta/wi/"]).lines().count(), 2);
    assert!(refs(&fx, &item).contains("reviewed"), "an unfinished item keeps its refs");
}

#[tokio::test]
async fn a_rebase_never_puts_upstream_code_in_the_delta() {
    need_git!();
    let fx = Fx::new();
    let (w, item, claude) = started(&fx).await;
    let wt = item.worktree.clone();
    std::fs::write(wt.join("login.rs"), "fn a() {}\n").unwrap();
    git(&wt, &["add", "-A"]);
    git(&wt, &["commit", "-qm", "login"]);
    fx.core.publish(hook(&claude, "Stop", json!({})));
    wait_item(&fx, &item, |w| w.review_due).await;
    w.mark_reviewed(&item.id).await.unwrap();

    // origin/main gains 500 lines; the branch is rebased onto it.
    git(&wt, &["checkout", "-q", "-b", "upstream", "origin/main"]);
    std::fs::write(wt.join("upstream.txt"), "x\n".repeat(500)).unwrap();
    git(&wt, &["add", "-A"]);
    git(&wt, &["commit", "-qm", "upstream"]);
    git(&wt, &["push", "-q", "origin", "upstream:main"]);
    git(&wt, &["checkout", "-q", "-"]);
    git(&wt, &["fetch", "-q", "origin"]);
    git(&wt, &["rebase", "-q", "origin/main"]);

    fx.core.publish(hook(&claude, "UserPromptSubmit", json!({})));
    std::fs::write(wt.join("signup.rs"), "fn s() {}\n").unwrap();
    fx.core.publish(hook(&claude, "Stop", json!({})));
    let d = wait_item(&fx, &item, |w| w.review_due).await.delta.unwrap();
    assert_eq!((d.lines, d.files), (2, 2), "the whole branch, no upstream lines");
}

//! GitLab code host: MR lists, version-gated draft/wip, approvals, sha approve (409), todos gate.

use crate::support::*;
use kelta_proto::codehost::{
    CiState, MyReviewState, PrCreate, PrState, ReviewDecision, ReviewKind, ReviewQuery,
};
use kelta_proto::error::ErrorCode;
use serde_json::json;
use wiremock::matchers::{body_partial_json, header, method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn gl(server: &MockServer) -> std::sync::Arc<dyn kelta_proto::api::CodeHost> {
    host("gitlab-acme", "gitlab", &server.uri())
}

async fn common(server: &MockServer, version: &str) {
    mount(server, "GET", "/api/v4/user", 200, "gitlab/user.json").await;
    mount(server, "GET", "/api/v4/version", 200, version).await;
}

#[tokio::test]
async fn review_requested_list_uses_reviewer_username_updated_after_and_draft_no() {
    let server = MockServer::start().await;
    common(&server, "gitlab/version_16.json").await;
    Mock::given(method("GET"))
        .and(path("/api/v4/merge_requests"))
        .and(query_param("scope", "all"))
        .and(query_param("state", "opened"))
        .and(query_param("reviewer_username", "louis"))
        .and(query_param("draft", "no"))
        .and(header("private-token", TOKEN))
        .respond_with(ResponseTemplate::new(200).set_body_string(fixture_text("gitlab/mrs_review.json")))
        .mount(&server)
        .await;
    // !8: I was asked on 09-29 and mine is the one approval left; !9 has neither endpoint (best effort).
    let reply = |body: serde_json::Value| ResponseTemplate::new(200).set_body_json(body);
    let mr8 = "/api/v4/projects/grp%2Fother/merge_requests/8";
    Mock::given(path(format!("{mr8}/reviewers")))
        .respond_with(reply(json!([
            { "user": { "id": 43, "username": "zed" }, "state": "reviewed", "created_at": "2026-09-28T10:00:00Z" },
            { "user": { "id": 42, "username": "louis" }, "state": "unreviewed", "created_at": "2026-09-29T10:00:00Z" },
        ])))
        .mount(&server)
        .await;
    Mock::given(path(format!("{mr8}/approvals")))
        .respond_with(reply(json!({
            "approvals_left": 1,
            "approved_by": [{ "user": { "id": 43 } }],
            "approvers": [],
            "suggested_approvers": [{ "id": 42, "username": "louis" }],
        })))
        .mount(&server)
        .await;
    Mock::given(path(format!("{mr8}/changes")))
        .respond_with(reply(
            json!({ "changes": [{ "new_path": "a.rs", "diff": "@@ -1 +1,2 @@\n-x\n+y\n+z\n" }] }),
        ))
        .expect(1)
        .mount(&server)
        .await;
    let h = gl(&server);
    let list = h.list_reviews(&query(ReviewKind::ReviewRequested, true, false)).await.unwrap();
    assert_eq!((list[0].additions, list[0].deletions), (Some(2), Some(1)), "size from /changes");
    assert_eq!(list[1].additions, None, "no /changes: no size");
    assert_eq!(list[0].requested_at.as_deref(), Some("2026-09-29T10:00:00Z"));
    assert!(list[0].blocking, "one approval left and I am an approver");
    assert!(list[1].requested_at.is_none() && !list[1].blocking);
    let again = h.list_reviews(&query(ReviewKind::ReviewRequested, true, false)).await.unwrap();
    assert_eq!(again[0].additions, Some(2), "same head: size from cache, /changes fetched once");
    let q = &queries(&server, "GET", "/api/v4/merge_requests").await[0];
    assert!(q.contains("updated_after=20"), "{q}");
    assert!(!q.contains("wip="));
    assert_eq!(list.iter().map(|r| r.r#ref.number).collect::<Vec<_>>(), vec![8, 9], "draft MR dropped");
    let a = &list[0];
    assert_eq!(a.r#ref.repo, "grp/other");
    assert_eq!(a.ci, CiState::Success);
    assert_eq!(a.decision, Some(ReviewDecision::ReviewRequired));
    assert_eq!(a.my_state, Some(MyReviewState::Pending));
    assert_eq!(a.head_sha, "deadbeef08");
    assert_eq!(a.linked_tickets, vec!["#12"]);
    assert_eq!(a.labels, vec!["bug"]);
    assert_eq!(a.author.login.as_deref(), Some("dave"));
    assert_eq!(list[1].mergeable, Some(false));
    assert_eq!(list[1].ci, CiState::Failure);

    // the version is cached: a second list does not call /version again
    h.list_reviews(&query(ReviewKind::ReviewRequested, true, false)).await.unwrap();
    assert_eq!(count(&server, "GET", "/api/v4/version").await, 1);
}

#[tokio::test]
async fn old_servers_get_wip_instead_of_draft() {
    let server = MockServer::start().await;
    common(&server, "gitlab/version_15.json").await;
    mount(&server, "GET", "/api/v4/merge_requests", 200, "gitlab/mrs_review.json").await;
    gl(&server).list_reviews(&query(ReviewKind::ReviewRequested, true, false)).await.unwrap();
    let q = &queries(&server, "GET", "/api/v4/merge_requests").await[0];
    assert!(q.contains("wip=no") && !q.contains("draft="), "{q}");
}

#[tokio::test]
async fn including_drafts_sends_neither_filter() {
    let server = MockServer::start().await;
    common(&server, "gitlab/version_16.json").await;
    mount(&server, "GET", "/api/v4/merge_requests", 200, "gitlab/mrs_review.json").await;
    let list = gl(&server).list_reviews(&query(ReviewKind::ReviewRequested, true, true)).await.unwrap();
    assert_eq!(list.len(), 3);
    assert!(list[0].draft);
    let q = &queries(&server, "GET", "/api/v4/merge_requests").await[0];
    assert!(!q.contains("draft=") && !q.contains("wip="), "{q}");
    assert_eq!(count(&server, "GET", "/api/v4/version").await, 0);
}

#[tokio::test]
async fn authored_list_uses_created_by_me() {
    let server = MockServer::start().await;
    common(&server, "gitlab/version_16.json").await;
    Mock::given(path("/api/v4/merge_requests"))
        .and(query_param("scope", "created_by_me"))
        .respond_with(ResponseTemplate::new(200).set_body_string(fixture_text("gitlab/mrs_authored.json")))
        .expect(1)
        .mount(&server)
        .await;
    let list = gl(&server).list_reviews(&query(ReviewKind::Authored, true, false)).await.unwrap();
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].kind, ReviewKind::Authored);
    assert_eq!(list[0].my_state, None);
    assert_eq!(list[0].ci, CiState::Pending);
}

const MR: &str = "/api/v4/projects/grp%2Fother/merge_requests/8";

async fn mount_detail(server: &MockServer, mr: &str, approvals: &str) {
    mount(server, "GET", "/api/v4/user", 200, "gitlab/user.json").await;
    mount(server, "GET", &format!("{MR}/draft_notes"), 200, "gitlab/draft_notes.json").await;
    mount(server, "GET", MR, 200, mr).await;
    mount(server, "GET", &format!("{MR}/approvals"), 200, approvals).await;
    mount(server, "GET", &format!("{MR}/changes"), 200, "gitlab/changes.json").await;
    mount(server, "GET", "/api/v4/projects/grp%2Fother/pipelines/908/jobs", 200, "gitlab/jobs.json").await;
}

#[tokio::test]
async fn detail_with_approvals_changes_and_jobs() {
    let server = MockServer::start().await;
    mount_detail(&server, "gitlab/mr.json", "gitlab/approvals.json").await;
    let d = gl(&server).get(&rref("gitlab-acme", "grp/other", 8)).await.unwrap();
    assert_eq!(d.review.decision, Some(ReviewDecision::ReviewRequired));
    assert_eq!(d.review.my_state, Some(MyReviewState::Pending));
    assert_eq!(d.review.kind, ReviewKind::ReviewRequested);
    assert_eq!((d.review.additions, d.review.deletions), (Some(3), Some(2)));
    assert_eq!(
        d.files.iter().map(|f| (f.path.as_str(), f.additions, f.deletions)).collect::<Vec<_>>(),
        vec![("a.rs", 2, 1), ("docs/b.md", 1, 1)]
    );
    assert_eq!(
        d.checks.iter().map(|c| c.state).collect::<Vec<_>>(),
        vec![CiState::Success, CiState::Failure, CiState::Pending]
    );
    let zed = d.reviewers.iter().find(|r| r.user.login.as_deref() == Some("zed")).unwrap();
    assert_eq!(zed.state, Some(MyReviewState::Approved));
    assert!(d.body_html.contains("<strong>thing</strong>"));
}

#[tokio::test]
async fn detail_when_i_approved_or_authored() {
    let server = MockServer::start().await;
    mount_detail(&server, "gitlab/mr.json", "gitlab/approvals_me.json").await;
    let d = gl(&server).get(&rref("gitlab-acme", "grp/other", 8)).await.unwrap();
    assert_eq!(d.review.my_state, Some(MyReviewState::Approved));
    assert_eq!(d.review.decision, Some(ReviewDecision::Approved));

    let server = MockServer::start().await;
    mount_detail(&server, "gitlab/mr_mine.json", "gitlab/approvals.json").await;
    let d = gl(&server).get(&rref("gitlab-acme", "grp/other", 8)).await.unwrap();
    assert_eq!(d.review.kind, ReviewKind::Authored);
    assert_eq!(d.review.my_state, None);
    assert_eq!(d.state, PrState::Merged);
}

#[tokio::test]
async fn detail_survives_missing_approvals_and_changes() {
    let server = MockServer::start().await;
    mount(&server, "GET", "/api/v4/user", 200, "gitlab/user.json").await;
    mount(&server, "GET", MR, 200, "gitlab/mr.json").await;
    Mock::given(path(format!("{MR}/approvals")))
        .respond_with(ResponseTemplate::new(403))
        .mount(&server)
        .await;
    Mock::given(path(format!("{MR}/changes"))).respond_with(ResponseTemplate::new(404)).mount(&server).await;
    Mock::given(path("/api/v4/projects/grp%2Fother/pipelines/908/jobs"))
        .respond_with(ResponseTemplate::new(403))
        .mount(&server)
        .await;
    let d = gl(&server).get(&rref("gitlab-acme", "grp/other", 8)).await.unwrap();
    assert!(d.files.is_empty() && d.checks.is_empty());
    assert_eq!(d.review.additions, None);
}

#[tokio::test]
async fn approve_sends_the_sha_and_a_409_is_a_conflict() {
    let server = MockServer::start().await;
    mount(&server, "GET", &format!("{MR}/draft_notes"), 200, "gitlab/no_drafts.json").await;
    Mock::given(method("POST"))
        .and(path(format!("{MR}/approve")))
        .and(body_partial_json(json!({"sha": "deadbeef08"})))
        .respond_with(ResponseTemplate::new(201).set_body_string(fixture_text("gitlab/approve_ok.json")))
        .expect(1)
        .mount(&server)
        .await;
    gl(&server).approve(&rref("gitlab-acme", "grp/other", 8), "deadbeef08").await.unwrap();

    let moved = MockServer::start().await;
    mount(&moved, "GET", &format!("{MR}/draft_notes"), 200, "gitlab/no_drafts.json").await;
    Mock::given(path(format!("{MR}/approve")))
        .respond_with(
            ResponseTemplate::new(409)
                .set_body_string("{\"message\":\"SHA does not match HEAD of source branch\"}"),
        )
        .mount(&moved)
        .await;
    let e = gl(&moved).approve(&rref("gitlab-acme", "grp/other", 8), "stale").await.unwrap_err();
    assert_eq!(e.code, ErrorCode::Conflict);
    assert_eq!(bodies(&moved, "POST", &format!("{MR}/approve")).await[0], json!({"sha": "stale"}));
}

#[tokio::test]
async fn request_changes_is_a_note_plus_unapprove_that_may_fail() {
    let server = MockServer::start().await;
    mount(&server, "GET", &format!("{MR}/draft_notes"), 200, "gitlab/no_drafts.json").await;
    Mock::given(method("POST"))
        .and(path(format!("{MR}/notes")))
        .respond_with(ResponseTemplate::new(201).set_body_string(fixture_text("gitlab/note.json")))
        .expect(2)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path(format!("{MR}/unapprove")))
        .respond_with(ResponseTemplate::new(404))
        .expect(1)
        .mount(&server)
        .await;
    let h = gl(&server);
    let r = rref("gitlab-acme", "grp/other", 8);
    h.request_changes(&r, "Needs tests").await.unwrap();
    h.comment(&r, "FYI").await.unwrap();
    assert_eq!(bodies(&server, "POST", &format!("{MR}/notes")).await[0], json!({"body": "Needs tests"}));
}

#[tokio::test]
async fn create_prefixes_draft_and_maps_duplicates_to_conflict() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/api/v4/projects/grp%2Fsub%2Fproj/merge_requests"))
        .and(body_partial_json(json!({"source_branch": "feature/SHOP-142-limit", "target_branch": "main", "title": "Draft: SHOP-142 add limiter", "description": "body"})))
        .respond_with(ResponseTemplate::new(201).set_body_string(fixture_text("gitlab/create_mr.json")))
        .expect(1)
        .mount(&server)
        .await;
    let d = PrCreate {
        repo: "grp/sub/proj".into(),
        head: "feature/SHOP-142-limit".into(),
        base: "main".into(),
        title: "SHOP-142 add limiter".into(),
        body: "body".into(),
        draft: true,
    };
    let r = gl(&server).create(&d).await.unwrap();
    assert_eq!(r.r#ref.number, 30);
    assert_eq!(r.r#ref.repo, "grp/sub/proj");
    assert!(r.draft);
    assert_eq!(r.kind, ReviewKind::Authored);

    let dup = MockServer::start().await;
    Mock::given(path("/api/v4/projects/grp%2Fsub%2Fproj/merge_requests"))
        .respond_with(ResponseTemplate::new(409).set_body_string(
            "{\"message\":[\"Another open merge request already exists for this source branch: !21\"]}",
        ))
        .mount(&dup)
        .await;
    assert_eq!(gl(&dup).create(&d).await.unwrap_err().code, ErrorCode::Conflict);
}

#[tokio::test]
async fn find_for_branch_by_source_branch() {
    let server = MockServer::start().await;
    mount(&server, "GET", "/api/v4/user", 200, "gitlab/user.json").await;
    Mock::given(path("/api/v4/projects/grp%2Fsub%2Fproj/merge_requests"))
        .and(query_param("source_branch", "feature/mine"))
        .and(query_param("state", "opened"))
        .respond_with(ResponseTemplate::new(200).set_body_string(fixture_text("gitlab/mrs_for_branch.json")))
        .mount(&server)
        .await;
    Mock::given(path("/api/v4/projects/grp%2Fsub%2Fproj/merge_requests"))
        .respond_with(ResponseTemplate::new(200).set_body_string("[]"))
        .mount(&server)
        .await;
    let h = gl(&server);
    let found = h.find_for_branch("grp/sub/proj", "feature/mine").await.unwrap().unwrap();
    assert_eq!(found.r#ref.number, 21);
    assert_eq!(found.kind, ReviewKind::Authored);
    assert!(h.find_for_branch("grp/sub/proj", "nope").await.unwrap().is_none());
}

#[tokio::test]
async fn todos_gate_stays_changed_until_list_reviews_succeeds() {
    let server = MockServer::start().await;
    common(&server, "gitlab/version_16.json").await;
    Mock::given(path("/api/v4/todos"))
        .respond_with(ResponseTemplate::new(200).set_body_string(fixture_text("gitlab/todos.json")))
        .up_to_n_times(1)
        .with_priority(1)
        .mount(&server)
        .await;
    Mock::given(path("/api/v4/todos"))
        .respond_with(ResponseTemplate::new(200).set_body_string(fixture_text("gitlab/todos_new.json")))
        .with_priority(2)
        .mount(&server)
        .await;
    Mock::given(path("/api/v4/merge_requests"))
        .respond_with(ResponseTemplate::new(500))
        .up_to_n_times(1)
        .with_priority(1)
        .mount(&server)
        .await;
    Mock::given(path("/api/v4/merge_requests"))
        .respond_with(ResponseTemplate::new(200).set_body_string("[]"))
        .with_priority(2)
        .mount(&server)
        .await;
    let h = gl(&server);
    let q = ReviewQuery { kind: ReviewKind::ReviewRequested, include_drafts: false, include_team: false };
    assert!(h.changed_since_last().await.unwrap(), "first poll");
    assert!(h.changed_since_last().await.unwrap(), "still pending: list not done");
    assert!(h.list_reviews(&q).await.is_err());
    assert!(h.changed_since_last().await.unwrap(), "the list failed, change not lost");
    h.list_reviews(&q).await.unwrap();
    assert!(!h.changed_since_last().await.unwrap());
}

#[tokio::test]
async fn a_forbidden_todos_endpoint_disables_the_gate() {
    let server = MockServer::start().await;
    Mock::given(path("/api/v4/todos"))
        .respond_with(ResponseTemplate::new(403))
        .expect(1)
        .mount(&server)
        .await;
    let h = gl(&server);
    assert!(h.changed_since_last().await.unwrap());
    assert!(h.changed_since_last().await.unwrap());
}

#[test]
fn refspec_and_remotes() {
    let h = host("gitlab-acme", "gitlab", "https://gitlab.acme.example");
    assert_eq!(h.fetch_refspec(&rref("a", "grp/proj", 8), "review/8"), "merge-requests/8/head:review/8");
    for (url, want) in [
        ("git@gitlab.acme.example:grp/sub/proj.git", Some("grp/sub/proj")),
        ("https://gitlab.acme.example/grp/proj", Some("grp/proj")),
        ("ssh://git@gitlab.acme.example:2222/grp/sub/proj.git", Some("grp/sub/proj")),
        ("https://gitlab.com/grp/proj.git", None),
        ("https://gitlab.acme.example/proj", None),
    ] {
        assert_eq!(h.repo_from_remote(url).as_deref(), want, "{url}");
    }
    let com = host("gitlab-com", "gitlab", "https://gitlab.com");
    assert_eq!(com.repo_from_remote("git@gitlab.com:a/b.git").as_deref(), Some("a/b"));
}

#[tokio::test]
async fn unauthorized_is_needs_auth() {
    let server = MockServer::start().await;
    Mock::given(path("/api/v4/user")).respond_with(ResponseTemplate::new(401)).mount(&server).await;
    assert_eq!(gl(&server).me().await.unwrap_err().code, ErrorCode::NeedsAuth);
}

#[tokio::test]
async fn blocking_discussions_and_requested_changes_mean_changes_requested() {
    let server = MockServer::start().await;
    mount_detail(&server, "gitlab/mr_blocked.json", "gitlab/approvals.json").await;
    let d = gl(&server).get(&rref("gitlab-acme", "grp/other", 8)).await.unwrap();
    assert_eq!(d.review.decision, Some(ReviewDecision::ChangesRequested), "approvals never override it");
    assert_eq!(d.review.decision_head.as_deref(), Some("deadbeef08"));

    let server = MockServer::start().await;
    mount_detail(&server, "gitlab/mr.json", "gitlab/approvals.json").await;
    mount(&server, "GET", &format!("{MR}/reviewers"), 200, "gitlab/reviewers_requested_changes.json").await;
    let d = gl(&server).get(&rref("gitlab-acme", "grp/other", 8)).await.unwrap();
    assert_eq!(d.review.decision, Some(ReviewDecision::ChangesRequested), "GitLab 17 reviewer state");
    let zed = d.reviewers.iter().find(|r| r.user.login.as_deref() == Some("zed")).unwrap();
    assert_eq!(zed.state, Some(MyReviewState::ChangesRequested));
}

#[tokio::test]
async fn feedback_unresolved_discussions_and_failed_job_traces() {
    let server = MockServer::start().await;
    mount_detail(&server, "gitlab/mr.json", "gitlab/approvals.json").await;
    mount(&server, "GET", &format!("{MR}/discussions"), 200, "gitlab/discussions.json").await;
    Mock::given(method("GET"))
        .and(path("/api/v4/projects/grp%2Fother/jobs/901/trace"))
        .respond_with(ResponseTemplate::new(200).set_body_string("compiling\nerror: test failed\n"))
        .mount(&server)
        .await;
    let f = gl(&server).feedback(&rref("gitlab-acme", "grp/other", 8)).await.unwrap();
    assert_eq!(f.threads.len(), 1, "resolved, non-resolvable and system notes are dropped");
    let t = &f.threads[0];
    assert_eq!((t.id.as_str(), t.author.as_str()), ("d-open", "zed"));
    assert_eq!((t.path.as_deref(), t.line), (Some("a.rs"), Some(12)));
    assert_eq!(t.body_md, "zed: Use a constant here.\n\nlouis: Agreed.");
    assert_eq!(t.url, "https://gitlab.acme.test/grp/other/-/merge_requests/8#note_301");
    assert_eq!(f.reviewers, vec!["zed"], "me excluded");
    assert_eq!(f.failed_checks.len(), 1);
    assert_eq!(f.failed_checks[0].name, "test");
    assert_eq!(f.failed_checks[0].log_tail.as_deref(), Some("compiling\nerror: test failed"));
}

#[tokio::test]
async fn feedback_refused_names_the_missing_scope() {
    let server = MockServer::start().await;
    mount(&server, "GET", "/api/v4/user", 200, "gitlab/user.json").await;
    Mock::given(path(MR)).respond_with(ResponseTemplate::new(403)).mount(&server).await;
    Mock::given(path(format!("{MR}/discussions")))
        .respond_with(ResponseTemplate::new(403))
        .mount(&server)
        .await;
    let e = gl(&server).feedback(&rref("gitlab-acme", "grp/other", 8)).await.unwrap_err();
    assert_eq!(e.code, ErrorCode::PermissionDenied);
    assert_eq!(e.message, "GitLab refused the review discussions (403: token lacks `read_api`).");
}

#[tokio::test]
async fn rerequest_posts_the_quick_action_and_resolve_puts_each_discussion() {
    let server = MockServer::start().await;
    mount(&server, "GET", "/api/v4/user", 200, "gitlab/user.json").await;
    mount(&server, "GET", MR, 200, "gitlab/mr.json").await;
    mount(&server, "POST", &format!("{MR}/notes"), 201, "gitlab/note.json").await;
    let h = gl(&server);
    let who = h.rerequest_review(&rref("gitlab-acme", "grp/other", 8)).await.unwrap();
    assert_eq!(who, vec!["zed"]);
    assert_eq!(bodies(&server, "POST", &format!("{MR}/notes")).await[0]["body"], "/request_review @zed");

    Mock::given(method("PUT"))
        .and(path(format!("{MR}/discussions/d-open")))
        .and(query_param("resolved", "true"))
        .respond_with(ResponseTemplate::new(200).set_body_string("{}"))
        .expect(1)
        .mount(&server)
        .await;
    h.resolve_threads(&rref("gitlab-acme", "grp/other", 8), &["d-open".into()]).await.unwrap();
}

#[tokio::test]
async fn draft_notes_are_the_pending_review() {
    let server = MockServer::start().await;
    mount_detail(&server, "gitlab/mr.json", "gitlab/approvals.json").await;
    let r = rref("gitlab-acme", "grp/other", 8);
    assert_eq!(gl(&server).get(&r).await.unwrap().pending_comments, 2);

    // a line comment becomes a draft note positioned on the MR diff refs
    mount(&server, "POST", &format!("{MR}/draft_notes"), 201, "gitlab/note.json").await;
    gl(&server).add_pending_comment(&r, "src/a.rs", 12, "nit").await.unwrap();
    let body = &bodies(&server, "POST", &format!("{MR}/draft_notes")).await[0];
    assert_eq!(body["note"], "nit");
    assert_eq!(
        body["position"],
        json!({"position_type": "text", "base_sha": "b0", "start_sha": "s0", "head_sha": "h0",
               "new_path": "src/a.rs", "old_path": "src/a.rs", "new_line": 12})
    );
}

#[tokio::test]
async fn decisions_publish_the_drafts_first_and_an_empty_comment_sends_no_note() {
    let server = MockServer::start().await;
    mount(&server, "GET", &format!("{MR}/draft_notes"), 200, "gitlab/draft_notes.json").await;
    mount(&server, "POST", &format!("{MR}/draft_notes/bulk_publish"), 204, "gitlab/note.json").await;
    mount(&server, "POST", &format!("{MR}/approve"), 201, "gitlab/approve_ok.json").await;
    let h = gl(&server);
    let r = rref("gitlab-acme", "grp/other", 8);
    h.approve(&r, "deadbeef08").await.unwrap();
    h.comment(&r, "").await.unwrap();
    assert_eq!(count(&server, "POST", &format!("{MR}/draft_notes/bulk_publish")).await, 2);
    assert_eq!(count(&server, "POST", &format!("{MR}/notes")).await, 0);
}

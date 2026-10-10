//! GitHub code host: aliased GraphQL search, notifications gate, REST review actions, GHE.

mod support;

use std::time::Duration;

use kelta_proto::codehost::{CiState, MyReviewState, PrCreate, ReviewDecision, ReviewKind};
use kelta_proto::error::ErrorCode;
use serde_json::{Value, json};
use support::*;
use wiremock::matchers::{body_partial_json, header, method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn gh(server: &MockServer) -> std::sync::Arc<dyn kelta_proto::api::CodeHost> {
    host("github-work", "github", &server.uri())
}

#[tokio::test]
async fn one_graphql_request_with_only_the_requested_alias() {
    let server = MockServer::start().await;
    mount(&server, "POST", "/graphql", 200, "github/gql_list.json").await;
    let h = gh(&server);
    let list = h.list_reviews(&query(ReviewKind::ReviewRequested, true, false)).await.unwrap();
    let reqs = bodies(&server, "POST", "/graphql").await;
    assert_eq!(reqs.len(), 1, "one request per kind");
    let q = reqs[0]["query"].as_str().unwrap();
    assert!(q.contains("reviewRequested: search") && !q.contains("authored: search"));
    assert_eq!(reqs[0]["variables"]["s"], "is:pr is:open review-requested:@me archived:false draft:false");

    assert_eq!(
        list.iter().map(|r| r.r#ref.number).collect::<Vec<_>>(),
        vec![101, 103],
        "drafts and empty nodes are dropped"
    );
    let a = &list[0];
    assert_eq!(a.r#ref.repo, "acme/shop");
    assert_eq!(a.kind, ReviewKind::ReviewRequested);
    assert_eq!(a.my_state, Some(MyReviewState::Pending));
    assert_eq!(a.decision, Some(ReviewDecision::ReviewRequired));
    assert_eq!(a.ci, CiState::Success);
    assert_eq!(a.head_sha, "sha101");
    assert_eq!(a.source_branch, "feature/SHOP-142-limit");
    assert_eq!(a.linked_tickets, vec!["SHOP-142"]);
    assert_eq!(a.author.login.as_deref(), Some("carol"));
    assert_eq!(a.mergeable, Some(true));
    assert!(a.additions.is_some() && a.deletions.is_some());
    let b = &list[1];
    assert_eq!(b.my_state, Some(MyReviewState::Approved));
    assert_eq!(b.ci, CiState::Pending, "EXPECTED counts as pending");
    assert_eq!(b.mergeable, Some(false));
    assert_eq!(b.linked_tickets, vec!["#12"]);
}

#[tokio::test]
async fn direct_requests_only_and_drafts_on_demand() {
    let server = MockServer::start().await;
    mount(&server, "POST", "/graphql", 200, "github/gql_list.json").await;
    let h = gh(&server);
    let list = h.list_reviews(&query(ReviewKind::ReviewRequested, false, true)).await.unwrap();
    assert_eq!(list.iter().map(|r| r.r#ref.number).collect::<Vec<_>>(), vec![101, 102, 103]);
    let v = &bodies(&server, "POST", "/graphql").await[0]["variables"];
    assert_eq!(v["s"], "is:pr is:open user-review-requested:@me archived:false");
}

#[tokio::test]
async fn authored_list() {
    let server = MockServer::start().await;
    mount(&server, "POST", "/graphql", 200, "github/gql_list.json").await;
    let list = gh(&server).list_reviews(&query(ReviewKind::Authored, true, false)).await.unwrap();
    assert_eq!(list.len(), 1);
    let r = &list[0];
    assert_eq!(r.kind, ReviewKind::Authored);
    assert_eq!(r.ci, CiState::Failure);
    assert_eq!(r.decision, Some(ReviewDecision::ChangesRequested));
    assert_eq!(r.my_state, None);
    assert_eq!(r.mergeable, None);
}

#[tokio::test]
async fn ghe_uses_its_own_graphql_endpoint() {
    let server = MockServer::start().await;
    Mock::given(path("/api/graphql"))
        .respond_with(ResponseTemplate::new(200).set_body_string(fixture_text("github/gql_list.json")))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(path("/api/v3/user"))
        .respond_with(ResponseTemplate::new(200).set_body_string(fixture_text("github/user.json")))
        .expect(1)
        .mount(&server)
        .await;
    let h = host("ghe", "github", &format!("{}/api/v3", server.uri()));
    h.list_reviews(&query(ReviewKind::Authored, true, false)).await.unwrap();
    assert_eq!(h.me().await.unwrap().login.as_deref(), Some("louis"));
}

#[tokio::test(start_paused = true)]
async fn notifications_gate_uses_etag_and_honours_poll_interval_for_classic_tokens() {
    let server = MockServer::start().await;
    // 3rd request: unchanged → 304. 4th request: changed.
    Mock::given(path("/notifications"))
        .and(query_param("participating", "true"))
        .and(header("if-none-match", "\"n2\""))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("etag", "\"n3\"")
                .insert_header("x-poll-interval", "60")
                .set_body_string("[{}]"),
        )
        .up_to_n_times(1)
        .with_priority(1)
        .mount(&server)
        .await;
    Mock::given(path("/notifications"))
        .and(header("if-none-match", "\"n1\""))
        .respond_with(ResponseTemplate::new(304).insert_header("x-poll-interval", "60"))
        .up_to_n_times(1)
        .with_priority(2)
        .mount(&server)
        .await;
    Mock::given(path("/notifications"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("etag", "\"n1\"")
                .insert_header("x-poll-interval", "60")
                .set_body_string(fixture_text("github/notifications.json")),
        )
        .up_to_n_times(1)
        .with_priority(3)
        .mount(&server)
        .await;
    let (h, _) = host_with("github-work", "github", &server.uri(), "ghp_classictoken");
    assert!(h.changed_since_last().await.unwrap(), "first poll: nothing cached, so changed");
    assert_eq!(count(&server, "GET", "/notifications").await, 1);

    // the follow-up list has not succeeded yet: the change stays pending, still no request
    assert!(h.changed_since_last().await.unwrap());
    assert_eq!(count(&server, "GET", "/notifications").await, 1);
    mount(&server, "POST", "/graphql", 200, "github/gql_list.json").await;
    h.list_reviews(&query(ReviewKind::ReviewRequested, true, false)).await.unwrap();

    // within X-Poll-Interval: no request at all
    tokio::time::advance(Duration::from_secs(30)).await;
    assert!(!h.changed_since_last().await.unwrap());
    assert_eq!(count(&server, "GET", "/notifications").await, 1);

    // after the interval: conditional request, 304 → unchanged
    tokio::time::advance(Duration::from_secs(31)).await;
    assert!(!h.changed_since_last().await.unwrap());
    assert_eq!(count(&server, "GET", "/notifications").await, 2);
    let reqs: Vec<_> = server
        .received_requests()
        .await
        .unwrap()
        .into_iter()
        .filter(|r| r.url.path() == "/notifications")
        .collect();
    assert_eq!(reqs[1].headers.get("if-none-match").unwrap().to_str().unwrap(), "\"n1\"");
}

#[tokio::test]
async fn fine_grained_tokens_skip_the_gate_entirely() {
    let server = MockServer::start().await;
    Mock::given(path("/notifications"))
        .respond_with(ResponseTemplate::new(200))
        .expect(0)
        .mount(&server)
        .await;
    for tok in ["github_pat_11ABC", "ghs_app", "ghu_user"] {
        let (h, _) = host_with("github-work", "github", &server.uri(), tok);
        assert!(h.changed_since_last().await.unwrap(), "{tok}");
        assert!(h.changed_since_last().await.unwrap(), "{tok}");
    }
}

#[tokio::test]
async fn a_forbidden_gate_is_disabled_for_good() {
    let server = MockServer::start().await;
    Mock::given(path("/notifications"))
        .respond_with(ResponseTemplate::new(403).set_body_string("scope"))
        .expect(1)
        .mount(&server)
        .await;
    let (h, _) = host_with("github-work", "github", &server.uri(), "ghp_noscope");
    assert!(h.changed_since_last().await.unwrap());
    assert!(h.changed_since_last().await.unwrap());
}

async fn mount_detail(server: &MockServer, pull: &str, reviews: &str, runs: &str) {
    mount(server, "GET", "/user", 200, "github/user.json").await;
    mount(server, "GET", "/repos/acme/shop/pulls/101", 200, pull).await;
    mount(server, "GET", "/repos/acme/shop/pulls/101/reviews", 200, reviews).await;
    mount(server, "GET", "/repos/acme/shop/pulls/101/files", 200, "github/files.json").await;
    mount(server, "GET", "/repos/acme/shop/commits/abc123/check-runs", 200, runs).await;
    mount(server, "GET", "/repos/acme/shop/commits/abc123/status", 200, "github/combined_status.json").await;
}

#[tokio::test]
async fn detail_combines_reviews_checks_and_files() {
    let server = MockServer::start().await;
    mount_detail(&server, "github/pull.json", "github/reviews.json", "github/check_runs.json").await;
    let d = gh(&server).get(&rref("github-work", "acme/shop", 101)).await.unwrap();
    assert_eq!(d.review.kind, ReviewKind::ReviewRequested);
    assert_eq!(d.review.head_sha, "abc123");
    assert_eq!(d.review.ci, CiState::Pending, "one run still in progress");
    assert_eq!(d.review.decision, Some(ReviewDecision::ChangesRequested));
    assert_eq!(d.review.decision_head.as_deref(), Some("abc123"), "head of erin's change request");
    assert_eq!(d.review.my_state, Some(MyReviewState::Pending));
    assert_eq!(d.review.linked_tickets, vec!["SHOP-142"]);
    assert_eq!(
        d.checks.iter().map(|c| (c.name.as_str(), c.state)).collect::<Vec<_>>(),
        vec![
            ("build", CiState::Success),
            ("lint", CiState::Success),
            ("e2e", CiState::Pending),
            ("ci/legacy", CiState::Success)
        ]
    );
    assert_eq!(d.files.len(), 2);
    assert_eq!(d.files[0].path, "src/limiter.rs");
    let state = |login: &str| d.reviewers.iter().find(|r| r.user.id == login).and_then(|r| r.state);
    assert_eq!(
        state("erin"),
        Some(MyReviewState::ChangesRequested),
        "a later plain comment does not erase the change request"
    );
    assert_eq!(state("zed"), Some(MyReviewState::Commented));
    assert_eq!(state("louis"), Some(MyReviewState::Pending), "requested, PENDING draft review ignored");
    assert!(d.body_html.contains("<strong>limiter</strong>") && !d.body_html.contains("<script"));
}

#[tokio::test]
async fn detail_of_my_approved_and_failing_pull_requests() {
    let server = MockServer::start().await;
    mount_detail(
        &server,
        "github/pull.json",
        "github/reviews_approved.json",
        "github/check_runs_failed.json",
    )
    .await;
    let d = gh(&server).get(&rref("github-work", "acme/shop", 101)).await.unwrap();
    assert_eq!(d.review.my_state, Some(MyReviewState::Approved));
    assert_eq!(d.review.decision, Some(ReviewDecision::Approved));
    assert_eq!(d.review.ci, CiState::Failure);

    let server = MockServer::start().await;
    mount_detail(&server, "github/pull_mine.json", "github/reviews.json", "github/check_runs.json").await;
    let d = gh(&server).get(&rref("github-work", "acme/shop", 101)).await.unwrap();
    assert_eq!(d.review.kind, ReviewKind::Authored);
    assert_eq!(d.review.my_state, None);
}

#[tokio::test]
async fn approve_posts_the_review_with_commit_id_and_maps_a_stale_head_to_conflict() {
    let server = MockServer::start().await;
    mount(&server, "GET", "/repos/acme/shop/pulls/101/reviews", 200, "github/reviews.json").await;
    Mock::given(method("POST"))
        .and(path("/repos/acme/shop/pulls/101/reviews"))
        .and(body_partial_json(json!({"event": "APPROVE", "commit_id": "abc123"})))
        .respond_with(ResponseTemplate::new(200).set_body_string(fixture_text("github/review_posted.json")))
        .expect(1)
        .mount(&server)
        .await;
    let h = gh(&server);
    h.approve(&rref("github-work", "acme/shop", 101), "abc123").await.unwrap();

    let stale = MockServer::start().await;
    mount(&stale, "GET", "/repos/acme/shop/pulls/101/reviews", 200, "github/reviews.json").await;
    Mock::given(method("POST"))
        .and(path("/repos/acme/shop/pulls/101/reviews"))
        .respond_with(
            ResponseTemplate::new(422).set_body_string(fixture_text("github/error_stale_commit.json")),
        )
        .mount(&stale)
        .await;
    let e = gh(&stale).approve(&rref("github-work", "acme/shop", 101), "old").await.unwrap_err();
    assert_eq!(e.code, ErrorCode::Conflict);
}

#[tokio::test]
async fn comments_and_requested_changes() {
    let server = MockServer::start().await;
    mount(&server, "GET", "/repos/acme/shop/pulls/101/reviews", 200, "github/reviews.json").await;
    Mock::given(method("POST"))
        .and(path("/repos/acme/shop/issues/101/comments"))
        .respond_with(ResponseTemplate::new(201).set_body_string("{}"))
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/repos/acme/shop/pulls/101/reviews"))
        .respond_with(ResponseTemplate::new(200).set_body_string("{}"))
        .mount(&server)
        .await;
    let h = gh(&server);
    let r = rref("github-work", "acme/shop", 101);
    h.comment(&r, "LGTM modulo nits").await.unwrap();
    h.request_changes(&r, "Please add tests").await.unwrap();
    assert_eq!(
        bodies(&server, "POST", "/repos/acme/shop/issues/101/comments").await[0],
        json!({"body": "LGTM modulo nits"})
    );
    assert_eq!(
        bodies(&server, "POST", "/repos/acme/shop/pulls/101/reviews").await[0],
        json!({"event": "REQUEST_CHANGES", "body": "Please add tests"})
    );
}

#[tokio::test]
async fn create_and_duplicate_detection() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/repos/acme/shop/pulls"))
        .and(body_partial_json(json!({"title": "SHOP-142 add rate limiter", "head": "feature/SHOP-142-limit", "base": "main", "draft": true})))
        .respond_with(ResponseTemplate::new(201).set_body_string(fixture_text("github/create_pull.json")))
        .expect(1)
        .mount(&server)
        .await;
    let d = PrCreate {
        repo: "acme/shop".into(),
        head: "feature/SHOP-142-limit".into(),
        base: "main".into(),
        title: "SHOP-142 add rate limiter".into(),
        body: "body".into(),
        draft: true,
    };
    let r = gh(&server).create(&d).await.unwrap();
    assert_eq!(r.r#ref.number, 202);
    assert!(r.draft);
    assert_eq!(r.kind, ReviewKind::Authored);

    let dup = MockServer::start().await;
    Mock::given(path("/repos/acme/shop/pulls"))
        .respond_with(ResponseTemplate::new(422).set_body_string(fixture_text("github/error_pr_exists.json")))
        .mount(&dup)
        .await;
    assert_eq!(gh(&dup).create(&d).await.unwrap_err().code, ErrorCode::Conflict);
}

#[tokio::test]
async fn find_for_branch_filters_by_head() {
    let server = MockServer::start().await;
    mount(&server, "GET", "/user", 200, "github/user.json").await;
    Mock::given(path("/repos/acme/shop/pulls"))
        .and(query_param("head", "acme:feature/mine"))
        .and(query_param("state", "open"))
        .respond_with(
            ResponseTemplate::new(200).set_body_string(fixture_text("github/pulls_for_branch.json")),
        )
        .mount(&server)
        .await;
    Mock::given(path("/repos/acme/shop/pulls"))
        .respond_with(ResponseTemplate::new(200).set_body_string("[]"))
        .mount(&server)
        .await;
    let h = gh(&server);
    let found = h.find_for_branch("acme/shop", "feature/mine").await.unwrap().unwrap();
    assert_eq!(found.r#ref.number, 101);
    assert_eq!(found.kind, ReviewKind::Authored);
    assert!(h.find_for_branch("acme/shop", "nope").await.unwrap().is_none());
}

#[test]
fn refspec_and_remotes() {
    let h = host("github-work", "github", "https://api.github.com");
    assert_eq!(h.fetch_refspec(&rref("a", "acme/shop", 101), "review/101"), "pull/101/head:review/101");
    for (url, want) in [
        ("https://github.com/acme/shop.git", Some("acme/shop")),
        ("git@github.com:acme/shop.git", Some("acme/shop")),
        ("ssh://git@ssh.github.com:443/acme/shop.git", Some("acme/shop")),
        ("https://gitlab.com/acme/shop.git", None),
        ("https://github.com/acme", None),
        ("https://github.com/acme/shop/tree/main", None),
    ] {
        assert_eq!(h.repo_from_remote(url).as_deref(), want, "{url}");
    }
    let ghe = host("ghe", "github", "https://ghe.acme.example/api/v3");
    assert_eq!(ghe.repo_from_remote("git@ghe.acme.example:acme/shop.git").as_deref(), Some("acme/shop"));
    assert_eq!(ghe.repo_from_remote("git@github.com:acme/shop.git"), None);
}

#[tokio::test]
async fn unauthorized_is_needs_auth_and_secret_is_invalidated() {
    let server = MockServer::start().await;
    Mock::given(path("/user")).respond_with(ResponseTemplate::new(401)).mount(&server).await;
    let (h, secrets) = host_with("github-work", "github", &server.uri(), TOKEN);
    assert_eq!(h.me().await.unwrap_err().code, ErrorCode::NeedsAuth);
    assert_eq!(secrets.invalidated(), vec!["env:TOK"]);
}

#[tokio::test]
async fn feedback_unresolved_threads_reviews_with_a_body_and_failed_check_logs() {
    let server = MockServer::start().await;
    mount(&server, "POST", "/graphql", 200, "github/gql_feedback.json").await;
    mount(
        &server,
        "GET",
        "/repos/acme/shop/commits/abc123/check-runs",
        200,
        "github/check_runs_actions.json",
    )
    .await;
    let log: String = (1..=60).map(|i| format!("line {i}\n")).collect();
    // GitHub answers a 302 to a signed blob URL on another host ("localhost" vs "127.0.0.1").
    let blob = server.uri().replace("127.0.0.1", "localhost") + "/blob/9";
    Mock::given(method("GET"))
        .and(path("/repos/acme/shop/actions/jobs/9/logs"))
        .respond_with(ResponseTemplate::new(302).insert_header("location", blob.as_str()))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/blob/9"))
        .respond_with(ResponseTemplate::new(200).set_body_string(log))
        .mount(&server)
        .await;
    let f = gh(&server).feedback(&rref("github-work", "acme/shop", 101)).await.unwrap();
    let q = &bodies(&server, "POST", "/graphql").await[0];
    assert_eq!((q["variables"]["o"].as_str(), q["variables"]["num"].as_u64()), (Some("acme"), Some(101)));

    assert_eq!(f.threads.len(), 1, "resolved threads are dropped");
    let t = &f.threads[0];
    assert_eq!((t.id.as_str(), t.author.as_str()), ("PRRT_open", "erin"));
    assert_eq!((t.path.as_deref(), t.line), (Some("src/limiter.rs"), Some(42)), "outdated → original line");
    assert_eq!(t.body_md, "erin: Reset the counter on success.\n\nlouis: Will do.");
    assert_eq!(f.reviews.len(), 2, "empty bodies dropped");
    assert_eq!(f.reviews[0].state, Some(MyReviewState::ChangesRequested));
    assert_eq!(f.reviewers, vec!["erin", "zed"], "me excluded");
    assert_eq!(f.failed_checks.len(), 1);
    let c = &f.failed_checks[0];
    assert_eq!((c.name.as_str(), c.url.as_deref()), ("tests", Some("https://github.com/acme/shop/runs/9")));
    let tail = c.log_tail.as_deref().unwrap();
    assert_eq!(tail.lines().count(), 40);
    assert!(tail.starts_with("line 21") && tail.ends_with("line 60"));
    let blob_req =
        server.received_requests().await.unwrap().into_iter().find(|r| r.url.path() == "/blob/9").unwrap();
    assert!(!blob_req.headers.contains_key("authorization"), "the token never goes to the blob host");
}

#[tokio::test]
async fn feedback_refused_names_the_missing_scope() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/graphql"))
        .respond_with(ResponseTemplate::new(403))
        .mount(&server)
        .await;
    let e = gh(&server).feedback(&rref("github-work", "acme/shop", 101)).await.unwrap_err();
    assert_eq!(e.code, ErrorCode::PermissionDenied);
    assert_eq!(e.message, "GitHub refused the review threads (403: token lacks `pull_requests:read`).");
}

#[tokio::test]
async fn rerequest_asks_previous_reviewers_and_resolve_runs_one_mutation_per_thread() {
    let server = MockServer::start().await;
    mount(&server, "GET", "/user", 200, "github/user.json").await;
    mount(&server, "GET", "/repos/acme/shop/pulls/101/reviews", 200, "github/reviews.json").await;
    Mock::given(method("POST"))
        .and(path("/repos/acme/shop/pulls/101/requested_reviewers"))
        .respond_with(ResponseTemplate::new(201).set_body_string("{}"))
        .mount(&server)
        .await;
    let h = gh(&server);
    let who = h.rerequest_review(&rref("github-work", "acme/shop", 101)).await.unwrap();
    assert_eq!(who, vec!["zed", "erin"], "me (louis) never re-requested");
    let body = &bodies(&server, "POST", "/repos/acme/shop/pulls/101/requested_reviewers").await[0];
    assert_eq!(body["reviewers"], json!(["zed", "erin"]));

    Mock::given(method("POST"))
        .and(path("/graphql"))
        .respond_with(
            ResponseTemplate::new(200).set_body_string(
                r#"{"data":{"resolveReviewThread":{"thread":{"id":"x","isResolved":true}}}}"#,
            ),
        )
        .mount(&server)
        .await;
    h.resolve_threads(&rref("github-work", "acme/shop", 101), &["T1".into(), "T2".into()]).await.unwrap();
    let ids: Vec<Value> =
        bodies(&server, "POST", "/graphql").await.iter().map(|b| b["variables"]["id"].clone()).collect();
    assert_eq!(ids, vec![json!("T1"), json!("T2")]);
    assert!(
        bodies(&server, "POST", "/graphql").await[0]["query"]
            .as_str()
            .unwrap()
            .contains("resolveReviewThread")
    );
}

#[tokio::test]
async fn reviewed_by_me_search_brings_back_only_prs_pushed_after_my_review() {
    let server = MockServer::start().await;
    mount(&server, "POST", "/graphql", 200, "github/gql_list_reviewed.json").await;
    let list = gh(&server).list_reviews(&query(ReviewKind::ReviewRequested, false, false)).await.unwrap();
    let reqs = bodies(&server, "POST", "/graphql").await;
    assert_eq!(reqs.len(), 1, "both searches travel in one request");
    assert_eq!(
        reqs[0]["variables"]["r"],
        "is:pr is:open reviewed-by:@me -author:@me archived:false draft:false sort:updated-desc"
    );
    // 202 and the 101 duplicate were reviewed on their current head: not requests any more
    let numbers: Vec<_> = list.iter().map(|r| r.r#ref.number).collect();
    // 301 matches both searches (re-requested): listed once; 302 was only commented on
    assert_eq!(numbers, vec![101, 301, 201, 302]);
    assert_eq!(list[3].my_state, Some(MyReviewState::Pending));
    assert_eq!(list[3].reviewed_head.as_deref(), Some("old302"));
    let updated = &list[2];
    assert_eq!(updated.my_state, Some(MyReviewState::Approved));
    assert_eq!((updated.reviewed_head.as_deref(), updated.head_sha.as_str()), (Some("old201"), "new201"));
    // authored lists never carry the second search
    let authored = MockServer::start().await;
    mount(&authored, "POST", "/graphql", 200, "github/gql_list.json").await;
    gh(&authored).list_reviews(&query(ReviewKind::Authored, false, false)).await.unwrap();
    assert!(bodies(&authored, "POST", "/graphql").await[0]["variables"].get("r").is_none());
}

#[tokio::test]
async fn detail_reports_my_last_reviewed_head_and_pending_comments() {
    let server = MockServer::start().await;
    mount_detail(&server, "github/pull.json", "github/reviews_pending.json", "github/check_runs.json").await;
    mount(
        &server,
        "GET",
        "/repos/acme/shop/pulls/101/reviews/9/comments",
        200,
        "github/review_comments.json",
    )
    .await;
    let d = gh(&server).get(&rref("github-work", "acme/shop", 101)).await.unwrap();
    assert_eq!(d.review.reviewed_head.as_deref(), Some("old111"), "the pending review is not a review");
    assert_eq!(d.review.my_state, Some(MyReviewState::Approved));
    assert_eq!(d.pending_comments, 2);
}

#[tokio::test]
async fn pending_comment_creates_the_review_once_and_adds_a_thread() {
    let server = MockServer::start().await;
    mount(&server, "GET", "/repos/acme/shop/pulls/101/reviews", 200, "github/reviews.json").await;
    mount(&server, "POST", "/repos/acme/shop/pulls/101/reviews", 200, "github/review_pending_created.json")
        .await;
    mount(&server, "POST", "/graphql", 200, "github/gql_thread.json").await;
    let r = rref("github-work", "acme/shop", 101);
    gh(&server).add_pending_comment(&r, "src/a.rs", 12, "nit").await.unwrap();
    assert_eq!(bodies(&server, "POST", "/repos/acme/shop/pulls/101/reviews").await, vec![json!({})]);
    let q = &bodies(&server, "POST", "/graphql").await[0];
    assert_eq!(q["variables"], json!({"id": "PRR_9", "path": "src/a.rs", "line": 12, "body": "nit"}));

    // an existing pending review is reused, not recreated
    let again = MockServer::start().await;
    mount(&again, "GET", "/repos/acme/shop/pulls/101/reviews", 200, "github/reviews_pending.json").await;
    mount(&again, "POST", "/graphql", 200, "github/gql_thread.json").await;
    gh(&again).add_pending_comment(&r, "src/a.rs", 3, "x").await.unwrap();
    assert_eq!(count(&again, "POST", "/repos/acme/shop/pulls/101/reviews").await, 0);
    assert_eq!(bodies(&again, "POST", "/graphql").await[0]["variables"]["id"], "PRR_9");
}

#[tokio::test]
async fn decisions_publish_the_pending_review_with_its_comments() {
    let server = MockServer::start().await;
    mount(&server, "GET", "/repos/acme/shop/pulls/101/reviews", 200, "github/reviews_pending.json").await;
    mount(&server, "POST", "/repos/acme/shop/pulls/101/reviews/9/events", 200, "github/review_posted.json")
        .await;
    let h = gh(&server);
    let r = rref("github-work", "acme/shop", 101);
    h.approve(&r, "abc123").await.unwrap();
    h.request_changes(&r, "Please add tests").await.unwrap();
    h.comment(&r, "").await.unwrap();
    let events = bodies(&server, "POST", "/repos/acme/shop/pulls/101/reviews/9/events").await;
    assert_eq!(
        events,
        vec![
            json!({"event": "APPROVE", "body": ""}),
            json!({"event": "REQUEST_CHANGES", "body": "Please add tests"}),
            json!({"event": "COMMENT", "body": ""}),
        ]
    );
    assert_eq!(count(&server, "POST", "/repos/acme/shop/issues/101/comments").await, 0);
    // the pending review began on abc123: approving another head would approve unseen code
    let e = h.approve(&r, "newer").await.unwrap_err();
    assert_eq!(e.code, ErrorCode::InvalidArgument);
}

//! Bitbucket Cloud code host: Basic / Bearer auth, workspace walk, `next` pagination, request changes.

use crate::support::*;
use kelta_proto::codehost::{CiState, MyReviewState, ReviewDecision, ReviewKind};
use kelta_proto::error::ErrorCode;
use serde_json::json;
use wiremock::matchers::{header, method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

const ME: &str = "%7B470c176d-3574-44ea-bb41-89e8638bcca4%7D";

fn bb(server: &MockServer) -> std::sync::Arc<dyn kelta_proto::api::CodeHost> {
    host("bitbucket-acme", "bitbucket", &server.uri())
}

async fn common(server: &MockServer) {
    mount(server, "GET", "/user", 200, "bitbucket/user.json").await;
    mount(server, "GET", "/user/workspaces", 200, "bitbucket/workspaces.json").await;
}

#[tokio::test]
async fn api_tokens_are_basic_auth_with_the_account_email() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/user"))
        // base64("louis@acme.test:tok-123")
        .and(header("authorization", "Basic bG91aXNAYWNtZS50ZXN0OnRvay0xMjM="))
        .respond_with(ResponseTemplate::new(200).set_body_string(fixture_text("bitbucket/user.json")))
        .mount(&server)
        .await;
    let me = bb(&server).me().await.unwrap();
    assert_eq!((me.login.as_deref(), me.name.as_str()), (Some("louis"), "Louis D"));
}

#[tokio::test]
async fn access_tokens_are_bearer_and_basic_without_email_is_refused() {
    use kelta_codehosts::CodeHostFactory;
    use kelta_http::ProviderFactory;
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/user"))
        .and(header("authorization", format!("Bearer {TOKEN}").as_str()))
        .respond_with(ResponseTemplate::new(200).set_body_string(fixture_text("bitbucket/user.json")))
        .mount(&server)
        .await;
    let h = CodeHostFactory
        .code_host(
            &account("bitbucket", &server.uri(), json!({"auth": "bearer"})),
            http("b"),
            secrets_with(TOKEN),
        )
        .unwrap();
    h.me().await.unwrap();
    let e = CodeHostFactory
        .code_host(&account("bitbucket", &server.uri(), json!({})), http("b"), secrets_with(TOKEN))
        .err()
        .unwrap();
    assert_eq!(e.code, ErrorCode::InvalidArgument);
}

#[tokio::test]
async fn authored_list_walks_workspaces_follows_next_and_dedups() {
    let server = MockServer::start().await;
    common(&server).await;
    let url = format!("/workspaces/acme/pullrequests/{ME}");
    let next = format!("{}{url}?page=2", server.uri());
    Mock::given(method("GET"))
        .and(path(url.as_str()))
        .and(query_param("page", "2"))
        .respond_with(ResponseTemplate::new(200).set_body_string(fixture_text("bitbucket/prs_p2.json")))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path(url.as_str()))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_string(fixture_text("bitbucket/prs_p1.json").replace("__NEXT__", &next)),
        )
        .mount(&server)
        .await;
    let list = bb(&server).list_reviews(&query(ReviewKind::Authored, true, false)).await.unwrap();
    assert_eq!(list.iter().map(|r| r.r#ref.number).collect::<Vec<_>>(), [7, 11]);
    assert!(list.iter().all(|r| r.kind == ReviewKind::Authored && r.my_state.is_none()));
    assert_eq!(list[0].r#ref.repo, "acme/shop");
    assert_eq!(list[0].head_sha, "abc123def456");
    assert_eq!(list[0].decision, Some(ReviewDecision::ReviewRequired));
}

#[tokio::test]
async fn review_requests_query_recent_member_repositories_by_reviewer_uuid() {
    let server = MockServer::start().await;
    common(&server).await;
    mount(&server, "GET", "/repositories/acme", 200, "bitbucket/repos.json").await;
    mount(&server, "GET", "/repositories/acme/shop/pullrequests", 200, "bitbucket/prs.json").await;
    let list = bb(&server).list_reviews(&query(ReviewKind::ReviewRequested, true, true)).await.unwrap();
    assert_eq!(list.len(), 2);
    assert!(list[1].draft);
    assert_eq!(list[0].my_state, Some(MyReviewState::Pending), "I have not reviewed it yet");
    let repos = &queries(&server, "GET", "/repositories/acme").await[0];
    assert!(repos.contains("role=member") && repos.contains("updated_on"), "{repos}");
    let q = &queries(&server, "GET", "/repositories/acme/shop/pullrequests").await[0];
    assert!(q.contains("reviewers.uuid") && q.contains("OPEN"), "{q}");
}

#[tokio::test]
async fn detail_rolls_up_statuses_and_sums_the_diffstat() {
    let server = MockServer::start().await;
    common(&server).await;
    let pr = "/repositories/acme/shop/pullrequests/7";
    mount(&server, "GET", pr, 200, "bitbucket/pr_7.json").await;
    mount(&server, "GET", &format!("{pr}/statuses"), 200, "bitbucket/statuses.json").await;
    mount(&server, "GET", &format!("{pr}/diffstat"), 200, "bitbucket/diffstat.json").await;
    let d = bb(&server).get(&rref("bitbucket-acme", "acme/shop", 7)).await.unwrap();
    assert_eq!(d.review.ci, CiState::Failure, "worst check wins");
    assert_eq!(
        d.checks.iter().map(|c| c.state).collect::<Vec<_>>(),
        [CiState::Success, CiState::Failure, CiState::Pending]
    );
    assert_eq!((d.review.additions, d.review.deletions), (Some(14), Some(3)));
    assert_eq!(d.files[1].path, "src/lib.rs");
    assert_eq!(d.reviewers.len(), 2);
    assert!(d.reviewers.iter().any(|r| r.state == Some(MyReviewState::Approved)));
    assert_eq!(d.review.kind, ReviewKind::ReviewRequested);
    assert!(!d.body_html.contains("<script"));
}

#[tokio::test]
async fn a_rate_limited_repository_fails_the_whole_review_poll() {
    let server = MockServer::start().await;
    common(&server).await;
    mount(&server, "GET", "/repositories/acme", 200, "bitbucket/repos.json").await;
    mount(&server, "GET", "/repositories/acme/shop/pullrequests", 429, "bitbucket/prs.json").await;
    // same retries as production, without ~1.75 s of real backoff sleeps
    use kelta_codehosts::CodeHostFactory;
    use kelta_http::{HttpClient, HttpCtx, HttpPolicy, ProviderFactory};
    let policy = HttpPolicy { max_backoff: std::time::Duration::from_millis(1), ..HttpPolicy::default() };
    let http = HttpCtx::new(
        HttpClient::with_timeout("kelta-test", None),
        kelta_proto::ids::AccountId::new("bitbucket-acme"),
        policy,
    );
    let account = account("bitbucket", &server.uri(), json!({"email": "louis@acme.test"}));
    let h = CodeHostFactory.code_host(&account, http, secrets_with(TOKEN)).unwrap();
    let err = h.list_reviews(&query(ReviewKind::ReviewRequested, true, true)).await.unwrap_err();
    assert_eq!(err.code, ErrorCode::RateLimited);
}

#[tokio::test]
async fn request_changes_comments_first_then_flags_the_pr_and_refspec_is_the_source_branch() {
    let server = MockServer::start().await;
    let pr = "/repositories/acme/shop/pullrequests/7";
    for tail in ["comments", "request-changes", "approve"] {
        Mock::given(method("POST"))
            .and(path(format!("{pr}/{tail}").as_str()))
            .respond_with(ResponseTemplate::new(200).set_body_string("{}"))
            .mount(&server)
            .await;
    }
    mount(&server, "GET", "/repositories/acme/shop/pullrequests", 200, "bitbucket/prs.json").await;
    mount(&server, "GET", pr, 200, "bitbucket/pr_7.json").await;
    let h = bb(&server);
    let r = rref("bitbucket-acme", "acme/shop", 7);
    h.request_changes(&r, "please fix").await.unwrap();
    assert_eq!(
        bodies(&server, "POST", &format!("{pr}/comments")).await[0],
        json!({"content": {"raw": "please fix"}})
    );
    assert_eq!(count(&server, "POST", &format!("{pr}/request-changes")).await, 1);
    assert_eq!(h.approve(&r, "old").await.unwrap_err().code, ErrorCode::Conflict);
    assert_eq!(count(&server, "POST", &format!("{pr}/approve")).await, 0);
    h.approve(&r, "abc123def456").await.unwrap();
    assert_eq!(count(&server, "POST", &format!("{pr}/approve")).await, 1);

    // never listed in this process: no ref to fetch
    assert!(h.fetch_refspec(&r, "kelta/pr-7").starts_with("pull-requests/7/"));
    let found = h.find_for_branch("acme/shop", "feature/SHOP-142-limiter").await.unwrap().unwrap();
    assert_eq!(h.fetch_refspec(&found.r#ref, "kelta/pr-7"), "feature/SHOP-142-limiter:kelta/pr-7");
    let q = &queries(&server, "GET", "/repositories/acme/shop/pullrequests").await[0];
    assert!(q.contains("source.branch.name"), "{q}");
}

#[tokio::test]
async fn bad_repos_are_rejected_before_any_request() {
    let server = MockServer::start().await;
    let h = bb(&server);
    for bad in ["acme", "../x", "a/b/c"] {
        assert_eq!(
            h.comment(&rref("b", bad, 1), "x").await.unwrap_err().code,
            ErrorCode::InvalidArgument,
            "{bad}"
        );
    }
    assert!(server.received_requests().await.unwrap().is_empty());
}

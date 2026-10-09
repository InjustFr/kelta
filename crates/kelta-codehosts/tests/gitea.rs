//! Gitea code host: search + pull expansion, Link pagination, review events, draft prefix.

mod support;

use kelta_proto::codehost::{CiState, MyReviewState, PrCreate, ReviewDecision, ReviewKind};
use kelta_proto::error::ErrorCode;
use serde_json::json;
use support::*;
use wiremock::matchers::{body_partial_json, header, method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn gt(server: &MockServer) -> std::sync::Arc<dyn kelta_proto::api::CodeHost> {
    host("gitea-main", "gitea", &server.uri())
}

async fn common(server: &MockServer) {
    mount(server, "GET", "/api/v1/user", 200, "gitea/user.json").await;
    mount(server, "GET", "/api/v1/repos/acme/shop/pulls/7", 200, "gitea/pull_7.json").await;
    mount(server, "GET", "/api/v1/repos/acme/shop/pulls/11", 200, "gitea/pull_11.json").await;
    mount(server, "GET", "/api/v1/repos/acme/web/pulls/9", 200, "gitea/pull_9.json").await;
}

#[tokio::test]
async fn review_requested_search_follows_link_pages_and_drops_drafts() {
    let server = MockServer::start().await;
    common(&server).await;
    let next = format!("{}/api/v1/repos/issues/search?page=2", server.uri());
    Mock::given(method("GET"))
        .and(path("/api/v1/repos/issues/search"))
        .and(query_param("page", "1"))
        .and(query_param("type", "pulls"))
        .and(query_param("review_requested", "true"))
        .and(query_param("state", "open"))
        .and(header("authorization", format!("Bearer {TOKEN}").as_str()))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("link", format!("<{next}>; rel=\"next\"").as_str())
                .set_body_string(fixture_text("gitea/search_pulls.json")),
        )
        .mount(&server)
        .await;
    mount_page2(&server).await;
    let list = gt(&server).list_reviews(&query(ReviewKind::ReviewRequested, true, false)).await.unwrap();
    assert_eq!(
        list.iter().map(|r| (r.r#ref.repo.as_str(), r.r#ref.number)).collect::<Vec<_>>(),
        [("acme/shop", 7), ("acme/shop", 11)]
    );
    let a = &list[0];
    assert_eq!(a.my_state, Some(MyReviewState::Pending));
    assert_eq!(a.linked_tickets, vec!["SHOP-142"]);
    assert_eq!(a.labels, vec!["bug"]);
    assert_eq!((a.additions, a.deletions), (Some(14), Some(3)));
    assert_eq!(a.author.login.as_deref(), Some("dave"));

    let all = gt(&server).list_reviews(&query(ReviewKind::ReviewRequested, true, true)).await.unwrap();
    assert_eq!(all.len(), 3);
    assert!(all.iter().any(|r| r.draft && r.r#ref.number == 9));
}

async fn mount_page2(server: &MockServer) {
    mount(server, "GET", "/api/v1/repos/issues/search", 200, "gitea/search_pulls_p2.json").await;
}

#[tokio::test]
async fn authored_list_uses_created_flag() {
    let server = MockServer::start().await;
    common(&server).await;
    Mock::given(method("GET"))
        .and(path("/api/v1/repos/issues/search"))
        .and(query_param("created", "true"))
        .respond_with(ResponseTemplate::new(200).set_body_string(fixture_text("gitea/search_pulls_p2.json")))
        .mount(&server)
        .await;
    let list = gt(&server).list_reviews(&query(ReviewKind::Authored, true, false)).await.unwrap();
    assert_eq!(list.len(), 1);
    assert_eq!((list[0].kind, list[0].my_state), (ReviewKind::Authored, None));
}

#[tokio::test]
async fn detail_rolls_up_reviews_and_ci() {
    let server = MockServer::start().await;
    common(&server).await;
    let pr = "/api/v1/repos/acme/shop/pulls/7";
    mount(&server, "GET", &format!("{pr}/reviews"), 200, "gitea/reviews.json").await;
    mount(&server, "GET", &format!("{pr}/files"), 200, "gitea/files.json").await;
    mount(
        &server,
        "GET",
        "/api/v1/repos/acme/shop/commits/abc1230000000000000000000000000000000000/status",
        200,
        "gitea/status.json",
    )
    .await;
    let h = gt(&server);
    let d = h.get(&rref("gitea-main", "acme/shop", 7)).await.unwrap();
    // erin vetoed, fay's approval was dismissed, dave only commented, louis is still requested
    assert_eq!(d.review.decision, Some(ReviewDecision::ChangesRequested));
    assert_eq!(d.review.my_state, Some(MyReviewState::Pending));
    assert_eq!(d.review.ci, CiState::Failure);
    assert_eq!(
        d.checks.iter().map(|c| (c.name.as_str(), c.state)).collect::<Vec<_>>(),
        [("ci/build", CiState::Success), ("ci/test", CiState::Failure)]
    );
    assert_eq!(d.checks[1].url, None, "empty target_url is dropped");
    assert_eq!(
        d.files.iter().map(|f| (f.path.as_str(), f.additions, f.deletions)).collect::<Vec<_>>(),
        [("src/limiter.rs", 12, 0), ("src/lib.rs", 2, 3)]
    );
    let names: Vec<_> = d.reviewers.iter().map(|r| (r.user.login.clone().unwrap(), r.state)).collect();
    assert!(names.contains(&("louis".into(), Some(MyReviewState::Pending))));
    assert!(names.contains(&("erin".into(), Some(MyReviewState::ChangesRequested))));
    assert!(names.contains(&("dave".into(), Some(MyReviewState::Commented))));
    assert!(!d.body_html.contains("<script"));
}

#[tokio::test]
async fn approve_and_request_changes_are_reviews_and_comment_is_an_issue_comment() {
    let server = MockServer::start().await;
    let reviews = "/api/v1/repos/acme/shop/pulls/7/reviews";
    mount(&server, "POST", reviews, 200, "gitea/reviews_approved_by_me.json").await;
    Mock::given(method("POST"))
        .and(path("/api/v1/repos/acme/shop/issues/7/comments"))
        .respond_with(ResponseTemplate::new(201).set_body_string("{}"))
        .mount(&server)
        .await;
    let h = gt(&server);
    let r = rref("gitea-main", "acme/shop", 7);
    h.approve(&r, "abc123").await.unwrap();
    h.request_changes(&r, "please fix").await.unwrap();
    h.comment(&r, "hi").await.unwrap();
    let b = bodies(&server, "POST", reviews).await;
    assert_eq!(b[0], json!({"event": "APPROVED", "commit_id": "abc123"}));
    assert_eq!(b[1], json!({"event": "REQUEST_CHANGES", "body": "please fix"}));
    assert_eq!(
        bodies(&server, "POST", "/api/v1/repos/acme/shop/issues/7/comments").await[0],
        json!({"body": "hi"})
    );
}

#[tokio::test]
async fn drafts_are_created_with_a_wip_prefix_and_bad_repos_are_rejected() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/api/v1/repos/acme/shop/pulls"))
        .and(body_partial_json(json!({"title": "WIP: SHOP-1 x", "head": "feature/x", "base": "main"})))
        .respond_with(ResponseTemplate::new(201).set_body_string(fixture_text("gitea/pull_created.json")))
        .mount(&server)
        .await;
    let h = gt(&server);
    let mut d = PrCreate {
        repo: "acme/shop".into(),
        head: "feature/x".into(),
        base: "main".into(),
        title: "SHOP-1 x".into(),
        body: "b".into(),
        draft: true,
    };
    assert_eq!(h.create(&d).await.unwrap().r#ref.number, 21);
    for bad in ["acme", "acme/../x", "a/b/c", "/x"] {
        d.repo = bad.into();
        assert_eq!(h.create(&d).await.unwrap_err().code, ErrorCode::InvalidArgument, "{bad}");
    }
}

#[tokio::test]
async fn find_for_branch_scans_open_pulls_by_head() {
    let server = MockServer::start().await;
    mount(&server, "GET", "/api/v1/user", 200, "gitea/user.json").await;
    mount(&server, "GET", "/api/v1/repos/acme/shop/pulls", 200, "gitea/pulls_open.json").await;
    let h = gt(&server);
    let mine = h.find_for_branch("acme/shop", "feature/mine").await.unwrap().unwrap();
    assert_eq!((mine.r#ref.number, mine.kind), (8, ReviewKind::Authored));
    assert!(h.find_for_branch("acme/shop", "nope").await.unwrap().is_none());
}

#[test]
fn base_url_is_required_and_api_suffix_tolerated() {
    use kelta_codehosts::CodeHostFactory;
    use kelta_http::ProviderFactory;
    let acc = |extra: serde_json::Value| {
        let mut v = json!({"kind": "gitea", "secret": "env:TOK"});
        v.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
        serde_json::from_value(v).unwrap()
    };
    let make = |a| CodeHostFactory.code_host(&a, http("g"), secrets_with(TOKEN));
    assert_eq!(make(acc(json!({}))).err().unwrap().code, ErrorCode::InvalidArgument);
    let h = make(acc(json!({"base_url": "https://git.acme.example/api/v1/"}))).unwrap();
    assert_eq!(h.repo_from_remote("git@git.acme.example:a/b.git").as_deref(), Some("a/b"));
}

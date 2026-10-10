//! Gitea Issues: assigned search, project filter, Link pagination, close/reopen, assignment.

use crate::support::*;
use kelta_proto::api::Tracker;
use kelta_proto::error::ErrorCode;
use kelta_proto::tracker::{Assignee, Cursor, StatusCategory, Who};
use serde_json::json;
use wiremock::matchers::{header, method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

const ISSUE: &str = "/api/v1/repos/acme/shop/issues/12";

fn gt(server: &MockServer) -> std::sync::Arc<dyn Tracker> {
    tracker("gitea-main", "gitea", &server.uri(), json!({}))
}

fn r() -> kelta_proto::tracker::TicketRef {
    tref("gitea-main", "acme/shop#12", "1012")
}

#[tokio::test]
async fn list_assigned_pages_and_filters_by_project() {
    let server = MockServer::start().await;
    let next = format!("{}/api/v1/repos/issues/search?page=2", server.uri());
    Mock::given(method("GET"))
        .and(path("/api/v1/repos/issues/search"))
        .and(query_param("assigned", "true"))
        .and(query_param("type", "issues"))
        .and(query_param("state", "open"))
        .and(query_param("page", "1"))
        .and(header("authorization", format!("Bearer {TOKEN}").as_str()))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("link", format!("<{next}>; rel=\"next\"").as_str())
                .set_body_string(fixture_text("gitea/issues_p1.json")),
        )
        .mount(&server)
        .await;
    let h = gt(&server);
    let mut v = view("mine");
    let p1 = h.list(&v, None).await.unwrap();
    assert_eq!(
        p1.items.iter().map(|t| t.r#ref.key.as_str()).collect::<Vec<_>>(),
        ["acme/shop#12", "acme/other#13"]
    );
    assert_eq!(p1.next, Some(Cursor::Page(2)));
    let t = &p1.items[0];
    assert_eq!(
        (t.status.category, t.priority.as_deref(), t.assignee.as_ref().map(|u| u.id.as_str())),
        (StatusCategory::Todo, Some("high"), Some("louis"))
    );

    v.project = Some("acme/shop".into());
    let filtered = h.list(&v, None).await.unwrap();
    assert_eq!(filtered.items.len(), 1);
    assert_eq!(
        filtered.next,
        Some(Cursor::Page(2)),
        "paging continues even when the filter trimmed the page"
    );

    assert_eq!(h.list(&v, Some(Cursor::Offset(3))).await.unwrap_err().code, ErrorCode::InvalidArgument);
}

#[tokio::test]
async fn scope_all_with_a_project_lists_the_repository() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/v1/repos/acme/shop/issues"))
        .and(query_param("state", "all"))
        .respond_with(ResponseTemplate::new(200).set_body_string(fixture_text("gitea/issues_p2.json")))
        .mount(&server)
        .await;
    let mut v = view("all");
    v.project = Some("acme/shop".into());
    v.scope = Some("all".into());
    v.status = Some("*".into());
    let page = gt(&server).list(&v, None).await.unwrap();
    assert_eq!(page.items[0].r#ref.key, "acme/shop#14");
    assert!(page.items[0].assignee.is_none() && page.next.is_none());
    assert!(!server.received_requests().await.unwrap()[0].url.query().unwrap().contains("assigned"));
    v.project = Some("acme/../x".into());
    assert_eq!(gt(&server).list(&v, None).await.unwrap_err().code, ErrorCode::InvalidArgument);
}

#[tokio::test]
async fn detail_keeps_the_last_twenty_comments_in_order() {
    let server = MockServer::start().await;
    mount(&server, "GET", ISSUE, 200, "gitea/issue.json").await;
    mount(&server, "GET", &format!("{ISSUE}/comments"), 200, "gitea/comments.json").await;
    let d = gt(&server).get(&r()).await.unwrap();
    assert_eq!(d.comments.len(), 20);
    assert!(d.comments[0].body_html.contains("comment 6") && d.comments[19].body_html.contains("comment 25"));
    assert!(d.comments[0].body_html.contains("<strong>bold</strong>"));
    assert!(!d.body_html.contains("<script"));
}

#[tokio::test]
async fn close_and_reopen_patch_the_state() {
    let server = MockServer::start().await;
    mount(&server, "GET", ISSUE, 200, "gitea/issue.json").await;
    mount(&server, "PATCH", ISSUE, 200, "gitea/issue_closed.json").await;
    let h = gt(&server);
    let ts = h.transitions(&r()).await.unwrap();
    assert_eq!(ts.len(), 1);
    assert_eq!((ts[0].id.as_str(), ts[0].to.category), ("close", StatusCategory::Done));
    let moved = h.transition(&r(), "close", None).await.unwrap();
    assert_eq!(moved.status.category, StatusCategory::Done);
    h.transition(&r(), "reopen", None).await.unwrap();
    let b = bodies(&server, "PATCH", ISSUE).await;
    assert_eq!((&b[0], &b[1]), (&json!({"state": "closed"}), &json!({"state": "open"})));
    assert_eq!(h.transition(&r(), "nope", None).await.unwrap_err().code, ErrorCode::InvalidArgument);
}

#[tokio::test]
async fn assign_me_user_and_nobody() {
    let server = MockServer::start().await;
    mount(&server, "GET", "/api/v1/user", 200, "gitea/user.json").await;
    mount(&server, "PATCH", ISSUE, 200, "gitea/issue.json").await;
    let h = gt(&server);
    h.assign(&r(), Assignee::Me).await.unwrap();
    h.assign(&r(), Assignee::User { id: "dave".into() }).await.unwrap();
    h.assign(&r(), Assignee::None).await.unwrap();
    let b = bodies(&server, "PATCH", ISSUE).await;
    assert_eq!(
        b,
        vec![json!({"assignees": ["louis"]}), json!({"assignees": ["dave"]}), json!({"assignees": []})]
    );
}

#[test]
fn bitbucket_has_no_tracker_and_gitea_needs_a_base_url() {
    use kelta_http::ProviderFactory;
    use kelta_trackers::TrackerFactory;
    let bb = account("bitbucket", "https://api.bitbucket.org/2.0", json!({}));
    assert_eq!(TrackerFactory.tracker(&bb, http("b"), secrets()).err().unwrap().code, ErrorCode::Unsupported);
    let no_url: kelta_proto::settings::AccountConfig =
        serde_json::from_value(json!({"kind": "gitea", "secret": "env:TOK"})).unwrap();
    assert_eq!(
        TrackerFactory.tracker(&no_url, http("g"), secrets()).err().unwrap().code,
        ErrorCode::InvalidArgument
    );
}

#[tokio::test]
async fn who_picks_assigned_all_or_client_side_unassigned() {
    let server = MockServer::start().await;
    mount(&server, "GET", "/api/v1/repos/acme/shop/issues", 200, "gitea/issues_p1.json").await;
    mount(&server, "GET", "/api/v1/repos/issues/search", 200, "gitea/issues_p1.json").await;
    let h = gt(&server);
    let mut v = view("v");
    v.project = Some("acme/shop".into());
    v.scope = Some("all".into()); // ignored once `who` is set
    v.who = Some(Who::Mine);
    assert_eq!(h.list(&v, None).await.unwrap().items.len(), 1);
    v.who = Some(Who::Anyone);
    assert_eq!(h.list(&v, None).await.unwrap().items.len(), 1);
    v.who = Some(Who::Unassigned);
    assert!(h.list(&v, None).await.unwrap().items.is_empty(), "both fixture issues are assigned");
    let urls: Vec<_> = server.received_requests().await.unwrap().iter().map(|r| r.url.to_string()).collect();
    assert!(urls[0].contains("/repos/issues/search?") && urls[0].contains("assigned=true"));
    assert!(urls[1].contains("/repos/acme/shop/issues?") && !urls[1].contains("assigned"));
}

#[tokio::test]
async fn unassigned_keeps_only_issues_without_assignees() {
    let server = MockServer::start().await;
    mount(&server, "GET", "/api/v1/repos/acme/shop/issues", 200, "gitea/issues_p2.json").await;
    let mut v = view("v");
    v.project = Some("acme/shop".into());
    v.who = Some(Who::Unassigned);
    let page = gt(&server).list(&v, None).await.unwrap();
    assert_eq!(page.items[0].r#ref.key, "acme/shop#14");
    v.project = None;
    assert_eq!(gt(&server).list(&v, None).await.unwrap_err().code, ErrorCode::InvalidArgument);
}

#[tokio::test]
async fn sources_search_repositories() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/v1/repos/search"))
        .and(query_param("q", "shop"))
        .respond_with(ResponseTemplate::new(200).set_body_string(fixture_text("gitea/repos_search.json")))
        .expect(1)
        .mount(&server)
        .await;
    let hits = gt(&server).sources(" shop ").await.unwrap();
    assert_eq!(hits.len(), 2);
    let h = &hits[0];
    assert_eq!(
        (h.kind.as_str(), h.label.as_str(), h.detail.as_deref()),
        ("repo", "acme/shop", Some("The storefront"))
    );
    assert_eq!(
        (h.view.id.as_str(), h.view.project.as_deref(), h.view.who),
        ("gitea:repo:acme/shop", Some("acme/shop"), Some(Who::Mine))
    );
    assert_eq!(hits[1].detail, None, "empty descriptions are dropped");
}

//! GitHub Issues: REST lists with ETag, comments paging, Projects v2 moves resolved by name.

use crate::support::*;
use kelta_proto::error::ErrorCode;
use kelta_proto::settings::{ProjectV2Ref, TrackerBinding};
use kelta_proto::tracker::{Assignee, Cursor, StatusCategory};
use serde_json::json;
use wiremock::matchers::{body_partial_json, body_string_contains, header, method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn gh(server: &MockServer) -> std::sync::Arc<dyn kelta_proto::api::Tracker> {
    tracker("github-work", "github", &server.uri(), json!({}))
}

fn r() -> kelta_proto::tracker::TicketRef {
    tref("github-work", "acme/shop#12", "1012")
}

fn project_view() -> kelta_proto::settings::TrackerView {
    let mut v = view("board");
    v.project_v2 = Some(ProjectV2Ref { owner: "acme".into(), number: 5, status_field: "Status".into() });
    v
}

async fn gql(server: &MockServer, contains: &str, fixture_name: &str) {
    Mock::given(method("POST"))
        .and(path("/graphql"))
        .and(body_string_contains(contains))
        .respond_with(ResponseTemplate::new(200).set_body_string(fixture_text(fixture_name)))
        .mount(server)
        .await;
}

#[tokio::test]
async fn assigned_list_drops_pull_requests_and_follows_link_headers() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/issues"))
        .and(query_param("filter", "assigned"))
        .and(query_param("state", "open"))
        .and(header("authorization", "Bearer tok-123"))
        .and(header("accept", "application/vnd.github+json"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("link", "<https://api.github.com/issues?page=2>; rel=\"next\", <https://api.github.com/issues?page=3>; rel=\"last\"")
                .set_body_string(fixture_text("github/issues_assigned_p1.json")),
        )
        .mount(&server)
        .await;
    let page = gh(&server).list(&view("mine"), None).await.unwrap();
    assert_eq!(page.items.len(), 2, "the pull request is dropped");
    assert_eq!(page.next, Some(Cursor::Page(2)));
    let t = &page.items[0];
    assert_eq!(t.r#ref.key, "acme/shop#12");
    assert_eq!(t.status.category, StatusCategory::Todo);
    assert_eq!(t.labels, vec!["bug"]);
    assert_eq!(t.assignee.as_ref().unwrap().id, "louis");
    assert_eq!(t.project_hint.as_deref(), Some("acme/shop"));
    assert_eq!(page.items[1].r#ref.key, "acme/web#7");
}

#[tokio::test]
async fn a_304_is_answered_from_the_etag_cache_without_refetching_the_body() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/issues"))
        .and(header("if-none-match", "\"etag-1\""))
        .respond_with(ResponseTemplate::new(304))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/issues"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("etag", "\"etag-1\"")
                .set_body_string(fixture_text("github/issues_assigned_p1.json")),
        )
        .expect(1)
        .mount(&server)
        .await;
    let t = gh(&server);
    let a = t.list(&view("mine"), None).await.unwrap();
    let b = t.list(&view("mine"), None).await.unwrap();
    assert_eq!(a, b);
    assert_eq!(b.items.len(), 2);
}

#[tokio::test]
async fn repo_views_filter_by_state_and_assignee() {
    let server = MockServer::start().await;
    mount(&server, "GET", "/user", 200, "github/user.json").await;
    Mock::given(method("GET"))
        .and(path("/repos/acme/shop/issues"))
        .and(query_param("state", "closed"))
        .and(query_param("assignee", "louis"))
        .respond_with(ResponseTemplate::new(200).set_body_string(fixture_text("github/issues_repo.json")))
        .expect(1)
        .mount(&server)
        .await;
    let mut v = view("repo");
    v.repo = Some("acme/shop".into());
    v.status = Some("closed".into());
    v.assigned_to = Some("me".into());
    let page = gh(&server).list(&v, None).await.unwrap();
    assert_eq!(page.items.len(), 2);
    assert!(page.next.is_none());
}

#[tokio::test]
async fn search_views_use_the_search_api() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/search/issues"))
        .and(query_param("q", "org:acme label:bug is:issue"))
        .respond_with(ResponseTemplate::new(200).set_body_string(fixture_text("github/search.json")))
        .expect(1)
        .mount(&server)
        .await;
    let mut v = view("search");
    v.search = Some("org:acme label:bug".into());
    let page = gh(&server).list(&v, None).await.unwrap();
    assert_eq!(
        page.items.iter().map(|t| t.r#ref.key.as_str()).collect::<Vec<_>>(),
        vec!["acme/shop#12", "acme/api#3"]
    );
}

#[tokio::test]
async fn project_v2_views_take_the_status_from_the_board() {
    let server = MockServer::start().await;
    gql(&server, "projectV2(number", "github/gql_project_items.json").await;
    let page = gh(&server).list(&project_view(), None).await.unwrap();
    assert_eq!(page.items.len(), 2, "items without an issue are skipped");
    assert_eq!(page.items[0].status.name, "In Progress");
    assert_eq!(page.items[0].status.category, StatusCategory::InProgress);
    assert_eq!(page.items[0].kind.as_deref(), Some("Bug"));
    assert_eq!(page.items[1].status.category, StatusCategory::Done);
    assert_eq!(page.next, Some(Cursor::After("CUR1".into())));
    // the cursor goes back as the `after` variable
    gh(&server).list(&project_view(), page.next).await.unwrap();
    let b = bodies(&server, "POST", "/graphql").await;
    assert_eq!(b[0]["variables"]["owner"], "acme");
    assert_eq!(b[0]["variables"]["number"], 5);
    assert_eq!(b[1]["variables"]["after"], "CUR1");
}

#[tokio::test]
async fn ghe_base_urls_use_the_api_graphql_endpoint() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/api/graphql"))
        .respond_with(
            ResponseTemplate::new(200).set_body_string(fixture_text("github/gql_project_items.json")),
        )
        .expect(1)
        .mount(&server)
        .await;
    let t = tracker("ghe", "github", &format!("{}/api/v3", server.uri()), json!({}));
    t.list(&project_view(), None).await.unwrap();
    let r = tref("ghe", "acme/shop#12", "1");
    assert_eq!(t.browser_url(&r), format!("{}/acme/shop/issues/12", server.uri()));
}

#[tokio::test]
async fn detail_shows_the_newest_twenty_comments() {
    let server = MockServer::start().await;
    mount(&server, "GET", "/repos/acme/shop/issues/12", 200, "github/issue.json").await;
    Mock::given(method("GET"))
        .and(path("/repos/acme/shop/issues/12/comments"))
        .and(query_param("page", "2"))
        .respond_with(ResponseTemplate::new(200).set_body_string(fixture_text("github/comments_last.json")))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/repos/acme/shop/issues/12/comments"))
        .and(query_param("page", "1"))
        .respond_with(ResponseTemplate::new(200).set_body_string(fixture_text("github/comments_prev.json")))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/repos/acme/shop/issues/12/comments"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("link", "<https://api.github.com/x/comments?per_page=20&page=2>; rel=\"next\", <https://api.github.com/x/comments?per_page=20&page=2>; rel=\"last\"")
                .set_body_string(fixture_text("github/comments_p1.json")),
        )
        .mount(&server)
        .await;
    let d = gh(&server).get(&r()).await.unwrap();
    assert_eq!(d.comments.len(), 20);
    assert!(d.comments[19].body_html.contains("<strong>23</strong>"));
    assert!(d.comments[0].body_html.contains("<strong>4</strong>"));
    assert!(d.body_html.contains("<ol>") && !d.body_html.contains("<script"));
    assert_eq!(d.ticket.title, "Cart total wrong");
}

#[tokio::test]
async fn issues_outside_projects_move_with_open_and_closed() {
    let server = MockServer::start().await;
    gql(&server, "projectItems", "github/gql_issue_no_projects.json").await;
    Mock::given(method("PATCH"))
        .and(path("/repos/acme/shop/issues/12"))
        .and(body_partial_json(json!({"state": "closed"})))
        .respond_with(ResponseTemplate::new(200).set_body_string(fixture_text("github/issue_closed.json")))
        .expect(1)
        .mount(&server)
        .await;
    let t = gh(&server);
    let ts = t.transitions(&r()).await.unwrap();
    assert_eq!(ts.len(), 1);
    assert_eq!(ts[0].id, "closed");
    assert_eq!(ts[0].to.category, StatusCategory::Done);
    let moved = t.transition(&r(), "closed", None).await.unwrap();
    assert_eq!(moved.status.category, StatusCategory::Done);
}

#[tokio::test]
async fn project_moves_resolve_field_and_option_ids_by_name_and_cache_them() {
    let server = MockServer::start().await;
    gql(&server, "projectItems", "github/gql_issue_projects.json").await;
    gql(&server, "node(id", "github/gql_project_fields.json").await;
    gql(&server, "updateProjectV2ItemFieldValue", "github/gql_mutation.json").await;
    mount(&server, "GET", "/repos/acme/shop/issues/12", 200, "github/issue.json").await;
    let t = gh(&server);
    let ts = t.transitions(&r()).await.unwrap();
    // the current status ("In Progress") is not offered; ids are names, never hard-coded option ids
    assert_eq!(
        ts.iter().map(|t| t.id.as_str()).collect::<Vec<_>>(),
        vec!["status:Todo", "status:In Review", "status:Done"]
    );
    assert_eq!(ts[1].to.category, StatusCategory::InReview);
    let moved = t.transition(&r(), "status:In Review", None).await.unwrap();
    assert_eq!(moved.status.name, "In Review");
    assert_eq!(moved.status.category, StatusCategory::InReview);
    let gql_bodies = bodies(&server, "POST", "/graphql").await;
    let field_queries =
        gql_bodies.iter().filter(|b| b["query"].as_str().unwrap_or("").contains("node(id")).count();
    assert_eq!(field_queries, 1, "field metadata is cached across transitions() and transition()");
    let m = gql_bodies
        .iter()
        .find(|b| b["query"].as_str().unwrap_or("").contains("updateProjectV2ItemFieldValue"))
        .unwrap();
    assert_eq!(
        m["variables"],
        json!({"p": "PVT_proj1", "i": "PVTI_item1", "f": "PVTSSF_status", "o": "opt_rev"})
    );
}

#[tokio::test]
async fn stale_option_ids_are_resolved_again_once() {
    let server = MockServer::start().await;
    gql(&server, "projectItems", "github/gql_issue_projects.json").await;
    Mock::given(method("POST"))
        .and(path("/graphql"))
        .and(body_string_contains("node(id"))
        .respond_with(
            ResponseTemplate::new(200).set_body_string(fixture_text("github/gql_project_fields.json")),
        )
        .up_to_n_times(1)
        .mount(&server)
        .await;
    gql(&server, "node(id", "github/gql_project_fields_renamed.json").await;
    Mock::given(method("POST"))
        .and(path("/graphql"))
        .and(body_string_contains("updateProjectV2ItemFieldValue"))
        .and(body_partial_json(json!({"variables": {"o": "opt_rev"}})))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!({"errors": [{"type": "NOT_FOUND", "message": "option not found"}]})),
        )
        .mount(&server)
        .await;
    gql(&server, "updateProjectV2ItemFieldValue", "github/gql_mutation.json").await;
    mount(&server, "GET", "/repos/acme/shop/issues/12", 200, "github/issue.json").await;
    let moved = gh(&server).transition(&r(), "status:In Review", None).await.unwrap();
    assert_eq!(moved.status.name, "In Review");
    let ms: Vec<_> = bodies(&server, "POST", "/graphql")
        .await
        .into_iter()
        .filter(|b| b["query"].as_str().unwrap_or("").contains("updateProjectV2ItemFieldValue"))
        .collect();
    assert_eq!(ms.len(), 2);
    assert_eq!(ms[1]["variables"]["o"], "opt_new_rev");
}

#[tokio::test]
async fn unknown_status_names_are_not_found() {
    let server = MockServer::start().await;
    gql(&server, "projectItems", "github/gql_issue_projects.json").await;
    gql(&server, "node(id", "github/gql_project_fields.json").await;
    let e = gh(&server).transition(&r(), "status:Shipped", None).await.unwrap_err();
    assert_eq!(e.code, ErrorCode::NotFound);
    let e = gh(&server).transition(&r(), "bogus", None).await.unwrap_err();
    assert_eq!(e.code, ErrorCode::InvalidArgument);
}

#[tokio::test]
async fn columns_from_the_project_status_field() {
    let server = MockServer::start().await;
    gql(&server, "projectV2(number", "github/gql_columns.json").await;
    let b = TrackerBinding { views: vec![project_view()], ..TrackerBinding::default() };
    let cols = gh(&server).columns(&b).await.unwrap();
    assert_eq!(cols.iter().map(|c| c.name.as_str()).collect::<Vec<_>>(), vec!["Todo", "In Progress", "Done"]);
    assert_eq!(cols[2].category, StatusCategory::Done);
    // without a project: open / closed
    let plain = gh(&server).columns(&TrackerBinding::default()).await.unwrap();
    assert_eq!(plain.iter().map(|c| c.id.as_str()).collect::<Vec<_>>(), vec!["open", "closed"]);
}

#[tokio::test]
async fn comment_and_assign() {
    let server = MockServer::start().await;
    mount(&server, "GET", "/user", 200, "github/user.json").await;
    Mock::given(method("POST"))
        .and(path("/repos/acme/shop/issues/12/comments"))
        .respond_with(ResponseTemplate::new(201).set_body_string("{}"))
        .mount(&server)
        .await;
    Mock::given(method("PATCH"))
        .and(path("/repos/acme/shop/issues/12"))
        .respond_with(ResponseTemplate::new(200).set_body_string(fixture_text("github/issue.json")))
        .mount(&server)
        .await;
    let t = gh(&server);
    t.comment(&r(), "On it").await.unwrap();
    assert_eq!(
        bodies(&server, "POST", "/repos/acme/shop/issues/12/comments").await[0],
        json!({"body": "On it"})
    );
    t.assign(&r(), Assignee::Me).await.unwrap();
    t.assign(&r(), Assignee::None).await.unwrap();
    let p = bodies(&server, "PATCH", "/repos/acme/shop/issues/12").await;
    assert_eq!(p[0], json!({"assignees": ["louis"]}));
    assert_eq!(p[1], json!({"assignees": []}));
}

#[tokio::test]
async fn auth_and_rate_limit_errors() {
    let server = MockServer::start().await;
    Mock::given(path("/user")).respond_with(ResponseTemplate::new(401)).mount(&server).await;
    Mock::given(method("POST"))
        .and(path("/graphql"))
        .respond_with(
            ResponseTemplate::new(200).set_body_string(fixture_text("github/gql_rate_limited.json")),
        )
        .mount(&server)
        .await;
    let t = gh(&server);
    assert_eq!(t.me().await.unwrap_err().code, ErrorCode::NeedsAuth);
    assert_eq!(t.list(&project_view(), None).await.unwrap_err().code, ErrorCode::RateLimited);
}

#[tokio::test]
async fn keys_and_caps() {
    let server = MockServer::start().await;
    let t = gh(&server);
    assert!(t.caps().projects_v2);
    assert_eq!(t.branch_key(&r()), "gh-12");
    assert_eq!(t.browser_url(&r()), format!("{}/acme/shop/issues/12", server.uri()));
    let real = tracker("github-work", "github", "https://api.github.com", json!({}));
    assert_eq!(real.browser_url(&r()), "https://github.com/acme/shop/issues/12");
}

#[tokio::test]
async fn rejected_mutation_with_null_field_is_an_error() {
    let server = MockServer::start().await;
    gql(&server, "projectItems", "github/gql_issue_projects.json").await;
    gql(&server, "node(id", "github/gql_project_fields.json").await;
    Mock::given(method("POST"))
        .and(path("/graphql"))
        .and(body_string_contains("updateProjectV2ItemFieldValue"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "data": {"updateProjectV2ItemFieldValue": null},
            "errors": [{"type": "FORBIDDEN", "message": "no write access"}]
        })))
        .mount(&server)
        .await;
    mount(&server, "GET", "/repos/acme/shop/issues/12", 200, "github/issue.json").await;
    let e = gh(&server).transition(&r(), "status:In Review", None).await.unwrap_err();
    assert_eq!(e.code, ErrorCode::PermissionDenied);
}

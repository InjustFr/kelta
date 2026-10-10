//! Redmine against fixtures: list/paging, detail with allowed_statuses, status PUT, 422, notes.

use crate::support::*;
use kelta_proto::error::ErrorCode;
use kelta_proto::settings::TrackerBinding;
use kelta_proto::tracker::{Assignee, BodyFormat, Cursor, StatusCategory};
use serde_json::json;
use wiremock::matchers::{body_partial_json, header, method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn rm(server: &MockServer, extra: serde_json::Value) -> std::sync::Arc<dyn kelta_proto::api::Tracker> {
    tracker("redmine-client", "redmine", &server.uri(), extra)
}

fn r() -> kelta_proto::tracker::TicketRef {
    tref("redmine-client", "4567", "4567")
}

#[tokio::test]
async fn list_pages_with_offset_and_sends_the_api_key_header() {
    let server = MockServer::start().await;
    mount(&server, "GET", "/issue_statuses.json", 200, "redmine/issue_statuses.json").await;
    Mock::given(method("GET"))
        .and(path("/issues.json"))
        .and(query_param("offset", "2"))
        .respond_with(ResponseTemplate::new(200).set_body_string(fixture_text("redmine/issues_p2.json")))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/issues.json"))
        .and(query_param("assigned_to_id", "me"))
        .and(query_param("status_id", "open"))
        .and(query_param("sort", "updated_on:desc"))
        .and(query_param("limit", "100"))
        .and(header("x-redmine-api-key", TOKEN))
        .respond_with(ResponseTemplate::new(200).set_body_string(fixture_text("redmine/issues_p1.json")))
        .mount(&server)
        .await;
    let t = rm(&server, json!({}));
    let p1 = t.list(&view("mine"), None).await.unwrap();
    assert_eq!(p1.items.len(), 2);
    assert_eq!(p1.next, Some(Cursor::Offset(2)));
    let a = &p1.items[0];
    assert_eq!(a.r#ref.key, "4567");
    assert_eq!(a.title, "Checkout button misaligned");
    assert_eq!(a.status.category, StatusCategory::Todo);
    assert_eq!(p1.items[1].status.category, StatusCategory::InProgress);
    assert_eq!(a.kind.as_deref(), Some("Bug"));
    assert_eq!(a.assignee.as_ref().unwrap().name, "Louis Duprat");
    assert_eq!(a.url, format!("{}/issues/4567", server.uri()));
    let p2 = t.list(&view("mine"), p1.next).await.unwrap();
    assert_eq!(p2.items.len(), 1);
    assert!(p2.next.is_none());
}

#[tokio::test]
async fn view_options_map_to_query_parameters() {
    let server = MockServer::start().await;
    mount(&server, "GET", "/issue_statuses.json", 200, "redmine/issue_statuses.json").await;
    mount(&server, "GET", "/issues.json", 200, "redmine/issues_p2.json").await;
    let t = rm(&server, json!({}));
    let mut v = view("v");
    v.project_id = Some("client-site".into());
    v.assigned_to = Some("any".into());
    v.status = Some("*".into());
    t.list(&v, None).await.unwrap();
    let mut q = view("q");
    q.query_id = Some(12);
    t.list(&q, None).await.unwrap();
    let reqs: Vec<String> = server
        .received_requests()
        .await
        .unwrap()
        .iter()
        .filter(|r| r.url.path() == "/issues.json")
        .map(|r| r.url.query().unwrap_or("").to_owned())
        .collect();
    assert!(
        reqs[0].contains("project_id=client-site")
            && reqs[0].contains("status_id=*")
            && !reqs[0].contains("assigned_to_id")
    );
    assert!(reqs[1].contains("query_id=12") && !reqs[1].contains("status_id"));
}

#[tokio::test]
async fn detail_keeps_textile_preformatted_and_only_notes_become_comments() {
    let server = MockServer::start().await;
    mount(&server, "GET", "/issue_statuses.json", 200, "redmine/issue_statuses.json").await;
    mount(&server, "GET", "/issues/4567.json", 200, "redmine/issue.json").await;
    let d = rm(&server, json!({})).get(&r()).await.unwrap();
    assert_eq!(d.body_format, BodyFormat::Textile);
    assert!(d.body_md.starts_with("h1. Steps"));
    assert!(d.body_html.starts_with("<pre>"));
    assert_eq!(d.comments.len(), 2); // the journal without notes is skipped
    assert_eq!(d.comments[0].author.name, "Carol Client");
    assert!(d.comments[1].body_html.contains("&lt;b&gt;now&lt;/b&gt;"), "{}", d.comments[1].body_html);
    assert_eq!(d.parent.unwrap().key, "4500");
    let q = server.received_requests().await.unwrap();
    assert!(q.iter().any(|r| r.url.query().unwrap_or("").contains("include=journals")));
}

#[tokio::test]
async fn markdown_accounts_render_markdown() {
    let server = MockServer::start().await;
    mount(&server, "GET", "/issue_statuses.json", 200, "redmine/issue_statuses.json").await;
    mount(&server, "GET", "/issues/4567.json", 200, "redmine/issue.json").await;
    let d = rm(&server, json!({"text_format": "markdown"})).get(&r()).await.unwrap();
    assert_eq!(d.body_format, BodyFormat::Markdown);
    assert!(d.body_html.contains("<em>save</em>"));
}

#[tokio::test]
async fn transitions_are_the_allowed_statuses_minus_the_current_one() {
    let server = MockServer::start().await;
    mount(&server, "GET", "/issue_statuses.json", 200, "redmine/issue_statuses.json").await;
    mount(&server, "GET", "/issues/4567.json", 200, "redmine/issue.json").await;
    let ts = rm(&server, json!({})).transitions(&r()).await.unwrap();
    assert_eq!(ts.iter().map(|t| t.id.as_str()).collect::<Vec<_>>(), vec!["2", "3", "5"]);
    assert_eq!(ts[2].to.category, StatusCategory::Done);
    assert!(ts.iter().all(|t| !t.needs_fields));
    let q = server.received_requests().await.unwrap();
    assert!(q.iter().any(|r| r.url.query().unwrap_or("").contains("include=allowed_statuses")));
}

#[tokio::test]
async fn status_put_sends_status_id_and_returns_the_fresh_ticket() {
    let server = MockServer::start().await;
    mount(&server, "GET", "/issue_statuses.json", 200, "redmine/issue_statuses.json").await;
    Mock::given(method("PUT"))
        .and(path("/issues/4567.json"))
        .and(body_partial_json(json!({"issue": {"status_id": 2}})))
        .respond_with(ResponseTemplate::new(204))
        .expect(1)
        .mount(&server)
        .await;
    mount(&server, "GET", "/issues/4567.json", 200, "redmine/issue_after_move.json").await;
    let t = rm(&server, json!({})).transition(&r(), "2", None).await.unwrap();
    assert_eq!(t.status.name, "In Progress");
}

#[tokio::test]
async fn a_422_is_surfaced_with_the_redmine_messages() {
    let server = MockServer::start().await;
    Mock::given(method("PUT"))
        .and(path("/issues/4567.json"))
        .respond_with(
            ResponseTemplate::new(422).set_body_string(fixture_text("redmine/issue_error_422.json")),
        )
        .mount(&server)
        .await;
    let e = rm(&server, json!({})).transition(&r(), "5", None).await.unwrap_err();
    assert_eq!(e.code, ErrorCode::InvalidArgument);
    assert!(e.message.contains("Target version cannot be blank"), "{}", e.message);
    assert_eq!(e.detail.unwrap()["errors"][0], "Assignee is invalid");
}

#[tokio::test]
async fn notes_and_assignment() {
    let server = MockServer::start().await;
    mount(&server, "GET", "/issue_statuses.json", 200, "redmine/issue_statuses.json").await;
    mount(&server, "GET", "/users/current.json", 200, "redmine/current_user.json").await;
    Mock::given(method("PUT"))
        .and(path("/issues/4567.json"))
        .respond_with(ResponseTemplate::new(204))
        .mount(&server)
        .await;
    mount(&server, "GET", "/issues/4567.json", 200, "redmine/issue.json").await;
    let t = rm(&server, json!({}));
    t.comment(&r(), "Done on my side").await.unwrap();
    t.assign(&r(), Assignee::Me).await.unwrap();
    t.assign(&r(), Assignee::None).await.unwrap();
    let puts = bodies(&server, "PUT", "/issues/4567.json").await;
    assert_eq!(puts[0], json!({"issue": {"notes": "Done on my side"}}));
    assert_eq!(puts[1], json!({"issue": {"assigned_to_id": 7}}));
    assert_eq!(puts[2], json!({"issue": {"assigned_to_id": ""}}));
}

#[tokio::test]
async fn columns_group_statuses_by_category() {
    let server = MockServer::start().await;
    mount(&server, "GET", "/issue_statuses.json", 200, "redmine/issue_statuses.json").await;
    let cols = rm(&server, json!({})).columns(&TrackerBinding::default()).await.unwrap();
    let names: Vec<_> = cols.iter().map(|c| c.name.as_str()).collect();
    assert_eq!(names, vec!["To do", "In progress", "In review", "Done"]);
    assert_eq!(cols[3].match_names, vec!["Resolved", "Closed", "Rejected"]);
}

#[tokio::test]
async fn unauthorized_is_needs_auth() {
    let server = MockServer::start().await;
    Mock::given(path("/users/current.json")).respond_with(ResponseTemplate::new(401)).mount(&server).await;
    assert_eq!(rm(&server, json!({})).me().await.unwrap_err().code, ErrorCode::NeedsAuth);
}

#[tokio::test]
async fn me_does_not_leak_the_api_key() {
    let server = MockServer::start().await;
    mount(&server, "GET", "/users/current.json", 200, "redmine/current_user.json").await;
    let me = rm(&server, json!({})).me().await.unwrap();
    assert_eq!(me.login.as_deref(), Some("louis"));
    assert!(!format!("{me:?}").contains("must-never-leak"));
}

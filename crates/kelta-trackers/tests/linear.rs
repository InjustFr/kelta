//! Linear: one GraphQL endpoint, filters per view, cursor pagination, moves by team state.

mod support;

use kelta_proto::error::ErrorCode;
use kelta_proto::settings::TrackerBinding;
use kelta_proto::tracker::{Assignee, Cursor, StatusCategory};
use serde_json::json;
use support::*;
use wiremock::matchers::{body_string_contains, header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn lin(server: &MockServer) -> std::sync::Arc<dyn kelta_proto::api::Tracker> {
    tracker("linear-acme", "linear", &server.uri(), json!({}))
}

fn r() -> kelta_proto::tracker::TicketRef {
    tref("linear-acme", "ENG-12", "iss-uuid-12")
}

async fn gql_bodies(server: &MockServer, needle: &str) -> Vec<serde_json::Value> {
    bodies(server, "POST", "/graphql")
        .await
        .into_iter()
        .filter(|b| b["query"].as_str().is_some_and(|q| q.contains(needle)))
        .collect()
}

#[tokio::test]
async fn list_sends_the_raw_key_filters_to_me_and_maps_fields() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/graphql"))
        .and(header("authorization", TOKEN))
        .respond_with(ResponseTemplate::new(200).set_body_string(fixture_text("linear/issues_p1.json")))
        .mount(&server)
        .await;
    let page = lin(&server).list(&view("mine"), None).await.unwrap();
    assert_eq!(page.items.len(), 2);
    let t = &page.items[0];
    assert_eq!((t.r#ref.key.as_str(), t.r#ref.id.as_str()), ("ENG-12", "iss-uuid-12"));
    assert_eq!(t.status.category, StatusCategory::InProgress);
    assert_eq!(t.priority.as_deref(), Some("High"));
    assert_eq!(t.labels, vec!["bug"]);
    assert_eq!(t.assignee.as_ref().unwrap().login.as_deref(), Some("louis"));
    assert_eq!(t.project_hint.as_deref(), Some("ENG"));
    assert_eq!(page.items[1].status.category, StatusCategory::Todo);
    assert!(page.items[1].priority.is_none() && page.items[1].assignee.is_none());
    let f = &gql_bodies(&server, "issues(filter").await[0]["variables"]["filter"];
    assert_eq!(f["assignee"], json!({"isMe": {"eq": true}}));
    assert_eq!(f["state"]["type"]["nin"], json!(["completed", "canceled"]));
}

#[tokio::test]
async fn team_and_project_views_filter_server_side() {
    let server = MockServer::start().await;
    linear_mocks(&server).await;
    let mut v = view("eng");
    v.team = Some("ENG".into());
    v.project = Some("Website".into());
    v.scope = Some("all".into());
    v.labels = Some(vec!["bug".into()]);
    lin(&server).list(&v, None).await.unwrap();
    let f = &gql_bodies(&server, "issues(filter").await[0]["variables"]["filter"];
    assert!(f.get("assignee").is_none());
    assert_eq!(f["team"], json!({"key": {"eq": "ENG"}}));
    assert_eq!(f["project"], json!({"name": {"eq": "Website"}}));
    assert_eq!(f["labels"], json!({"name": {"in": ["bug"]}}));
}

#[tokio::test]
async fn pagination_follows_cursors_and_stops_at_the_cap() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(body_string_contains("\"after\":\"cur-1\""))
        .respond_with(ResponseTemplate::new(200).set_body_string(fixture_text("linear/issues_p2.json")))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200).set_body_string(fixture_text("linear/issues_p1.json")))
        .mount(&server)
        .await;
    let t = lin(&server);
    let p1 = t.list(&view("mine"), None).await.unwrap();
    let next = p1.next.expect("more pages");
    assert_eq!(next, Cursor::After("1:cur-1".into()));
    let p2 = t.list(&view("mine"), Some(next)).await.unwrap();
    assert_eq!(p2.items.len(), 1);
    assert!(p2.next.is_none(), "hasNextPage=false ends the list");
    // page 20 still reports hasNextPage: the cap withholds the cursor
    let capped = t.list(&view("mine"), Some(Cursor::After("19:cur-x".into()))).await.unwrap();
    assert!(capped.next.is_none());
    assert_eq!(
        t.list(&view("mine"), Some(Cursor::Page(2))).await.unwrap_err().code,
        ErrorCode::InvalidArgument
    );
}

#[tokio::test]
async fn detail_renders_markdown_and_lists_comments() {
    let server = MockServer::start().await;
    linear_mocks(&server).await;
    let d = lin(&server).get(&r()).await.unwrap();
    assert!(d.body_html.contains("<li>step one</li>"));
    assert!(!d.body_html.contains("<script"), "sanitized");
    assert!(d.body_md.starts_with("# Login"));
    assert_eq!(d.comments.len(), 2);
    assert!(d.comments[0].body_html.contains("<strong>note</strong>"));
    assert_eq!(gql_bodies(&server, "comments(last").await[0]["variables"]["id"], "iss-uuid-12");
}

#[tokio::test]
async fn moves_resolve_state_ids_from_the_team_by_name() {
    let server = MockServer::start().await;
    linear_mocks(&server).await;
    let t = lin(&server);
    let ts = t.transitions(&r()).await.unwrap();
    assert_eq!(
        ts.iter().map(|t| t.name.as_str()).collect::<Vec<_>>(),
        vec!["Backlog", "Todo", "In Review", "Done", "Canceled"],
        "position order, current state left out"
    );
    let review = ts.iter().find(|t| t.name == "In Review").unwrap();
    assert_eq!(review.to.category, StatusCategory::InReview);
    assert_eq!(ts.iter().find(|t| t.name == "Done").unwrap().to.category, StatusCategory::Done);
    let moved = t.transition(&r(), &review.id, None).await.unwrap();
    assert_eq!(moved.status.name, "In Review");
    let put = &gql_bodies(&server, "issueUpdate").await[0]["variables"];
    assert_eq!(put["id"], "iss-uuid-12");
    assert_eq!(put["input"], json!({"stateId": "st-review"}));
}

#[tokio::test]
async fn columns_are_workflow_states_deduplicated_by_name_or_the_binding() {
    let server = MockServer::start().await;
    linear_mocks(&server).await;
    let mut v = view("eng");
    v.team = Some("ENG".into());
    let cols =
        lin(&server).columns(&TrackerBinding { views: vec![v], ..TrackerBinding::default() }).await.unwrap();
    assert_eq!(
        cols.iter().map(|c| c.name.as_str()).collect::<Vec<_>>(),
        vec!["Backlog", "Todo", "In Progress", "Done"]
    );
    assert_eq!(cols[2].category, StatusCategory::InProgress);
    assert_eq!(cols.iter().map(|c| c.order).collect::<Vec<_>>(), vec![0, 1, 2, 3]);
    let f = &gql_bodies(&server, "workflowStates").await[0]["variables"]["filter"];
    assert_eq!(f, &json!({"team": {"key": {"eq": "ENG"}}}));
}

#[tokio::test]
async fn assign_and_comment() {
    let server = MockServer::start().await;
    linear_mocks(&server).await;
    let t = lin(&server);
    t.assign(&r(), Assignee::Me).await.unwrap();
    t.assign(&r(), Assignee::None).await.unwrap();
    let ups = gql_bodies(&server, "issueUpdate").await;
    assert_eq!(ups[0]["variables"]["input"], json!({"assigneeId": "user-1"}));
    assert_eq!(ups[1]["variables"]["input"], json!({"assigneeId": null}));
    t.comment(&r(), "Taking this").await.unwrap();
    assert_eq!(
        gql_bodies(&server, "commentCreate").await[0]["variables"]["input"],
        json!({"issueId": "iss-uuid-12", "body": "Taking this"})
    );
}

#[tokio::test]
async fn bearer_auth_is_for_oauth_tokens() {
    let server = MockServer::start().await;
    Mock::given(header("authorization", format!("Bearer {TOKEN}").as_str()))
        .respond_with(ResponseTemplate::new(200).set_body_string(fixture_text("linear/viewer.json")))
        .mount(&server)
        .await;
    let t = tracker("linear-acme", "linear", &server.uri(), json!({"auth": "bearer"}));
    assert_eq!(t.me().await.unwrap().id, "user-1");
}

#[tokio::test]
async fn auth_errors_and_rate_limits() {
    let server = MockServer::start().await;
    Mock::given(path("/graphql")).respond_with(ResponseTemplate::new(401)).mount(&server).await;
    assert_eq!(lin(&server).me().await.unwrap_err().code, ErrorCode::NeedsAuth);

    // Linear also answers a bad key with 400 + AUTHENTICATION_ERROR
    let bad = MockServer::start().await;
    Mock::given(path("/graphql"))
        .respond_with(ResponseTemplate::new(400).set_body_json(
            json!({"errors": [{"message": "x", "extensions": {"code": "AUTHENTICATION_ERROR"}}]}),
        ))
        .mount(&bad)
        .await;
    assert_eq!(lin(&bad).me().await.unwrap_err().code, ErrorCode::NeedsAuth);

    // spent quota: HTTP 400 + RATELIMITED, reset in epoch milliseconds (an hour away: fails fast)
    let limited = MockServer::start().await;
    let reset =
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis() + 3_600_000;
    Mock::given(path("/graphql"))
        .respond_with(
            ResponseTemplate::new(400)
                .insert_header("x-ratelimit-requests-reset", reset.to_string().as_str())
                .set_body_json(json!({"errors": [{"message": "x", "extensions": {"code": "RATELIMITED"}}]})),
        )
        .mount(&limited)
        .await;
    let e = lin(&limited).me().await.unwrap_err();
    assert_eq!(e.code, ErrorCode::RateLimited);
    assert!(e.retry_after_ms.unwrap() > 3_000_000);
}

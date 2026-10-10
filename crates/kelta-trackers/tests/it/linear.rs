//! Linear: one GraphQL endpoint, filters per view, cursor pagination, moves by team state.

use crate::support::*;
use kelta_proto::error::ErrorCode;
use kelta_proto::settings::TrackerBinding;
use kelta_proto::tracker::{Assignee, Cursor, StatusCategory, Who};
use serde_json::json;
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

#[tokio::test]
async fn who_and_current_cycle_shape_the_filter() {
    let server = MockServer::start().await;
    linear_mocks(&server).await;
    let t = lin(&server);
    for (who, legacy) in
        [(Who::Mine, "all"), (Who::Unassigned, "assigned_to_me"), (Who::Anyone, "assigned_to_me")]
    {
        let mut v = view("v");
        v.who = Some(who);
        v.scope = Some(legacy.into());
        v.current_iteration = who == Who::Anyone;
        t.list(&v, None).await.unwrap();
    }
    let f: Vec<_> = gql_bodies(&server, "issues(filter")
        .await
        .into_iter()
        .map(|b| b["variables"]["filter"].clone())
        .collect();
    assert_eq!(f[0]["assignee"], json!({"isMe": {"eq": true}}));
    assert_eq!(f[1]["assignee"], json!({"null": true}));
    assert!(f[0].get("cycle").is_none() && f[1].get("cycle").is_none());
    assert!(f[2].get("assignee").is_none());
    assert_eq!(f[2]["cycle"], json!({"isActive": {"eq": true}}));
}

#[tokio::test]
async fn sources_offer_teams_cycles_and_projects() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(body_string_contains("teams(filter"))
        .respond_with(ResponseTemplate::new(200).set_body_string(fixture_text("linear/sources.json")))
        .mount(&server)
        .await;
    let hits = lin(&server).sources("e").await.unwrap();
    let got: Vec<_> = hits.iter().map(|h| (h.kind.as_str(), h.view.id.as_str(), h.label.as_str())).collect();
    assert_eq!(
        got,
        [
            ("team", "team-ENG", "Engineering"),
            ("cycle", "team-ENG-cycle", "Engineering current cycle"),
            ("team", "team-OPS", "Operations"),
            ("project", "project-prj-1", "Website"),
        ],
        "no cycle variant for a team without cycles"
    );
    assert!(hits.iter().all(|h| h.view.who == Some(Who::Mine)));
    assert_eq!((hits[1].view.team.as_deref(), hits[1].view.current_iteration), (Some("ENG"), true));
    assert_eq!((hits[0].view.current_iteration, hits[3].view.project.as_deref()), (false, Some("Website")));
    let v = &gql_bodies(&server, "teams(filter").await[0]["variables"];
    assert_eq!(v["projects"], json!({"name": {"containsIgnoreCase": "e"}}));
    assert_eq!(
        v["teams"],
        json!({"or": [{"name": {"containsIgnoreCase": "e"}}, {"key": {"containsIgnoreCase": "e"}}]})
    );
}

#[tokio::test]
async fn planning_fields_map_priority_cycle_estimate_due_and_start() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/graphql"))
        .respond_with(ResponseTemplate::new(200).set_body_string(fixture_text("linear/issues_planning.json")))
        .mount(&server)
        .await;
    let items = lin(&server).list(&view("mine"), None).await.unwrap().items;
    let [a, b, c, d] = &items[..] else { panic!("four issues") };
    // urgent → 0, low → 3, medium → 2, none → None
    assert_eq!([a, b, c, d].map(|t| t.priority_rank), [Some(0), Some(3), Some(2), None]);
    let cy = a.sprint.as_ref().unwrap();
    assert_eq!((cy.id.as_str(), cy.name.as_str(), cy.active), ("cyc-1", "Sprint 12", true));
    assert_eq!(cy.ends_at.as_deref(), Some("2026-10-16T00:00:00.000Z"));
    let cy = b.sprint.as_ref().unwrap();
    assert_eq!((cy.name.as_str(), cy.active), ("Cycle 11", false), "unnamed cycle falls back to its number");
    assert!(c.sprint.is_none() && d.sprint.is_none());
    assert_eq!([a, b, c, d].map(|t| t.estimate.as_deref()), [Some("3"), Some("2.5"), None, None]);
    assert_eq!([a, b, c, d].map(|t| t.due.as_deref()), [Some("2026-10-14"), None, None, None]);
    // startedAt only for a started state; otherwise (unstarted, or started without it) updatedAt
    assert_eq!(a.status_since.as_deref(), Some("2026-10-02T09:00:00.000Z"));
    for t in [b, c, d] {
        assert_eq!(t.status_since.as_deref(), Some("2026-10-05T08:00:00.000Z"));
    }
    let q = &gql_bodies(&server, "issues(filter").await[0]["query"];
    assert!(
        ["priority ", "estimate", "dueDate", "startedAt", "cycle {"]
            .iter()
            .all(|f| q.as_str().unwrap().contains(f))
    );
}

#[tokio::test]
async fn search_ors_title_and_description_into_the_view_filter() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/graphql"))
        .respond_with(ResponseTemplate::new(200).set_body_string(fixture_text("linear/issues_p1.json")))
        .mount(&server)
        .await;
    assert_eq!(lin(&server).search(&view("mine"), "login").await.unwrap().len(), 2);
    let f = &gql_bodies(&server, "issues(filter").await[0]["variables"]["filter"];
    assert_eq!(f["assignee"], json!({"isMe": {"eq": true}}));
    let m = |k: &str| json!({k: {"containsIgnoreCase": "login"}});
    assert_eq!(f["or"], json!([m("title"), m("description")]));
}

//! Jira Cloud + Data Center against recorded-shape fixtures (wiremock, no network).

use crate::support::*;
use kelta_proto::error::ErrorCode;
use kelta_proto::settings::{ColumnSpec, TrackerBinding};
use kelta_proto::tracker::{Assignee, BodyFormat, Cursor, StatusCategory};
use serde_json::json;
use wiremock::matchers::{body_partial_json, header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn cloud(server: &MockServer) -> std::sync::Arc<dyn kelta_proto::api::Tracker> {
    tracker("jira-acme", "jira", &server.uri(), json!({"flavor": "cloud", "email": "louis@acme.test"}))
}

fn dc(server: &MockServer) -> std::sync::Arc<dyn kelta_proto::api::Tracker> {
    tracker("jira-dc", "jira", &server.uri(), json!({"flavor": "dc"}))
}

fn jql_view() -> kelta_proto::settings::TrackerView {
    let mut v = view("mine");
    v.jql = Some("project = SHOP AND assignee = currentUser() ORDER BY updated DESC".into());
    v
}

#[tokio::test]
async fn flavor_is_detected_from_server_info_and_cached() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/rest/api/2/serverInfo"))
        .respond_with(ResponseTemplate::new(200).set_body_string(fixture_text("jira/serverinfo_cloud.json")))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/rest/api/3/myself"))
        .and(header("authorization", "Basic bG91aXNAYWNtZS50ZXN0OnRvay0xMjM="))
        .respond_with(ResponseTemplate::new(200).set_body_string(fixture_text("jira/myself_cloud.json")))
        .expect(1)
        .mount(&server)
        .await;
    mount(&server, "POST", "/rest/api/3/search/jql", 200, "jira/search_jql_p2.json").await;
    let t = tracker("jira-acme", "jira", &server.uri(), json!({"email": "louis@acme.test"}));
    let me = t.me().await.unwrap();
    assert_eq!(me.id, "5b10ac8d82e05b22cc7d4ef5");
    // Further calls reuse the cached flavor: serverInfo is requested exactly once (expect(1) above).
    t.list(&jql_view(), None).await.unwrap();
    t.list(&jql_view(), None).await.unwrap();
}

#[tokio::test]
async fn dc_flavor_switches_api_version_and_auth() {
    let server = MockServer::start().await;
    mount(&server, "GET", "/rest/api/2/serverInfo", 200, "jira/serverinfo_dc.json").await;
    Mock::given(method("GET"))
        .and(path("/rest/api/2/myself"))
        .and(header("authorization", "Bearer tok-123"))
        .respond_with(ResponseTemplate::new(200).set_body_string(fixture_text("jira/myself_dc.json")))
        .expect(1)
        .mount(&server)
        .await;
    let t = tracker("jira-dc", "jira", &server.uri(), json!({}));
    assert_eq!(t.me().await.unwrap().id, "louis");
}

#[tokio::test]
async fn unreachable_server_info_falls_back_to_the_host_name() {
    let server = MockServer::start().await;
    Mock::given(path("/rest/api/2/serverInfo")).respond_with(ResponseTemplate::new(401)).mount(&server).await;
    mount(&server, "GET", "/rest/api/2/myself", 200, "jira/myself_dc.json").await;
    // 127.0.0.1 is not *.atlassian.net: Data Center, bearer.
    let t = tracker("jira-dc", "jira", &server.uri(), json!({}));
    assert_eq!(t.me().await.unwrap().id, "louis");
}

#[tokio::test]
async fn cloud_search_never_sends_a_token_on_the_first_page_and_stops_without_one() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/rest/api/3/search/jql"))
        .and(body_partial_json(json!({"nextPageToken": "tok-page-2"})))
        .respond_with(ResponseTemplate::new(200).set_body_string(fixture_text("jira/search_jql_p2.json")))
        .mount(&server)
        .await;
    mount(&server, "POST", "/rest/api/3/search/jql", 200, "jira/search_jql_p1.json").await;
    let t = cloud(&server);
    let p1 = t.list(&jql_view(), None).await.unwrap();
    assert_eq!(p1.items.len(), 2);
    let first = &bodies(&server, "POST", "/rest/api/3/search/jql").await[0];
    assert!(first.get("nextPageToken").is_none(), "first request must not carry nextPageToken: {first}");
    assert!(first["fields"].as_array().unwrap().iter().any(|f| f == "summary"), "explicit fields");
    assert_eq!(first["jql"], jql_view().jql.unwrap());
    let next = p1.next.clone().expect("second page");
    let p2 = t.list(&jql_view(), Some(next)).await.unwrap();
    assert_eq!(p2.items.len(), 1);
    assert_eq!(p2.items[0].r#ref.key, "SHOP-144");
    assert!(p2.next.is_none(), "isLast / missing token ends the listing");
    let all = bodies(&server, "POST", "/rest/api/3/search/jql").await;
    assert_eq!(all[1]["nextPageToken"], "tok-page-2");
}

#[tokio::test]
async fn cloud_search_stops_on_an_empty_page_even_when_a_token_is_returned() {
    let server = MockServer::start().await;
    mount(&server, "POST", "/rest/api/3/search/jql", 200, "jira/search_jql_empty_with_token.json").await;
    let page = cloud(&server).list(&jql_view(), None).await.unwrap();
    assert!(page.items.is_empty());
    assert!(page.next.is_none());
}

#[tokio::test]
async fn cloud_search_is_capped_at_twenty_pages() {
    let server = MockServer::start().await;
    mount(&server, "POST", "/rest/api/3/search/jql", 200, "jira/search_jql_endless.json").await;
    let t = cloud(&server);
    let mut cursor = None;
    let mut pages = 0;
    loop {
        let p = t.list(&jql_view(), cursor).await.unwrap();
        pages += 1;
        assert!(pages <= 25, "runaway pagination");
        match p.next {
            Some(c) => cursor = Some(c),
            None => break,
        }
    }
    assert_eq!(pages, 20);
    assert_eq!(count(&server, "POST", "/rest/api/3/search/jql").await, 20);
}

#[tokio::test]
async fn cloud_ticket_mapping() {
    let server = MockServer::start().await;
    mount(&server, "POST", "/rest/api/3/search/jql", 200, "jira/search_jql_p1.json").await;
    let page = cloud(&server).list(&jql_view(), None).await.unwrap();
    let t = &page.items[0];
    assert_eq!(t.r#ref.key, "SHOP-142");
    assert_eq!(t.r#ref.id, "10142");
    assert_eq!(t.status.category, StatusCategory::Todo);
    assert_eq!(t.status.name, "To Do");
    assert_eq!(t.kind.as_deref(), Some("Story"));
    assert_eq!(t.priority.as_deref(), Some("High"));
    assert_eq!(t.labels, vec!["backend"]);
    assert_eq!(t.project_hint.as_deref(), Some("SHOP"));
    assert_eq!(t.updated_at, "2026-09-30T10:15:00.000+02:00");
    assert_eq!(t.assignee.as_ref().unwrap().name, "Louis Duprat");
    assert_eq!(t.url, format!("{}/browse/SHOP-142", server.uri()));
    // "In Review" is "indeterminate" in Jira: promoted to InReview by name.
    assert_eq!(page.items[1].status.category, StatusCategory::InReview);
    assert!(page.items[1].assignee.is_none());
}

#[tokio::test]
async fn dc_search_uses_start_at() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/rest/api/2/search"))
        .and(body_partial_json(json!({"startAt": 2})))
        .respond_with(ResponseTemplate::new(200).set_body_string(fixture_text("jira/search_dc_p2.json")))
        .mount(&server)
        .await;
    mount(&server, "POST", "/rest/api/2/search", 200, "jira/search_dc_p1.json").await;
    let t = dc(&server);
    let p1 = t.list(&jql_view(), None).await.unwrap();
    assert_eq!(p1.items.len(), 2);
    assert_eq!(p1.next, Some(Cursor::Offset(2)));
    let p2 = t.list(&jql_view(), p1.next).await.unwrap();
    assert_eq!(p2.items.len(), 1);
    assert!(p2.next.is_none());
    let b = bodies(&server, "POST", "/rest/api/2/search").await;
    assert_eq!(b[0]["startAt"], 0);
    assert!(b[0].get("nextPageToken").is_none());
}

#[tokio::test]
async fn list_requires_a_jql() {
    let server = MockServer::start().await;
    let e = cloud(&server).list(&view("empty"), None).await.unwrap_err();
    assert_eq!(e.code, ErrorCode::InvalidArgument);
}

#[tokio::test]
async fn cloud_detail_converts_adf_and_keeps_the_last_twenty_comments() {
    let server = MockServer::start().await;
    mount(&server, "GET", "/rest/api/3/issue/SHOP-142", 200, "jira/issue_cloud.json").await;
    let d = cloud(&server).get(&tref("jira-acme", "SHOP-142", "10142")).await.unwrap();
    assert_eq!(d.body_format, BodyFormat::Adf);
    assert_eq!(
        d.body_md,
        "Add a **rate limiter** to `/login`.\n\n- 5 attempts / minute / IP\n- return 429 with Retry-After"
    );
    assert!(d.body_html.contains("<strong>rate limiter</strong>"));
    assert!(d.body_html.contains("<li>5 attempts / minute / IP</li>"));
    assert_eq!(d.comments.len(), 20);
    assert!(d.comments[0].body_html.contains("Comment 6"));
    assert!(d.comments[19].body_html.contains("Comment 25"));
    assert_eq!(d.comments[0].author.name, "Alice");
    assert_eq!(d.parent.as_ref().map(|p| p.key.as_str()), Some("SHOP-100"));
}

#[tokio::test]
async fn dc_detail_keeps_wiki_text_and_sanitizes_rendered_html() {
    let server = MockServer::start().await;
    mount(&server, "GET", "/rest/api/2/issue/SHOP-142", 200, "jira/issue_dc.json").await;
    let d = dc(&server).get(&tref("jira-dc", "SHOP-142", "10142")).await.unwrap();
    assert_eq!(d.body_format, BodyFormat::JiraWiki);
    assert!(d.body_md.starts_with("h2. Goal"));
    assert!(d.body_html.contains("<b>rate limiter</b>"));
    assert!(!d.body_html.contains("<script"));
    assert_eq!(d.comments.len(), 2);
    assert!(d.comments[0].body_html.contains("<b>comment</b>"));
}

#[tokio::test]
async fn transitions_are_fetched_never_hard_coded() {
    let server = MockServer::start().await;
    mount(&server, "GET", "/rest/api/3/issue/SHOP-142/transitions", 200, "jira/transitions.json").await;
    let ts = cloud(&server).transitions(&tref("jira-acme", "SHOP-142", "10142")).await.unwrap();
    assert_eq!(ts.iter().map(|t| t.id.as_str()).collect::<Vec<_>>(), vec!["11", "21", "31"]);
    assert_eq!(ts[0].to.category, StatusCategory::InProgress);
    assert_eq!(ts[1].to.category, StatusCategory::InReview);
    assert_eq!(ts[2].to.category, StatusCategory::Done);
    assert_eq!(ts.iter().map(|t| t.needs_fields).collect::<Vec<_>>(), vec![false, false, true]);
    let reqs = server.received_requests().await.unwrap();
    assert!(reqs[0].url.query().unwrap_or("").contains("expand=transitions.fields"));
}

#[tokio::test]
async fn transition_posts_the_id_and_returns_the_fresh_ticket() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/rest/api/3/issue/SHOP-142/transitions"))
        .and(body_partial_json(json!({"transition": {"id": "11"}})))
        .respond_with(ResponseTemplate::new(204))
        .expect(1)
        .mount(&server)
        .await;
    mount(&server, "GET", "/rest/api/3/issue/SHOP-142", 200, "jira/issue_after_transition.json").await;
    let t = cloud(&server).transition(&tref("jira-acme", "SHOP-142", "10142"), "11", None).await.unwrap();
    assert_eq!(t.status.name, "In Progress");
    assert_eq!(t.status.category, StatusCategory::InProgress);
}

#[tokio::test]
async fn transition_with_missing_fields_maps_to_needs_fields() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/rest/api/3/issue/SHOP-142/transitions"))
        .respond_with(
            ResponseTemplate::new(400).set_body_string(fixture_text("jira/transition_error_400.json")),
        )
        .mount(&server)
        .await;
    mount(&server, "GET", "/rest/api/3/issue/SHOP-142/transitions", 200, "jira/transitions.json").await;
    let e = cloud(&server).transition(&tref("jira-acme", "SHOP-142", "10142"), "31", None).await.unwrap_err();
    assert_eq!(e.code, ErrorCode::NeedsFields);
    let f = &e.detail.as_ref().unwrap()["fields"][0];
    assert_eq!(f["id"], "resolution");
    assert_eq!(f["name"], "Resolution");
    assert_eq!(f["message"], "Resolution is required.");
    assert_eq!(f["allowed_values"][0]["name"], "Done");
}

#[tokio::test]
async fn transition_forwards_user_supplied_fields() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/rest/api/3/issue/SHOP-142/transitions"))
        .and(body_partial_json(
            json!({"transition": {"id": "31"}, "fields": {"resolution": {"name": "Done"}}}),
        ))
        .respond_with(ResponseTemplate::new(204))
        .expect(1)
        .mount(&server)
        .await;
    mount(&server, "GET", "/rest/api/3/issue/SHOP-142", 200, "jira/issue_after_transition.json").await;
    cloud(&server)
        .transition(
            &tref("jira-acme", "SHOP-142", "10142"),
            "31",
            Some(json!({"resolution": {"name": "Done"}})),
        )
        .await
        .unwrap();
}

#[tokio::test]
async fn cloud_comments_are_adf_paragraphs_dc_comments_are_text() {
    let server = MockServer::start().await;
    Mock::given(path("/rest/api/3/issue/SHOP-1/comment"))
        .respond_with(ResponseTemplate::new(201).set_body_string("{}"))
        .mount(&server)
        .await;
    Mock::given(path("/rest/api/2/issue/SHOP-1/comment"))
        .respond_with(ResponseTemplate::new(201).set_body_string("{}"))
        .mount(&server)
        .await;
    cloud(&server).comment(&tref("jira-acme", "SHOP-1", "1"), "line one\nline two").await.unwrap();
    let b = &bodies(&server, "POST", "/rest/api/3/issue/SHOP-1/comment").await[0];
    assert_eq!(b["body"]["type"], "doc");
    assert_eq!(b["body"]["content"].as_array().unwrap().len(), 2);
    assert_eq!(b["body"]["content"][1]["content"][0]["text"], "line two");
    dc(&server).comment(&tref("jira-dc", "SHOP-1", "1"), "line one\nline two").await.unwrap();
    assert_eq!(
        bodies(&server, "POST", "/rest/api/2/issue/SHOP-1/comment").await[0]["body"],
        "line one\nline two"
    );
}

#[tokio::test]
async fn assign_uses_account_id_on_cloud_and_name_on_dc() {
    let server = MockServer::start().await;
    mount(&server, "GET", "/rest/api/3/myself", 200, "jira/myself_cloud.json").await;
    mount(&server, "GET", "/rest/api/2/myself", 200, "jira/myself_dc.json").await;
    Mock::given(method("PUT"))
        .and(path("/rest/api/3/issue/SHOP-142/assignee"))
        .respond_with(ResponseTemplate::new(204))
        .mount(&server)
        .await;
    Mock::given(method("PUT"))
        .and(path("/rest/api/2/issue/SHOP-142/assignee"))
        .respond_with(ResponseTemplate::new(204))
        .mount(&server)
        .await;
    mount(&server, "GET", "/rest/api/3/issue/SHOP-142", 200, "jira/issue_assigned_cloud.json").await;
    mount(&server, "GET", "/rest/api/2/issue/SHOP-142", 200, "jira/issue_assigned_dc.json").await;
    let r = tref("jira-acme", "SHOP-142", "10142");
    let t = cloud(&server).assign(&r, Assignee::Me).await.unwrap();
    assert_eq!(t.assignee.unwrap().id, "5b10ac8d82e05b22cc7d4ef5");
    assert_eq!(
        bodies(&server, "PUT", "/rest/api/3/issue/SHOP-142/assignee").await[0],
        json!({"accountId": "5b10ac8d82e05b22cc7d4ef5"})
    );
    dc(&server).assign(&r, Assignee::User { id: "alice".into() }).await.unwrap();
    assert_eq!(
        bodies(&server, "PUT", "/rest/api/2/issue/SHOP-142/assignee").await[0],
        json!({"name": "alice"})
    );
    cloud(&server).assign(&r, Assignee::None).await.unwrap();
    assert_eq!(
        bodies(&server, "PUT", "/rest/api/3/issue/SHOP-142/assignee").await[1],
        json!({"accountId": null})
    );
}

#[tokio::test]
async fn columns_come_from_project_statuses_the_board_or_the_binding() {
    let server = MockServer::start().await;
    mount(&server, "GET", "/rest/api/3/project/SHOP/statuses", 200, "jira/project_statuses.json").await;
    mount(&server, "GET", "/rest/api/3/status", 200, "jira/statuses.json").await;
    mount(&server, "GET", "/rest/agile/1.0/board/7/configuration", 200, "jira/board_config.json").await;
    let t = cloud(&server);

    // 1. statuses of the project named in the JQL
    let b = TrackerBinding { views: vec![jql_view()], ..TrackerBinding::default() };
    let cols = t.columns(&b).await.unwrap();
    assert_eq!(
        cols.iter().map(|c| c.name.as_str()).collect::<Vec<_>>(),
        vec!["To do", "In progress", "In review", "Done"]
    );
    assert_eq!(cols[2].match_names, vec!["In Review"]);

    // 2. the board when the view names one
    let mut v = jql_view();
    v.board_id = Some(7);
    let cols = t.columns(&TrackerBinding { views: vec![v], ..TrackerBinding::default() }).await.unwrap();
    assert_eq!(
        cols.iter().map(|c| c.name.as_str()).collect::<Vec<_>>(),
        vec!["Backlog", "Doing", "Code Review", "Shipped"]
    );
    assert_eq!(cols[1].category, StatusCategory::InProgress);
    assert_eq!(cols[2].category, StatusCategory::InReview);
    assert_eq!(cols[3].match_names, vec!["Done"]);

    // 3. no project in the JQL: the global status list
    let mut v = view("all");
    v.jql = Some("assignee = currentUser()".into());
    assert_eq!(
        t.columns(&TrackerBinding { views: vec![v], ..TrackerBinding::default() }).await.unwrap().len(),
        4
    );

    // 4. explicit columns win and cost no request
    let before = server.received_requests().await.unwrap().len();
    let b = TrackerBinding {
        columns: vec![ColumnSpec {
            id: "todo".into(),
            label: "To do".into(),
            categories: vec![StatusCategory::Todo],
            names: vec![],
        }],
        ..TrackerBinding::default()
    };
    assert_eq!(t.columns(&b).await.unwrap()[0].id, "todo");
    assert_eq!(server.received_requests().await.unwrap().len(), before);
}

#[tokio::test]
async fn unauthorized_is_needs_auth_and_invalidates_the_secret() {
    let server = MockServer::start().await;
    Mock::given(path("/rest/api/3/myself")).respond_with(ResponseTemplate::new(401)).mount(&server).await;
    let s = secrets();
    let acc = account("jira", &server.uri(), json!({"flavor": "cloud", "email": "louis@acme.test"}));
    let t = tracker_with("jira-acme", &acc, s.clone());
    let e = t.me().await.unwrap_err();
    assert_eq!(e.code, ErrorCode::NeedsAuth);
    assert_eq!(s.invalidated(), vec!["env:TOK"]);
}

#[tokio::test]
async fn cloud_without_email_needs_auth() {
    let server = MockServer::start().await;
    let t = tracker("jira-acme", "jira", &server.uri(), json!({"flavor": "cloud"}));
    assert_eq!(t.me().await.unwrap_err().code, ErrorCode::NeedsAuth);
}

#[tokio::test]
async fn caps_and_keys() {
    let server = MockServer::start().await;
    let t = cloud(&server);
    assert!(t.caps().board_columns && t.caps().assign && t.caps().comment && t.caps().transitions_need_fetch);
    assert!(!t.caps().projects_v2);
    let r = tref("jira-acme", "SHOP-142", "10142");
    assert_eq!(t.branch_key(&r), "SHOP-142");
    assert_eq!(t.browser_url(&r), format!("{}/browse/SHOP-142", server.uri()));
}

//! Jira Cloud + Data Center against recorded-shape fixtures (wiremock, no network).

use crate::support::*;
use kelta_proto::error::ErrorCode;
use kelta_proto::settings::{ColumnSpec, TrackerBinding, TrackerView};
use kelta_proto::tracker::{Assignee, BodyFormat, Cursor, StatusCategory, Who};
use serde_json::json;
use wiremock::matchers::{body_partial_json, header, method, path, query_param};
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
    let kids: Vec<_> =
        d.children.iter().map(|c| (c.ticket.r#ref.key.as_str(), c.ticket.status.category)).collect();
    assert_eq!(kids, vec![("SHOP-143", StatusCategory::Todo)]);
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
async fn create_files_a_task_in_the_jql_project() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/rest/api/3/issue"))
        .respond_with(ResponseTemplate::new(201).set_body_string(r#"{"id":"10142","key":"SHOP-142"}"#))
        .mount(&server)
        .await;
    mount(&server, "GET", "/rest/api/3/issue/SHOP-142", 200, "jira/issue_assigned_cloud.json").await;
    let mut v = view("mine");
    v.jql = Some("project = SHOP AND assignee = currentUser()".into());
    let t = cloud(&server).create(&v, "Refund export", "line one").await.unwrap();
    assert_eq!(t.r#ref.key, "SHOP-142");
    let b = &bodies(&server, "POST", "/rest/api/3/issue").await[0]["fields"];
    assert_eq!(b["project"]["key"], "SHOP");
    assert_eq!(b["summary"], "Refund export");
    assert_eq!(b["issuetype"]["name"], "Task");
    assert_eq!(b["description"]["type"], "doc");
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

async fn jql_sent(
    t: &std::sync::Arc<dyn kelta_proto::api::Tracker>,
    server: &MockServer,
    v: &TrackerView,
) -> String {
    t.list(v, None).await.unwrap();
    let all = bodies(server, "POST", "/rest/api/3/search/jql").await;
    all.last().unwrap()["jql"].as_str().unwrap().to_owned()
}

#[tokio::test]
async fn who_and_iteration_compose_the_jql_and_none_keeps_the_legacy_one() {
    let server = MockServer::start().await;
    mount(&server, "POST", "/rest/api/3/search/jql", 200, "jira/search_jql_p2.json").await;
    let t = cloud(&server);
    let mut v = view("v");
    v.jql = Some("project = SHOP ORDER BY rank".into());
    assert_eq!(jql_sent(&t, &server, &v).await, "project = SHOP ORDER BY rank");
    v.who = Some(Who::Mine);
    assert_eq!(
        jql_sent(&t, &server, &v).await,
        "(project = SHOP) AND assignee = currentUser() ORDER BY rank"
    );
    v.who = Some(Who::Unassigned);
    assert_eq!(jql_sent(&t, &server, &v).await, "(project = SHOP) AND assignee is EMPTY ORDER BY rank");
    v.who = Some(Who::Anyone);
    v.current_iteration = true;
    assert_eq!(jql_sent(&t, &server, &v).await, "(project = SHOP) AND sprint in openSprints() ORDER BY rank");
}

#[tokio::test]
async fn a_view_without_jql_gets_a_default_from_its_project_or_board_or_who() {
    let server = MockServer::start().await;
    mount(&server, "POST", "/rest/api/3/search/jql", 200, "jira/search_jql_p2.json").await;
    mount(&server, "GET", "/rest/agile/1.0/board/7/configuration", 200, "jira/board_config.json").await;
    let t = cloud(&server);
    let mut v = view("v");
    v.project_id = Some("SHOP".into());
    v.who = Some(Who::Mine);
    assert_eq!(
        jql_sent(&t, &server, &v).await,
        "(project = \"SHOP\" AND statusCategory != Done) AND assignee = currentUser() ORDER BY updated DESC"
    );
    let mut b = view("b");
    b.board_id = Some(7);
    b.who = Some(Who::Unassigned);
    assert_eq!(
        jql_sent(&t, &server, &b).await,
        "(filter = 10010 AND statusCategory != Done) AND assignee is EMPTY ORDER BY updated DESC"
    );
    // `closed` (the core's recently-done fetch) flips the open clause, defaults and source JQL alike.
    v.status = Some("closed".into());
    assert_eq!(
        jql_sent(&t, &server, &v).await,
        "(project = \"SHOP\" AND statusCategory = Done) AND assignee = currentUser() ORDER BY updated DESC"
    );
    let src = TrackerView {
        jql: Some("project = SHOP AND statusCategory != Done ORDER BY updated DESC".into()),
        status: Some("closed".into()),
        ..view("s")
    };
    assert_eq!(
        jql_sent(&t, &server, &src).await,
        "project = SHOP AND statusCategory = Done ORDER BY updated DESC"
    );
    let mut m = view("m");
    m.who = Some(Who::Mine);
    assert_eq!(jql_sent(&t, &server, &m).await, "assignee = currentUser() ORDER BY updated DESC");
    // Nothing bounds the search: still an error, as before.
    for bare in [view("n"), TrackerView { who: Some(Who::Anyone), ..view("a") }] {
        assert_eq!(t.list(&bare, None).await.unwrap_err().code, ErrorCode::InvalidArgument);
    }
}

#[tokio::test]
async fn sources_offer_projects_boards_sprints_and_favourite_filters() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/rest/api/3/project/search"))
        .and(query_param("query", "shop"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"values": [
            {"key": "SHOP", "name": "Shop"}
        ]})))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/rest/agile/1.0/board"))
        .and(query_param("name", "shop"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"values": [
            {"id": 7, "name": "SHOP board", "type": "scrum"},
            {"id": 8, "name": "SHOP flow", "type": "kanban"}
        ]})))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/rest/api/3/filter/favourite"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([
            {"id": "10", "name": "Shop hot bugs", "jql": "priority = Highest"},
            {"id": "11", "name": "Other", "jql": "x = y"}
        ])))
        .mount(&server)
        .await;
    let hits = cloud(&server).sources(" Shop ").await.unwrap();
    let got: Vec<_> = hits.iter().map(|h| (h.kind.as_str(), h.view.id.as_str(), h.label.as_str())).collect();
    assert_eq!(
        got,
        vec![
            ("project", "jira-acme-project-SHOP", "Shop"),
            ("board", "jira-acme-board-7", "SHOP board"),
            ("sprint", "jira-acme-board-7-sprint", "SHOP board (current sprint)"),
            ("board", "jira-acme-board-8", "SHOP flow"),
            ("filter", "jira-acme-filter-10", "Shop hot bugs"),
        ]
    );
    assert_eq!(
        hits[0].view.jql.as_deref(),
        Some("project = SHOP AND statusCategory != Done ORDER BY updated DESC")
    );
    assert_eq!((hits[1].view.board_id, hits[1].view.current_iteration), (Some(7), false));
    assert_eq!((hits[2].view.board_id, hits[2].view.current_iteration), (Some(7), true));
    assert_eq!(
        hits[4].view.jql.as_deref(),
        Some("filter = 10 AND statusCategory != Done ORDER BY updated DESC")
    );
    assert_eq!(hits[4].detail.as_deref(), Some("priority = Highest"));
    assert!(hits.iter().all(|h| h.view.who == Some(Who::Mine)));
}

#[tokio::test]
async fn sources_survive_a_site_without_jira_software_and_data_center_filters_projects_locally() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/rest/api/2/project"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([
            {"key": "SHOP", "name": "Shop"}, {"key": "OPS", "name": "Operations"}
        ])))
        .mount(&server)
        .await;
    Mock::given(path("/rest/agile/1.0/board")).respond_with(ResponseTemplate::new(403)).mount(&server).await;
    Mock::given(path("/rest/api/2/filter/favourite"))
        .respond_with(ResponseTemplate::new(403))
        .mount(&server)
        .await;
    let hits = dc(&server).sources("ops").await.unwrap();
    assert_eq!(hits.len(), 1);
    assert_eq!(
        hits[0].view.jql.as_deref(),
        Some("project = OPS AND statusCategory != Done ORDER BY updated DESC")
    );
}

async fn mount_get(server: &MockServer, p: &str, body: serde_json::Value) {
    Mock::given(method("GET"))
        .and(path(p))
        .respond_with(ResponseTemplate::new(200).set_body_json(body))
        .mount(server)
        .await;
}

async fn mount_lookups(server: &MockServer, v: u8) {
    mount_get(
        server,
        &format!("/rest/api/{v}/field"),
        json!([
            {"id": "summary", "name": "Summary", "schema": {"type": "string"}},
            {"id": "customfield_10020", "name": "Sprint",
             "schema": {"type": "array", "custom": "com.pyxis.greenhopper.jira:gh-sprint"}},
            {"id": "customfield_10016", "name": "Story point estimate", "schema": {"type": "number"}},
        ]),
    )
    .await;
    mount_get(
        server,
        &format!("/rest/api/{v}/priority"),
        json!([{"id": "1", "name": "Highest"}, {"id": "2", "name": "High"}, {"id": "3", "name": "Medium"}]),
    )
    .await;
}

fn issue(extra: serde_json::Value) -> serde_json::Value {
    let mut fields = json!({
        "summary": "S", "status": {"id": "1", "name": "To Do"}, "updated": "2026-09-30T10:15:00.000+0200",
        "priority": {"id": "2", "name": "High"},
    });
    fields.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
    json!({"id": "1", "key": "SHOP-1", "fields": fields})
}

#[tokio::test]
async fn cloud_tickets_carry_rank_status_date_sprint_estimate_and_due() {
    let server = MockServer::start().await;
    mount_lookups(&server, 3).await;
    let sprints = json!([
        {"id": 7, "name": "Sprint 7", "state": "closed", "endDate": "2026-09-01T00:00:00.000Z"},
        {"id": 8, "name": "Sprint 8", "state": "active", "endDate": "2026-10-20T10:00:00.000+0200"},
        {"id": 9, "name": "Sprint 9", "state": "future"},
    ]);
    let full = issue(json!({
        "statuscategorychangedate": "2026-09-28T09:00:00.000+0200", "duedate": "2026-10-14",
        "customfield_10020": sprints, "customfield_10016": 5.0, "timeoriginalestimate": 7200,
    }));
    let bare = issue(json!({"priority": {"id": "99", "name": "Custom"}, "timeoriginalestimate": 5400}));
    Mock::given(method("POST"))
        .and(path("/rest/api/3/search/jql"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"issues": [full, bare]})))
        .mount(&server)
        .await;
    let page = cloud(&server).list(&jql_view(), None).await.unwrap();
    let t = &page.items[0];
    assert_eq!(t.priority_rank, Some(1));
    assert_eq!(t.status_since.as_deref(), Some("2026-09-28T09:00:00.000+02:00"));
    let sp = t.sprint.as_ref().unwrap();
    assert_eq!((sp.id.as_str(), sp.name.as_str(), sp.active), ("8", "Sprint 8", true));
    assert_eq!(sp.ends_at.as_deref(), Some("2026-10-20T10:00:00.000+02:00"));
    assert_eq!(t.estimate.as_deref(), Some("5 pts"), "story points win over the time estimate");
    assert_eq!(t.due.as_deref(), Some("2026-10-14"));
    let b = &page.items[1];
    assert_eq!(
        (b.priority_rank, b.status_since.clone(), b.sprint.clone(), b.due.clone()),
        (None, None, None, None)
    );
    assert_eq!(b.estimate.as_deref(), Some("1h 30m"));
    // Only the needed fields are requested, custom ones included; the lookups run once.
    let req = &bodies(&server, "POST", "/rest/api/3/search/jql").await[0];
    let asked: Vec<&str> = req["fields"].as_array().unwrap().iter().filter_map(|f| f.as_str()).collect();
    for f in [
        "statuscategorychangedate",
        "duedate",
        "timeoriginalestimate",
        "customfield_10020",
        "customfield_10016",
    ] {
        assert!(asked.contains(&f), "{f} not requested: {asked:?}");
    }
    cloud(&server).list(&jql_view(), None).await.unwrap();
}

#[tokio::test]
async fn sprint_without_active_state_is_the_latest_and_inactive() {
    let server = MockServer::start().await;
    mount_lookups(&server, 3).await;
    let i = issue(json!({"customfield_10020": [
        {"id": 7, "name": "Sprint 7", "state": "closed"}, {"id": 8, "name": "Sprint 8", "state": "closed"}]}));
    Mock::given(method("POST"))
        .and(path("/rest/api/3/search/jql"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"issues": [i]})))
        .mount(&server)
        .await;
    let sp = cloud(&server).list(&jql_view(), None).await.unwrap().items[0].sprint.clone().unwrap();
    assert_eq!((sp.id.as_str(), sp.active, sp.ends_at), ("8", false, None));
}

#[tokio::test]
async fn dc_reads_the_legacy_string_sprint_and_a_refused_field_lookup_just_drops_the_extras() {
    let server = MockServer::start().await;
    mount_lookups(&server, 2).await;
    let legacy = "com.atlassian.greenhopper.service.sprint.Sprint@1f[id=12,rapidViewId=3,state=ACTIVE,\
                  name=Sprint 12,startDate=2026-10-06T08:00:00.000Z,endDate=2026-10-20T08:00:00.000Z,sequence=12]";
    let i = issue(json!({"customfield_10020": [legacy]}));
    mount_get(&server, "/rest/api/2/issue/SHOP-1", i).await;
    let t = dc(&server).get(&tref("jira-dc", "SHOP-1", "1")).await.unwrap().ticket;
    let sp = t.sprint.unwrap();
    assert_eq!((sp.id.as_str(), sp.name.as_str(), sp.active), ("12", "Sprint 12", true));
    assert_eq!(sp.ends_at.as_deref(), Some("2026-10-20T08:00:00.000Z"));
    assert_eq!(t.priority_rank, Some(1));

    // Lookups refused (403): the ticket still loads, without the discovered fields.
    let server = MockServer::start().await;
    Mock::given(path("/rest/api/2/field")).respond_with(ResponseTemplate::new(403)).mount(&server).await;
    Mock::given(path("/rest/api/2/priority")).respond_with(ResponseTemplate::new(403)).mount(&server).await;
    mount_get(&server, "/rest/api/2/issue/SHOP-1", issue(json!({"duedate": "2026-10-14"}))).await;
    let t = dc(&server).get(&tref("jira-dc", "SHOP-1", "1")).await.unwrap().ticket;
    assert_eq!((t.priority_rank, t.sprint, t.due.as_deref()), (None, None, Some("2026-10-14")));
}

#[tokio::test]
async fn a_server_error_on_lookups_is_retried_and_either_story_points_field_counts() {
    let server = MockServer::start().await;
    Mock::given(path("/rest/api/3/field"))
        .respond_with(ResponseTemplate::new(500))
        .up_to_n_times(1)
        .mount(&server)
        .await;
    mount_get(
        &server,
        "/rest/api/3/field",
        json!([
            {"id": "customfield_10020", "name": "Sprint",
             "schema": {"type": "array", "custom": "com.pyxis.greenhopper.jira:gh-sprint"}},
            {"id": "customfield_10016", "name": "Story Points", "schema": {"type": "number"}},
            {"id": "customfield_10017", "name": "Story point estimate", "schema": {"type": "number"}},
        ]),
    )
    .await;
    mount_get(&server, "/rest/api/3/priority", json!([{"id": "2", "name": "High"}])).await;
    let i = issue(
        json!({"customfield_10017": 3.0, "customfield_10020": [{"id": 1, "name": "S1", "state": "active"}]}),
    );
    Mock::given(method("POST"))
        .and(path("/rest/api/3/search/jql"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"issues": [i]})))
        .mount(&server)
        .await;
    let p = cloud(&server);
    assert!(p.list(&jql_view(), None).await.is_err());
    let t = p.list(&jql_view(), None).await.unwrap().items.remove(0);
    assert_eq!((t.sprint.is_some(), t.estimate.as_deref()), (true, Some("3 pts")));
}

#[tokio::test]
async fn search_ands_an_escaped_text_clause_onto_the_view_jql() {
    let server = MockServer::start().await;
    mount(&server, "POST", "/rest/api/3/search/jql", 200, "jira/search_jql_p2.json").await;
    let mut v = view("v");
    v.jql = Some("project = SHOP ORDER BY rank".into());
    v.who = Some(Who::Mine);
    cloud(&server).search(&v, r#"say "hi""#).await.unwrap();
    let all = bodies(&server, "POST", "/rest/api/3/search/jql").await;
    assert_eq!(
        all.last().unwrap()["jql"],
        r#"((project = SHOP) AND assignee = currentUser()) AND text ~ "say \"hi\"" ORDER BY rank"#
    );
}

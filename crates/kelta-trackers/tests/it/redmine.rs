//! Redmine against fixtures: list/paging, detail with allowed_statuses, status PUT, 422, notes.

use crate::support::*;
use kelta_proto::error::ErrorCode;
use kelta_proto::settings::{TrackerBinding, TrackerView};
use kelta_proto::tracker::{Assignee, BodyFormat, Cursor, StatusCategory, Who};
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
    Mock::given(method("GET"))
        .and(path("/issues.json"))
        .and(query_param("parent_id", "4567"))
        .and(query_param("status_id", "*"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"issues": [{
            "id": 4568, "subject": "Child", "status": {"id": 1, "name": "New"},
            "project": {"id": 1, "name": "Client"}, "updated_on": "2026-10-01T08:30:00Z"
        }]})))
        .mount(&server)
        .await;
    let d = rm(&server, json!({})).get(&r()).await.unwrap();
    assert_eq!(d.children.iter().map(|c| c.ticket.r#ref.key.as_str()).collect::<Vec<_>>(), vec!["4568"]);
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
async fn reassign_picker_and_priority_change() {
    let server = MockServer::start().await;
    mount(&server, "GET", "/issue_statuses.json", 200, "redmine/issue_statuses.json").await;
    mount(&server, "GET", "/issues/4567.json", 200, "redmine/issue.json").await;
    mount_json(
        &server,
        "/enumerations/issue_priorities.json",
        json!({"issue_priorities": [
            {"id": 1, "name": "Low"}, {"id": 2, "name": "Normal"}, {"id": 9, "name": "Old", "active": false},
            {"id": 3, "name": "High"}
        ]}),
    )
    .await;
    mount_json(
        &server,
        "/projects/3/memberships.json",
        json!({"memberships": [
            {"id": 1, "user": {"id": 7, "name": "Louis Dupont"}},
            {"id": 2, "group": {"id": 20, "name": "Developers"}},
            {"id": 3, "user": {"id": 8, "name": "Dave Lee"}}
        ]}),
    )
    .await;
    Mock::given(method("PUT"))
        .and(path("/issues/4567.json"))
        .respond_with(ResponseTemplate::new(204))
        .mount(&server)
        .await;
    let t = rm(&server, json!({}));
    let users = t.assignable_users(&r(), "dave").await.unwrap();
    assert_eq!(users.iter().map(|u| u.id.as_str()).collect::<Vec<_>>(), ["8"]);
    assert_eq!(t.assignable_users(&r(), "").await.unwrap().len(), 2, "groups left out");
    assert_eq!(t.priorities(&r()).await.unwrap(), ["High", "Normal", "Low"]);
    t.set_priority(&r(), "high").await.unwrap();
    assert_eq!(bodies(&server, "PUT", "/issues/4567.json").await[0], json!({"issue": {"priority_id": "3"}}));
    assert_eq!(t.set_priority(&r(), "Old").await.unwrap_err().code, ErrorCode::NotFound);
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

async fn issues_query(
    server: &MockServer,
    t: &std::sync::Arc<dyn kelta_proto::api::Tracker>,
    v: &TrackerView,
) -> String {
    t.list(v, None).await.unwrap();
    let reqs = server.received_requests().await.unwrap();
    let r = reqs.iter().rev().find(|r| r.url.path() == "/issues.json").unwrap();
    r.url.query().unwrap_or("").to_owned()
}

#[tokio::test]
async fn who_sets_assigned_to_id_and_overrides_the_legacy_field() {
    let server = MockServer::start().await;
    mount(&server, "GET", "/issue_statuses.json", 200, "redmine/issue_statuses.json").await;
    Mock::given(method("GET"))
        .and(path("/issues.json"))
        .and(query_param("assigned_to_id", "!*"))
        .respond_with(ResponseTemplate::new(200).set_body_string(fixture_text("redmine/issues_p2.json")))
        .expect(1)
        .mount(&server)
        .await;
    mount(&server, "GET", "/issues.json", 200, "redmine/issues_p2.json").await;
    let t = rm(&server, json!({}));
    let mut v = view("v");
    v.assigned_to = Some("any".into());
    v.who = Some(Who::Mine);
    assert!(issues_query(&server, &t, &v).await.contains("assigned_to_id=me"));
    v.who = Some(Who::Unassigned);
    assert!(issues_query(&server, &t, &v).await.contains("assigned_to_id=%21*"));
    v.who = Some(Who::Anyone);
    v.assigned_to = None; // legacy would say `me`
    assert!(!issues_query(&server, &t, &v).await.contains("assigned_to_id"));
    v.who = None;
    assert!(issues_query(&server, &t, &v).await.contains("assigned_to_id=me"));
}

#[tokio::test]
async fn current_iteration_filters_on_the_projects_current_version() {
    let server = MockServer::start().await;
    mount(&server, "GET", "/issue_statuses.json", 200, "redmine/issue_statuses.json").await;
    mount(&server, "GET", "/issues.json", 200, "redmine/issues_p2.json").await;
    Mock::given(method("GET"))
        .and(path("/projects/client-site/versions.json"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"versions": [
            {"id": 1, "status": "closed", "due_date": "2999-01-01"},
            {"id": 9, "status": "open", "due_date": "2999-02-01"},
            {"id": 8, "status": "open", "due_date": "2999-01-15"},
            {"id": 7, "status": "open", "due_date": "2000-01-01"}
        ]})))
        .mount(&server)
        .await;
    let t = rm(&server, json!({}));
    let mut v = view("v");
    v.project_id = Some("client-site".into());
    v.current_iteration = true;
    assert!(issues_query(&server, &t, &v).await.contains("fixed_version_id=8"));
    // Without a project there is no version to pick: no filter.
    v.project_id = None;
    assert!(!issues_query(&server, &t, &v).await.contains("fixed_version_id"));
}

#[tokio::test]
async fn who_on_a_saved_query_filters_the_returned_page() {
    let server = MockServer::start().await;
    mount(&server, "GET", "/issue_statuses.json", 200, "redmine/issue_statuses.json").await;
    mount(&server, "GET", "/users/current.json", 200, "redmine/current_user.json").await;
    let issue = |id: u64, who: serde_json::Value| json!({"id": id, "subject": "s", "status": {"id": 1, "name": "New"}, "assigned_to": who});
    mount_json(
        &server,
        "/issues.json",
        json!({"total_count": 3, "issues": [
            json!({"id": 1, "subject": "s", "status": {"id": 1, "name": "New"}, "assigned_to": {"id": 7, "name": "Me"}, "fixed_version": {"id": 8}}),
            issue(2, json!({"id": 8, "name": "Other"})),
            issue(3, serde_json::Value::Null),
        ]}),
    )
    .await;
    let t = rm(&server, json!({}));
    let mut v = view("q");
    v.query_id = Some(12);
    let keys =
        |items: Vec<kelta_proto::tracker::Ticket>| items.into_iter().map(|i| i.r#ref.key).collect::<Vec<_>>();
    assert_eq!(keys(t.list(&v, None).await.unwrap().items), ["1", "2", "3"]);
    v.who = Some(Who::Mine);
    assert_eq!(keys(t.list(&v, None).await.unwrap().items), ["1"]);
    v.who = Some(Who::Unassigned);
    assert_eq!(keys(t.list(&v, None).await.unwrap().items), ["3"]);
    // A saved query ignores `fixed_version_id`: the current version is applied to the returned page.
    mount_json(&server, "/projects/p/versions.json", json!({"versions": [{"id": 8, "status": "open"}]}))
        .await;
    v.who = None;
    v.project_id = Some("p".into());
    v.current_iteration = true;
    assert_eq!(keys(t.list(&v, None).await.unwrap().items), ["1"]);
    assert!(!issues_query(&server, &t, &v).await.contains("fixed_version_id"));
}

#[tokio::test]
async fn saved_query_cursor_follows_the_raw_page_not_the_filtered_one() {
    let server = MockServer::start().await;
    mount(&server, "GET", "/issue_statuses.json", 200, "redmine/issue_statuses.json").await;
    mount(&server, "GET", "/users/current.json", 200, "redmine/current_user.json").await;
    let other = |id: u64| json!({"id": id, "subject": "s", "status": {"id": 1, "name": "New"}, "assigned_to": {"id": 8, "name": "O"}});
    mount_json(&server, "/issues.json", json!({"total_count": 200, "issues": [other(1), other(2)]})).await;
    let mut v = view("q");
    v.query_id = Some(12);
    v.who = Some(Who::Mine);
    let page = rm(&server, json!({})).list(&v, None).await.unwrap();
    assert!(page.items.is_empty());
    assert_eq!(page.next, Some(Cursor::Offset(2)));
}

#[tokio::test]
async fn sources_list_projects_and_saved_queries_filtered_by_name() {
    let server = MockServer::start().await;
    mount_json(
        &server,
        "/projects.json",
        json!({"projects": [
            {"id": 3, "name": "Client Site", "identifier": "client-site"},
            {"id": 4, "name": "Intranet", "identifier": "intranet"}
        ]}),
    )
    .await;
    mount_json(
        &server,
        "/queries.json",
        json!({"queries": [
            {"id": 12, "name": "Client bugs", "is_public": true, "project_id": 3},
            {"id": 13, "name": "Mine, all projects", "is_public": false}
        ]}),
    )
    .await;
    let t = rm(&server, json!({}));
    let hits = t.sources("client").await.unwrap();
    let got: Vec<_> = hits.iter().map(|h| (h.kind.as_str(), h.view.id.as_str(), h.label.as_str())).collect();
    assert_eq!(
        got,
        vec![
            ("project", "redmine-client-project-client-site", "Client Site"),
            ("query", "redmine-client-query-12", "Client bugs"),
        ]
    );
    assert_eq!(hits[0].view.project_id.as_deref(), Some("client-site"));
    assert_eq!((hits[1].view.query_id, hits[1].view.project_id.as_deref()), (Some(12), Some("3")));
    assert!(hits.iter().all(|h| h.view.who == Some(Who::Mine)));
    assert_eq!(t.sources("").await.unwrap().len(), 4);
}

async fn mount_json(server: &MockServer, p: &str, body: serde_json::Value) {
    Mock::given(method("GET"))
        .and(path(p))
        .respond_with(ResponseTemplate::new(200).set_body_json(body))
        .mount(server)
        .await;
}

async fn mount_priorities(server: &MockServer) {
    mount_json(
        server,
        "/enumerations/issue_priorities.json",
        json!({"issue_priorities": [
            {"id": 1, "name": "Low"}, {"id": 2, "name": "Normal"}, {"id": 3, "name": "High"}, {"id": 4, "name": "Urgent"}
        ]}),
    )
    .await;
}

fn issue(id: u64, extra: serde_json::Value) -> serde_json::Value {
    let mut i = json!({
        "id": id, "subject": "S", "status": {"id": 1, "name": "New"}, "priority": {"id": 4, "name": "Urgent"},
        "updated_on": "2026-09-30T08:30:00Z",
    });
    i.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
    i
}

#[tokio::test]
async fn tickets_carry_rank_status_date_estimate_due_and_resolved_sprint() {
    let server = MockServer::start().await;
    mount_priorities(&server).await;
    let v = |id: u64, name: &str, status: &str, due: serde_json::Value| json!({"version": {"id": id, "name": name, "status": status, "due_date": due}});
    mount_json(&server, "/versions/1.json", v(1, "v1", "open", json!("2999-01-01"))).await;
    mount_json(&server, "/versions/2.json", v(2, "v2", "open", json!("2000-01-01"))).await;
    mount_json(&server, "/versions/3.json", v(3, "v3", "closed", json!("2999-01-01"))).await;
    mount_json(&server, "/versions/4.json", v(4, "v4", "open", json!(null))).await;
    let with = |id, ver: u64| {
        issue(
            id,
            json!({"fixed_version": {"id": ver, "name": format!("v{ver}")}, "priority": {"id": 1, "name": "Low"}}),
        )
    };
    let first = issue(
        10,
        json!({"fixed_version": {"id": 1, "name": "v1"}, "estimated_hours": 2.5, "due_date": "2026-10-14"}),
    );
    mount_json(
        &server,
        "/issues.json",
        json!({"issues": [first, with(11, 1), with(12, 2), with(13, 3), with(14, 4), issue(15, json!({}))],
               "total_count": 6}),
    )
    .await;
    let items = rm(&server, json!({})).list(&view("v"), None).await.unwrap().items;
    let t = &items[0];
    assert_eq!(t.priority_rank, Some(0), "Urgent is the last position: highest");
    assert_eq!(items[1].priority_rank, Some(3), "Low is the first position: lowest");
    assert_eq!(t.status_since.as_deref(), Some("2026-09-30T08:30:00Z"));
    assert_eq!((t.estimate.as_deref(), t.due.as_deref()), (Some("2.5h"), Some("2026-10-14")));
    let sp = t.sprint.as_ref().unwrap();
    assert_eq!((sp.id.as_str(), sp.name.as_str(), sp.active), ("1", "v1", true));
    assert_eq!(sp.ends_at.as_deref(), Some("2999-01-01"));
    let active: Vec<bool> = items[1..5].iter().map(|i| i.sprint.as_ref().unwrap().active).collect();
    assert_eq!(
        active,
        [true, false, false, true],
        "open and future/undated is current; past or closed is not"
    );
    assert_eq!(items[3].sprint.as_ref().unwrap().ends_at.as_deref(), Some("2999-01-01"));
    assert!(items[5].sprint.is_none() && items[5].estimate.is_none() && items[5].due.is_none());
    // One lookup per distinct version, not per issue.
    assert_eq!(count(&server, "GET", "/versions/1.json").await, 1);
}

#[tokio::test]
async fn a_failed_priority_or_version_lookup_leaves_rank_and_an_inactive_sprint() {
    let server = MockServer::start().await;
    let i = issue(10, json!({"fixed_version": {"id": 1, "name": "v1"}}));
    mount_json(&server, "/issues.json", json!({"issues": [i], "total_count": 1})).await;
    let t = rm(&server, json!({})).list(&view("v"), None).await.unwrap().items.remove(0);
    assert_eq!(t.priority_rank, None);
    let sp = t.sprint.unwrap();
    assert_eq!((sp.name.as_str(), sp.active, sp.ends_at), ("v1", false, None));
}

#[tokio::test]
async fn sprints_resolve_from_one_project_versions_call() {
    let server = MockServer::start().await;
    mount_priorities(&server).await;
    mount_json(
        &server,
        "/projects/5/versions.json",
        json!({"versions": [
            {"id": 1, "name": "v1", "status": "open", "due_date": "2999-01-01"},
            {"id": 2, "name": "v2", "status": "closed", "due_date": null}]}),
    )
    .await;
    let with = |id, ver: u64| {
        issue(id, json!({"project": {"id": 5}, "fixed_version": {"id": ver, "name": format!("v{ver}")}}))
    };
    mount_json(
        &server,
        "/issues.json",
        json!({"issues": [with(10, 1), with(11, 2), with(12, 1)], "total_count": 3}),
    )
    .await;
    let items = rm(&server, json!({})).list(&view("v"), None).await.unwrap().items;
    let active: Vec<bool> = items.iter().map(|i| i.sprint.as_ref().unwrap().active).collect();
    assert_eq!(active, [true, false, true]);
    assert_eq!(count(&server, "GET", "/projects/5/versions.json").await, 1);
    assert_eq!(count(&server, "GET", "/versions/1.json").await, 0);
}

#[tokio::test]
async fn search_filters_the_subject_server_side() {
    let server = MockServer::start().await;
    mount(&server, "GET", "/issue_statuses.json", 200, "redmine/issue_statuses.json").await;
    Mock::given(method("GET"))
        .and(path("/issues.json"))
        .and(query_param("subject", "~login"))
        .and(query_param("assigned_to_id", "me"))
        .respond_with(ResponseTemplate::new(200).set_body_string(fixture_text("redmine/issues_p2.json")))
        .expect(1)
        .mount(&server)
        .await;
    assert!(!rm(&server, json!({})).search(&view("mine"), "login").await.unwrap().is_empty());
}

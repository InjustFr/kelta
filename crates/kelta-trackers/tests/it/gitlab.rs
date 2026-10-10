//! GitLab Issues: encoded project paths, scoped-label moves, close/reopen.

use crate::support::*;
use kelta_proto::error::ErrorCode;
use kelta_proto::settings::TrackerBinding;
use kelta_proto::tracker::{Assignee, Cursor, StatusCategory, Who};
use serde_json::json;
use wiremock::matchers::{header, method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

const ISSUE: &str = "/api/v4/projects/grp%2Fsub%2Fproj/issues/12";

fn gl(server: &MockServer) -> std::sync::Arc<dyn kelta_proto::api::Tracker> {
    tracker("gitlab-acme", "gitlab", &server.uri(), json!({}))
}

fn r() -> kelta_proto::tracker::TicketRef {
    tref("gitlab-acme", "grp/sub/proj#12", "9012")
}

async fn issue_with(server: &MockServer, fixture_name: &str) {
    mount(server, "GET", ISSUE, 200, fixture_name).await;
}

#[tokio::test]
async fn list_assigned_pages_and_maps_scoped_labels() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/v4/issues"))
        .and(query_param("scope", "assigned_to_me"))
        .and(query_param("state", "opened"))
        .and(header("private-token", TOKEN))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("x-next-page", "2")
                .set_body_string(fixture_text("gitlab/issues_p1.json")),
        )
        .mount(&server)
        .await;
    let page = gl(&server).list(&view("mine"), None).await.unwrap();
    assert_eq!(page.items.len(), 2);
    assert_eq!(page.next, Some(Cursor::Page(2)));
    let t = &page.items[0];
    assert_eq!(t.r#ref.key, "grp/sub/proj#12");
    assert_eq!(t.r#ref.id, "9012");
    assert_eq!(t.status.name, "doing");
    assert_eq!(t.status.category, StatusCategory::InProgress);
    assert_eq!(t.priority.as_deref(), Some("high"));
    assert_eq!(t.assignee.as_ref().unwrap().id, "42");
    assert_eq!(t.assignee.as_ref().unwrap().login.as_deref(), Some("louis"));
    assert_eq!(page.items[1].status.category, StatusCategory::Todo);
}

#[tokio::test]
async fn project_views_use_the_url_encoded_path_and_last_page_has_no_cursor() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/v4/projects/grp%2Fsub%2Fproj/issues"))
        .and(query_param("scope", "all"))
        .and(query_param("labels", "bug,workflow::todo"))
        .and(query_param("page", "2"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("x-next-page", "")
                .set_body_string(fixture_text("gitlab/issues_p2.json")),
        )
        .expect(1)
        .mount(&server)
        .await;
    let mut v = view("proj");
    v.project = Some("grp/sub/proj".into());
    v.scope = Some("all".into());
    v.labels = Some(vec!["bug".into(), "workflow::todo".into()]);
    let page = gl(&server).list(&v, Some(Cursor::Page(2))).await.unwrap();
    assert_eq!(page.items.len(), 1);
    assert!(page.next.is_none());
}

#[tokio::test]
async fn a_304_reuses_the_cached_list() {
    let server = MockServer::start().await;
    Mock::given(path("/api/v4/issues"))
        .and(header("if-none-match", "W/\"abc\""))
        .respond_with(ResponseTemplate::new(304))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(path("/api/v4/issues"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("etag", "W/\"abc\"")
                .set_body_string(fixture_text("gitlab/issues_p1.json")),
        )
        .expect(1)
        .mount(&server)
        .await;
    let t = gl(&server);
    let a = t.list(&view("mine"), None).await.unwrap();
    let b = t.list(&view("mine"), None).await.unwrap();
    assert_eq!(a, b);
}

#[tokio::test]
async fn detail_lists_user_notes_oldest_first() {
    let server = MockServer::start().await;
    issue_with(&server, "gitlab/issue.json").await;
    mount(&server, "GET", &format!("{ISSUE}/notes"), 200, "gitlab/notes_desc.json").await;
    Mock::given(method("POST"))
        .and(path("/api/graphql"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"data": {"project": {"workItems": {"nodes": [
            {"widgets": [{}, {"children": {"nodes": [{"iid": "13", "namespace": {"fullPath": "grp/sub/proj"}}]}}]}
        ]}}}})))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/api/v4/projects/grp%2Fsub%2Fproj/issues"))
        .and(query_param("iids[]", "13"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([{
            "iid": 13, "title": "Task", "state": "closed", "web_url": "https://gitlab.example/grp/sub/proj/-/work_items/13",
            "references": {"full": "grp/sub/proj#13"}, "labels": [], "updated_at": "2026-10-01T08:30:00Z"
        }])))
        .mount(&server)
        .await;
    let d = gl(&server).get(&r()).await.unwrap();
    let kids: Vec<_> =
        d.children.iter().map(|c| (c.ticket.r#ref.key.as_str(), c.ticket.status.category)).collect();
    assert_eq!(kids, vec![("grp/sub/proj#13", StatusCategory::Done)]);
    assert!(d.body_html.contains("<li>step one</li>"));
    assert_eq!(d.comments.len(), 2, "system notes are dropped");
    assert!(d.comments[0].body_html.contains("<strong>note</strong>"));
    assert!(d.comments[1].body_html.contains("third"));
}

#[tokio::test]
async fn transitions_come_from_the_projects_scoped_labels() {
    let server = MockServer::start().await;
    issue_with(&server, "gitlab/issue.json").await;
    Mock::given(method("GET"))
        .and(path("/api/v4/projects/grp%2Fsub%2Fproj/labels"))
        .and(query_param("search", "workflow::"))
        .respond_with(ResponseTemplate::new(200).set_body_string(fixture_text("gitlab/labels.json")))
        .mount(&server)
        .await;
    let ts = gl(&server).transitions(&r()).await.unwrap();
    assert_eq!(
        ts.iter().map(|t| t.id.as_str()).collect::<Vec<_>>(),
        vec!["label:workflow::todo", "label:workflow::review", "label:workflow::done"],
        "current label and unrelated labels (workflowish) are not offered; a done label replaces plain close"
    );
    assert_eq!(ts[2].to.category, StatusCategory::Done);
}

#[tokio::test]
async fn without_a_done_label_close_is_offered() {
    let server = MockServer::start().await;
    issue_with(&server, "gitlab/issue.json").await;
    mount(&server, "GET", "/api/v4/projects/grp%2Fsub%2Fproj/labels", 200, "gitlab/labels_no_done.json")
        .await;
    let ts = gl(&server).transitions(&r()).await.unwrap();
    assert_eq!(ts.last().unwrap().id, "close");
    assert_eq!(ts.last().unwrap().to.category, StatusCategory::Done);
}

#[tokio::test]
async fn moving_adds_the_target_label_and_removes_the_other_scoped_labels() {
    let server = MockServer::start().await;
    issue_with(&server, "gitlab/issue.json").await;
    Mock::given(method("PUT"))
        .and(path(ISSUE))
        .respond_with(ResponseTemplate::new(200).set_body_string(fixture_text("gitlab/issue_moved.json")))
        .mount(&server)
        .await;
    let t = gl(&server).transition(&r(), "label:workflow::review", None).await.unwrap();
    assert_eq!(t.status.name, "review");
    let put = &bodies(&server, "PUT", ISSUE).await[0];
    assert_eq!(put["add_labels"], "workflow::review");
    assert_eq!(
        put["remove_labels"], "workflow::doing",
        "only same-scope labels are removed (bug / priority stay)"
    );
    assert!(put.get("state_event").is_none());
}

#[tokio::test]
async fn done_labels_close_and_other_labels_reopen_a_closed_issue() {
    let server = MockServer::start().await;
    issue_with(&server, "gitlab/issue.json").await;
    Mock::given(method("PUT"))
        .and(path(ISSUE))
        .respond_with(ResponseTemplate::new(200).set_body_string(fixture_text("gitlab/issue_closed.json")))
        .mount(&server)
        .await;
    let t = gl(&server).transition(&r(), "label:workflow::done", None).await.unwrap();
    assert_eq!(t.status.category, StatusCategory::Done);
    assert_eq!(bodies(&server, "PUT", ISSUE).await[0]["state_event"], "close");

    // closed issue moved to an open workflow state is reopened
    let server2 = MockServer::start().await;
    mount(&server2, "GET", ISSUE, 200, "gitlab/issue_closed.json").await;
    Mock::given(method("PUT"))
        .and(path(ISSUE))
        .respond_with(ResponseTemplate::new(200).set_body_string(fixture_text("gitlab/issue.json")))
        .mount(&server2)
        .await;
    gl(&server2).transition(&r(), "label:workflow::doing", None).await.unwrap();
    let put = &bodies(&server2, "PUT", ISSUE).await[0];
    assert_eq!(put["state_event"], "reopen");
    assert_eq!(put["remove_labels"], "workflow::done");
}

#[tokio::test]
async fn plain_close_and_reopen() {
    let server = MockServer::start().await;
    Mock::given(method("PUT"))
        .and(path(ISSUE))
        .respond_with(ResponseTemplate::new(200).set_body_string(fixture_text("gitlab/issue_closed.json")))
        .mount(&server)
        .await;
    let t = gl(&server);
    t.transition(&r(), "close", None).await.unwrap();
    t.transition(&r(), "reopen", None).await.unwrap();
    let puts = bodies(&server, "PUT", ISSUE).await;
    assert_eq!(puts[0], json!({"state_event": "close"}));
    assert_eq!(puts[1], json!({"state_event": "reopen"}));
    assert_eq!(t.transition(&r(), "nope", None).await.unwrap_err().code, ErrorCode::InvalidArgument);
}

#[tokio::test]
async fn closed_issues_without_labels_offer_reopen() {
    let server = MockServer::start().await;
    let mut closed = fixture("gitlab/issue_closed.json");
    closed["labels"] = json!([]);
    Mock::given(path(ISSUE))
        .respond_with(ResponseTemplate::new(200).set_body_json(closed))
        .mount(&server)
        .await;
    Mock::given(path("/api/v4/projects/grp%2Fsub%2Fproj/labels"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([])))
        .mount(&server)
        .await;
    let ts = gl(&server).transitions(&r()).await.unwrap();
    assert_eq!(ts.iter().map(|t| t.id.as_str()).collect::<Vec<_>>(), vec!["reopen"]);
    assert_eq!(ts[0].to.category, StatusCategory::Todo);
}

#[tokio::test]
async fn assign_and_comment() {
    let server = MockServer::start().await;
    mount(&server, "GET", "/api/v4/user", 200, "gitlab/user.json").await;
    Mock::given(method("PUT"))
        .and(path(ISSUE))
        .respond_with(ResponseTemplate::new(200).set_body_string(fixture_text("gitlab/issue_assigned.json")))
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path(format!("{ISSUE}/notes")))
        .respond_with(ResponseTemplate::new(201).set_body_string("{}"))
        .mount(&server)
        .await;
    let t = gl(&server);
    t.assign(&r(), Assignee::Me).await.unwrap();
    t.assign(&r(), Assignee::None).await.unwrap();
    let puts = bodies(&server, "PUT", ISSUE).await;
    assert_eq!(puts[0], json!({"assignee_ids": [42]}));
    assert_eq!(puts[1], json!({"assignee_ids": []}));
    t.comment(&r(), "Taking this").await.unwrap();
    assert_eq!(bodies(&server, "POST", &format!("{ISSUE}/notes")).await[0], json!({"body": "Taking this"}));
}

#[tokio::test]
async fn columns_from_scoped_labels_or_the_binding() {
    let server = MockServer::start().await;
    mount(&server, "GET", "/api/v4/projects/grp%2Fsub%2Fproj/labels", 200, "gitlab/labels_no_done.json")
        .await;
    let mut v = view("proj");
    v.project = Some("grp/sub/proj".into());
    let cols =
        gl(&server).columns(&TrackerBinding { views: vec![v], ..TrackerBinding::default() }).await.unwrap();
    assert_eq!(
        cols.iter().map(|c| c.name.as_str()).collect::<Vec<_>>(),
        vec!["todo", "doing", "review", "Closed"]
    );
    assert_eq!(cols.last().unwrap().category, StatusCategory::Done);
    assert_eq!(cols.iter().map(|c| c.order).collect::<Vec<_>>(), vec![0, 1, 2, 3]);
    // no project: open / closed
    let plain = gl(&server).columns(&TrackerBinding::default()).await.unwrap();
    assert_eq!(plain.iter().map(|c| c.id.as_str()).collect::<Vec<_>>(), vec!["opened", "closed"]);
}

#[tokio::test]
async fn custom_workflow_scope_from_the_view() {
    let server = MockServer::start().await;
    let mut issue = fixture("gitlab/issue.json");
    issue["labels"] = json!(["stage::build"]);
    Mock::given(method("GET"))
        .and(path("/api/v4/issues"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([issue])))
        .mount(&server)
        .await;
    let mut v = view("mine");
    v.workflow_scope = Some("stage".into());
    let page = gl(&server).list(&v, None).await.unwrap();
    assert_eq!(page.items[0].status.name, "build");
}

#[tokio::test]
async fn unauthorized_and_keys() {
    let server = MockServer::start().await;
    Mock::given(path("/api/v4/user")).respond_with(ResponseTemplate::new(401)).mount(&server).await;
    let t = gl(&server);
    assert_eq!(t.me().await.unwrap_err().code, ErrorCode::NeedsAuth);
    assert_eq!(t.branch_key(&r()), "gl-12");
    assert_eq!(t.browser_url(&r()), format!("{}/grp/sub/proj/-/issues/12", server.uri()));
    // a base_url that already ends in /api/v4 works too
    let t2 = tracker("gitlab-acme", "gitlab", &format!("{}/api/v4", server.uri()), json!({}));
    assert_eq!(t2.me().await.unwrap_err().code, ErrorCode::NeedsAuth);
}

async fn issue_queries(server: &MockServer) -> Vec<String> {
    server
        .received_requests()
        .await
        .unwrap_or_default()
        .into_iter()
        .filter(|r| r.url.path().ends_with("/issues"))
        .map(|r| r.url.query().unwrap_or("").to_owned())
        .collect()
}

#[tokio::test]
async fn who_picks_the_scope_and_the_unassigned_filter_and_overrides_legacy_scope() {
    let server = MockServer::start().await;
    mount(&server, "GET", "/api/v4/issues", 200, "gitlab/issues_p1.json").await;
    let t = gl(&server);
    for (who, legacy) in
        [(Who::Mine, "all"), (Who::Unassigned, "assigned_to_me"), (Who::Anyone, "assigned_to_me")]
    {
        let mut v = view("v");
        v.who = Some(who);
        v.scope = Some(legacy.into());
        t.list(&v, None).await.unwrap();
    }
    let q = issue_queries(&server).await;
    assert!(q[0].contains("scope=assigned_to_me") && !q[0].contains("assignee_id"), "{}", q[0]);
    assert!(q[1].contains("scope=all") && q[1].contains("assignee_id=None"), "{}", q[1]);
    assert!(q[2].contains("scope=all") && !q[2].contains("assignee_id"), "{}", q[2]);
}

#[tokio::test]
async fn current_iteration_filters_on_the_current_milestone() {
    let server = MockServer::start().await;
    let ms = r#"[{"title":"v1.0","due_date":"2000-01-01"},{"title":"Later","due_date":"2999-12-31"},
        {"title":"Sprint 42","due_date":"2999-01-01"},{"title":"Backlog","due_date":null}]"#;
    Mock::given(path("/api/v4/projects/grp%2Fsub%2Fproj/milestones"))
        .and(query_param("state", "active"))
        .respond_with(ResponseTemplate::new(200).set_body_string(ms))
        .mount(&server)
        .await;
    Mock::given(path("/api/v4/projects/grp%2Fsub%2Fproj/issues"))
        .and(query_param("scope", "all"))
        .and(query_param("milestone", "Sprint 42"))
        .respond_with(ResponseTemplate::new(200).set_body_string(fixture_text("gitlab/issues_p2.json")))
        .expect(1)
        .mount(&server)
        .await;
    let mut v = view("proj");
    v.project = Some("grp/sub/proj".into());
    v.who = Some(Who::Anyone);
    v.current_iteration = true;
    gl(&server).list(&v, None).await.unwrap();
    // and without the flag no milestone filter is sent
    v.current_iteration = false;
    Mock::given(path("/api/v4/projects/grp%2Fsub%2Fproj/issues"))
        .respond_with(ResponseTemplate::new(200).set_body_string("[]"))
        .mount(&server)
        .await;
    gl(&server).list(&v, None).await.unwrap();
    assert!(!issue_queries(&server).await[1].contains("milestone_id"));
}

#[tokio::test]
async fn sources_lists_member_projects_as_mine_views() {
    let server = MockServer::start().await;
    Mock::given(path("/api/v4/projects"))
        .and(query_param("membership", "true"))
        .and(query_param("search", "pro"))
        .respond_with(ResponseTemplate::new(200).set_body_string(fixture_text("gitlab/projects.json")))
        .expect(1)
        .mount(&server)
        .await;
    let hits = gl(&server).sources("pro").await.unwrap();
    assert_eq!(hits.len(), 2);
    let h = &hits[0];
    assert_eq!(
        (h.kind.as_str(), h.label.as_str(), h.detail.as_deref()),
        ("project", "grp/sub/proj", Some("The shop"))
    );
    assert_eq!(h.view.id, "project-grp/sub/proj");
    assert_eq!(h.view.project.as_deref(), Some("grp/sub/proj"));
    assert_eq!(h.view.who, Some(Who::Mine));
    assert_ne!(hits[0].view.id, hits[1].view.id);
    assert!(hits[1].detail.is_none(), "empty description is no detail");
}

#[tokio::test]
async fn planning_fields_come_from_iteration_milestone_labels_and_time_stats() {
    let server = MockServer::start().await;
    mount(&server, "GET", "/api/v4/issues", 200, "gitlab/issues_planning.json").await;
    let items = gl(&server).list(&view("mine"), None).await.unwrap().items;
    let [a, b, c, d, e] = &items[..] else { panic!("five issues") };
    // priority_rank: bare P1, scoped name, scoped number, scoped name (a bare `high` label is not a priority), none
    assert_eq!([a, b, c, d, e].map(|t| t.priority_rank), [Some(1), Some(3), Some(0), Some(0), None]);
    // sprint: iteration wins over milestone; open milestone inside its dates is active; expired and closed are not
    let sp = a.sprint.as_ref().unwrap();
    assert_eq!((sp.id.as_str(), sp.name.as_str(), sp.active), ("5", "Sprint 7", true));
    assert_eq!(sp.ends_at.as_deref(), Some("2026-10-19"));
    let sp = b.sprint.as_ref().unwrap();
    assert_eq!((sp.name.as_str(), sp.active, sp.ends_at.as_deref()), ("v1", true, Some("2999-12-31")));
    assert!(!c.sprint.as_ref().unwrap().active, "expired milestone");
    let sp = d.sprint.as_ref().unwrap();
    assert_eq!(
        (sp.name.as_str(), sp.active),
        ("2026-09-01 – 2026-09-14", false),
        "closed untitled iteration named by dates"
    );
    assert!(!e.sprint.as_ref().unwrap().active, "undated open milestone is not the current sprint");
    // estimate: time estimate wins over weight, weight is the fallback
    assert_eq!(
        [a, b, c, d, e].map(|t| t.estimate.as_deref()),
        [Some("1h 30m"), Some("5"), Some("2h"), None, None]
    );
    assert_eq!([a, b, c, d, e].map(|t| t.due.as_deref()), [Some("2026-10-14"), None, None, None, None]);
    // status_since: closing date for closed issues, else updated_at
    assert_eq!(c.status_since.as_deref(), Some("2026-10-02T10:00:00.000Z"));
    assert_eq!(a.status_since.as_deref(), Some(a.updated_at.as_str()));
}

#[tokio::test]
async fn search_sends_the_text_as_search() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/v4/issues"))
        .and(query_param("search", "login"))
        .and(query_param("scope", "assigned_to_me"))
        .respond_with(ResponseTemplate::new(200).set_body_string(fixture_text("gitlab/issues_p1.json")))
        .expect(1)
        .mount(&server)
        .await;
    assert_eq!(gl(&server).search(&view("mine"), "login").await.unwrap().len(), 2);
}

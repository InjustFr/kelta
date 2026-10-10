//! One conformance harness, run against every tracker provider (Jira Cloud, Jira DC, Redmine,
//! GitHub Issues, GitLab Issues, Gitea Issues, Linear). Each provider only supplies its mocks and expectations.

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod support;

use std::sync::Arc;

use kelta_proto::api::Tracker;
use kelta_proto::error::ErrorCode;
use kelta_proto::settings::{TrackerBinding, TrackerView};
use kelta_proto::testing::conformance::{TrackerCase, tracker_contract};
use kelta_proto::tracker::{Assignee, TicketRef, TrackerKind};
use serde_json::json;
use support::*;
use wiremock::matchers::{any, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn err_code<T: std::fmt::Debug>(r: Result<T, kelta_proto::error::KeltaError>) -> ErrorCode {
    r.unwrap_err().code
}

struct Case {
    name: &'static str,
    kind: TrackerKind,
    account_id: &'static str,
    ticket: TicketRef,
    view: TrackerView,
    binding: TrackerBinding,
    /// Pattern of the branch key (`SHOP-142`, `4567`, `gh-12`, `gl-12`).
    branch_key: &'static str,
    build: fn(&str) -> Arc<dyn Tracker>,
    mock_ok: for<'a> fn(&'a MockServer) -> std::pin::Pin<Box<dyn std::future::Future<Output = ()> + 'a>>,
}

macro_rules! mocks {
    ($name:ident, |$s:ident| $body:block) => {
        fn $name<'a>($s: &'a MockServer) -> std::pin::Pin<Box<dyn std::future::Future<Output = ()> + 'a>> {
            Box::pin(async move $body)
        }
    };
}

fn jql_view() -> TrackerView {
    let mut v = view("mine");
    v.jql = Some("project = SHOP".into());
    v
}

fn binding(v: TrackerView) -> TrackerBinding {
    TrackerBinding { views: vec![v], ..TrackerBinding::default() }
}

async fn ok_status(server: &MockServer, m: &str, p: &str, status: u16) {
    Mock::given(method(m))
        .and(path(p))
        .respond_with(ResponseTemplate::new(status).set_body_string("{}"))
        .mount(server)
        .await;
}

mocks!(jira_cloud_mocks, |s| {
    mount(s, "GET", "/rest/api/3/myself", 200, "jira/myself_cloud.json").await;
    mount(s, "POST", "/rest/api/3/search/jql", 200, "jira/search_jql_p2.json").await;
    mount(s, "GET", "/rest/api/3/issue/SHOP-142", 200, "jira/issue_cloud.json").await;
    mount(s, "GET", "/rest/api/3/issue/SHOP-142/transitions", 200, "jira/transitions.json").await;
    ok_status(s, "POST", "/rest/api/3/issue/SHOP-142/transitions", 204).await;
    ok_status(s, "POST", "/rest/api/3/issue/SHOP-142/comment", 201).await;
    ok_status(s, "PUT", "/rest/api/3/issue/SHOP-142/assignee", 204).await;
    mount(s, "GET", "/rest/api/3/project/SHOP/statuses", 200, "jira/project_statuses.json").await;
});

mocks!(jira_dc_mocks, |s| {
    mount(s, "GET", "/rest/api/2/myself", 200, "jira/myself_dc.json").await;
    mount(s, "POST", "/rest/api/2/search", 200, "jira/search_dc_p2.json").await;
    mount(s, "GET", "/rest/api/2/issue/SHOP-142", 200, "jira/issue_dc.json").await;
    mount(s, "GET", "/rest/api/2/issue/SHOP-142/transitions", 200, "jira/transitions.json").await;
    ok_status(s, "POST", "/rest/api/2/issue/SHOP-142/transitions", 204).await;
    ok_status(s, "POST", "/rest/api/2/issue/SHOP-142/comment", 201).await;
    ok_status(s, "PUT", "/rest/api/2/issue/SHOP-142/assignee", 204).await;
    mount(s, "GET", "/rest/api/2/project/SHOP/statuses", 200, "jira/project_statuses.json").await;
});

mocks!(redmine_mocks, |s| {
    mount(s, "GET", "/users/current.json", 200, "redmine/current_user.json").await;
    mount(s, "GET", "/issue_statuses.json", 200, "redmine/issue_statuses.json").await;
    mount(s, "GET", "/issues.json", 200, "redmine/issues_p2.json").await;
    mount(s, "GET", "/issues/4567.json", 200, "redmine/issue.json").await;
    ok_status(s, "PUT", "/issues/4567.json", 204).await;
});

mocks!(github_mocks, |s| {
    mount(s, "GET", "/user", 200, "github/user.json").await;
    mount(s, "GET", "/issues", 200, "github/issues_assigned_p1.json").await;
    mount(s, "GET", "/repos/acme/shop/issues/12", 200, "github/issue.json").await;
    mount(s, "GET", "/repos/acme/shop/issues/12/comments", 200, "github/comments_last.json").await;
    ok_status(s, "POST", "/repos/acme/shop/issues/12/comments", 201).await;
    mount(s, "PATCH", "/repos/acme/shop/issues/12", 200, "github/issue_closed.json").await;
    mount(s, "POST", "/graphql", 200, "github/gql_issue_no_projects.json").await;
});

mocks!(gitlab_mocks, |s| {
    mount(s, "GET", "/api/v4/user", 200, "gitlab/user.json").await;
    mount(s, "GET", "/api/v4/issues", 200, "gitlab/issues_p1.json").await;
    mount(s, "GET", "/api/v4/projects/grp%2Fsub%2Fproj/issues", 200, "gitlab/issues_p1.json").await;
    mount(s, "GET", "/api/v4/projects/grp%2Fsub%2Fproj/issues/12", 200, "gitlab/issue.json").await;
    mount(s, "GET", "/api/v4/projects/grp%2Fsub%2Fproj/issues/12/notes", 200, "gitlab/notes_desc.json").await;
    mount(s, "GET", "/api/v4/projects/grp%2Fsub%2Fproj/labels", 200, "gitlab/labels.json").await;
    mount(s, "PUT", "/api/v4/projects/grp%2Fsub%2Fproj/issues/12", 200, "gitlab/issue_moved.json").await;
    ok_status(s, "POST", "/api/v4/projects/grp%2Fsub%2Fproj/issues/12/notes", 201).await;
});

mocks!(gitea_mocks, |s| {
    mount(s, "GET", "/api/v1/user", 200, "gitea/user.json").await;
    mount(s, "GET", "/api/v1/repos/issues/search", 200, "gitea/issues_p1.json").await;
    mount(s, "GET", "/api/v1/repos/acme/shop/issues/12", 200, "gitea/issue.json").await;
    mount(s, "GET", "/api/v1/repos/acme/shop/issues/12/comments", 200, "gitea/comments.json").await;
    ok_status(s, "POST", "/api/v1/repos/acme/shop/issues/12/comments", 201).await;
    mount(s, "PATCH", "/api/v1/repos/acme/shop/issues/12", 200, "gitea/issue_closed.json").await;
});

mocks!(linear_mocks_ok, |s| {
    linear_mocks(s).await;
});

fn cases() -> Vec<Case> {
    let mut gl_view = view("mine");
    gl_view.project = Some("grp/sub/proj".into());
    vec![
        Case {
            name: "jira cloud",
            kind: TrackerKind::Jira,
            account_id: "jira-acme",
            ticket: tref("jira-acme", "SHOP-142", "10142"),
            view: jql_view(),
            binding: binding(jql_view()),
            branch_key: "SHOP-142",
            build: |base| {
                tracker("jira-acme", "jira", base, json!({"flavor": "cloud", "email": "louis@acme.test"}))
            },
            mock_ok: jira_cloud_mocks,
        },
        Case {
            name: "jira dc",
            kind: TrackerKind::Jira,
            account_id: "jira-dc",
            ticket: tref("jira-dc", "SHOP-142", "10142"),
            view: jql_view(),
            binding: binding(jql_view()),
            branch_key: "SHOP-142",
            build: |base| tracker("jira-dc", "jira", base, json!({"flavor": "dc"})),
            mock_ok: jira_dc_mocks,
        },
        Case {
            name: "redmine",
            kind: TrackerKind::Redmine,
            account_id: "redmine-client",
            ticket: tref("redmine-client", "4567", "4567"),
            view: view("mine"),
            binding: TrackerBinding::default(),
            branch_key: "4567",
            build: |base| tracker("redmine-client", "redmine", base, json!({})),
            mock_ok: redmine_mocks,
        },
        Case {
            name: "github issues",
            kind: TrackerKind::GithubIssues,
            account_id: "github-work",
            ticket: tref("github-work", "acme/shop#12", "1012"),
            view: view("mine"),
            binding: TrackerBinding::default(),
            branch_key: "gh-12",
            build: |base| tracker("github-work", "github", base, json!({})),
            mock_ok: github_mocks,
        },
        Case {
            name: "gitlab issues",
            kind: TrackerKind::GitlabIssues,
            account_id: "gitlab-acme",
            ticket: tref("gitlab-acme", "grp/sub/proj#12", "9012"),
            view: gl_view.clone(),
            binding: binding(gl_view),
            branch_key: "gl-12",
            build: |base| tracker("gitlab-acme", "gitlab", base, json!({})),
            mock_ok: gitlab_mocks,
        },
        Case {
            name: "gitea issues",
            kind: TrackerKind::GiteaIssues,
            account_id: "gitea-main",
            ticket: tref("gitea-main", "acme/shop#12", "1012"),
            view: view("mine"),
            binding: TrackerBinding::default(),
            branch_key: "gt-12",
            build: |base| tracker("gitea-main", "gitea", base, json!({})),
            mock_ok: gitea_mocks,
        },
        Case {
            name: "linear",
            kind: TrackerKind::Linear,
            account_id: "linear-acme",
            ticket: tref("linear-acme", "ENG-12", "iss-uuid-12"),
            view: view("mine"),
            binding: binding(view("mine")),
            branch_key: "eng-12",
            build: |base| tracker("linear-acme", "linear", base, json!({})),
            mock_ok: linear_mocks_ok,
        },
    ]
}

#[tokio::test]
async fn every_tracker_honours_the_contract() {
    for case in cases() {
        let server = MockServer::start().await;
        (case.mock_ok)(&server).await;
        let t = (case.build)(&server.uri());
        let contract = TrackerCase {
            kind: case.kind,
            account_id: case.account_id.into(),
            view: case.view.clone(),
            binding: case.binding.clone(),
            ticket: Some(case.ticket.clone()),
            branch_key: Some(case.branch_key.into()),
        };
        let n = case.name;
        tracker_contract(&*t, &contract).await.unwrap_or_else(|e| panic!("{n}: {e}"));
        let caps = t.caps();
        assert!(caps.comment && caps.assign, "{n}: every v0.1 provider supports comment and assign");
    }
}

#[tokio::test]
async fn every_tracker_maps_401_to_needs_auth_and_404_to_not_found() {
    for case in cases() {
        let n = case.name;
        let denied = MockServer::start().await;
        Mock::given(any())
            .respond_with(ResponseTemplate::new(401).set_body_string("{}"))
            .mount(&denied)
            .await;
        let t = (case.build)(&denied.uri());
        assert_eq!(err_code(t.me().await), ErrorCode::NeedsAuth, "{n}: me");
        assert_eq!(err_code(t.list(&case.view, None).await), ErrorCode::NeedsAuth, "{n}: list");
        assert_eq!(err_code(t.get(&case.ticket).await), ErrorCode::NeedsAuth, "{n}: get");
        assert_eq!(err_code(t.transitions(&case.ticket).await), ErrorCode::NeedsAuth, "{n}: transitions");
        assert_eq!(err_code(t.comment(&case.ticket, "x").await), ErrorCode::NeedsAuth, "{n}: comment");
        assert_eq!(
            err_code(t.assign(&case.ticket, Assignee::User { id: "1".into() }).await),
            ErrorCode::NeedsAuth,
            "{n}: assign"
        );

        let missing = MockServer::start().await;
        Mock::given(any())
            .respond_with(ResponseTemplate::new(404).set_body_string("{}"))
            .mount(&missing)
            .await;
        let t = (case.build)(&missing.uri());
        assert_eq!(err_code(t.get(&case.ticket).await), ErrorCode::NotFound, "{n}: get 404");
    }
}

#[tokio::test]
async fn every_tracker_survives_garbage_payloads() {
    // Truncated / foreign JSON must surface as an error or an empty result, never a panic.
    for case in cases() {
        let server = MockServer::start().await;
        Mock::given(any())
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_string("{\"unexpected\": [1, 2, 3], \"issues\": \"nope\"}"),
            )
            .mount(&server)
            .await;
        let t = (case.build)(&server.uri());
        let _ = t.me().await;
        let _ = t.list(&case.view, None).await;
        let _ = t.get(&case.ticket).await;
        let _ = t.transitions(&case.ticket).await;
        let _ = t.columns(&case.binding).await;
    }
}

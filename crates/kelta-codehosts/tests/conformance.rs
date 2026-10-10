//! One conformance harness, run against every code-host provider (GitHub, GitLab, Gitea, Bitbucket).

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod support;

use std::sync::Arc;

use kelta_proto::api::CodeHost;
use kelta_proto::codehost::{CodeHostKind, ReviewKind};
use kelta_proto::error::{ErrorCode, KeltaError};
use kelta_proto::testing::conformance::{CodeHostCase, code_host_contract, pr_create};
use support::*;
use wiremock::matchers::{any, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

type Mocks = for<'a> fn(&'a MockServer) -> std::pin::Pin<Box<dyn std::future::Future<Output = ()> + 'a>>;

struct Case {
    name: &'static str,
    kind: CodeHostKind,
    account_id: &'static str,
    kind_str: &'static str,
    /// Repo used for create / find_for_branch.
    repo: &'static str,
    /// Start of the fetch refspec of review `#{n}` (the rest is `:<local branch>`).
    refspec: &'static str,
    /// The provider has a cheap change gate (`changed_since_last`); without one it always says yes.
    gate: bool,
    mock_ok: Mocks,
}

macro_rules! mocks {
    ($name:ident, |$s:ident| $body:block) => {
        fn $name<'a>($s: &'a MockServer) -> std::pin::Pin<Box<dyn std::future::Future<Output = ()> + 'a>> {
            Box::pin(async move $body)
        }
    };
}

async fn ok(server: &MockServer, m: &str, p: &str, status: u16) {
    Mock::given(method(m))
        .and(path(p))
        .respond_with(ResponseTemplate::new(status).set_body_string("{}"))
        .mount(server)
        .await;
}

mocks!(github_mocks, |s| {
    mount(s, "GET", "/user", 200, "github/user.json").await;
    mount(s, "GET", "/notifications", 200, "github/notifications.json").await;
    mount(s, "POST", "/graphql", 200, "github/gql_list.json").await;
    mount(s, "GET", "/repos/acme/shop/pulls/101", 200, "github/pull.json").await;
    mount(s, "GET", "/repos/acme/shop/pulls/101/reviews", 200, "github/reviews.json").await;
    mount(s, "GET", "/repos/acme/shop/pulls/101/files", 200, "github/files.json").await;
    mount(s, "GET", "/repos/acme/shop/commits/abc123/check-runs", 200, "github/check_runs.json").await;
    mount(s, "GET", "/repos/acme/shop/commits/abc123/status", 200, "github/combined_status.json").await;
    mount(s, "POST", "/repos/acme/shop/pulls/101/reviews", 200, "github/review_posted.json").await;
    ok(s, "POST", "/repos/acme/shop/issues/101/comments", 201).await;
    mount(s, "POST", "/repos/acme/shop/pulls", 201, "github/create_pull.json").await;
    mount(s, "GET", "/repos/acme/shop/pulls", 200, "github/pulls_for_branch.json").await;
});

mocks!(gitlab_mocks, |s| {
    mount(s, "GET", "/api/v4/user", 200, "gitlab/user.json").await;
    mount(s, "GET", "/api/v4/version", 200, "gitlab/version_16.json").await;
    mount(s, "GET", "/api/v4/todos", 200, "gitlab/todos.json").await;
    mount(s, "GET", "/api/v4/merge_requests", 200, "gitlab/mrs_review.json").await;
    let mr = "/api/v4/projects/grp%2Fother/merge_requests/8";
    mount(s, "GET", mr, 200, "gitlab/mr.json").await;
    mount(s, "GET", &format!("{mr}/approvals"), 200, "gitlab/approvals.json").await;
    mount(s, "GET", &format!("{mr}/changes"), 200, "gitlab/changes.json").await;
    mount(s, "GET", &format!("{mr}/draft_notes"), 200, "gitlab/no_drafts.json").await;
    mount(s, "GET", "/api/v4/projects/grp%2Fother/pipelines/908/jobs", 200, "gitlab/jobs.json").await;
    mount(s, "POST", &format!("{mr}/approve"), 201, "gitlab/approve_ok.json").await;
    ok(s, "POST", &format!("{mr}/notes"), 201).await;
    ok(s, "POST", &format!("{mr}/unapprove"), 201).await;
    mount(s, "POST", "/api/v4/projects/grp%2Fsub%2Fproj/merge_requests", 201, "gitlab/create_mr.json").await;
    mount(s, "GET", "/api/v4/projects/grp%2Fsub%2Fproj/merge_requests", 200, "gitlab/mrs_for_branch.json")
        .await;
});

mocks!(gitea_mocks, |s| {
    mount(s, "GET", "/api/v1/user", 200, "gitea/user.json").await;
    mount(s, "GET", "/api/v1/repos/issues/search", 200, "gitea/search_pulls.json").await;
    mount(s, "GET", "/api/v1/repos/acme/shop/pulls/7", 200, "gitea/pull_7.json").await;
    mount(s, "GET", "/api/v1/repos/acme/web/pulls/9", 200, "gitea/pull_9.json").await;
    mount(s, "GET", "/api/v1/repos/acme/shop/pulls/7/reviews", 200, "gitea/reviews.json").await;
    mount(s, "GET", "/api/v1/repos/acme/shop/pulls/7/files", 200, "gitea/files.json").await;
    mount(
        s,
        "GET",
        "/api/v1/repos/acme/shop/commits/abc1230000000000000000000000000000000000/status",
        200,
        "gitea/status.json",
    )
    .await;
    mount(s, "POST", "/api/v1/repos/acme/shop/pulls/7/reviews", 200, "gitea/reviews_approved_by_me.json")
        .await;
    ok(s, "POST", "/api/v1/repos/acme/shop/issues/7/comments", 201).await;
    mount(s, "POST", "/api/v1/repos/acme/shop/pulls", 201, "gitea/pull_created.json").await;
    mount(s, "GET", "/api/v1/repos/acme/shop/pulls", 200, "gitea/pulls_open.json").await;
});

mocks!(bitbucket_mocks, |s| {
    mount(s, "GET", "/user", 200, "bitbucket/user.json").await;
    mount(s, "GET", "/user/workspaces", 200, "bitbucket/workspaces.json").await;
    mount(
        s,
        "GET",
        "/workspaces/acme/pullrequests/%7B470c176d-3574-44ea-bb41-89e8638bcca4%7D",
        200,
        "bitbucket/prs.json",
    )
    .await;
    mount(s, "GET", "/repositories/acme", 200, "bitbucket/repos.json").await;
    let pr = "/repositories/acme/shop/pullrequests";
    mount(s, "GET", pr, 200, "bitbucket/prs.json").await;
    mount(s, "GET", &format!("{pr}/7"), 200, "bitbucket/pr_7.json").await;
    mount(s, "GET", &format!("{pr}/7/statuses"), 200, "bitbucket/statuses.json").await;
    mount(s, "GET", &format!("{pr}/7/diffstat"), 200, "bitbucket/diffstat.json").await;
    ok(s, "POST", &format!("{pr}/7/approve"), 200).await;
    ok(s, "POST", &format!("{pr}/7/comments"), 201).await;
    ok(s, "POST", &format!("{pr}/7/request-changes"), 200).await;
    mount(s, "POST", pr, 201, "bitbucket/pr_7.json").await;
});

fn cases() -> Vec<Case> {
    vec![
        Case {
            name: "github",
            kind: CodeHostKind::Github,
            account_id: "github-work",
            kind_str: "github",
            repo: "acme/shop",
            refspec: "pull/{n}/head:",
            gate: true,
            mock_ok: github_mocks,
        },
        Case {
            name: "gitlab",
            kind: CodeHostKind::Gitlab,
            account_id: "gitlab-acme",
            kind_str: "gitlab",
            repo: "grp/sub/proj",
            refspec: "merge-requests/{n}/head:",
            gate: true,
            mock_ok: gitlab_mocks,
        },
        Case {
            name: "gitea",
            kind: CodeHostKind::Gitea,
            account_id: "gitea-main",
            kind_str: "gitea",
            repo: "acme/shop",
            refspec: "pull/{n}/head:",
            gate: false,
            mock_ok: gitea_mocks,
        },
        Case {
            name: "bitbucket",
            kind: CodeHostKind::Bitbucket,
            account_id: "bitbucket-acme",
            kind_str: "bitbucket",
            repo: "acme/shop",
            // no PR refs on Bitbucket Cloud: the source branch of the listed review is fetched
            refspec: "feature/SHOP-142-limiter:",
            gate: false,
            mock_ok: bitbucket_mocks,
        },
    ]
}

/// The account base URL: the mock server, or for the remote-parsing assertions a fixed host.
fn build(case: &Case, base: &str) -> Arc<dyn CodeHost> {
    host(case.account_id, case.kind_str, base)
}

fn err_code<T: std::fmt::Debug>(r: Result<T, KeltaError>) -> ErrorCode {
    r.unwrap_err().code
}

#[tokio::test]
async fn every_code_host_honours_the_contract() {
    for case in cases() {
        let server = MockServer::start().await;
        (case.mock_ok)(&server).await;
        let h = build(&case, &server.uri());
        let contract = CodeHostCase {
            kind: case.kind,
            account_id: case.account_id.into(),
            repo: Some(case.repo.into()),
            refspec: Some(case.refspec.into()),
        };
        code_host_contract(&*h, &contract).await.unwrap_or_else(|e| panic!("{}: {e}", case.name));
    }
}

#[tokio::test]
async fn every_code_host_maps_401_to_needs_auth() {
    for case in cases() {
        let n = case.name;
        let server = MockServer::start().await;
        Mock::given(any())
            .respond_with(ResponseTemplate::new(401).set_body_string("{}"))
            .mount(&server)
            .await;
        let h = build(&case, &server.uri());
        let r = rref(case.account_id, case.repo, 1);
        assert_eq!(err_code(h.me().await), ErrorCode::NeedsAuth, "{n}: me");
        assert_eq!(
            err_code(h.list_reviews(&query(ReviewKind::Authored, true, false)).await),
            ErrorCode::NeedsAuth,
            "{n}: list"
        );
        assert_eq!(err_code(h.get(&r).await), ErrorCode::NeedsAuth, "{n}: get");
        assert_eq!(err_code(h.approve(&r, "sha").await), ErrorCode::NeedsAuth, "{n}: approve");
        assert_eq!(err_code(h.comment(&r, "x").await), ErrorCode::NeedsAuth, "{n}: comment");
        assert_eq!(err_code(h.request_changes(&r, "x").await), ErrorCode::NeedsAuth, "{n}: request_changes");
        assert_eq!(err_code(h.create(&pr_create(case.repo)).await), ErrorCode::NeedsAuth, "{n}: create");
        assert_eq!(err_code(h.find_for_branch(case.repo, "b").await), ErrorCode::NeedsAuth, "{n}: find");
        if case.gate {
            assert_eq!(err_code(h.changed_since_last().await), ErrorCode::NeedsAuth, "{n}: gate");
        } else {
            assert!(h.changed_since_last().await.unwrap(), "{n}: no gate means always changed");
        }
    }
}

#[tokio::test]
async fn every_code_host_survives_garbage_payloads() {
    for case in cases() {
        let server = MockServer::start().await;
        Mock::given(any())
            .respond_with(
                ResponseTemplate::new(200).set_body_string("{\"unexpected\": [1, 2, 3], \"data\": {}}"),
            )
            .mount(&server)
            .await;
        let h = build(&case, &server.uri());
        let r = rref(case.account_id, case.repo, 1);
        let _ = h.me().await;
        let _ = h.list_reviews(&query(ReviewKind::ReviewRequested, true, true)).await;
        let _ = h.get(&r).await;
        let _ = h.find_for_branch(case.repo, "b").await;
        let _ = h.changed_since_last().await;
    }
}

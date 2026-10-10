//! One conformance harness, run against every code-host provider (GitHub, GitLab).

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod support;

use std::collections::HashSet;
use std::sync::Arc;

use kelta_proto::api::CodeHost;
use kelta_proto::codehost::{CodeHostKind, PrCreate, ReviewKind};
use kelta_proto::error::{ErrorCode, KeltaError};
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

fn cases() -> Vec<Case> {
    vec![
        Case {
            name: "github",
            kind: CodeHostKind::Github,
            account_id: "github-work",
            kind_str: "github",
            repo: "acme/shop",
            mock_ok: github_mocks,
        },
        Case {
            name: "gitlab",
            kind: CodeHostKind::Gitlab,
            account_id: "gitlab-acme",
            kind_str: "gitlab",
            repo: "grp/sub/proj",
            mock_ok: gitlab_mocks,
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

fn create_req(repo: &str) -> PrCreate {
    PrCreate {
        repo: repo.into(),
        head: "feature/x".into(),
        base: "main".into(),
        title: "SHOP-1 x".into(),
        body: "b".into(),
        draft: false,
    }
}

#[tokio::test]
async fn every_code_host_honours_the_contract() {
    for case in cases() {
        let n = case.name;
        let server = MockServer::start().await;
        (case.mock_ok)(&server).await;
        let h = build(&case, &server.uri());
        assert_eq!(h.kind(), case.kind, "{n}: kind");

        let me = h.me().await.unwrap_or_else(|e| panic!("{n}: me: {e}"));
        assert!(!me.id.is_empty() && me.login.is_some(), "{n}: me");
        h.changed_since_last().await.unwrap_or_else(|e| panic!("{n}: gate: {e}"));

        let mut first = None;
        for kind in [ReviewKind::ReviewRequested, ReviewKind::Authored] {
            let list = h
                .list_reviews(&query(kind, true, false))
                .await
                .unwrap_or_else(|e| panic!("{n}: list {kind:?}: {e}"));
            assert!(!list.is_empty(), "{n}: {kind:?} list is empty");
            let mut seen = HashSet::new();
            for r in &list {
                assert_eq!(r.kind, kind, "{n}: kind of {}", r.r#ref.number);
                assert_eq!(r.r#ref.account.as_str(), case.account_id, "{n}: account");
                assert!(r.r#ref.number > 0 && !r.r#ref.repo.is_empty(), "{n}: ref");
                assert!(!r.title.is_empty() && !r.url.is_empty() && !r.head_sha.is_empty(), "{n}: {r:?}");
                assert!(
                    !r.source_branch.is_empty() && !r.target_branch.is_empty() && !r.updated_at.is_empty(),
                    "{n}: {r:?}"
                );
                assert!(!r.draft, "{n}: drafts are excluded when include_drafts is false");
                assert!(r.author.login.is_some(), "{n}: author");
                assert!(seen.insert(r.r#ref.clone()), "{n}: duplicate ref");
            }
            if kind == ReviewKind::ReviewRequested {
                first = list.into_iter().next();
            }
        }
        let first = first.unwrap_or_else(|| panic!("{n}: no review"));

        let d = h.get(&first.r#ref).await.unwrap_or_else(|e| panic!("{n}: get: {e}"));
        assert_eq!(d.review.r#ref, first.r#ref, "{n}: detail ref");
        assert!(!d.body_html.to_ascii_lowercase().contains("<script"), "{n}: sanitized");

        h.approve(&first.r#ref, &first.head_sha).await.unwrap_or_else(|e| panic!("{n}: approve: {e}"));
        h.comment(&first.r#ref, "hello").await.unwrap_or_else(|e| panic!("{n}: comment: {e}"));
        h.request_changes(&first.r#ref, "please fix")
            .await
            .unwrap_or_else(|e| panic!("{n}: request_changes: {e}"));
        let created = h.create(&create_req(case.repo)).await.unwrap_or_else(|e| panic!("{n}: create: {e}"));
        assert!(created.r#ref.number > 0 && created.kind == ReviewKind::Authored, "{n}: created");
        let found =
            h.find_for_branch(case.repo, "feature/mine").await.unwrap_or_else(|e| panic!("{n}: find: {e}"));
        assert!(found.is_some(), "{n}: find_for_branch");

        let spec = h.fetch_refspec(&first.r#ref, "kelta/review-1");
        assert!(
            spec.contains(&first.r#ref.number.to_string())
                && spec.ends_with(":kelta/review-1")
                && spec.contains("/head:"),
            "{n}: {spec}"
        );
    }
}

#[test]
fn every_code_host_resolves_remotes_of_its_own_host_only() {
    let github = host("github-work", "github", "https://api.github.com");
    let gitlab = host("gitlab-acme", "gitlab", "https://gitlab.acme.example");
    assert_eq!(github.repo_from_remote("git@github.com:acme/shop.git").as_deref(), Some("acme/shop"));
    assert_eq!(
        gitlab.repo_from_remote("git@GITLAB.acme.example:grp/sub/proj.git").as_deref(),
        Some("grp/sub/proj")
    );
    assert_eq!(github.repo_from_remote("git@gitlab.acme.example:grp/proj.git"), None);
    assert_eq!(gitlab.repo_from_remote("git@github.com:acme/shop.git"), None);
    for h in [&github, &gitlab] {
        assert_eq!(h.repo_from_remote("/local/path"), None);
        assert_eq!(h.repo_from_remote(""), None);
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
        assert_eq!(err_code(h.create(&create_req(case.repo)).await), ErrorCode::NeedsAuth, "{n}: create");
        assert_eq!(err_code(h.find_for_branch(case.repo, "b").await), ErrorCode::NeedsAuth, "{n}: find");
        assert_eq!(err_code(h.changed_since_last().await), ErrorCode::NeedsAuth, "{n}: gate");
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

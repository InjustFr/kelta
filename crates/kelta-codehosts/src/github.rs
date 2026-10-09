//! GitHub (github.com and GHE) code host.
//!
//! List: one GraphQL request with aliased searches (`review-requested:@me` or
//! `user-review-requested:@me`, and `author:@me`). Gate: `GET /notifications?participating=true`
//! with `If-None-Match` and `X-Poll-Interval`, only for classic / `gh` OAuth tokens (fine-grained
//! tokens cannot read notifications). Actions: REST (`/pulls/{n}/reviews` with `commit_id`).

use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use kelta_http::graphql::graphql;
use kelta_http::util::{trim_url, url_host};
use kelta_http::{AuthScheme, Authed, HttpCtx, HttpRequest, markdown};
use kelta_proto::api::{CodeHost, SecretResolver};
use kelta_proto::codehost::{
    CiCheck, CiState, CodeHostKind, FileChange, MyReviewState, PrCreate, Review, ReviewDecision,
    ReviewDetail, ReviewKind, ReviewQuery, ReviewRef, Reviewer,
};
use kelta_proto::error::{ErrorCode, KeltaError};
use kelta_proto::ids::AccountId;
use kelta_proto::settings::AccountConfig;
use kelta_proto::tracker::User;
use parking_lot::Mutex;
use serde_json::{Value, json};
use tokio::time::Instant;

use crate::common::{host_matches, linked_tickets, parse_remote, s, user};

const DEFAULT_POLL_INTERVAL: Duration = Duration::from_secs(60);

const PR_FRAGMENT: &str = "fragment Pr on PullRequest { number title url isDraft headRefOid headRefName baseRefName updatedAt additions deletions mergeable reviewDecision \
author{ login avatarUrl ... on User{ name } } repository{ nameWithOwner } labels(first:10){ nodes{ name } } \
commits(last:1){ nodes{ commit{ statusCheckRollup{ state } } } } \
latestOpinionatedReviews(first:20){ nodes{ state commit{ oid } author{ login } } } \
latestReviews(first:20){ nodes{ commit{ oid } author{ login } } } }";

#[derive(Default)]
struct Gate {
    /// The token cannot read notifications (or the endpoint is unavailable): never gate again.
    disabled: bool,
    last_poll: Option<Instant>,
    interval: Option<Duration>,
    /// A poll reported a change that no successful `list_reviews` has consumed yet.
    pending: bool,
}

pub struct GithubHost {
    api: String,
    graphql: String,
    web: String,
    web_host: String,
    auth: Authed,
    me: Mutex<Option<User>>,
    gate: Mutex<Gate>,
}

impl GithubHost {
    pub fn new(
        account: &AccountConfig,
        http: HttpCtx,
        secrets: Arc<dyn SecretResolver>,
    ) -> Result<Self, KeltaError> {
        let api = account
            .effective_base_url()
            .map(|u| trim_url(&u))
            .ok_or_else(|| KeltaError::invalid("github account needs a base_url"))?;
        let web = account.web_url.as_deref().map(trim_url).unwrap_or_else(|| web_base(&api));
        let scheme = AuthScheme::from_account(account).unwrap_or(AuthScheme::Bearer);
        let auth = Authed::new(http, secrets, account.effective_secret(), Some(&api), scheme)
            .with_header("Accept", "application/vnd.github+json")
            .with_header("X-GitHub-Api-Version", "2022-11-28");
        Ok(Self {
            graphql: graphql_url(&api),
            web_host: url_host(&web).unwrap_or_default(),
            web,
            api,
            auth,
            me: Mutex::new(None),
            gate: Mutex::new(Gate::default()),
        })
    }

    fn account(&self) -> &AccountId {
        self.auth.http().account_id()
    }

    async fn rest(&self, req: HttpRequest) -> Result<kelta_http::HttpResponse<Value>, KeltaError> {
        self.auth.send_json::<Value>(req).await
    }

    fn repo_url(&self, repo: &str, tail: &str) -> String {
        format!("{}/repos/{repo}{tail}", self.api)
    }

    fn user_from(v: &Value) -> User {
        match s(v, "login") {
            Some(l) => user(l, s(v, "name"), s(v, "avatar_url").or_else(|| s(v, "avatarUrl"))),
            None => User {
                id: "ghost".into(),
                name: "ghost".into(),
                login: Some("ghost".into()),
                avatar_url: None,
            },
        }
    }

    fn graphql_pr(&self, n: &Value, kind: ReviewKind, me: &str) -> Option<Review> {
        let number = n.get("number").and_then(Value::as_u64)?;
        let repo = n.pointer("/repository/nameWithOwner").and_then(Value::as_str)?.to_owned();
        let title = s(n, "title").unwrap_or("").to_owned();
        let branch = s(n, "headRefName").unwrap_or("").to_owned();
        let mine_node = n
            .pointer("/latestOpinionatedReviews/nodes")
            .and_then(Value::as_array)
            .and_then(|a| a.iter().find(|r| r.pointer("/author/login").and_then(Value::as_str) == Some(me)));
        let mine = mine_node.and_then(|r| s(r, "state"));
        // latestReviews also holds COMMENTED reviews, which the opinionated list leaves out.
        let reviewed_head = n
            .pointer("/latestReviews/nodes")
            .and_then(Value::as_array)
            .and_then(|a| a.iter().find(|r| r.pointer("/author/login").and_then(Value::as_str) == Some(me)))
            .filter(|_| kind != ReviewKind::Authored)
            .and_then(|r| r.pointer("/commit/oid").and_then(Value::as_str))
            .map(str::to_owned);
        let my_state = match (kind, mine) {
            (ReviewKind::Authored, _) => None,
            (_, Some("APPROVED")) => Some(MyReviewState::Approved),
            (_, Some("CHANGES_REQUESTED")) => Some(MyReviewState::ChangesRequested),
            (_, Some("COMMENTED")) => Some(MyReviewState::Commented),
            (_, _) => Some(MyReviewState::Pending),
        };
        Some(Review {
            r#ref: ReviewRef { account: self.account().clone(), repo, number },
            url: s(n, "url").unwrap_or("").to_owned(),
            author: n.get("author").map(Self::user_from).unwrap_or_else(|| Self::user_from(&Value::Null)),
            draft: n.get("isDraft").and_then(Value::as_bool).unwrap_or(false),
            head_sha: s(n, "headRefOid").unwrap_or("").to_owned(),
            source_branch: branch.clone(),
            target_branch: s(n, "baseRefName").unwrap_or("").to_owned(),
            ci: ci_from_rollup(
                n.pointer("/commits/nodes/0/commit/statusCheckRollup/state").and_then(Value::as_str),
            ),
            decision: decision_from(s(n, "reviewDecision")),
            my_state,
            mergeable: mergeable_from(s(n, "mergeable")),
            reviewed_head,
            labels: n
                .pointer("/labels/nodes")
                .and_then(Value::as_array)
                .map(|a| a.iter().filter_map(|l| s(l, "name").map(str::to_owned)).collect())
                .unwrap_or_default(),
            kind,
            updated_at: s(n, "updatedAt").unwrap_or("").to_owned(),
            linked_tickets: linked_tickets(&[&branch, &title]),
            additions: n.get("additions").and_then(Value::as_u64).map(|x| x as u32),
            deletions: n.get("deletions").and_then(Value::as_u64).map(|x| x as u32),
            title,
        })
    }

    /// REST pull request → review (no CI / decision: callers enrich).
    fn rest_pr(&self, v: &Value, repo_hint: &str, kind: ReviewKind) -> Option<Review> {
        let number = v.get("number").and_then(Value::as_u64)?;
        let repo = v.pointer("/base/repo/full_name").and_then(Value::as_str).unwrap_or(repo_hint).to_owned();
        let title = s(v, "title").unwrap_or("").to_owned();
        let branch = v.pointer("/head/ref").and_then(Value::as_str).unwrap_or("").to_owned();
        Some(Review {
            r#ref: ReviewRef { account: self.account().clone(), repo, number },
            url: s(v, "html_url").unwrap_or("").to_owned(),
            author: v.get("user").map(Self::user_from).unwrap_or_else(|| Self::user_from(&Value::Null)),
            draft: v.get("draft").and_then(Value::as_bool).unwrap_or(false),
            head_sha: v.pointer("/head/sha").and_then(Value::as_str).unwrap_or("").to_owned(),
            source_branch: branch.clone(),
            target_branch: v.pointer("/base/ref").and_then(Value::as_str).unwrap_or("").to_owned(),
            ci: CiState::None,
            decision: None,
            my_state: None,
            mergeable: v.get("mergeable").and_then(Value::as_bool),
            reviewed_head: None,
            labels: v
                .get("labels")
                .and_then(Value::as_array)
                .map(|a| a.iter().filter_map(|l| s(l, "name").map(str::to_owned)).collect())
                .unwrap_or_default(),
            kind,
            updated_at: s(v, "updated_at").unwrap_or("").to_owned(),
            linked_tickets: linked_tickets(&[&branch, &title]),
            additions: v.get("additions").and_then(Value::as_u64).map(|x| x as u32),
            deletions: v.get("deletions").and_then(Value::as_u64).map(|x| x as u32),
            title,
        })
    }

    /// Fine-grained / app tokens cannot read `/notifications`.
    fn token_supports_notifications(token: &str) -> bool {
        !(token.starts_with("github_pat_") || token.starts_with("ghs_") || token.starts_with("ghu_"))
    }
}

pub(crate) fn web_base(api: &str) -> String {
    if api == "https://api.github.com" {
        return "https://github.com".into();
    }
    api.strip_suffix("/api/v3").unwrap_or(api).to_owned()
}

pub(crate) fn graphql_url(api: &str) -> String {
    match api.strip_suffix("/api/v3") {
        Some(root) => format!("{root}/api/graphql"),
        None => format!("{api}/graphql"),
    }
}

fn ci_from_rollup(state: Option<&str>) -> CiState {
    match state {
        Some("SUCCESS") => CiState::Success,
        Some("FAILURE") => CiState::Failure,
        Some("ERROR") => CiState::Error,
        Some("PENDING" | "EXPECTED") => CiState::Pending,
        _ => CiState::None,
    }
}

fn decision_from(state: Option<&str>) -> Option<ReviewDecision> {
    match state {
        Some("APPROVED") => Some(ReviewDecision::Approved),
        Some("CHANGES_REQUESTED") => Some(ReviewDecision::ChangesRequested),
        Some("REVIEW_REQUIRED") => Some(ReviewDecision::ReviewRequired),
        _ => None,
    }
}

fn mergeable_from(state: Option<&str>) -> Option<bool> {
    match state {
        Some("MERGEABLE") => Some(true),
        Some("CONFLICTING") => Some(false),
        _ => None,
    }
}

fn review_state(state: &str) -> Option<MyReviewState> {
    match state {
        "APPROVED" => Some(MyReviewState::Approved),
        "CHANGES_REQUESTED" => Some(MyReviewState::ChangesRequested),
        "COMMENTED" => Some(MyReviewState::Commented),
        _ => None, // PENDING / DISMISSED carry no opinion
    }
}

/// Latest opinionated state per reviewer, oldest review first in `reviews`.
fn latest_by_reviewer(reviews: &[Value]) -> Vec<(String, MyReviewState)> {
    let mut out: Vec<(String, MyReviewState)> = Vec::new();
    for r in reviews {
        let (Some(login), Some(st)) =
            (r.pointer("/user/login").and_then(Value::as_str), s(r, "state").and_then(review_state))
        else {
            continue;
        };
        // A plain comment does not override an approval / change request.
        if st == MyReviewState::Commented && out.iter().any(|(l, _)| l == login) {
            continue;
        }
        match out.iter_mut().find(|(l, _)| l == login) {
            Some(e) => e.1 = st,
            None => out.push((login.to_owned(), st)),
        }
    }
    out
}

fn decision_from_reviews(by_reviewer: &[(String, MyReviewState)]) -> ReviewDecision {
    if by_reviewer.iter().any(|(_, s)| *s == MyReviewState::ChangesRequested) {
        ReviewDecision::ChangesRequested
    } else if by_reviewer.iter().any(|(_, s)| *s == MyReviewState::Approved) {
        ReviewDecision::Approved
    } else {
        ReviewDecision::ReviewRequired
    }
}

fn check_state(run: &Value) -> CiState {
    match (s(run, "status"), s(run, "conclusion")) {
        (Some("completed"), Some("success" | "neutral" | "skipped")) => CiState::Success,
        (Some("completed"), Some("failure" | "timed_out" | "action_required" | "startup_failure")) => {
            CiState::Failure
        }
        (Some("completed"), Some("cancelled")) => CiState::Error,
        (Some("completed"), _) => CiState::None,
        _ => CiState::Pending,
    }
}

fn combine_ci(checks: &[CiCheck]) -> CiState {
    let any = |st: CiState| checks.iter().any(|c| c.state == st);
    if any(CiState::Failure) || any(CiState::Error) {
        CiState::Failure
    } else if any(CiState::Pending) {
        CiState::Pending
    } else if any(CiState::Success) {
        CiState::Success
    } else {
        CiState::None
    }
}

/// My pending (draft) review of a pull request; GitHub only shows a pending review to its author.
struct Pending {
    id: u64,
    node_id: String,
    commit: String,
}

fn pending_of(reviews: &[Value]) -> Option<Pending> {
    let v = reviews.iter().find(|r| s(r, "state") == Some("PENDING"))?;
    Some(Pending {
        id: v.get("id").and_then(Value::as_u64)?,
        node_id: s(v, "node_id")?.to_owned(),
        commit: s(v, "commit_id").unwrap_or("").to_owned(),
    })
}

/// 422 answers of the review endpoints that really mean "your view of the PR is stale".
fn stale_head(e: KeltaError) -> KeltaError {
    if e.code != ErrorCode::InvalidArgument {
        return e;
    }
    let text = format!("{} {}", e.message, e.detail.as_ref().and_then(|d| d["body"].as_str()).unwrap_or(""));
    let t = text.to_ascii_lowercase();
    if t.contains("commit") || t.contains("head branch was modified") || t.contains("stale") {
        KeltaError::conflict("the pull request head changed; refresh and review again")
            .with_detail(json!({"status": 422}))
    } else {
        e
    }
}

#[async_trait]
impl CodeHost for GithubHost {
    fn kind(&self) -> CodeHostKind {
        CodeHostKind::Github
    }

    async fn me(&self) -> Result<User, KeltaError> {
        if let Some(u) = self.me.lock().clone() {
            return Ok(u);
        }
        let v = self.rest(HttpRequest::get(format!("{}/user", self.api))).await?.body;
        if s(&v, "login").is_none() {
            return Err(KeltaError::upstream("github /user returned no login"));
        }
        let u = Self::user_from(&v);
        *self.me.lock() = Some(u.clone());
        Ok(u)
    }

    async fn changed_since_last(&self) -> Result<bool, KeltaError> {
        let token = self.auth.token().await?;
        if !Self::token_supports_notifications(token.expose()) {
            return Ok(true);
        }
        {
            let g = self.gate.lock();
            if g.disabled {
                return Ok(true);
            }
            // X-Poll-Interval: asking earlier than the server allows is both rude and pointless.
            if g.pending {
                return Ok(true);
            }
            if let (Some(last), Some(iv)) = (g.last_poll, g.interval)
                && Instant::now() < last + iv
            {
                return Ok(false);
            }
        }
        let req = HttpRequest::get(format!("{}/notifications", self.api))
            .query("participating", "true")
            .with_etag();
        match self.auth.send_text(req).await {
            Ok(resp) => {
                let interval = resp
                    .headers
                    .get("x-poll-interval")
                    .and_then(|v| v.trim().parse::<u64>().ok())
                    .map(Duration::from_secs)
                    .unwrap_or(DEFAULT_POLL_INTERVAL);
                let mut g = self.gate.lock();
                g.last_poll = Some(Instant::now());
                g.interval = Some(interval);
                g.pending = !resp.not_modified;
                Ok(!resp.not_modified)
            }
            // Classic token without the `notifications` scope, or a server without the endpoint.
            Err(e) if matches!(e.code, ErrorCode::PermissionDenied | ErrorCode::NotFound) => {
                self.gate.lock().disabled = true;
                Ok(true)
            }
            Err(e) => Err(e),
        }
    }

    async fn list_reviews(&self, q: &ReviewQuery) -> Result<Vec<Review>, KeltaError> {
        let draft = if q.include_drafts { "" } else { " draft:false" };
        let requested = if q.include_team { "review-requested:@me" } else { "user-review-requested:@me" };
        let (alias, kind, search) = match q.kind {
            ReviewKind::ReviewRequested => (
                "reviewRequested",
                ReviewKind::ReviewRequested,
                format!("is:pr is:open {requested} archived:false{draft}"),
            ),
            ReviewKind::Authored => {
                ("authored", ReviewKind::Authored, format!("is:pr is:open author:@me archived:false{draft}"))
            }
        };
        // user-review-requested drops a PR once I submit a review, so a second search in the same
        // request finds the PRs I reviewed; they only come back when the head moved since (below).
        let reviewed =
            format!("is:pr is:open reviewed-by:@me -author:@me archived:false{draft} sort:updated-desc");
        let (decl, extra, vars) = match kind {
            ReviewKind::ReviewRequested => (
                ",$r:String!",
                " reviewedByMe: search(query:$r, type:ISSUE, first:50){ nodes{ ...Pr } }",
                json!({ "s": search, "r": reviewed }),
            ),
            ReviewKind::Authored => ("", "", json!({ "s": search })),
        };
        let query = format!(
            "query($s:String!{decl}){{ viewer{{ login }} {alias}: search(query:$s, type:ISSUE, first:50){{ nodes{{ ...Pr }} }}{extra} }} {PR_FRAGMENT}"
        );
        let data = graphql(&self.auth, &self.graphql, &query, vars).await?;
        self.gate.lock().pending = false;
        let me = data.pointer("/viewer/login").and_then(Value::as_str).unwrap_or("").to_owned();
        let nodes = |alias: &str| {
            data.pointer(&format!("/{alias}/nodes")).and_then(Value::as_array).cloned().unwrap_or_default()
        };
        let requested = nodes(alias).into_iter().filter_map(|n| self.graphql_pr(&n, kind, &me));
        let updated = nodes("reviewedByMe")
            .into_iter()
            .filter_map(|n| self.graphql_pr(&n, kind, &me))
            .filter(|r| r.reviewed_head.as_deref().is_some_and(|h| h != r.head_sha));
        let requested: Vec<_> = requested.collect();
        // A re-requested PR matches both searches.
        let updated: Vec<_> = updated.filter(|u| !requested.iter().any(|r| r.r#ref == u.r#ref)).collect();
        Ok(requested.into_iter().chain(updated).filter(|r| q.include_drafts || !r.draft).collect())
    }

    async fn get(&self, r: &ReviewRef) -> Result<ReviewDetail, KeltaError> {
        let pull_url = self.repo_url(&r.repo, &format!("/pulls/{}", r.number));
        let me = self.me().await?;
        let (pull, reviews, files) = tokio::join!(
            self.rest(HttpRequest::get(&pull_url)),
            self.rest(HttpRequest::get(format!("{pull_url}/reviews")).query("per_page", "100")),
            self.rest(HttpRequest::get(format!("{pull_url}/files")).query("per_page", "100")),
        );
        let pull = pull?.body;
        let reviews = reviews?.body.as_array().cloned().unwrap_or_default();
        let files = files?.body.as_array().cloned().unwrap_or_default();
        let kind = if pull.pointer("/user/login").and_then(Value::as_str) == me.login.as_deref() {
            ReviewKind::Authored
        } else {
            ReviewKind::ReviewRequested
        };
        let mut review = self
            .rest_pr(&pull, &r.repo, kind)
            .ok_or_else(|| KeltaError::upstream("pull request response without number"))?;
        review.r#ref = r.clone();

        // CI: check runs + legacy commit statuses of the head commit.
        let sha = review.head_sha.clone();
        let (runs, statuses) = tokio::join!(
            self.rest(
                HttpRequest::get(self.repo_url(&r.repo, &format!("/commits/{sha}/check-runs")))
                    .query("per_page", "100")
            ),
            self.rest(HttpRequest::get(self.repo_url(&r.repo, &format!("/commits/{sha}/status")))),
        );
        let mut checks: Vec<CiCheck> = Vec::new();
        if let Ok(runs) = runs {
            for run in runs.body.get("check_runs").and_then(Value::as_array).into_iter().flatten() {
                checks.push(CiCheck {
                    name: s(run, "name").unwrap_or("").to_owned(),
                    state: check_state(run),
                    url: s(run, "html_url").map(str::to_owned),
                });
            }
        }
        if let Ok(st) = statuses {
            for c in st.body.get("statuses").and_then(Value::as_array).into_iter().flatten() {
                let state = match s(c, "state") {
                    Some("success") => CiState::Success,
                    Some("failure") => CiState::Failure,
                    Some("error") => CiState::Error,
                    Some("pending") => CiState::Pending,
                    _ => CiState::None,
                };
                checks.push(CiCheck {
                    name: s(c, "context").unwrap_or("").to_owned(),
                    state,
                    url: s(c, "target_url").map(str::to_owned),
                });
            }
        }
        review.ci = combine_ci(&checks);

        let by_reviewer = latest_by_reviewer(&reviews);
        review.decision = Some(decision_from_reviews(&by_reviewer));
        let is_me = |r: &&Value| r.pointer("/user/login").and_then(Value::as_str) == me.login.as_deref();
        if kind == ReviewKind::ReviewRequested {
            review.reviewed_head = reviews
                .iter()
                .filter(is_me)
                .rfind(|r| s(r, "state").and_then(review_state).is_some())
                .and_then(|r| s(r, "commit_id").map(str::to_owned));
        }
        let pending_comments = match pending_of(&reviews) {
            Some(p) => self
                .rest(
                    HttpRequest::get(format!("{pull_url}/reviews/{}/comments", p.id))
                        .query("per_page", "100"),
                )
                .await?
                .body
                .as_array()
                .map_or(0, Vec::len) as u32,
            None => 0,
        };
        review.my_state = match kind {
            ReviewKind::Authored => None,
            ReviewKind::ReviewRequested => Some(
                by_reviewer
                    .iter()
                    .find(|(l, _)| Some(l.as_str()) == me.login.as_deref())
                    .map(|(_, s)| *s)
                    .unwrap_or(MyReviewState::Pending),
            ),
        };

        let mut reviewers: Vec<Reviewer> = by_reviewer
            .iter()
            .map(|(login, st)| Reviewer { user: user(login, None, None), state: Some(*st) })
            .collect();
        for u in pull.get("requested_reviewers").and_then(Value::as_array).into_iter().flatten() {
            let rv = Self::user_from(u);
            if !reviewers.iter().any(|x| x.user.id == rv.id) {
                reviewers.push(Reviewer { user: rv, state: Some(MyReviewState::Pending) });
            }
        }
        let files = files
            .iter()
            .map(|f| FileChange {
                path: s(f, "filename").unwrap_or("").to_owned(),
                additions: f.get("additions").and_then(Value::as_u64).unwrap_or(0) as u32,
                deletions: f.get("deletions").and_then(Value::as_u64).unwrap_or(0) as u32,
            })
            .collect();
        Ok(ReviewDetail {
            body_html: markdown::to_html(s(&pull, "body").unwrap_or("")),
            review,
            reviewers,
            checks,
            files,
            pending_comments,
        })
    }

    async fn approve(&self, r: &ReviewRef, head_sha: &str) -> Result<(), KeltaError> {
        if let Some(p) = self.pending_review(r).await? {
            // The events endpoint takes no commit_id: a pending review is fixed to the head it began on.
            if p.commit != head_sha {
                return Err(KeltaError::invalid(
                    "the head moved after your pending comments began; submit them with Comment or Request changes first",
                ));
            }
            return self.publish(r, &p, "APPROVE", "").await;
        }
        let url = self.repo_url(&r.repo, &format!("/pulls/{}/reviews", r.number));
        self.auth
            .send_text(HttpRequest::post(url).json(json!({"event": "APPROVE", "commit_id": head_sha})))
            .await
            .map(|_| ())
            .map_err(stale_head)
    }

    async fn comment(&self, r: &ReviewRef, body: &str) -> Result<(), KeltaError> {
        if let Some(p) = self.pending_review(r).await? {
            return self.publish(r, &p, "COMMENT", body).await;
        }
        // PR conversation comments live on the issue endpoint.
        let url = self.repo_url(&r.repo, &format!("/issues/{}/comments", r.number));
        self.auth.send_text(HttpRequest::post(url).json(json!({"body": body}))).await?;
        Ok(())
    }

    async fn request_changes(&self, r: &ReviewRef, body: &str) -> Result<(), KeltaError> {
        if let Some(p) = self.pending_review(r).await? {
            return self.publish(r, &p, "REQUEST_CHANGES", body).await;
        }
        let url = self.repo_url(&r.repo, &format!("/pulls/{}/reviews", r.number));
        self.auth
            .send_text(HttpRequest::post(url).json(json!({"event": "REQUEST_CHANGES", "body": body})))
            .await
            .map(|_| ())
            .map_err(stale_head)
    }

    async fn add_pending_comment(
        &self,
        r: &ReviewRef,
        path: &str,
        line: u32,
        body: &str,
    ) -> Result<(), KeltaError> {
        let p = match self.pending_review(r).await? {
            Some(p) => p,
            // No event = a pending review, created on the current head.
            None => {
                let url = self.repo_url(&r.repo, &format!("/pulls/{}/reviews", r.number));
                let v = self.auth.send_json::<Value>(HttpRequest::post(url).json(json!({}))).await?.body;
                let (Some(id), Some(node_id)) = (v["id"].as_u64(), s(&v, "node_id")) else {
                    return Err(KeltaError::upstream("create pending review response without id"));
                };
                Pending { id, node_id: node_id.to_owned(), commit: String::new() }
            }
        };
        // REST cannot add to an existing pending review; GraphQL can.
        let q = "mutation($id:ID!,$path:String!,$line:Int!,$body:String!){ addPullRequestReviewThread(input:{pullRequestReviewId:$id, path:$path, line:$line, side:RIGHT, body:$body}){ thread{ id } } }";
        graphql(
            &self.auth,
            &self.graphql,
            q,
            json!({"id": p.node_id, "path": path, "line": line, "body": body}),
        )
        .await
        .map(|_| ())
    }

    async fn create(&self, d: &PrCreate) -> Result<Review, KeltaError> {
        let url = self.repo_url(&d.repo, "/pulls");
        let body =
            json!({"title": d.title, "head": d.head, "base": d.base, "body": d.body, "draft": d.draft});
        let v = self.auth.send_json::<Value>(HttpRequest::post(url).json(body)).await.map_err(|e| {
            let text =
                format!("{} {}", e.message, e.detail.as_ref().and_then(|x| x["body"].as_str()).unwrap_or(""));
            if e.code == ErrorCode::InvalidArgument && text.contains("already exists") {
                KeltaError::conflict("a pull request already exists for this branch")
            } else {
                e
            }
        })?;
        self.rest_pr(&v.body, &d.repo, ReviewKind::Authored)
            .ok_or_else(|| KeltaError::upstream("create pull request response without number"))
    }

    async fn find_for_branch(&self, repo: &str, branch: &str) -> Result<Option<Review>, KeltaError> {
        let owner = repo.split('/').next().unwrap_or(repo);
        let head = if branch.contains(':') { branch.to_owned() } else { format!("{owner}:{branch}") };
        let v = self
            .rest(
                HttpRequest::get(self.repo_url(repo, "/pulls"))
                    .query("head", head)
                    .query("state", "open")
                    .query("per_page", "1"),
            )
            .await?
            .body;
        let me = self.me().await.ok();
        Ok(v.get(0).and_then(|p| {
            let mine = p.pointer("/user/login").and_then(Value::as_str)
                == me.as_ref().and_then(|m| m.login.as_deref());
            self.rest_pr(p, repo, if mine { ReviewKind::Authored } else { ReviewKind::ReviewRequested })
        }))
    }

    fn fetch_refspec(&self, r: &ReviewRef, local_branch: &str) -> String {
        format!("pull/{}/head:{local_branch}", r.number)
    }

    fn repo_from_remote(&self, url: &str) -> Option<String> {
        let (host, path) = parse_remote(url)?;
        if !host_matches(&host, &self.web_host) {
            return None;
        }
        let mut seg = path.split('/');
        let (owner, repo) = (seg.next()?, seg.next()?);
        (seg.next().is_none() && !owner.is_empty() && !repo.is_empty()).then(|| format!("{owner}/{repo}"))
    }
}

impl GithubHost {
    async fn pending_review(&self, r: &ReviewRef) -> Result<Option<Pending>, KeltaError> {
        let url = self.repo_url(&r.repo, &format!("/pulls/{}/reviews", r.number));
        let v = self.rest(HttpRequest::get(url).query("per_page", "100")).await?.body;
        Ok(v.as_array().and_then(|a| pending_of(a)))
    }

    async fn publish(&self, r: &ReviewRef, p: &Pending, event: &str, body: &str) -> Result<(), KeltaError> {
        let url = self.repo_url(&r.repo, &format!("/pulls/{}/reviews/{}/events", r.number, p.id));
        self.auth
            .send_text(HttpRequest::post(url).json(json!({"event": event, "body": body})))
            .await
            .map(|_| ())
            .map_err(stale_head)
    }

    /// Browser base URL (`https://github.com`, GHE web root).
    pub fn web_url(&self) -> &str {
        &self.web
    }
}

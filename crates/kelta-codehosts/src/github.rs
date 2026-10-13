//! GitHub (github.com and GHE) code host.
//!
//! List: one GraphQL request with aliased searches (`review-requested:@me` or
//! `user-review-requested:@me`, and `author:@me`). Gate: `GET /notifications?participating=true`
//! with `If-None-Match` and `X-Poll-Interval`, only for classic / `gh` OAuth tokens (fine-grained
//! tokens cannot read notifications). Actions: REST (`/pulls/{n}/reviews` with `commit_id`).

use std::collections::HashMap;
use std::sync::{Arc, OnceLock};
use std::time::Duration;

use async_trait::async_trait;
use kelta_http::graphql::graphql;
use kelta_http::util::{link_rel, trim_url, url_host};
use kelta_http::{AuthScheme, Authed, HttpCtx, HttpRequest, markdown};
use kelta_proto::api::{CodeHost, SecretResolver};
use kelta_proto::codehost::{
    CiCheck, CiState, CodeHostKind, FailedCheck, Feedback, FeedbackReview, FeedbackThread, FileChange,
    MergeMethod, MyReviewState, PrCreate, PrState, Review, ReviewDecision, ReviewDetail, ReviewKind,
    ReviewQuery, ReviewRef, Reviewer,
};
use kelta_proto::error::{ErrorCode, KeltaError};
use kelta_proto::ids::AccountId;
use kelta_proto::settings::AccountConfig;
use kelta_proto::tracker::User;
use parking_lot::Mutex;
use regex::Regex;
use serde_json::{Value, json};
use tokio::time::Instant;

use crate::common::{
    MAX_LOGS, clean_log_line, feedback_error, host_matches, linked_tickets, log_tail, parse_remote,
    repo_path, s, user,
};

/// shortcut: at most this many REST pages per list, upgrade when PRs carry more reviews or files.
const MAX_PAGES: u32 = 5;
const DEFAULT_POLL_INTERVAL: Duration = Duration::from_secs(60);

const PR_FRAGMENT: &str = "fragment Pr on PullRequest { number title url isDraft headRefOid headRefName baseRefName updatedAt createdAt additions deletions changedFiles mergeable reviewDecision \
files(first:100){ nodes{ path additions deletions } } \
author{ login avatarUrl ... on User{ name email } } repository{ nameWithOwner } labels(first:10){ nodes{ name } } \
commits(last:1){ nodes{ commit{ committedDate statusCheckRollup{ state contexts(first:30){ nodes{ ... on CheckRun{ name conclusion } ... on StatusContext{ context state } } } } } } } \
reviewRequests(first:20){ nodes{ requestedReviewer{ ... on User{ login } ... on Team{ combinedSlug } } } } \
latestOpinionatedReviews(first:20){ nodes{ state submittedAt commit{ oid } author{ login } } } \
latestReviews(first:20){ nodes{ state commit{ oid } author{ login } } } \
timelineItems(last:20, itemTypes:[REVIEW_REQUESTED_EVENT]){ nodes{ ... on ReviewRequestedEvent{ createdAt requestedReviewer{ ... on User{ login } } } } } }";

/// Review threads, spread into the authored search only (#208). Their last comments cost one
/// point per PR (100 threads × a comments connection), so [`LAST_COMMENTS`] reads them apart.
const THREADS_FRAGMENT: &str =
    "fragment Threads on PullRequest { id reviewThreads(first:100){ nodes{ isResolved } } }";

/// The last comment of each review thread, for the PRs whose threads moved (#263).
const LAST_COMMENTS: &str = "query($ids:[ID!]!){ nodes(ids:$ids){ ... on PullRequest { reviewThreads(first:100){ nodes{ isResolved \
comments(last:1){ nodes{ createdAt author{ login } } } } } } } }";

/// My PR `(repo, number)` → `((updatedAt, unresolved threads), last comment at)`.
type ThreadCache = HashMap<(String, u64), ((String, u32), Option<String>)>;

/// Unresolved threads (filtered client side: the connection has no `isResolved` argument), the
/// latest review per reviewer and the head commit (FLOW §4.2).
const FEEDBACK_QUERY: &str = "query($o:String!,$n:String!,$num:Int!){ viewer{ login } repository(owner:$o,name:$n){ \
pullRequest(number:$num){ headRefOid url reviewThreads(first:100){ nodes{ id isResolved path line originalLine \
comments(first:30){ nodes{ author{ login } body url createdAt } } } } \
latestReviews(first:50){ nodes{ state body submittedAt author{ login } } } } } }";

const RESOLVE_THREAD: &str =
    "mutation($id:ID!){ resolveReviewThread(input:{threadId:$id}){ thread{ id isResolved } } }";

/// GitHub's own verdict on a PR for the detail (#379): the REST check runs keep the cancelled runs
/// of superseded suites, and the REST reviews know nothing of the required approval count.
const PR_VERDICT: &str = "query($o:String!,$n:String!,$num:Int!){ repository(owner:$o,name:$n){ pullRequest(number:$num){ reviewDecision \
commits(last:1){ nodes{ commit{ oid statusCheckRollup{ state contexts(first:100){ nodes{ ... on CheckRun{ name conclusion } ... on StatusContext{ context state } } } } } } } } } }";

const PR_ID: &str = "query($o:String!,$n:String!,$num:Int!){ repository(owner:$o,name:$n){ pullRequest(number:$num){ id mergeStateStatus } } }";

const ARM_AUTO_MERGE: &str = "mutation($id:ID!,$m:PullRequestMergeMethod!){ \
enablePullRequestAutoMerge(input:{pullRequestId:$id,mergeMethod:$m}){ clientMutationId } }";

const MERGE_PR: &str = "mutation($id:ID!,$m:PullRequestMergeMethod!){ \
mergePullRequest(input:{pullRequestId:$id,mergeMethod:$m}){ clientMutationId } }";

const DISARM_AUTO_MERGE: &str =
    "mutation($id:ID!){ disablePullRequestAutoMerge(input:{pullRequestId:$id}){ clientMutationId } }";
const MARK_READY: &str =
    "mutation($id:ID!){ markPullRequestReadyForReview(input:{pullRequestId:$id}){ clientMutationId } }";
const TO_DRAFT: &str =
    "mutation($id:ID!){ convertPullRequestToDraft(input:{pullRequestId:$id}){ clientMutationId } }";

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
    threads: Mutex<ThreadCache>,
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
            threads: Mutex::new(HashMap::new()),
        })
    }

    fn account(&self) -> &AccountId {
        self.auth.http().account_id()
    }

    async fn rest(&self, req: HttpRequest) -> Result<kelta_http::HttpResponse<Value>, KeltaError> {
        self.auth.send_json::<Value>(req).await
    }

    /// Rows of a paged REST list, following `Link: rel="next"` up to [`MAX_PAGES`].
    async fn paged(&self, req: HttpRequest) -> Result<Vec<Value>, KeltaError> {
        self.paged_at(req, "").await
    }

    /// [`Self::paged`] for a list wrapped in an object: the rows sit at JSON `pointer` of each page.
    async fn paged_at(&self, req: HttpRequest, pointer: &str) -> Result<Vec<Value>, KeltaError> {
        let mut out = Vec::new();
        let mut next = Some(req);
        for _ in 0..MAX_PAGES {
            let Some(req) = next.take() else { break };
            let resp = self.rest(req).await?;
            out.extend(resp.body.pointer(pointer).and_then(Value::as_array).cloned().unwrap_or_default());
            // The token rides on every page: never follow a `next` that leaves the API origin.
            next = link_rel(&resp.headers, "next")
                .filter(|u| u.starts_with(&format!("{}/", self.api)))
                .map(HttpRequest::get);
        }
        Ok(out)
    }

    fn repo_url(&self, repo: &str, tail: &str) -> Result<String, KeltaError> {
        Ok(format!("{}/repos/{}{tail}", self.api, repo_path(repo)?))
    }

    fn user_from(v: &Value) -> User {
        match s(v, "login") {
            Some(l) => User {
                email: s(v, "email").filter(|e| !e.is_empty()).map(str::to_owned),
                ..user(l, s(v, "name"), s(v, "avatar_url").or_else(|| s(v, "avatarUrl")))
            },
            None => User {
                id: "ghost".into(),
                name: "ghost".into(),
                login: Some("ghost".into()),
                avatar_url: None,
                email: None,
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
            .filter(|_| kind == ReviewKind::ReviewRequested)
            .and_then(|r| r.pointer("/commit/oid").and_then(Value::as_str))
            .map(str::to_owned);
        let my_state = match (kind, mine) {
            (ReviewKind::Authored | ReviewKind::Team, _) => None,
            (_, Some("APPROVED")) => Some(MyReviewState::Approved),
            (_, Some("CHANGES_REQUESTED")) => Some(MyReviewState::ChangesRequested),
            (_, Some("COMMENTED")) => Some(MyReviewState::Commented),
            (_, _) => Some(MyReviewState::Pending),
        };
        let decision = decision_from(s(n, "reviewDecision"));
        let head_sha = s(n, "headRefOid").unwrap_or("").to_owned();
        let (ci, failed_check_names) = rollup(n.pointer("/commits/nodes/0/commit").unwrap_or(&Value::Null));
        let (unresolved_threads, last_comment_at) = thread_state(n, me);
        let (waiting_on, since) = waiting_state(n, &head_sha);
        let (requested_at, blocking) = match kind {
            ReviewKind::Authored | ReviewKind::Team => (since.clone(), false),
            ReviewKind::ReviewRequested => request_state(n, me, decision),
        };
        let changes_on_head: Vec<&Value> = n
            .pointer("/latestOpinionatedReviews/nodes")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter(|r| {
                s(r, "state") == Some("CHANGES_REQUESTED")
                    && r.pointer("/commit/oid").and_then(Value::as_str) == Some(head_sha.as_str())
            })
            .collect();
        let author_to_act = !changes_on_head.is_empty();
        let author_since =
            changes_on_head.iter().filter_map(|r| s(r, "submittedAt")).max().map(str::to_owned);
        Some(Review {
            r#ref: ReviewRef { account: self.account().clone(), repo, number },
            url: s(n, "url").unwrap_or("").to_owned(),
            author: n.get("author").map(Self::user_from).unwrap_or_else(|| Self::user_from(&Value::Null)),
            draft: n.get("isDraft").and_then(Value::as_bool).unwrap_or(false),
            head_sha,
            source_branch: branch.clone(),
            target_branch: s(n, "baseRefName").unwrap_or("").to_owned(),
            ci,
            failed_check_names,
            decision,
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
            decision_head: n
                .pointer("/latestOpinionatedReviews/nodes")
                .and_then(Value::as_array)
                .and_then(|a| {
                    a.iter()
                        .filter(|r| matches!(s(r, "state"), Some("APPROVED" | "CHANGES_REQUESTED")))
                        .max_by_key(|r| s(r, "submittedAt").unwrap_or(""))
                })
                .and_then(|r| r.pointer("/commit/oid").and_then(Value::as_str))
                .map(str::to_owned),
            requested_at,
            blocking,
            waiting_on,
            author_to_act,
            waiting_since: since,
            author_since,
            unresolved_threads,
            last_comment_at,
            created_at: s(n, "createdAt").map(str::to_owned),
            files: n
                .pointer("/files/nodes")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(|f| {
                    Some(FileChange {
                        path: s(f, "path")?.to_owned(),
                        additions: f.get("additions").and_then(Value::as_u64).unwrap_or(0) as u32,
                        deletions: f.get("deletions").and_then(Value::as_u64).unwrap_or(0) as u32,
                    })
                })
                .collect(),
            changed_files: n.get("changedFiles").and_then(Value::as_u64).map(|x| x as u32),
            reviewers: latest_reviewers(n),
            title,
            ..Review::default()
        })
    }

    /// My PRs' newest last comment on an open thread not by me: [`LAST_COMMENTS`] for the PRs with
    /// open threads whose `(updatedAt, unresolved threads)` moved since the last poll. Best effort.
    async fn last_comments(&self, reviews: &mut [Review], nodes: &[Value], me: &str) {
        let key = |r: &Review| (r.r#ref.repo.clone(), r.r#ref.number);
        let stamp = |r: &Review| (r.updated_at.clone(), r.unresolved_threads.unwrap_or(0));
        let node_id = |r: &Review| {
            nodes.iter().find_map(|n| {
                (n.get("number").and_then(Value::as_u64) == Some(r.r#ref.number)
                    && n.pointer("/repository/nameWithOwner").and_then(Value::as_str) == Some(&r.r#ref.repo))
                .then(|| s(n, "id"))
                .flatten()
            })
        };
        let mut todo: Vec<(usize, &str)> = Vec::new();
        {
            let mut cache = self.threads.lock();
            cache.retain(|k, _| reviews.iter().any(|r| key(r) == *k));
            for (i, r) in reviews.iter_mut().enumerate() {
                if r.unresolved_threads.unwrap_or(0) == 0 {
                    continue;
                }
                match cache.get(&key(r)).filter(|c| c.0 == stamp(r)) {
                    Some(c) => r.last_comment_at = c.1.clone(),
                    None => todo.extend(node_id(r).map(|id| (i, id))),
                }
            }
        }
        if todo.is_empty() {
            return;
        }
        let ids: Vec<&str> = todo.iter().map(|(_, id)| *id).collect();
        let Ok(data) = graphql(&self.auth, &self.graphql, LAST_COMMENTS, json!({ "ids": ids })).await else {
            return;
        };
        let found = data.get("nodes").and_then(Value::as_array).cloned().unwrap_or_default();
        // `nodes(ids:)` answers in the order asked, null for a node gone since.
        for ((i, _), n) in todo.iter().zip(&found) {
            let r = &mut reviews[*i];
            let (Some(_), at) = thread_state(n, me) else { continue };
            r.last_comment_at = at.clone();
            self.threads.lock().insert(key(r), (stamp(r), at));
        }
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
            mergeable: v.get("mergeable").and_then(Value::as_bool),
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
            changed_files: v.get("changed_files").and_then(Value::as_u64).map(|x| x as u32),
            created_at: s(v, "created_at").map(str::to_owned),
            title,
            ..Review::default()
        })
    }

    /// Check runs of `sha` (REST), shared by `get` and `feedback`.
    async fn check_runs(&self, repo: &str, sha: &str) -> Result<Vec<Value>, KeltaError> {
        let url = self.repo_url(repo, &format!("/commits/{sha}/check-runs"))?;
        self.paged_at(HttpRequest::get(url).query("per_page", "100"), "/check_runs").await
    }

    /// Last lines of a GitHub Actions job log (`None` for other apps or when unreadable).
    async fn job_log_tail(&self, repo: &str, run: &Value) -> Option<String> {
        if run.pointer("/app/slug").and_then(Value::as_str) != Some("github-actions") {
            return None;
        }
        let id = run.get("id").and_then(Value::as_u64)?;
        // 302 to a signed blob URL on another host: the client stops cross-host redirects, so
        // fetch the `location` ourselves without credentials.
        let url = self.repo_url(repo, &format!("/actions/jobs/{id}/logs")).ok()?;
        let text = match self.auth.send_text(HttpRequest::get(url)).await {
            Ok(r) => r.body,
            Err(e) => {
                let loc = e.detail.as_ref()?.get("location")?.as_str()?;
                self.auth.http().send_text(HttpRequest::get(loc)).await.ok()?.body
            }
        };
        Some(actions_log_tail(&text))
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

/// Tail of a GitHub Actions log: no line timestamps, cut after the last `##[error]` line, which
/// drops the "Post job cleanup" steps.
fn actions_log_tail(text: &str) -> String {
    static STAMP: OnceLock<Option<Regex>> = OnceLock::new();
    let stamp =
        STAMP.get_or_init(|| Regex::new(r"^\d{4}-\d\d-\d\dT\d\d:\d\d:\d\d(?:\.\d+)?Z ").ok()).as_ref();
    let mut lines: Vec<String> = text
        .lines()
        .map(|l| {
            let l = clean_log_line(l);
            stamp.map_or(l.clone(), |re| re.replace(&l, "").into_owned())
        })
        .collect();
    if let Some(last) = lines.iter().rposition(|l| l.contains("##[error]")) {
        lines.truncate(last + 1);
    }
    log_tail(&lines)
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

/// A commit's `statusCheckRollup`: GitHub's state, which ignores superseded suites, and the failed
/// contexts (a cancelled run is left out of the names but still keeps the state off green).
fn rollup(commit: &Value) -> (CiState, Vec<String>) {
    let ci = ci_from_rollup(commit.pointer("/statusCheckRollup/state").and_then(Value::as_str));
    let failed = commit
        .pointer("/statusCheckRollup/contexts/nodes")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|c| {
            matches!(
                s(c, "conclusion").or(s(c, "state")),
                Some("FAILURE" | "TIMED_OUT" | "STARTUP_FAILURE" | "ACTION_REQUIRED" | "ERROR")
            )
        })
        .filter_map(|c| s(c, "name").or(s(c, "context")).map(str::to_owned))
        .collect();
    (ci, failed)
}

fn decision_from(state: Option<&str>) -> Option<ReviewDecision> {
    match state {
        Some("APPROVED") => Some(ReviewDecision::Approved),
        Some("CHANGES_REQUESTED") => Some(ReviewDecision::ChangesRequested),
        Some("REVIEW_REQUIRED") => Some(ReviewDecision::ReviewRequired),
        _ => None,
    }
}

/// When my review was last requested, and whether I am the last required reviewer: the review is
/// required and every other user asked has reviewed since their latest request.
// shortcut: team requests and removed requests are ignored (no extra GraphQL field); add
// `reviewRequests` if a pending team request must keep a PR out of the blocking band.
fn request_state(n: &Value, me: &str, decision: Option<ReviewDecision>) -> (Option<String>, bool) {
    let nodes = |p: &str| n.pointer(p).and_then(Value::as_array).cloned().unwrap_or_default();
    let login = |v: &Value, p: &str| v.pointer(p).and_then(Value::as_str).map(str::to_owned);
    let mut asked: Vec<(String, String)> = Vec::new(); // (login, latest request)
    for e in nodes("/timelineItems/nodes") {
        let (Some(who), Some(at)) = (login(&e, "/requestedReviewer/login"), s(&e, "createdAt")) else {
            continue;
        };
        match asked.iter_mut().find(|(l, _)| *l == who) {
            Some((_, t)) if t.as_str() < at => *t = at.to_owned(),
            Some(_) => {}
            None => asked.push((who, at.to_owned())),
        }
    }
    let reviews = nodes("/latestOpinionatedReviews/nodes");
    let reviewed_since = |who: &str, at: &str| {
        reviews.iter().any(|r| {
            login(r, "/author/login").as_deref() == Some(who) && s(r, "submittedAt").is_some_and(|t| t > at)
        })
    };
    let requested_at = asked.iter().find(|(l, _)| l == me).map(|(_, t)| t.clone());
    let others_pending = asked.iter().any(|(l, t)| l != me && !reviewed_since(l, t));
    (requested_at, decision == Some(ReviewDecision::ReviewRequired) && !others_pending)
}

/// Who my PR waits on and since when: users still asked (since their latest request) and those
/// who requested changes on an older head (since the head commit, i.e. my last push).
// shortcut: the push time is the head's `committedDate` (a rebase keeps the old date) and team
// requests have no request time; read `HeadRefForcePushedEvent` if that misleads.
/// Unresolved review threads and the newest last comment among them that is not `me`'s (answered
/// threads wait on the reviewer); `(None, None)` without the `Threads` fragment.
/// `is:pr is:open … repo:a/b repo:c/d` searches, each within GitHub's 256-character query limit.
fn repo_searches(repos: &[String], include_drafts: bool) -> Vec<String> {
    let base = format!("is:pr is:open archived:false{}", if include_drafts { "" } else { " draft:false" });
    let mut out: Vec<String> = Vec::new();
    for r in repos {
        let term = format!(" repo:{r}");
        match out.last_mut() {
            Some(q) if q.len() + term.len() <= 256 => q.push_str(&term),
            _ => out.push(format!("{base}{term}")),
        }
    }
    out
}

fn thread_state(n: &Value, me: &str) -> (Option<u32>, Option<String>) {
    let Some(threads) = n.pointer("/reviewThreads/nodes").and_then(Value::as_array) else {
        return (None, None);
    };
    let open: Vec<&Value> =
        threads.iter().filter(|t| t.get("isResolved").and_then(Value::as_bool) == Some(false)).collect();
    let at = open
        .iter()
        .filter_map(|t| t.pointer("/comments/nodes/0"))
        .filter(|c| c.pointer("/author/login").and_then(Value::as_str) != Some(me))
        .filter_map(|c| s(c, "createdAt"))
        .fold(None, |a, b| kelta_proto::later_rfc3339(a, Some(b.to_owned())));
    (Some(open.len() as u32), at)
}

fn waiting_state(n: &Value, head: &str) -> (Vec<String>, Option<String>) {
    let nodes = |p: &str| n.pointer(p).and_then(Value::as_array).cloned().unwrap_or_default();
    let str_at = |v: &Value, p: &str| v.pointer(p).and_then(Value::as_str).map(str::to_owned);
    let events = nodes("/timelineItems/nodes");
    let mut waiting: Vec<(String, Option<String>)> = nodes("/reviewRequests/nodes")
        .iter()
        .filter_map(|rq| {
            str_at(rq, "/requestedReviewer/login").or_else(|| str_at(rq, "/requestedReviewer/combinedSlug"))
        })
        .map(|who| {
            let asked = events
                .iter()
                .filter(|e| str_at(e, "/requestedReviewer/login").as_deref() == Some(who.as_str()))
                .filter_map(|e| str_at(e, "/createdAt"))
                .max();
            (who, asked)
        })
        .collect();
    let pushed = str_at(n, "/commits/nodes/0/commit/committedDate");
    for r in nodes("/latestOpinionatedReviews/nodes") {
        let Some(who) = str_at(&r, "/author/login") else { continue };
        if s(&r, "state") == Some("CHANGES_REQUESTED")
            && str_at(&r, "/commit/oid").as_deref() != Some(head)
            && !waiting.iter().any(|(w, _)| *w == who)
        {
            waiting.push((who, pushed.clone()));
        }
    }
    let since = waiting.iter().filter_map(|(_, t)| t.clone()).min();
    (waiting.into_iter().map(|(w, _)| w).collect(), since)
}

/// Each reviewer's latest opinion, the PR author left out. `latestReviews` holds a user's last
/// review of any kind, so a later comment would hide an approval: the opinionated list wins and
/// `latestReviews` only adds reviewers it does not know.
fn latest_reviewers(n: &Value) -> Vec<Reviewer> {
    let author = n.pointer("/author/login").and_then(Value::as_str);
    let mut out: Vec<Reviewer> = Vec::new();
    for list in ["/latestOpinionatedReviews/nodes", "/latestReviews/nodes"] {
        for r in n.pointer(list).and_then(Value::as_array).into_iter().flatten() {
            let Some(login) =
                r.pointer("/author/login").and_then(Value::as_str).filter(|l| Some(*l) != author)
            else {
                continue;
            };
            let Some(state) = s(r, "state").and_then(review_state) else { continue };
            if !out.iter().any(|x| x.user.login.as_deref() == Some(login)) {
                out.push(Reviewer { user: user(login, None, None), state: Some(state) });
            }
        }
    }
    out
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
            ReviewKind::Authored | ReviewKind::Team => {
                ("authored", ReviewKind::Authored, format!("is:pr is:open author:@me archived:false{draft}"))
            }
        };
        // user-review-requested drops a PR once I submit a review, so a second search in the same
        // request finds the PRs I reviewed; they only come back when the head moved since (below).
        let reviewed =
            format!("is:pr is:open reviewed-by:@me -author:@me archived:false{draft} sort:updated-desc");
        // Mine also lists the PRs assigned to me (#374), from a second search in the same request.
        let assigned = format!("is:pr is:open assignee:@me -author:@me archived:false{draft}");
        let (decl, extra, vars) = match kind {
            ReviewKind::ReviewRequested => (
                ",$r:String!",
                " reviewedByMe: search(query:$r, type:ISSUE, first:50){ nodes{ ...Pr } }",
                json!({ "s": search, "r": reviewed }),
            ),
            ReviewKind::Authored | ReviewKind::Team => (
                ",$a:String!",
                " assigned: search(query:$a, type:ISSUE, first:20){ nodes{ ...Pr ...Threads } }",
                json!({ "s": search, "a": assigned }),
            ),
        };
        let (spread, threads) =
            if kind == ReviewKind::Authored { (" ...Threads", THREADS_FRAGMENT) } else { ("", "") };
        let query = format!(
            "query($s:String!{decl}){{ viewer{{ login }} {alias}: search(query:$s, type:ISSUE, first:50){{ nodes{{ ...Pr{spread} }} }}{extra} }} {PR_FRAGMENT} {threads}"
        );
        let data = graphql(&self.auth, &self.graphql, &query, vars).await?;
        // Only the review-requested list answers what the gate saw change.
        if q.kind == ReviewKind::ReviewRequested {
            self.gate.lock().pending = false;
        }
        let me = data.pointer("/viewer/login").and_then(Value::as_str).unwrap_or("").to_owned();
        let nodes = |alias: &str| {
            data.pointer(&format!("/{alias}/nodes")).and_then(Value::as_array).cloned().unwrap_or_default()
        };
        let mut listed = nodes(alias);
        listed.extend(nodes("assigned"));
        let requested = listed.iter().filter_map(|n| self.graphql_pr(n, kind, &me));
        let updated = nodes("reviewedByMe")
            .into_iter()
            .filter_map(|n| self.graphql_pr(&n, kind, &me))
            .filter(|r| r.reviewed_head.as_deref().is_some_and(|h| h != r.head_sha));
        let mut requested: Vec<_> = requested.collect();
        if kind == ReviewKind::Authored {
            self.last_comments(&mut requested, &listed, &me).await;
            // The authored search only returns my own PRs: the others came from `assigned`.
            requested.iter_mut().for_each(|r| r.assigned = r.author.login.as_deref() != Some(me.as_str()));
        }
        // A re-requested PR matches both searches.
        let updated: Vec<_> = updated.filter(|u| !requested.iter().any(|r| r.r#ref == u.r#ref)).collect();
        Ok(requested.into_iter().chain(updated).filter(|r| q.include_drafts || !r.draft).collect())
    }

    async fn list_open(&self, repos: &[String], include_drafts: bool) -> Result<Vec<Review>, KeltaError> {
        let chunks = repo_searches(repos, include_drafts);
        if chunks.is_empty() {
            return Ok(Vec::new());
        }
        // shortcut: 50 PRs per chunk, page when a source's repos hold more open PRs
        let decl: Vec<String> = (0..chunks.len()).map(|i| format!("$c{i}:String!")).collect();
        let fields: String = (0..chunks.len())
            .map(|i| format!(" c{i}: search(query:$c{i}, type:ISSUE, first:50){{ nodes{{ ...Pr }} }}"))
            .collect();
        let vars: serde_json::Map<String, Value> =
            chunks.iter().enumerate().map(|(i, c)| (format!("c{i}"), json!(c))).collect();
        let query = format!("query({}){{ viewer{{ login }}{fields} }} {PR_FRAGMENT}", decl.join(","));
        let data = graphql(&self.auth, &self.graphql, &query, Value::Object(vars)).await?;
        let me = data.pointer("/viewer/login").and_then(Value::as_str).unwrap_or("");
        Ok((0..chunks.len())
            .filter_map(|i| data.pointer(&format!("/c{i}/nodes")).and_then(Value::as_array))
            .flatten()
            .filter_map(|n| self.graphql_pr(n, ReviewKind::Team, me))
            .filter(|r| include_drafts || !r.draft)
            .collect())
    }

    async fn get(&self, r: &ReviewRef) -> Result<ReviewDetail, KeltaError> {
        let pull_url = self.repo_url(&r.repo, &format!("/pulls/{}", r.number))?;
        let me = self.me().await?;
        let (owner, name) = r.repo.split_once('/').unwrap_or((r.repo.as_str(), ""));
        let (pull, reviews, files, verdict) = tokio::join!(
            self.rest(HttpRequest::get(&pull_url)),
            self.paged(HttpRequest::get(format!("{pull_url}/reviews")).query("per_page", "100")),
            self.paged(HttpRequest::get(format!("{pull_url}/files")).query("per_page", "100")),
            graphql(&self.auth, &self.graphql, PR_VERDICT, json!({ "o": owner, "n": name, "num": r.number })),
        );
        // Best effort: without the verdict, CI and decision are recomputed from the REST lists.
        let verdict = verdict.ok().and_then(|d| d.pointer("/repository/pullRequest").cloned());
        let pull = pull?.body;
        let (reviews, files) = (reviews?, files?);
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
            self.check_runs(&r.repo, &sha),
            self.rest(HttpRequest::get(self.repo_url(&r.repo, &format!("/commits/{sha}/status"))?)),
        );
        let mut checks: Vec<CiCheck> = Vec::new();
        for run in runs.unwrap_or_default() {
            checks.push(CiCheck {
                name: s(&run, "name").unwrap_or("").to_owned(),
                state: check_state(&run),
                url: s(&run, "html_url").map(str::to_owned),
            });
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
        // The checks stay for display; the rollup of the head (not an older commit) decides.
        let head = verdict
            .as_ref()
            .and_then(|v| v.pointer("/commits/nodes/0/commit"))
            .filter(|c| s(c, "oid") == Some(sha.as_str()));
        (review.ci, review.failed_check_names) = match head {
            Some(c) => rollup(c),
            None => (
                combine_ci(&checks),
                checks
                    .iter()
                    .filter(|c| matches!(c.state, CiState::Failure | CiState::Error))
                    .map(|c| c.name.clone())
                    .collect(),
            ),
        };

        let by_reviewer = latest_by_reviewer(&reviews);
        // `reviewDecision` is null when the base branch requires no review.
        review.decision = verdict
            .as_ref()
            .and_then(|v| decision_from(s(v, "reviewDecision")))
            .or_else(|| Some(decision_from_reviews(&by_reviewer)));
        review.decision_head = reviews
            .iter()
            .rev()
            .find(|x| matches!(s(x, "state"), Some("APPROVED" | "CHANGES_REQUESTED")))
            .and_then(|x| s(x, "commit_id"))
            .map(str::to_owned);
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
                .paged(
                    HttpRequest::get(format!("{pull_url}/reviews/{}/comments", p.id))
                        .query("per_page", "100"),
                )
                .await?
                .len() as u32,
            None => 0,
        };
        review.my_state = match kind {
            ReviewKind::Authored | ReviewKind::Team => None,
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
        let state = match (pull.get("merged_at").is_some_and(|m| !m.is_null()), s(&pull, "state")) {
            (true, _) => PrState::Merged,
            (false, Some("closed")) => PrState::Closed,
            _ => PrState::Open,
        };
        Ok(ReviewDetail {
            body_html: markdown::to_html(s(&pull, "body").unwrap_or("")),
            review,
            state,
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
        let url = self.repo_url(&r.repo, &format!("/pulls/{}/reviews", r.number))?;
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
        self.post_note(r, body).await
    }

    async fn post_note(&self, r: &ReviewRef, body: &str) -> Result<(), KeltaError> {
        // PR conversation comments live on the issue endpoint.
        let url = self.repo_url(&r.repo, &format!("/issues/{}/comments", r.number))?;
        self.auth.send_text(HttpRequest::post(url).json(json!({"body": body}))).await?;
        Ok(())
    }

    async fn request_changes(&self, r: &ReviewRef, body: &str) -> Result<(), KeltaError> {
        if let Some(p) = self.pending_review(r).await? {
            return self.publish(r, &p, "REQUEST_CHANGES", body).await;
        }
        let url = self.repo_url(&r.repo, &format!("/pulls/{}/reviews", r.number))?;
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
                let url = self.repo_url(&r.repo, &format!("/pulls/{}/reviews", r.number))?;
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
        let url = self.repo_url(&d.repo, "/pulls")?;
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

    async fn update_title(&self, r: &ReviewRef, title: &str) -> Result<(), KeltaError> {
        let url = self.repo_url(&r.repo, &format!("/pulls/{}", r.number))?;
        self.auth.send_text(HttpRequest::patch(url).json(json!({ "title": title }))).await.map(|_| ())
    }

    async fn find_for_branch(&self, repo: &str, branch: &str) -> Result<Option<Review>, KeltaError> {
        let owner = repo.split('/').next().unwrap_or(repo);
        let head = if branch.contains(':') { branch.to_owned() } else { format!("{owner}:{branch}") };
        let v = self
            .rest(
                HttpRequest::get(self.repo_url(repo, "/pulls")?)
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

    async fn feedback(&self, r: &ReviewRef) -> Result<Feedback, KeltaError> {
        let refused = feedback_error("GitHub", "review threads", "pull_requests:read");
        let (owner, name) = r.repo.split_once('/').unwrap_or((r.repo.as_str(), ""));
        let vars = json!({ "o": owner, "n": name, "num": r.number });
        let data = graphql(&self.auth, &self.graphql, FEEDBACK_QUERY, vars).await.map_err(&refused)?;
        let me = data.pointer("/viewer/login").and_then(Value::as_str).unwrap_or("");
        let pr = data
            .pointer("/repository/pullRequest")
            .filter(|p| !p.is_null())
            .ok_or_else(|| KeltaError::not_found(format!("pull request {}#{}", r.repo, r.number)))?;
        let nodes = |ptr: &str| pr.pointer(ptr).and_then(Value::as_array).cloned().unwrap_or_default();
        let login =
            |v: &Value| v.pointer("/author/login").and_then(Value::as_str).unwrap_or("ghost").to_owned();

        let threads = nodes("/reviewThreads/nodes")
            .iter()
            .filter(|t| t.get("isResolved").and_then(Value::as_bool) == Some(false))
            .filter_map(|t| {
                let comments = t.pointer("/comments/nodes").and_then(Value::as_array)?;
                let first = comments.first()?;
                let line = t
                    .get("line")
                    .and_then(Value::as_u64)
                    .or_else(|| t.get("originalLine").and_then(Value::as_u64));
                Some(FeedbackThread {
                    id: s(t, "id")?.to_owned(),
                    author: login(first),
                    path: s(t, "path").map(str::to_owned),
                    line: line.map(|l| l as u32),
                    body_md: comments
                        .iter()
                        .map(|c| format!("{}: {}", login(c), s(c, "body").unwrap_or("").trim()))
                        .collect::<Vec<_>>()
                        .join("\n\n"),
                    url: s(first, "url").unwrap_or("").to_owned(),
                    last_comment_at: comments
                        .last()
                        .filter(|c| login(c) != me)
                        .and_then(|c| s(c, "createdAt"))
                        .map(str::to_owned),
                })
            })
            .collect();
        let latest = nodes("/latestReviews/nodes");
        let mut reviewers: Vec<String> = latest.iter().map(login).filter(|l| l != me).collect();
        reviewers.dedup();
        let reviews = latest
            .iter()
            .filter(|v| s(v, "body").is_some_and(|b| !b.trim().is_empty()))
            .map(|v| FeedbackReview {
                author: login(v),
                state: s(v, "state").and_then(review_state),
                body_md: s(v, "body").unwrap_or("").trim().to_owned(),
                submitted_at: s(v, "submittedAt").map(str::to_owned),
            })
            .collect();

        let sha = s(pr, "headRefOid").unwrap_or("");
        let (runs, statuses) = tokio::join!(
            self.check_runs(&r.repo, sha),
            self.rest(HttpRequest::get(self.repo_url(&r.repo, &format!("/commits/{sha}/status"))?)),
        );
        let runs = runs.map_err(&refused)?;
        let failed: Vec<&Value> = runs.iter().filter(|run| check_state(run) == CiState::Failure).collect();
        let mut tails = Vec::new();
        for run in failed.iter().take(MAX_LOGS) {
            tails.push(self.job_log_tail(&r.repo, run).await);
        }
        let mut failed_checks: Vec<FailedCheck> = failed
            .iter()
            .enumerate()
            .map(|(i, run)| FailedCheck {
                name: s(run, "name").unwrap_or("").to_owned(),
                url: s(run, "html_url").map(str::to_owned),
                log_tail: tails.get(i).cloned().flatten(),
            })
            .collect();
        // Legacy commit statuses (Jenkins, CircleCI, ...): no log to read. Unreadable is ignored.
        let statuses = statuses.map(|st| st.body).unwrap_or_default();
        for c in statuses.get("statuses").and_then(Value::as_array).into_iter().flatten() {
            if matches!(s(c, "state"), Some("failure" | "error")) {
                failed_checks.push(FailedCheck {
                    name: s(c, "context").unwrap_or("").to_owned(),
                    url: s(c, "target_url").map(str::to_owned),
                    log_tail: None,
                });
            }
        }
        Ok(Feedback { threads, reviews, failed_checks, reviewers })
    }

    async fn rerequest_review(&self, r: &ReviewRef, who: &[String]) -> Result<Vec<String>, KeltaError> {
        let pull_url = self.repo_url(&r.repo, &format!("/pulls/{}", r.number))?;
        if !who.is_empty() {
            // `waiting_on` names teams `org/slug`; logins never hold a `/`.
            let (teams, users): (Vec<&String>, Vec<&String>) = who.iter().partition(|w| w.contains('/'));
            let teams: Vec<&str> = teams.iter().filter_map(|t| t.split_once('/').map(|(_, s)| s)).collect();
            let body = json!({ "reviewers": users, "team_reviewers": teams });
            let url = format!("{pull_url}/requested_reviewers");
            // A still-pending request is not notified again: drop it first. Best effort, the POST re-adds it.
            let _ = self.auth.send_text(HttpRequest::delete(url.clone()).json(body.clone())).await;
            let post = || self.auth.send_text(HttpRequest::post(url.clone()).json(body.clone()));
            // The DELETE may have dropped every request: one retry, then say who was lost.
            if post().await.is_err() {
                post().await.map_err(|e| {
                    KeltaError::upstream(format!(
                        "review requests for {} were removed and could not be re-added: {e}",
                        who.join(", ")
                    ))
                })?;
            }
            return Ok(who.to_vec());
        }
        let me = self.me().await?;
        let reviews =
            self.paged(HttpRequest::get(format!("{pull_url}/reviews")).query("per_page", "100")).await?;
        let logins: Vec<String> = latest_by_reviewer(&reviews)
            .into_iter()
            .map(|(l, _)| l)
            .filter(|l| Some(l.as_str()) != me.login.as_deref())
            .collect();
        if logins.is_empty() {
            return Err(KeltaError::invalid("nobody has reviewed this pull request yet"));
        }
        let req =
            HttpRequest::post(format!("{pull_url}/requested_reviewers")).json(json!({ "reviewers": logins }));
        self.auth.send_text(req).await?;
        Ok(logins)
    }

    async fn resolve_threads(&self, _r: &ReviewRef, ids: &[String]) -> Result<(), KeltaError> {
        for id in ids {
            graphql(&self.auth, &self.graphql, RESOLVE_THREAD, json!({ "id": id })).await?;
        }
        Ok(())
    }

    async fn arm_auto_merge(&self, r: &ReviewRef, method: MergeMethod) -> Result<(), KeltaError> {
        let m = match method {
            MergeMethod::Squash => "SQUASH",
            MergeMethod::Merge => "MERGE",
            MergeMethod::Rebase => "REBASE",
        };
        let (id, state) = self.pr_node(r).await?;
        // GitHub refuses auto-merge on a PR that is already mergeable ("clean status"): merge it now.
        let q = if state.as_deref() == Some("CLEAN") { MERGE_PR } else { ARM_AUTO_MERGE };
        graphql(&self.auth, &self.graphql, q, json!({ "id": id, "m": m })).await.map(|_| ())
    }

    async fn disarm_auto_merge(&self, r: &ReviewRef) -> Result<(), KeltaError> {
        let (id, _) = self.pr_node(r).await?;
        graphql(&self.auth, &self.graphql, DISARM_AUTO_MERGE, json!({ "id": id })).await.map(|_| ())
    }

    async fn set_draft(&self, r: &ReviewRef, draft: bool) -> Result<(), KeltaError> {
        let (id, _) = self.pr_node(r).await?;
        let q = if draft { TO_DRAFT } else { MARK_READY };
        graphql(&self.auth, &self.graphql, q, json!({ "id": id })).await.map(|_| ())
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
    /// Node id and `mergeStateStatus` of a pull request.
    async fn pr_node(&self, r: &ReviewRef) -> Result<(String, Option<String>), KeltaError> {
        let (owner, name) = r.repo.split_once('/').unwrap_or((r.repo.as_str(), ""));
        let vars = json!({ "o": owner, "n": name, "num": r.number });
        let data = graphql(&self.auth, &self.graphql, PR_ID, vars).await?;
        let state = data
            .pointer("/repository/pullRequest/mergeStateStatus")
            .and_then(Value::as_str)
            .map(str::to_owned);
        data.pointer("/repository/pullRequest/id")
            .and_then(Value::as_str)
            .map(|id| (id.to_owned(), state))
            .ok_or_else(|| KeltaError::not_found(format!("pull request {}#{}", r.repo, r.number)))
    }

    async fn pending_review(&self, r: &ReviewRef) -> Result<Option<Pending>, KeltaError> {
        let url = self.repo_url(&r.repo, &format!("/pulls/{}/reviews", r.number))?;
        let v = self.paged(HttpRequest::get(url).query("per_page", "100")).await?;
        Ok(pending_of(&v))
    }

    async fn publish(&self, r: &ReviewRef, p: &Pending, event: &str, body: &str) -> Result<(), KeltaError> {
        let url = self.repo_url(&r.repo, &format!("/pulls/{}/reviews/{}/events", r.number, p.id))?;
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn actions_logs_lose_timestamps_and_post_job_steps() {
        let log = "2024-05-01T10:00:00.1234567Z ##[group]Run cargo test\n\
                   2024-05-01T10:00:01.0000000Z test a ... FAILED\n\
                   2024-05-01T10:00:02.0000000Z ##[error]Process completed with exit code 101.\n\
                   2024-05-01T10:00:03.0000000Z Post job cleanup.\n\
                   2024-05-01T10:00:04.0000000Z Cleaning up orphan processes";
        assert_eq!(
            actions_log_tail(log),
            "##[group]Run cargo test\ntest a ... FAILED\n##[error]Process completed with exit code 101."
        );
    }

    #[test]
    fn another_reviewer_asked_after_their_review_keeps_me_off_the_blocking_band() {
        let n = json!({
            "timelineItems": { "nodes": [
                { "createdAt": "2026-09-28T08:00:00Z", "requestedReviewer": { "login": "louis" } },
                { "createdAt": "2026-09-29T08:00:00Z", "requestedReviewer": { "login": "bob" } },
            ] },
            "latestOpinionatedReviews": { "nodes": [
                { "state": "CHANGES_REQUESTED", "submittedAt": "2026-09-28T12:00:00Z", "author": { "login": "bob" } },
            ] },
        });
        let required = Some(ReviewDecision::ReviewRequired);
        assert_eq!(request_state(&n, "louis", required), (Some("2026-09-28T08:00:00Z".into()), false));
        assert!(!request_state(&n, "bob", required).1, "louis never reviewed: still pending");
        assert!(!request_state(&n, "louis", Some(ReviewDecision::Approved)).1);
    }

    #[test]
    fn my_pr_waits_on_asked_reviewers_and_on_changes_requested_before_my_push() {
        let n = json!({
            "reviewRequests": { "nodes": [
                { "requestedReviewer": { "login": "anna" } },
                { "requestedReviewer": { "combinedSlug": "acme/backend" } },
            ] },
            "timelineItems": { "nodes": [
                { "createdAt": "2026-09-28T08:00:00Z", "requestedReviewer": { "login": "anna" } },
                { "createdAt": "2026-09-29T08:00:00Z", "requestedReviewer": { "login": "anna" } },
            ] },
            "commits": { "nodes": [ { "commit": { "committedDate": "2026-09-30T08:00:00Z" } } ] },
            "latestOpinionatedReviews": { "nodes": [
                { "state": "CHANGES_REQUESTED", "commit": { "oid": "old" }, "author": { "login": "bob" } },
                { "state": "CHANGES_REQUESTED", "commit": { "oid": "head" }, "author": { "login": "carl" } },
                { "state": "APPROVED", "commit": { "oid": "old" }, "author": { "login": "dana" } },
            ] },
        });
        let (who, since) = waiting_state(&n, "head");
        assert_eq!(who, ["anna", "acme/backend", "bob"]);
        assert_eq!(
            since.as_deref(),
            Some("2026-09-29T08:00:00Z"),
            "anna's latest request is the oldest wait"
        );
        assert_eq!(waiting_state(&json!({}), "head"), (vec![], None));
    }

    #[test]
    fn a_later_comment_keeps_the_approval() {
        let n = json!({
            "author": { "login": "me" },
            "latestOpinionatedReviews": { "nodes": [ { "state": "APPROVED", "author": { "login": "bob" } } ] },
            "latestReviews": { "nodes": [
                { "state": "COMMENTED", "author": { "login": "bob" } },
                { "state": "COMMENTED", "author": { "login": "dan" } },
                { "state": "COMMENTED", "author": { "login": "me" } },
            ] },
        });
        let got: Vec<_> =
            latest_reviewers(&n).into_iter().map(|r| (r.user.login.unwrap_or_default(), r.state)).collect();
        assert_eq!(
            got,
            [("bob".into(), Some(MyReviewState::Approved)), ("dan".into(), Some(MyReviewState::Commented))]
        );
    }
}

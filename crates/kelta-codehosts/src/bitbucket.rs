//! Bitbucket Cloud code host: pull requests over REST 2.0 (`https://api.bitbucket.org/2.0`).
//!
//! Auth is an Atlassian API token (`Basic <email>:<token>`; app passwords were retired in 2026) or a
//! repository/workspace access token (`auth = "bearer"`). The cross-workspace endpoints are gone, so
//! lists walk `/user/workspaces`: my PRs come from `/workspaces/{ws}/pullrequests/{me}`, review
//! requests from `reviewers.uuid` queries on the workspace's recently updated repositories.
//! shortcut: per workspace only the 50 most recently updated repositories are scanned for review
//! requests and a list shows no CI (one extra request per PR); upgrade when either is missed.
//! shortcut: PR refs are not exposed, so "review locally" fetches the source branch (forks are
//! not supported).

use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use kelta_http::util::{percent_encode, trim_url, url_host};
use kelta_http::{AuthScheme, Authed, HttpCtx, HttpRequest, markdown};
use kelta_proto::api::{CodeHost, SecretResolver};
use kelta_proto::codehost::{
    CiCheck, CiState, CodeHostKind, FileChange, MyReviewState, PrCreate, PrState, Review, ReviewDecision,
    ReviewDetail, ReviewKind, ReviewQuery, ReviewRef, Reviewer,
};
use kelta_proto::error::{ErrorCode, KeltaError};
use kelta_proto::ids::AccountId;
use kelta_proto::settings::AccountConfig;
use kelta_proto::tracker::User;
use parking_lot::Mutex;
use serde_json::{Value, json};
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

use crate::common::{host_matches, linked_tickets, parse_remote, repo_path, s, user};

const STALE_AFTER_DAYS: i64 = 90;
const PAGELEN: &str = "50";
/// shortcut: at most this many pages per list, upgrade when a workspace has more.
const MAX_PAGES: u32 = 5;
const DEFAULT_WEB: &str = "https://bitbucket.org";

pub struct BitbucketHost {
    api: String,
    web_host: String,
    auth: Authed,
    me: Mutex<Option<User>>,
    /// Source branch of the reviews seen so far (fetch refspec of "review locally").
    branches: Mutex<HashMap<ReviewRef, String>>,
}

impl BitbucketHost {
    pub fn new(
        account: &AccountConfig,
        http: HttpCtx,
        secrets: Arc<dyn SecretResolver>,
    ) -> Result<Self, KeltaError> {
        let api = trim_url(&account.effective_base_url().unwrap_or_default());
        let scheme = AuthScheme::from_account(account).unwrap_or_else(|| AuthScheme::Basic {
            user: account.email.clone().or_else(|| account.user.clone()).unwrap_or_default(),
        });
        if matches!(&scheme, AuthScheme::Basic { user } if user.is_empty()) {
            return Err(KeltaError::invalid(
                "bitbucket account needs `email` (API token) or `auth = \"bearer\"` (access token)",
            ));
        }
        let auth = Authed::new(http, secrets, account.effective_secret(), Some(&api), scheme)
            .with_header("Accept", "application/json");
        let web = account.web_url.as_deref().unwrap_or(DEFAULT_WEB);
        Ok(Self {
            api,
            web_host: url_host(web).unwrap_or_default(),
            auth,
            me: Mutex::new(None),
            branches: Mutex::new(HashMap::new()),
        })
    }

    fn account(&self) -> &AccountId {
        self.auth.http().account_id()
    }

    async fn json(&self, req: HttpRequest) -> Result<kelta_http::HttpResponse<Value>, KeltaError> {
        self.auth.send_json::<Value>(req).await
    }

    fn pr_url(&self, repo: &str, tail: &str) -> Result<String, KeltaError> {
        Ok(format!("{}/repositories/{}/pullrequests{tail}", self.api, repo_path(repo)?))
    }

    /// `values` of a paged list, following `next` up to [`MAX_PAGES`].
    async fn paged(&self, req: HttpRequest) -> Result<Vec<Value>, KeltaError> {
        let mut out = Vec::new();
        let mut next = Some(req);
        for _ in 0..MAX_PAGES {
            let Some(req) = next.take() else { break };
            let body = self.json(req).await?.body;
            out.extend(body.get("values").and_then(Value::as_array).cloned().unwrap_or_default());
            next = s(&body, "next").map(HttpRequest::get);
        }
        Ok(out)
    }

    fn user_from(v: &Value) -> User {
        let login = s(v, "nickname").or_else(|| s(v, "display_name")).unwrap_or("ghost");
        let mut u =
            user(login, s(v, "display_name"), v.pointer("/links/avatar/href").and_then(Value::as_str));
        if let Some(id) = s(v, "uuid") {
            u.id = id.to_owned();
        }
        u
    }

    fn review_from(&self, v: &Value, kind: ReviewKind, me: Option<&str>) -> Option<Review> {
        let number = v.get("id").and_then(Value::as_u64)?;
        let repo = v.pointer("/destination/repository/full_name").and_then(Value::as_str)?.to_owned();
        let title = s(v, "title").unwrap_or("").to_owned();
        let branch = v.pointer("/source/branch/name").and_then(Value::as_str).unwrap_or("").to_owned();
        let r#ref = ReviewRef { account: self.account().clone(), repo, number };
        self.branches.lock().insert(r#ref.clone(), branch.clone());
        let parts: Vec<&Value> = v
            .get("participants")
            .and_then(Value::as_array)
            .map(|a| a.iter().filter(|p| s(p, "role") == Some("REVIEWER")).collect())
            .unwrap_or_default();
        let state_of = |p: &Value| match (s(p, "state"), p.get("approved").and_then(Value::as_bool)) {
            (Some("changes_requested"), _) => MyReviewState::ChangesRequested,
            (_, Some(true)) | (Some("approved"), _) => MyReviewState::Approved,
            _ => MyReviewState::Pending,
        };
        let decision = if parts.is_empty() {
            None
        } else if parts.iter().any(|p| state_of(p) == MyReviewState::ChangesRequested) {
            Some(ReviewDecision::ChangesRequested)
        } else if parts.iter().all(|p| state_of(p) == MyReviewState::Approved) {
            Some(ReviewDecision::Approved)
        } else {
            Some(ReviewDecision::ReviewRequired)
        };
        let mine = parts.iter().find(|p| p.pointer("/user/uuid").and_then(Value::as_str) == me);
        Some(Review {
            reviewed_head: None,
            r#ref,
            url: v.pointer("/links/html/href").and_then(Value::as_str).unwrap_or("").to_owned(),
            author: v.get("author").map(Self::user_from).unwrap_or_else(|| Self::user_from(&Value::Null)),
            draft: v.get("draft").and_then(Value::as_bool).unwrap_or(false),
            head_sha: v.pointer("/source/commit/hash").and_then(Value::as_str).unwrap_or("").to_owned(),
            source_branch: branch.clone(),
            target_branch: v
                .pointer("/destination/branch/name")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_owned(),
            ci: CiState::None,
            decision,
            decision_head: None,
            my_state: (kind == ReviewKind::ReviewRequested)
                .then(|| mine.map_or(MyReviewState::Pending, |p| state_of(p))),
            mergeable: None,
            labels: Vec::new(),
            kind,
            updated_at: s(v, "updated_on").unwrap_or("").to_owned(),
            linked_tickets: linked_tickets(&[&branch, &title]),
            additions: None,
            deletions: None,
            title,
        })
    }

    /// Workspace slugs of the current user.
    async fn workspaces(&self) -> Result<Vec<String>, KeltaError> {
        let rows = self
            .paged(HttpRequest::get(format!("{}/user/workspaces", self.api)).query("pagelen", PAGELEN))
            .await?;
        Ok(rows
            .iter()
            .filter_map(|w| w.pointer("/workspace/slug").and_then(Value::as_str).map(str::to_owned))
            .collect())
    }

    async fn review_requested(&self, ws: &str, me: &str) -> Result<Vec<Value>, KeltaError> {
        let cutoff = (OffsetDateTime::now_utc() - time::Duration::days(STALE_AFTER_DAYS))
            .replace_time(time::Time::MIDNIGHT);
        let repos = self
            .json(
                HttpRequest::get(format!("{}/repositories/{}", self.api, percent_encode(ws)))
                    .query("role", "member")
                    .query("sort", "-updated_on")
                    .query("q", format!("updated_on>={}", cutoff.format(&Rfc3339).unwrap_or_default()))
                    .query("pagelen", PAGELEN),
            )
            .await?
            .body;
        let q = format!("state=\"OPEN\" AND reviewers.uuid=\"{me}\"");
        let slugs: Vec<&str> = repos
            .get("values")
            .and_then(Value::as_array)
            .map(|a| a.iter().filter_map(|r| s(r, "full_name")).collect())
            .unwrap_or_default();
        let results = futures::future::join_all(slugs.into_iter().map(|full| {
            let q = q.clone();
            async move {
                let r = self
                    .pr_url(full, "")
                    .map(|u| HttpRequest::get(u).query("q", q).query("pagelen", PAGELEN));
                match r {
                    Ok(req) => self.paged(req).await,
                    Err(e) => Err(e),
                }
            }
        }))
        .await;
        let mut out = Vec::new();
        for r in results {
            match r {
                Ok(v) => out.extend(v),
                Err(e) if matches!(e.code, ErrorCode::NeedsAuth | ErrorCode::RateLimited) => return Err(e),
                Err(e) => tracing::debug!(code = %e.code, "bitbucket repository skipped"),
            }
        }
        Ok(out)
    }
}

fn ci_from(state: Option<&str>) -> CiState {
    match state {
        Some("SUCCESSFUL") => CiState::Success,
        Some("FAILED") => CiState::Failure,
        Some("STOPPED") => CiState::Error,
        Some("INPROGRESS") => CiState::Pending,
        _ => CiState::None,
    }
}

/// Worst state wins (failure > error > pending > success).
fn rollup(checks: &[CiCheck]) -> CiState {
    [CiState::Failure, CiState::Error, CiState::Pending, CiState::Success]
        .into_iter()
        .find(|s| checks.iter().any(|c| c.state == *s))
        .unwrap_or(CiState::None)
}

/// Escape a value placed inside a double-quoted `q` filter string.
fn q_str(v: &str) -> String {
    v.replace('\\', "\\\\").replace('"', "\\\"")
}

#[async_trait]
impl CodeHost for BitbucketHost {
    fn kind(&self) -> CodeHostKind {
        CodeHostKind::Bitbucket
    }

    async fn me(&self) -> Result<User, KeltaError> {
        if let Some(u) = self.me.lock().clone() {
            return Ok(u);
        }
        let v = self.json(HttpRequest::get(format!("{}/user", self.api))).await?.body;
        if s(&v, "uuid").is_none() {
            return Err(KeltaError::upstream("bitbucket /user returned no uuid"));
        }
        let u = Self::user_from(&v);
        *self.me.lock() = Some(u.clone());
        Ok(u)
    }

    async fn list_reviews(&self, q: &ReviewQuery) -> Result<Vec<Review>, KeltaError> {
        let me = self.me().await?;
        let mut rows = Vec::new();
        for ws in self.workspaces().await? {
            if q.kind == ReviewKind::Authored {
                let url = format!(
                    "{}/workspaces/{}/pullrequests/{}",
                    self.api,
                    percent_encode(&ws),
                    percent_encode(&me.id)
                );
                rows.extend(
                    self.paged(HttpRequest::get(url).query("state", "OPEN").query("pagelen", PAGELEN))
                        .await?,
                );
            } else {
                rows.extend(self.review_requested(&ws, &me.id).await?);
            }
        }
        let mut seen = Vec::new();
        Ok(rows
            .iter()
            .filter_map(|p| self.review_from(p, q.kind, Some(&me.id)))
            .filter(|r| q.include_drafts || !r.draft)
            .filter(|r| {
                let fresh = !seen.contains(&r.r#ref);
                seen.push(r.r#ref.clone());
                fresh
            })
            .collect())
    }

    async fn get(&self, r: &ReviewRef) -> Result<ReviewDetail, KeltaError> {
        let me = self.me().await?;
        let pr = |tail: &str| self.pr_url(&r.repo, &format!("/{}{tail}", r.number));
        let (pull, statuses, diffstat) = tokio::join!(
            self.json(HttpRequest::get(pr("")?)),
            self.paged(HttpRequest::get(pr("/statuses")?).query("pagelen", PAGELEN)),
            self.paged(HttpRequest::get(pr("/diffstat")?).query("pagelen", PAGELEN)),
        );
        let pull = pull?.body;
        let mine = pull.pointer("/author/uuid").and_then(Value::as_str) == Some(me.id.as_str());
        let kind = if mine { ReviewKind::Authored } else { ReviewKind::ReviewRequested };
        let mut review = self
            .review_from(&pull, kind, Some(&me.id))
            .ok_or_else(|| KeltaError::upstream("bitbucket pull request response without id"))?;
        review.r#ref = r.clone();

        // Statuses and diffstat are best effort (no CI configured, huge diffs).
        let checks: Vec<CiCheck> = statuses
            .unwrap_or_default()
            .iter()
            .map(|c| CiCheck {
                name: s(c, "name").or_else(|| s(c, "key")).unwrap_or("").to_owned(),
                state: ci_from(s(c, "state")),
                url: s(c, "url").map(str::to_owned),
            })
            .collect();
        review.ci = rollup(&checks);
        let files: Vec<FileChange> = diffstat
            .unwrap_or_default()
            .iter()
            .map(|f| FileChange {
                path: f
                    .pointer("/new/path")
                    .or_else(|| f.pointer("/old/path"))
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_owned(),
                additions: f.get("lines_added").and_then(Value::as_u64).unwrap_or(0) as u32,
                deletions: f.get("lines_removed").and_then(Value::as_u64).unwrap_or(0) as u32,
            })
            .collect();
        review.additions = Some(files.iter().map(|f| f.additions).sum());
        review.deletions = Some(files.iter().map(|f| f.deletions).sum());

        let reviewers: Vec<Reviewer> = pull
            .get("participants")
            .and_then(Value::as_array)
            .map(|a| {
                a.iter()
                    .filter(|p| s(p, "role") == Some("REVIEWER"))
                    .map(|p| Reviewer {
                        user: p
                            .get("user")
                            .map(Self::user_from)
                            .unwrap_or_else(|| Self::user_from(&Value::Null)),
                        state: Some(match (s(p, "state"), p.get("approved").and_then(Value::as_bool)) {
                            (Some("changes_requested"), _) => MyReviewState::ChangesRequested,
                            (_, Some(true)) => MyReviewState::Approved,
                            _ => MyReviewState::Pending,
                        }),
                    })
                    .collect()
            })
            .unwrap_or_default();
        let state = match s(&pull, "state") {
            Some("MERGED") => PrState::Merged,
            Some("DECLINED" | "SUPERSEDED") => PrState::Closed,
            _ => PrState::Open,
        };
        Ok(ReviewDetail {
            state,
            pending_comments: 0,
            body_html: markdown::to_html(s(&pull, "description").unwrap_or("")),
            review,
            reviewers,
            checks,
            files,
        })
    }

    async fn approve(&self, r: &ReviewRef, head_sha: &str) -> Result<(), KeltaError> {
        // The approve endpoint has no head guard: compare here (a small race remains).
        let pull = self.json(HttpRequest::get(self.pr_url(&r.repo, &format!("/{}", r.number))?)).await?.body;
        if pull.pointer("/source/commit/hash").and_then(Value::as_str) != Some(head_sha) {
            return Err(KeltaError::conflict("the pull request head changed; refresh and review again"));
        }
        self.auth
            .send_text(HttpRequest::post(self.pr_url(&r.repo, &format!("/{}/approve", r.number))?))
            .await?;
        Ok(())
    }

    async fn comment(&self, r: &ReviewRef, body: &str) -> Result<(), KeltaError> {
        let url = self.pr_url(&r.repo, &format!("/{}/comments", r.number))?;
        self.auth.send_text(HttpRequest::post(url).json(json!({"content": {"raw": body}}))).await?;
        Ok(())
    }

    async fn request_changes(&self, r: &ReviewRef, body: &str) -> Result<(), KeltaError> {
        self.comment(r, body).await?;
        let url = self.pr_url(&r.repo, &format!("/{}/request-changes", r.number))?;
        self.auth.send_text(HttpRequest::post(url)).await?;
        Ok(())
    }

    async fn create(&self, d: &PrCreate) -> Result<Review, KeltaError> {
        let body = json!({
            "title": d.title,
            "description": d.body,
            "draft": d.draft,
            "source": {"branch": {"name": d.head}},
            "destination": {"branch": {"name": d.base}},
        });
        let v = self.json(HttpRequest::post(self.pr_url(&d.repo, "")?).json(body)).await?.body;
        self.review_from(&v, ReviewKind::Authored, None)
            .ok_or_else(|| KeltaError::upstream("create pull request response without id"))
    }

    async fn find_for_branch(&self, repo: &str, branch: &str) -> Result<Option<Review>, KeltaError> {
        let q = format!("state=\"OPEN\" AND source.branch.name=\"{}\"", q_str(branch));
        let v = self
            .json(HttpRequest::get(self.pr_url(repo, "")?).query("q", q).query("pagelen", "1"))
            .await?
            .body;
        let me = self.me().await.ok().map(|u| u.id);
        Ok(v.pointer("/values/0").and_then(|p| {
            let mine = p.pointer("/author/uuid").and_then(Value::as_str) == me.as_deref();
            self.review_from(
                p,
                if mine { ReviewKind::Authored } else { ReviewKind::ReviewRequested },
                me.as_deref(),
            )
        }))
    }

    fn fetch_refspec(&self, r: &ReviewRef, local_branch: &str) -> String {
        match self.branches.lock().get(r) {
            Some(b) => format!("{b}:{local_branch}"),
            // Never listed in this process: the ref does not exist, the fetch reports it.
            None => format!("pull-requests/{}/from:{local_branch}", r.number),
        }
    }

    fn repo_from_remote(&self, url: &str) -> Option<String> {
        let (host, path) = parse_remote(url)?;
        (host_matches(&host, &self.web_host) && path.split('/').count() == 2).then_some(path)
    }
}

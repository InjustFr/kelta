//! Gitea / Forgejo code host: pull requests over REST v1 (`<base>/api/v1`).
//!
//! Lists: `GET /repos/issues/search?type=pulls` with `review_requested=true` / `created=true`
//! (issue-shaped rows, so each hit is expanded with `GET /repos/{o}/{r}/pulls/{n}`). Approve and
//! request changes are reviews (`commit_id` guards a moved head). Draft = `draft` or a `WIP:` title.
//! shortcut: no cheap "changed" gate (default `Ok(true)`), the poll scheduler's interval is the limit;
//! upgrade to `/notifications` when Gitea polling load matters.

use std::sync::Arc;

use async_trait::async_trait;
use kelta_http::util::{link_rel, trim_url, url_host};
use kelta_http::{AuthScheme, Authed, HttpCtx, HttpRequest, markdown};
use kelta_proto::api::{CodeHost, SecretResolver};
use kelta_proto::codehost::{
    CiCheck, CiState, CodeHostKind, FileChange, MyReviewState, PrCreate, PrState, Review, ReviewDecision,
    ReviewDetail, ReviewKind, ReviewQuery, ReviewRef, Reviewer,
};
use kelta_proto::error::{ErrorCode, KeltaError};
use kelta_proto::ids::AccountId;
use kelta_proto::settings::{AccountConfig, AuthKind};
use kelta_proto::tracker::User;
use parking_lot::Mutex;
use serde_json::{Value, json};
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

use crate::common::{host_matches, linked_tickets, parse_remote, repo_path, s, user};

/// Pull requests untouched for longer than this are not listed.
const STALE_AFTER_DAYS: i64 = 90;
const LIMIT: &str = "50";
/// shortcut: at most this many result pages per list, upgrade when someone has more open PRs.
const MAX_PAGES: u32 = 5;

pub struct GiteaHost {
    api: String,
    web_host: String,
    auth: Authed,
    me: Mutex<Option<User>>,
}

impl GiteaHost {
    pub fn new(
        account: &AccountConfig,
        http: HttpCtx,
        secrets: Arc<dyn SecretResolver>,
    ) -> Result<Self, KeltaError> {
        let base = account
            .base_url
            .as_deref()
            .map(trim_url)
            .filter(|u| !u.is_empty())
            .ok_or_else(|| KeltaError::invalid("gitea account needs a base_url"))?;
        let root = base.strip_suffix("/api/v1").unwrap_or(&base).to_owned();
        let scheme = match account.auth {
            Some(AuthKind::Basic) => AuthScheme::from_account(account).unwrap_or(AuthScheme::Bearer),
            _ => AuthScheme::Bearer,
        };
        let auth = Authed::new(http, secrets, account.effective_secret(), Some(&root), scheme)
            .with_header("Accept", "application/json");
        let web = account.web_url.as_deref().map(trim_url).unwrap_or_else(|| root.clone());
        Ok(Self {
            api: format!("{root}/api/v1"),
            web_host: url_host(&web).unwrap_or_default(),
            auth,
            me: Mutex::new(None),
        })
    }

    fn account(&self) -> &AccountId {
        self.auth.http().account_id()
    }

    async fn json(&self, req: HttpRequest) -> Result<kelta_http::HttpResponse<Value>, KeltaError> {
        self.auth.send_json::<Value>(req).await
    }

    fn pulls(&self, repo: &str, tail: &str) -> Result<String, KeltaError> {
        Ok(format!("{}/repos/{}/pulls{tail}", self.api, repo_path(repo)?))
    }

    /// Rows of a paged list, following `Link: rel="next"` up to [`MAX_PAGES`].
    async fn paged(&self, req: HttpRequest) -> Result<Vec<Value>, KeltaError> {
        let mut out = Vec::new();
        let mut next = Some(req);
        for _ in 0..MAX_PAGES {
            let Some(req) = next.take() else { break };
            let resp = self.json(req).await?;
            out.extend(resp.body.as_array().cloned().unwrap_or_default());
            next = link_rel(&resp.headers, "next").map(HttpRequest::get);
        }
        Ok(out)
    }

    fn user_from(v: &Value) -> User {
        match s(v, "login") {
            Some(l) => user(l, s(v, "full_name"), s(v, "avatar_url")),
            None => user("ghost", None, None),
        }
    }

    fn review_from(&self, v: &Value, kind: ReviewKind) -> Option<Review> {
        let number = v.get("number").and_then(Value::as_u64)?;
        let repo = v.pointer("/base/repo/full_name").and_then(Value::as_str)?.to_owned();
        let title = s(v, "title").unwrap_or("").to_owned();
        let branch = v.pointer("/head/ref").and_then(Value::as_str).unwrap_or("").to_owned();
        let upper = title.to_ascii_uppercase();
        let draft = v.get("draft").and_then(Value::as_bool).unwrap_or(false)
            || upper.starts_with("WIP:")
            || upper.starts_with("[WIP]");
        Some(Review {
            reviewed_head: None,
            r#ref: ReviewRef { account: self.account().clone(), repo, number },
            url: s(v, "html_url").unwrap_or("").to_owned(),
            author: v.get("user").map(Self::user_from).unwrap_or_else(|| Self::user_from(&Value::Null)),
            draft,
            head_sha: v.pointer("/head/sha").and_then(Value::as_str).unwrap_or("").to_owned(),
            source_branch: branch.clone(),
            target_branch: v.pointer("/base/ref").and_then(Value::as_str).unwrap_or("").to_owned(),
            ci: CiState::None,
            decision: None,
            decision_head: None,
            my_state: (kind == ReviewKind::ReviewRequested).then_some(MyReviewState::Pending),
            mergeable: v.get("mergeable").and_then(Value::as_bool),
            labels: v
                .get("labels")
                .and_then(Value::as_array)
                .map(|a| a.iter().filter_map(|l| s(l, "name").map(str::to_owned)).collect())
                .unwrap_or_default(),
            kind,
            updated_at: s(v, "updated_at").unwrap_or("").to_owned(),
            linked_tickets: linked_tickets(&[&branch, &title]),
            additions: v.get("additions").and_then(Value::as_u64).map(|n| n as u32),
            deletions: v.get("deletions").and_then(Value::as_u64).map(|n| n as u32),
            title,
        })
    }

    async fn pull(&self, repo: &str, number: u64, kind: ReviewKind) -> Result<Review, KeltaError> {
        let v = self.json(HttpRequest::get(self.pulls(repo, &format!("/{number}"))?)).await?.body;
        self.review_from(&v, kind)
            .ok_or_else(|| KeltaError::upstream("gitea pull request response without number"))
    }

    async fn review_post(&self, r: &ReviewRef, body: Value) -> Result<(), KeltaError> {
        let url = self.pulls(&r.repo, &format!("/{}/reviews", r.number))?;
        self.auth.send_text(HttpRequest::post(url).json(body)).await?;
        Ok(())
    }
}

fn ci_from(state: Option<&str>) -> CiState {
    match state {
        Some("success") => CiState::Success,
        Some("failure") => CiState::Failure,
        Some("error" | "warning") => CiState::Error,
        Some("pending") => CiState::Pending,
        _ => CiState::None,
    }
}

#[async_trait]
impl CodeHost for GiteaHost {
    fn kind(&self) -> CodeHostKind {
        CodeHostKind::Gitea
    }

    async fn me(&self) -> Result<User, KeltaError> {
        if let Some(u) = self.me.lock().clone() {
            return Ok(u);
        }
        let v = self.json(HttpRequest::get(format!("{}/user", self.api))).await?.body;
        if s(&v, "login").is_none() {
            return Err(KeltaError::upstream("gitea /user returned no login"));
        }
        let u = Self::user_from(&v);
        *self.me.lock() = Some(u.clone());
        Ok(u)
    }

    async fn list_reviews(&self, q: &ReviewQuery) -> Result<Vec<Review>, KeltaError> {
        let flag = if q.kind == ReviewKind::ReviewRequested { "review_requested" } else { "created" };
        // Day granularity keeps the URL (and so the cache entry) stable within a day.
        let cutoff = (OffsetDateTime::now_utc() - time::Duration::days(STALE_AFTER_DAYS))
            .replace_time(time::Time::MIDNIGHT);
        let rows = self
            .paged(
                HttpRequest::get(format!("{}/repos/issues/search", self.api))
                    .query("type", "pulls")
                    .query("state", "open")
                    .query(flag, "true")
                    .query("since", cutoff.format(&Rfc3339).unwrap_or_default())
                    .query("limit", LIMIT)
                    .query("page", "1"),
            )
            .await?;
        // Search rows carry no branches or head sha: expand each one (a vanished repo is skipped).
        let found = rows.iter().filter_map(|i| {
            let repo = i.pointer("/repository/full_name").and_then(Value::as_str)?;
            Some((repo.to_owned(), i.get("number").and_then(Value::as_u64)?))
        });
        let pulls = futures::future::join_all(found.map(|(repo, n)| async move {
            let r = self.pull(&repo, n, q.kind).await;
            (repo, n, r)
        }))
        .await;
        let mut out = Vec::new();
        for (repo, n, r) in pulls {
            match r {
                Ok(r) if q.include_drafts || !r.draft => out.push(r),
                Ok(_) => {}
                Err(e) if matches!(e.code, ErrorCode::NeedsAuth | ErrorCode::RateLimited) => return Err(e),
                Err(e) => tracing::debug!(%repo, n, code = %e.code, "gitea pull request skipped"),
            }
        }
        Ok(out)
    }

    async fn get(&self, r: &ReviewRef) -> Result<ReviewDetail, KeltaError> {
        let me = self.me().await?;
        let (pull, reviews, files) = tokio::join!(
            self.json(HttpRequest::get(self.pulls(&r.repo, &format!("/{}", r.number))?)),
            self.json(
                HttpRequest::get(self.pulls(&r.repo, &format!("/{}/reviews", r.number))?)
                    .query("limit", LIMIT)
            ),
            self.json(
                HttpRequest::get(self.pulls(&r.repo, &format!("/{}/files", r.number))?).query("limit", "100")
            ),
        );
        let pull = pull?.body;
        let mine = pull.pointer("/user/login").and_then(Value::as_str) == me.login.as_deref();
        let kind = if mine { ReviewKind::Authored } else { ReviewKind::ReviewRequested };
        let mut review = self
            .review_from(&pull, kind)
            .ok_or_else(|| KeltaError::upstream("gitea pull request response without number"))?;
        review.r#ref = r.clone();

        // Latest meaningful state per reviewer; a plain COMMENT never overrides an approval or a veto.
        let mut states: Vec<(User, MyReviewState)> = Vec::new();
        for rv in reviews?.body.as_array().map(Vec::as_slice).unwrap_or(&[]) {
            let st = match s(rv, "state") {
                Some("APPROVED") => MyReviewState::Approved,
                Some("REQUEST_CHANGES") => MyReviewState::ChangesRequested,
                Some("COMMENT") => MyReviewState::Commented,
                _ => continue,
            };
            if rv.get("dismissed").and_then(Value::as_bool) == Some(true) {
                continue;
            }
            let u = rv.get("user").map(Self::user_from).unwrap_or_else(|| Self::user_from(&Value::Null));
            match states.iter_mut().find(|(x, _)| x.id == u.id) {
                Some((_, old)) if st == MyReviewState::Commented && *old != MyReviewState::Commented => {}
                Some((_, old)) => *old = st,
                None => states.push((u, st)),
            }
        }
        let mut reviewers: Vec<Reviewer> =
            states.iter().map(|(u, st)| Reviewer { user: u.clone(), state: Some(*st) }).collect();
        for u in pull.get("requested_reviewers").and_then(Value::as_array).map(Vec::as_slice).unwrap_or(&[]) {
            let u = Self::user_from(u);
            if !reviewers.iter().any(|x| x.user.id == u.id) {
                reviewers.push(Reviewer { user: u, state: Some(MyReviewState::Pending) });
            }
        }
        let has = |st| states.iter().any(|(_, x)| *x == st);
        review.decision = Some(if has(MyReviewState::ChangesRequested) {
            ReviewDecision::ChangesRequested
        } else if has(MyReviewState::Approved) {
            ReviewDecision::Approved
        } else {
            ReviewDecision::ReviewRequired
        });
        if kind == ReviewKind::ReviewRequested {
            review.my_state = Some(
                states
                    .iter()
                    .find(|(u, _)| u.id == me.id)
                    .map(|(_, st)| *st)
                    .unwrap_or(MyReviewState::Pending),
            );
        }

        // CI and the file list are best effort (no CI configured, huge diffs).
        let mut checks = Vec::new();
        if !review.head_sha.is_empty() {
            let st = self
                .json(HttpRequest::get(format!(
                    "{}/repos/{}/commits/{}/status",
                    self.api,
                    repo_path(&r.repo)?,
                    kelta_http::util::percent_encode(&review.head_sha)
                )))
                .await
                .map(|x| x.body)
                .unwrap_or(Value::Null);
            review.ci =
                ci_from(s(&st, "state").filter(|_| st.get("total_count").and_then(Value::as_u64) != Some(0)));
            for c in st.get("statuses").and_then(Value::as_array).map(Vec::as_slice).unwrap_or(&[]) {
                checks.push(CiCheck {
                    name: s(c, "context").unwrap_or("").to_owned(),
                    state: ci_from(s(c, "status")),
                    url: s(c, "target_url").filter(|u| !u.is_empty()).map(str::to_owned),
                });
            }
        }
        let files: Vec<FileChange> = files
            .map(|f| f.body)
            .unwrap_or(Value::Null)
            .as_array()
            .map(|a| {
                a.iter()
                    .map(|f| FileChange {
                        path: s(f, "filename").unwrap_or("").to_owned(),
                        additions: f.get("additions").and_then(Value::as_u64).unwrap_or(0) as u32,
                        deletions: f.get("deletions").and_then(Value::as_u64).unwrap_or(0) as u32,
                    })
                    .collect()
            })
            .unwrap_or_default();
        let state = match (pull.get("merged").and_then(Value::as_bool), s(&pull, "state")) {
            (Some(true), _) => PrState::Merged,
            (_, Some("closed")) => PrState::Closed,
            _ => PrState::Open,
        };
        Ok(ReviewDetail {
            state,
            pending_comments: 0,
            body_html: markdown::to_html(s(&pull, "body").unwrap_or("")),
            review,
            reviewers,
            checks,
            files,
        })
    }

    async fn approve(&self, r: &ReviewRef, head_sha: &str) -> Result<(), KeltaError> {
        // The server only marks a mismatched `commit_id` review stale and still records it: check here.
        let pull = self.json(HttpRequest::get(self.pulls(&r.repo, &format!("/{}", r.number))?)).await?.body;
        if pull.pointer("/head/sha").and_then(Value::as_str) != Some(head_sha) {
            return Err(KeltaError::conflict("the pull request head changed; refresh and review again"));
        }
        self.review_post(r, json!({"event": "APPROVED", "commit_id": head_sha})).await
    }

    async fn comment(&self, r: &ReviewRef, body: &str) -> Result<(), KeltaError> {
        let url = format!("{}/repos/{}/issues/{}/comments", self.api, repo_path(&r.repo)?, r.number);
        self.auth.send_text(HttpRequest::post(url).json(json!({"body": body}))).await?;
        Ok(())
    }

    async fn request_changes(&self, r: &ReviewRef, body: &str) -> Result<(), KeltaError> {
        self.review_post(r, json!({"event": "REQUEST_CHANGES", "body": body})).await
    }

    async fn create(&self, d: &PrCreate) -> Result<Review, KeltaError> {
        let title = if d.draft && !d.title.to_ascii_uppercase().starts_with("WIP:") {
            format!("WIP: {}", d.title)
        } else {
            d.title.clone()
        };
        let body = json!({"head": d.head, "base": d.base, "title": title, "body": d.body});
        let v = self.json(HttpRequest::post(self.pulls(&d.repo, "")?).json(body)).await?.body;
        self.review_from(&v, ReviewKind::Authored)
            .ok_or_else(|| KeltaError::upstream("create pull request response without number"))
    }

    async fn find_for_branch(&self, repo: &str, branch: &str) -> Result<Option<Review>, KeltaError> {
        // The list has no head filter: scan the open PRs (bounded by MAX_PAGES).
        let rows = self
            .paged(HttpRequest::get(self.pulls(repo, "")?).query("state", "open").query("limit", LIMIT))
            .await?;
        let me = self.me().await.ok().and_then(|u| u.login);
        Ok(rows.iter().find(|p| p.pointer("/head/ref").and_then(Value::as_str) == Some(branch)).and_then(
            |p| {
                let mine = p.pointer("/user/login").and_then(Value::as_str) == me.as_deref();
                self.review_from(p, if mine { ReviewKind::Authored } else { ReviewKind::ReviewRequested })
            },
        ))
    }

    fn fetch_refspec(&self, r: &ReviewRef, local_branch: &str) -> String {
        format!("pull/{}/head:{local_branch}", r.number)
    }

    fn repo_from_remote(&self, url: &str) -> Option<String> {
        let (host, path) = parse_remote(url)?;
        (host_matches(&host, &self.web_host) && path.split('/').count() == 2).then_some(path)
    }
}

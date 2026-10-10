//! GitLab (gitlab.com and self-managed) code host: merge requests over REST v4.
//!
//! Lists: `reviewer_username=<me>` / `scope=created_by_me`, bounded by `updated_after`, `draft=`
//! on GitLab >= 16 and `wip=` before (`GET /version`). Approve sends the head `sha` (409 →
//! `Conflict`). Request changes is a note (+ unapprove). Gate: pending `review_requested` todos
//! (count + max id).

use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use kelta_http::util::{percent_encode, trim_url, url_host};
use kelta_http::{AuthScheme, Authed, HttpCtx, HttpRequest, markdown};
use kelta_proto::api::{CodeHost, SecretResolver};
use kelta_proto::codehost::{
    CiCheck, CiState, CodeHostKind, FailedCheck, Feedback, FeedbackThread, FileChange, MyReviewState,
    PrCreate, PrState, Review, ReviewDecision, ReviewDetail, ReviewKind, ReviewQuery, ReviewRef, Reviewer,
};
use kelta_proto::error::{ErrorCode, KeltaError};
use kelta_proto::ids::AccountId;
use kelta_proto::settings::{AccountConfig, AuthKind};
use kelta_proto::tracker::User;
use parking_lot::Mutex;
use serde_json::{Value, json};
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

use crate::common::{MAX_LOGS, feedback_error, host_matches, linked_tickets, log_tail, parse_remote, s};

/// Merge requests untouched for longer than this are not listed.
const STALE_AFTER_DAYS: i64 = 90;
const PER_PAGE: &str = "100";

/// `(repo, iid, head sha)`.
type DiffKey = (String, u64, String);

#[derive(Default)]
struct TodoGate {
    disabled: bool,
    last: Option<(usize, u64)>,
    /// A change was seen but `list_reviews` has not yet succeeded since.
    pending: bool,
}

pub struct GitlabHost {
    api: String,
    web: String,
    web_host: String,
    auth: Authed,
    me: Mutex<Option<User>>,
    /// Major version from `GET /version` (cached; `None` = not fetched or unknown).
    major: Mutex<Option<u32>>,
    gate: Mutex<TodoGate>,
    /// Requested MRs' `(additions, deletions)` by `(repo, iid, head sha)`: a diff only changes
    /// with its head, so `/changes` (the full diff) is fetched once per head, not every poll.
    diff_sizes: Mutex<HashMap<DiffKey, (u32, u32)>>,
}

impl GitlabHost {
    pub fn new(
        account: &AccountConfig,
        http: HttpCtx,
        secrets: Arc<dyn SecretResolver>,
    ) -> Result<Self, KeltaError> {
        let base = account
            .effective_base_url()
            .map(|u| trim_url(&u))
            .ok_or_else(|| KeltaError::invalid("gitlab account needs a base_url"))?;
        let root = base.strip_suffix("/api/v4").unwrap_or(&base).to_owned();
        let scheme = match account.auth {
            Some(AuthKind::Bearer | AuthKind::Basic | AuthKind::Oauth) => {
                AuthScheme::from_account(account).unwrap_or(AuthScheme::Bearer)
            }
            _ => AuthScheme::Header("PRIVATE-TOKEN".into()),
        };
        let auth = Authed::new(http, secrets, account.effective_secret(), Some(&root), scheme)
            .with_header("Accept", "application/json");
        let web = account.web_url.as_deref().map(trim_url).unwrap_or_else(|| root.clone());
        Ok(Self {
            api: format!("{root}/api/v4"),
            web_host: url_host(&web).unwrap_or_default(),
            web,
            auth,
            me: Mutex::new(None),
            major: Mutex::new(None),
            gate: Mutex::new(TodoGate::default()),
            diff_sizes: Mutex::new(HashMap::new()),
        })
    }

    fn account(&self) -> &AccountId {
        self.auth.http().account_id()
    }

    async fn json(&self, req: HttpRequest) -> Result<kelta_http::HttpResponse<Value>, KeltaError> {
        self.auth.send_json::<Value>(req).await
    }

    fn mr_url(&self, repo: &str, tail: &str) -> String {
        format!("{}/projects/{}/merge_requests{tail}", self.api, percent_encode(repo))
    }

    fn user_from(v: &Value) -> User {
        match s(v, "username") {
            Some(l) => {
                let id = v
                    .get("id")
                    .and_then(Value::as_u64)
                    .map(|i| i.to_string())
                    .unwrap_or_else(|| l.to_owned());
                User {
                    id,
                    name: s(v, "name").filter(|n| !n.is_empty()).unwrap_or(l).to_owned(),
                    login: Some(l.to_owned()),
                    avatar_url: s(v, "avatar_url").map(str::to_owned),
                }
            }
            None => User {
                id: "ghost".into(),
                name: "ghost".into(),
                login: Some("ghost".into()),
                avatar_url: None,
            },
        }
    }

    /// Server major version (cached); a failure means "recent" (`draft=`).
    async fn major_version(&self) -> u32 {
        if let Some(m) = *self.major.lock() {
            return m;
        }
        let major = match self.json(HttpRequest::get(format!("{}/version", self.api))).await {
            Ok(r) => {
                s(&r.body, "version").and_then(|v| v.split('.').next()).and_then(|m| m.parse::<u32>().ok())
            }
            Err(_) => None,
        };
        if let Some(m) = major {
            *self.major.lock() = Some(m);
        }
        major.unwrap_or(16)
    }

    /// `grp/proj` of a merge request (from `references.full` or `web_url`).
    fn repo_of(&self, v: &Value) -> Option<String> {
        if let Some(full) = v.pointer("/references/full").and_then(Value::as_str)
            && let Some((repo, _)) = full.rsplit_once('!')
            && !repo.is_empty()
        {
            return Some(repo.to_owned());
        }
        let url = s(v, "web_url")?;
        let path = url.strip_prefix(&self.web)?.trim_start_matches('/');
        path.split_once("/-/merge_requests").map(|(p, _)| p.to_owned())
    }

    fn review_from(&self, v: &Value, repo_hint: Option<&str>, kind: ReviewKind) -> Option<Review> {
        let number = v.get("iid").and_then(Value::as_u64)?;
        let repo = self.repo_of(v).or_else(|| repo_hint.map(str::to_owned))?;
        let title = s(v, "title").unwrap_or("").to_owned();
        let branch = s(v, "source_branch").unwrap_or("").to_owned();
        let merge_status = s(v, "detailed_merge_status");
        let draft = v
            .get("draft")
            .and_then(Value::as_bool)
            .or_else(|| v.get("work_in_progress").and_then(Value::as_bool))
            .unwrap_or(false);
        let pipeline = v.get("head_pipeline").filter(|p| !p.is_null()).or_else(|| v.get("pipeline"));
        // GitLab has no "changes requested" review state before 17: unresolved blocking threads
        // are the reviewer's way to ask for changes (FLOW §2.2 notes).
        let blocked = v.get("blocking_discussions_resolved").and_then(Value::as_bool) == Some(false)
            || merge_status == Some("discussions_not_resolved");
        let head_sha = s(v, "sha").unwrap_or("").to_owned();
        Some(Review {
            r#ref: ReviewRef { account: self.account().clone(), repo, number },
            url: s(v, "web_url").unwrap_or("").to_owned(),
            author: v.get("author").map(Self::user_from).unwrap_or_else(|| Self::user_from(&Value::Null)),
            draft,
            decision_head: blocked.then(|| head_sha.clone()),
            head_sha,
            source_branch: branch.clone(),
            target_branch: s(v, "target_branch").unwrap_or("").to_owned(),
            ci: ci_from(pipeline.and_then(|p| s(p, "status"))),
            decision: if blocked {
                Some(ReviewDecision::ChangesRequested)
            } else {
                (merge_status == Some("not_approved")).then_some(ReviewDecision::ReviewRequired)
            },
            my_state: (kind == ReviewKind::ReviewRequested).then_some(MyReviewState::Pending),
            // shortcut: GitLab REST keeps no commit per approval, so no "updated since your review"; upgrade via GraphQL reviewer states.
            reviewed_head: None,
            mergeable: match (merge_status, v.get("has_conflicts").and_then(Value::as_bool)) {
                (_, Some(true)) | (Some("conflict"), _) => Some(false),
                (Some("mergeable"), _) => Some(true),
                _ => None,
            },
            labels: v
                .get("labels")
                .and_then(Value::as_array)
                .map(|a| a.iter().filter_map(|l| l.as_str().map(str::to_owned)).collect())
                .unwrap_or_default(),
            kind,
            updated_at: s(v, "updated_at").unwrap_or("").to_owned(),
            linked_tickets: linked_tickets(&[&branch, &title]),
            additions: None,
            deletions: None,
            requested_at: None,
            blocking: false,
            title,
        })
    }

    /// Review requests: when I was asked (`/reviewers`), whether mine is the last approval
    /// missing (`/approvals`: one left and I am an approver) and the diff size (`/changes`, the
    /// list has none). Best effort, like the detail.
    async fn request_state(&self, review: &mut Review, me: &User) {
        let url = self.mr_url(&review.r#ref.repo, &format!("/{}", review.r#ref.number));
        let key = (review.r#ref.repo.clone(), review.r#ref.number, review.head_sha.clone());
        let cached = self.diff_sizes.lock().get(&key).copied();
        let (reviewers, approvals, changes) = tokio::join!(
            self.json(HttpRequest::get(format!("{url}/reviewers"))),
            self.json(HttpRequest::get(format!("{url}/approvals"))),
            async {
                match cached {
                    Some(_) => None,
                    None => self.json(HttpRequest::get(format!("{url}/changes"))).await.ok(),
                }
            },
        );
        // `approvers` / `approved_by` / `/reviewers` wrap the user, `suggested_approvers` does not.
        let is_me = |x: &Value| {
            x.get("user").unwrap_or(x).get("id").and_then(Value::as_u64).map(|i| i.to_string())
                == Some(me.id.clone())
        };
        let list = |v: &Value, k: &str| v.get(k).and_then(Value::as_array).cloned().unwrap_or_default();
        if let Ok(r) = reviewers {
            review.requested_at = r
                .body
                .as_array()
                .and_then(|a| a.iter().find(|x| is_me(x)))
                .and_then(|x| s(x, "created_at"))
                .map(str::to_owned);
        }
        if let Ok(a) = approvals {
            let a = a.body;
            review.blocking = a.get("approvals_left").and_then(Value::as_u64) == Some(1)
                && !list(&a, "approved_by").iter().any(is_me)
                && (list(&a, "approvers").iter().any(is_me)
                    || list(&a, "suggested_approvers").iter().any(is_me));
        }
        // shortcut: GitLab truncates huge diffs (`overflow`), so a very large MR reads smaller; upgrade via `/diffs` paging if it matters.
        let size = cached.or_else(|| {
            let files = changes?.body.get("changes")?.as_array()?.clone();
            Some(
                files
                    .iter()
                    .map(|c| diff_counts(s(c, "diff").unwrap_or("")))
                    .fold((0, 0), |x, y| (x.0 + y.0, x.1 + y.1)),
            )
        });
        if let Some((a, d)) = size {
            if !key.2.is_empty() {
                self.diff_sizes.lock().insert(key, (a, d));
            }
            (review.additions, review.deletions) = (Some(a), Some(d));
        }
    }

    fn project_url(&self, repo: &str, tail: &str) -> String {
        format!("{}/projects/{}{tail}", self.api, percent_encode(repo))
    }

    /// Jobs of the MR's head pipeline (empty without one).
    async fn pipeline_jobs(&self, repo: &str, mr: &Value) -> Vec<Value> {
        let Some(pid) = mr.pointer("/head_pipeline/id").and_then(Value::as_u64) else { return Vec::new() };
        let req = HttpRequest::get(self.project_url(repo, &format!("/pipelines/{pid}/jobs")))
            .query("per_page", PER_PAGE);
        self.json(req).await.ok().and_then(|j| j.body.as_array().cloned()).unwrap_or_default()
    }

    async fn list_query(
        &self,
        kind: ReviewKind,
        include_drafts: bool,
    ) -> Result<Vec<(String, String)>, KeltaError> {
        let mut q: Vec<(String, String)> = vec![("state".into(), "opened".into())];
        match kind {
            ReviewKind::ReviewRequested => {
                let me = self.me().await?;
                q.push(("scope".into(), "all".into()));
                q.push(("reviewer_username".into(), me.login.unwrap_or(me.id)));
            }
            ReviewKind::Authored => q.push(("scope".into(), "created_by_me".into())),
        }
        if !include_drafts {
            let key = if self.major_version().await >= 16 { "draft" } else { "wip" };
            q.push((key.into(), "no".into()));
        }
        // Day granularity keeps the URL (and so the ETag entry) stable within a day.
        let cutoff = (OffsetDateTime::now_utc() - time::Duration::days(STALE_AFTER_DAYS))
            .replace_time(time::Time::MIDNIGHT);
        q.push(("updated_after".into(), cutoff.format(&Rfc3339).unwrap_or_default()));
        q.push(("order_by".into(), "updated_at".into()));
        q.push(("sort".into(), "desc".into()));
        q.push(("per_page".into(), PER_PAGE.into()));
        Ok(q)
    }
}

fn ci_from(status: Option<&str>) -> CiState {
    match status {
        Some("success") => CiState::Success,
        Some("failed") => CiState::Failure,
        Some("canceled" | "canceling" | "skipped") => CiState::Error,
        Some(
            "running"
            | "pending"
            | "created"
            | "waiting_for_resource"
            | "preparing"
            | "scheduled"
            | "manual"
            | "waiting_for_callback",
        ) => CiState::Pending,
        _ => CiState::None,
    }
}

/// `+`/`-` line counts of a unified diff (headers excluded).
fn diff_counts(diff: &str) -> (u32, u32) {
    let (mut a, mut d) = (0, 0);
    for l in diff.lines() {
        if l.starts_with('+') && !l.starts_with("+++") {
            a += 1;
        } else if l.starts_with('-') && !l.starts_with("---") {
            d += 1;
        }
    }
    (a, d)
}

#[async_trait]
impl CodeHost for GitlabHost {
    fn kind(&self) -> CodeHostKind {
        CodeHostKind::Gitlab
    }

    async fn me(&self) -> Result<User, KeltaError> {
        if let Some(u) = self.me.lock().clone() {
            return Ok(u);
        }
        let v = self.json(HttpRequest::get(format!("{}/user", self.api))).await?.body;
        if s(&v, "username").is_none() {
            return Err(KeltaError::upstream("gitlab /user returned no username"));
        }
        let u = Self::user_from(&v);
        *self.me.lock() = Some(u.clone());
        Ok(u)
    }

    async fn changed_since_last(&self) -> Result<bool, KeltaError> {
        if self.gate.lock().disabled {
            return Ok(true);
        }
        let req = HttpRequest::get(format!("{}/todos", self.api))
            .query("state", "pending")
            .query("action", "review_requested")
            .query("per_page", PER_PAGE);
        let resp = match self.json(req).await {
            Ok(r) => r,
            Err(e) if matches!(e.code, ErrorCode::PermissionDenied | ErrorCode::NotFound) => {
                self.gate.lock().disabled = true;
                return Ok(true);
            }
            Err(e) => return Err(e),
        };
        let items = resp.body.as_array().map(Vec::as_slice).unwrap_or(&[]);
        let count = resp.headers.get("x-total").and_then(|v| v.parse::<usize>().ok()).unwrap_or(items.len());
        let max_id = items.iter().filter_map(|t| t.get("id").and_then(Value::as_u64)).max().unwrap_or(0);
        let mut g = self.gate.lock();
        g.pending |= g.last != Some((count, max_id));
        g.last = Some((count, max_id));
        Ok(g.pending)
    }

    async fn list_reviews(&self, q: &ReviewQuery) -> Result<Vec<Review>, KeltaError> {
        let mut req = HttpRequest::get(format!("{}/merge_requests", self.api));
        for (k, v) in self.list_query(q.kind, q.include_drafts).await? {
            req = req.query(k, v);
        }
        let resp = self.json(req.with_etag()).await?;
        self.gate.lock().pending = false;
        let mut list: Vec<Review> = resp
            .body
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|m| self.review_from(m, None, q.kind))
                    .filter(|r| q.include_drafts || !r.draft)
                    .collect()
            })
            .unwrap_or_default();
        if q.kind == ReviewKind::ReviewRequested {
            let me = self.me().await?;
            futures::future::join_all(list.iter_mut().map(|r| self.request_state(r, &me))).await;
            // Bounded by the current list: closed or re-pushed MRs drop out.
            self.diff_sizes.lock().retain(|(repo, n, sha), _| {
                list.iter().any(|r| r.r#ref.repo == *repo && r.r#ref.number == *n && r.head_sha == *sha)
            });
        }
        Ok(list)
    }

    async fn get(&self, r: &ReviewRef) -> Result<ReviewDetail, KeltaError> {
        let url = self.mr_url(&r.repo, &format!("/{}", r.number));
        let me = self.me().await?;
        let (mr, approvals, changes, states, drafts) = tokio::join!(
            self.json(HttpRequest::get(&url)),
            self.json(HttpRequest::get(format!("{url}/approvals"))),
            self.json(HttpRequest::get(format!("{url}/changes"))),
            self.json(HttpRequest::get(format!("{url}/reviewers"))),
            self.draft_count(r),
        );
        let mr = mr?.body;
        // Approvals, changes and reviewer states are best effort (rules / tiers / versions differ).
        let approvals = approvals.map(|a| a.body).unwrap_or(Value::Null);
        let changes = changes.map(|c| c.body).unwrap_or(Value::Null);
        // GitLab 17+: `{user, state: "requested_changes" | "reviewed" | "unreviewed" | …}`.
        let requested_changes: Vec<String> = states
            .ok()
            .and_then(|r| r.body.as_array().cloned())
            .unwrap_or_default()
            .iter()
            .filter(|x| s(x, "state") == Some("requested_changes"))
            .filter_map(|x| x.pointer("/user/id").and_then(Value::as_u64).map(|i| i.to_string()))
            .collect();
        let mine =
            mr.pointer("/author/id").and_then(Value::as_u64).map(|i| i.to_string()) == Some(me.id.clone());
        let kind = if mine { ReviewKind::Authored } else { ReviewKind::ReviewRequested };
        let mut review = self
            .review_from(&mr, Some(&r.repo), kind)
            .ok_or_else(|| KeltaError::upstream("merge request response without iid"))?;
        review.r#ref = r.clone();

        let approved_by: Vec<String> = approvals
            .get("approved_by")
            .and_then(Value::as_array)
            .map(|a| {
                a.iter()
                    .filter_map(|x| x.pointer("/user/id").and_then(Value::as_u64).map(|i| i.to_string()))
                    .collect()
            })
            .unwrap_or_default();
        if !requested_changes.is_empty() {
            review.decision = Some(ReviewDecision::ChangesRequested);
            review.decision_head = Some(review.head_sha.clone());
        } else if approvals.is_object() && review.decision != Some(ReviewDecision::ChangesRequested) {
            review.decision = Some(if approvals.get("approved").and_then(Value::as_bool) == Some(true) {
                ReviewDecision::Approved
            } else {
                ReviewDecision::ReviewRequired
            });
        }
        let reviewers: Vec<Reviewer> = mr
            .get("reviewers")
            .and_then(Value::as_array)
            .map(|a| {
                a.iter()
                    .map(|u| {
                        let user = Self::user_from(u);
                        let st = if requested_changes.contains(&user.id) {
                            MyReviewState::ChangesRequested
                        } else if approved_by.contains(&user.id) {
                            MyReviewState::Approved
                        } else {
                            MyReviewState::Pending
                        };
                        Reviewer { user, state: Some(st) }
                    })
                    .collect()
            })
            .unwrap_or_default();
        review.my_state = match kind {
            ReviewKind::Authored => None,
            ReviewKind::ReviewRequested => Some(if approved_by.contains(&me.id) {
                MyReviewState::Approved
            } else {
                MyReviewState::Pending
            }),
        };

        let (mut total_add, mut total_del) = (0u32, 0u32);
        let files: Vec<FileChange> = changes
            .get("changes")
            .and_then(Value::as_array)
            .map(|a| {
                a.iter()
                    .map(|c| {
                        let (add, del) = diff_counts(s(c, "diff").unwrap_or(""));
                        total_add += add;
                        total_del += del;
                        FileChange {
                            path: s(c, "new_path").or_else(|| s(c, "old_path")).unwrap_or("").to_owned(),
                            additions: add,
                            deletions: del,
                        }
                    })
                    .collect()
            })
            .unwrap_or_default();
        if changes.is_object() {
            review.additions = Some(total_add);
            review.deletions = Some(total_del);
        }

        let checks: Vec<CiCheck> = self
            .pipeline_jobs(&r.repo, &mr)
            .await
            .iter()
            .map(|j| CiCheck {
                name: s(j, "name").unwrap_or("").to_owned(),
                state: ci_from(s(j, "status")),
                url: s(j, "web_url").map(str::to_owned),
            })
            .collect();
        let state = match s(&mr, "state") {
            Some("merged") => PrState::Merged,
            Some("closed") => PrState::Closed,
            _ => PrState::Open,
        };
        Ok(ReviewDetail {
            body_html: markdown::to_html(s(&mr, "description").unwrap_or("")),
            review,
            state,
            reviewers,
            checks,
            files,
            pending_comments: drafts.unwrap_or(0),
        })
    }

    async fn approve(&self, r: &ReviewRef, head_sha: &str) -> Result<(), KeltaError> {
        self.publish_drafts(r).await?;
        let url = self.mr_url(&r.repo, &format!("/{}/approve", r.number));
        // 409 (head moved: sha mismatch) maps to `Conflict` in the shared status mapping.
        self.auth.send_text(HttpRequest::post(url).json(json!({"sha": head_sha}))).await?;
        Ok(())
    }

    async fn comment(&self, r: &ReviewRef, body: &str) -> Result<(), KeltaError> {
        let published = self.publish_drafts(r).await?;
        if published > 0 && body.trim().is_empty() {
            return Ok(());
        }
        let url = self.mr_url(&r.repo, &format!("/{}/notes", r.number));
        self.auth.send_text(HttpRequest::post(url).json(json!({"body": body}))).await?;
        Ok(())
    }

    async fn request_changes(&self, r: &ReviewRef, body: &str) -> Result<(), KeltaError> {
        // GitLab REST has no "request changes" state: leave a note and withdraw our approval.
        self.comment(r, body).await?;
        let url = self.mr_url(&r.repo, &format!("/{}/unapprove", r.number));
        if let Err(e) = self.auth.send_text(HttpRequest::post(url)).await {
            if e.code == ErrorCode::NeedsAuth {
                return Err(e);
            }
            tracing::debug!(code = %e.code, "unapprove after request-changes note failed (nothing to withdraw?)");
        }
        Ok(())
    }

    async fn add_pending_comment(
        &self,
        r: &ReviewRef,
        path: &str,
        line: u32,
        body: &str,
    ) -> Result<(), KeltaError> {
        let refs = self.json(HttpRequest::get(self.mr_url(&r.repo, &format!("/{}", r.number)))).await?.body;
        let refs = refs.get("diff_refs").filter(|d| d.is_object()).ok_or_else(|| {
            KeltaError::upstream("merge request has no diff_refs yet (still being created?)")
        })?;
        // shortcut: old_path = new_path, so a comment on a renamed file is rejected by GitLab; pass the old path when needed.
        let position = json!({
            "position_type": "text", "base_sha": refs["base_sha"], "start_sha": refs["start_sha"],
            "head_sha": refs["head_sha"], "new_path": path, "old_path": path, "new_line": line,
        });
        let url = self.mr_url(&r.repo, &format!("/{}/draft_notes", r.number));
        self.auth.send_text(HttpRequest::post(url).json(json!({"note": body, "position": position}))).await?;
        Ok(())
    }

    async fn create(&self, d: &PrCreate) -> Result<Review, KeltaError> {
        let title = if d.draft && !d.title.to_ascii_lowercase().starts_with("draft:") {
            format!("Draft: {}", d.title)
        } else {
            d.title.clone()
        };
        let body =
            json!({"source_branch": d.head, "target_branch": d.base, "title": title, "description": d.body});
        let v = self.json(HttpRequest::post(self.mr_url(&d.repo, "")).json(body)).await?.body;
        self.review_from(&v, Some(&d.repo), ReviewKind::Authored)
            .ok_or_else(|| KeltaError::upstream("create merge request response without iid"))
    }

    async fn update_title(&self, r: &ReviewRef, title: &str) -> Result<(), KeltaError> {
        let url = self.mr_url(&r.repo, &format!("/{}", r.number));
        self.auth.send_text(HttpRequest::put(url).json(json!({ "title": title }))).await.map(|_| ())
    }

    async fn find_for_branch(&self, repo: &str, branch: &str) -> Result<Option<Review>, KeltaError> {
        let v = self
            .json(
                HttpRequest::get(self.mr_url(repo, ""))
                    .query("source_branch", branch)
                    .query("state", "opened")
                    .query("per_page", "1"),
            )
            .await?
            .body;
        let me = self.me().await.ok();
        Ok(v.get(0).and_then(|m| {
            let mine =
                m.pointer("/author/id").and_then(Value::as_u64).map(|i| i.to_string()) == me.map(|u| u.id);
            self.review_from(
                m,
                Some(repo),
                if mine { ReviewKind::Authored } else { ReviewKind::ReviewRequested },
            )
        }))
    }

    fn fetch_refspec(&self, r: &ReviewRef, local_branch: &str) -> String {
        format!("merge-requests/{}/head:{local_branch}", r.number)
    }

    async fn feedback(&self, r: &ReviewRef) -> Result<Feedback, KeltaError> {
        let refused = feedback_error("GitLab", "review discussions", "read_api");
        let url = self.mr_url(&r.repo, &format!("/{}", r.number));
        let (mr, discussions) = tokio::join!(
            self.json(HttpRequest::get(&url)),
            self.json(HttpRequest::get(format!("{url}/discussions")).query("per_page", PER_PAGE)),
        );
        let mr = mr.map_err(&refused)?.body;
        let discussions = discussions.map_err(&refused)?.body;
        let web = s(&mr, "web_url").unwrap_or("").to_owned();
        let me = self.me().await.ok().and_then(|u| u.login);
        let username =
            |n: &Value| n.pointer("/author/username").and_then(Value::as_str).unwrap_or("ghost").to_owned();
        let threads = discussions
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|d| {
                let notes: Vec<&Value> = d
                    .get("notes")
                    .and_then(Value::as_array)?
                    .iter()
                    .filter(|n| n.get("system").and_then(Value::as_bool) != Some(true))
                    .collect();
                let first = *notes.first()?;
                let resolvable =
                    notes.iter().any(|n| n.get("resolvable").and_then(Value::as_bool) == Some(true));
                let resolved = notes
                    .iter()
                    .filter(|n| n.get("resolvable").and_then(Value::as_bool) == Some(true))
                    .all(|n| n.get("resolved").and_then(Value::as_bool) == Some(true));
                if !resolvable || resolved {
                    return None;
                }
                let pos = first.get("position").filter(|p| !p.is_null());
                let line = pos
                    .and_then(|p| p.get("new_line").filter(|l| !l.is_null()).or_else(|| p.get("old_line")));
                Some(FeedbackThread {
                    id: s(d, "id")?.to_owned(),
                    author: username(first),
                    path: pos.and_then(|p| s(p, "new_path").or_else(|| s(p, "old_path"))).map(str::to_owned),
                    line: line.and_then(Value::as_u64).map(|l| l as u32),
                    body_md: notes
                        .iter()
                        .map(|n| format!("{}: {}", username(n), s(n, "body").unwrap_or("").trim()))
                        .collect::<Vec<_>>()
                        .join("\n\n"),
                    url: first
                        .get("id")
                        .and_then(Value::as_u64)
                        .map(|id| format!("{web}#note_{id}"))
                        .unwrap_or(web.clone()),
                })
            })
            .collect();
        let reviewers = mr
            .get("reviewers")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|u| s(u, "username").map(str::to_owned))
            .filter(|u| Some(u) != me.as_ref())
            .collect();
        let failed: Vec<Value> = self
            .pipeline_jobs(&r.repo, &mr)
            .await
            .into_iter()
            .filter(|j| s(j, "status") == Some("failed"))
            .collect();
        let mut failed_checks = Vec::new();
        for (i, j) in failed.iter().enumerate() {
            let log_tail = match j.get("id").and_then(Value::as_u64) {
                Some(id) if i < MAX_LOGS => {
                    let req = HttpRequest::get(self.project_url(&r.repo, &format!("/jobs/{id}/trace")));
                    self.auth.send_text(req).await.ok().map(|t| log_tail(&t.body))
                }
                _ => None,
            };
            failed_checks.push(FailedCheck {
                name: s(j, "name").unwrap_or("").to_owned(),
                url: s(j, "web_url").map(str::to_owned),
                log_tail,
            });
        }
        // GitLab review summaries are plain notes: the unresolved threads carry the feedback.
        Ok(Feedback { threads, reviews: Vec::new(), failed_checks, reviewers })
    }

    async fn rerequest_review(&self, r: &ReviewRef) -> Result<Vec<String>, KeltaError> {
        let mr = self.json(HttpRequest::get(self.mr_url(&r.repo, &format!("/{}", r.number)))).await?.body;
        let me = self.me().await?;
        let logins: Vec<String> = mr
            .get("reviewers")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|u| s(u, "username").map(str::to_owned))
            .filter(|u| Some(u) != me.login.as_ref())
            .collect();
        if logins.is_empty() {
            return Err(KeltaError::invalid("this merge request has no reviewers"));
        }
        // shortcut: the `/request_review` quick action needs GitLab 17; older servers post it as a
        // plain comment. Upgrade if a REST re-request endpoint appears.
        let body = format!(
            "/request_review {}",
            logins.iter().map(|l| format!("@{l}")).collect::<Vec<_>>().join(" ")
        );
        // Not `comment`: that would also publish my pending draft notes.
        let url = self.mr_url(&r.repo, &format!("/{}/notes", r.number));
        self.auth.send_text(HttpRequest::post(url).json(json!({"body": body}))).await?;
        Ok(logins)
    }

    async fn resolve_threads(&self, r: &ReviewRef, ids: &[String]) -> Result<(), KeltaError> {
        for id in ids {
            let url = self.mr_url(&r.repo, &format!("/{}/discussions/{}", r.number, percent_encode(id)));
            self.auth.send_text(HttpRequest::put(url).query("resolved", "true")).await?;
        }
        Ok(())
    }

    fn repo_from_remote(&self, url: &str) -> Option<String> {
        let (host, path) = parse_remote(url)?;
        if !host_matches(&host, &self.web_host) {
            return None;
        }
        // Self-managed instances may live under a relative URL root; the project path is the tail
        // that has at least a namespace and a project.
        (path.split('/').filter(|p| !p.is_empty()).count() >= 2).then_some(path)
    }
}

impl GitlabHost {
    async fn draft_count(&self, r: &ReviewRef) -> Result<u32, KeltaError> {
        let url = self.mr_url(&r.repo, &format!("/{}/draft_notes", r.number));
        let v = self.json(HttpRequest::get(url).query("per_page", PER_PAGE)).await?.body;
        Ok(v.as_array().map_or(0, Vec::len) as u32)
    }

    /// Publishes my draft notes (GitLab's pending review); returns how many went out.
    async fn publish_drafts(&self, r: &ReviewRef) -> Result<u32, KeltaError> {
        let n = self.draft_count(r).await?;
        if n > 0 {
            let url = self.mr_url(&r.repo, &format!("/{}/draft_notes/bulk_publish", r.number));
            self.auth.send_text(HttpRequest::post(url)).await?;
        }
        Ok(n)
    }

    /// Browser base URL.
    pub fn web_url(&self) -> &str {
        &self.web
    }
}

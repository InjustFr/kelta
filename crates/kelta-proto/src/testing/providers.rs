//! `FakeTracker` and `FakeCodeHost`: fixture-backed providers with mutable state.

use std::collections::VecDeque;

use async_trait::async_trait;
use parking_lot::Mutex;
use serde_json::Value;

use crate::api::{CodeHost, Tracker};
use crate::codehost::{
    CodeHostKind, Feedback, MyReviewState, PrCreate, Review, ReviewDetail, ReviewQuery, ReviewRef,
};
use crate::error::KeltaError;
use crate::ids::AccountId;
use crate::samples;
use crate::settings::{TrackerBinding, TrackerView};
use crate::tracker::{
    Assignee, Column, Comment, Cursor, Page, Status, StatusCategory, Ticket, TicketDetail, TicketRef,
    TrackerCaps, TrackerKind, Transition, User,
};

/// The four statuses of the fake workflow.
pub fn workflow() -> Vec<Status> {
    vec![
        samples::status("1", "To Do", StatusCategory::Todo),
        samples::status("3", "In Progress", StatusCategory::InProgress),
        samples::status("10", "In Review", StatusCategory::InReview),
        samples::status("5", "Done", StatusCategory::Done),
    ]
}

pub struct FakeTracker {
    pub account: AccountId,
    pub kind: TrackerKind,
    pub caps: TrackerCaps,
    pub me: User,
    tickets: Mutex<Vec<TicketDetail>>,
    /// Transition ids requiring fields (→ `NeedsFields`).
    needs_fields: Mutex<Vec<String>>,
    errors: Mutex<VecDeque<KeltaError>>,
    calls: Mutex<Vec<String>>,
    page_size: usize,
}

impl FakeTracker {
    /// Three tickets (To Do, In Progress, In Review) for account `jira-acme`.
    pub fn new() -> Self {
        let base = samples::ticket();
        let mk = |n: u32, title: &str, st: Status| {
            let mut t = base.clone();
            t.r#ref.key = format!("SHOP-{n}");
            t.r#ref.id = format!("10{n}");
            t.title = title.to_owned();
            t.url = format!("https://acme.atlassian.net/browse/SHOP-{n}");
            t.status = st;
            TicketDetail {
                ticket: t,
                body_md: format!("Body of SHOP-{n}"),
                body_html: format!("<p>Body of SHOP-{n}</p>"),
                body_format: crate::tracker::BodyFormat::Markdown,
                comments: vec![],
                parent: None,
            }
        };
        let wf = workflow();
        Self::with_tickets(vec![
            mk(141, "Add login form", wf[0].clone()),
            mk(142, "Rate-limit login", wf[1].clone()),
            mk(143, "Audit log", wf[2].clone()),
        ])
    }

    pub fn with_tickets(tickets: Vec<TicketDetail>) -> Self {
        let account = tickets
            .first()
            .map(|t| t.ticket.r#ref.account.clone())
            .unwrap_or_else(|| AccountId::new("jira-acme"));
        Self {
            account,
            kind: TrackerKind::Jira,
            caps: TrackerCaps {
                board_columns: true,
                assign: true,
                comment: true,
                transitions_need_fetch: true,
                projects_v2: false,
            },
            me: samples::user(),
            tickets: Mutex::new(tickets),
            needs_fields: Mutex::new(Vec::new()),
            errors: Mutex::new(VecDeque::new()),
            calls: Mutex::new(Vec::new()),
            page_size: 50,
        }
    }

    pub fn with_page_size(mut self, n: usize) -> Self {
        self.page_size = n.max(1);
        self
    }

    /// The next call fails with this error (queue).
    pub fn fail_next(&self, e: KeltaError) {
        self.errors.lock().push_back(e);
    }

    /// Transition `id` will return `NeedsFields`.
    pub fn require_fields(&self, transition_id: &str) {
        self.needs_fields.lock().push(transition_id.to_owned());
    }

    pub fn calls(&self) -> Vec<String> {
        self.calls.lock().clone()
    }

    pub fn ticket(&self, key: &str) -> Option<TicketDetail> {
        self.tickets.lock().iter().find(|t| t.ticket.r#ref.key == key).cloned()
    }

    fn enter(&self, call: &str) -> Result<(), KeltaError> {
        self.calls.lock().push(call.to_owned());
        match self.errors.lock().pop_front() {
            Some(e) => Err(e),
            None => Ok(()),
        }
    }

    fn find(&self, t: &TicketRef) -> Result<TicketDetail, KeltaError> {
        self.ticket(&t.key).ok_or_else(|| KeltaError::not_found(format!("ticket {}", t.key)))
    }

    fn update(&self, t: &TicketRef, f: impl FnOnce(&mut TicketDetail)) -> Result<Ticket, KeltaError> {
        let mut all = self.tickets.lock();
        let d = all
            .iter_mut()
            .find(|d| d.ticket.r#ref.key == t.key)
            .ok_or_else(|| KeltaError::not_found(format!("ticket {}", t.key)))?;
        f(d);
        Ok(d.ticket.clone())
    }
}

impl Default for FakeTracker {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Tracker for FakeTracker {
    fn kind(&self) -> TrackerKind {
        self.kind
    }

    fn caps(&self) -> TrackerCaps {
        self.caps.clone()
    }

    async fn me(&self) -> Result<User, KeltaError> {
        self.enter("me")?;
        Ok(self.me.clone())
    }

    async fn list(&self, view: &TrackerView, cursor: Option<Cursor>) -> Result<Page<Ticket>, KeltaError> {
        self.enter(&format!("list:{}", view.id))?;
        let start = match cursor {
            Some(Cursor::Offset(n)) => n as usize,
            None => 0,
            Some(other) => return Err(KeltaError::invalid(format!("unexpected cursor {other:?}"))),
        };
        let all = self.tickets.lock();
        let items: Vec<Ticket> =
            all.iter().skip(start).take(self.page_size).map(|d| d.ticket.clone()).collect();
        let end = start + items.len();
        let next = (end < all.len()).then_some(Cursor::Offset(end as u32));
        Ok(Page { items, next })
    }

    async fn get(&self, t: &TicketRef) -> Result<TicketDetail, KeltaError> {
        self.enter(&format!("get:{}", t.key))?;
        self.find(t)
    }

    async fn columns(&self, _b: &TrackerBinding) -> Result<Vec<Column>, KeltaError> {
        self.enter("columns")?;
        Ok(workflow()
            .into_iter()
            .enumerate()
            .map(|(i, s)| Column {
                id: s.id.clone(),
                name: s.name.clone(),
                category: s.category,
                order: i as u32,
                match_names: vec![s.name],
            })
            .collect())
    }

    async fn transitions(&self, t: &TicketRef) -> Result<Vec<Transition>, KeltaError> {
        self.enter(&format!("transitions:{}", t.key))?;
        let current = self.find(t)?.ticket.status;
        let needs = self.needs_fields.lock().clone();
        Ok(workflow()
            .into_iter()
            .filter(|s| s.id != current.id)
            .map(|s| {
                let id = format!("t{}", s.id);
                Transition {
                    needs_fields: needs.contains(&id),
                    id,
                    name: format!("Move to {}", s.name),
                    to: s,
                }
            })
            .collect())
    }

    async fn transition(
        &self,
        t: &TicketRef,
        transition_id: &str,
        fields: Option<Value>,
    ) -> Result<Ticket, KeltaError> {
        self.enter(&format!("transition:{}:{transition_id}", t.key))?;
        if fields.is_none() && self.needs_fields.lock().iter().any(|x| x == transition_id) {
            return Err(KeltaError::new(crate::error::ErrorCode::NeedsFields, "fields required")
                .with_detail(serde_json::json!({"fields": [{"id": "resolution", "name": "Resolution"}]})));
        }
        let to = workflow()
            .into_iter()
            .find(|s| format!("t{}", s.id) == transition_id)
            .ok_or_else(|| KeltaError::not_found(format!("transition {transition_id}")))?;
        self.update(t, |d| d.ticket.status = to)
    }

    async fn comment(&self, t: &TicketRef, markdown: &str) -> Result<(), KeltaError> {
        self.enter(&format!("comment:{}", t.key))?;
        let me = self.me.clone();
        self.update(t, |d| {
            d.comments.push(Comment {
                author: me,
                created_at: crate::now_rfc3339(),
                body_html: format!("<p>{markdown}</p>"),
            });
        })?;
        Ok(())
    }

    async fn assign(&self, t: &TicketRef, who: Assignee) -> Result<Ticket, KeltaError> {
        self.enter(&format!("assign:{}", t.key))?;
        let me = self.me.clone();
        self.update(t, |d| {
            d.ticket.assignee = match who {
                Assignee::Me => Some(me),
                Assignee::User { id } => {
                    Some(User { id: id.clone(), name: id, login: None, avatar_url: None })
                }
                Assignee::None => None,
            }
        })
    }

    fn browser_url(&self, t: &TicketRef) -> String {
        format!("https://acme.atlassian.net/browse/{}", t.key)
    }

    fn branch_key(&self, t: &TicketRef) -> String {
        t.key.clone()
    }
}

pub struct FakeCodeHost {
    pub account: AccountId,
    pub kind: CodeHostKind,
    pub me: User,
    reviews: Mutex<Vec<ReviewDetail>>,
    approvals: Mutex<Vec<(ReviewRef, String)>>,
    comments: Mutex<Vec<(ReviewRef, String, bool)>>,
    errors: Mutex<VecDeque<KeltaError>>,
    calls: Mutex<Vec<String>>,
    changed: Mutex<bool>,
}

impl FakeCodeHost {
    /// One requested review (`acme/shop-api#87`) and one authored PR (`#90`).
    pub fn new() -> Self {
        let requested = samples::review();
        let mut authored = samples::review();
        authored.r#ref.number = 90;
        authored.title = "SHOP-142: Rate-limit login".into();
        authored.url = "https://github.com/acme/shop-api/pull/90".into();
        authored.kind = crate::codehost::ReviewKind::Authored;
        authored.author = samples::user();
        authored.source_branch = "feat/SHOP-142-rate-limit-login".into();
        authored.my_state = None;
        Self::with_reviews(vec![requested, authored])
    }

    pub fn with_reviews(reviews: Vec<Review>) -> Self {
        let account =
            reviews.first().map(|r| r.r#ref.account.clone()).unwrap_or_else(|| AccountId::new("github-work"));
        Self {
            account,
            kind: CodeHostKind::Github,
            me: samples::user(),
            reviews: Mutex::new(
                reviews
                    .into_iter()
                    .map(|review| ReviewDetail {
                        body_html: format!("<p>{}</p>", review.title),
                        review,
                        reviewers: vec![],
                        checks: vec![],
                        files: vec![],
                    })
                    .collect(),
            ),
            approvals: Mutex::new(Vec::new()),
            comments: Mutex::new(Vec::new()),
            errors: Mutex::new(VecDeque::new()),
            calls: Mutex::new(Vec::new()),
            changed: Mutex::new(true),
        }
    }

    pub fn fail_next(&self, e: KeltaError) {
        self.errors.lock().push_back(e);
    }

    pub fn set_changed(&self, changed: bool) {
        *self.changed.lock() = changed;
    }

    /// Simulate a push: change the head sha of a review.
    pub fn set_head(&self, r: &ReviewRef, sha: &str) {
        if let Some(d) = self.reviews.lock().iter_mut().find(|d| &d.review.r#ref == r) {
            d.review.head_sha = sha.to_owned();
        }
    }

    pub fn approvals(&self) -> Vec<(ReviewRef, String)> {
        self.approvals.lock().clone()
    }

    /// `(review, body, request_changes)`.
    pub fn comments(&self) -> Vec<(ReviewRef, String, bool)> {
        self.comments.lock().clone()
    }

    pub fn calls(&self) -> Vec<String> {
        self.calls.lock().clone()
    }

    fn enter(&self, call: &str) -> Result<(), KeltaError> {
        self.calls.lock().push(call.to_owned());
        match self.errors.lock().pop_front() {
            Some(e) => Err(e),
            None => Ok(()),
        }
    }

    fn find(&self, r: &ReviewRef) -> Result<ReviewDetail, KeltaError> {
        self.reviews
            .lock()
            .iter()
            .find(|d| &d.review.r#ref == r)
            .cloned()
            .ok_or_else(|| KeltaError::not_found(format!("review {}#{}", r.repo, r.number)))
    }
}

impl Default for FakeCodeHost {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl CodeHost for FakeCodeHost {
    fn kind(&self) -> CodeHostKind {
        self.kind
    }

    async fn me(&self) -> Result<User, KeltaError> {
        self.enter("me")?;
        Ok(self.me.clone())
    }

    async fn changed_since_last(&self) -> Result<bool, KeltaError> {
        self.enter("changed_since_last")?;
        Ok(*self.changed.lock())
    }

    async fn list_reviews(&self, q: &ReviewQuery) -> Result<Vec<Review>, KeltaError> {
        self.enter("list_reviews")?;
        Ok(self
            .reviews
            .lock()
            .iter()
            .map(|d| d.review.clone())
            .filter(|r| r.kind == q.kind && (q.include_drafts || !r.draft))
            .collect())
    }

    async fn get(&self, r: &ReviewRef) -> Result<ReviewDetail, KeltaError> {
        self.enter(&format!("get:{}", r.number))?;
        self.find(r)
    }

    async fn approve(&self, r: &ReviewRef, head_sha: &str) -> Result<(), KeltaError> {
        self.enter(&format!("approve:{}", r.number))?;
        let d = self.find(r)?;
        if d.review.head_sha != head_sha {
            return Err(KeltaError::conflict("head moved; refresh the pull request"));
        }
        self.approvals.lock().push((r.clone(), head_sha.to_owned()));
        if let Some(x) = self.reviews.lock().iter_mut().find(|x| &x.review.r#ref == r) {
            x.review.my_state = Some(MyReviewState::Approved);
        }
        Ok(())
    }

    async fn comment(&self, r: &ReviewRef, body: &str) -> Result<(), KeltaError> {
        self.enter(&format!("comment:{}", r.number))?;
        self.find(r)?;
        self.comments.lock().push((r.clone(), body.to_owned(), false));
        Ok(())
    }

    async fn request_changes(&self, r: &ReviewRef, body: &str) -> Result<(), KeltaError> {
        self.enter(&format!("request_changes:{}", r.number))?;
        self.find(r)?;
        self.comments.lock().push((r.clone(), body.to_owned(), true));
        Ok(())
    }

    async fn create(&self, d: &PrCreate) -> Result<Review, KeltaError> {
        self.enter("create")?;
        let mut reviews = self.reviews.lock();
        let number = reviews.iter().map(|x| x.review.r#ref.number).max().unwrap_or(0) + 1;
        let mut review = samples::review();
        review.r#ref = ReviewRef { account: self.account.clone(), repo: d.repo.clone(), number };
        review.title = d.title.clone();
        review.url = format!("https://github.com/{}/pull/{number}", d.repo);
        review.draft = d.draft;
        review.source_branch = d.head.clone();
        review.target_branch = d.base.clone();
        review.kind = crate::codehost::ReviewKind::Authored;
        review.author = self.me.clone();
        review.my_state = None;
        reviews.push(ReviewDetail {
            review: review.clone(),
            body_html: format!("<p>{}</p>", d.body),
            reviewers: vec![],
            checks: vec![],
            files: vec![],
        });
        Ok(review)
    }

    async fn find_for_branch(&self, repo: &str, branch: &str) -> Result<Option<Review>, KeltaError> {
        self.enter(&format!("find_for_branch:{branch}"))?;
        Ok(self
            .reviews
            .lock()
            .iter()
            .map(|d| d.review.clone())
            .find(|r| r.r#ref.repo == repo && r.source_branch == branch))
    }

    fn fetch_refspec(&self, r: &ReviewRef, local_branch: &str) -> String {
        match self.kind {
            CodeHostKind::Github => format!("pull/{}/head:{local_branch}", r.number),
            CodeHostKind::Gitlab => format!("merge-requests/{}/head:{local_branch}", r.number),
        }
    }

    async fn feedback(&self, r: &ReviewRef) -> Result<Feedback, KeltaError> {
        self.enter(&format!("feedback:{}", r.number))?;
        Ok(samples::feedback())
    }

    async fn rerequest_review(&self, r: &ReviewRef) -> Result<Vec<String>, KeltaError> {
        self.enter(&format!("rerequest_review:{}", r.number))?;
        Ok(samples::feedback().reviewers)
    }

    async fn resolve_threads(&self, r: &ReviewRef, ids: &[String]) -> Result<(), KeltaError> {
        self.enter(&format!("resolve_threads:{}:{}", r.number, ids.join(",")))
    }

    fn repo_from_remote(&self, url: &str) -> Option<String> {
        let rest = url
            .strip_prefix("git@")
            .and_then(|r| r.split_once(':').map(|(_, p)| p))
            .or_else(|| url.split_once("://").and_then(|(_, r)| r.split_once('/').map(|(_, p)| p)))?;
        Some(rest.trim_end_matches(".git").trim_matches('/').to_owned())
    }
}

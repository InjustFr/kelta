//! Tickets and reviews (ARCHITECTURE §8.4, SPEC §2-3): `Scope::All` aggregation with de-dup,
//! project tagging and the "Other" bucket, `provider_cache` stale-while-revalidate,
//! `seen_reviews` diffing (silent first poll) → `pr.*` bus events, tracker/review command handlers
//! incl. `tracker_move` column → transition resolution, scheduler subscriptions + refresher.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use kelta_proto::api::{CodeHost, Tracker};
use kelta_proto::codehost::{
    CiState, MyReviewState, Review, ReviewDecision, ReviewDetail, ReviewItem, ReviewKind, ReviewPage,
    ReviewQuery, ReviewRef,
};
use kelta_proto::error::{ErrorCode, KeltaError};
use kelta_proto::events::{AccountStatus, BusEvent, Notification, UiEvent, bus};
use kelta_proto::ext::Urgency;
use kelta_proto::ids::{AccountId, ProjectId, WorkItemId};
use kelta_proto::model::{PaneContent, Scope, WorkState};
use kelta_proto::settings::{AccountKind, ColumnSpec, ProjectConfig, Settings, TrackerBinding, TrackerView};
use kelta_proto::store::{ProviderCacheRow, SeenReviewRow};
use kelta_proto::tracker::{
    AccountError, Assignee, Column, Cursor, Page, StatusCategory, Ticket, TicketDetail, TicketItem,
    TicketPage, TicketRef, Transition,
};
use parking_lot::Mutex;
use sha2::{Digest, Sha256};

use crate::Core;
use crate::scheduler::{IntervalPolicy, Refresher, SubKey};
use crate::status::NotifyKind;
use crate::store::q;

#[derive(Default)]
pub struct Feeds {
    /// Accounts whose review-requested list was fetched once in this process.
    primed: Mutex<HashSet<AccountId>>,
    /// Last authored reviews per account (baseline for `pr.*` change events).
    authored: Mutex<HashMap<AccountId, HashMap<ReviewRef, Review>>>,
    /// Ticket queries by cache key.
    views: Mutex<HashMap<String, TicketQuery>>,
    /// Background revalidations in flight (cache keys).
    inflight: Mutex<HashSet<String>>,
    status: Mutex<HashMap<AccountId, AccountStatus>>,
}

/// One tracker list query shared by the projects that use it.
#[derive(Debug, Clone, PartialEq)]
pub struct TicketQuery {
    pub account: AccountId,
    pub view: TrackerView,
    pub projects: Vec<ProjectId>,
    pub cache_key: String,
}

fn hash16(s: &str) -> String {
    let d = Sha256::digest(s.as_bytes());
    d.iter().take(8).map(|b| format!("{b:02x}")).collect()
}

pub fn tickets_key(account: &AccountId, view: &TrackerView) -> String {
    let v = serde_json::to_string(view).unwrap_or_default();
    format!("tickets:{}", hash16(&format!("{account}\n{v}")))
}

pub fn reviews_key(account: &AccountId, kind: ReviewKind) -> String {
    format!("reviews:{account}:{}", review_kind_str(kind))
}

fn review_kind_str(k: ReviewKind) -> &'static str {
    match k {
        ReviewKind::ReviewRequested => "review_requested",
        ReviewKind::Authored => "authored",
    }
}

fn default_view() -> TrackerView {
    TrackerView { id: "mine".into(), label: "My open".into(), ..TrackerView::default() }
}

/// Seconds since an RFC 3339 timestamp (`u64::MAX`-ish when unparsable).
pub fn age_of(ts: &str) -> Duration {
    use time::format_description::well_known::Rfc3339;
    match time::OffsetDateTime::parse(ts, &Rfc3339) {
        Ok(t) => {
            let d = time::OffsetDateTime::now_utc() - t;
            Duration::from_secs(d.whole_seconds().max(0) as u64)
        }
        Err(_) => Duration::from_secs(u64::from(u32::MAX)),
    }
}

/// Merge pages from several queries: de-dup by ref, union of project ids.
pub fn merge_tickets(
    pages: Vec<(Vec<Ticket>, Vec<ProjectId>)>,
    work: &HashMap<TicketRef, WorkItemId>,
) -> Vec<TicketItem> {
    let mut out: Vec<TicketItem> = Vec::new();
    let mut index: HashMap<TicketRef, usize> = HashMap::new();
    for (tickets, projects) in pages {
        for t in tickets {
            match index.get(&t.r#ref) {
                Some(&i) => {
                    for p in &projects {
                        if !out[i].project_ids.contains(p) {
                            out[i].project_ids.push(p.clone());
                        }
                    }
                }
                None => {
                    index.insert(t.r#ref.clone(), out.len());
                    let work_item_id = work.get(&t.r#ref).cloned();
                    out.push(TicketItem { ticket: t, project_ids: projects.clone(), work_item_id });
                }
            }
        }
    }
    out
}

/// De-dup reviews by ref and tag them with the projects binding `(account, repo)`; unbound → `[]`.
pub fn merge_reviews(reviews: Vec<Review>, bindings: &[(AccountId, String, ProjectId)]) -> Vec<ReviewItem> {
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for r in reviews {
        if !seen.insert(r.r#ref.clone()) {
            continue;
        }
        let mut project_ids: Vec<ProjectId> = bindings
            .iter()
            .filter(|(a, repo, _)| a == &r.r#ref.account && repo.eq_ignore_ascii_case(&r.r#ref.repo))
            .map(|(_, _, p)| p.clone())
            .collect();
        project_ids.dedup();
        out.push(ReviewItem { review: r, project_ids });
    }
    out
}

/// Column → transition: names first (status or transition name, case-insensitive), else
/// categories. None → `NotFound`; several → `Conflict` with `detail.candidates`.
pub fn resolve_move(
    transitions: &[Transition],
    column: &str,
    categories: &[StatusCategory],
    names: &[String],
) -> Result<Transition, KeltaError> {
    let by_name: Vec<&Transition> = transitions
        .iter()
        .filter(|t| {
            names.iter().any(|n| n.eq_ignore_ascii_case(&t.to.name) || n.eq_ignore_ascii_case(&t.name))
        })
        .collect();
    let candidates: Vec<&Transition> = if by_name.is_empty() {
        transitions.iter().filter(|t| categories.contains(&t.to.category)).collect()
    } else {
        by_name
    };
    match candidates.as_slice() {
        [] => Err(KeltaError::not_found(format!("No transition to {column}"))
            .with_detail(serde_json::json!({ "column": column }))),
        [one] => Ok((*one).clone()),
        many => Err(KeltaError::conflict(format!("Several transitions lead to {column}"))
            .with_detail(serde_json::json!({ "column": column, "candidates": many }))),
    }
}

/// Default board columns by status category.
pub fn default_columns() -> Vec<Column> {
    [
        ("todo", "To do", StatusCategory::Todo),
        ("in_progress", "In progress", StatusCategory::InProgress),
        ("in_review", "In review", StatusCategory::InReview),
        ("done", "Done", StatusCategory::Done),
    ]
    .into_iter()
    .enumerate()
    .map(|(i, (id, name, category))| Column {
        id: id.into(),
        name: name.into(),
        category,
        order: i as u32,
        match_names: vec![],
    })
    .collect()
}

fn spec_column(i: usize, s: &ColumnSpec) -> Column {
    Column {
        id: s.id.clone(),
        name: if s.label.is_empty() { s.id.clone() } else { s.label.clone() },
        category: s.categories.first().copied().unwrap_or_default(),
        order: i as u32,
        match_names: s.names.clone(),
    }
}

fn account_status_of(r: &Result<(), &KeltaError>) -> AccountStatus {
    match r {
        Ok(()) => AccountStatus::Ok,
        Err(e) => match e.code {
            ErrorCode::NeedsAuth => AccountStatus::NeedsAuth,
            ErrorCode::RateLimited => AccountStatus::RateLimited,
            ErrorCode::Network | ErrorCode::Timeout => AccountStatus::Offline,
            _ => AccountStatus::Error,
        },
    }
}

fn globs(patterns: &[String]) -> Option<globset::GlobSet> {
    if patterns.is_empty() {
        return None;
    }
    let mut b = globset::GlobSetBuilder::new();
    for p in patterns {
        if let Ok(g) = globset::Glob::new(p) {
            b.add(g);
        }
    }
    b.build().ok()
}

impl Core {
    fn accounts_settings(&self) -> Arc<Settings> {
        self.cfg.effective(None)
    }

    // =========================================================================================
    // Account status
    // =========================================================================================

    fn note_account<T>(&self, account: &AccountId, r: &Result<T, KeltaError>) {
        let status = account_status_of(&r.as_ref().map(|_| ()));
        let detail = r.as_ref().err().map(|e| e.message.clone());
        let changed = {
            let mut m = self.feeds.status.lock();
            let prev = m.insert(account.clone(), status);
            prev != Some(status) && !(prev.is_none() && status == AccountStatus::Ok)
        };
        if changed {
            self.emit(UiEvent::AccountStatusChanged { account_id: account.clone(), status, detail });
        }
    }

    fn tracker_of(&self, account: &AccountId) -> Result<Arc<dyn Tracker>, KeltaError> {
        let r = self.providers.tracker(account, &self.accounts_settings().accounts);
        if let Err(e) = &r
            && e.code != ErrorCode::NotFound
        {
            self.note_account::<()>(account, &Err(e.clone()));
        }
        r
    }

    fn code_host_of(&self, account: &AccountId) -> Result<Arc<dyn CodeHost>, KeltaError> {
        let r = self.providers.code_host(account, &self.accounts_settings().accounts);
        if let Err(e) = &r
            && e.code != ErrorCode::NotFound
        {
            self.note_account::<()>(account, &Err(e.clone()));
        }
        r
    }

    // =========================================================================================
    // Cache
    // =========================================================================================

    async fn cache_get<T: serde::de::DeserializeOwned>(&self, key: &str) -> Option<(T, Duration)> {
        let k = key.to_owned();
        let row = self.store.call(move |c| q::cache_get(c, &k)).await.ok().flatten()?;
        let v = serde_json::from_str(&row.body_json).ok()?;
        Some((v, age_of(&row.fetched_at)))
    }

    fn cache_put<T: serde::Serialize>(&self, key: &str, v: &T) {
        let Ok(body) = serde_json::to_string(v) else { return };
        let row = ProviderCacheRow {
            key: key.to_owned(),
            etag: None,
            body_json: body,
            fetched_at: kelta_proto::now_rfc3339(),
        };
        self.store.exec("cache_put", move |c| q::cache_put(c, &row));
    }

    fn focused_interval(&self) -> Duration {
        let p = &self.accounts_settings().polling;
        Duration::from_secs(u64::from(p.focused_secs.max(p.min_secs).max(30)))
    }

    // =========================================================================================
    // Tickets
    // =========================================================================================

    /// The list queries behind a scope (one per distinct `(account, view)`).
    pub fn ticket_queries(&self, scope: &Scope, view_id: Option<&str>) -> Vec<TicketQuery> {
        let projects: Vec<Arc<ProjectConfig>> = match scope {
            Scope::Project { id } => self.cfg.project(id).into_iter().collect(),
            Scope::All => {
                let open = self.open_projects();
                self.project_configs().into_iter().filter(|p| open.contains(&p.id)).collect()
            }
        };
        let mut out: Vec<TicketQuery> = Vec::new();
        for p in projects {
            let Some(b) = &p.tracker else { continue };
            let view = match view_id {
                Some(v) => b.views.iter().find(|x| x.id == v).or(b.views.first()),
                None => b.views.first(),
            }
            .cloned()
            .unwrap_or_else(default_view);
            let key = tickets_key(&b.account, &view);
            match out.iter_mut().find(|q| q.cache_key == key) {
                Some(q) => q.projects.push(p.id.clone()),
                None => out.push(TicketQuery {
                    account: b.account.clone(),
                    view,
                    projects: vec![p.id.clone()],
                    cache_key: key,
                }),
            }
        }
        let mut views = self.feeds.views.lock();
        for q in &out {
            views.insert(q.cache_key.clone(), q.clone());
        }
        out
    }

    async fn fetch_tickets(
        &self,
        q: &TicketQuery,
        cursor: Option<Cursor>,
    ) -> Result<Page<Ticket>, KeltaError> {
        let tracker = self.tracker_of(&q.account)?;
        let r = tracker.list(&q.view, cursor.clone()).await;
        self.note_account(&q.account, &r);
        let page = r?;
        if cursor.is_none() {
            self.cache_put(&q.cache_key, &page);
        }
        Ok(page)
    }

    fn emit_tickets_changed(&self, projects: &[ProjectId]) {
        self.emit(UiEvent::TicketsChanged { scope: Scope::All });
        for p in projects {
            self.emit(UiEvent::TicketsChanged { scope: Scope::Project { id: p.clone() } });
        }
    }

    fn revalidate_tickets(&self, q: TicketQuery) {
        if !self.feeds.inflight.lock().insert(q.cache_key.clone()) {
            return;
        }
        let weak = self.me.clone();
        self.rt.spawn(async move {
            let Some(core) = weak.upgrade() else { return };
            let r = core.fetch_tickets(&q, None).await;
            core.feeds.inflight.lock().remove(&q.cache_key);
            if r.is_ok() {
                core.emit_tickets_changed(&q.projects);
            }
        });
    }

    async fn load_ticket_query(
        &self,
        q: &TicketQuery,
        cursor: Option<Cursor>,
        refresh: bool,
    ) -> (Option<Page<Ticket>>, bool, Option<AccountError>) {
        let err = |e: KeltaError| Some(AccountError { account_id: q.account.clone(), error: e });
        if cursor.is_some() {
            return match self.fetch_tickets(q, cursor).await {
                Ok(p) => (Some(p), false, None),
                Err(e) => (None, false, err(e)),
            };
        }
        let cached: Option<(Page<Ticket>, Duration)> = self.cache_get(&q.cache_key).await;
        if !refresh && let Some((page, age)) = &cached {
            let stale = *age > self.focused_interval();
            if stale {
                self.revalidate_tickets(q.clone());
            }
            return (Some(page.clone()), stale, None);
        }
        match self.fetch_tickets(q, None).await {
            Ok(p) => (Some(p), false, None),
            Err(e) => {
                let stale = cached.is_some();
                (cached.map(|(p, _)| p), stale, err(e))
            }
        }
    }

    async fn work_by_ticket(&self) -> HashMap<TicketRef, WorkItemId> {
        let items = self.store.call(|c| q::work_list(c, None)).await.unwrap_or_default();
        items
            .into_iter()
            .filter(|w| w.state != WorkState::Finished)
            .filter_map(|w| w.ticket.clone().map(|t| (t, w.id)))
            .collect()
    }

    /// `tracker_list`.
    pub async fn tracker_list(
        &self,
        scope: Scope,
        view_id: Option<String>,
        cursor: Option<Cursor>,
        refresh: bool,
    ) -> Result<TicketPage, KeltaError> {
        self.rt.capture();
        if let Scope::Project { id } = &scope
            && !self.project_exists(id)
        {
            return Err(KeltaError::not_found(format!("project {id}")));
        }
        let queries = self.ticket_queries(&scope, view_id.as_deref());
        if queries.is_empty() {
            return Ok(TicketPage::default());
        }
        let single = queries.len() == 1;
        let cursor = if single { cursor } else { None };
        let results = futures::future::join_all(
            queries.iter().map(|q| self.load_ticket_query(q, cursor.clone(), refresh)),
        )
        .await;
        let mut pages = Vec::new();
        let mut stale = false;
        let mut errors = Vec::new();
        let mut next = None;
        for (q, (page, s, e)) in queries.iter().zip(results) {
            stale |= s;
            if let Some(e) = e {
                errors.push(e);
            }
            if let Some(p) = page {
                if single {
                    next = p.next.clone();
                }
                pages.push((p.items, q.projects.clone()));
            }
        }
        let work = self.work_by_ticket().await;
        Ok(TicketPage { items: merge_tickets(pages, &work), next, stale, errors })
    }

    /// `tracker_search` over the (cached) lists of a scope.
    pub async fn tracker_search(&self, scope: Scope, text: &str) -> Result<Vec<TicketItem>, KeltaError> {
        let page = self.tracker_list(scope, None, None, false).await?;
        let needle = text.trim().to_lowercase();
        Ok(page
            .items
            .into_iter()
            .filter(|i| {
                needle.is_empty()
                    || i.ticket.r#ref.key.to_lowercase().contains(&needle)
                    || i.ticket.title.to_lowercase().contains(&needle)
            })
            .take(50)
            .collect())
    }

    pub async fn tracker_get(&self, t: &TicketRef) -> Result<TicketDetail, KeltaError> {
        self.rt.capture();
        let r = self.tracker_of(&t.account)?.get(t).await;
        self.note_account(&t.account, &r);
        r
    }

    /// The project bound to a ticket's account (active first, then open, then any).
    fn project_for_account(&self, account: &AccountId) -> Option<Arc<ProjectConfig>> {
        let all: Vec<Arc<ProjectConfig>> = self
            .project_configs()
            .into_iter()
            .filter(|p| p.tracker.as_ref().is_some_and(|b| &b.account == account))
            .collect();
        let active = self.active_project();
        let open = self.open_projects();
        all.iter()
            .find(|p| p.id == active)
            .or_else(|| all.iter().find(|p| open.contains(&p.id)))
            .or(all.first())
            .cloned()
    }

    fn binding_of(&self, project: &ProjectId) -> Result<TrackerBinding, KeltaError> {
        self.cfg
            .project(project)
            .ok_or_else(|| KeltaError::not_found(format!("project {project}")))?
            .tracker
            .clone()
            .ok_or_else(|| KeltaError::invalid(format!("project {project} has no tracker")))
    }

    /// `tracker_columns`: project `tracker.columns` override, else the provider's, else by category.
    pub async fn tracker_columns(&self, project: &ProjectId) -> Result<Vec<Column>, KeltaError> {
        self.rt.capture();
        let b = self.binding_of(project)?;
        if !b.columns.is_empty() {
            return Ok(b.columns.iter().enumerate().map(|(i, s)| spec_column(i, s)).collect());
        }
        match self.tracker_of(&b.account)?.columns(&b).await {
            Ok(c) if !c.is_empty() => Ok(c),
            Ok(_) => Ok(default_columns()),
            Err(e) if e.code == ErrorCode::Unsupported => Ok(default_columns()),
            Err(e) => Err(e),
        }
    }

    pub async fn tracker_transitions(&self, t: &TicketRef) -> Result<Vec<Transition>, KeltaError> {
        self.rt.capture();
        let r = self.tracker_of(&t.account)?.transitions(t).await;
        self.note_account(&t.account, &r);
        r
    }

    pub async fn tracker_transition(
        &self,
        t: &TicketRef,
        transition_id: &str,
        fields: Option<serde_json::Value>,
    ) -> Result<kelta_proto::tracker::Ticket, KeltaError> {
        self.rt.capture();
        let tracker = self.tracker_of(&t.account)?;
        let from = self.cached_ticket(t).await.map(|x| x.status);
        let ticket = tracker.transition(t, transition_id, fields).await?;
        self.publish_ev(BusEvent::new(
            bus::TICKET_TRANSITIONED,
            serde_json::json!({ "ticket": t, "from": from, "to": ticket.status }),
        ));
        self.after_ticket_write(&ticket).await;
        Ok(ticket)
    }

    /// `tracker_move`: column → transition (by names, then categories); ambiguous → `Conflict`.
    pub async fn tracker_move(
        &self,
        t: &TicketRef,
        column_id: &str,
    ) -> Result<kelta_proto::tracker::Ticket, KeltaError> {
        self.rt.capture();
        let project = self
            .project_for_account(&t.account)
            .ok_or_else(|| KeltaError::not_found(format!("no project uses tracker account {}", t.account)))?;
        let binding = project.tracker.clone().unwrap_or_default();
        let (label, categories, names) = match binding.columns.iter().find(|c| c.id == column_id) {
            Some(s) => (
                if s.label.is_empty() { s.id.clone() } else { s.label.clone() },
                s.categories.clone(),
                s.names.clone(),
            ),
            None => {
                let cols = self.tracker_columns(&project.id).await?;
                let c = cols
                    .into_iter()
                    .find(|c| c.id == column_id)
                    .ok_or_else(|| KeltaError::not_found(format!("column {column_id}")))?;
                (c.name.clone(), vec![c.category], c.match_names.clone())
            }
        };
        let tracker = self.tracker_of(&t.account)?;
        let transitions = tracker.transitions(t).await?;
        let chosen = resolve_move(&transitions, &label, &categories, &names).map_err(|e| {
            if e.code == ErrorCode::NotFound {
                let url = tracker.browser_url(t);
                let mut d = e.detail.clone().unwrap_or_default();
                if let Some(o) = d.as_object_mut() {
                    o.insert("url".into(), serde_json::json!(url));
                }
                e.with_detail(d)
            } else {
                e
            }
        })?;
        self.tracker_transition(t, &chosen.id, None).await
    }

    pub async fn tracker_comment(&self, t: &TicketRef, markdown: &str) -> Result<(), KeltaError> {
        self.rt.capture();
        self.tracker_of(&t.account)?.comment(t, markdown).await?;
        self.publish_ev(BusEvent::new(
            bus::TICKET_COMMENTED,
            serde_json::json!({ "ticket": t, "markdown": markdown }),
        ));
        Ok(())
    }

    pub async fn tracker_assign(
        &self,
        t: &TicketRef,
        who: Assignee,
    ) -> Result<kelta_proto::tracker::Ticket, KeltaError> {
        self.rt.capture();
        let ticket = self.tracker_of(&t.account)?.assign(t, who.clone()).await?;
        self.publish_ev(BusEvent::new(
            bus::TICKET_ASSIGNED,
            serde_json::json!({ "ticket": t, "assignee": who }),
        ));
        self.after_ticket_write(&ticket).await;
        Ok(ticket)
    }

    async fn cached_ticket(&self, t: &TicketRef) -> Option<Ticket> {
        let keys: Vec<String> = self
            .feeds
            .views
            .lock()
            .values()
            .filter(|q| q.account == t.account)
            .map(|q| q.cache_key.clone())
            .collect();
        for k in keys {
            if let Some((page, _)) = self.cache_get::<Page<Ticket>>(&k).await
                && let Some(x) = page.items.into_iter().find(|x| &x.r#ref == t)
            {
                return Some(x);
            }
        }
        None
    }

    /// After a write: patch cached pages, notify views, refresh the account soon.
    async fn after_ticket_write(&self, ticket: &Ticket) {
        let queries: Vec<TicketQuery> =
            self.feeds.views.lock().values().filter(|q| q.account == ticket.r#ref.account).cloned().collect();
        let mut projects = Vec::new();
        for q in queries {
            if let Some((mut page, _)) = self.cache_get::<Page<Ticket>>(&q.cache_key).await {
                let mut hit = false;
                for x in page.items.iter_mut().filter(|x| x.r#ref == ticket.r#ref) {
                    *x = ticket.clone();
                    hit = true;
                }
                if hit {
                    self.cache_put(&q.cache_key, &page);
                }
            }
            projects.extend(q.projects.clone());
        }
        self.emit_tickets_changed(&projects);
        self.scheduler.kick(Some(ticket.r#ref.account.clone()));
    }

    // =========================================================================================
    // Reviews
    // =========================================================================================

    /// Code-host accounts behind a scope: the project's repo bindings, or every configured one.
    pub fn review_accounts(&self, scope: &Scope) -> Vec<AccountId> {
        let mut out: Vec<AccountId> = match scope {
            Scope::Project { id } => self
                .cfg
                .project(id)
                .map(|p| {
                    p.repos.iter().filter_map(|r| r.code_host.as_ref().map(|c| c.account.clone())).collect()
                })
                .unwrap_or_default(),
            Scope::All => self
                .accounts_settings()
                .accounts
                .iter()
                .filter(|(_, a)| a.kind.is_code_host())
                .map(|(id, _)| id.clone())
                .collect(),
        };
        out.sort();
        out.dedup();
        out
    }

    fn review_bindings(&self) -> Vec<(AccountId, String, ProjectId)> {
        self.project_configs()
            .iter()
            .flat_map(|p| {
                p.repos
                    .iter()
                    .filter_map(|r| {
                        r.code_host.as_ref().map(|c| (c.account.clone(), c.repo.clone(), p.id.clone()))
                    })
                    .collect::<Vec<_>>()
            })
            .collect()
    }

    async fn fetch_reviews(&self, account: &AccountId, kind: ReviewKind) -> Result<Vec<Review>, KeltaError> {
        let host = self.code_host_of(account)?;
        let rs = &self.accounts_settings().reviews;
        let query =
            ReviewQuery { kind, include_team: rs.include_team_requests, include_drafts: rs.include_drafts };
        let r = host.list_reviews(&query).await;
        self.note_account(account, &r);
        let mut list = r?;
        if let Ok(re) = regex::Regex::new(&rs.ticket_key_regex) {
            for x in list.iter_mut().filter(|x| x.linked_tickets.is_empty()) {
                let hay = format!("{} {}", x.source_branch, x.title);
                let mut keys: Vec<String> = re.find_iter(&hay).map(|m| m.as_str().to_owned()).collect();
                keys.dedup();
                x.linked_tickets = keys;
            }
        }
        self.cache_put(&reviews_key(account, kind), &list);
        match kind {
            ReviewKind::ReviewRequested => self.diff_requested(account, &list).await?,
            ReviewKind::Authored => self.diff_authored(account, &list),
        }
        Ok(list)
    }

    /// `seen_reviews`: the first poll of an account in this process fills silently; afterwards a
    /// new key (or a new head after my review) → `pr.review_requested` + notification.
    async fn diff_requested(&self, account: &AccountId, list: &[Review]) -> Result<(), KeltaError> {
        let primed = self.feeds.primed.lock().contains(account);
        let acc = account.clone();
        let reviews = list.to_vec();
        let fresh: Vec<Review> = self
            .store
            .call(move |c| {
                let rows = q::seen_reviews(c, acc.as_str())?;
                let known: HashMap<(String, u64), String> =
                    rows.into_iter().map(|r| ((r.repo, r.number), r.head_sha)).collect();
                let mut out = Vec::new();
                for r in &reviews {
                    let k = (r.r#ref.repo.clone(), r.r#ref.number);
                    let row = SeenReviewRow {
                        account: acc.to_string(),
                        repo: r.r#ref.repo.clone(),
                        number: r.r#ref.number,
                        head_sha: r.head_sha.clone(),
                        first_seen: kelta_proto::now_rfc3339(),
                    };
                    match known.get(&k) {
                        None => {
                            q::seen_review_put(c, &row)?;
                            if primed {
                                out.push(r.clone());
                            }
                        }
                        Some(head) if head != &r.head_sha => {
                            q::seen_review_put(c, &row)?;
                            let reviewed = matches!(
                                r.my_state,
                                Some(
                                    MyReviewState::Approved
                                        | MyReviewState::ChangesRequested
                                        | MyReviewState::Commented
                                )
                            );
                            if primed && reviewed {
                                out.push(r.clone());
                            }
                        }
                        Some(_) => {}
                    }
                }
                Ok(out)
            })
            .await?;
        self.feeds.primed.lock().insert(account.clone());
        if fresh.is_empty() {
            return Ok(());
        }
        let bindings = self.review_bindings();
        let mut new_keys = Vec::new();
        for r in &fresh {
            new_keys.push(r.r#ref.clone());
            let project = bindings
                .iter()
                .find(|(a, repo, _)| a == account && repo == &r.r#ref.repo)
                .map(|(_, _, p)| p.clone());
            let mut ev = BusEvent::new(bus::PR_REVIEW_REQUESTED, serde_json::json!({ "review": r }));
            if let Some(p) = &project {
                ev = ev.with_project(p.clone());
            }
            self.publish_ev(ev);
            self.review_notify(NotifyKind::ReviewRequested, "Review requested", r, project);
        }
        self.emit(UiEvent::ReviewsChanged { scope: Scope::All, new_keys: new_keys.clone() });
        let mut projects: Vec<ProjectId> = bindings
            .iter()
            .filter(|(a, repo, _)| a == account && fresh.iter().any(|r| &r.r#ref.repo == repo))
            .map(|(_, _, p)| p.clone())
            .collect();
        projects.dedup();
        for p in projects {
            self.emit(UiEvent::ReviewsChanged {
                scope: Scope::Project { id: p },
                new_keys: new_keys.clone(),
            });
        }
        Ok(())
    }

    /// Authored PRs: CI / decision / head changes → `pr.*` (silent first poll).
    fn diff_authored(&self, account: &AccountId, list: &[Review]) {
        let prev = {
            let mut m = self.feeds.authored.lock();
            m.insert(account.clone(), list.iter().map(|r| (r.r#ref.clone(), r.clone())).collect())
        };
        let Some(prev) = prev else { return };
        let bindings = self.review_bindings();
        for r in list {
            let Some(p) = prev.get(&r.r#ref) else { continue };
            let project = bindings
                .iter()
                .find(|(a, repo, _)| a == account && repo == &r.r#ref.repo)
                .map(|(_, _, p)| p.clone());
            let with = |ev: BusEvent| match &project {
                Some(p) => ev.with_project(p.clone()),
                None => ev,
            };
            let mut changes = Vec::new();
            if r.ci != p.ci {
                changes.push("ci");
                self.publish_ev(with(BusEvent::new(
                    bus::PR_CI_CHANGED,
                    serde_json::json!({ "review": r, "state": r.ci, "previous": p.ci }),
                )));
                if r.ci == CiState::Failure {
                    self.review_notify(NotifyKind::CiFailedMine, "CI failed", r, project.clone());
                }
            }
            if r.decision != p.decision {
                changes.push("decision");
                match r.decision {
                    Some(ReviewDecision::Approved) => {
                        self.publish_ev(with(BusEvent::new(
                            bus::PR_APPROVED,
                            serde_json::json!({ "review": r, "linked_tickets": r.linked_tickets }),
                        )));
                        self.review_notify(NotifyKind::PrApproved, "PR approved", r, project.clone());
                    }
                    Some(ReviewDecision::ChangesRequested) => {
                        self.publish_ev(with(BusEvent::new(
                            bus::PR_CHANGES_REQUESTED,
                            serde_json::json!({ "review": r, "linked_tickets": r.linked_tickets }),
                        )));
                        self.review_notify(
                            NotifyKind::PrChangesRequested,
                            "Changes requested",
                            r,
                            project.clone(),
                        );
                    }
                    _ => {}
                }
            }
            if r.head_sha != p.head_sha {
                changes.push("head");
            }
            if !changes.is_empty() {
                self.publish_ev(with(BusEvent::new(
                    bus::PR_UPDATED,
                    serde_json::json!({ "review": r, "changes": changes }),
                )));
            }
        }
    }

    fn review_notify(&self, kind: NotifyKind, title: &str, r: &Review, project: Option<ProjectId>) {
        let settings = self.accounts_settings();
        if !crate::notifier::should_notify(
            kind,
            &settings.notifications,
            self.bridge.window_state(),
            false,
            crate::notifier::local_minute_of_day(),
        ) {
            return;
        }
        self.deliver(Notification {
            title: title.to_owned(),
            body: Some(format!("{}#{} {}", r.r#ref.repo, r.r#ref.number, r.title)),
            urgency: Urgency::Normal,
            project_id: project,
            session_id: None,
        });
    }

    fn revalidate_reviews(&self, account: AccountId, kind: ReviewKind) {
        let key = reviews_key(&account, kind);
        if !self.feeds.inflight.lock().insert(key.clone()) {
            return;
        }
        let weak = self.me.clone();
        self.rt.spawn(async move {
            let Some(core) = weak.upgrade() else { return };
            let r = core.fetch_reviews(&account, kind).await;
            core.feeds.inflight.lock().remove(&key);
            if r.is_ok() {
                core.emit_reviews_changed(&account);
            }
        });
    }

    fn emit_reviews_changed(&self, account: &AccountId) {
        self.emit(UiEvent::ReviewsChanged { scope: Scope::All, new_keys: vec![] });
        let mut projects: Vec<ProjectId> =
            self.review_bindings().into_iter().filter(|(a, _, _)| a == account).map(|(_, _, p)| p).collect();
        projects.dedup();
        for p in projects {
            self.emit(UiEvent::ReviewsChanged { scope: Scope::Project { id: p }, new_keys: vec![] });
        }
    }

    async fn load_reviews(
        &self,
        account: &AccountId,
        kind: ReviewKind,
        refresh: bool,
    ) -> (Option<Vec<Review>>, bool, Option<AccountError>) {
        let cached: Option<(Vec<Review>, Duration)> = self.cache_get(&reviews_key(account, kind)).await;
        if !refresh && let Some((list, age)) = &cached {
            let stale = *age > self.focused_interval();
            if stale {
                self.revalidate_reviews(account.clone(), kind);
            }
            return (Some(list.clone()), stale, None);
        }
        match self.fetch_reviews(account, kind).await {
            Ok(l) => (Some(l), false, None),
            Err(e) => {
                let stale = cached.is_some();
                (cached.map(|(l, _)| l), stale, Some(AccountError { account_id: account.clone(), error: e }))
            }
        }
    }

    /// `review_list` (page form).
    pub async fn review_page(
        &self,
        scope: Scope,
        kind: ReviewKind,
        refresh: bool,
    ) -> Result<ReviewPage, KeltaError> {
        self.rt.capture();
        if let Scope::Project { id } = &scope
            && !self.project_exists(id)
        {
            return Err(KeltaError::not_found(format!("project {id}")));
        }
        let accounts = self.review_accounts(&scope);
        let results =
            futures::future::join_all(accounts.iter().map(|a| self.load_reviews(a, kind, refresh))).await;
        let rs = self.accounts_settings().reviews.clone();
        let (allow, deny) = (globs(&rs.repos_allow), globs(&rs.repos_deny));
        let bindings = self.review_bindings();
        let mut all = Vec::new();
        let mut stale = false;
        let mut errors = Vec::new();
        for (list, s, e) in results {
            stale |= s;
            if let Some(e) = e {
                errors.push(e);
            }
            all.extend(list.unwrap_or_default());
        }
        all.retain(|r| {
            allow.as_ref().is_none_or(|g| g.is_match(&r.r#ref.repo))
                && !deny.as_ref().is_some_and(|g| g.is_match(&r.r#ref.repo))
        });
        if let Scope::Project { id } = &scope {
            all.retain(|r| {
                bindings.iter().any(|(a, repo, p)| {
                    p == id && a == &r.r#ref.account && repo.eq_ignore_ascii_case(&r.r#ref.repo)
                })
            });
        }
        all.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
        Ok(ReviewPage { items: merge_reviews(all, &bindings), stale, errors })
    }

    pub async fn review_get(&self, r: &ReviewRef) -> Result<ReviewDetail, KeltaError> {
        self.rt.capture();
        let res = self.code_host_of(&r.account)?.get(r).await;
        self.note_account(&r.account, &res);
        res
    }

    async fn after_review_write(&self, r: &ReviewRef) {
        self.emit_reviews_changed(&r.account);
        self.scheduler.kick(Some(r.account.clone()));
    }

    pub async fn review_approve(&self, r: &ReviewRef, head_sha: &str) -> Result<(), KeltaError> {
        self.rt.capture();
        self.code_host_of(&r.account)?.approve(r, head_sha).await?;
        self.after_review_write(r).await;
        Ok(())
    }

    pub async fn review_comment(&self, r: &ReviewRef, body: &str) -> Result<(), KeltaError> {
        self.rt.capture();
        self.code_host_of(&r.account)?.comment(r, body).await?;
        self.after_review_write(r).await;
        Ok(())
    }

    pub async fn review_request_changes(&self, r: &ReviewRef, body: &str) -> Result<(), KeltaError> {
        self.rt.capture();
        self.code_host_of(&r.account)?.request_changes(r, body).await?;
        self.after_review_write(r).await;
        Ok(())
    }

    // =========================================================================================
    // Subscriptions
    // =========================================================================================

    fn policy_for(&self, account: &AccountId, s: &Settings) -> IntervalPolicy {
        let acc = s.accounts.get(account);
        let floor = if acc.is_some_and(|a| a.kind == AccountKind::Redmine) { 60 } else { 0 };
        IntervalPolicy::from_settings(&s.polling, floor, acc.and_then(|a| a.poll_secs))
    }

    /// Recompute subscriptions: visible panes of the active tab + notification rules.
    pub(crate) fn resubscribe(&self) {
        let s = self.accounts_settings();
        let active = self.active_project();
        let contents: Vec<PaneContent> =
            self.layouts.lock().get(&active).map(crate::layout::visible_contents).unwrap_or_default();
        let mut want: BTreeMap<SubKey, IntervalPolicy> = BTreeMap::new();
        let tickets = |scope: &Scope, view: Option<&str>, want: &mut BTreeMap<SubKey, IntervalPolicy>| {
            for q in self.ticket_queries(scope, view) {
                if s.accounts.contains_key(&q.account) {
                    let p = self.policy_for(&q.account, &s);
                    want.insert(SubKey { account: q.account, query: q.cache_key }, p);
                }
            }
        };
        let reviews =
            |accounts: Vec<AccountId>, kinds: &[ReviewKind], want: &mut BTreeMap<SubKey, IntervalPolicy>| {
                for a in accounts {
                    if !s.accounts.get(&a).is_some_and(|x| x.kind.is_code_host()) {
                        continue;
                    }
                    for k in kinds {
                        want.insert(
                            SubKey { account: a.clone(), query: reviews_key(&a, *k) },
                            self.policy_for(&a, &s),
                        );
                    }
                }
            };
        let both = [ReviewKind::ReviewRequested, ReviewKind::Authored];
        for c in &contents {
            match c {
                PaneContent::Tickets { scope, view_id, .. } => tickets(scope, view_id.as_deref(), &mut want),
                PaneContent::Reviews { scope } => reviews(self.review_accounts(scope), &both, &mut want),
                PaneContent::Inbox => {
                    tickets(&Scope::All, None, &mut want);
                    reviews(self.review_accounts(&Scope::All), &both, &mut want);
                }
                _ => {}
            }
        }
        let n = &s.notifications;
        if n.enabled {
            let mut kinds = Vec::new();
            if n.review_requested {
                kinds.push(ReviewKind::ReviewRequested);
            }
            if n.ci_failed_mine || n.pr_approved || n.pr_changes_requested {
                kinds.push(ReviewKind::Authored);
            }
            if !kinds.is_empty() {
                // open projects' accounts + accounts bound to no project (§8.4)
                let open = self.open_projects();
                let (mut bound, mut live) = (HashSet::new(), HashSet::new());
                for (a, _, p) in self.review_bindings() {
                    if open.contains(&p) {
                        live.insert(a.clone());
                    }
                    bound.insert(a);
                }
                let accounts = self
                    .review_accounts(&Scope::All)
                    .into_iter()
                    .filter(|a| live.contains(a) || !bound.contains(a))
                    .collect();
                reviews(accounts, &kinds, &mut want);
            }
        }
        self.scheduler.set_subscriptions(want.into_iter().collect());
    }

    async fn refresh_key(&self, key: &SubKey) -> Result<(), KeltaError> {
        if key.query.starts_with("tickets:") {
            let q = self.feeds.views.lock().get(&key.query).cloned();
            let Some(q) = q else { return Ok(()) };
            self.fetch_tickets(&q, None).await?;
            self.emit_tickets_changed(&q.projects);
            return Ok(());
        }
        let kind =
            if key.query.ends_with(":authored") { ReviewKind::Authored } else { ReviewKind::ReviewRequested };
        let host = self.code_host_of(&key.account)?;
        let has_cache = self.cache_get::<Vec<Review>>(&reviews_key(&key.account, kind)).await.is_some();
        // The gate is stateful per account ("changed since the last call"): only the requested
        // feed may consume it, or the authored poll could swallow a new review request.
        // Authored always polls (CI / approvals do not move the notifications ETag anyway).
        if kind == ReviewKind::ReviewRequested
            && has_cache
            && self.feeds.primed.lock().contains(&key.account)
            && matches!(host.changed_since_last().await, Ok(false))
        {
            return Ok(());
        }
        self.fetch_reviews(&key.account, kind).await?;
        self.emit_reviews_changed(&key.account);
        Ok(())
    }
}

#[async_trait]
impl Refresher for Core {
    async fn refresh(&self, key: &SubKey) -> Result<(), KeltaError> {
        self.refresh_key(key).await
    }
}

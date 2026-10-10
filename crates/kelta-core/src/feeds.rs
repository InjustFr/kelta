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
    CiState, MyReviewState, PrLink, PrSource, PrState, Review, ReviewDecision, ReviewDetail, ReviewItem,
    ReviewKind, ReviewPage, ReviewQuery, ReviewRef,
};
use kelta_proto::error::{ErrorCode, KeltaError};
use kelta_proto::events::{AccountStatus, BusEvent, Notification, UiEvent, bus};
use kelta_proto::ext::Urgency;
use kelta_proto::ids::{AccountId, ProjectId, SessionId, WorkItemId};
use kelta_proto::model::{PaneContent, Scope, WorkItem, WorkState};
use kelta_proto::settings::{AccountKind, ColumnSpec, ProjectConfig, Settings, TrackerBinding, TrackerView};
use kelta_proto::store::{ProviderCacheRow, SeenReviewRow};
use kelta_proto::tracker::{
    AccountError, Assignee, Column, Cursor, Page, SourceHit, StatusCategory, Ticket, TicketDetail,
    TicketItem, TicketPage, TicketRef, Transition, Who,
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
    /// Unfinished work items with an open PR → their project (keeps Authored subscribed, B7).
    pr_items: Mutex<HashMap<WorkItemId, ProjectId>>,
    /// PRs whose merge / close was published in this process (live diff and check publish once).
    ended: Mutex<HashSet<ReviewRef>>,
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

/// PR number from its URL (GitHub `/pull/n`, GitLab `/merge_requests/n`, Gitea, Bitbucket).
fn pr_number(url: &str) -> Option<u64> {
    url.trim_end_matches('/').rsplit('/').next()?.parse().ok()
}

/// A review's `linked_tickets` badge names `key` (UI `sameKey`): case-insensitive; `#12` is
/// `repo#12` on forges and the bare `12` on Redmine.
fn same_key(key: &str, linked: &str, repo: &str) -> bool {
    let (k, l) = (key.to_lowercase(), linked.to_lowercase());
    k == l || (l.starts_with('#') && (k == format!("{}{l}", repo.to_lowercase()) || format!("#{k}") == l))
}

fn pr_link(r: &Review, source: PrSource) -> PrLink {
    PrLink {
        url: r.url.clone(),
        account: Some(r.r#ref.account.clone()),
        repo: r.r#ref.repo.clone(),
        number: r.r#ref.number,
        title: r.title.clone(),
        branch: r.source_branch.clone(),
        // The polled feeds only list open PRs.
        state: PrState::Open,
        draft: r.draft,
        ci: r.ci,
        review: r.decision,
        source,
    }
}

/// A ticket's PRs (TICKETS.md T1), no network: its work item's `pr_url` (enriched from the feed
/// when listed there), then open PRs of the polled feeds naming the ticket; de-duplicated by URL.
/// `bound` is the code-host `(account, repo)` of the work item's repo; `repos` are the code-host
/// repos of the ticket's projects, the only ones a bare-number key (Redmine `12` ~ `#12`) matches in.
pub fn ticket_prs(
    key: &str,
    work: Option<&WorkItem>,
    bound: Option<(AccountId, String)>,
    reviews: &[Review],
    repos: &[String],
) -> Vec<PrLink> {
    let mut out: Vec<PrLink> = Vec::new();
    if let Some(w) = work
        && let Some(url) = &w.pr_url
    {
        let mut link = match reviews.iter().find(|r| &r.url == url) {
            Some(r) => pr_link(r, PrSource::WorkItem),
            None => PrLink {
                url: url.clone(),
                account: bound.as_ref().map(|(a, _)| a.clone()),
                repo: bound.map(|(_, r)| r).unwrap_or_default(),
                number: pr_number(url).unwrap_or(0),
                title: w.title.clone().unwrap_or_default(),
                branch: w.branch.clone(),
                state: PrState::Open,
                draft: false,
                ci: CiState::None,
                review: None,
                source: PrSource::WorkItem,
            },
        };
        link.state = match w.state {
            WorkState::Merged { .. } => PrState::Merged,
            WorkState::PrClosed => PrState::Closed,
            _ => link.state,
        };
        out.push(link);
    }
    let bare = key.bytes().all(|b| b.is_ascii_digit());
    let in_scope = |r: &Review| !bare || repos.iter().any(|x| x.eq_ignore_ascii_case(&r.r#ref.repo));
    for r in reviews
        .iter()
        .filter(|r| in_scope(r) && r.linked_tickets.iter().any(|l| same_key(key, l, &r.r#ref.repo)))
    {
        if !out.iter().any(|l| l.url == r.url) {
            out.push(pr_link(r, PrSource::KeyMatch));
        }
    }
    out
}

/// Merge pages from several queries: de-dup by ref, union of project ids and view ids.
pub fn merge_tickets(
    pages: Vec<(Vec<Ticket>, Vec<ProjectId>, String)>,
    work: &HashMap<TicketRef, WorkItem>,
) -> Vec<TicketItem> {
    let mut out: Vec<TicketItem> = Vec::new();
    let mut index: HashMap<TicketRef, usize> = HashMap::new();
    for (tickets, projects, view_id) in pages {
        for t in tickets {
            match index.get(&t.r#ref) {
                Some(&i) => {
                    for p in &projects {
                        if !out[i].project_ids.contains(p) {
                            out[i].project_ids.push(p.clone());
                        }
                    }
                    if !out[i].view_ids.contains(&view_id) {
                        out[i].view_ids.push(view_id.clone());
                    }
                }
                None => {
                    index.insert(t.r#ref.clone(), out.len());
                    let work_item_id = work.get(&t.r#ref).map(|w| w.id.clone());
                    out.push(TicketItem {
                        ticket: t,
                        project_ids: projects.clone(),
                        work_item_id,
                        view_ids: vec![view_id.clone()],
                        ..TicketItem::default()
                    });
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

    pub(crate) fn tracker_of(&self, account: &AccountId) -> Result<Arc<dyn Tracker>, KeltaError> {
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

    /// The list queries behind a scope (one per distinct `(account, view)`). `view_id` None = every
    /// view of each project (the union). `who` overrides `view.who`.
    pub fn ticket_queries(&self, scope: &Scope, view_id: Option<&str>, who: Option<Who>) -> Vec<TicketQuery> {
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
            // shortcut: first page per source, add per-source Load more when needed
            // an unknown view id (source removed or renamed) falls back to the union
            let views: Vec<TrackerView> = match view_id.and_then(|v| b.views.iter().find(|x| x.id == v)) {
                Some(v) => vec![v.clone()],
                None => b.views.clone(),
            };
            for mut view in views {
                if who.is_some() {
                    view.who = who;
                }
                let account = view.account.clone().unwrap_or_else(|| b.account.clone());
                let key = tickets_key(&account, &view);
                match out.iter_mut().find(|q| q.cache_key == key) {
                    Some(q) if !q.projects.contains(&p.id) => q.projects.push(p.id.clone()),
                    Some(_) => {}
                    None => {
                        out.push(TicketQuery { account, view, projects: vec![p.id.clone()], cache_key: key })
                    }
                }
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
        // `Invalid` is a view the provider cannot answer (e.g. Unassigned without a repo), not account health
        if !matches!(&r, Err(e) if e.code == ErrorCode::InvalidArgument) {
            self.note_account(&q.account, &r);
        }
        let mut page = r?;
        if cursor.is_none() {
            // Open-only default lists never return done tickets: add the ones done in the last 7 days (Flow's Done).
            // shortcut: first page of the provider's closed list, page on when 7 days of closures exceed it.
            if q.view.status.is_none() {
                let closed = TrackerView { status: Some("closed".into()), ..q.view.clone() };
                let week = Duration::from_secs(7 * 86_400);
                let done = tracker.list(&closed, None).await.map(|p| p.items).unwrap_or_default();
                let fresh: Vec<Ticket> = done
                    .into_iter()
                    .filter(|t| t.status.category == StatusCategory::Done)
                    .filter(|t| age_of(t.status_since.as_deref().unwrap_or(&t.updated_at)) <= week)
                    .filter(|t| !page.items.iter().any(|o| o.r#ref == t.r#ref))
                    .collect();
                page.items.extend(fresh);
            }
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

    async fn work_by_ticket(&self) -> HashMap<TicketRef, WorkItem> {
        let items = self.store.call(|c| q::work_list(c, None)).await.unwrap_or_default();
        items
            .into_iter()
            .filter(|w| w.state != WorkState::Finished)
            .filter_map(|w| w.ticket.clone().map(|t| (t, w)))
            .collect()
    }

    /// Every review list in the provider cache (both kinds, all code-host accounts); never fetches.
    async fn cached_reviews(&self) -> Vec<Review> {
        let mut out = Vec::new();
        for a in self.review_accounts(&Scope::All) {
            for kind in [ReviewKind::Authored, ReviewKind::ReviewRequested] {
                if let Some((list, _)) = self.cache_get::<Vec<Review>>(&reviews_key(&a, kind)).await {
                    out.extend(list);
                }
            }
        }
        out
    }

    /// Core-filled ticket fields: linked PRs and the tracker's caps.
    fn enrich(&self, i: &mut TicketItem, work: &HashMap<TicketRef, WorkItem>, reviews: &[Review]) {
        let w = work.get(&i.ticket.r#ref);
        let repos = self.code_repos(&i.project_ids);
        i.prs = ticket_prs(&i.ticket.r#ref.key, w, w.and_then(|w| self.work_binding(w)), reviews, &repos);
        i.caps = self.tracker_of(&i.ticket.r#ref.account).map(|t| t.caps()).unwrap_or_default();
    }

    /// `tracker_list`.
    pub async fn tracker_list(
        &self,
        scope: Scope,
        view_id: Option<String>,
        who: Option<Who>,
        cursor: Option<Cursor>,
        refresh: bool,
    ) -> Result<TicketPage, KeltaError> {
        self.rt.capture();
        if let Scope::Project { id } = &scope
            && !self.project_exists(id)
        {
            return Err(KeltaError::not_found(format!("project {id}")));
        }
        let queries = self.ticket_queries(&scope, view_id.as_deref(), who);
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
                pages.push((p.items, q.projects.clone(), q.view.id.clone()));
            }
        }
        let work = self.work_by_ticket().await;
        let reviews = self.cached_reviews().await;
        let mut items = merge_tickets(pages, &work);
        for i in &mut items {
            self.enrich(i, &work, &reviews);
        }
        Ok(TicketPage { items, next, stale, errors })
    }

    /// `tracker_search` over the (cached) lists of a scope.
    pub async fn tracker_search(&self, scope: Scope, text: &str) -> Result<Vec<TicketItem>, KeltaError> {
        let page = self.tracker_list(scope.clone(), None, None, None, false).await?;
        let needle = text.trim().to_lowercase();
        let hits: Vec<TicketItem> = page
            .items
            .into_iter()
            .filter(|i| {
                needle.is_empty()
                    || i.ticket.r#ref.key.to_lowercase().contains(&needle)
                    || i.ticket.title.to_lowercase().contains(&needle)
            })
            .take(50)
            .collect();
        // a key outside every list (someone else's or nobody's ticket): resolve it by GET on each account
        let key = text.trim();
        if !hits.is_empty()
            || !key.contains(|c: char| c.is_ascii_digit())
            || key.contains(char::is_whitespace)
        {
            return Ok(hits);
        }
        let mut tried: Vec<AccountId> = Vec::new();
        for q in self.ticket_queries(&scope, None, None) {
            if tried.contains(&q.account) {
                continue;
            }
            tried.push(q.account.clone());
            let probe = TicketRef { account: q.account, key: key.into(), id: key.into() };
            if let Ok(d) = self.tracker_get(&probe).await {
                let work = self.work_by_ticket().await;
                return Ok(vec![TicketItem {
                    work_item_id: work.get(&d.ticket.r#ref).map(|w| w.id.clone()),
                    ticket: d.ticket,
                    project_ids: q.projects,
                    prs: d.prs,
                    caps: d.caps,
                    ..TicketItem::default()
                }]);
            }
        }
        Ok(hits)
    }

    /// `tracker_sources`: ticket sources of an account matching `query`.
    pub async fn tracker_sources(
        &self,
        account: &AccountId,
        query: &str,
    ) -> Result<Vec<SourceHit>, KeltaError> {
        self.rt.capture();
        // the core owns `view.account`: providers leave it None
        let r = self.tracker_of(account)?.sources(query).await.map(|mut hits| {
            for h in &mut hits {
                h.view.account = Some(account.clone());
            }
            hits
        });
        // `Unsupported` says nothing about the account's health
        if !matches!(&r, Err(e) if e.code == ErrorCode::Unsupported) {
            self.note_account(account, &r);
        }
        r
    }

    pub async fn tracker_get(&self, t: &TicketRef) -> Result<TicketDetail, KeltaError> {
        self.rt.capture();
        let r = self.tracker_of(&t.account)?.get(t).await;
        // a missing or malformed key says nothing about the account's health
        if !matches!(&r, Err(e) if matches!(e.code, ErrorCode::NotFound | ErrorCode::InvalidArgument)) {
            self.note_account(&t.account, &r);
        }
        let mut d = r?;
        let work = self.work_by_ticket().await;
        let w = work.get(&d.ticket.r#ref);
        let projects: Vec<ProjectId> =
            self.project_for_account(&t.account).map(|p| p.id.clone()).into_iter().collect();
        let repos = self.code_repos(&projects);
        d.prs =
            ticket_prs(&t.key, w, w.and_then(|w| self.work_binding(w)), &self.cached_reviews().await, &repos);
        d.caps = self.tracker_of(&t.account)?.caps();
        Ok(d)
    }

    /// The project whose binding or one of its views uses a ticket's account (active first, then open, then any).
    fn project_for_account(&self, account: &AccountId) -> Option<Arc<ProjectConfig>> {
        let all: Vec<Arc<ProjectConfig>> = self
            .project_configs()
            .into_iter()
            .filter(|p| {
                p.tracker.as_ref().is_some_and(|b| {
                    &b.account == account || b.views.iter().any(|v| v.account.as_ref() == Some(account))
                })
            })
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
        session: Option<&SessionId>,
    ) -> Result<kelta_proto::tracker::Ticket, KeltaError> {
        self.rt.capture();
        let tracker = self.tracker_of(&t.account)?;
        let from = self.cached_ticket(t).await.map(|x| x.status);
        let ticket = tracker.transition(t, transition_id, fields).await?;
        self.after_ticket_write(&ticket).await;
        self.publish_from(
            session,
            BusEvent::new(
                bus::TICKET_TRANSITIONED,
                serde_json::json!({ "ticket": t, "from": from, "to": ticket.status }),
            ),
        );
        Ok(ticket)
    }

    /// `tracker_move`: column → transition (by names, then categories); ambiguous → `Conflict`.
    /// `project` picks the columns; `None` = the project bound to the ticket's account.
    pub async fn tracker_move(
        &self,
        t: &TicketRef,
        column_id: &str,
        project: Option<&ProjectId>,
    ) -> Result<kelta_proto::tracker::Ticket, KeltaError> {
        self.rt.capture();
        let project = match project {
            Some(id) => self.cfg.project(id).ok_or_else(|| KeltaError::not_found(format!("project {id}")))?,
            None => self.project_for_account(&t.account).ok_or_else(|| {
                KeltaError::not_found(format!("no project uses tracker account {}", t.account))
            })?,
        };
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
        self.tracker_transition(t, &chosen.id, None, None).await
    }

    pub async fn tracker_comment(
        &self,
        t: &TicketRef,
        markdown: &str,
        session: Option<&SessionId>,
    ) -> Result<(), KeltaError> {
        self.rt.capture();
        self.tracker_of(&t.account)?.comment(t, markdown).await?;
        self.publish_from(
            session,
            BusEvent::new(bus::TICKET_COMMENTED, serde_json::json!({ "ticket": t, "markdown": markdown })),
        );
        Ok(())
    }

    /// Publish with the acting session (and its project) as event context.
    fn publish_from(&self, session: Option<&SessionId>, mut ev: BusEvent) {
        if let Some(sid) = session {
            ev = ev.with_session(sid.clone());
            if let Some(p) = self.sessions.lock().get(sid).map(|e| e.info.project_id.clone()) {
                ev = ev.with_project(p);
            }
        }
        self.publish_ev(ev);
    }

    pub async fn tracker_assign(
        &self,
        t: &TicketRef,
        who: Assignee,
    ) -> Result<kelta_proto::tracker::Ticket, KeltaError> {
        self.rt.capture();
        let ticket = self.tracker_of(&t.account)?.assign(t, who.clone()).await?;
        self.after_ticket_write(&ticket).await;
        self.publish_ev(BusEvent::new(
            bus::TICKET_ASSIGNED,
            serde_json::json!({ "ticket": t, "assignee": who }),
        ));
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

    /// After a write, before its bus event: patch cached pages and notify views (the event's
    /// [`Core::on_ticket_written`] refreshes the account; patching first keeps that fetch last).
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
    }

    /// `ticket.transitioned` / `ticket.assigned` from anywhere (`tracker_*`, the work saga,
    /// plugins): refresh the account's subscribed lists now instead of at the next poll.
    // shortcut: only subscribed lists refresh, upgrade = patch cached rows like tracker_*
    pub(crate) fn on_ticket_written(&self, payload: &serde_json::Value) {
        // a `TicketRef`, or a whole `Ticket` (plugin actions)
        let t = payload.get("ticket").map(|v| v.get("ref").unwrap_or(v));
        if let Some(Ok(t)) = t.cloned().map(serde_json::from_value::<TicketRef>) {
            self.scheduler.kick(Some(t.account));
        }
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
        // Authored always includes drafts: reviewing on GitHub means a draft PR (B6).
        let include_drafts = kind == ReviewKind::Authored || rs.include_drafts;
        let query = ReviewQuery { kind, include_team: rs.include_team_requests, include_drafts };
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
            ReviewKind::Authored => {
                for gone in self.diff_authored(account, &list) {
                    self.pr_left_open(&gone).await;
                }
                self.join_work_prs(account, &list).await;
            }
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
                .find(|(a, repo, _)| a == account && repo.eq_ignore_ascii_case(&r.r#ref.repo))
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
            .filter(|(a, repo, _)| {
                a == account && fresh.iter().any(|r| repo.eq_ignore_ascii_case(&r.r#ref.repo))
            })
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

    /// Authored PRs: CI / decision / head changes → `pr.*` (silent first poll). Returns the PRs
    /// that left the open list (merged or closed: the caller asks the host which).
    fn diff_authored(&self, account: &AccountId, list: &[Review]) -> Vec<ReviewRef> {
        let prev = {
            let mut m = self.feeds.authored.lock();
            m.insert(account.clone(), list.iter().map(|r| (r.r#ref.clone(), r.clone())).collect())
        };
        let Some(prev) = prev else { return Vec::new() };
        let gone = prev.keys().filter(|k| !list.iter().any(|r| &r.r#ref == *k)).cloned().collect();
        let bindings = self.review_bindings();
        for r in list {
            let Some(p) = prev.get(&r.r#ref) else { continue };
            let project = bindings
                .iter()
                .find(|(a, repo, _)| a == account && repo.eq_ignore_ascii_case(&r.r#ref.repo))
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
        gone
    }

    /// A PR is not in the authored open list: one `get` tells merged / closed, published once
    /// as `pr.merged` / `pr.closed` (kelta-work moves the work item, FLOW §4.6).
    async fn pr_left_open(&self, r: &ReviewRef) {
        if self.feeds.ended.lock().contains(r) {
            return;
        }
        let d = match self.review_get(r).await {
            Ok(d) => d,
            Err(e) => {
                tracing::warn!(pr = %format!("{}#{}", r.repo, r.number), error = %e.message, "PR state check failed");
                return;
            }
        };
        let name = match d.state {
            PrState::Merged => bus::PR_MERGED,
            PrState::Closed => bus::PR_CLOSED,
            PrState::Open => return,
        };
        if !self.feeds.ended.lock().insert(r.clone()) {
            return;
        }
        let mut ev = BusEvent::new(
            name,
            serde_json::json!({ "review": d.review, "linked_tickets": d.review.linked_tickets }),
        );
        if let Some((_, _, p)) =
            self.review_bindings().into_iter().find(|(a, repo, _)| a == &r.account && repo == &r.repo)
        {
            ev = ev.with_project(p);
        }
        self.publish_ev(ev);
    }

    /// The code-host repos of `projects`.
    fn code_repos(&self, projects: &[ProjectId]) -> Vec<String> {
        projects
            .iter()
            .filter_map(|id| self.cfg.project(id))
            .flat_map(|p| {
                p.repos
                    .iter()
                    .filter_map(|r| r.code_host.as_ref().map(|c| c.repo.clone()))
                    .collect::<Vec<_>>()
            })
            .collect()
    }

    /// `(account, repo)` of the code host bound to a work item's repo.
    fn work_binding(&self, w: &WorkItem) -> Option<(AccountId, String)> {
        let p = self.cfg.project(&w.project_id)?;
        let ch = p.repos.iter().find(|r| r.id == w.repo_id)?.code_host.clone()?;
        Some((ch.account, ch.repo))
    }

    /// Branch join (FLOW §3.1): an authored PR whose head is an active item's branch is that
    /// item's PR (Claude ran `gh pr create`, or the web UI): `pr_url` backfill + `on_pr`, once.
    async fn join_work_prs(&self, account: &AccountId, list: &[Review]) {
        let items = self.store.call(|c| q::work_list(c, None)).await.unwrap_or_default();
        for w in items.iter().filter(|w| w.pr_url.is_none() && w.state == WorkState::Active) {
            let Some((acc, repo)) = self.work_binding(w) else { continue };
            let hit = list.iter().find(|r| {
                &acc == account && r.r#ref.repo.eq_ignore_ascii_case(&repo) && r.source_branch == w.branch
            });
            if let Some(r) = hit
                && let Err(e) = self.work.link_pr(&w.id, r).await
            {
                tracing::warn!(work = %w.id, error = %e.message, "PR not linked to its work item");
            }
        }
    }

    /// Keeps the set of unfinished work items with an open PR (from `work.updated`); a change of
    /// their projects re-subscribes Authored.
    pub(crate) fn note_work_pr(&self, w: &WorkItem) {
        let open = w.pr_url.is_some() && w.state != WorkState::Finished && !w.state.pr_done();
        let changed = {
            let mut m = self.feeds.pr_items.lock();
            if open {
                m.insert(w.id.clone(), w.project_id.clone()).as_ref() != Some(&w.project_id)
            } else {
                m.remove(&w.id).is_some()
            }
        };
        if changed {
            self.resubscribe();
        }
    }

    /// Startup and Now open (FLOW §3.6): every unfinished work item whose PR is not in its
    /// account's authored open list gets one `get`, so a merge while Kelta was closed is seen.
    pub async fn check_work_prs(&self) -> Result<(), KeltaError> {
        self.rt.capture();
        self.work.ensure_listener();
        let items = self.store.call(|c| q::work_list(c, None)).await?;
        let mut open: HashMap<AccountId, Vec<Review>> = HashMap::new();
        for w in &items {
            self.note_work_pr(w);
        }
        for w in items.iter().filter(|w| w.state != WorkState::Finished && !w.state.pr_done()) {
            let (Some(url), Some((account, repo))) = (&w.pr_url, self.work_binding(w)) else { continue };
            if !open.contains_key(&account) {
                match self.fetch_reviews(&account, ReviewKind::Authored).await {
                    Ok(list) => open.insert(account.clone(), list),
                    Err(_) => continue, // account error already noted; the next check retries
                };
            }
            if open.get(&account).is_some_and(|l| l.iter().any(|r| &r.url == url)) {
                continue;
            }
            // shortcut: the PR number is the last URL segment (GitHub /pull/n, GitLab /merge_requests/n).
            let Some(number) = pr_number(url) else { continue };
            self.pr_left_open(&ReviewRef { account, repo, number }).await;
        }
        Ok(())
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
        let stamps = self.reviewed_stamps().await;
        all.iter_mut().for_each(|r| fill_reviewed_head(r, &stamps));
        Ok(ReviewPage { items: merge_reviews(all, &bindings), stale, errors })
    }

    pub async fn review_get(&self, r: &ReviewRef) -> Result<ReviewDetail, KeltaError> {
        self.rt.capture();
        let mut res = self.code_host_of(&r.account)?.get(r).await;
        self.note_account(&r.account, &res);
        if let Ok(d) = &mut res {
            fill_reviewed_head(&mut d.review, &self.reviewed_stamps().await);
        }
        res
    }

    /// Heads Louis approved in Kelta (`seen_reviews.reviewed_sha`).
    async fn reviewed_stamps(&self) -> HashMap<(String, String, u64), String> {
        self.store.call(|c| q::reviewed_shas(c)).await.unwrap_or_default()
    }

    async fn after_review_write(&self, r: &ReviewRef) {
        self.emit_reviews_changed(&r.account);
        self.scheduler.kick(Some(r.account.clone()));
    }

    pub async fn review_approve(&self, r: &ReviewRef, head_sha: &str) -> Result<(), KeltaError> {
        self.rt.capture();
        self.code_host_of(&r.account)?.approve(r, head_sha).await?;
        let (rr, sha) = (r.clone(), head_sha.to_owned());
        if let Err(e) = self.store.call(move |c| q::seen_review_stamp(c, &rr, &sha)).await {
            tracing::warn!(error = %e.message, "reviewed head not stamped");
        }
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
        // Bitbucket has no change gate and review polling costs ~50 requests (limit ~1000/h).
        let floor = match acc.map(|a| a.kind) {
            Some(AccountKind::Redmine) => 60,
            Some(AccountKind::Bitbucket) => 300,
            _ => 0,
        };
        IntervalPolicy::from_settings(&s.polling, floor, acc.and_then(|a| a.poll_secs))
    }

    /// Recompute subscriptions: visible panes of the active tab + notification rules.
    pub(crate) fn resubscribe(&self) {
        let s = self.accounts_settings();
        let active = self.active_project();
        let contents: Vec<PaneContent> =
            self.layouts.lock().get(&active).map(crate::layout::visible_contents).unwrap_or_default();
        let mut want: BTreeMap<SubKey, IntervalPolicy> = BTreeMap::new();
        let tickets = |scope: &Scope,
                       view: Option<&str>,
                       who: Option<Who>,
                       want: &mut BTreeMap<SubKey, IntervalPolicy>| {
            for q in self.ticket_queries(scope, view, who) {
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
                PaneContent::Tickets { scope, view_id, who, .. } => {
                    tickets(scope, view_id.as_deref(), *who, &mut want)
                }
                PaneContent::Reviews { scope } => reviews(self.review_accounts(scope), &both, &mut want),
                PaneContent::Inbox => {
                    // Now reads `tracker_list(All, None, Mine)`: poll that cache key
                    tickets(&Scope::All, None, Some(Who::Mine), &mut want);
                    reviews(self.review_accounts(&Scope::All), &both, &mut want);
                }
                _ => {}
            }
        }
        // Own work-item PRs stay visible whatever panes are shown (B7).
        let pr_projects: HashSet<ProjectId> = self.feeds.pr_items.lock().values().cloned().collect();
        let pr_accounts =
            self.review_bindings().into_iter().filter(|(_, _, p)| pr_projects.contains(p)).map(|(a, _, _)| a);
        reviews(pr_accounts.collect(), &[ReviewKind::Authored], &mut want);
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

/// Hosts that report no `reviewed_head` (GitLab, Bitbucket, Gitea) read the head Louis approved in
/// Kelta, so "Updated since your review" works there too.
/// shortcut: a review made on the host's website does not move the stamp; stamp other review kinds
/// (comment, request changes) too if that shows up.
fn fill_reviewed_head(r: &mut Review, stamps: &HashMap<(String, String, u64), String>) {
    if r.reviewed_head.is_none() {
        let k = (r.r#ref.account.as_str().to_owned(), r.r#ref.repo.clone(), r.r#ref.number);
        r.reviewed_head = stamps.get(&k).cloned();
    }
}

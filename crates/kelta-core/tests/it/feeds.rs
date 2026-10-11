//! Tickets/reviews: Scope::All de-dup + project tagging + "Other", provider_cache
//! stale-while-revalidate, per-account errors, seen_reviews first-poll silence, authored PR
//! change events, tracker_move resolution.

use std::sync::Arc;

use crate::common::*;
use kelta_core::store::q;
use kelta_proto::ErrorCode;
use kelta_proto::api::{CodeHost, CoreApi, Tracker};
use kelta_proto::codehost::{CiState, PrSource, PrState, Review, ReviewKind, ReviewRef};
use kelta_proto::events::{BusEvent, UiEvent, bus};
use kelta_proto::ids::{AccountId, ProjectId};
use kelta_proto::ipc::WindowState;
use kelta_proto::model::{Scope, WorkItem, WorkKind, WorkState};
use kelta_proto::settings::{AccountKind, CodeHostBinding, ColumnSpec, ProjectConfig, Settings, TrackerView};
use kelta_proto::store::ProviderCacheRow;
use kelta_proto::testing::{FakeTerminalHost, FakeTracker};
use kelta_proto::tracker::{Assignee, StatusCategory, TicketRef, WaitWhy, Who};

fn settings() -> Settings {
    let mut s = Settings::defaults();
    s.accounts.insert("jira-acme".into(), account(AccountKind::Jira));
    s.accounts.insert("github-work".into(), account(AccountKind::Github));
    s
}

fn projects(root: &std::path::Path) -> Vec<ProjectConfig> {
    let mut shop = project("shop", root);
    shop.tracker = Some(kelta_proto::samples::tracker_binding());
    shop.repos[0].code_host =
        Some(CodeHostBinding { account: "github-work".into(), repo: "acme/shop-api".into() });
    let mut blog = project("blog", root);
    let mut b = kelta_proto::samples::tracker_binding();
    b.columns.clear();
    blog.tracker = Some(b);
    vec![shop, blog]
}

fn review(repo: &str, number: u64, kind: ReviewKind) -> Review {
    let mut r = kelta_proto::samples::review();
    r.r#ref = ReviewRef { account: "github-work".into(), repo: repo.into(), number };
    r.kind = kind;
    r
}

struct Env {
    h: H,
    tracker: Arc<FakeTracker>,
    host: Arc<ListHost>,
    factory: Arc<Factory>,
}

fn env(root: &std::path::Path, reviews: Vec<Review>) -> Env {
    env_with(root, ListHost::new(reviews))
}

fn env_with(root: &std::path::Path, host: Arc<ListHost>) -> Env {
    env_full(root, host, FakeTracker::new())
}

fn env_full(root: &std::path::Path, host: Arc<ListHost>, tracker: FakeTracker) -> Env {
    env_in(root, host, tracker, projects(root))
}

fn env_in(
    root: &std::path::Path,
    host: Arc<ListHost>,
    tracker: FakeTracker,
    projects: Vec<ProjectConfig>,
) -> Env {
    let tracker = Arc::new(tracker);
    let factory = Arc::new(Factory::default());
    factory.trackers.lock().insert(AccountId::new("jira-acme"), tracker.clone() as Arc<dyn Tracker>);
    factory.hosts.lock().insert(AccountId::new("github-work"), host.clone() as Arc<dyn CodeHost>);
    let h = start_in(root, MemConfig::new(settings(), projects), FakeTerminalHost::new(), factory.clone());
    h.core.project_open(&ProjectId::new("shop")).unwrap();
    h.core.project_open(&ProjectId::new("blog")).unwrap();
    Env { h, tracker, host, factory }
}

fn bus_names(rx: &mut tokio::sync::broadcast::Receiver<BusEvent>) -> Vec<BusEvent> {
    let mut v = Vec::new();
    while let Ok(e) = rx.try_recv() {
        v.push(e);
    }
    v
}

#[tokio::test]
async fn all_scope_dedups_and_tags_projects() {
    let tmp = tempfile::tempdir().unwrap();
    let e = env(
        tmp.path(),
        vec![
            review("acme/shop-api", 87, ReviewKind::ReviewRequested),
            review("acme/unbound", 5, ReviewKind::ReviewRequested),
        ],
    );
    let page = e.h.core.tracker_list(Scope::All, None, None, None, None, true).await.unwrap();
    // shop and blog share the same view → one provider call
    assert_eq!(e.tracker.calls().iter().filter(|c| c.starts_with("list:")).count(), 1);
    assert_eq!(page.items.len(), 3);
    for item in &page.items {
        assert_eq!(item.project_ids, vec![ProjectId::new("shop"), ProjectId::new("blog")]);
    }
    assert!(page.errors.is_empty() && !page.stale);

    let all = e.h.core.review_page(Scope::All, ReviewKind::ReviewRequested, true).await.unwrap();
    assert_eq!(all.items.len(), 2);
    let bound = all.items.iter().find(|i| i.review.r#ref.number == 87).unwrap();
    assert_eq!(bound.project_ids, vec![ProjectId::new("shop")]);
    let other = all.items.iter().find(|i| i.review.r#ref.number == 5).unwrap();
    assert!(other.project_ids.is_empty(), "unbound repos land in Other");
    let shop =
        e.h.core
            .review_page(Scope::Project { id: "shop".into() }, ReviewKind::ReviewRequested, false)
            .await
            .unwrap();
    assert_eq!(shop.items.len(), 1);
    // linked tickets from branch + title
    assert_eq!(bound.review.linked_tickets, vec!["SHOP-140"]);
}

#[tokio::test]
async fn default_lists_add_tickets_done_this_week() {
    let tmp = tempfile::tempdir().unwrap();
    let base = FakeTracker::new();
    let done = |key: &str, since: String| {
        let mut d = base.ticket("SHOP-143").unwrap();
        d.ticket.r#ref.key = key.into();
        d.ticket.status = kelta_proto::samples::status("5", "Done", StatusCategory::Done);
        d.ticket.status_since = Some(since);
        d
    };
    let fake = FakeTracker::with_tickets(vec![
        base.ticket("SHOP-141").unwrap(),
        base.ticket("SHOP-142").unwrap(),
        done("SHOP-143", kelta_proto::now_rfc3339()),
        done("SHOP-144", "2020-01-01T00:00:00Z".into()),
    ])
    .with_page_size(2);
    let e = env_full(tmp.path(), ListHost::new(vec![]), fake);
    let shop = Scope::Project { id: "shop".into() };
    let page = e.h.core.tracker_list(shop.clone(), None, None, None, None, true).await.unwrap();
    let keys: Vec<&str> = page.items.iter().map(|i| i.ticket.r#ref.key.as_str()).collect();
    assert_eq!(keys, ["SHOP-141", "SHOP-142", "SHOP-143"], "open first page + done in the last 7 days");

    // the closed list fails: Done keeps the last page's tickets instead of going empty
    e.tracker.pass_next();
    e.tracker.fail_next(kelta_proto::KeltaError::upstream("502"));
    let page = e.h.core.tracker_list(shop, None, None, None, None, true).await.unwrap();
    let keys: Vec<&str> = page.items.iter().map(|i| i.ticket.r#ref.key.as_str()).collect();
    assert_eq!(keys, ["SHOP-141", "SHOP-142", "SHOP-143"]);
    assert_eq!(e.tracker.calls().iter().filter(|c| c.starts_with("list-closed:")).count(), 2);
}

#[tokio::test]
async fn cache_serves_first_then_revalidates_when_stale() {
    let tmp = tempfile::tempdir().unwrap();
    let e = env(tmp.path(), vec![]);
    let shop = Scope::Project { id: "shop".into() };
    let lists = || e.tracker.calls().iter().filter(|c| c.starts_with("list:")).count();
    let p1 = e.h.core.tracker_list(shop.clone(), None, None, None, None, false).await.unwrap();
    assert_eq!((lists(), p1.stale), (1, false));
    // fresh cache: no network
    let p2 = e.h.core.tracker_list(shop.clone(), None, None, None, None, false).await.unwrap();
    assert_eq!((lists(), p2.stale, p2.items.len()), (1, false, 3));
    // age the cache row → served stale + background refresh + tickets.changed
    let key = e.h.core.ticket_queries(&shop, None, None)[0].cache_key.clone();
    let row = e.h.core.store().call(move |c| q::cache_get(c, &key)).await.unwrap().unwrap();
    let old = ProviderCacheRow { fetched_at: "2020-01-01T00:00:00Z".into(), ..row };
    e.h.core.store().call(move |c| q::cache_put(c, &old)).await.unwrap();
    e.h.ui.take_events();
    let p3 = e.h.core.tracker_list(shop.clone(), None, None, None, None, false).await.unwrap();
    assert!(p3.stale);
    for _ in 0..50 {
        settle().await;
        if lists() == 2 {
            break;
        }
    }
    assert_eq!(lists(), 2);
    settle().await;
    assert!(e.h.ui.event_names().contains(&"tickets.changed"));

    // a failing account keeps serving the cache and reports the error
    e.tracker.fail_next(kelta_proto::KeltaError::needs_auth("401"));
    let p4 = e.h.core.tracker_list(shop, None, None, None, None, true).await.unwrap();
    assert!(p4.stale);
    assert_eq!(p4.items.len(), 3);
    assert_eq!(p4.errors[0].account_id.as_str(), "jira-acme");
    assert_eq!(p4.errors[0].error.code, ErrorCode::NeedsAuth);
    assert!(e.h.ui.events().iter().any(|ev| matches!(
        ev,
        UiEvent::AccountStatusChanged { status: kelta_proto::events::AccountStatus::NeedsAuth, .. }
    )));
}

/// Backdate a cache row so it reads `secs` old.
async fn age_cache(e: &Env, key: &str, secs: i64) {
    let key = key.to_owned();
    let row = e.h.core.store().call(move |c| q::cache_get(c, &key)).await.unwrap().unwrap();
    let at = time::OffsetDateTime::now_utc() - time::Duration::seconds(secs);
    let fetched_at = at.format(&time::format_description::well_known::Rfc3339).unwrap();
    e.h.core.store().call(move |c| q::cache_put(c, &ProviderCacheRow { fetched_at, ..row })).await.unwrap();
}

#[tokio::test]
async fn revalidation_follows_the_polling_policy() {
    let tmp = tempfile::tempdir().unwrap();
    let e = env(tmp.path(), vec![]);
    let shop = Scope::Project { id: "shop".into() };
    let lists = || e.tracker.calls().iter().filter(|c| c.starts_with("list:")).count();
    let key = e.h.core.ticket_queries(&shop, None, None)[0].cache_key.clone();
    let load = || e.h.core.tracker_list(shop.clone(), None, None, None, None, false);
    load().await.unwrap();
    let n = lists();

    // background (600 s): a tickets.changed reload of a 5 min old list stays on the cache
    e.h.ui.set_window_state(WindowState { exists: true, visible: true, focused: false });
    age_cache(&e, &key, 300).await;
    assert!(!load().await.unwrap().stale);
    settle().await;
    assert_eq!(lists(), n, "no provider fetch for a list younger than the background interval");
    age_cache(&e, &key, 700).await;
    assert!(load().await.unwrap().stale, "older than the background interval: revalidates");

    // focused (120 s), `poll_secs` 600 wins
    e.h.ui.set_window_state(WindowState { exists: true, visible: true, focused: true });
    e.h.cfg.update(|s| s.accounts.get_mut("jira-acme").unwrap().poll_secs = Some(600));
    age_cache(&e, &key, 300).await;
    assert!(!load().await.unwrap().stale, "poll_secs respected");

    // focused 30 s, the Redmine floor (60 s) wins
    e.h.cfg.update(|s| {
        s.polling.focused_secs = 30;
        let a = s.accounts.get_mut("jira-acme").unwrap();
        a.poll_secs = None;
        a.kind = AccountKind::Redmine;
    });
    age_cache(&e, &key, 45).await;
    assert!(!load().await.unwrap().stale, "Redmine floor respected");
    age_cache(&e, &key, 90).await;
    assert!(load().await.unwrap().stale);
}

#[tokio::test]
async fn first_review_poll_is_silent() {
    let tmp = tempfile::tempdir().unwrap();
    {
        let e = env(tmp.path(), vec![review("acme/shop-api", 87, ReviewKind::ReviewRequested)]);
        e.h.ui.set_window_state(WindowState { exists: true, visible: true, focused: false });
        let mut rx = e.h.core.subscribe();
        e.h.core.review_page(Scope::All, ReviewKind::ReviewRequested, true).await.unwrap();
        assert!(bus_names(&mut rx).iter().all(|ev| ev.name != "pr.review_requested"));
        assert!(e.h.ui.notifications().is_empty());

        // a new request on the next poll fires once (host spells the repo with another case)
        e.host.reviews.lock().push(review("Acme/Shop-API", 88, ReviewKind::ReviewRequested));
        e.h.core.review_page(Scope::All, ReviewKind::ReviewRequested, true).await.unwrap();
        let evs: Vec<BusEvent> =
            bus_names(&mut rx).into_iter().filter(|ev| ev.name == "pr.review_requested").collect();
        assert_eq!(evs.len(), 1);
        assert_eq!(evs[0].payload["review"]["ref"]["number"], 88);
        assert_eq!(evs[0].project_id.as_ref().map(|p| p.as_str()), Some("shop"));
        assert_eq!(e.h.ui.notifications().len(), 1);
        assert!(e.h.ui.events().iter().any(|ev| matches!(ev, UiEvent::ReviewsChanged { new_keys, .. } if new_keys.len() == 1 && new_keys[0].number == 88)));
        assert!(e.h.ui.events().iter().any(|ev| matches!(ev, UiEvent::ReviewsChanged { scope: Scope::Project { id }, .. } if id.as_str() == "shop")));
        // nothing new → nothing fired
        e.h.core.review_page(Scope::All, ReviewKind::ReviewRequested, true).await.unwrap();
        assert!(bus_names(&mut rx).iter().all(|ev| ev.name != "pr.review_requested"));
        e.h.core.store().flush().await.unwrap();
    }
    // next start: #89 appeared while closed → the first poll fills silently again
    let e = env(
        tmp.path(),
        vec![
            review("acme/shop-api", 87, ReviewKind::ReviewRequested),
            review("Acme/Shop-API", 88, ReviewKind::ReviewRequested),
            review("acme/shop-api", 89, ReviewKind::ReviewRequested),
        ],
    );
    e.h.ui.set_window_state(WindowState { exists: true, visible: true, focused: false });
    let mut rx = e.h.core.subscribe();
    e.h.core.review_page(Scope::All, ReviewKind::ReviewRequested, true).await.unwrap();
    assert!(bus_names(&mut rx).iter().all(|ev| ev.name != "pr.review_requested"));
    assert!(e.h.ui.notifications().is_empty());
    let seen = e.h.core.store().call(|c| q::seen_reviews(c, "github-work")).await.unwrap();
    assert_eq!(seen.len(), 3);
}

#[tokio::test]
async fn authored_changes_emit_pr_events() {
    let tmp = tempfile::tempdir().unwrap();
    let mut mine = review("acme/shop-api", 90, ReviewKind::Authored);
    mine.ci = CiState::Pending;
    mine.decision = None;
    let e = env(tmp.path(), vec![mine.clone()]);
    e.h.ui.set_window_state(WindowState { exists: true, visible: true, focused: false });
    let mut rx = e.h.core.subscribe();
    e.h.core.review_page(Scope::All, ReviewKind::Authored, true).await.unwrap();
    assert!(bus_names(&mut rx).iter().all(|ev| !ev.name.starts_with("pr.")));
    {
        let mut l = e.host.reviews.lock();
        l[0].ci = CiState::Error;
        l[0].decision = Some(kelta_proto::codehost::ReviewDecision::Approved);
    }
    e.h.core.review_page(Scope::All, ReviewKind::Authored, true).await.unwrap();
    let pr_names = |rx: &mut _| -> Vec<String> {
        bus_names(rx).into_iter().map(|ev| ev.name).filter(|n| n.starts_with("pr.")).collect()
    };
    assert_eq!(pr_names(&mut rx), vec!["pr.ci_changed", "pr.approved", "pr.updated"]);
    assert_eq!(e.h.ui.notifications().len(), 2, "ci failed (errored rollup) + approved");
    e.host.reviews.lock()[0].ci = CiState::Failure;
    e.h.core.review_page(Scope::All, ReviewKind::Authored, true).await.unwrap();
    assert_eq!(pr_names(&mut rx), vec!["pr.ci_changed", "pr.updated"]);
    assert_eq!(e.h.ui.notifications().len(), 3, "error → failure notifies again");
    e.h.core.review_page(Scope::All, ReviewKind::Authored, true).await.unwrap();
    assert!(pr_names(&mut rx).is_empty(), "failure → failure on the same head publishes nothing");
    assert_eq!(e.h.ui.notifications().len(), 3);
}

#[tokio::test]
async fn who_override_view_account_and_sources() {
    let tmp = tempfile::tempdir().unwrap();
    let e = env(tmp.path(), vec![]);
    let shop = Scope::Project { id: "shop".into() };
    let key = |who| e.h.core.ticket_queries(&shop, None, who)[0].cache_key.clone();
    assert_ne!(key(None), key(Some(Who::Mine)));
    assert_ne!(key(Some(Who::Mine)), key(Some(Who::Unassigned)));
    let t = e.tracker.ticket("SHOP-142").unwrap().ticket.r#ref;
    e.tracker.assign(&t, Assignee::None).await.unwrap();
    let page =
        e.h.core.tracker_list(shop.clone(), None, None, Some(Who::Unassigned), None, true).await.unwrap();
    assert_eq!(page.items.iter().map(|i| i.ticket.r#ref.key.as_str()).collect::<Vec<_>>(), ["SHOP-142"]);
    assert_eq!(page.items[0].view_ids, ["mine"]);
    assert_eq!(
        e.h.core.tracker_list(shop.clone(), None, None, None, None, true).await.unwrap().items.len(),
        3
    );
    let jira = AccountId::new("jira-acme");
    let hits = e.h.core.tracker_sources(&jira, "sh").await.unwrap();
    assert_eq!(hits.len(), 2);
    assert!(hits.iter().all(|h| h.view.account.as_ref() == Some(&jira)), "the core stamps the account");
    // a provider without discovery leaves the account status alone
    e.tracker.fail_next(kelta_proto::KeltaError::unsupported("no sources"));
    assert_eq!(e.h.core.tracker_sources(&jira, "sh").await.unwrap_err().code, ErrorCode::Unsupported);
    assert!(!e.h.ui.events().iter().any(|ev| matches!(ev, UiEvent::AccountStatusChanged { .. })));
    // a view's own account wins over the binding's, and move finds the project through it
    {
        let mut ps = e.h.cfg.projects.write();
        let mut p = (*ps[0]).clone();
        let b = p.tracker.as_mut().unwrap();
        b.account = "github-work".into();
        b.views[0].account = Some(jira.clone());
        ps[0] = Arc::new(p);
        let mut blog = (*ps[1]).clone();
        blog.tracker = None;
        ps[1] = Arc::new(blog);
    }
    assert_eq!(e.h.core.ticket_queries(&shop, None, None)[0].account, jira);
    e.h.core.tracker_move(&t, "todo", None).await.unwrap();
}

#[tokio::test]
async fn transitions_are_cached_until_a_move() {
    let tmp = tempfile::tempdir().unwrap();
    let e = env(tmp.path(), vec![]);
    let shop = Scope::Project { id: "shop".into() };
    e.h.core.tracker_list(shop, None, None, None, None, false).await.unwrap();
    let t = e.tracker.ticket("SHOP-142").unwrap().ticket.r#ref;
    let fetches = || e.tracker.calls().iter().filter(|c| c.starts_with("transitions:")).count();
    let first = e.h.core.tracker_transitions(&t).await.unwrap();
    assert_eq!(e.h.core.tracker_transitions(&t).await.unwrap(), first);
    assert_eq!(fetches(), 1, "a reopened picker within the interval makes no tracker call");
    e.h.core.tracker_transition(&t, &first[0].id, None, None).await.unwrap();
    let after = e.h.core.tracker_transitions(&t).await.unwrap();
    assert_eq!(fetches(), 2, "a move invalidates the ticket's transitions");
    assert_ne!(after, first);
}

#[tokio::test]
async fn tracker_move_resolves_columns() {
    let tmp = tempfile::tempdir().unwrap();
    let e = env(tmp.path(), vec![]);
    // shop overrides the board columns
    {
        let mut ps = e.h.cfg.projects.write();
        let mut shop = (*ps[0]).clone();
        let b = shop.tracker.as_mut().unwrap();
        b.columns = vec![
            ColumnSpec {
                id: "todo".into(),
                label: "To do".into(),
                categories: vec![StatusCategory::Todo],
                names: vec![],
            },
            ColumnSpec {
                id: "late".into(),
                label: "Late".into(),
                categories: vec![StatusCategory::InReview, StatusCategory::Done],
                names: vec![],
            },
            ColumnSpec {
                id: "named".into(),
                label: "Named".into(),
                categories: vec![StatusCategory::Done],
                names: vec!["In Review".into()],
            },
            ColumnSpec {
                id: "weird".into(),
                label: "Weird".into(),
                categories: vec![],
                names: vec!["Nope".into()],
            },
        ];
        ps[0] = Arc::new(shop);
    }
    e.h.core.project_activate(&ProjectId::new("shop")).unwrap();
    let t = e.tracker.ticket("SHOP-142").unwrap().ticket.r#ref;
    // the boards still on disk as `[project.tracker]` are sources (read path)
    let source_of = |p: &str| {
        e.h.core.sources().into_iter().find(|s| s.projects.contains(&ProjectId::new(p))).unwrap().id
    };
    let (shop_src, blog_src) = (source_of("shop"), source_of("blog"));
    let cols = e.h.core.tracker_columns(&shop_src).await.unwrap();
    assert_eq!(
        cols.iter().map(|c| c.id.as_str()).collect::<Vec<_>>(),
        vec!["todo", "late", "named", "weird"]
    );

    let err = e.h.core.tracker_move(&t, "late", None).await.unwrap_err();
    assert_eq!(err.code, ErrorCode::Conflict);
    assert_eq!(err.detail.unwrap()["candidates"].as_array().unwrap().len(), 2);
    let err = e.h.core.tracker_move(&t, "weird", None).await.unwrap_err();
    assert_eq!(err.code, ErrorCode::NotFound);
    assert!(err.detail.unwrap()["url"].as_str().unwrap().ends_with("SHOP-142"));

    let mut rx = e.h.core.subscribe();
    // names win over categories
    let moved = e.h.core.tracker_move(&t, "named", None).await.unwrap();
    assert_eq!(moved.status.name, "In Review");
    // two sources on one account: an explicit source picks its own columns (blog's has no
    // `late`; shop's has no provider column `3` = In Progress)
    let err = e.h.core.tracker_move(&t, "late", Some(&blog_src)).await.unwrap_err();
    assert_eq!(err.code, ErrorCode::NotFound);
    assert_eq!(e.h.core.tracker_move(&t, "3", None).await.unwrap_err().code, ErrorCode::NotFound);
    let started = e.h.core.tracker_move(&t, "3", Some(&blog_src)).await.unwrap();
    assert_eq!(started.status.category, StatusCategory::InProgress);
    let moved = e.h.core.tracker_move(&t, "todo", None).await.unwrap();
    assert_eq!(moved.status.category, StatusCategory::Todo);
    assert!(bus_names(&mut rx).iter().any(|ev| ev.name == "ticket.transitioned"));
    // blog (no override) uses the provider's columns
    let blog_cols = e.h.core.tracker_columns(&blog_src).await.unwrap();
    assert_eq!(blog_cols.len(), 4);
    assert_eq!(e.h.core.tracker_columns("nope").await.unwrap_err().code, ErrorCode::NotFound);
    // NeedsFields propagates with its detail
    e.tracker.require_fields("t5");
    let err = e.h.core.tracker_transition(&t, "t5", None, None).await.unwrap_err();
    assert_eq!(err.code, ErrorCode::NeedsFields);
}

#[tokio::test]
async fn unchanged_gate_skips_requested_but_authored_still_polls() {
    use kelta_core::feeds::reviews_key;
    use kelta_core::scheduler::{Refresher, SubKey};
    let tmp = tempfile::tempdir().unwrap();
    let e = env(tmp.path(), vec![review("acme/shop-api", 87, ReviewKind::ReviewRequested)]);
    for k in [ReviewKind::ReviewRequested, ReviewKind::Authored] {
        e.h.core.review_page(Scope::All, k, true).await.unwrap();
    }
    *e.host.changed.lock() = false;
    *e.host.calls.lock() = 0;
    let acc = AccountId::new("github-work");
    let sub = |k| SubKey { account: acc.clone(), query: reviews_key(&acc, k), review: Some(k) };
    e.h.core.refresh(&sub(ReviewKind::ReviewRequested)).await.unwrap();
    assert_eq!(*e.host.calls.lock(), 0, "gate says unchanged");
    e.h.core.refresh(&sub(ReviewKind::Authored)).await.unwrap();
    assert_eq!(*e.host.calls.lock(), 1, "authored never consumes the gate");
    e.h.core.refresh(&sub(ReviewKind::Team)).await.unwrap();
    assert_eq!(*e.host.calls.lock(), 2, "team never reads the gate either");
    e.h.core.refresh(&sub(ReviewKind::ReviewRequested)).await.unwrap();
    assert_eq!(*e.host.calls.lock(), 2, "and leaves it to the requested feed");
}

/// A ticket-less work item on shop's bound repo.
fn work_item(branch: &str, pr_url: Option<&str>) -> WorkItem {
    WorkItem {
        repo_id: "main".into(),
        kind: WorkKind::Branch,
        ticket: None,
        branch: branch.into(),
        pr_url: pr_url.map(str::to_owned),
        state: if pr_url.is_some() { WorkState::PrOpen } else { WorkState::Active },
        session_ids: vec![],
        ..kelta_proto::samples::work_item()
    }
}

async fn wait_work(e: &Env, pred: impl Fn(&WorkItem) -> bool) -> WorkItem {
    for _ in 0..200 {
        if let Some(w) = e.h.core.work().list(None).await.unwrap().into_iter().find(|w| pred(w)) {
            return w;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    panic!("work item never reached the expected state");
}

#[tokio::test]
async fn authored_always_includes_drafts_requested_follows_the_setting() {
    let tmp = tempfile::tempdir().unwrap();
    let mut mine = review("acme/shop-api", 90, ReviewKind::Authored);
    mine.draft = true;
    let mut asked = review("acme/shop-api", 91, ReviewKind::ReviewRequested);
    asked.draft = true;
    let e = env(tmp.path(), vec![mine, asked]);
    assert!(!settings().reviews.include_drafts);
    let authored = e.h.core.review_page(Scope::All, ReviewKind::Authored, true).await.unwrap();
    assert_eq!(authored.items.len(), 1, "a draft PR is how Louis reviews on GitHub (B6)");
    let requested = e.h.core.review_page(Scope::All, ReviewKind::ReviewRequested, true).await.unwrap();
    assert!(requested.items.is_empty());
}

#[tokio::test]
async fn work_item_prs_keep_authored_subscribed_without_panes() {
    let tmp = tempfile::tempdir().unwrap();
    let e = env(tmp.path(), vec![]);
    e.h.cfg.update(|s| s.notifications.enabled = false);
    settle().await;
    assert_eq!(e.h.core.scheduler_snapshot().subscriptions, 0);
    let mut w = work_item("feat/x", Some("https://github.com/acme/shop-api/pull/90"));
    e.h.core.publish(BusEvent::new(bus::WORK_UPDATED, serde_json::json!({ "work": w })));
    settle().await;
    assert_eq!(e.h.core.scheduler_snapshot().subscriptions, 1, "authored for github-work (B7)");
    w.state = WorkState::Finished;
    e.h.core.publish(BusEvent::new(bus::WORK_UPDATED, serde_json::json!({ "work": w })));
    settle().await;
    assert_eq!(e.h.core.scheduler_snapshot().subscriptions, 0);
}

#[tokio::test]
async fn merge_while_kelta_was_closed_lands_as_merged_on_next_launch() {
    let tmp = tempfile::tempdir().unwrap();
    let mut pr = review("acme/shop-api", 90, ReviewKind::Authored);
    pr.url = "https://github.com/acme/shop-api/pull/90".into();
    let item = work_item("feat/x", Some(&pr.url));
    {
        let e = env(tmp.path(), vec![pr.clone()]);
        let it = item.clone();
        e.h.core.store().call(move |c| q::work_put(c, &it)).await.unwrap();
        e.h.core.shutdown().await.unwrap();
    }
    // Merged on the host while Kelta was closed: no longer in the authored open list.
    let host = ListHost::new(vec![]);
    host.ended.lock().push((pr.clone(), PrState::Merged));
    let e = env_with(tmp.path(), host);
    let mut rx = e.h.core.subscribe();
    // What start_async runs at launch (start_services is off in tests, like work.startup).
    e.h.core.check_work_prs().await.unwrap();
    let got = wait_work(&e, |w| w.id == item.id && w.state != WorkState::PrOpen).await;
    assert_eq!(got.state, WorkState::Merged { detail: None });
    // The next check (Now open) publishes nothing new: once per PR.
    e.h.core.check_work_prs().await.unwrap();
    let merged = bus_names(&mut rx).into_iter().filter(|ev| ev.name == bus::PR_MERGED).count();
    assert_eq!(merged, 1);
}

#[tokio::test]
async fn pr_merged_carries_the_project_whatever_the_repo_case() {
    let tmp = tempfile::tempdir().unwrap();
    // Bound as `acme/shop-api`; the host reports `Acme/Shop-API`.
    let pr = review("Acme/Shop-API", 90, ReviewKind::Authored);
    let e = env(tmp.path(), vec![pr.clone()]);
    e.h.core.review_page(Scope::All, ReviewKind::Authored, true).await.unwrap();
    e.host.reviews.lock().clear();
    e.host.ended.lock().push((pr, PrState::Merged));
    let mut rx = e.h.core.subscribe();
    e.h.core.review_page(Scope::All, ReviewKind::Authored, true).await.unwrap(); // live diff
    settle().await;
    let merged: Vec<_> = bus_names(&mut rx).into_iter().filter(|ev| ev.name == bus::PR_MERGED).collect();
    assert_eq!(merged.len(), 1);
    assert_eq!(merged[0].project_id, Some(ProjectId::new("shop")));
}

#[tokio::test]
async fn closed_pr_leaving_the_authored_list_is_pr_closed() {
    let tmp = tempfile::tempdir().unwrap();
    let pr = review("acme/shop-api", 91, ReviewKind::Authored);
    let e = env(tmp.path(), vec![pr.clone()]);
    let item = work_item("feat/y", Some(&pr.url));
    let it = item.clone();
    e.h.core.store().call(move |c| q::work_put(c, &it)).await.unwrap();
    e.h.core.check_work_prs().await.unwrap(); // listed: open, nothing to do
    e.h.core.review_page(Scope::All, ReviewKind::Authored, true).await.unwrap();
    e.host.reviews.lock().clear();
    e.host.ended.lock().push((pr, PrState::Closed));
    e.h.core.review_page(Scope::All, ReviewKind::Authored, true).await.unwrap(); // live diff
    let got = wait_work(&e, |w| w.id == item.id && w.state != WorkState::PrOpen).await;
    assert_eq!(got.state, WorkState::PrClosed);
}

#[tokio::test]
async fn authored_pr_on_an_item_branch_is_joined_once() {
    let tmp = tempfile::tempdir().unwrap();
    let mut pr = review("acme/shop-api", 92, ReviewKind::Authored);
    pr.source_branch = "feat/SHOP-142-rate-limit-login".into();
    let e = env(tmp.path(), vec![pr.clone()]);
    let mut item = work_item(&pr.source_branch, None);
    item.kind = WorkKind::Ticket;
    item.ticket = Some(kelta_proto::samples::ticket_ref());
    let it = item.clone();
    e.h.core.store().call(move |c| q::work_put(c, &it)).await.unwrap();
    for _ in 0..2 {
        e.h.core.review_page(Scope::All, ReviewKind::Authored, true).await.unwrap();
    }
    let got = wait_work(&e, |w| w.id == item.id && w.pr_url.is_some()).await;
    assert_eq!((got.pr_url.as_deref(), got.state), (Some(pr.url.as_str()), WorkState::PrOpen));
    // on_pr ran once: one move to In Review.
    let moves = e.tracker.calls().iter().filter(|c| c.starts_with("transition:SHOP-142")).count();
    assert_eq!(moves, 1, "{:?}", e.tracker.calls());
}

#[tokio::test]
async fn approving_in_kelta_stamps_the_reviewed_head_hosts_do_not_report() {
    let tmp = tempfile::tempdir().unwrap();
    let e = env(tmp.path(), vec![review("acme/shop-api", 87, ReviewKind::ReviewRequested)]);
    let r = ReviewRef { account: "github-work".into(), repo: "acme/shop-api".into(), number: 87 };
    let head = e.h.core.review_get(&r).await.unwrap().review.head_sha;
    assert_eq!(e.h.core.review_get(&r).await.unwrap().review.reviewed_head, None);
    e.h.core.review_approve(&r, &head).await.unwrap();
    // The PR moves on: "Updated since your review", on any host.
    e.host.reviews.lock()[0].head_sha = "f".repeat(40);
    assert_eq!(e.h.core.review_get(&r).await.unwrap().review.reviewed_head, Some(head.clone()));
    let page = e.h.core.review_page(Scope::All, ReviewKind::ReviewRequested, true).await.unwrap();
    assert_eq!(page.items[0].review.reviewed_head, Some(head));
}

/// shop: views `mine` + `sprint` on jira-acme (same tickets) and `team` (who: mine) on linear-team,
/// which also holds an unassigned TEAM-9; blog: `mine`.
fn union_env(root: &std::path::Path) -> (Env, AccountId) {
    let e = env(root, vec![]);
    let linear = AccountId::new("linear-team");
    // same key as a jira ticket (kept apart: dedup is per account) + one only linear has
    let tickets =
        [("SHOP-141", "SHOP-141"), ("SHOP-142", "TEAM-7"), ("SHOP-143", "TEAM-9")].map(|(k, as_key)| {
            let mut d = e.tracker.ticket(k).unwrap();
            d.ticket.r#ref = TicketRef { account: linear.clone(), key: as_key.into(), id: as_key.into() };
            d.ticket.assignee = d.ticket.assignee.filter(|_| as_key != "TEAM-9");
            d
        });
    e.factory.trackers.lock().insert(linear.clone(), Arc::new(FakeTracker::with_tickets(tickets.to_vec())));
    e.h.cfg.update(|s| {
        s.accounts.insert(linear.clone(), account(AccountKind::Linear));
    });
    {
        let mut ps = e.h.cfg.projects.write();
        let mut shop = (*ps[0]).clone();
        let b = shop.tracker.as_mut().unwrap();
        let mine = b.views[0].clone();
        b.views.push(TrackerView {
            id: "sprint".into(),
            jql: Some("sprint in openSprints()".into()),
            ..mine
        });
        b.views.push(TrackerView {
            id: "team".into(),
            account: Some(linear.clone()),
            who: Some(Who::Mine),
            ..TrackerView::default()
        });
        ps[0] = Arc::new(shop);
    }
    (e, linear)
}

#[tokio::test]
async fn union_of_views_dedups_across_views_and_accounts() {
    let tmp = tempfile::tempdir().unwrap();
    let (e, linear) = union_env(tmp.path());
    let page = e.h.core.tracker_list(Scope::All, None, None, None, None, true).await.unwrap();
    let mut lists: Vec<String> = e.tracker.calls().into_iter().filter(|c| c.starts_with("list:")).collect();
    lists.sort();
    assert_eq!(lists, ["list:mine", "list:sprint"], "blog shares shop's `mine` query");
    let got: Vec<(&str, &str, Vec<&str>, Vec<&str>)> = page
        .items
        .iter()
        .map(|i| {
            (
                i.ticket.r#ref.account.as_str(),
                i.ticket.r#ref.key.as_str(),
                i.project_ids.iter().map(|p| p.as_str()).collect(),
                i.view_ids.iter().map(String::as_str).collect(),
            )
        })
        .collect();
    let jira = |k| ("jira-acme", k, vec!["shop", "blog"], vec!["mine", "sprint"]);
    let lin = |k| ("linear-team", k, vec!["shop"], vec!["team"]);
    assert_eq!(got, [jira("SHOP-141"), jira("SHOP-142"), jira("SHOP-143"), lin("SHOP-141"), lin("TEAM-7")]);
    assert!(page.next.is_none() && page.errors.is_empty());
    // an explicit view stays a single query
    let shop = Scope::Project { id: "shop".into() };
    let team =
        e.h.core.tracker_list(shop.clone(), None, Some("team".into()), None, None, true).await.unwrap();
    assert!(team.items.iter().all(|i| i.ticket.r#ref.account == linear && i.view_ids == ["team"]));
    // who overrides every view of the union, under its own cache keys
    let keys = |who| -> Vec<String> {
        e.h.core.ticket_queries(&shop, None, who).into_iter().map(|q| q.cache_key).collect()
    };
    let mine = e.h.core.ticket_queries(&shop, None, Some(Who::Mine));
    assert_eq!(mine.len(), 3);
    assert!(mine.iter().all(|q| q.view.who == Some(Who::Mine)));
    assert!(keys(None).iter().all(|k| !keys(Some(Who::Anyone)).contains(k)));
    // `kelta start <bare key>` finds the account that has the key, even unassigned (off every list)
    use kelta_proto::ctl::CtlCommand;
    for (key, account) in [("SHOP-143", "jira-acme"), ("TEAM-7", "linear-team"), ("TEAM-9", "linear-team")] {
        let plan =
            e.h.core
                .ctl(CtlCommand::Start { ticket: key.into(), project: Some("shop".into()), resolved: None })
                .await;
        let plan = plan.unwrap();
        assert_eq!(plan["source"]["ticket"]["account"], account, "{plan}");
    }
    // without --project: the same default as the Start sheet (SPEC §3.1)
    let plan =
        e.h.core
            .ctl(CtlCommand::Start { ticket: "TEAM-7".into(), project: None, resolved: None })
            .await
            .unwrap();
    assert_eq!(
        (&plan["source"]["ticket"]["account"], &plan["project_id"]),
        (&"linear-team".into(), &"shop".into())
    );
    e.h.core.project_close(&ProjectId::new("blog"), true).unwrap();
    // an explicit --project that cannot host the start is refused, never swapped for another
    let e2 = e
        .h
        .core
        .ctl(CtlCommand::Start { ticket: "SHOP-143".into(), project: Some("blog".into()), resolved: None })
        .await;
    let m = e2.unwrap_err().message;
    assert!(m.contains("closed or has no repository"), "{m}");
}

#[tokio::test]
async fn all_scope_pages_each_query_and_search_reaches_past_the_first_page() {
    let tmp = tempfile::tempdir().unwrap();
    let e = env_full(tmp.path(), ListHost::new(vec![]), FakeTracker::new().with_page_size(1));
    let shop = Scope::Project { id: "shop".into() };
    let keys = |p: &kelta_proto::tracker::TicketPage| -> Vec<String> {
        p.items.iter().map(|i| i.ticket.r#ref.key.clone()).collect()
    };
    // a tracker search that fails falls back to the cached first page
    e.tracker.fail_next(kelta_proto::error::KeltaError::upstream("down"));
    let hits = e.h.core.tracker_search(shop.clone(), "LOGIN").await.unwrap();
    assert_eq!(hits.iter().map(|i| i.ticket.r#ref.key.as_str()).collect::<Vec<_>>(), ["SHOP-141"]);
    let hits = e.h.core.tracker_search(shop.clone(), "login").await.unwrap();
    assert_eq!(
        hits.iter().map(|i| i.ticket.r#ref.key.as_str()).collect::<Vec<_>>(),
        ["SHOP-141", "SHOP-142"]
    );
    assert_eq!(hits[0].project_ids, [ProjectId::from("shop")]);
    assert!(e.tracker.calls().contains(&"search:mine:login".to_owned()));
    {
        let mut ps = e.h.cfg.projects.write();
        let mut p = (*ps[0]).clone();
        let b = p.tracker.as_mut().unwrap();
        let sprint = TrackerView {
            id: "sprint".into(),
            jql: Some("sprint in openSprints()".into()),
            ..b.views[0].clone()
        };
        b.views.push(sprint);
        ps[0] = Arc::new(p);
    }
    // two queries (mine, sprint): the cursor carries one provider cursor per query
    let mut seen = Vec::new();
    let mut page = e.h.core.tracker_list(Scope::All, None, None, None, None, true).await.unwrap();
    loop {
        assert!(page.items.iter().all(|i| i.view_ids == ["mine", "sprint"]), "{:?}", page.items);
        seen.extend(keys(&page));
        let Some(next) = page.next.clone() else { break };
        page = e.h.core.tracker_list(Scope::All, None, None, None, Some(next), false).await.unwrap();
    }
    assert_eq!(seen, ["SHOP-141", "SHOP-142", "SHOP-143"]);
    let bad = e.h.core.tracker_list(
        Scope::All,
        None,
        None,
        None,
        Some(kelta_proto::tracker::Cursor::Offset(1)),
        false,
    );
    assert_eq!(bad.await.unwrap_err().code, ErrorCode::InvalidArgument);
}

#[tokio::test]
async fn search_by_key_prefix_still_reads_the_cached_page() {
    let tmp = tempfile::tempdir().unwrap();
    let e = env(tmp.path(), vec![]);
    // FakeTracker::search matches titles only, like Jira `text ~`: the key hits come from the cache
    let hits = e.h.core.tracker_search(Scope::Project { id: "shop".into() }, "SHOP-14").await.unwrap();
    let keys: Vec<_> = hits.iter().map(|i| i.ticket.r#ref.key.as_str()).collect();
    assert!(keys.contains(&"SHOP-141") && keys.contains(&"SHOP-142"), "{keys:?}");
}

#[tokio::test]
async fn global_tickets_view_is_polled_without_a_tickets_pane() {
    use kelta_proto::model::{PaneContent, TicketsMode};
    let tmp = tempfile::tempdir().unwrap();
    let e = env(tmp.path(), vec![]);
    e.h.cfg.update(|s| s.notifications.enabled = false);
    settle().await;
    assert_eq!(e.h.core.scheduler_snapshot().subscriptions, 0, "the active layout has no Tickets pane");
    show_global(
        &e.h.core,
        Some(PaneContent::Tickets {
            scope: Scope::All,
            view_id: None,
            source_id: None,
            mode: TicketsMode::List,
            who: Some(Who::Mine),
            group: None,
            sort: None,
            person: None,
        }),
    );
    settle().await;
    assert!(e.h.core.scheduler_snapshot().subscriptions > 0, "the view's cache keys are subscribed");
    show_global(&e.h.core, None);
    settle().await;
    assert_eq!(e.h.core.scheduler_snapshot().subscriptions, 0, "back to a project");
}

#[tokio::test]
async fn visible_list_refreshes_after_a_saga_transition() {
    use kelta_proto::model::{OpenPaneRequest, PaneContent, Placement, TicketsMode};
    let tmp = tempfile::tempdir().unwrap();
    let e = env(tmp.path(), vec![]);
    let shop = Scope::Project { id: "shop".into() };
    e.h.core.project_activate(&ProjectId::new("shop")).unwrap();
    let content = PaneContent::Tickets {
        scope: shop.clone(),
        view_id: None,
        source_id: None,
        mode: TicketsMode::List,
        who: None,
        group: None,
        sort: None,
        person: None,
    };
    let req = OpenPaneRequest {
        content,
        placement: Placement::NewTab,
        focus: true,
        tab_title: None,
        work_item_id: None,
    };
    e.h.core.layout_open(&ProjectId::new("shop"), req).await.unwrap();
    e.h.core.tracker_list(shop.clone(), None, None, None, None, true).await.unwrap();
    // the saga moves the ticket through the provider itself, then publishes the bus event
    let t = e.tracker.ticket("SHOP-141").unwrap().ticket.r#ref;
    let moved = e.tracker.transition(&t, "t3", None).await.unwrap();
    e.h.core.publish(BusEvent::new(
        bus::TICKET_TRANSITIONED,
        serde_json::json!({ "ticket": t, "to": moved.status }),
    ));
    let status = || async {
        let page = e.h.core.tracker_list(shop.clone(), None, None, None, None, false).await.unwrap();
        page.items.into_iter().find(|i| i.ticket.r#ref == t).unwrap().ticket.status.category
    };
    for _ in 0..200 {
        if status().await == StatusCategory::InProgress {
            return;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    panic!("the cached list still shows {:?}", status().await);
}

#[tokio::test]
async fn sources_removed_unknown_views_and_unlisted_keys() {
    let tmp = tempfile::tempdir().unwrap();
    let e = env(tmp.path(), vec![]);
    let shop = Scope::Project { id: "shop".into() };
    // a view id that no longer exists falls back to the union, not to another source
    assert_eq!(e.h.core.ticket_queries(&shop, Some("gone"), None).len(), 1);
    // a key outside every list resolves by GET; a miss leaves the account status alone
    {
        let mut ps = e.h.cfg.projects.write();
        let mut p = (*ps[0]).clone();
        p.tracker.as_mut().unwrap().views[0].who = Some(Who::Mine);
        ps[0] = Arc::new(p);
        ps.truncate(1);
    }
    let t = e.tracker.ticket("SHOP-142").unwrap().ticket.r#ref;
    e.tracker.assign(&t, Assignee::None).await.unwrap();
    let hits = e.h.core.tracker_search(shop.clone(), "SHOP-142").await.unwrap();
    assert_eq!(hits.iter().map(|i| i.ticket.r#ref.key.as_str()).collect::<Vec<_>>(), ["SHOP-142"]);
    assert_eq!(hits[0].project_ids, [ProjectId::from("shop")]);
    assert!(e.h.core.tracker_search(shop.clone(), "SHOP-999").await.unwrap().is_empty());
    assert!(!e.h.ui.events().iter().any(|ev| matches!(ev, UiEvent::AccountStatusChanged { .. })));
    // removing the last source stops the project's queries (no implicit "mine" view)
    {
        let mut ps = e.h.cfg.projects.write();
        let mut p = (*ps[0]).clone();
        p.tracker.as_mut().unwrap().views.clear();
        ps[0] = Arc::new(p);
    }
    assert!(e.h.core.ticket_queries(&shop, None, Some(Who::Mine)).is_empty());
}

#[tokio::test]
async fn tickets_carry_work_item_and_key_matched_prs_and_caps() {
    let tmp = tempfile::tempdir().unwrap();
    // The work item's PR, also in the authored feed (enriched, listed once).
    let mut own = review("acme/shop-api", 90, ReviewKind::Authored);
    own.url = "https://github.com/acme/shop-api/pull/90".into();
    own.ci = CiState::Failure;
    own.linked_tickets = vec!["SHOP-142".into()];
    // Someone else's PR naming the ticket (lower case) in its title.
    let mut other = review("acme/shop-api", 95, ReviewKind::ReviewRequested);
    other.url = "https://github.com/acme/shop-api/pull/95".into();
    other.linked_tickets = vec!["shop-142".into(), "SHOP-143".into()];
    let e = env(tmp.path(), vec![own.clone(), other.clone()]);
    let mut item = work_item("feat/SHOP-142", Some(&own.url));
    item.kind = WorkKind::Ticket;
    item.ticket = Some(kelta_proto::samples::ticket_ref());
    let it = item.clone();
    e.h.core.store().call(move |c| q::work_put(c, &it)).await.unwrap();
    let shop = || Scope::Project { id: "shop".into() };
    // Before any review poll: only the work item's PR, from its URL and the repo binding.
    let page = e.h.core.tracker_list(shop(), None, None, None, None, true).await.unwrap();
    let prs = |p: &kelta_proto::tracker::TicketPage, key: &str| {
        p.items.iter().find(|i| i.ticket.r#ref.key == key).unwrap().prs.clone()
    };
    let bare = prs(&page, "SHOP-142");
    assert_eq!(bare.len(), 1);
    assert_eq!((bare[0].number, bare[0].repo.as_str(), bare[0].ci), (90, "acme/shop-api", CiState::None));
    assert_eq!(bare[0].account, Some(AccountId::new("github-work")));
    // plus `block`, core-filled: Jira's default marker, the flag, is writable
    let caps = kelta_proto::tracker::TrackerCaps { block: true, ..e.tracker.caps.clone() };
    assert!(page.items.iter().all(|i| i.caps == caps), "caps of the ticket's tracker");

    for kind in [ReviewKind::Authored, ReviewKind::ReviewRequested] {
        e.h.core.review_page(Scope::All, kind, true).await.unwrap();
    }
    let calls = *e.host.calls.lock();
    let page = e.h.core.tracker_list(shop(), None, None, None, None, false).await.unwrap();
    assert_eq!(*e.host.calls.lock(), calls, "PR links come from the cache, never the network");
    let linked = prs(&page, "SHOP-142");
    let got: Vec<_> = linked.iter().map(|l| (l.number, l.source, l.ci)).collect();
    assert_eq!(got, vec![(90, PrSource::WorkItem, CiState::Failure), (95, PrSource::KeyMatch, other.ci)]);
    assert_eq!(prs(&page, "SHOP-143").iter().map(|l| l.number).collect::<Vec<_>>(), vec![95]);
    assert!(prs(&page, "SHOP-141").is_empty());
    let detail = e.h.core.tracker_get(&kelta_proto::samples::ticket_ref()).await.unwrap();
    assert_eq!(detail.prs, linked);
    // Sub-tasks come back as list rows: project, PRs and caps filled like the list's.
    let [child] = detail.children.as_slice() else { panic!("one sub-task") };
    assert_eq!(child.ticket.r#ref.key, "SHOP-143");
    assert_eq!(child.project_ids, vec![ProjectId::new("shop")]);
    assert_eq!(child.prs.iter().map(|l| l.number).collect::<Vec<_>>(), vec![95]);
    assert_eq!(child.caps, caps);
}

#[test]
fn hash_keys_resolve_against_the_pr_repo_and_merged_items_win() {
    use kelta_core::feeds::ticket_prs;
    let mut r = review("acme/shop", 7, ReviewKind::Authored);
    r.url = "https://github.com/acme/shop/pull/7".into();
    r.linked_tickets = vec!["#12".into()];
    let one = std::slice::from_ref(&r);
    assert_eq!(ticket_prs("acme/shop#12", None, None, one, &[]).len(), 1);
    assert_eq!(ticket_prs("12", None, None, one, &["Acme/Shop".into()]).len(), 1, "Redmine bare number");
    assert!(
        ticket_prs("12", None, None, one, &["acme/other".into()]).is_empty(),
        "outside the ticket's repos"
    );
    assert!(ticket_prs("acme/other#12", None, None, one, &[]).is_empty());
    assert!(ticket_prs("SHOP-12", None, None, one, &[]).is_empty());
    // A merged work item keeps its PR, unbound repo → no account (browser only).
    let mut w = work_item("feat/x", Some("https://git.example/a/b/-/merge_requests/44/"));
    w.state = WorkState::Merged { detail: None };
    let got = ticket_prs("X-1", Some(&w), None, &[], &[]);
    assert_eq!((got[0].number, got[0].state, got[0].account.clone()), (44, PrState::Merged, None));
}

#[tokio::test]
async fn a_nudge_is_refused_for_24_hours_even_after_a_restart() {
    let tmp = tempfile::tempdir().unwrap();
    let pr = review("acme/shop-api", 93, ReviewKind::Authored);
    let r = pr.r#ref.clone();
    {
        let e = env(tmp.path(), vec![pr.clone()]);
        e.h.core.review_nudge(&r, &[], Some("@anna ping")).await.unwrap();
        let page = e.h.core.review_page(Scope::All, ReviewKind::Authored, true).await.unwrap();
        assert!(page.items[0].review.nudged_at.is_some());
        e.h.core.shutdown().await.unwrap();
    }
    let e = env(tmp.path(), vec![pr]);
    let err = e.h.core.review_nudge(&r, &["anna".into()], None).await.unwrap_err();
    assert_eq!(err.code, ErrorCode::InvalidArgument, "{err:?}");
    // An older nudge no longer counts.
    let rr = r.clone();
    e.h.core.store().call(move |c| q::nudge_put(c, &rr, "2026-01-01T00:00:00Z")).await.unwrap();
    e.h.core.review_nudge(&r, &[], Some("ping")).await.unwrap();
}

#[cfg(unix)]
#[tokio::test]
async fn refine_feeds_claude_the_ticket_and_serves_get_ticket_over_mcp() {
    use std::os::unix::fs::PermissionsExt;
    let tmp = tempfile::tempdir().unwrap();
    let e = env(tmp.path(), vec![]);
    // Fake Claude: echoes the first stdin line, its args, then calls MCP get_ticket.
    let fake = tmp.path().join("fake-claude");
    std::fs::write(
        &fake,
        "#!/bin/sh\nhead -n1\necho \"args: $*\"\n/usr/bin/curl -s -H \"Authorization: Bearer $KELTA_MCP_TOKEN\" \
         -H 'Content-Type: application/json' \
         -d '{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"tools/call\",\"params\":{\"name\":\"get_ticket\"}}' \
         \"$KELTA_MCP_URL\"\n",
    )
    .unwrap();
    std::fs::set_permissions(&fake, std::fs::Permissions::from_mode(0o755)).unwrap();
    e.h.cfg.update(|s| s.claude.binary = fake.to_string_lossy().into_owned());
    let t = e.tracker.ticket("SHOP-142").unwrap().ticket.r#ref;

    let out = e.h.core.tracker_refine(&t, Some(&ProjectId::new("shop"))).await.unwrap();
    assert!(out.starts_with("# SHOP-142: "), "{out}");
    assert!(out.contains("args: -p ") && out.contains("--allowedTools mcp__kelta__get_ticket"), "{out}");
    assert!(out.contains("Acceptance criteria"), "the prompt asks for the criteria: {out}");
    assert!(out.contains("# SHOP-142: ") && out.contains("\"isError\":false"), "get_ticket answered: {out}");

    std::fs::write(&fake, "#!/bin/sh\necho 'not logged in' >&2\nexit 1\n").unwrap();
    let err = e.h.core.tracker_refine(&t, None).await.unwrap_err();
    assert!(err.message.contains("could not refine SHOP-142: not logged in"), "{err:?}");
}

#[tokio::test]
async fn concurrent_cold_lists_share_one_provider_call() {
    let tmp = tempfile::tempdir().unwrap();
    let e = env(tmp.path(), vec![]);
    let core: Arc<dyn CoreApi> = e.h.core.clone();
    let pages =
        futures::future::join_all((0..5).map(|_| core.ticket_list(Scope::All, None, None, false))).await;
    for p in pages {
        assert_eq!(p.unwrap().items.len(), 3);
    }
    let lists = e.tracker.calls().iter().filter(|c| c.starts_with("list:")).count();
    assert_eq!(lists, 1, "{:?}", e.tracker.calls());
}

#[tokio::test]
async fn writes_patch_the_cached_list() {
    let tmp = tempfile::tempdir().unwrap();
    let e = env(tmp.path(), vec![]);
    let core: Arc<dyn CoreApi> = e.h.core.clone();
    let lists = || e.tracker.calls().iter().filter(|c| c.starts_with("list")).count();
    let status = |page: &kelta_proto::tracker::TicketPage, key: &str| {
        page.items.iter().find(|i| i.ticket.r#ref.key == key).unwrap().ticket.status.name.clone()
    };
    let page = core.ticket_list(Scope::All, None, None, false).await.unwrap();
    let before = lists();
    let r = |key: &str| page.items.iter().find(|i| i.ticket.r#ref.key == key).unwrap().ticket.r#ref.clone();
    // SHOP-141 To Do → In Progress (FakeTracker transition ids are `t<status id>`).
    core.ticket_transition(&r("SHOP-141"), "t3", None, None).await.unwrap();
    core.ticket_close(&r("SHOP-142")).await.unwrap();
    let page = core.ticket_list(Scope::All, None, None, false).await.unwrap();
    assert_eq!(status(&page, "SHOP-141"), "In Progress");
    assert_eq!(status(&page, "SHOP-142"), "Done");
    assert_eq!(lists(), before, "served from the patched cache: {:?}", e.tracker.calls());
}

#[tokio::test]
async fn ticket_pages_carry_the_team_people_and_waits_name_merged_people() {
    let tmp = tempfile::tempdir().unwrap();
    let mut authored = review("acme/shop-api", 87, ReviewKind::Authored);
    authored.waiting_on = vec!["bob".into()];
    authored.author_to_act = true;
    let e = env(tmp.path(), vec![authored]);
    let shop = Scope::Project { id: "shop".into() };

    let team = e.h.core.tracker_list(shop.clone(), None, None, Some(Who::Anyone), None, true).await.unwrap();
    let page = e.h.core.review_page(shop.clone(), ReviewKind::Authored, true).await.unwrap();
    let names: Vec<_> = page.items[0].waiting.iter().map(|w| (w.name.as_str(), w.why)).collect();
    assert_eq!(names, [("Bob", WaitWhy::Review), ("Bob", WaitWhy::Author)]);
    let (host_calls, tracker_calls) = (*e.host.calls.lock(), e.tracker.calls().len());

    // Any tab's page names the Team list's people, from caches only.
    let mine = e.h.core.tracker_list(shop, None, None, Some(Who::Mine), None, false).await.unwrap();
    let people = &mine.people;
    assert!(people.iter().any(|p| p.ids.iter().any(|i| i.id == "bob" && i.code_host && !p.matched)));
    assert!(people.iter().any(|p| p.me));
    let assignees = team.items.iter().filter_map(|i| i.ticket.assignee.as_ref());
    for a in assignees {
        assert!(people.iter().any(|p| p.ids.iter().any(|i| !i.code_host && i.id == a.id)), "{a:?}");
    }
    let tracker_lists = e.tracker.calls()[tracker_calls..].iter().filter(|c| c.starts_with("list:")).count();
    assert_eq!((*e.host.calls.lock(), tracker_lists), (host_calls, 1), "Mine fetched once, the rest cached");
}

#[tokio::test]
async fn ticket_by_key_reads_the_cached_lists_without_enrichment() {
    let tmp = tempfile::tempdir().unwrap();
    let e = env(tmp.path(), vec![]);
    let page = e.h.core.ticket_list(Scope::All, None, None, true).await.unwrap();
    let want = &page.items[0];
    let key = want.ticket.r#ref.key.clone();
    let calls = e.tracker.calls().len();
    let got = e.h.core.ticket_by_key(&key, None).await.unwrap().unwrap();
    assert_eq!(got.ticket.r#ref, want.ticket.r#ref);
    assert_eq!((&got.project_ids, &got.source_ids), (&want.project_ids, &want.source_ids));
    let mine = e.h.core.ticket_by_key(&key, Some(&want.ticket.r#ref.account)).await.unwrap();
    assert_eq!(mine.map(|i| i.ticket.r#ref), Some(want.ticket.r#ref.clone()));
    assert!(e.h.core.ticket_by_key(&key, Some(&AccountId::new("other"))).await.unwrap().is_none());
    assert!(e.h.core.ticket_by_key("NOPE-1", None).await.unwrap().is_none());
    assert_eq!(e.tracker.calls().len(), calls, "served from the cache: {:?}", e.tracker.calls());
}

#[tokio::test]
async fn ticket_by_key_on_an_account_loads_only_that_accounts_lists() {
    let tmp = tempfile::tempdir().unwrap();
    let (e, linear) = union_env(tmp.path());
    let other = Arc::new(FakeTracker::new());
    e.factory.trackers.lock().insert(linear, other.clone());
    let jira = AccountId::new("jira-acme");
    let got = e.h.core.ticket_by_key("SHOP-141", Some(&jira)).await.unwrap().unwrap();
    assert_eq!(got.ticket.r#ref.account, jira);
    assert!(e.tracker.calls().iter().any(|c| c.starts_with("list:")), "{:?}", e.tracker.calls());
    assert!(other.calls().is_empty(), "the other account stays cold: {:?}", other.calls());
}

/// shop (a source feeds it, bound to acme/shop-api) + lab (no source, bound to acme/lab).
fn team_env(root: &std::path::Path, reviews: Vec<Review>) -> Env {
    let mut ps = projects(root);
    let mut lab = project("lab", root);
    lab.repos[0].code_host = Some(CodeHostBinding { account: "github-work".into(), repo: "acme/lab".into() });
    ps.push(lab);
    env_in(root, ListHost::new(reviews), FakeTracker::new(), ps)
}

#[tokio::test]
async fn team_lists_open_prs_on_source_fed_repos_only() {
    let tmp = tempfile::tempdir().unwrap();
    let e = team_env(
        tmp.path(),
        vec![
            review("acme/shop-api", 1, ReviewKind::Team),
            review("acme/lab", 2, ReviewKind::Team),
            review("acme/shop-api", 3, ReviewKind::Authored),
        ],
    );
    let acc = AccountId::new("github-work");
    assert_eq!(e.h.core.team_repos(&Scope::All), vec![(acc, "acme/shop-api".into(), ProjectId::new("shop"))]);
    let all = e.h.core.review_page(Scope::All, ReviewKind::Team, true).await.unwrap();
    let numbers: Vec<u64> = all.items.iter().map(|i| i.review.r#ref.number).collect();
    assert_eq!(numbers, vec![1], "lab is fed by no source: never in Team");
    assert_eq!(all.items[0].project_ids, vec![ProjectId::new("shop")]);
    let shop =
        e.h.core.review_page(Scope::Project { id: "shop".into() }, ReviewKind::Team, false).await.unwrap();
    assert_eq!(shop.items.len(), 1);
    let lab =
        e.h.core.review_page(Scope::Project { id: "lab".into() }, ReviewKind::Team, true).await.unwrap();
    assert!(lab.items.is_empty() && lab.errors.is_empty());
    let mine =
        e.h.core.review_page(Scope::Project { id: "lab".into() }, ReviewKind::Authored, true).await.unwrap();
    assert!(mine.items.is_empty(), "the project scope keeps its own repos");
}

#[tokio::test]
async fn team_is_polled_only_while_its_tab_is_shown() {
    use kelta_proto::model::PaneContent;
    let tmp = tempfile::tempdir().unwrap();
    let e = team_env(tmp.path(), vec![]);
    e.h.cfg.update(|s| s.notifications.enabled = false);
    settle().await;
    let view = |tab| PaneContent::Reviews { scope: Scope::All, tab };
    show_global(&e.h.core, Some(view(None)));
    settle().await;
    assert_eq!(e.h.core.scheduler_snapshot().subscriptions, 2, "For me + Mine");
    show_global(&e.h.core, Some(view(Some(ReviewKind::Team))));
    settle().await;
    assert_eq!(e.h.core.scheduler_snapshot().subscriptions, 3, "+ Team");
    show_global(&e.h.core, Some(view(Some(ReviewKind::Authored))));
    settle().await;
    assert_eq!(e.h.core.scheduler_snapshot().subscriptions, 2);
}

#[tokio::test]
async fn a_failing_side_keeps_the_account_status_until_it_recovers() {
    let tmp = tempfile::tempdir().unwrap();
    let e = env(tmp.path(), vec![]);
    // github-work is both a tracker and a code host
    e.factory.trackers.lock().insert(AccountId::new("github-work"), e.tracker.clone() as Arc<dyn Tracker>);
    let pr = review("acme/shop", 999, ReviewKind::ReviewRequested).r#ref;
    let ticket =
        TicketRef { account: "github-work".into(), ..e.tracker.ticket("SHOP-141").unwrap().ticket.r#ref };
    let statuses = || {
        e.h.ui
            .events()
            .iter()
            .filter_map(|ev| match ev {
                UiEvent::AccountStatusChanged { status, .. } => Some(*status),
                _ => None,
            })
            .collect::<Vec<_>>()
    };
    e.h.ui.take_events();
    for _ in 0..2 {
        assert!(e.h.core.review_get(&pr).await.is_err());
        e.h.core.tracker_get(&ticket).await.unwrap();
    }
    assert_eq!(statuses(), [kelta_proto::events::AccountStatus::Error], "no Ok/Error flip-flop");
    e.host.reviews.lock().push(review("acme/shop", 999, ReviewKind::ReviewRequested));
    e.h.core.review_get(&pr).await.unwrap();
    assert_eq!(
        statuses().last(),
        Some(&kelta_proto::events::AccountStatus::Ok),
        "the failing side recovered"
    );
}

#[tokio::test]
async fn review_by_ref_reads_the_cache_then_fetches_when_cold_and_none_on_error() {
    let tmp = tempfile::tempdir().unwrap();
    let (cached, cold) = (
        review("acme/shop-api", 87, ReviewKind::ReviewRequested),
        review("acme/shop-api", 88, ReviewKind::Authored),
    );
    let e = env(tmp.path(), vec![cached.clone(), cold.clone()]);
    e.h.core.review_page(Scope::All, ReviewKind::ReviewRequested, true).await.unwrap();
    // Cached: the host no longer has it, so only the cache can answer.
    e.host.reviews.lock().retain(|r| r.r#ref != cached.r#ref);
    assert_eq!(e.h.core.review(&cached.r#ref).await.map(|r| r.r#ref), Some(cached.r#ref.clone()));
    // Cold (its kind was never listed): fetched from the host.
    assert_eq!(e.h.core.review(&cold.r#ref).await.map(|r| r.r#ref), Some(cold.r#ref.clone()));
    // Neither cached nor on the host: the failed fetch is None.
    let gone = ReviewRef { number: 999, ..cold.r#ref };
    assert_eq!(e.h.core.review(&gone).await, None);
}

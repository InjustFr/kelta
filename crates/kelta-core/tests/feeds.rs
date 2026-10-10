//! Tickets/reviews: Scope::All de-dup + project tagging + "Other", provider_cache
//! stale-while-revalidate, per-account errors, seen_reviews first-poll silence, authored PR
//! change events, tracker_move resolution.
#![allow(clippy::unwrap_used)] // fixture helpers outside #[test] fns

mod common;

use std::sync::Arc;

use common::*;
use kelta_core::store::q;
use kelta_proto::ErrorCode;
use kelta_proto::api::{CodeHost, CoreApi, Tracker};
use kelta_proto::codehost::{CiState, Review, ReviewKind, ReviewRef};
use kelta_proto::events::{BusEvent, UiEvent};
use kelta_proto::ids::{AccountId, ProjectId};
use kelta_proto::ipc::WindowState;
use kelta_proto::model::Scope;
use kelta_proto::settings::{AccountKind, CodeHostBinding, ColumnSpec, ProjectConfig, Settings};
use kelta_proto::store::ProviderCacheRow;
use kelta_proto::testing::{FakeTerminalHost, FakeTracker};
use kelta_proto::tracker::StatusCategory;

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
}

fn env(root: &std::path::Path, reviews: Vec<Review>) -> Env {
    let tracker = Arc::new(FakeTracker::new());
    let host = ListHost::new(reviews);
    let factory = Arc::new(Factory::default());
    factory.trackers.lock().insert(AccountId::new("jira-acme"), tracker.clone() as Arc<dyn Tracker>);
    factory.hosts.lock().insert(AccountId::new("github-work"), host.clone() as Arc<dyn CodeHost>);
    let h = start_in(root, MemConfig::new(settings(), projects(root)), FakeTerminalHost::new(), factory);
    h.core.project_open(&ProjectId::new("shop")).unwrap();
    h.core.project_open(&ProjectId::new("blog")).unwrap();
    Env { h, tracker, host }
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
    let page = e.h.core.tracker_list(Scope::All, None, None, true).await.unwrap();
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
async fn cache_serves_first_then_revalidates_when_stale() {
    let tmp = tempfile::tempdir().unwrap();
    let e = env(tmp.path(), vec![]);
    let shop = Scope::Project { id: "shop".into() };
    let lists = || e.tracker.calls().iter().filter(|c| c.starts_with("list:")).count();
    let p1 = e.h.core.tracker_list(shop.clone(), None, None, false).await.unwrap();
    assert_eq!((lists(), p1.stale), (1, false));
    // fresh cache: no network
    let p2 = e.h.core.tracker_list(shop.clone(), None, None, false).await.unwrap();
    assert_eq!((lists(), p2.stale, p2.items.len()), (1, false, 3));
    // age the cache row → served stale + background refresh + tickets.changed
    let key = e.h.core.ticket_queries(&shop, None)[0].cache_key.clone();
    let row = e.h.core.store().call(move |c| q::cache_get(c, &key)).await.unwrap().unwrap();
    let old = ProviderCacheRow { fetched_at: "2020-01-01T00:00:00Z".into(), ..row };
    e.h.core.store().call(move |c| q::cache_put(c, &old)).await.unwrap();
    e.h.ui.take_events();
    let p3 = e.h.core.tracker_list(shop.clone(), None, None, false).await.unwrap();
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
    let p4 = e.h.core.tracker_list(shop, None, None, true).await.unwrap();
    assert!(p4.stale);
    assert_eq!(p4.items.len(), 3);
    assert_eq!(p4.errors[0].account_id.as_str(), "jira-acme");
    assert_eq!(p4.errors[0].error.code, ErrorCode::NeedsAuth);
    assert!(e.h.ui.events().iter().any(|ev| matches!(
        ev,
        UiEvent::AccountStatusChanged { status: kelta_proto::events::AccountStatus::NeedsAuth, .. }
    )));
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
        l[0].ci = CiState::Failure;
        l[0].decision = Some(kelta_proto::codehost::ReviewDecision::Approved);
    }
    e.h.core.review_page(Scope::All, ReviewKind::Authored, true).await.unwrap();
    let names: Vec<String> =
        bus_names(&mut rx).into_iter().map(|ev| ev.name).filter(|n| n.starts_with("pr.")).collect();
    assert_eq!(names, vec!["pr.ci_changed", "pr.approved", "pr.updated"]);
    assert_eq!(e.h.ui.notifications().len(), 2, "ci failed + approved");
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
    let cols = e.h.core.tracker_columns(&ProjectId::new("shop")).await.unwrap();
    assert_eq!(
        cols.iter().map(|c| c.id.as_str()).collect::<Vec<_>>(),
        vec!["todo", "late", "named", "weird"]
    );

    let err = e.h.core.tracker_move(&t, "late").await.unwrap_err();
    assert_eq!(err.code, ErrorCode::Conflict);
    assert_eq!(err.detail.unwrap()["candidates"].as_array().unwrap().len(), 2);
    let err = e.h.core.tracker_move(&t, "weird").await.unwrap_err();
    assert_eq!(err.code, ErrorCode::NotFound);
    assert!(err.detail.unwrap()["url"].as_str().unwrap().ends_with("SHOP-142"));

    let mut rx = e.h.core.subscribe();
    // names win over categories
    let moved = e.h.core.tracker_move(&t, "named").await.unwrap();
    assert_eq!(moved.status.name, "In Review");
    let moved = e.h.core.tracker_move(&t, "todo").await.unwrap();
    assert_eq!(moved.status.category, StatusCategory::Todo);
    assert!(bus_names(&mut rx).iter().any(|ev| ev.name == "ticket.transitioned"));
    // blog (no override) uses the provider's columns
    let blog_cols = e.h.core.tracker_columns(&ProjectId::new("blog")).await.unwrap();
    assert_eq!(blog_cols.len(), 4);
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
    let sub = |k| SubKey { account: acc.clone(), query: reviews_key(&acc, k) };
    e.h.core.refresh(&sub(ReviewKind::ReviewRequested)).await.unwrap();
    assert_eq!(*e.host.calls.lock(), 0, "gate says unchanged");
    e.h.core.refresh(&sub(ReviewKind::Authored)).await.unwrap();
    assert_eq!(*e.host.calls.lock(), 1, "authored never consumes the gate");
}

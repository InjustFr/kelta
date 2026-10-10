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
use kelta_proto::tracker::{Assignee, StatusCategory, TicketRef, Who};

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
    let tracker = Arc::new(tracker);
    let factory = Arc::new(Factory::default());
    factory.trackers.lock().insert(AccountId::new("jira-acme"), tracker.clone() as Arc<dyn Tracker>);
    factory.hosts.lock().insert(AccountId::new("github-work"), host.clone() as Arc<dyn CodeHost>);
    let h =
        start_in(root, MemConfig::new(settings(), projects(root)), FakeTerminalHost::new(), factory.clone());
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
    let page = e.h.core.tracker_list(Scope::All, None, None, None, true).await.unwrap();
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
    let page = e.h.core.tracker_list(shop, None, None, None, true).await.unwrap();
    let keys: Vec<&str> = page.items.iter().map(|i| i.ticket.r#ref.key.as_str()).collect();
    assert_eq!(keys, ["SHOP-141", "SHOP-142", "SHOP-143"], "open first page + done in the last 7 days");
}

#[tokio::test]
async fn cache_serves_first_then_revalidates_when_stale() {
    let tmp = tempfile::tempdir().unwrap();
    let e = env(tmp.path(), vec![]);
    let shop = Scope::Project { id: "shop".into() };
    let lists = || e.tracker.calls().iter().filter(|c| c.starts_with("list:")).count();
    let p1 = e.h.core.tracker_list(shop.clone(), None, None, None, false).await.unwrap();
    assert_eq!((lists(), p1.stale), (1, false));
    // fresh cache: no network
    let p2 = e.h.core.tracker_list(shop.clone(), None, None, None, false).await.unwrap();
    assert_eq!((lists(), p2.stale, p2.items.len()), (1, false, 3));
    // age the cache row → served stale + background refresh + tickets.changed
    let key = e.h.core.ticket_queries(&shop, None, None)[0].cache_key.clone();
    let row = e.h.core.store().call(move |c| q::cache_get(c, &key)).await.unwrap().unwrap();
    let old = ProviderCacheRow { fetched_at: "2020-01-01T00:00:00Z".into(), ..row };
    e.h.core.store().call(move |c| q::cache_put(c, &old)).await.unwrap();
    e.h.ui.take_events();
    let p3 = e.h.core.tracker_list(shop.clone(), None, None, None, false).await.unwrap();
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
    let p4 = e.h.core.tracker_list(shop, None, None, None, true).await.unwrap();
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
async fn who_override_view_account_and_sources() {
    let tmp = tempfile::tempdir().unwrap();
    let e = env(tmp.path(), vec![]);
    let shop = Scope::Project { id: "shop".into() };
    let key = |who| e.h.core.ticket_queries(&shop, None, who)[0].cache_key.clone();
    assert_ne!(key(None), key(Some(Who::Mine)));
    assert_ne!(key(Some(Who::Mine)), key(Some(Who::Unassigned)));
    let t = e.tracker.ticket("SHOP-142").unwrap().ticket.r#ref;
    e.tracker.assign(&t, Assignee::None).await.unwrap();
    let page = e.h.core.tracker_list(shop.clone(), None, Some(Who::Unassigned), None, true).await.unwrap();
    assert_eq!(page.items.iter().map(|i| i.ticket.r#ref.key.as_str()).collect::<Vec<_>>(), ["SHOP-142"]);
    assert_eq!(page.items[0].view_ids, ["mine"]);
    assert_eq!(e.h.core.tracker_list(shop.clone(), None, None, None, true).await.unwrap().items.len(), 3);
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
    e.h.core.tracker_list(shop, None, None, None, false).await.unwrap();
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
    let cols = e.h.core.tracker_columns(&ProjectId::new("shop")).await.unwrap();
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
    // two projects on one account: an explicit project picks its own columns (blog has no
    // `late`; shop has no provider column `3` = In Progress)
    let err = e.h.core.tracker_move(&t, "late", Some(&ProjectId::new("blog"))).await.unwrap_err();
    assert_eq!(err.code, ErrorCode::NotFound);
    assert_eq!(e.h.core.tracker_move(&t, "3", None).await.unwrap_err().code, ErrorCode::NotFound);
    let blog = Some(ProjectId::new("blog"));
    let started = e.h.core.tracker_move(&t, "3", blog.as_ref()).await.unwrap();
    assert_eq!(started.status.category, StatusCategory::InProgress);
    let moved = e.h.core.tracker_move(&t, "todo", None).await.unwrap();
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
    let page = e.h.core.tracker_list(Scope::All, None, None, None, true).await.unwrap();
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
    let team = e.h.core.tracker_list(shop.clone(), Some("team".into()), None, None, true).await.unwrap();
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
        let plan = e.h.core.ctl(CtlCommand::Start { ticket: key.into(), project: Some("shop".into()) }).await;
        let plan = plan.unwrap();
        assert_eq!(plan["source"]["ticket"]["account"], account, "{plan}");
    }
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
    e.h.core.tracker_list(shop.clone(), None, None, None, true).await.unwrap();
    // the saga moves the ticket through the provider itself, then publishes the bus event
    let t = e.tracker.ticket("SHOP-141").unwrap().ticket.r#ref;
    let moved = e.tracker.transition(&t, "t3", None).await.unwrap();
    e.h.core.publish(BusEvent::new(
        bus::TICKET_TRANSITIONED,
        serde_json::json!({ "ticket": t, "to": moved.status }),
    ));
    let status = || async {
        let page = e.h.core.tracker_list(shop.clone(), None, None, None, false).await.unwrap();
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
    let page = e.h.core.tracker_list(shop(), None, None, None, true).await.unwrap();
    let prs = |p: &kelta_proto::tracker::TicketPage, key: &str| {
        p.items.iter().find(|i| i.ticket.r#ref.key == key).unwrap().prs.clone()
    };
    let bare = prs(&page, "SHOP-142");
    assert_eq!(bare.len(), 1);
    assert_eq!((bare[0].number, bare[0].repo.as_str(), bare[0].ci), (90, "acme/shop-api", CiState::None));
    assert_eq!(bare[0].account, Some(AccountId::new("github-work")));
    assert!(page.items.iter().all(|i| i.caps == e.tracker.caps), "caps of the ticket's tracker");

    for kind in [ReviewKind::Authored, ReviewKind::ReviewRequested] {
        e.h.core.review_page(Scope::All, kind, true).await.unwrap();
    }
    let calls = *e.host.calls.lock();
    let page = e.h.core.tracker_list(shop(), None, None, None, false).await.unwrap();
    assert_eq!(*e.host.calls.lock(), calls, "PR links come from the cache, never the network");
    let linked = prs(&page, "SHOP-142");
    let got: Vec<_> = linked.iter().map(|l| (l.number, l.source, l.ci)).collect();
    assert_eq!(got, vec![(90, PrSource::WorkItem, CiState::Failure), (95, PrSource::KeyMatch, other.ci)]);
    assert_eq!(prs(&page, "SHOP-143").iter().map(|l| l.number).collect::<Vec<_>>(), vec![95]);
    assert!(prs(&page, "SHOP-141").is_empty());
    let detail = e.h.core.tracker_get(&kelta_proto::samples::ticket_ref()).await.unwrap();
    assert_eq!(detail.prs, linked);
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
        e.h.core.review_nudge(&r, Some("@anna ping")).await.unwrap();
        let page = e.h.core.review_page(Scope::All, ReviewKind::Authored, true).await.unwrap();
        assert!(page.items[0].review.nudged_at.is_some());
        e.h.core.shutdown().await.unwrap();
    }
    let e = env(tmp.path(), vec![pr]);
    let err = e.h.core.review_nudge(&r, None).await.unwrap_err();
    assert_eq!(err.code, ErrorCode::InvalidArgument, "{err:?}");
    // An older nudge no longer counts.
    let rr = r.clone();
    e.h.core.store().call(move |c| q::nudge_put(c, &rr, "2026-01-01T00:00:00Z")).await.unwrap();
    e.h.core.review_nudge(&r, Some("ping")).await.unwrap();
}

//! Ticket sources (#238): startup migration of `[project.tracker]`, source-based ticket queries,
//! `Scope::Source`, and the derived `ProjectInfo.tracker`.

use std::sync::Arc;

use crate::common::*;
use kelta_core::Core;
use kelta_core::store::q;
use kelta_proto::ErrorCode;
use kelta_proto::api::Tracker;
use kelta_proto::dirs::{CliArgs, Dirs};
use kelta_proto::events::UiEvent;
use kelta_proto::ids::{AccountId, ProjectId};
use kelta_proto::model::{Scope, WorkSource};
use kelta_proto::samples;
use kelta_proto::settings::{
    AccountKind, BlockedMarker, RepoMatch, Settings, Source, SourceField, SourceRoute, TrackerView,
};
use kelta_proto::store::ProviderCacheRow;
use kelta_proto::testing::{FakeSecrets, FakeTerminalHost, FakeTracker, FakeUiBridge};
use kelta_proto::tracker::Who;

fn view(id: &str, jql: &str) -> TrackerView {
    TrackerView { id: id.into(), label: id.into(), jql: Some(jql.into()), ..TrackerView::default() }
}

fn source(id: &str, projects: &[&str], views: Vec<TrackerView>) -> Source {
    let mut s = Source { id: id.into(), label: id.into(), ..Source::default() };
    s.projects = projects.iter().map(|p| ProjectId::new(*p)).collect();
    s.tracker.account = "jira-acme".into();
    s.tracker.views = views;
    s
}

/// shop (open) and blog (closed); sources: product (shop, blog), shop-only, closed (blog), team (none).
fn env(root: &std::path::Path) -> (H, Arc<FakeTracker>) {
    let mut s = Settings::defaults();
    s.accounts.insert("jira-acme".into(), account(AccountKind::Jira));
    let mine = view("mine", "assignee = currentUser()");
    let mut product = source("product", &["shop", "blog"], vec![mine.clone()]);
    product.tracker.status_map = kelta_proto::samples::tracker_binding().status_map;
    product.routes = vec![SourceRoute {
        r#match: RepoMatch { component: Some("Web".into()), ..RepoMatch::default() },
        project: "shop".into(),
        repo: "main".into(),
    }];
    s.sources = vec![
        product,
        source("shop-only", &["shop"], vec![view("bugs", "type = Bug"), mine]),
        source("closed", &["blog"], vec![view("blog", "project = BLOG")]),
        source("team", &[], vec![view("team", "project = TEAM")]),
    ];
    let tracker = Arc::new(FakeTracker::new());
    let factory = Arc::new(Factory::default());
    factory.trackers.lock().insert(AccountId::new("jira-acme"), tracker.clone() as Arc<dyn Tracker>);
    let cfg = MemConfig::new(s, vec![project("shop", root), project("blog", root)]);
    let h = start_in(root, cfg, FakeTerminalHost::new(), factory);
    h.core.project_open(&ProjectId::new("shop")).unwrap();
    (h, tracker)
}

fn views(h: &H, scope: Scope) -> Vec<(String, Vec<String>, Vec<String>)> {
    let ids = |v: &[ProjectId]| v.iter().map(ToString::to_string).collect();
    h.core
        .ticket_queries(&scope, None, None)
        .into_iter()
        .map(|q| (q.view.id, ids(&q.projects), q.sources))
        .collect()
}

fn strs(v: &[&str]) -> Vec<String> {
    v.iter().map(ToString::to_string).collect()
}

#[tokio::test]
async fn queries_read_sources_by_scope() {
    let tmp = tempfile::tempdir().unwrap();
    let (h, _) = env(tmp.path());
    let source = views(&h, Scope::Source { id: "shop-only".into() });
    assert_eq!(
        source,
        [
            ("bugs".into(), strs(&["shop"]), strs(&["shop-only"])),
            ("mine".into(), strs(&["shop"]), strs(&["shop-only"]))
        ]
    );
    let shop = views(&h, Scope::Project { id: "shop".into() });
    // `mine` is shared by product and shop-only: one query, union of projects and sources
    assert_eq!(
        shop,
        [
            ("mine".into(), strs(&["shop", "blog"]), strs(&["product", "shop-only"])),
            ("bugs".into(), strs(&["shop"]), strs(&["shop-only"])),
        ]
    );
    // every source, including one feeding only a closed project and one feeding none
    let all: Vec<String> = views(&h, Scope::All).into_iter().map(|v| v.0).collect();
    assert_eq!(all, ["mine", "bugs", "blog", "team"]);
    assert!(views(&h, Scope::Source { id: "nope".into() }).is_empty());
}

#[tokio::test]
async fn shared_view_is_fetched_once_and_unknown_source_is_not_found() {
    let tmp = tempfile::tempdir().unwrap();
    let (h, tracker) = env(tmp.path());
    let page = h
        .core
        .tracker_list(Scope::Project { id: "shop".into() }, None, Some("mine".into()), None, None, true)
        .await
        .unwrap();
    assert_eq!(tracker.calls().iter().filter(|c| c.starts_with("list:")).count(), 1);
    assert!(!page.items.is_empty());
    assert!(page.items.iter().all(|i| i.project_ids == [ProjectId::new("shop"), ProjectId::new("blog")]));
    let err = h
        .core
        .tracker_list(Scope::Source { id: "nope".into() }, None, None, None, None, false)
        .await
        .unwrap_err();
    assert_eq!(err.code, ErrorCode::NotFound);
}

#[tokio::test]
async fn a_fetch_emits_tickets_changed_for_the_source_scope() {
    let tmp = tempfile::tempdir().unwrap();
    let (h, tracker) = env(tmp.path());
    let team = Scope::Source { id: "team".into() };
    h.core.tracker_list(team.clone(), None, None, None, None, false).await.unwrap();
    // age the cache row → served stale + background refresh + tickets.changed
    let key = h.core.ticket_queries(&team, None, None)[0].cache_key.clone();
    let row = h.core.store().call(move |c| q::cache_get(c, &key)).await.unwrap().unwrap();
    let old = ProviderCacheRow { fetched_at: "2020-01-01T00:00:00Z".into(), ..row };
    h.core.store().call(move |c| q::cache_put(c, &old)).await.unwrap();
    h.ui.take_events();
    assert!(h.core.tracker_list(team.clone(), None, None, None, None, false).await.unwrap().stale);
    for _ in 0..50 {
        settle().await;
        if tracker.calls().iter().filter(|c| c.starts_with("list:")).count() == 2 {
            break;
        }
    }
    settle().await;
    let changed =
        h.ui.events().into_iter().any(|e| matches!(e, UiEvent::TicketsChanged { scope } if scope == team));
    assert!(changed, "{:?}", h.ui.event_names());
}

#[tokio::test]
async fn now_lists_my_tickets_of_a_source_feeding_no_project() {
    let tmp = tempfile::tempdir().unwrap();
    let (h, _) = env(tmp.path());
    // Now's Up next reads `tracker_list(All, None, Mine)`
    let all = h.core.tracker_list(Scope::All, None, None, Some(Who::Mine), None, true).await.unwrap();
    assert!(all.items.iter().any(|i| i.source_ids.contains(&"team".to_owned())));
    let team =
        h.core.tracker_list(Scope::Source { id: "team".into() }, None, None, None, None, true).await.unwrap();
    assert!(!team.items.is_empty());
    assert!(team.items.iter().all(|i| i.source_ids == ["team"] && i.project_ids.is_empty()));
}

#[tokio::test]
async fn a_tickets_pane_narrowed_to_a_source_polls_only_that_source() {
    use kelta_proto::model::{PaneContent, TicketsMode};
    let tmp = tempfile::tempdir().unwrap();
    let (h, _) = env(tmp.path());
    h.cfg.update(|s| s.notifications.enabled = false);
    let pane = |source_id: Option<&str>| PaneContent::Tickets {
        scope: Scope::All,
        view_id: None,
        source_id: source_id.map(Into::into),
        mode: TicketsMode::List,
        who: None,
        group: None,
        sort: None,
        person: None,
    };
    show_global(&h.core, Some(pane(None)));
    settle().await;
    assert_eq!(h.core.scheduler_snapshot().subscriptions, 4);
    show_global(&h.core, Some(pane(Some("team"))));
    settle().await;
    assert_eq!(h.core.scheduler_snapshot().subscriptions, 1);
    // a source that no longer exists falls back to the pane's own scope
    show_global(&h.core, Some(pane(Some("gone"))));
    settle().await;
    assert_eq!(h.core.scheduler_snapshot().subscriptions, 4);
}

#[tokio::test]
async fn tracker_list_returns_the_scope_and_view_it_resolved_the_pane_to() {
    let tmp = tempfile::tempdir().unwrap();
    let (h, _) = env(tmp.path());
    let shop = Scope::Project { id: "shop".into() };
    let list = |source: Option<&str>, view: Option<&str>| {
        let (h, shop) = (&h, shop.clone());
        let (source, view) = (source.map(str::to_owned), view.map(str::to_owned));
        async move {
            let p = h.core.tracker_list(shop, source, view, None, None, false).await.unwrap();
            (p.scope, p.view_id)
        }
    };
    let src = |id: &str| Scope::Source { id: id.into() };
    assert_eq!(list(Some("shop-only"), Some("bugs")).await, (src("shop-only"), Some("bugs".into())));
    assert_eq!(list(Some("shop-only"), Some("blog")).await, (src("shop-only"), None), "not a view of it");
    // `closed` does not feed shop, `gone` does not exist: the pane's scope, every view
    assert_eq!(list(Some("closed"), Some("blog")).await, (shop.clone(), None));
    assert_eq!(list(Some("gone"), Some("mine")).await, (shop.clone(), None));
    assert_eq!(list(None, Some("mine")).await, (shop.clone(), Some("mine".into())));
}

#[tokio::test]
async fn project_info_tracker_is_the_first_source_binding() {
    let tmp = tempfile::tempdir().unwrap();
    let (h, _) = env(tmp.path());
    let shop = h.core.project_info(&ProjectId::new("shop")).unwrap().tracker.unwrap();
    assert_eq!(shop.views[0].id, "mine");
    assert_eq!(shop.status_map, kelta_proto::samples::tracker_binding().status_map);
    assert_eq!(shop.repo_rules.len(), 1);
    assert_eq!(shop.repo_rules[0].repo, "main");
    let blog = h.core.project_info(&ProjectId::new("blog")).unwrap().tracker.unwrap();
    assert!(blog.repo_rules.is_empty(), "routes of another project stay out");
}

#[tokio::test]
async fn work_plan_routes_a_ticket_by_its_source_and_remembers_the_source() {
    let tmp = tempfile::tempdir().unwrap();
    let mut s = Settings::defaults();
    s.accounts.insert("jira-acme".into(), account(AccountKind::Jira));
    // blog listed first (the source's projects lead the choices) and the route sends Stories there.
    let mut product = source("product", &["blog", "shop"], vec![view("mine", "assignee = currentUser()")]);
    product.routes = vec![SourceRoute {
        r#match: RepoMatch { kind: Some("Story".into()), ..RepoMatch::default() },
        project: "blog".into(),
        repo: "main".into(),
    }];
    // a second source on the same account, feeding shop only
    s.sources = vec![product, source("tech", &["shop"], vec![view("bugs", "type = Bug")])];
    let factory = Arc::new(Factory::default());
    factory
        .trackers
        .lock()
        .insert(AccountId::new("jira-acme"), Arc::new(FakeTracker::new()) as Arc<dyn Tracker>);
    let cfg = MemConfig::new(s, vec![project("shop", tmp.path()), project("blog", tmp.path())]);
    let h = start_in(tmp.path(), cfg, FakeTerminalHost::new(), factory);
    for p in ["shop", "blog"] {
        h.core.project_open(&ProjectId::new(p)).unwrap();
    }
    // Uncached ticket: the route comes from the tracker.
    let ticket = kelta_proto::tracker::TicketRef {
        key: "SHOP-141".into(),
        id: "10141".into(),
        ..samples::ticket_ref()
    };
    // no row (CLI): the first source using the account
    let plan = h.core.work_plan(None, WorkSource::Ticket { ticket: ticket.clone() }, None).await.unwrap();
    assert_eq!(plan.ticket_source.as_deref(), Some("product"));
    assert_eq!(plan.project_id, ProjectId::new("blog"));
    assert_eq!(plan.project_choices, [ProjectId::new("blog"), ProjectId::new("shop")]);
    // the clicked row's source wins over that guess (#264)
    let plan = h.core.work_plan(None, WorkSource::Ticket { ticket }, Some("tech".into())).await.unwrap();
    assert_eq!(plan.ticket_source.as_deref(), Some("tech"));
    assert_eq!(plan.project_id, ProjectId::new("shop"));
}

// ---- startup migration on a real config dir -----------------------------------------------

fn boot(root: &std::path::Path) -> (Arc<Core>, Arc<FakeUiBridge>) {
    let ui = FakeUiBridge::new();
    let mut deps = kelta_core::CoreDeps::new(Dirs::under(root), CliArgs::default(), ui.clone());
    deps.terminal = Some(FakeTerminalHost::new());
    deps.secrets = Some(FakeSecrets::new());
    deps.login_env = Some(login_env(root));
    deps.in_memory_store = true;
    deps.install_ctl = false;
    deps.start_services = false;
    (Core::start_with(deps).unwrap(), ui)
}

fn toasts(ui: &FakeUiBridge) -> Vec<String> {
    ui.events()
        .into_iter()
        .filter_map(|e| match e {
            UiEvent::Toast { toast } => Some(toast.text),
            _ => None,
        })
        .collect()
}

#[test]
fn startup_moves_project_trackers_to_sources_once() {
    let tmp = tempfile::tempdir().unwrap();
    let dirs = Dirs::under(tmp.path());
    std::fs::create_dir_all(dirs.projects_dir()).unwrap();
    let board = |jql: &str| {
        format!(
            "[project.tracker]\naccount = \"jira-acme\"\nviews = [{{ id = \"mine\", label = \"Mine\", jql = \"{jql}\" }}]\n"
        )
    };
    for (id, jql) in
        [("web", "assignee = currentUser()"), ("api", "assignee = currentUser()"), ("cli", "project = CLI")]
    {
        let text = format!("[project]\nid = \"{id}\"\nname = \"{id}\"\n\n{}", board(jql));
        std::fs::write(dirs.projects_dir().join(format!("{id}.toml")), text).unwrap();
    }
    let (core, ui) = boot(tmp.path());
    // the window does not exist yet: the toast waits for `app_ready`
    assert!(toasts(&ui).is_empty());
    tokio::runtime::Runtime::new().unwrap().block_on(core.app_ready(0.0)).unwrap();
    assert_eq!(toasts(&ui), ["Ticket sources moved out of your projects"]);
    let global = std::fs::read_to_string(dirs.global_config()).unwrap();
    assert_eq!(global.matches("[[sources]]").count(), 2, "{global}");
    let trash = std::fs::read_dir(dirs.projects_dir().join(".trash")).unwrap().count();
    assert_eq!(trash, 3);
    for id in ["web", "api", "cli"] {
        let text = std::fs::read_to_string(dirs.projects_dir().join(format!("{id}.toml"))).unwrap();
        assert!(!text.contains("tracker"), "{text}");
    }
    // the next launch has nothing to move
    let (core, ui) = boot(tmp.path());
    tokio::runtime::Runtime::new().unwrap().block_on(core.app_ready(0.0)).unwrap();
    assert!(toasts(&ui).is_empty());
    assert_eq!(std::fs::read_to_string(dirs.global_config()).unwrap(), global);
}

#[test]
fn unopenable_state_database_toasts_at_app_ready() {
    let tmp = tempfile::tempdir().unwrap();
    // A directory where the database file should be: SQLite cannot open it.
    std::fs::create_dir_all(Dirs::under(tmp.path()).db_path()).unwrap();
    let ui = FakeUiBridge::new();
    let mut deps = kelta_core::CoreDeps::new(Dirs::under(tmp.path()), CliArgs::default(), ui.clone());
    deps.terminal = Some(FakeTerminalHost::new());
    deps.secrets = Some(FakeSecrets::new());
    deps.login_env = Some(login_env(tmp.path()));
    deps.install_ctl = false;
    deps.start_services = false;
    let core = Core::start_with(deps).unwrap();
    tokio::runtime::Runtime::new().unwrap().block_on(core.app_ready(0.0)).unwrap();
    let t = toasts(&ui);
    assert!(t.len() == 1 && t[0].contains("changes this session will not be saved"), "{t:?}");
}

#[tokio::test]
async fn a_source_label_marks_tickets_blocked_and_block_round_trips() {
    let tmp = tempfile::tempdir().unwrap();
    let mut s = Settings::defaults();
    s.accounts.insert("jira-acme".into(), account(AccountKind::Jira));
    let mut labelled = source("labelled", &["shop"], vec![view("mine", "assignee = currentUser()")]);
    labelled.blocked = Some(BlockedMarker::Label("blocked".into()));
    s.sources = vec![labelled, source("flagged", &[], vec![view("team", "project = TEAM")])];
    let tracker = Arc::new(FakeTracker::new());
    let factory = Arc::new(Factory::default());
    factory.trackers.lock().insert(AccountId::new("jira-acme"), tracker.clone() as Arc<dyn Tracker>);
    let h = start_in(
        tmp.path(),
        MemConfig::new(s, vec![project("shop", tmp.path())]),
        FakeTerminalHost::new(),
        factory,
    );
    let list = |id: &str| {
        let scope = Scope::Source { id: id.into() };
        let core = h.core.clone();
        async move { core.tracker_list(scope, None, None, None, None, true).await.unwrap() }
    };
    let blocked = |p: &kelta_proto::tracker::TicketPage| -> Vec<String> {
        p.items.iter().filter(|i| i.ticket.blocked).map(|i| i.ticket.r#ref.key.clone()).collect()
    };
    let page = list("labelled").await;
    assert!(blocked(&page).is_empty());
    assert!(page.items.iter().all(|i| i.caps.block), "Jira writes labels");

    let t = page.items.iter().find(|i| i.ticket.r#ref.key == "SHOP-141").unwrap().ticket.r#ref.clone();
    assert!(h.core.tracker_set_blocked(&t, true).await.unwrap().blocked);
    assert!(tracker.calls().contains(&"set_blocked:SHOP-141:true".to_owned()));
    assert_eq!(blocked(&list("labelled").await), ["SHOP-141"]);
    // Another source on the same tracker keeps Jira's default, the flag: the label means nothing there.
    assert!(blocked(&list("flagged").await).is_empty());

    assert!(!h.core.tracker_set_blocked(&t, false).await.unwrap().blocked);
    assert!(blocked(&list("labelled").await).is_empty());
}

#[tokio::test]
async fn a_source_field_reaches_the_list_call_and_the_detail() {
    let tmp = tempfile::tempdir().unwrap();
    let mut s = Settings::defaults();
    s.accounts.insert("jira-acme".into(), account(AccountKind::Jira));
    let mut chosen = source("chosen", &["shop"], vec![view("mine", "assignee = currentUser()")]);
    chosen.fields = vec![SourceField { id: "customfield_1".into(), label: "QA".into() }];
    s.sources = vec![chosen, source("plain", &[], vec![view("mine", "assignee = currentUser()")])];
    let factory = Arc::new(Factory::default());
    factory
        .trackers
        .lock()
        .insert(AccountId::new("jira-acme"), Arc::new(FakeTracker::new()) as Arc<dyn Tracker>);
    let h = start_in(
        tmp.path(),
        MemConfig::new(s, vec![project("shop", tmp.path())]),
        FakeTerminalHost::new(),
        factory,
    );
    let list = |id: &str| {
        let scope = Scope::Source { id: id.into() };
        let core = h.core.clone();
        async move { core.tracker_list(scope, None, None, None, None, true).await.unwrap() }
    };
    let page = list("chosen").await;
    let f = &page.items[0].ticket.fields;
    assert_eq!((f.len(), f[0].label.as_str(), f[0].value.as_str()), (1, "QA", "customfield_1"));
    // same view, other fields: its own query and cache entry
    let plain = list("plain").await;
    assert!(plain.items.iter().all(|i| i.ticket.fields.iter().all(|f| f.label != "QA")));
    let d = h.core.tracker_get(&page.items[0].ticket.r#ref).await.unwrap();
    assert_eq!(&d.ticket.fields, f, "the detail shows the listed fields");
}

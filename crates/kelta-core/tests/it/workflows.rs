//! Saved workflows run on a ticket (#233): discovery, the pre-claim, the background Claude session
//! and the workflow taking the claim over through `args.run`. Config workflows (#355): a prompt on a
//! ticket, a PR or with no ticket, and a second run opening the first.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::common::*;
use kelta_proto::ErrorCode;
use kelta_proto::api::{CodeHost, CoreApi, Tracker};
use kelta_proto::error::KeltaError;
use kelta_proto::ext::{WorkflowDef, WorkflowOn};
use kelta_proto::ids::{AccountId, ProjectId, SessionId};
use kelta_proto::model::{
    OpenPaneRequest, PaneContent, Placement, RunClaim, RunState, SessionKind, SpawnRequest, StartWorkPlan,
    WorkSource, WorkflowSource,
};
use kelta_proto::samples;
use kelta_proto::settings::{AccountKind, CodeHostBinding, Settings};
use kelta_proto::tracker::TicketRef;
use kelta_proto_testing::{FakeTerminalHost, FakeTracker};

fn workflow(dir: &Path, file: &str, meta: &str) {
    std::fs::create_dir_all(dir).unwrap();
    std::fs::write(
        dir.join(file),
        format!("export const meta = {meta};\nexport default async () => {{}};\n"),
    )
    .unwrap();
}

/// Project `shop` (a real git repo, the ticket's project) with repo and home workflows.
fn env(root: &Path) -> (H, PathBuf) {
    let mut s = Settings::defaults();
    s.accounts.insert("jira-acme".into(), account(AccountKind::Jira));
    s.accounts.insert("github-work".into(), account(AccountKind::Github));
    let def = |id: &str, on, prompt: &str| WorkflowDef {
        id: id.into(),
        label: format!("{id} label"),
        prompt: prompt.into(),
        on,
        ..WorkflowDef::default()
    };
    s.workflows = vec![
        def("brief", WorkflowOn::Ticket, "/brief {ticket.key} {ticket.title} from {file}"),
        def(
            "describe",
            WorkflowOn::Pr,
            "/describe-pr {pr.number} {pr.url} {pr.head}..{pr.base} {pr.repo} {ticket.key}",
        ),
        WorkflowDef { cwd: Some("~/x".into()), ..def("describe-at", WorkflowOn::Pr, "x") },
        def("daily", WorkflowOn::Global, "Write the daily report"),
        WorkflowDef { enabled: false, ..def("off", WorkflowOn::Ticket, "x") },
    ];
    let mut shop = project("shop", root);
    shop.tracker = Some(samples::tracker_binding());
    shop.repos[0].code_host =
        Some(CodeHostBinding { account: AccountId::new("github-work"), repo: "acme/shop-api".into() });
    let repo = PathBuf::from(&shop.repos[0].path);
    std::fs::remove_dir_all(repo.join(".git")).unwrap();
    git(&repo, &["init", "-q", "-b", "main"]);
    git(&repo, &["commit", "-q", "--allow-empty", "-m", "init"]);
    let factory = Arc::new(Factory::default());
    let tracker: Arc<dyn Tracker> = Arc::new(FakeTracker::new());
    factory.trackers.lock().insert(AccountId::new("jira-acme"), tracker);
    let host: Arc<dyn CodeHost> = ListHost::new(vec![samples::review()]);
    factory.hosts.lock().insert(AccountId::new("github-work"), host);
    let h = start_in(root, MemConfig::new(s, vec![shop]), FakeTerminalHost::new(), factory);
    h.core.project_open(&ProjectId::new("shop")).unwrap();
    let home = root.join("home/.claude/workflows");
    workflow(&home, "pass.js", "{ name: 'kelta-ticket-pass', description: 'Implement, review, merge' }");
    workflow(&home, "shared.js", "{ name: 'shared', description: 'from home' }");
    let local = repo.join(".claude/workflows");
    workflow(&local, "shared.js", "{ name: \"shared\", description: \"from the repo\" }");
    workflow(&local, "broken.js", "{ description: 'no name' }");
    std::fs::write(local.join("notes.txt"), "export const meta = { name: 'txt' };").unwrap();
    (h, repo)
}

#[tokio::test]
async fn lists_repo_and_home_workflows_and_the_repo_shadows_home() {
    let tmp = tempfile::tempdir().unwrap();
    let (h, repo) = env(tmp.path());
    let list = h.core.workflows_list(&samples::ticket_ref()).await.unwrap();
    let got: Vec<_> =
        list.iter().map(|w| (w.name.as_str(), w.label.as_str(), w.description.as_str(), w.source)).collect();
    assert_eq!(
        got,
        [
            ("shared", "shared", "from the repo", WorkflowSource::Repo),
            ("kelta-ticket-pass", "kelta-ticket-pass", "Implement, review, merge", WorkflowSource::Home),
            ("brief", "brief label", "", WorkflowSource::Config),
        ],
        "PR, global and disabled config workflows are not offered on a ticket"
    );
    assert_eq!(list[0].path, Some(repo.join(".claude/workflows/shared.js")));
    assert_eq!(list[2].path, None);
}

#[tokio::test]
async fn run_claims_then_starts_claude_in_a_background_tab() {
    let tmp = tempfile::tempdir().unwrap();
    let (h, repo) = env(tmp.path());
    let shop = ProjectId::new("shop");
    let t = samples::ticket_ref();
    // The pane Louis is in keeps the focus.
    let shell = h
        .core
        .session_spawn(SpawnRequest {
            id: None,
            project_id: shop.clone(),
            kind: SessionKind::Shell,
            name: None,
            program: None,
            args: vec![],
            cwd: None,
            env: Default::default(),
            cols: 80,
            rows: 24,
            work_item_id: None,
            restore: Default::default(),
            close_on_exit: Default::default(),
            template_id: None,
        })
        .await
        .unwrap();
    let req = |id| OpenPaneRequest {
        content: PaneContent::Terminal { session_id: id },
        placement: Placement::NewTab,
        focus: true,
        tab_title: None,
        work_item_id: None,
    };
    h.core.layout_open(&shop, req(shell.id.clone())).await.unwrap();
    let active = h.core.layout_get(&shop).unwrap().active_tab;

    let start = h.core.workflow_run(t.clone(), "kelta-ticket-pass").await.unwrap();
    assert!(!start.running);
    let info = start.session;
    assert!(matches!(info.kind, SessionKind::Claude));
    let (args, cwd) = h.term.with_session(&info.id, |s| (s.spec.args.clone(), s.spec.cwd.clone())).unwrap();
    assert_eq!(cwd, repo, "the run's cwd is the primary repo");
    let prompt = args.last().unwrap();
    assert!(prompt.contains("\"kelta-ticket-pass\"") && prompt.contains("SHOP-142"), "{prompt}");
    let path = |name: &str| {
        let at = prompt.find(name).unwrap() + name.len();
        PathBuf::from(prompt[..at].rsplit(' ').next().unwrap())
    };
    let json: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(path("args.json")).unwrap()).unwrap();
    let ticket_md = path("ticket.md");
    assert!(std::fs::read_to_string(&ticket_md).unwrap().starts_with("# SHOP-142: "));
    assert_eq!(json["ticket"]["key"], "SHOP-142");
    assert_eq!(json["ticket"]["file"], ticket_md.to_string_lossy().as_ref());

    let items = h.core.work_list(None).await.unwrap();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].session_ids, vec![info.id.clone()], "the item adopts the run's Claude (#491)");
    assert_eq!(items[0].claude_uuid.as_deref(), info.claude.as_ref().map(|c| c.session_uuid.as_str()));
    let run = items[0].run.clone().unwrap();
    assert_eq!(json["run"], run.run_id.clone().unwrap());
    assert_eq!((run.state, run.phase.as_deref()), (RunState::Running, Some("Starting")));
    assert_eq!((run.name.as_deref(), run.session_id.as_ref()), (Some("kelta-ticket-pass"), Some(&info.id)));
    assert_eq!(items[0].worktree.canonicalize().unwrap(), repo.canonicalize().unwrap());
    let core: &dyn CoreApi = h.core.as_ref();
    assert_eq!(core.ticket_for_session(&info.id).await, Some(t.clone()), "MCP `get_ticket` resolves");

    let layout = h.core.layout_get(&shop).unwrap();
    assert_eq!(layout.tabs.len(), 2, "a new tab");
    assert_eq!(layout.tabs[1].title, "kelta-ticket-pass · SHOP-142");
    assert_eq!(layout.active_tab, active, "focus stays where it was");

    // Held by a live run: that run is returned, nothing spawned.
    let spawned = h.term.spawned_ids().len();
    let again = h.core.workflow_run(t.clone(), "shared").await.unwrap();
    assert!(again.running);
    assert_eq!(again.session.id, info.id);
    assert_eq!(h.term.spawned_ids().len(), spawned);
    // A blocked run keeps its session and its claim: it still opens.
    core.work_report("SHOP-142", None, RunState::Blocked, Some("question".into()), None, Some(&info.id))
        .await
        .unwrap();
    let blocked = h.core.workflow_run(t.clone(), "shared").await.unwrap();
    assert!(blocked.running);
    assert_eq!(blocked.session.id, info.id);
    let err = h.core.workflow_run(t.clone(), "nope").await.unwrap_err();
    assert_eq!(err.code, ErrorCode::NotFound);

    // The workflow takes the claim over with `args.run` from its own worktree: same work item.
    let wt = tmp.path().join("runs/ticket-142");
    git(&repo, &["worktree", "add", "-q", "-b", "feat/ticket-142", wt.to_str().unwrap()]);
    let lane = RunClaim {
        name: Some("kelta-ticket-pass".into()),
        run_id: run.run_id.clone(),
        session_id: Some(SessionId::new("orchestrator")),
        phase: Some("Implement".into()),
        state: RunState::Running,
        note: None,
        sha: None,
    };
    let took = core.work_claim(&shop, t, Some(wt.clone()), None, lane).await.unwrap();
    assert_eq!(took.id, items[0].id);
    assert_eq!(took.branch, "feat/ticket-142");
    assert_eq!(took.worktree.canonicalize().unwrap(), wt.canonicalize().unwrap());
    assert_eq!(took.session_ids, vec![info.id.clone()]);

    // Opening the item moves the run's Claude into the item's tab and closes the run's.
    h.core.work().resume(&took.id).await.unwrap();
    let l = h.core.layout_get(&shop).unwrap();
    assert!(l.tabs.iter().all(|t| t.title != "kelta-ticket-pass · SHOP-142"), "the run's tab is gone");
    let tab = l.tabs.iter().find(|t| t.work_item_id.as_ref() == Some(&took.id)).unwrap();
    assert!(kelta_core::layout::sessions_in(&tab.root).contains(&info.id));
}

/// #493: a run has its own `mcp.json` (its MCP calls carry the session); its first MCP claim gives its
/// Claude to the item, a second one and a plain Claude's claims do not.
#[tokio::test]
async fn a_global_run_that_claims_through_mcp_gives_its_claude_to_the_item_once() {
    let tmp = tempfile::tempdir().unwrap();
    let (h, repo) = env(tmp.path());
    let shop = ProjectId::new("shop");
    let start = h.core.workflow_run_on(shop.clone(), "daily", None).await.unwrap();
    let sid = start.session.id.clone();
    let (args, env) = h.term.with_session(&sid, |s| (s.spec.args.clone(), s.spec.env.clone())).unwrap();
    let at = args.iter().position(|a| a == "--mcp-config").expect("--mcp-config");
    assert!(at < args.iter().position(|a| a == "--").unwrap());
    assert_eq!(args.last().unwrap(), "Write the daily report");
    let mcp: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&args[at + 1]).unwrap()).unwrap();
    assert!(mcp["mcpServers"]["kelta"]["url"].as_str().unwrap().ends_with(&format!("/mcp/{sid}")), "{mcp}");
    assert!(env.contains_key("KELTA_MCP_TOKEN"));

    let wt = tmp.path().join("runs/ticket-142");
    git(&repo, &["worktree", "add", "-q", "-b", "feat/ticket-142", wt.to_str().unwrap()]);
    let core: &dyn CoreApi = h.core.as_ref();
    let claim = |session: &SessionId| RunClaim {
        name: Some("daily".into()),
        session_id: Some(session.clone()),
        ..RunClaim::default()
    };
    let w = core.work_claim(&shop, samples::ticket_ref(), Some(wt.clone()), None, claim(&sid)).await.unwrap();
    assert_eq!(w.run.as_ref().unwrap().session_id.as_ref(), Some(&sid));
    assert_eq!(w.session_ids, vec![sid.clone()]);
    assert_eq!(w.claude_uuid.as_deref(), start.session.claude.as_ref().map(|c| c.session_uuid.as_str()));
    assert_eq!(
        w.worktree.canonicalize().unwrap(),
        wt.canonicalize().unwrap(),
        "the editor and git use the named worktree"
    );
    assert_eq!(start.session.cwd, repo, "the respawn cwd stays the session's");
    h.core.work().resume(&w.id).await.unwrap();
    let l = h.core.layout_get(&shop).unwrap();
    assert!(l.tabs.iter().all(|t| t.title != "daily label"), "the run's tab is gone");
    let tab = l.tabs.iter().find(|t| t.work_item_id.as_ref() == Some(&w.id)).unwrap();
    assert!(kelta_core::layout::sessions_in(&tab.root).contains(&sid));

    // At most one item per run: the next claim only records the session.
    let next = |key: &str| TicketRef { key: key.into(), id: key.into(), ..samples::ticket_ref() };
    let second = core.work_claim(&shop, next("SHOP-143"), None, None, claim(&sid)).await.unwrap();
    assert_eq!(second.run.unwrap().session_id, Some(sid.clone()));
    assert!(second.session_ids.is_empty());
    let first = h.core.work_list(None).await.unwrap().into_iter().find(|i| i.id == w.id).unwrap();
    assert!(first.session_ids.contains(&sid), "still the run's");

    // A plain Kelta Claude (a board orchestrator) is never adopted.
    let plain = h
        .core
        .session_spawn(SpawnRequest {
            id: None,
            project_id: shop.clone(),
            kind: SessionKind::Claude,
            name: None,
            program: Some("claude".into()),
            args: vec![],
            cwd: None,
            env: Default::default(),
            cols: 80,
            rows: 24,
            work_item_id: None,
            restore: Default::default(),
            close_on_exit: Default::default(),
            template_id: None,
        })
        .await
        .unwrap();
    let third = core.work_claim(&shop, next("SHOP-144"), None, None, claim(&plain.id)).await.unwrap();
    assert_eq!(third.run.unwrap().session_id, Some(plain.id));
    assert!(third.session_ids.is_empty());
}

#[tokio::test]
async fn a_failed_spawn_leaves_no_live_claim() {
    let tmp = tempfile::tempdir().unwrap();
    let (h, _) = env(tmp.path());
    let t = samples::ticket_ref();
    h.term.fail_next_spawn(KeltaError::internal("no pty"));
    assert!(h.core.workflow_run(t.clone(), "kelta-ticket-pass").await.is_err());
    let items = h.core.work_list(None).await.unwrap();
    assert!(items.iter().all(|w| w.run.as_ref().is_some_and(|r| r.state == RunState::Done)), "{items:?}");
    // The ticket is free again.
    h.core.workflow_run(t, "kelta-ticket-pass").await.unwrap();
}

fn spawned(h: &H, info: &kelta_proto::model::SessionInfo) -> (String, PathBuf) {
    h.term.with_session(&info.id, |s| (s.spec.args.last().cloned().unwrap(), s.spec.cwd.clone())).unwrap()
}

#[tokio::test]
async fn a_config_prompt_runs_on_a_ticket_with_its_placeholders() {
    let tmp = tempfile::tempdir().unwrap();
    let (h, repo) = env(tmp.path());
    let t = samples::ticket_ref();
    let start = h.core.workflow_run(t.clone(), "brief").await.unwrap();
    let (prompt, cwd) = spawned(&h, &start.session);
    assert!(prompt.starts_with("/brief SHOP-142 "), "{prompt}");
    assert!(prompt.ends_with("ticket.md"), "{prompt}");
    assert_eq!(cwd, repo);
    let run = h.core.work_list(None).await.unwrap()[0].run.clone().unwrap();
    assert_eq!((run.name.as_deref(), run.phase), (Some("brief"), None));
    assert_eq!(
        h.core.layout_get(&ProjectId::new("shop")).unwrap().tabs.last().unwrap().title,
        "brief label · SHOP-142"
    );
    let err = h.core.workflow_run(t, "off").await.unwrap_err();
    assert_eq!(err.code, ErrorCode::NotFound, "a disabled workflow is not offered");
}

#[tokio::test]
async fn a_pr_workflow_runs_in_the_bound_repo_once_per_pr() {
    let tmp = tempfile::tempdir().unwrap();
    let (h, repo) = env(tmp.path());
    let shop = ProjectId::new("shop");
    let r = samples::review_ref();
    let start = h.core.workflow_run_on(shop.clone(), "describe", Some(r.clone())).await.unwrap();
    assert!(!start.running);
    let (prompt, cwd) = spawned(&h, &start.session);
    assert_eq!(
        prompt,
        "/describe-pr 87 https://github.com/acme/shop-api/pull/87 feat/SHOP-140-cache-prices..main acme/shop-api SHOP-140"
    );
    assert_eq!(cwd, repo, "no work item: the repo bound to acme/shop-api");
    assert!(h.core.work_list(None).await.unwrap().is_empty(), "no ticket claimed");
    assert_eq!(
        h.core.layout_get(&shop).unwrap().tabs.last().unwrap().title,
        "describe label · acme/shop-api#87"
    );

    // A second run on the PR opens the first.
    let n = h.term.spawned_ids().len();
    let again = h.core.workflow_run_on(shop.clone(), "describe", Some(r.clone())).await.unwrap();
    assert!(again.running);
    assert_eq!(again.session.id, start.session.id);
    assert_eq!(h.term.spawned_ids().len(), n);

    // A ticket workflow is not a PR one.
    let err = h.core.workflow_run_on(shop, "brief", Some(r)).await.unwrap_err();
    assert_eq!(err.code, ErrorCode::NotFound);
}

#[tokio::test]
async fn a_global_workflow_runs_with_no_ticket() {
    let tmp = tempfile::tempdir().unwrap();
    let (h, repo) = env(tmp.path());
    let start = h.core.workflow_run_on(ProjectId::new("shop"), "daily", None).await.unwrap();
    let (prompt, cwd) = spawned(&h, &start.session);
    assert_eq!((prompt.as_str(), cwd), ("Write the daily report", repo));
    assert!(h.core.work_list(None).await.unwrap().is_empty());
    let err = h.core.workflow_run_on(ProjectId::new("shop"), "describe", None).await.unwrap_err();
    assert_eq!(err.code, ErrorCode::NotFound, "a PR workflow needs its PR");
}

#[tokio::test]
async fn a_pr_workflow_runs_in_its_work_items_worktree_unless_the_def_sets_a_cwd() {
    let tmp = tempfile::tempdir().unwrap();
    let (h, repo) = env(tmp.path());
    let shop = ProjectId::new("shop");
    let r = samples::review_ref();
    let plan = h
        .core
        .work_plan(Some(shop.clone()), WorkSource::Review { review: r.clone() }, None, false)
        .await
        .unwrap();
    let plan = StartWorkPlan { worktree_path: tmp.path().join("wt/review-87"), ..plan };
    // The saga fetches the PR head from `origin`: the repo is its own remote.
    git(&repo, &["remote", "add", "origin", repo.to_str().unwrap()]);
    git(&repo, &["update-ref", "refs/pull/87/head", "HEAD"]);
    let item = h.core.work_start(plan).await.unwrap();
    assert_eq!(item.review, Some(r.clone()));
    assert_ne!(item.worktree, repo);

    std::fs::create_dir_all(tmp.path().join("home/x")).unwrap();
    // `cwd` in the def beats the work item's worktree; `~` expands to the home dir.
    let at = h.core.workflow_run_on(shop.clone(), "describe-at", Some(r.clone())).await.unwrap();
    assert_eq!(spawned(&h, &at.session).1, tmp.path().join("home/x"));
    h.core.session_kill(&at.session.id, true).await.unwrap();

    let start = h.core.workflow_run_on(shop, "describe", Some(r)).await.unwrap();
    assert!(!start.running);
    assert_eq!(spawned(&h, &start.session).1, item.worktree, "the PR's work item wins over the bound repo");
}

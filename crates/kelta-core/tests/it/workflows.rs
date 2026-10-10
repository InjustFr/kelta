//! Saved workflows run on a ticket (#233): discovery, the pre-claim, the background Claude session
//! and the workflow taking the claim over through `args.run`.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;

use crate::common::*;
use kelta_proto::ErrorCode;
use kelta_proto::api::{CoreApi, Tracker};
use kelta_proto::error::KeltaError;
use kelta_proto::ids::{AccountId, ProjectId, SessionId};
use kelta_proto::model::{
    OpenPaneRequest, PaneContent, Placement, RunClaim, RunState, SessionKind, SpawnRequest, WorkflowSource,
};
use kelta_proto::samples;
use kelta_proto::settings::{AccountKind, Settings};
use kelta_proto::testing::{FakeTerminalHost, FakeTracker};

fn git(dir: &Path, args: &[&str]) {
    let out = Command::new("git")
        .args(["-c", "commit.gpgsign=false", "-c", "user.name=Kelta Test", "-c", "user.email=test@kelta.dev"])
        .args(args)
        .current_dir(dir)
        .output()
        .unwrap();
    assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
}

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
    let mut shop = project("shop", root);
    shop.tracker = Some(samples::tracker_binding());
    let repo = PathBuf::from(&shop.repos[0].path);
    std::fs::remove_dir_all(repo.join(".git")).unwrap();
    git(&repo, &["init", "-q", "-b", "main"]);
    git(&repo, &["commit", "-q", "--allow-empty", "-m", "init"]);
    let factory = Arc::new(Factory::default());
    let tracker: Arc<dyn Tracker> = Arc::new(FakeTracker::new());
    factory.trackers.lock().insert(AccountId::new("jira-acme"), tracker);
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
    let got: Vec<_> = list.iter().map(|w| (w.name.as_str(), w.description.as_str(), w.source)).collect();
    assert_eq!(
        got,
        [
            ("shared", "from the repo", WorkflowSource::Repo),
            ("kelta-ticket-pass", "Implement, review, merge", WorkflowSource::Home)
        ]
    );
    assert_eq!(list[0].path, repo.join(".claude/workflows/shared.js"));
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

    let info = h.core.workflow_run(t.clone(), "kelta-ticket-pass").await.unwrap();
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

    // Held: refused, nothing spawned.
    let spawned = h.term.spawned_ids().len();
    let err = h.core.workflow_run(t.clone(), "shared").await.unwrap_err();
    assert_eq!(err.code, ErrorCode::Conflict, "{err:?}");
    assert!(err.message.starts_with("SHOP-142 is held by session "), "{}", err.message);
    assert_eq!(h.term.spawned_ids().len(), spawned);
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

//! Start-work saga against FakeCore + FakeTracker + a temp git repo.

use std::sync::Arc;

use crate::common::{Fx, git, has_git, project};
use async_trait::async_trait;
use kelta_proto::error::{ErrorCode, KeltaError};
use kelta_proto::events::BusEvent;
use kelta_proto::ext::BlockingOutcome;
use kelta_proto::ids::AccountId;
use kelta_proto::model::{
    BranchChoice, PaneContent, Placement, SessionKind, StepStatus, WORK_STEPS, WorkSource, WorkState,
};
use kelta_proto::samples;
use kelta_proto::tracker::TicketRef;
use kelta_work::WorkHost;

fn ticket(key: &str) -> WorkSource {
    let mut t: TicketRef = samples::ticket_ref();
    t.key = key.into();
    WorkSource::Ticket { ticket: t }
}

macro_rules! need_git {
    () => {
        if !has_git() {
            eprintln!("skipping: git not found");
            return;
        }
    };
}

#[tokio::test]
async fn plan_templating_and_unicode_slug() {
    need_git!();
    let fx = Fx::new();
    let w = fx.service();
    let plan = w.plan(&project(), ticket("SHOP-141")).await.unwrap();
    assert_eq!(plan.repo_id, "api");
    assert_eq!(plan.repo_choices, vec!["api".to_owned()]);
    assert_eq!(plan.base, "main");
    assert_eq!(plan.branch, "feat/SHOP-141-add-login-form");
    assert_eq!(plan.worktree_path, fx.wt_root.join("shop/api/SHOP-141-add-login-form"));
    assert_eq!(plan.template_id, "claude+editor");
    assert!(plan.branch_exists.is_none());
    assert!(plan.existing.is_none());
    assert_eq!(plan.claude.profile, "default");
    assert!(plan.claude.prompt.starts_with("Work on SHOP-141: Add login form."), "{}", plan.claude.prompt);
    assert!(
        plan.claude.prompt.contains("{run}/ticket.md"),
        "run is resolved at launch: {}",
        plan.claude.prompt
    );
    assert!(plan.side_effects.assign_me);
    assert!(!plan.side_effects.run_setup);

    // Unicode title, bug type, 40-char cut at a word boundary.
    let mut d = fx.tracker.ticket("SHOP-141").unwrap();
    d.ticket.r#ref.key = "SHOP-9".into();
    d.ticket.title = "Ünïcödé façade — naïve Straße: handle très long titles gracefully please".into();
    d.ticket.kind = Some("Bug".into());
    let tr = Arc::new(kelta_proto::testing::FakeTracker::with_tickets(vec![d]));
    fx.core.add_tracker(AccountId::new("jira-acme"), tr);
    let plan = w.plan(&project(), ticket("SHOP-9")).await.unwrap();
    let slug = plan.branch.strip_prefix("fix/SHOP-9-").unwrap();
    assert!(slug.len() <= 40, "{slug}");
    assert_eq!(slug, "unicode-facade-naive-strasse-handle-tres");
}

#[tokio::test]
async fn invalid_branch_template_is_sanitized_or_rejected() {
    need_git!();
    let fx = Fx::new();
    fx.settings(|s| s.worktree.branch_template = "{type}/{key} weird..name~".into());
    let plan = fx.service().plan(&project(), ticket("SHOP-141")).await.unwrap();
    assert_eq!(plan.branch, "feat/SHOP-141-weird.name");
    let e = fx
        .service()
        .plan(&project(), WorkSource::Branch { name: "-bad".into(), task: None, repo: None })
        .await
        .unwrap_err();
    assert_eq!(e.code, ErrorCode::InvalidArgument);
}

#[tokio::test]
async fn start_work_end_to_end() {
    need_git!();
    let fx = Fx::new();
    let w = fx.service();
    let plan = w.plan(&project(), ticket("SHOP-141")).await.unwrap();
    let item = w.start(plan.clone()).await.unwrap();
    assert_eq!(item.state, WorkState::Active, "{:?}", item.steps);
    assert_eq!(item.steps.len(), WORK_STEPS.len());
    assert!(item.steps.iter().all(|s| s.status == StepStatus::Done), "{:?}", item.steps);

    // git: worktree on the new branch, .kelta/ excluded, .env copied.
    assert!(item.worktree.join("README.md").exists());
    assert_eq!(git(&item.worktree, &["rev-parse", "--abbrev-ref", "HEAD"]), "feat/SHOP-141-add-login-form");
    let exclude = std::fs::read_to_string(fx.repo.join(".git/info/exclude")).unwrap();
    assert!(exclude.lines().any(|l| l == ".kelta/"));
    assert_eq!(std::fs::read_to_string(item.worktree.join(".env")).unwrap(), "SECRET=1\n");

    // Sessions: editor + claude, both linked to the work item.
    let claude = fx.spawned_of(|k| *k == SessionKind::Claude);
    let editor = fx.spawned_of(|k| matches!(k, SessionKind::Editor { .. }));
    assert_eq!((claude.len(), editor.len()), (1, 1));
    assert_eq!(item.session_ids.len(), 2);
    assert!(item.session_ids.contains(&claude[0].id) && item.session_ids.contains(&editor[0].id));
    assert_eq!(claude[0].work_item_id.as_ref(), Some(&item.id));
    assert_eq!(item.nvim_socket.as_ref().map(|s| s.ends_with("nvim.sock")), Some(true));

    // Claude argv / files.
    let calls = fx.core.calls();
    let spawn_claude =
        calls.iter().find(|c| c.method == "session_spawn" && c.args["kind"]["type"] == "claude").unwrap();
    let args: Vec<String> = serde_json::from_value(spawn_claude.args["args"].clone()).unwrap();
    assert_eq!(args[0], "--session-id");
    assert_eq!(Some(&args[1]), item.claude_uuid.as_ref());
    assert!(args.windows(2).any(|w| w[0] == "-n" && w[1] == "SHOP-141 Add login form"));
    assert!(args.windows(2).any(|w| w[0] == "--permission-mode" && w[1] == "acceptEdits"));
    let settings_file = args.windows(2).find(|w| w[0] == "--settings").map(|w| w[1].clone()).unwrap();
    let run = std::path::Path::new(&settings_file).parent().unwrap().to_path_buf();
    assert!(run.starts_with(fx.dirs.runtime.join("s")));
    let prompt = args.last().unwrap();
    assert!(prompt.contains(&format!("{}/ticket.md", run.display())), "{prompt}");
    let ticket_md = std::fs::read_to_string(run.join("ticket.md")).unwrap();
    assert!(ticket_md.starts_with("# SHOP-141: Add login form"));
    assert!(run.join("context.md").exists());
    // Not wired to the HTTP server: no MCP config.
    assert!(!args.contains(&"--mcp-config".to_owned()));
    assert_eq!(spawn_claude.args["cwd"], serde_json::json!(item.worktree));

    // Ordering: progress tab → editor spawn → claude spawn → panes (root replaced) → effects.
    let order: Vec<&str> = calls
        .iter()
        .filter(|c| matches!(c.method, "layout_open" | "session_spawn"))
        .map(|c| match (c.method, c.args["kind"]["type"].as_str(), c.args["req"]["placement"].as_str()) {
            ("session_spawn", Some(k), _) => k,
            (_, _, Some(p)) => p,
            _ => "?",
        })
        .collect();
    assert_eq!(
        order,
        vec![
            "new_tab",
            "editor",
            "claude",
            "focused",
            "replace_focused",
            "focused",
            "split_right",
            "focused"
        ],
        "{order:?}"
    );
    let opened = fx.core.opened();
    assert_eq!(opened[0].1.content, PaneContent::WorkItem { id: item.id.clone() });
    assert_eq!(opened[0].1.tab_title.as_deref(), Some("SHOP-141 Add login form"));
    assert_eq!(opened[0].1.placement, Placement::NewTab);
    assert_eq!(item.tab_id.as_ref().map(|t| t.as_str()), Some("tab-1"));

    // Tracker side effects: assign me, transition To Do → In Progress (after the spawns).
    let tcalls = fx.tracker.calls();
    assert!(tcalls.contains(&"assign:SHOP-141".to_owned()), "{tcalls:?}");
    assert!(tcalls.contains(&"transition:SHOP-141:t3".to_owned()), "{tcalls:?}");
    assert_eq!(fx.tracker.ticket("SHOP-141").unwrap().ticket.status.name, "In Progress");
    let names = fx.core.published().into_iter().map(|e| e.name).collect::<Vec<_>>();
    for n in [
        "ticket.before_start",
        "ticket.assigned",
        "ticket.transitioned",
        "ticket.started",
        "worktree.created",
        "work.updated",
    ] {
        assert!(names.iter().any(|x| x == n), "missing {n} in {names:?}");
    }
    let started = names.iter().position(|n| n == "ticket.started").unwrap();
    let transitioned = names.iter().position(|n| n == "ticket.transitioned").unwrap();
    assert!(transitioned < started);

    // Listing / for_session / plan resume.
    assert_eq!(w.list(Some(&project())).await.unwrap().len(), 1);
    assert_eq!(w.for_session(&claude[0].id).await.map(|i| i.id), Some(item.id.clone()));
    let again = w.plan(&project(), ticket("SHOP-141")).await.unwrap();
    assert_eq!(again.existing.as_ref(), Some(&item.id));
    // Starting the "Resume" plan focuses instead of duplicating.
    let resumed = w.start(again).await.unwrap();
    assert_eq!(resumed.id, item.id);
    assert_eq!(fx.spawned_of(|k| *k == SessionKind::Claude).len(), 1);
    assert_eq!(fx.worktree_count(), 2);
}

#[tokio::test]
async fn crash_after_every_step_resumes_without_duplicates() {
    need_git!();
    for step in WORK_STEPS {
        let fx = Fx::new();
        let w = fx.service();
        let plan = w.plan(&project(), ticket("SHOP-141")).await.unwrap();
        w.set_crash_after(Some(step));
        let err = w.start(plan).await.unwrap_err();
        assert!(err.message.contains("simulated crash"), "{step}: {err:?}");
        drop(w);
        let items = fx.store_items().await;
        assert_eq!(items.len(), 1, "{step}");
        let expect = if *step == "persist" { WorkState::Active } else { WorkState::Starting };
        assert_eq!(items[0].state, expect, "{step}");

        // "Restart": a fresh service over the same store/core resumes the saga.
        let w2 = fx.service();
        let done = w2.resume(&items[0].id).await.unwrap();
        assert_eq!(done.state, WorkState::Active, "{step}: {:?}", done.steps);
        assert!(done.steps.iter().all(|s| s.status == StepStatus::Done), "{step}: {:?}", done.steps);
        assert_eq!(fx.worktree_count(), 2, "{step}: one worktree");
        assert_eq!(fx.spawned_of(|k| *k == SessionKind::Claude).len(), 1, "{step}: one claude");
        assert_eq!(fx.spawned_of(|k| matches!(k, SessionKind::Editor { .. })).len(), 1, "{step}: one editor");
        let t = fx.tracker.calls();
        let count = |p: &str| t.iter().filter(|c| c.starts_with(p)).count();
        assert_eq!(count("assign:"), 1, "{step}: {t:?}");
        assert_eq!(count("transition:"), 1, "{step}: {t:?}");
        let new_tabs = fx.core.opened().iter().filter(|(_, r)| r.placement == Placement::NewTab).count();
        assert_eq!(new_tabs, 1, "{step}: one tab");
    }
}

impl Fx {
    async fn store_items(&self) -> Vec<kelta_proto::model::WorkItem> {
        use kelta_proto::api::WorkStore;
        self.store.list_items(None).await.unwrap()
    }
}

#[tokio::test]
async fn startup_marks_interrupted_sagas_failed_then_retry() {
    need_git!();
    let fx = Fx::new();
    let w = fx.service();
    let plan = w.plan(&project(), ticket("SHOP-141")).await.unwrap();
    w.set_crash_after(Some("worktree"));
    w.start(plan).await.unwrap_err();
    let w2 = fx.service();
    w2.startup().await.unwrap();
    let item = w2.list(None).await.unwrap().remove(0);
    assert_eq!(
        item.state,
        WorkState::Failed {
            step: "include_files".into(),
            message: "interrupted (Kelta quit during start)".into()
        }
    );
    let lazygit = std::fs::read_to_string(fx.dirs.data.join("lazygit-kelta.yml")).unwrap();
    assert!(lazygit.contains("editor-open {{filename}}:{{line}}"), "{lazygit}");
    let done = w2.retry_step(&item.id, "include_files").await.unwrap();
    assert_eq!(done.state, WorkState::Active);
    assert_eq!(fx.worktree_count(), 2);
}

#[tokio::test]
async fn collision_suffix_and_reuse() {
    need_git!();
    let fx = Fx::new();
    git(&fx.repo, &["branch", "feat/SHOP-141-add-login-form"]);
    let w = fx.service();
    let mut plan = w.plan(&project(), ticket("SHOP-141")).await.unwrap();
    let be = plan.branch_exists.clone().unwrap();
    assert!(!be.has_worktree);
    assert_eq!(be.choice, BranchChoice::Reuse);

    plan.branch_exists.as_mut().unwrap().choice = BranchChoice::Suffix;
    let item = w.start(plan.clone()).await.unwrap();
    assert_eq!(item.state, WorkState::Active, "{:?}", item.steps);
    assert_eq!(item.branch, "feat/SHOP-141-add-login-form-2");
    assert!(item.worktree.to_string_lossy().ends_with("SHOP-141-add-login-form-2"));

    // Reuse: the existing branch gets its own worktree.
    let fx = Fx::new();
    git(&fx.repo, &["branch", "feat/SHOP-141-add-login-form"]);
    let w = fx.service();
    let plan = w.plan(&project(), ticket("SHOP-141")).await.unwrap();
    let item = w.start(plan).await.unwrap();
    assert_eq!(item.branch, "feat/SHOP-141-add-login-form");
    assert_eq!(git(&item.worktree, &["rev-parse", "--abbrev-ref", "HEAD"]), "feat/SHOP-141-add-login-form");

    // A branch already checked out in another worktree → that worktree is reused.
    let fx = Fx::new();
    let other = fx.tmp.path().join("elsewhere");
    git(
        &fx.repo,
        &["worktree", "add", "-q", "-b", "feat/SHOP-141-add-login-form", other.to_str().unwrap(), "main"],
    );
    let w = fx.service();
    let plan = w.plan(&project(), ticket("SHOP-141")).await.unwrap();
    assert!(plan.branch_exists.as_ref().unwrap().has_worktree);
    let item = w.start(plan).await.unwrap();
    assert_eq!(item.state, WorkState::Active, "{:?}", item.steps);
    assert!(kelta_work::git::same_path(&item.worktree, &other));
    assert_eq!(fx.worktree_count(), 2);
}

#[tokio::test]
async fn setup_blocking_orders_claude_after_setup_and_skip_continues() {
    need_git!();
    let fx = Fx::new();
    fx.settings(|s| s.worktree.setup = vec!["pnpm install --frozen-lockfile".into(), "echo 'a b'".into()]);
    let w = fx.service();
    let plan = w.plan(&project(), ticket("SHOP-141")).await.unwrap();
    assert!(plan.side_effects.run_setup);
    let task = tokio::spawn({
        let w = w.clone();
        async move { w.start(plan).await }
    });
    let setup = fx.wait_session(|s| s.kind == SessionKind::Setup).await;
    // Claude must not start while setup runs (setup_blocking = true).
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    assert!(fx.spawned_of(|k| *k == SessionKind::Claude).is_empty());
    let spawn = fx.core.calls().into_iter().find(|c| c.method == "session_spawn").unwrap();
    assert_eq!(spawn.args["program"], "/bin/sh");
    assert_eq!(spawn.args["args"][1], "set -e\n'pnpm' 'install' '--frozen-lockfile'\n'echo' 'a b'\n");
    fx.core.exit_session(&setup, 1);
    let item = task.await.unwrap().unwrap();
    assert_eq!(item.state, WorkState::Failed { step: "setup".into(), message: item_msg(&item) });
    assert!(fx.spawned_of(|k| *k == SessionKind::Claude).is_empty());

    // "Continue anyway".
    let item = w.retry_step(&item.id, "skip:setup").await.unwrap();
    assert_eq!(item.state, WorkState::Active, "{:?}", item.steps);
    assert_eq!(item.steps.iter().find(|s| s.step == "setup").unwrap().status, StepStatus::Skipped);
    assert_eq!(fx.spawned_of(|k| *k == SessionKind::Claude).len(), 1);
    // Setup pane was split below the progress pane.
    let opened = fx.core.opened();
    assert!(opened.iter().any(|(_, r)| r.placement == Placement::SplitDown
        && r.content == PaneContent::Terminal { session_id: setup.clone() }));
}

fn item_msg(item: &kelta_proto::model::WorkItem) -> String {
    match &item.state {
        WorkState::Failed { message, .. } => {
            assert!(message.contains("setup failed (exit 1)"), "{message}");
            message.clone()
        }
        other => panic!("{other:?}"),
    }
}

#[tokio::test]
async fn setup_success_then_claude() {
    need_git!();
    let fx = Fx::new();
    fx.settings(|s| s.worktree.setup = vec!["true".into()]);
    let w = fx.service();
    let plan = w.plan(&project(), ticket("SHOP-141")).await.unwrap();
    let task = tokio::spawn({
        let w = w.clone();
        async move { w.start(plan).await }
    });
    let setup = fx.wait_session(|s| s.kind == SessionKind::Setup).await;
    fx.core.exit_session(&setup, 0);
    let item = task.await.unwrap().unwrap();
    assert_eq!(item.state, WorkState::Active);
    let names: Vec<String> = fx
        .core
        .calls()
        .into_iter()
        .filter(|c| c.method == "session_spawn")
        .map(|c| c.args["kind"]["type"].as_str().unwrap().to_owned())
        .collect();
    assert_eq!(names, vec!["setup", "editor", "claude"]);
}

struct Host {
    veto: bool,
}

#[async_trait]
impl WorkHost for Host {
    async fn ensure_http(&self) -> Result<u16, KeltaError> {
        Ok(4242)
    }
    fn release_http(&self) {}
    async fn run_blocking(&self, ev: &BusEvent) -> Result<BlockingOutcome, KeltaError> {
        assert_eq!(ev.name, "ticket.before_start");
        Ok(if self.veto {
            BlockingOutcome::Veto { trigger_id: "guard".into(), reason: "frozen".into() }
        } else {
            BlockingOutcome::Proceed {
                patch: Some(
                    serde_json::json!({ "branch": "feat/patched", "claude": { "prompt": "Go {ticket.key}" } }),
                ),
            }
        })
    }
}

#[tokio::test]
async fn host_veto_patch_and_mcp_port() {
    need_git!();
    let fx = Fx::new();
    let w = fx.service();
    w.set_host(Arc::new(Host { veto: true }));
    let plan = w.plan(&project(), ticket("SHOP-141")).await.unwrap();
    let item = w.start(plan).await.unwrap();
    match &item.state {
        WorkState::Failed { step, message } => {
            assert_eq!(step, "before_start");
            assert!(message.contains("frozen"));
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(fx.worktree_count(), 1);

    let fx = Fx::new();
    let w = fx.service();
    w.set_host(Arc::new(Host { veto: false }));
    let plan = w.plan(&project(), ticket("SHOP-141")).await.unwrap();
    let item = w.start(plan).await.unwrap();
    assert_eq!(item.state, WorkState::Active, "{:?}", item.steps);
    assert_eq!(item.branch, "feat/patched");
    let call = fx
        .core
        .calls()
        .into_iter()
        .find(|c| c.method == "session_spawn" && c.args["kind"]["type"] == "claude")
        .unwrap();
    let args: Vec<String> = serde_json::from_value(call.args["args"].clone()).unwrap();
    assert_eq!(args.last().unwrap(), "Go SHOP-141");
    let mcp = args.windows(2).find(|w| w[0] == "--mcp-config").map(|w| w[1].clone()).unwrap();
    let json: serde_json::Value = serde_json::from_slice(&std::fs::read(&mcp).unwrap()).unwrap();
    let sid = fx.spawned_of(|k| *k == SessionKind::Claude)[0].id.clone();
    assert_eq!(
        json["mcpServers"]["kelta"]["url"],
        format!("http://127.0.0.1:4242/mcp/{}", sid.as_str()),
        "mcp.json points at the session id Claude is spawned as"
    );
    assert_eq!(call.args["id"], sid.as_str());
}

#[tokio::test]
async fn review_locally() {
    need_git!();
    let fx = Fx::new();
    let w = fx.service();
    let source = WorkSource::Review { review: samples::review_ref() };
    let plan = w.plan(&project(), source).await.unwrap();
    assert_eq!(plan.branch, "kelta/pr-87");
    assert_eq!(plan.worktree_path, fx.wt_root.join("shop/api/review-87"));
    assert_eq!(plan.template_id, "review");
    assert_eq!(plan.claude.profile, "review");
    assert_eq!(serde_json::to_value(plan.claude.permission_mode).unwrap(), "plan");
    assert!(plan.claude.prompt.contains("https://github.com/acme/shop-api/pull/87"));
    let item = w.start(plan).await.unwrap();
    assert_eq!(item.state, WorkState::Active, "{:?}", item.steps);
    assert!(item.worktree.join("feature.txt").exists(), "worktree at the PR head");
    let shell = fx.spawned_of(|k| *k == SessionKind::Shell);
    assert_eq!(shell.len(), 1);
    // shell commands quote placeholders: a PR's base branch name cannot run as shell code
    assert_eq!(fx.core.written_text(&shell[0].id), "git diff --stat origin/'main'...HEAD\r");
    // No tracker side effects for reviews.
    assert!(fx.tracker.calls().is_empty());
    let placements: Vec<Placement> = fx.core.opened().iter().map(|(_, r)| r.placement).collect();
    assert!(placements.contains(&Placement::SplitRight) && placements.contains(&Placement::SplitDown));
}

#[tokio::test]
async fn branch_workspace_without_ticket() {
    need_git!();
    let fx = Fx::new();
    let w = fx.service();
    let plan = w
        .plan(&project(), WorkSource::Branch { name: "spike/caching".into(), task: None, repo: None })
        .await
        .unwrap();
    assert_eq!(plan.branch, "spike/caching");
    assert_eq!(plan.worktree_path, fx.wt_root.join("shop/api/spike-caching"));
    assert!(!plan.side_effects.assign_me && plan.side_effects.transition_to.is_none());
    let item = w.start(plan).await.unwrap();
    assert_eq!(item.state, WorkState::Active, "{:?}", item.steps);
    assert!(fx.tracker.calls().is_empty());
}

#[tokio::test]
async fn resume_respawns_exited_claude_with_continue_fallback() {
    need_git!();
    let fx = Fx::new();
    let w = fx.service();
    let plan = w.plan(&project(), ticket("SHOP-141")).await.unwrap();
    let item = w.start(plan).await.unwrap();
    let first = fx.spawned_of(|k| *k == SessionKind::Claude)[0].id.clone();
    fx.core.exit_session(&first, 0);

    let resumed = w.resume(&item.id).await.unwrap();
    let claude_args = |fx: &Fx| -> Vec<Vec<String>> {
        fx.core
            .calls()
            .into_iter()
            .filter(|c| c.method == "session_spawn" && c.args["kind"]["type"] == "claude")
            .map(|c| serde_json::from_value(c.args["args"].clone()).unwrap())
            .collect()
    };
    let spawns = claude_args(&fx);
    assert_eq!(spawns.len(), 2);
    assert_eq!(spawns[1][..2], ["--resume".to_owned(), item.claude_uuid.clone().unwrap()]);
    assert!(!spawns[1].iter().any(|a| a.starts_with("Work on")), "no prompt on resume");
    let second = fx.spawned_of(|k| *k == SessionKind::Claude).into_iter().find(|s| s.id != first).unwrap().id;
    assert!(resumed.session_ids.contains(&second) && !resumed.session_ids.contains(&first));
    let new_tabs = fx.core.opened().iter().filter(|(_, r)| r.placement == Placement::NewTab).count();
    assert_eq!(new_tabs, 2, "tab recreated");

    // `--resume` refused: fast non-zero exit → `--continue` in the same pane.
    fx.core.exit_session(&second, 1);
    for _ in 0..300 {
        if claude_args(&fx).len() == 3 {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    let spawns = claude_args(&fx);
    assert_eq!(spawns.len(), 3, "continue fallback spawned");
    assert_eq!(spawns[2][0], "--continue");
    let last = fx.core.opened().last().cloned().unwrap().1;
    assert_eq!(last.placement, Placement::ReplaceFocused);
}

#[tokio::test]
async fn claude_restore_request_regenerates_files() {
    need_git!();
    let fx = Fx::new();
    let w = fx.service();
    let plan = w.plan(&project(), ticket("SHOP-141")).await.unwrap();
    let item = w.start(plan).await.unwrap();
    let sid = fx.spawned_of(|k| *k == SessionKind::Claude)[0].id.clone();
    // Reboot wipes the runtime dir.
    std::fs::remove_dir_all(fx.dirs.runtime.join("s")).unwrap();
    let req = w.claude_restore_request(&sid, false, None).await.unwrap().unwrap();
    assert_eq!(req.args[..2], ["--resume".to_owned(), item.claude_uuid.clone().unwrap()]);
    assert_eq!(req.kind, SessionKind::Claude);
    assert_eq!(req.cwd.as_ref(), Some(&item.worktree));
    let settings = req.args.windows(2).find(|a| a[0] == "--settings").map(|a| a[1].clone()).unwrap();
    assert!(std::path::Path::new(&settings).exists(), "files regenerated");
    let req = w.claude_restore_request(&sid, true, None).await.unwrap().unwrap();
    assert_eq!(req.args[0], "--continue");
    assert!(
        w.claude_restore_request(&kelta_proto::ids::SessionId::new("nope"), false, None)
            .await
            .unwrap()
            .is_none()
    );
}

#[tokio::test]
async fn concurrent_starts_for_one_ticket_make_one_item() {
    need_git!();
    let fx = Fx::new();
    let w = fx.service();
    let plan = w.plan(&project(), ticket("SHOP-141")).await.unwrap();
    let (a, b) = tokio::join!(w.start(plan.clone()), w.start(plan));
    let ids: Vec<_> = [a, b].into_iter().filter_map(Result::ok).map(|i| i.id).collect();
    assert!(!ids.is_empty());
    assert!(ids.iter().all(|i| *i == ids[0]));
    assert_eq!(fx.store_items().await.len(), 1);
}

#[tokio::test]
async fn a_stale_remote_branch_of_the_same_name_does_not_seed_a_new_item() {
    need_git!();
    let fx = Fx::new();
    // An earlier item's branch was merged and deleted locally; origin kept it.
    let b = "feat/SHOP-141-add-login-form";
    git(&fx.repo, &["checkout", "-q", "-b", b]);
    git(&fx.repo, &["commit", "-q", "--allow-empty", "-m", "old merged work"]);
    git(&fx.repo, &["push", "-q", "origin", b]);
    git(&fx.repo, &["checkout", "-q", "main"]);
    git(&fx.repo, &["branch", "-q", "-D", b]);
    let w = fx.service();
    let plan = w.plan(&project(), ticket("SHOP-141")).await.unwrap();
    let item = w.start(plan).await.unwrap();
    assert_eq!(item.branch, b);
    assert_eq!(git(&item.worktree, &["rev-parse", "HEAD"]), git(&fx.repo, &["rev-parse", "origin/main"]));
}

#[tokio::test]
async fn concurrent_scratch_starts_for_one_task_make_one_item() {
    need_git!();
    let fx = Fx::new();
    let w = fx.service();
    let plan = w.plan(&project(), scratch("Explore caching")).await.unwrap();
    let (a, b) = tokio::join!(w.start(plan.clone()), w.start(plan));
    let errs: Vec<_> = [&a, &b].into_iter().filter_map(|r| r.as_ref().err()).collect();
    assert_eq!(errs.len(), 1, "{a:?} {b:?}");
    assert_eq!(errs[0].code, ErrorCode::Conflict);
    assert_eq!(fx.store_items().await.len(), 1);
}

fn scratch(task: &str) -> WorkSource {
    WorkSource::Branch { name: String::new(), task: Some(task.into()), repo: None }
}

#[tokio::test]
async fn scratch_item_from_a_task() {
    need_git!();
    let fx = Fx::new();
    let w = fx.service();
    let task = "Fix the login flake\nIt fails on CI about once a day.";
    let plan = w.plan(&project(), scratch(task)).await.unwrap();
    assert_eq!(plan.branch, "wip/fix-the-login-flake");
    assert_eq!(plan.worktree_path, fx.wt_root.join("shop/api/wip-fix-the-login-flake"));
    assert_eq!(plan.claude.prompt, task, "standalone defaults to {{task}}");
    assert!(!plan.side_effects.assign_me && plan.side_effects.transition_to.is_none());

    let item = w.start(plan).await.unwrap();
    assert_eq!(item.state, WorkState::Active, "{:?}", item.steps);
    assert_eq!(item.kind, kelta_proto::model::WorkKind::Branch);
    assert_eq!(item.title.as_deref(), Some("Fix the login flake"));
    assert!(fx.tracker.calls().is_empty(), "no tracker side effects");
    assert_eq!(fx.core.opened()[0].1.tab_title.as_deref(), Some("wip Fix the login flake"));
    let claude = fx
        .core
        .calls()
        .into_iter()
        .find(|c| c.method == "session_spawn" && c.args["kind"]["type"] == "claude");
    let argv: Vec<String> = serde_json::from_value(claude.unwrap().args["args"].clone()).unwrap();
    assert_eq!(argv.last().map(String::as_str), Some(task));

    // Same first line again: refused with a reason, never adopted.
    let e = w.plan(&project(), scratch(task)).await.unwrap_err();
    assert_eq!(e.code, ErrorCode::Conflict);
    assert!(e.message.contains("wip/fix-the-login-flake"), "{}", e.message);
    // An existing branch without an item is refused too.
    git(&fx.repo, &["branch", "wip/taken"]);
    let e = w.plan(&project(), scratch("Taken")).await.unwrap_err();
    assert!(e.message.contains("already exists"), "{}", e.message);
    // An edited branch name wins over the task; nothing to slug is invalid.
    let named = WorkSource::Branch { name: "spike/x".into(), task: Some("Try x".into()), repo: None };
    assert_eq!(w.plan(&project(), named).await.unwrap().branch, "spike/x");
    let e = w.plan(&project(), scratch("  \n ")).await.unwrap_err();
    assert_eq!(e.code, ErrorCode::InvalidArgument);
    let other = WorkSource::Branch { name: String::new(), task: Some("x".into()), repo: Some("nope".into()) };
    assert_eq!(w.plan(&project(), other).await.unwrap_err().code, ErrorCode::NotFound);
}

#[tokio::test]
async fn link_scratch_item_to_a_ticket() {
    need_git!();
    use kelta_proto::api::{CodeHost, WorkStore};
    let fx = Fx::new();
    let w = fx.service();
    let WorkSource::Ticket { ticket: t } = ticket("SHOP-141") else { unreachable!() };

    // Without side effects: only the ticket is read; the branch stays.
    let a = w.start(w.plan(&project(), scratch("Explore caching")).await.unwrap()).await.unwrap();
    let linked = w.link(&a.id, t.clone(), false).await.unwrap();
    assert_eq!(linked.kind, kelta_proto::model::WorkKind::Ticket);
    assert_eq!(linked.ticket.as_ref().map(|x| x.key.as_str()), Some("SHOP-141"));
    assert_eq!(linked.branch, "wip/explore-caching");
    assert!(!linked.pr_title_needs_key);
    assert_eq!(fx.tracker.calls(), vec!["get:SHOP-141".to_owned()]);
    let e = w.link(&a.id, t.clone(), false).await.unwrap_err();
    assert_eq!(e.code, ErrorCode::Conflict, "already linked");
    // The next resume's CONTEXT.md points at ticket.md: it must exist in the item's run dir.
    let spawn = fx
        .core
        .calls()
        .into_iter()
        .find(|c| c.method == "session_spawn" && c.args["kind"]["type"] == "claude");
    let argv: Vec<String> = serde_json::from_value(spawn.unwrap().args["args"].clone()).unwrap();
    let settings = argv.windows(2).find(|w| w[0] == "--settings").map(|w| w[1].clone()).unwrap();
    let md = std::fs::read_to_string(std::path::Path::new(&settings).with_file_name("ticket.md")).unwrap();
    assert!(md.starts_with("# SHOP-141"), "{md}");

    // With a PR and side effects: on_start (assign + In Progress) then on_pr (In Review).
    let b = w.start(w.plan(&project(), scratch("Speed up search")).await.unwrap()).await.unwrap();
    // `a` still holds SHOP-141: a second open item for it is refused.
    let e = w.link(&b.id, t.clone(), false).await.unwrap_err();
    assert!(e.code == ErrorCode::Conflict && e.message.contains("SHOP-141"), "{}", e.message);
    let mut done = fx.store.get_item(&a.id).await.unwrap().unwrap();
    done.state = WorkState::Finished;
    fx.store.put_item(&done).await.unwrap();
    let binding = samples::project_info().repos[0].code_host.clone().unwrap();
    let pr = fx
        .host
        .create(&kelta_proto::codehost::PrCreate {
            repo: binding.repo.clone(),
            head: b.branch.clone(),
            base: "main".into(),
            title: "Speed up search".into(),
            body: String::new(),
            draft: false,
        })
        .await
        .unwrap();
    let mut with_pr = fx.store.get_item(&b.id).await.unwrap().unwrap();
    with_pr.pr_url = Some(pr.url.clone());
    fx.store.put_item(&with_pr).await.unwrap();
    let linked = w.link(&b.id, t, true).await.unwrap();
    assert!(linked.pr_title_needs_key);
    assert_eq!(linked.branch, "wip/speed-up-search");
    let calls = fx.tracker.calls();
    assert!(calls.contains(&"assign:SHOP-141".to_owned()), "{calls:?}");
    assert_eq!(calls.iter().filter(|c| c.starts_with("transition:")).count(), 2, "{calls:?}");
    assert_eq!(fx.tracker.ticket("SHOP-141").unwrap().ticket.status.name, "In Review");
}

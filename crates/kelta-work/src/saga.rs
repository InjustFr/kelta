//! Start-work saga (SPEC §3.1 step 2): journaled, idempotent steps with Retry/Skip.
//!
//! Every step is recorded in `WorkStore` (`work_steps`) before and after it runs; the plan and the
//! side effects already performed live in a small journal file `<data>/work/<id>.json` (0600). A
//! resumed saga skips `done`/`skipped` steps and every step re-checks the world before acting
//! (existing worktree, sessions of this work item, tracker effects already applied), so a crash after
//! any step never duplicates worktrees, sessions or tracker calls.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use kelta_proto::api::{CoreApi, Tracker};
use kelta_proto::codehost::{Review, ReviewKind};
use kelta_proto::error::{ErrorCode, KeltaError};
use kelta_proto::events::{BusEvent, Toast, ToastAction, ToastLevel, bus};
use kelta_proto::ext::{BlockingOutcome, ToolHandle};
use kelta_proto::ids::{ProjectId, SessionId, ToolId, WorkItemId};
use kelta_proto::model::{
    BranchChoice, CloseOnExit, Lifecycle, OpenPaneRequest, PORT_BLOCK, PaneContent, Placement, ProjectInfo,
    RepoInfo, RestorePolicy, SessionInfo, SessionKind, SpawnRequest, StartWorkPlan, StepStatus, TemplateCtx,
    WORK_STEPS, WorkItem, WorkKind, WorkSource, WorkState,
};
use kelta_proto::settings::{EditorOpenMode, EditorRestore, SessionTemplate, Settings, TransitionTarget};
use kelta_proto::tracker::{Assignee, Status, TicketRef, Transition};
use serde::{Deserialize, Serialize};
use tokio::sync::broadcast;

use crate::claude::{self, ContextInfo, LaunchMode, LaunchSpec};
use crate::layout::{self, SlotKind};
use crate::plan::{self, TicketSnap};
use crate::template::{Ctx, Mode, render, render_shell, shell_quote, shell_words, slugify};
use crate::{WorkService, editor, files, git};

/// Default PTY size for sessions spawned before a view attaches (resized on attach).
pub(crate) const COLS: u16 = 120;
pub(crate) const ROWS: u16 = 40;

/// Saga context persisted next to the work item.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub(crate) struct Journal {
    pub plan: Option<StartWorkPlan>,
    pub ticket: Option<TicketSnap>,
    pub review: Option<Review>,
    pub claude_run: Option<PathBuf>,
    pub editor_run: Option<PathBuf>,
    pub editor_session_file: Option<PathBuf>,
    /// Kelta session id we asked core to use for Claude (mcp/hook URLs).
    pub claude_sid_hint: Option<SessionId>,
    pub claude_session: Option<SessionId>,
    /// Template leaf index → session (editor / shell leaves).
    pub sessions: BTreeMap<usize, SessionId>,
    pub setup_session: Option<SessionId>,
    pub progress_pane: bool,
    pub arranged: bool,
    pub worktree_created: bool,
    pub http_port: Option<u16>,
    /// Worktree-relative files `include_files` copied (finish may delete only these).
    #[serde(default)]
    pub include_copies: Vec<String>,
    /// Tracker side effects already attempted (`assign`, `transition`, `comment`).
    pub effects_done: BTreeSet<String>,
}

impl Journal {
    pub fn template_id(&self) -> String {
        self.plan.as_ref().map(|p| p.template_id.clone()).unwrap_or_else(|| "claude+editor".to_owned())
    }
}

/// Resolved project/repo/settings for one work item.
pub(crate) struct Env {
    pub core: Arc<dyn CoreApi>,
    pub project: ProjectInfo,
    pub repo: RepoInfo,
    pub settings: Arc<Settings>,
}

fn project_of(core: &Arc<dyn CoreApi>, id: &ProjectId) -> Result<ProjectInfo, KeltaError> {
    core.project(id).ok_or_else(|| KeltaError::not_found(format!("project {id}")))
}

fn repo_of(project: &ProjectInfo, id: &str) -> Result<RepoInfo, KeltaError> {
    project
        .repos
        .iter()
        .find(|r| r.id == id)
        .cloned()
        .ok_or_else(|| KeltaError::not_found(format!("repo {id} in project {}", project.id)))
}

/// Await `session.exited` for `sid` (subscribe *before* the session can exit).
pub(crate) async fn await_exit(
    core: &Arc<dyn CoreApi>,
    rx: &mut broadcast::Receiver<BusEvent>,
    sid: &SessionId,
    timeout: Duration,
) -> Result<i32, KeltaError> {
    let exited = |core: &Arc<dyn CoreApi>| {
        core.session_get(sid).filter(|s| s.lifecycle == Lifecycle::Exited).map(|s| s.exit_code.unwrap_or(-1))
    };
    if let Some(code) = exited(core) {
        return Ok(code);
    }
    // one-shot: bounded wait for this session's exit (armed by the spawn that preceded it).
    let wait = async {
        loop {
            match rx.recv().await {
                Ok(ev) if ev.name == bus::SESSION_EXITED && ev.session_id.as_ref() == Some(sid) => {
                    let code = ev.payload.get("code").and_then(serde_json::Value::as_i64).map(|c| c as i32);
                    return Ok(code.or_else(|| exited(core)).unwrap_or(-1));
                }
                Ok(_) => {}
                Err(broadcast::error::RecvError::Lagged(_)) => {
                    if let Some(code) = exited(core) {
                        return Ok(code);
                    }
                }
                Err(broadcast::error::RecvError::Closed) => {
                    return Err(KeltaError::cancelled("event bus closed"));
                }
            }
        }
    };
    tokio::time::timeout(timeout, wait)
        .await
        .map_err(|_| KeltaError::timeout(format!("session {sid} did not exit in time")))?
}

impl WorkService {
    // ---- journal ---------------------------------------------------------------------------

    pub(crate) fn journal_path(&self, id: &WorkItemId) -> PathBuf {
        self.dirs.data.join("work").join(format!("{}.json", id.as_str()))
    }

    pub(crate) fn load_journal(&self, id: &WorkItemId) -> Journal {
        std::fs::read(self.journal_path(id))
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default()
    }

    pub(crate) fn save_journal(&self, id: &WorkItemId, j: &Journal) -> Result<(), KeltaError> {
        let path = self.journal_path(id);
        if let Some(dir) = path.parent() {
            files::private_dir(dir)?;
        }
        files::write_private(&path, &serde_json::to_vec_pretty(j)?)
    }

    pub(crate) fn env(&self, project: &ProjectId, repo_id: &str) -> Result<Env, KeltaError> {
        let core = self.api()?;
        let project = project_of(&core, project)?;
        let repo = repo_of(&project, repo_id)?;
        let settings = core.settings(Some(&project.id));
        Ok(Env { core, project, repo, settings })
    }

    /// Placeholder context for a work item (SETTINGS §6).
    pub(crate) fn item_ctx(&self, env: &Env, item: &WorkItem, j: &Journal) -> Ctx {
        let mut c = plan::base_ctx(&env.project, Some(&env.repo), &self.dirs);
        c.set("worktree", item.worktree.to_string_lossy().into_owned());
        c.set("branch", item.branch.clone());
        c.set("base", item.base.clone());
        if let Some(WorkSource::Branch { task: Some(task), .. }) = j.plan.as_ref().map(|p| &p.source) {
            c.set("task", task.clone());
        }
        // `editor.review_args`: own work diffs merge base to working tree, a review the PR's commits.
        let range = format!("{}/{}", env.repo.remote, item.base);
        c.set("range", if item.kind == WorkKind::Review { format!("{range}...HEAD") } else { range });
        if let Some(t) = &j.ticket {
            plan::add_ticket(&mut c, t);
            c.set("key", t.branch_key.clone());
            c.set("slug", slugify(&t.title, env.settings.worktree.slug_max as usize));
            c.set("type", plan::type_for(&env.settings, t.kind.as_deref()));
        } else if let Some(t) = &item.ticket {
            c.set("ticket.key", t.key.clone());
            c.set("key", t.key.clone());
        }
        if let Some(r) = &j.review {
            plan::add_review(&mut c, r);
        }
        if let Some(run) = &j.claude_run {
            c.set("run", run.to_string_lossy().into_owned());
            c.set("sid8", run.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default());
            if j.ticket.is_some() {
                c.set("ticket.file", run.join(claude::TICKET_FILE).to_string_lossy().into_owned());
            }
        }
        c
    }

    /// Tab title: `KEY title`, `PR #n title`, `wip title` (scratch) or the branch.
    pub(crate) fn tab_title(item: &WorkItem, j: &Journal) -> String {
        if let Some(t) = &j.ticket {
            return format!("{} {}", t.key, t.title);
        }
        if let Some(r) = &j.review {
            return format!("#{} {}", r.r#ref.number, r.title);
        }
        if let Some(t) = &item.ticket {
            return t.key.clone();
        }
        if let Some(r) = &item.review {
            return format!("#{}", r.number);
        }
        match &item.title {
            Some(t) => format!("wip {t}"),
            None => item.branch.clone(),
        }
    }

    // ---- plan ------------------------------------------------------------------------------

    pub(crate) async fn build_plan(
        &self,
        project_id: &ProjectId,
        source: WorkSource,
    ) -> Result<StartWorkPlan, KeltaError> {
        let core = self.api()?;
        let project = project_of(&core, project_id)?;
        if project.repos.is_empty() {
            return Err(KeltaError::invalid(format!("project {project_id} has no repository")));
        }
        let settings = core.settings(Some(project_id));
        let items = self.store.list_items(Some(project_id)).await?;
        let existing = plan::existing_for(&items, &source).cloned();
        let slug_max = settings.worktree.slug_max as usize;
        let rules = project.tracker.as_ref().map(|t| t.repo_rules.clone()).unwrap_or_default();

        match &source {
            WorkSource::Ticket { ticket } => {
                let tracker = core.tracker_for(&ticket.account).await?;
                let detail = tracker.get(ticket).await?;
                let snap =
                    TicketSnap::from_ticket(&detail.ticket, tracker.branch_key(ticket), tracker.kind());
                let remembered = existing.as_ref().map(|w| w.repo_id.clone()).or_else(|| {
                    items
                        .iter()
                        .filter(|w| w.ticket.as_ref().is_some_and(|t| t.key == ticket.key))
                        .map(|w| w.repo_id.clone())
                        .next()
                });
                let repo_id = plan::select_repo(&project, &rules, Some(&snap), remembered.as_deref())
                    .ok_or_else(|| KeltaError::invalid("no repository"))?;
                let repo = repo_of(&project, &repo_id)?;
                let mut ctx = plan::base_ctx(&project, Some(&repo), &self.dirs);
                plan::add_ticket(&mut ctx, &snap);
                ctx.set("key", snap.branch_key.clone());
                ctx.set("slug", slugify(&snap.title, slug_max));
                ctx.set("type", plan::type_for(&settings, snap.kind.as_deref()));
                let (branch, path, exists) = match &existing {
                    Some(w) => (w.branch.clone(), w.worktree.clone(), None),
                    None => {
                        let raw = render(&settings.worktree.branch_template, &ctx, Mode::Strict)?;
                        let branch = plan::valid_branch(&repo.path, &raw).await?;
                        ctx.set("branch", branch.clone());
                        let path = plan::worktree_path(&settings, &ctx)?;
                        let exists = plan::branch_exists(&repo.path, &branch).await?;
                        (branch, path, exists)
                    }
                };
                ctx.set("branch", branch.clone());
                ctx.set("worktree", path.to_string_lossy().into_owned());
                let claude = plan::claude_plan(&settings, "default", "ticket", &ctx);
                let mut side = plan::side_effects(&settings, &project, true, &ctx);
                side.assign_me &= tracker.caps().assign;
                Ok(plan::assemble(
                    &project,
                    source.clone(),
                    &repo,
                    existing.as_ref().map(|w| w.base.clone()).unwrap_or_else(|| repo.base.clone()),
                    branch,
                    exists,
                    path,
                    settings.work.default_template.clone(),
                    claude,
                    side,
                    existing.as_ref(),
                ))
            }
            WorkSource::Review { review } => {
                let host = core.code_host_for(&review.account).await?;
                let r = host.get(review).await?.review;
                // B2: my own PR is the work item on its head branch, never a `kelta/pr-N` checkout.
                let existing = existing.or_else(|| plan::owner_of_pr(&items, &project, &r).cloned());
                if existing.is_none() && r.kind == ReviewKind::Authored {
                    // Made outside Kelta: adopt its head branch as a scratch item so pushes update it.
                    let source = WorkSource::Branch { name: r.source_branch.clone(), task: None, repo: None };
                    let mut plan = Box::pin(self.build_plan(project_id, source)).await?;
                    plan.base = r.target_branch.clone();
                    plan.adopt_pr = Some(r.url.clone());
                    return Ok(plan);
                }
                let repo = project
                    .repos
                    .iter()
                    .find(|x| x.code_host.as_ref().is_some_and(|c| c.repo == review.repo))
                    .or_else(|| project.repos.iter().find(|x| x.primary))
                    .or_else(|| project.repos.first())
                    .cloned()
                    .ok_or_else(|| KeltaError::invalid("no repository"))?;
                let mut ctx = plan::base_ctx(&project, Some(&repo), &self.dirs);
                plan::add_review(&mut ctx, &r);
                ctx.set("key", "review");
                ctx.set("slug", review.number.to_string());
                ctx.set("type", "review");
                ctx.set("base", r.target_branch.clone());
                let (branch, path, exists) = match &existing {
                    Some(w) => (w.branch.clone(), w.worktree.clone(), None),
                    None => {
                        let branch = plan::review_branch(host.kind(), review.number);
                        ctx.set("branch", branch.clone());
                        let path = plan::worktree_path(&settings, &ctx)?;
                        let exists = plan::branch_exists(&repo.path, &branch).await?;
                        (branch, path, exists)
                    }
                };
                ctx.set("branch", branch.clone());
                ctx.set("worktree", path.to_string_lossy().into_owned());
                let claude = plan::claude_plan(&settings, "review", "review", &ctx);
                let mut side = plan::side_effects(&settings, &project, false, &ctx);
                side.run_setup = !settings.worktree.setup.is_empty();
                Ok(plan::assemble(
                    &project,
                    source.clone(),
                    &repo,
                    r.target_branch.clone(),
                    branch,
                    exists,
                    path,
                    settings.work.review_template.clone(),
                    claude,
                    side,
                    existing.as_ref(),
                ))
            }
            WorkSource::Branch { name, task, repo } => {
                let repo_id = match repo {
                    Some(r) => r.clone(),
                    None => {
                        plan::select_repo(&project, &[], None, existing.as_ref().map(|w| w.repo_id.as_str()))
                            .ok_or_else(|| KeltaError::invalid("no repository"))?
                    }
                };
                let repo = repo_of(&project, &repo_id)?;
                let mut ctx = plan::base_ctx(&project, Some(&repo), &self.dirs);
                ctx.set("task", task.clone().unwrap_or_default());
                let branch = if name.trim().is_empty() {
                    let title = task.as_deref().map(plan::task_title).unwrap_or_default();
                    let slug = slugify(&title, slug_max);
                    if slug.is_empty() {
                        return Err(KeltaError::invalid("describe the task or name the branch"));
                    }
                    ctx.set("slug", slug);
                    let raw = render(&settings.work.scratch_branch_template, &ctx, Mode::Strict)?;
                    ctx.set("slug", "");
                    plan::valid_branch(&repo.path, &raw).await?
                } else {
                    git::check_branch_name(&repo.path, name.trim()).await?
                };
                // The resolved branch (not the possibly empty name) identifies the item.
                let by_branch = WorkSource::Branch { name: branch.clone(), task: None, repo: None };
                let existing = plan::existing_for(&items, &by_branch).cloned();
                ctx.set("key", slugify(&branch, slug_max));
                ctx.set("type", "");
                ctx.set("branch", branch.clone());
                let (path, exists) = match &existing {
                    Some(w) => (w.worktree.clone(), None),
                    None => (
                        plan::worktree_path(&settings, &ctx)?,
                        plan::branch_exists(&repo.path, &branch).await?,
                    ),
                };
                // New work item: never adopt an existing branch or item behind the user's back.
                if task.is_some() && (existing.is_some() || exists.is_some()) {
                    let what = if existing.is_some() { "has a work item" } else { "already exists" };
                    return Err(KeltaError::conflict(format!(
                        "Branch {branch} {what}. Edit the branch name or the task's first line."
                    )));
                }
                ctx.set("worktree", path.to_string_lossy().into_owned());
                let claude = plan::claude_plan(&settings, "default", "standalone", &ctx);
                let side = plan::side_effects(&settings, &project, false, &ctx);
                Ok(plan::assemble(
                    &project,
                    source.clone(),
                    &repo,
                    repo.base.clone(),
                    branch,
                    exists,
                    path,
                    settings.work.default_template.clone(),
                    claude,
                    side,
                    existing.as_ref(),
                ))
            }
        }
    }

    // ---- start -----------------------------------------------------------------------------

    pub(crate) async fn create_item(&self, mut plan: StartWorkPlan) -> Result<WorkItemId, KeltaError> {
        let env = self.env(&plan.project_id, &plan.repo_id)?;
        if !env.repo.path.is_dir() {
            return Err(KeltaError::not_found(format!("repository {} not found", env.repo.path.display())));
        }
        let template = layout::template(&env.settings.session_templates, &plan.template_id)
            .ok_or_else(|| KeltaError::invalid(format!("unknown session template `{}`", plan.template_id)))?;
        let mut branch = git::check_branch_name(&env.repo.path, plan.branch.trim()).await?;
        let mut path = plan.worktree_path.clone();
        if !path.is_absolute() {
            return Err(KeltaError::invalid(format!("worktree path must be absolute: {}", path.display())));
        }
        if plan.branch_exists.as_ref().is_some_and(|b| b.choice == BranchChoice::Suffix)
            && git::local_branch_exists(&env.repo.path, &branch).await?
        {
            (branch, path) = plan::suffixed(&env.repo.path, &branch, &path).await?;
        }
        plan.branch = branch.clone();
        plan.worktree_path = path.clone();
        let has_claude =
            layout::slots(&template.layout).iter().any(|s| matches!(s.kind, SlotKind::Claude { .. }));
        let (kind, ticket, review, title) = match &plan.source {
            WorkSource::Ticket { ticket } => (WorkKind::Ticket, Some(ticket.clone()), None, None),
            WorkSource::Review { review } => (WorkKind::Review, None, Some(review.clone()), None),
            WorkSource::Branch { task, .. } => {
                (WorkKind::Branch, None, None, task.as_deref().map(plan::task_title))
            }
        };
        // Under `start_lock`: no two new items can pick the same free block.
        let taken: Vec<u16> = self.store.list_items(None).await?.iter().filter_map(|w| w.port_base).collect();
        let port_base = plan::alloc_ports(&env.settings.ports.range, &taken, plan::port_free)?;
        let mut item = WorkItem {
            id: WorkItemId::generate(),
            project_id: plan.project_id.clone(),
            kind,
            ticket,
            review,
            repo_id: plan.repo_id.clone(),
            worktree: path,
            branch,
            base: plan.base.clone(),
            claude_uuid: has_claude.then(|| uuid::Uuid::new_v4().to_string()),
            nvim_socket: None,
            session_ids: Vec::new(),
            tab_id: None,
            pr_url: plan.adopt_pr.clone(),
            state: WorkState::Planned,
            steps: Vec::new(),
            created_at: kelta_proto::now_rfc3339(),
            sent_threads: Vec::new(),
            rebase: None,
            title: title.filter(|t| !t.is_empty()),
            pr_title_needs_key: false,
            review_due: false,
            claude_replied: false,
            claude_at: None,
            claude_message: None,
            delta: None,
            next_note: None,
            left_at: None,
            port_base,
            cost_usd: 0.0,
            auto_finish: false,
        };
        let journal = Journal { plan: Some(plan), ..Journal::default() };
        self.save_journal(&item.id, &journal)?;
        for step in WORK_STEPS {
            self.store.set_step(&item.id, step, StepStatus::Pending, None).await?;
        }
        item.steps = self.merged_steps(&item.id, &[]).await?;
        self.save(&mut item).await?;
        Ok(item.id)
    }

    pub(crate) async fn run_saga_locked(&self, id: &WorkItemId) -> Result<WorkItem, KeltaError> {
        let mut item = self.load(id).await?;
        let mut j = self.load_journal(id);
        if j.plan.is_none() {
            return Err(KeltaError::invalid(format!("work item {id} has no start journal")));
        }
        let env = self.env(&item.project_id, &item.repo_id)?;
        item.state = WorkState::Starting;
        self.save(&mut item).await?;
        for step in WORK_STEPS {
            let status =
                item.steps.iter().find(|s| s.step == *step).map(|s| s.status).unwrap_or(StepStatus::Pending);
            if matches!(status, StepStatus::Done | StepStatus::Skipped) {
                continue;
            }
            self.set_step(&mut item, step, StepStatus::Running, None).await?;
            let res = self.exec_step(step, &env, &mut item, &mut j).await;
            self.save_journal(id, &j)?;
            match res {
                Ok(detail) => {
                    self.set_step(&mut item, step, StepStatus::Done, detail).await?;
                    let crash = self.crash_after.lock().as_deref() == Some(step);
                    if crash {
                        *self.crash_after.lock() = None;
                        return Err(KeltaError::internal(format!("simulated crash after {step}")));
                    }
                }
                Err(e) => {
                    tracing::warn!(work_item = %id, step, error = %e.message, "start work step failed");
                    self.set_step(&mut item, step, StepStatus::Failed, Some(e.message.clone())).await?;
                    item.state = WorkState::Failed { step: (*step).to_owned(), message: e.message.clone() };
                    self.save(&mut item).await?;
                    env.core.toast(Toast {
                        level: ToastLevel::Error,
                        text: format!("Start work failed at {step}: {}", e.message),
                        action: Some(ToastAction {
                            label: "Retry".into(),
                            command: "work.retry_step".into(),
                            args: Some(serde_json::json!({ "id": id, "step": step })),
                        }),
                    });
                    return Ok(item);
                }
            }
        }
        Ok(item)
    }

    pub(crate) async fn retry(&self, id: &WorkItemId, step: &str) -> Result<WorkItem, KeltaError> {
        let lock = self.item_lock(id);
        let _guard = lock.try_lock().map_err(|_| self.busy(id))?;
        let (name, skip) = match step.strip_prefix("skip:") {
            Some(s) => (s, true),
            None => (step, false),
        };
        if !WORK_STEPS.contains(&name) {
            return Err(KeltaError::invalid(format!("unknown step `{name}`")));
        }
        let mut item = self.load(id).await?;
        if item.state == WorkState::Finished {
            return Err(KeltaError::conflict("work item is finished"));
        }
        if skip {
            self.set_step(&mut item, name, StepStatus::Skipped, Some("skipped by user".into())).await?;
        } else {
            let mut j = self.load_journal(id);
            match name {
                "setup" => j.setup_session = None,
                "tracker_side_effects" => j.effects_done.clear(),
                "claude" => j.arranged = false,
                _ => {}
            }
            self.save_journal(id, &j)?;
            self.set_step(&mut item, name, StepStatus::Pending, None).await?;
        }
        if matches!(
            item.state,
            WorkState::Active | WorkState::PrOpen | WorkState::Merged { .. } | WorkState::PrClosed
        ) {
            // Re-running a finished saga step keeps the item's state afterwards.
            let state = item.state.clone();
            let mut out = self.run_saga_locked(id).await?;
            if !matches!(out.state, WorkState::Failed { .. }) {
                out.state = state;
                self.save(&mut out).await?;
            }
            return Ok(out);
        }
        self.run_saga_locked(id).await
    }

    // ---- steps -----------------------------------------------------------------------------

    async fn exec_step(
        &self,
        step: &str,
        env: &Env,
        item: &mut WorkItem,
        j: &mut Journal,
    ) -> Result<Option<String>, KeltaError> {
        match step {
            "before_start" => self.step_before_start(env, item, j).await,
            "fetch_ticket" => self.step_fetch_ticket(env, item, j).await,
            "fetch_base" => self.step_fetch_base(env, item, j).await,
            "worktree" => self.step_worktree(env, item, j).await,
            "include_files" => self.step_include(env, item, j).await,
            "claude_files" => self.step_claude_files(env, item, j).await,
            "layout" => self.step_layout(env, item, j).await,
            "setup" => self.step_setup(env, item, j).await,
            "editor" => self.step_editor(env, item, j).await,
            "claude" => self.step_claude(env, item, j).await,
            "tracker_side_effects" => self.step_effects(env, item, j).await,
            "persist" => self.step_persist(env, item, j).await,
            other => Err(KeltaError::invalid(format!("unknown step {other}"))),
        }
    }

    /// Run a blocking pre-event through the trigger engine (or just publish it when not wired).
    pub(crate) async fn blocking(
        &self,
        core: &Arc<dyn CoreApi>,
        ev: BusEvent,
    ) -> Result<Option<serde_json::Value>, KeltaError> {
        match self.host() {
            Some(h) => match h.run_blocking(&ev).await? {
                BlockingOutcome::Proceed { patch } => Ok(patch),
                BlockingOutcome::Veto { trigger_id, reason } => {
                    Err(KeltaError::cancelled(format!("vetoed by trigger {trigger_id}: {reason}")))
                }
            },
            None => {
                core.publish(ev);
                Ok(None)
            }
        }
    }

    async fn step_before_start(
        &self,
        env: &Env,
        item: &mut WorkItem,
        j: &mut Journal,
    ) -> Result<Option<String>, KeltaError> {
        let Some(plan) = j.plan.clone() else { return Ok(None) };
        let ev = BusEvent::new(bus::TICKET_BEFORE_START, serde_json::json!({ "plan": plan }))
            .with_project(item.project_id.clone())
            .with_work_item(item.id.clone());
        let Some(patch) = self.blocking(&env.core, ev).await? else { return Ok(None) };
        let mut applied = Vec::new();
        let mut plan = plan;
        if let Some(b) = patch.get("branch").and_then(|v| v.as_str()) {
            let b = git::check_branch_name(&env.repo.path, b).await?;
            plan.branch = b.clone();
            item.branch = b;
            applied.push("branch");
        }
        if let Some(t) = patch.get("template_id").and_then(|v| v.as_str()) {
            layout::template(&env.settings.session_templates, t)
                .ok_or_else(|| KeltaError::invalid(format!("unknown session template `{t}`")))?;
            plan.template_id = t.to_owned();
            applied.push("template_id");
        }
        if let Some(p) = patch.get("claude").and_then(|c| c.get("prompt")).and_then(|v| v.as_str()) {
            plan.claude.prompt = p.to_owned();
            applied.push("claude.prompt");
        }
        j.plan = Some(plan);
        self.save(item).await?;
        Ok((!applied.is_empty()).then(|| format!("patched by trigger: {}", applied.join(", "))))
    }

    pub(crate) fn ensure_claude_run(&self, item: &WorkItem, j: &mut Journal) -> Result<PathBuf, KeltaError> {
        if let Some(r) = j.claude_run.as_ref().filter(|r| r.is_dir()) {
            return Ok(r.clone());
        }
        let run = match &j.claude_run {
            // Runtime dir wiped (reboot): recreate the same path.
            Some(r) => {
                files::private_dir(r)?;
                r.clone()
            }
            None => files::alloc_run_dir(&self.dirs, item.claude_uuid.as_deref())?,
        };
        j.claude_run = Some(run.clone());
        Ok(run)
    }

    async fn step_fetch_ticket(
        &self,
        env: &Env,
        item: &mut WorkItem,
        j: &mut Journal,
    ) -> Result<Option<String>, KeltaError> {
        match (&item.ticket, &item.review) {
            (Some(t), _) => {
                let tracker = env.core.tracker_for(&t.account).await?;
                let d = tracker.get(t).await?;
                let run = self.ensure_claude_run(item, j)?;
                files::write_private(&run.join(claude::TICKET_FILE), files::ticket_markdown(&d).as_bytes())?;
                j.ticket = Some(TicketSnap::from_ticket(&d.ticket, tracker.branch_key(t), tracker.kind()));
                Ok(Some(format!("{} — {}", d.ticket.r#ref.key, d.ticket.title)))
            }
            (None, Some(r)) => {
                let host = env.core.code_host_for(&r.account).await?;
                let d = host.get(r).await?;
                self.ensure_claude_run(item, j)?;
                let detail = format!("#{} {}", d.review.r#ref.number, d.review.title);
                j.review = Some(d.review);
                Ok(Some(detail))
            }
            (None, None) => Ok(Some("no ticket".into())),
        }
    }

    async fn step_fetch_base(
        &self,
        env: &Env,
        item: &mut WorkItem,
        _j: &mut Journal,
    ) -> Result<Option<String>, KeltaError> {
        let timeout = Duration::from_secs(u64::from(env.settings.worktree.fetch_timeout_secs.max(1)));
        let remote = env.repo.remote.as_str();
        let repo = env.repo.path.as_path();
        if let Some(r) = &item.review {
            let host = env.core.code_host_for(&r.account).await?;
            let in_use = git::worktree_for_branch(repo, &item.branch).await?.is_some();
            if !in_use {
                let spec = format!("+{}", host.fetch_refspec(r, &item.branch));
                if let Err(e) = git::fetch(repo, remote, &[&spec, &item.base], timeout).await {
                    if !git::local_branch_exists(repo, &item.branch).await? {
                        return Err(e);
                    }
                    return Ok(Some(format!("offline, using local {}: {}", item.branch, e.message)));
                }
                return Ok(Some(format!("fetched {spec}")));
            }
        }
        // An adopted PR's head branch comes from the remote.
        let refs: Vec<&str> =
            if item.pr_url.is_some() { vec![&item.base, &item.branch] } else { vec![&item.base] };
        match git::fetch(repo, remote, &refs, timeout).await {
            Ok(()) => Ok(Some(format!("fetched {remote}/{}", item.base))),
            Err(e) if matches!(e.code, ErrorCode::Network | ErrorCode::Timeout) => {
                Ok(Some(format!("offline: {}", e.message)))
            }
            Err(e) => Err(e),
        }
    }

    async fn step_worktree(
        &self,
        env: &Env,
        item: &mut WorkItem,
        j: &mut Journal,
    ) -> Result<Option<String>, KeltaError> {
        let repo = env.repo.path.as_path();
        let detail;
        if !item.worktree.exists() {
            // Deleted outside Kelta (Recreate): drop the stale registration first.
            git::worktree_prune(repo).await?;
        }
        if let Some(w) = git::worktree_at(repo, &item.worktree).await? {
            if w.branch.as_deref() != Some(item.branch.as_str()) {
                return Err(KeltaError::conflict(format!(
                    "{} is a worktree of branch {}",
                    item.worktree.display(),
                    w.branch.unwrap_or_else(|| "(detached)".into())
                )));
            }
            detail = format!("reused {}", item.worktree.display());
        } else if let Some(w) = git::worktree_for_branch(repo, &item.branch).await? {
            if git::same_path(&w.path, repo) {
                return Err(KeltaError::conflict(format!(
                    "branch {} is checked out in the main checkout; switch it first",
                    item.branch
                )));
            }
            item.worktree = w.path.clone();
            detail = format!("reused existing worktree {}", w.path.display());
        } else {
            if item.worktree.exists()
                && std::fs::read_dir(&item.worktree).map(|mut d| d.next().is_some()).unwrap_or(true)
            {
                return Err(KeltaError::conflict(format!(
                    "{} already exists and is not a worktree",
                    item.worktree.display()
                )));
            }
            if git::local_branch_exists(repo, &item.branch).await? {
                git::worktree_add(repo, &item.worktree, &item.branch, None).await?;
            } else {
                let remote_base = format!("{}/{}", env.repo.remote, item.base);
                let remote_branch = format!("{}/{}", env.repo.remote, item.branch);
                // Only an adopted PR or a review starts from its remote branch: a stale same-named
                // branch (merged and not deleted, a teammate's) must not seed a new item.
                let from_remote = item.pr_url.is_some() || item.review.is_some();
                let start = if from_remote
                    && git::ref_exists(repo, &format!("refs/remotes/{remote_branch}")).await?
                {
                    remote_branch
                } else if git::ref_exists(repo, &remote_base).await? {
                    remote_base
                } else if git::ref_exists(repo, &item.base).await? {
                    item.base.clone()
                } else {
                    return Err(KeltaError::not_found(format!(
                        "base {} not found (fetch failed?)",
                        item.base
                    )));
                };
                git::worktree_add(repo, &item.worktree, &item.branch, Some(&start)).await?;
            }
            j.worktree_created = true;
            detail = format!("created {}", item.worktree.display());
        }
        git::ensure_excluded(repo, ".kelta/").await?;
        self.save(item).await?;
        Ok(Some(detail))
    }

    async fn step_include(
        &self,
        env: &Env,
        item: &mut WorkItem,
        j: &mut Journal,
    ) -> Result<Option<String>, KeltaError> {
        if git::same_path(&env.repo.path, &item.worktree) {
            return Ok(Some("nothing to copy".into()));
        }
        // Before the copies, so the main checkout's `.env` does not take the rendered file's place.
        let tmpl = env.settings.worktree.env_template.trim();
        let rendered = if tmpl.is_empty() {
            None
        } else {
            let mut ctx = Ctx::new();
            if let Some(base) = item.port_base {
                ctx.set("port", base.to_string());
                for i in 1..PORT_BLOCK {
                    ctx.set(&format!("port.{i}"), (base + i).to_string());
                }
            }
            let (detail, kept) = files::render_env_template(&env.repo.path, &item.worktree, tmpl, &ctx)?;
            if kept {
                env.core.toast(Toast::warn(format!("{}: {detail}", item.branch)));
            }
            Some(detail)
        };
        let patterns = files::include_patterns(&env.repo.path, &env.settings.worktree.include);
        if patterns.is_empty() {
            return Ok(Some(rendered.unwrap_or_else(|| "nothing to copy".into())));
        }
        let candidates = git::untracked_candidates(&env.repo.path).await?;
        let (repo, wt) = (env.repo.path.clone(), item.worktree.clone());
        let copied =
            tokio::task::spawn_blocking(move || files::copy_includes(&repo, &wt, &candidates, &patterns))
                .await
                .map_err(|e| KeltaError::internal(e.to_string()))??;
        j.include_copies.extend(copied.iter().cloned());
        let copied = if copied.is_empty() {
            "nothing to copy".into()
        } else {
            format!("copied {}", copied.join(", "))
        };
        Ok(Some(match rendered {
            Some(r) => format!("{r}; {copied}"),
            None => copied,
        }))
    }

    pub(crate) fn template_of(&self, env: &Env, j: &Journal) -> SessionTemplate {
        layout::template(&env.settings.session_templates, &j.template_id())
            .or_else(|| {
                layout::template(&env.settings.session_templates, &env.settings.work.default_template)
            })
            .unwrap_or_default()
    }

    /// Port of the lazy HTTP server when MCP or HTTP hooks need it (`None` = not wired / failed).
    pub(crate) async fn http_port(&self, settings: &Settings) -> Option<u16> {
        let needs = settings.claude.mcp
            || settings.claude.hook_transport == kelta_proto::settings::HookTransport::Http;
        if !needs {
            return None;
        }
        let host = self.host()?;
        match host.ensure_http().await {
            Ok(p) => Some(p),
            Err(e) => {
                tracing::warn!(error = %e.message, "HTTP server unavailable; Claude starts without Kelta MCP");
                None
            }
        }
    }

    /// Give back the consumer `http_port` took (no-op when it returned `None`).
    pub(crate) fn release_port(&self, port: Option<u16>) {
        if port.is_some()
            && let Some(h) = self.host()
        {
            h.release_http();
        }
    }

    /// (Re)write `claude-settings.json`, `mcp.json`, `context.md` for `sid` / `port`.
    pub(crate) fn write_claude_files(
        &self,
        env: &Env,
        item: &WorkItem,
        j: &Journal,
        run: &Path,
        sid: &SessionId,
        port: Option<u16>,
    ) -> Result<(), KeltaError> {
        let cfg = &env.settings.claude;
        let settings = claude::settings_json(&self.dirs, cfg, port.map(|p| (p, sid.as_str())));
        let mcp = port.filter(|_| cfg.mcp).map(|p| claude::mcp_json(p, sid.as_str()));
        let info = ContextInfo {
            project: env.project.name.clone(),
            worktree: item.worktree.clone(),
            branch: item.branch.clone(),
            base: item.base.clone(),
            ticket: j.ticket.as_ref().map(|t| (t.key.clone(), t.title.clone(), t.url.clone())),
            ticket_file: j.ticket.as_ref().map(|_| run.join(claude::TICKET_FILE)),
            pr: j.review.as_ref().map(|r| (r.url.clone(), r.target_branch.clone())),
            mcp: mcp.is_some(),
            append: cfg.append_system_prompt.clone(),
        };
        claude::write_files(run, &settings, mcp.as_ref(), &claude::context_md(&info))
    }

    async fn step_claude_files(
        &self,
        env: &Env,
        item: &mut WorkItem,
        j: &mut Journal,
    ) -> Result<Option<String>, KeltaError> {
        let template = self.template_of(env, j);
        if !layout::slots(&template.layout).iter().any(|s| matches!(s.kind, SlotKind::Claude { .. })) {
            return Ok(Some("no Claude session in template".into()));
        }
        if item.claude_uuid.is_none() {
            item.claude_uuid = Some(uuid::Uuid::new_v4().to_string());
            self.save(item).await?;
        }
        let run = self.ensure_claude_run(item, j)?;
        let sid = j.claude_sid_hint.get_or_insert_with(SessionId::generate).clone();
        let port = self.http_port(&env.settings).await;
        // The Claude step takes its own consumer; this one only learned the port.
        self.release_port(port);
        j.http_port = port;
        self.write_claude_files(env, item, j, &run, &sid, port)?;
        Ok(Some(format!("{}", run.display())))
    }

    /// Open (or re-focus) a pane; returns nothing, records the tab.
    pub(crate) async fn open_pane(
        &self,
        env: &Env,
        item: &mut WorkItem,
        title: &str,
        content: PaneContent,
        placement: Placement,
        focus: bool,
    ) -> Result<(), KeltaError> {
        let req = OpenPaneRequest {
            content,
            placement,
            focus,
            tab_title: (placement == Placement::NewTab).then(|| title.to_owned()),
            work_item_id: Some(item.id.clone()),
        };
        let pane = env.core.layout_open(&env.project.id, req).await?;
        if item.tab_id.is_none() {
            item.tab_id = Some(pane.tab_id);
        }
        Ok(())
    }

    async fn step_layout(
        &self,
        env: &Env,
        item: &mut WorkItem,
        j: &mut Journal,
    ) -> Result<Option<String>, KeltaError> {
        let title = Self::tab_title(item, j);
        let content = PaneContent::WorkItem { id: item.id.clone() };
        if j.progress_pane {
            self.open_pane(env, item, &title, content, Placement::Focused, true).await?;
        } else {
            self.open_pane(env, item, &title, content, Placement::NewTab, true).await?;
            j.progress_pane = true;
        }
        self.save(item).await?;
        Ok(Some(title))
    }

    fn session_live(core: &Arc<dyn CoreApi>, sid: &SessionId) -> bool {
        core.session_get(sid).is_some_and(|s| s.lifecycle != Lifecycle::Exited)
    }

    async fn step_setup(
        &self,
        env: &Env,
        item: &mut WorkItem,
        j: &mut Journal,
    ) -> Result<Option<String>, KeltaError> {
        let cmds = &env.settings.worktree.setup;
        let run = j.plan.as_ref().is_none_or(|p| p.side_effects.run_setup);
        if cmds.is_empty() || !run {
            return Ok(Some("no setup".into()));
        }
        let blocking = env.settings.worktree.setup_blocking;
        let mut rx = env.core.subscribe();
        let sid = match j.setup_session.clone().filter(|s| env.core.session_get(s).is_some()) {
            Some(sid) => sid,
            None => {
                let info = self.spawn_script(env, item, "setup", cmds, CloseOnExit::Never).await?;
                j.setup_session = Some(info.id.clone());
                if !item.session_ids.contains(&info.id) {
                    item.session_ids.push(info.id.clone());
                }
                self.save_journal(&item.id, j)?;
                self.save(item).await?;
                let title = Self::tab_title(item, j);
                if j.progress_pane {
                    self.open_pane(
                        env,
                        item,
                        &title,
                        PaneContent::WorkItem { id: item.id.clone() },
                        Placement::Focused,
                        true,
                    )
                    .await?;
                }
                let placement = if j.progress_pane { Placement::SplitDown } else { Placement::NewTab };
                self.open_pane(
                    env,
                    item,
                    &title,
                    PaneContent::Terminal { session_id: info.id.clone() },
                    placement,
                    false,
                )
                .await?;
                info.id
            }
        };
        if !blocking {
            return Ok(Some("running (non-blocking)".into()));
        }
        let code = await_exit(&env.core, &mut rx, &sid, Duration::from_secs(6 * 3600)).await?;
        if code == 0 {
            Ok(Some("setup finished".into()))
        } else {
            Err(KeltaError::upstream(format!(
                "setup failed (exit {code}) — Retry, or Skip to continue anyway"
            )))
        }
    }

    /// The setup-pane runner: `cmds` as one `set -e` script in the item's worktree.
    pub(crate) async fn spawn_script(
        &self,
        env: &Env,
        item: &WorkItem,
        name: &str,
        cmds: &[String],
        close_on_exit: CloseOnExit,
    ) -> Result<SessionInfo, KeltaError> {
        let mut script = String::from("set -e\n");
        for c in cmds {
            let words = shell_words(c)?;
            if words.is_empty() {
                continue;
            }
            script.push_str(&words.iter().map(|w| shell_quote(w)).collect::<Vec<_>>().join(" "));
            script.push('\n');
        }
        env.core
            .session_spawn(SpawnRequest {
                id: None,
                project_id: item.project_id.clone(),
                kind: SessionKind::Setup,
                name: Some(name.into()),
                program: Some("/bin/sh".into()),
                args: vec!["-c".into(), script],
                cwd: Some(item.worktree.clone()),
                env: BTreeMap::new(),
                cols: COLS,
                rows: ROWS,
                work_item_id: Some(item.id.clone()),
                restore: RestorePolicy::None,
                close_on_exit,
                template_id: None,
            })
            .await
    }

    /// Spawn one editor/shell leaf.
    pub(crate) async fn spawn_leaf(
        &self,
        env: &Env,
        item: &mut WorkItem,
        j: &mut Journal,
        kind: &SlotKind,
        name: Option<&str>,
    ) -> Result<SessionId, KeltaError> {
        let ctx = self.item_ctx(env, item, j);
        let (pid, wid, cwd, tid) =
            (item.project_id.clone(), item.id.clone(), item.worktree.clone(), j.template_id());
        let base_req = |kind: SessionKind, name: String| SpawnRequest {
            id: None,
            project_id: pid.clone(),
            kind,
            name: Some(name),
            program: None,
            args: Vec::new(),
            cwd: Some(cwd.clone()),
            env: BTreeMap::new(),
            cols: COLS,
            rows: ROWS,
            work_item_id: Some(wid.clone()),
            restore: RestorePolicy::ShellInCwd,
            close_on_exit: CloseOnExit::Never,
            template_id: Some(tid.clone()),
        };
        match kind {
            SlotKind::Editor => {
                let preset = editor::preset(&env.settings.editor, None)
                    .cloned()
                    .ok_or_else(|| KeltaError::invalid("no editor preset enabled"))?;
                if preset.external {
                    let c =
                        editor::ctx(None, &item.worktree.to_string_lossy(), None, None, &item.worktree, "");
                    let mut argv = vec![preset.command.clone()];
                    argv.extend(editor::render_args(&preset.args, &c)?);
                    if let Err(e) = editor::run_detached(&argv, &item.worktree).await {
                        env.core
                            .toast(Toast::warn(format!("Could not launch {}: {}", preset.label, e.message)));
                    }
                    let info = env
                        .core
                        .session_spawn(base_req(SessionKind::Shell, name.unwrap_or("shell").into()))
                        .await?;
                    return Ok(info.id);
                }
                let run = match j.editor_run.clone() {
                    Some(r) => {
                        files::private_dir(&r)?;
                        r
                    }
                    None => {
                        let r = files::alloc_run_dir(&self.dirs, None)?;
                        j.editor_run = Some(r.clone());
                        r
                    }
                };
                let key = run.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                let sock = run.join("nvim.sock");
                let _ = std::fs::remove_file(&sock);
                let c = editor::ctx(Some(&sock), ".", None, None, &item.worktree, &key);
                let mut args = editor::render_args(&preset.args, &c)?;
                if item.kind == WorkKind::Review {
                    for a in &env.settings.editor.review_args {
                        args.push(render(a, &ctx, Mode::Lenient)?);
                    }
                }
                let restore = match preset.restore {
                    EditorRestore::Mksession => {
                        let f = self.dirs.data.join("sessions").join(format!("{key}.vim"));
                        j.editor_session_file = Some(f.clone());
                        RestorePolicy::Editor { session_file: Some(f) }
                    }
                    EditorRestore::None => RestorePolicy::Relaunch,
                };
                let mut req = base_req(
                    SessionKind::Editor { adapter: preset.id.clone() },
                    name.unwrap_or(&preset.id).into(),
                );
                req.program = Some(preset.command.clone());
                req.args = args;
                req.restore = restore;
                let info = env.core.session_spawn(req).await?;
                if preset.open == EditorOpenMode::Rpc {
                    item.nvim_socket = Some(sock);
                }
                Ok(info.id)
            }
            SlotKind::Shell { command } => {
                let info = env
                    .core
                    .session_spawn(base_req(SessionKind::Shell, name.unwrap_or("shell").into()))
                    .await?;
                if let Some(cmd) = command.as_ref().filter(|c| !c.trim().is_empty()) {
                    let line = render_shell(cmd, &ctx, Mode::Lenient)?;
                    env.core.session_write(&info.id, format!("{line}\r").as_bytes()).await?;
                }
                Ok(info.id)
            }
            other => Err(KeltaError::internal(format!("not a spawnable leaf: {other:?}"))),
        }
    }

    async fn step_editor(
        &self,
        env: &Env,
        item: &mut WorkItem,
        j: &mut Journal,
    ) -> Result<Option<String>, KeltaError> {
        let template = self.template_of(env, j);
        let mut spawned = Vec::new();
        for slot in layout::slots(&template.layout) {
            if !matches!(slot.kind, SlotKind::Editor | SlotKind::Shell { .. }) {
                continue;
            }
            if j.sessions.get(&slot.idx).is_some_and(|s| Self::session_live(&env.core, s)) {
                continue;
            }
            // Crash between spawn and journal: adopt a live session of this item with the same kind.
            let adopted = env
                .core
                .session_list(Some(&item.project_id))
                .into_iter()
                .filter(|s| s.work_item_id.as_ref() == Some(&item.id) && s.lifecycle != Lifecycle::Exited)
                .filter(|s| {
                    !j.sessions.values().any(|x| x == &s.id) && j.setup_session.as_ref() != Some(&s.id)
                })
                .find(|s| match slot.kind {
                    SlotKind::Editor => matches!(s.kind, SessionKind::Editor { .. }),
                    _ => matches!(s.kind, SessionKind::Shell),
                });
            let sid = match adopted {
                Some(s) => s.id,
                None => self.spawn_leaf(env, item, j, &slot.kind, slot.name.as_deref()).await?,
            };
            j.sessions.insert(slot.idx, sid.clone());
            if !item.session_ids.contains(&sid) {
                item.session_ids.push(sid.clone());
            }
            self.save_journal(&item.id, j)?;
            self.save(item).await?;
            spawned.push(sid);
        }
        Ok(Some(if spawned.is_empty() {
            "nothing to spawn".into()
        } else {
            format!("{} session(s)", spawned.len())
        }))
    }

    /// Resolve the Claude binary (process PATH) and warn once when older than `min_version`.
    pub(crate) async fn version_gate(&self, core: &Arc<dyn CoreApi>, settings: &Settings) {
        let Ok(bin) = which::which(&settings.claude.binary) else { return };
        if self.versions.lock().contains_key(&bin) {
            return;
        }
        let v = claude::probe_version(&bin).await;
        self.versions.lock().insert(bin, v.clone());
        if let Some(found) = &v
            && !claude::version_ok(found, &settings.claude.min_version)
        {
            core.toast(Toast::warn(format!(
                "Claude Code {found} is older than {} — status hooks may not work",
                settings.claude.min_version
            )));
        }
    }

    pub(crate) fn launch_spec(
        &self,
        env: &Env,
        item: &WorkItem,
        j: &Journal,
        run: &Path,
        mode: LaunchMode,
        port: Option<u16>,
    ) -> LaunchSpec {
        let plan = j.plan.as_ref();
        let profile_name = plan.map(|p| p.claude.profile.clone()).unwrap_or_else(|| "default".into());
        let profile = env.settings.claude.profiles.get(&profile_name).cloned().unwrap_or_default();
        LaunchSpec {
            mode,
            name: Self::tab_title(item, j),
            model: plan.map(|p| p.claude.model.clone()).unwrap_or(profile.model),
            effort: plan.map(|p| p.claude.effort).unwrap_or(profile.effort),
            permission_mode: plan.map(|p| p.claude.permission_mode).unwrap_or(profile.permission_mode),
            settings_file: run.join(claude::SETTINGS_FILE),
            mcp_file: port.filter(|_| env.settings.claude.mcp).map(|_| run.join(claude::MCP_FILE)),
            allowed_tools: env.settings.claude.allowed_tools.clone(),
            context_file: run.join(claude::CONTEXT_FILE),
            extra_args: env.settings.claude.extra_args.clone(),
        }
    }

    /// `SpawnRequest` for Claude in `item`'s worktree, spawned as session `sid` (the id the
    /// per-session files name).
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn claude_request(
        &self,
        env: &Env,
        item: &WorkItem,
        j: &Journal,
        run: &Path,
        mode: LaunchMode,
        port: Option<u16>,
        sid: &SessionId,
    ) -> SpawnRequest {
        let spec = self.launch_spec(env, item, j, run, mode, port);
        let uuid = item.claude_uuid.clone().unwrap_or_default();
        SpawnRequest {
            id: Some(sid.clone()),
            project_id: item.project_id.clone(),
            kind: SessionKind::Claude,
            name: Some(spec.name.clone()),
            program: Some(env.settings.claude.binary.clone()),
            args: claude::argv(&spec),
            cwd: Some(item.worktree.clone()),
            env: BTreeMap::new(),
            cols: COLS,
            rows: ROWS,
            work_item_id: Some(item.id.clone()),
            restore: RestorePolicy::ClaudeResume { uuid },
            close_on_exit: CloseOnExit::Never,
            template_id: Some(j.template_id()),
        }
    }

    pub(crate) async fn spawn_claude(
        &self,
        env: &Env,
        item: &mut WorkItem,
        j: &mut Journal,
        mode: LaunchMode,
    ) -> Result<SessionId, KeltaError> {
        self.version_gate(&env.core, &env.settings).await;
        let run = self.ensure_claude_run(item, j)?;
        // The files name the session id: spawn under the journaled id unless a session holds it.
        let hint = match j.claude_sid_hint.clone().filter(|s| env.core.session_get(s).is_none()) {
            Some(s) => s,
            None => j.claude_sid_hint.insert(SessionId::generate()).clone(),
        };
        let port = self.http_port(&env.settings).await;
        if let Err(e) = self.write_claude_files(env, item, j, &run, &hint, port) {
            self.release_port(port);
            return Err(e);
        }
        let req = self.claude_request(env, item, j, &run, mode, port, &hint);
        let info = match env.core.session_spawn(req).await {
            Ok(i) => i,
            Err(e) => {
                self.release_port(port);
                return Err(e);
            }
        };
        if port.is_some() {
            self.http_sessions.lock().insert(info.id.clone());
        }
        j.http_port = port;
        j.claude_session = Some(info.id.clone());
        if !item.session_ids.contains(&info.id) {
            item.session_ids.push(info.id.clone());
        }
        self.save_journal(&item.id, j)?;
        self.save(item).await?;
        Ok(info.id)
    }

    async fn step_claude(
        &self,
        env: &Env,
        item: &mut WorkItem,
        j: &mut Journal,
    ) -> Result<Option<String>, KeltaError> {
        let template = self.template_of(env, j);
        let slots = layout::slots(&template.layout);
        let mut detail = "no Claude session in template".to_owned();
        if slots.iter().any(|s| matches!(s.kind, SlotKind::Claude { .. })) {
            let live = j.claude_session.clone().filter(|s| Self::session_live(&env.core, s)).or_else(|| {
                env.core
                    .session_list(Some(&item.project_id))
                    .into_iter()
                    .find(|s| {
                        s.work_item_id.as_ref() == Some(&item.id)
                            && s.kind == SessionKind::Claude
                            && s.lifecycle != Lifecycle::Exited
                    })
                    .map(|s| s.id)
            });
            match live {
                Some(sid) => {
                    j.claude_session = Some(sid.clone());
                    if !item.session_ids.contains(&sid) {
                        item.session_ids.push(sid.clone());
                    }
                    detail = format!("reused {sid}");
                }
                None => {
                    let uuid =
                        item.claude_uuid.get_or_insert_with(|| uuid::Uuid::new_v4().to_string()).clone();
                    let ctx = self.item_ctx(env, item, j);
                    let prompt_t = j.plan.as_ref().map(|p| p.claude.prompt.clone()).unwrap_or_default();
                    let prompt = render(&prompt_t, &ctx, Mode::Lenient)?;
                    let sid = self.spawn_claude(env, item, j, LaunchMode::New { uuid, prompt }).await?;
                    detail = format!("started {sid}");
                }
            }
        }
        if !j.arranged {
            self.arrange(env, item, j, &template, j.progress_pane).await?;
            j.arranged = true;
        }
        self.save(item).await?;
        Ok(Some(detail))
    }

    /// Place the template's sessions into the work item's tab.
    pub(crate) async fn arrange(
        &self,
        env: &Env,
        item: &mut WorkItem,
        j: &mut Journal,
        template: &SessionTemplate,
        replace_progress: bool,
    ) -> Result<(), KeltaError> {
        let slots = layout::slots(&template.layout);
        let ops = layout::ops(&template.layout, Placement::NewTab);
        let title = Self::tab_title(item, j);
        let mut opened: BTreeMap<usize, PaneContent> = BTreeMap::new();
        let mut last: Option<PaneContent> = None;
        for op in ops {
            let Some(slot) = slots.get(op.leaf) else { continue };
            let placement = if last.is_none() {
                if replace_progress {
                    self.open_pane(
                        env,
                        item,
                        &title,
                        PaneContent::WorkItem { id: item.id.clone() },
                        Placement::Focused,
                        true,
                    )
                    .await?;
                    Placement::ReplaceFocused
                } else {
                    Placement::NewTab
                }
            } else {
                let anchor = op.anchor.and_then(|a| opened.get(&a).cloned()).or_else(|| last.clone());
                if let Some(a) = anchor {
                    self.open_pane(env, item, &title, a, Placement::Focused, true).await?;
                }
                op.placement
            };
            let content = match &slot.kind {
                SlotKind::Claude { .. } => {
                    j.claude_session.clone().map(|s| PaneContent::Terminal { session_id: s })
                }
                SlotKind::Editor | SlotKind::Shell { .. } => {
                    j.sessions.get(&slot.idx).cloned().map(|s| PaneContent::Terminal { session_id: s })
                }
                SlotKind::Setup => j.setup_session.clone().map(|s| PaneContent::Terminal { session_id: s }),
                SlotKind::Tool { id } => {
                    let ctx = TemplateCtx {
                        repo_id: Some(item.repo_id.clone()),
                        cwd: Some(item.worktree.clone()),
                        session_id: None,
                        work_item_id: Some(item.id.clone()),
                        ticket: item.ticket.clone(),
                        review: item.review.clone(),
                        extra: BTreeMap::new(),
                    };
                    match env.core.tool_open(&item.project_id, &ToolId::new(id.clone()), ctx, placement).await
                    {
                        Ok(ToolHandle::Pty { session_id }) => {
                            if !item.session_ids.contains(&session_id) {
                                item.session_ids.push(session_id.clone());
                            }
                            let c = PaneContent::Terminal { session_id };
                            opened.insert(op.leaf, c.clone());
                            last = Some(c);
                        }
                        Ok(ToolHandle::Web { instance_id, .. }) => {
                            let c = PaneContent::Web { tool_instance_id: instance_id };
                            opened.insert(op.leaf, c.clone());
                            last = Some(c);
                        }
                        // External tools have no pane; the template leaf just stays empty.
                        Ok(ToolHandle::External) => {}
                        Err(e) => env.core.toast(Toast::warn(format!("Tool {id}: {}", e.message))),
                    }
                    continue;
                }
            };
            let Some(content) = content else { continue };
            self.open_pane(env, item, &title, content.clone(), placement, true).await?;
            opened.insert(op.leaf, content.clone());
            last = Some(content);
        }
        if let Some(c) = &j.claude_session {
            self.open_pane(
                env,
                item,
                &title,
                PaneContent::Terminal { session_id: c.clone() },
                Placement::Focused,
                true,
            )
            .await?;
        }
        Ok(())
    }

    pub(crate) async fn step_effects(
        &self,
        env: &Env,
        item: &mut WorkItem,
        j: &mut Journal,
    ) -> Result<Option<String>, KeltaError> {
        let Some(t) = item.ticket.clone() else { return Ok(Some("no ticket".into())) };
        let Some(plan) = j.plan.clone() else { return Ok(None) };
        let fx = plan.side_effects;
        let tracker = match env.core.tracker_for(&t.account).await {
            Ok(tr) => tr,
            Err(e) => {
                env.core.toast(Toast::warn(format!("Tracker side effects skipped: {}", e.message)));
                return Ok(Some(format!("skipped: {}", e.message)));
            }
        };
        let ctx = self.item_ctx(env, item, j);
        let do_assign = fx.assign_me && !j.effects_done.contains("assign");
        let do_transition = fx.transition_to.is_some() && !j.effects_done.contains("transition");
        let comment = fx
            .comment
            .as_ref()
            .filter(|_| !j.effects_done.contains("comment"))
            .and_then(|c| render(c, &ctx, Mode::Lenient).ok())
            .filter(|c| !c.trim().is_empty());
        let core = env.core.clone();
        let (pid, wid) = (item.project_id.clone(), item.id.clone());
        let assign = async {
            if !do_assign {
                return None;
            }
            Some(tracker.assign(&t, Assignee::Me).await.map(|tk| {
                core.publish(
                    BusEvent::new(
                        bus::TICKET_ASSIGNED,
                        serde_json::json!({ "ticket": t, "assignee": tk.assignee }),
                    )
                    .with_project(pid.clone())
                    .with_work_item(wid.clone()),
                );
                "assigned to me".to_owned()
            }))
        };
        let transition = async {
            let target = fx.transition_to.as_ref().filter(|_| do_transition)?;
            Some(transition_ticket(&core, tracker.as_ref(), &t, target, &pid, &wid).await)
        };
        let comment_fut = async {
            let body = comment.as_ref()?;
            Some(tracker.comment(&t, body).await.map(|()| {
                core.publish(
                    BusEvent::new(
                        bus::TICKET_COMMENTED,
                        serde_json::json!({ "ticket": t, "markdown": body }),
                    )
                    .with_project(pid.clone())
                    .with_work_item(wid.clone()),
                );
                "commented".to_owned()
            }))
        };
        let (a, tr, c) = tokio::join!(assign, transition, comment_fut);
        let mut parts = Vec::new();
        for (name, res) in [("assign", a), ("transition", tr), ("comment", c)] {
            let Some(res) = res else { continue };
            j.effects_done.insert(name.to_owned());
            match res {
                Ok(msg) => parts.push(msg),
                Err(e) => {
                    parts.push(format!("{name} failed: {}", e.message));
                    let mut toast = Toast::warn(format!("{}: {name} failed — {}", t.key, e.message));
                    toast.action = Some(ToastAction {
                        label: "Open in browser".into(),
                        command: "open_external".into(),
                        args: Some(serde_json::json!({ "url": tracker.browser_url(&t) })),
                    });
                    env.core.toast(toast);
                }
            }
        }
        Ok(Some(if parts.is_empty() { "nothing to do".into() } else { parts.join("; ") }))
    }

    async fn step_persist(
        &self,
        env: &Env,
        item: &mut WorkItem,
        j: &mut Journal,
    ) -> Result<Option<String>, KeltaError> {
        item.state = if item.pr_url.is_some() { WorkState::PrOpen } else { WorkState::Active };
        self.save(item).await?;
        if let Some(t) = &item.ticket {
            env.core.publish(
                BusEvent::new(
                    bus::TICKET_STARTED,
                    serde_json::json!({ "ticket": t, "worktree": item.worktree, "branch": item.branch, "work_item_id": item.id }),
                )
                .with_project(item.project_id.clone())
                .with_work_item(item.id.clone()),
            );
        }
        if j.worktree_created {
            env.core.publish(
                BusEvent::new(
                    bus::WORKTREE_CREATED,
                    serde_json::json!({ "path": item.worktree, "branch": item.branch, "repo_id": item.repo_id }),
                )
                .with_project(item.project_id.clone())
                .with_work_item(item.id.clone()),
            );
        }
        Ok(None)
    }
}

/// Does `status` already satisfy `target`?
pub(crate) fn status_matches(status: &Status, target: &TransitionTarget) -> bool {
    match target {
        TransitionTarget::Category { category } => &status.category == category,
        TransitionTarget::Name { name } => status.name.eq_ignore_ascii_case(name),
    }
}

/// Resolve `target` through `transitions()` and apply it (skips when already there).
pub(crate) async fn transition_ticket(
    core: &Arc<dyn CoreApi>,
    tracker: &dyn Tracker,
    t: &TicketRef,
    target: &TransitionTarget,
    project: &ProjectId,
    work: &WorkItemId,
) -> Result<String, KeltaError> {
    apply_transition(core, tracker, t, target, project, work, false).await
}

/// [`transition_ticket`] that never guesses: `Conflict` unless exactly one transition fits and
/// it needs no fields (automatic Done move on merge, FLOW §4.6).
pub(crate) async fn transition_ticket_strict(
    core: &Arc<dyn CoreApi>,
    tracker: &dyn Tracker,
    t: &TicketRef,
    target: &TransitionTarget,
    project: &ProjectId,
    work: &WorkItemId,
) -> Result<String, KeltaError> {
    apply_transition(core, tracker, t, target, project, work, true).await
}

/// Transitions leading to `target`; strict = exactly one and it needs no fields, else `Conflict`.
pub(crate) fn pick_transition<'a>(
    transitions: &'a [Transition],
    target: &TransitionTarget,
    strict: bool,
) -> Result<&'a Transition, KeltaError> {
    let mut fits = transitions.iter().filter(|tr| match target {
        TransitionTarget::Category { category } => &tr.to.category == category,
        TransitionTarget::Name { name } => {
            tr.to.name.eq_ignore_ascii_case(name) || tr.name.eq_ignore_ascii_case(name)
        }
    });
    let first = fits.next();
    if strict && (fits.next().is_some() || first.is_some_and(|tr| tr.needs_fields)) {
        return Err(KeltaError::conflict("several statuses fit; choose one"));
    }
    first.ok_or_else(|| {
        let label = match target {
            TransitionTarget::Category { category } => serde_json::to_value(category)
                .ok()
                .and_then(|v| v.as_str().map(str::to_owned))
                .unwrap_or_default(),
            TransitionTarget::Name { name } => name.clone(),
        };
        KeltaError::not_found(format!("no transition to {label}"))
    })
}

async fn apply_transition(
    core: &Arc<dyn CoreApi>,
    tracker: &dyn Tracker,
    t: &TicketRef,
    target: &TransitionTarget,
    project: &ProjectId,
    work: &WorkItemId,
    strict: bool,
) -> Result<String, KeltaError> {
    let current = tracker.get(t).await.ok().map(|d| d.ticket.status);
    if let Some(cur) = &current
        && status_matches(cur, target)
    {
        return Ok(format!("already {}", cur.name));
    }
    let transitions = tracker.transitions(t).await?;
    let tr = pick_transition(&transitions, target, strict)?;
    let ticket = tracker.transition(t, &tr.id, None).await?;
    core.publish(
        BusEvent::new(
            bus::TICKET_TRANSITIONED,
            serde_json::json!({ "ticket": t, "from": current, "to": ticket.status }),
        )
        .with_project(project.clone())
        .with_work_item(work.clone()),
    );
    Ok(format!("moved to {}", ticket.status.name))
}

#[cfg(test)]
mod tests {
    use kelta_proto::samples::status;
    use kelta_proto::tracker::StatusCategory;

    use super::*;

    fn tr(id: &str, name: &str, category: StatusCategory, needs_fields: bool) -> Transition {
        Transition { id: id.into(), name: name.into(), to: status(id, name, category), needs_fields }
    }

    #[test]
    fn strict_pick_never_guesses_between_done_statuses() {
        let done = TransitionTarget::Category { category: StatusCategory::Done };
        let several =
            [tr("1", "Done", StatusCategory::Done, false), tr("2", "Won't Do", StatusCategory::Done, false)];
        assert_eq!(pick_transition(&several, &done, true).unwrap_err().code, ErrorCode::Conflict);
        assert_eq!(pick_transition(&several, &done, false).unwrap().id, "1", "interactive keeps first");
        let by_name = TransitionTarget::Name { name: "won't do".into() };
        assert_eq!(pick_transition(&several, &by_name, true).unwrap().id, "2");
        let fields = [tr("1", "Done", StatusCategory::Done, true)];
        assert_eq!(pick_transition(&fields, &done, true).unwrap_err().code, ErrorCode::Conflict);
        assert_eq!(pick_transition(&[], &done, true).unwrap_err().code, ErrorCode::NotFound);
    }
}

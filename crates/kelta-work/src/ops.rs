//! Work item operations after start: resume, create PR, finish, status; editor ops; app lifecycle.

use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use kelta_proto::api::CoreApi;
use kelta_proto::codehost::{PrCreate, PrDraft, Review};
use kelta_proto::error::{ErrorCode, KeltaError};
use kelta_proto::events::{BusEvent, Toast, bus};
use kelta_proto::ids::{SessionId, WorkItemId};
use kelta_proto::model::{
    CloseOnExit, EditorTarget, FinishMergedReport, FinishOpts, GitStatus, Lifecycle, OpenPaneRequest,
    PaneContent, Placement, RestorePolicy, SessionInfo, SessionKind, ShipOrigin, SkippedItem, SpawnRequest,
    StepStatus, WorkItem, WorkKind, WorkSource, WorkState,
};
use kelta_proto::settings::{EditorOpenMode, EditorRestore};
use kelta_proto::tracker::TicketRef;

use crate::claude::LaunchMode;
use crate::fixloop::refuse_if_busy;
use crate::layout::{self, SlotKind};
use crate::nvim::NvimClient;
use crate::plan;
use crate::saga::{self, COLS, Env, Journal, ROWS, await_exit};
use crate::template::{Mode, render};
use crate::{WorkService, editor, files, git};

/// Push sessions wait for credentials typed by the user; bounded anyway.
const PUSH_TIMEOUT: Duration = Duration::from_secs(15 * 60);
/// A resumed Claude that exits non-zero this fast had its `--resume` refused.
const RESUME_GRACE: Duration = Duration::from_secs(5);
/// `WorkState::Merged.detail` when several Done transitions fit (the Finish dialog asks).
pub const CHOOSE_DONE: &str = "choose Done status";

impl WorkService {
    // ---- resume ------------------------------------------------------------------------------

    /// Resume the item; `prompt` goes to its previous Claude conversation (pasted into a live
    /// idle Claude, or passed to `claude --resume`).
    pub(crate) async fn resume_item(
        &self,
        id: &WorkItemId,
        prompt: Option<String>,
    ) -> Result<WorkItem, KeltaError> {
        let lock = self.item_lock(id);
        let _guard = lock.try_lock().map_err(|_| self.busy(id))?;
        let mut item = self.load(id).await?;
        match &item.state {
            WorkState::Finished => Err(KeltaError::conflict("work item is finished")),
            _ if prompt.is_some() && !matches!(item.state, WorkState::Active | WorkState::PrOpen) => {
                Err(KeltaError::conflict("work item has not finished starting"))
            }
            WorkState::Planned | WorkState::Starting | WorkState::Failed { .. } => {
                if let WorkState::Failed { step, .. } = item.state.clone() {
                    self.set_step(&mut item, &step, StepStatus::Pending, None).await?;
                }
                self.run_saga_locked(id).await
            }
            WorkState::Active | WorkState::PrOpen | WorkState::Merged { .. } | WorkState::PrClosed => {
                self.reopen(&mut item, prompt).await.map(|()| item)
            }
        }
    }

    /// Focus the work item's tab, respawning sessions that are gone and recreating the tab if needed.
    async fn reopen(&self, item: &mut WorkItem, mut prompt: Option<String>) -> Result<(), KeltaError> {
        let env = self.env(&item.project_id, &item.repo_id)?;
        let mut j = self.load_journal(&item.id);
        let template = self.template_of(&env, &j);
        let alive = |s: &Option<SessionId>| -> Option<SessionInfo> {
            s.as_ref().and_then(|sid| env.core.session_get(sid)).filter(|i| i.lifecycle != Lifecycle::Exited)
        };
        let mut respawned = false;
        for slot in layout::slots(&template.layout) {
            match slot.kind {
                SlotKind::Claude { .. } => {
                    if let Some(live) = alive(&j.claude_session) {
                        let Some(p) = prompt.take() else { continue };
                        if live.lifecycle == Lifecycle::Live {
                            crate::fixloop::paste_prompt(&env.core, &live, &p).await?;
                            continue;
                        }
                        // Dormant (never attached since restart): respawn it below with the prompt.
                        env.core.session_kill(&live.id, false).await?;
                        prompt = Some(p);
                    }
                    let Some(uuid) = item.claude_uuid.clone() else { continue };
                    if let Some(old) = j.claude_session.take() {
                        item.session_ids.retain(|s| s != &old);
                    }
                    let rx = env.core.subscribe();
                    let mode = LaunchMode::Resume { uuid, prompt: prompt.clone() };
                    let sid = self.spawn_claude(&env, item, &mut j, mode).await?;
                    respawned = true;
                    self.watch_resume(rx, item.id.clone(), sid, prompt.take());
                }
                SlotKind::Editor | SlotKind::Shell { .. } => {
                    if alive(&j.sessions.get(&slot.idx).cloned()).is_some() {
                        continue;
                    }
                    if let Some(old) = j.sessions.remove(&slot.idx) {
                        item.session_ids.retain(|s| s != &old);
                    }
                    let sid = self.respawn_leaf(&env, item, &mut j, &slot.kind, slot.name.as_deref()).await?;
                    j.sessions.insert(slot.idx, sid.clone());
                    if !item.session_ids.contains(&sid) {
                        item.session_ids.push(sid);
                    }
                    respawned = true;
                }
                _ => {}
            }
        }
        item.session_ids.retain(|s| env.core.session_get(s).is_some());
        if respawned {
            self.arrange(&env, item, &mut j, &template, false).await?;
        } else {
            let focus = j.claude_session.clone().or_else(|| item.session_ids.first().cloned());
            if let Some(sid) = focus {
                let title = Self::tab_title(item, &j);
                self.open_pane(
                    &env,
                    item,
                    &title,
                    PaneContent::Terminal { session_id: sid },
                    Placement::Focused,
                    true,
                )
                .await?;
            }
        }
        self.save_journal(&item.id, &j)?;
        self.save(item).await
    }

    async fn respawn_leaf(
        &self,
        env: &Env,
        item: &mut WorkItem,
        j: &mut Journal,
        kind: &SlotKind,
        name: Option<&str>,
    ) -> Result<SessionId, KeltaError> {
        // nvim: restore the saved session when there is one.
        if matches!(kind, SlotKind::Editor)
            && let (Some(file), Some(preset)) =
                (j.editor_session_file.clone(), editor::preset(&env.settings.editor, None))
            && preset.restore == EditorRestore::Mksession
            && preset.open == EditorOpenMode::Rpc
            && file.exists()
        {
            let run = match j.editor_run.clone() {
                Some(r) => r,
                None => files::alloc_run_dir(&self.dirs, None)?,
            };
            files::private_dir(&run)?;
            j.editor_run = Some(run.clone());
            let sock = run.join("nvim.sock");
            let _ = std::fs::remove_file(&sock);
            let info = env
                .core
                .session_spawn(SpawnRequest {
                    id: None,
                    project_id: item.project_id.clone(),
                    kind: SessionKind::Editor { adapter: preset.id.clone() },
                    name: Some(name.unwrap_or(&preset.id).to_owned()),
                    program: Some(preset.command.clone()),
                    args: vec![
                        "-S".into(),
                        file.to_string_lossy().into_owned(),
                        "--listen".into(),
                        sock.to_string_lossy().into_owned(),
                    ],
                    cwd: Some(item.worktree.clone()),
                    env: BTreeMap::new(),
                    cols: COLS,
                    rows: ROWS,
                    work_item_id: Some(item.id.clone()),
                    restore: RestorePolicy::Editor { session_file: Some(file) },
                    close_on_exit: CloseOnExit::Never,
                    template_id: Some(j.template_id()),
                })
                .await?;
            item.nvim_socket = Some(sock);
            return Ok(info.id);
        }
        self.spawn_leaf(env, item, j, kind, name).await
    }

    /// `--resume` refused (fast non-zero exit) → relaunch with `--continue` in the same pane.
    fn watch_resume(
        &self,
        mut rx: tokio::sync::broadcast::Receiver<kelta_proto::events::BusEvent>,
        id: WorkItemId,
        sid: SessionId,
        prompt: Option<String>,
    ) {
        let me = self.me.clone();
        let Ok(rt) = tokio::runtime::Handle::try_current() else { return };
        rt.spawn(async move {
            let Some(svc) = me.upgrade() else { return };
            let Ok(core) = svc.api() else { return };
            let Ok(code) = await_exit(&core, &mut rx, &sid, RESUME_GRACE).await else { return };
            if code == 0 {
                return;
            }
            let Ok(mut item) = svc.load(&id).await else { return };
            let Ok(env) = svc.env(&item.project_id, &item.repo_id) else { return };
            let mut j = svc.load_journal(&id);
            tracing::info!(work_item = %id, "claude --resume refused; falling back to --continue");
            let with_prompt = prompt.is_some();
            match svc.spawn_claude(&env, &mut item, &mut j, LaunchMode::Continue { prompt }).await {
                Ok(new) => {
                    core.toast(Toast::warn(if with_prompt {
                        "Previous conversation not found; Claude continued the latest one in this worktree with your prompt."
                    } else {
                        "Previous conversation not found; Claude continued the latest one in this worktree."
                    }));
                    let title = Self::tab_title(&item, &j);
                    let old = PaneContent::Terminal { session_id: sid.clone() };
                    let _ = svc.open_pane(&env, &mut item, &title, old, Placement::Focused, true).await;
                    let _ = svc
                        .open_pane(
                            &env,
                            &mut item,
                            &title,
                            PaneContent::Terminal { session_id: new },
                            Placement::ReplaceFocused,
                            true,
                        )
                        .await;
                    item.session_ids.retain(|s| s != &sid);
                    let _ = svc.save_journal(&id, &j);
                    let _ = svc.save(&mut item).await;
                }
                Err(e) => core.toast(Toast::error(format!("Could not restart Claude: {}", e.message))),
            }
        });
    }

    /// Spawn request restoring a Dormant Claude session that belongs to a work item:
    /// `claude --resume <uuid>` (or `--continue` with `fallback`), with its per-session files
    /// regenerated (the runtime dir does not survive a reboot). `None` = not a work item session.
    pub async fn claude_restore_request(
        &self,
        session: &SessionId,
        fallback: bool,
        prompt: Option<String>,
    ) -> Result<Option<SpawnRequest>, KeltaError> {
        let Some(item) = self.for_session(session).await else { return Ok(None) };
        let Some(uuid) = item.claude_uuid.clone() else { return Ok(None) };
        let env = self.env(&item.project_id, &item.repo_id)?;
        let mut j = self.load_journal(&item.id);
        let run = self.ensure_claude_run(&item, &mut j)?;
        let port = self.http_port(&env.settings).await;
        if let Err(e) = self.write_claude_files(&env, &item, &j, &run, session, port) {
            self.release_port(port);
            return Err(e);
        }
        if port.is_some() {
            self.http_sessions.lock().insert(session.clone());
        }
        self.save_journal(&item.id, &j)?;
        let mode =
            if fallback { LaunchMode::Continue { prompt } } else { LaunchMode::Resume { uuid, prompt } };
        Ok(Some(self.claude_request(&env, &item, &j, &run, mode, port, session)))
    }

    // ---- create PR ---------------------------------------------------------------------------

    pub(crate) async fn create_pr_impl(
        &self,
        id: &WorkItemId,
        draft: PrDraft,
        origin: ShipOrigin,
    ) -> Result<WorkItem, KeltaError> {
        let lock = self.item_lock(id);
        let _guard = lock.try_lock().map_err(|_| self.busy(id))?;
        let mut item = self.load(id).await?;
        if item.state == WorkState::Finished {
            return Err(KeltaError::conflict("work item is finished"));
        }
        if item.state.pr_done() {
            return Err(KeltaError::conflict("The PR of this work item is already merged or closed."));
        }
        let env = self.env(&item.project_id, &item.repo_id)?;
        // MCP: Claude is the caller, so its session is Working by definition.
        if origin == ShipOrigin::Ui {
            refuse_if_busy(&env, &item, &self.load_journal(id), "Ship")?;
        }
        let remote_base = format!("{}/{}", env.repo.remote, item.base);
        if git::ref_exists(&item.worktree, &remote_base).await?
            && git::ahead_behind(&item.worktree, &remote_base).await?.0 == 0
        {
            return Err(KeltaError::conflict(format!("No commits ahead of {}.", item.base)));
        }
        let binding = env.repo.code_host.clone().ok_or_else(|| {
            KeltaError::unsupported(format!("repo {} has no code host binding", env.repo.id))
        })?;
        let host = env.core.code_host_for(&binding.account).await?;
        let j = self.load_journal(id);

        let ev = BusEvent::new(bus::PR_BEFORE_CREATE, serde_json::json!({ "draft": draft }))
            .with_project(item.project_id.clone())
            .with_work_item(item.id.clone());
        self.blocking(&env.core, ev).await?;

        let args = vec!["push".into(), "-u".into(), env.repo.remote.clone(), item.branch.clone()];
        let code = self.push_pane(&env, &mut item, &j, args).await?;
        if code != 0 {
            return Err(KeltaError::upstream(format!("git push failed (exit {code})")));
        }

        let review: Review = match host.find_for_branch(&binding.repo, &item.branch).await? {
            Some(r) => r,
            None => {
                let defaults = self.pr_defaults(&env, &item, &j, &binding.repo).await?;
                host.create(&PrCreate {
                    repo: binding.repo.clone(),
                    head: item.branch.clone(),
                    base: item.base.clone(),
                    title: draft
                        .title
                        .filter(|t| !t.trim().is_empty())
                        .or(defaults.title)
                        .unwrap_or_default(),
                    body: draft.body.or(defaults.body).unwrap_or_default(),
                    draft: draft.draft.or(defaults.draft).unwrap_or(false),
                })
                .await?
            }
        };
        self.add_title_key(&env, &item, Some(review.clone())).await;
        // Re-load: the push took a while and hooks may have written the item meanwhile.
        let item = self
            .update(id, |w| {
                w.pr_url = Some(review.url.clone());
                w.state = WorkState::PrOpen;
                true
            })
            .await?;
        env.core.publish(
            BusEvent::new(bus::PR_CREATED, serde_json::json!({ "review": review }))
                .with_project(item.project_id.clone())
                .with_work_item(item.id.clone()),
        );
        self.on_pr(&env, &item, &j, &review).await;
        Ok(item)
    }

    pub(crate) async fn pr_draft_impl(&self, id: &WorkItemId) -> Result<PrDraft, KeltaError> {
        let item = self.load(id).await?;
        let env = self.env(&item.project_id, &item.repo_id)?;
        let repo = env.repo.code_host.as_ref().map(|b| b.repo.clone()).unwrap_or_default();
        self.pr_defaults(&env, &item, &self.load_journal(id), &repo).await
    }

    /// Ship defaults: ticket items render `work.pr.*`; scratch items use their title (else the last
    /// commit subject) and their task as the body.
    async fn pr_defaults(
        &self,
        env: &Env,
        item: &WorkItem,
        j: &Journal,
        host_repo: &str,
    ) -> Result<PrDraft, KeltaError> {
        let mut ctx = self.item_ctx(env, item, j);
        let closes = match &j.ticket {
            Some(t) if t.is_github() => {
                let (repo, num) = t.key.rsplit_once('#').unwrap_or(("", t.key.as_str()));
                if repo.is_empty() || repo == host_repo {
                    format!("Closes #{num}")
                } else {
                    format!("Closes {}", t.key)
                }
            }
            _ => String::new(),
        };
        ctx.set("closes", closes);
        let pr = &env.settings.work.pr;
        let title = if j.ticket.is_some() || item.ticket.is_some() {
            render(&pr.title_template, &ctx, Mode::Lenient)?
        } else if let Some(t) = item.title.clone() {
            t
        } else {
            git::last_subject(&item.worktree)
                .await
                .ok()
                .filter(|s| !s.is_empty())
                .unwrap_or(item.branch.clone())
        };
        let body = render(&pr.body_template, &ctx, Mode::Lenient)?;
        // Scratch items: the task is the PR's description (FLOW §4.3 step 3).
        let task = if item.ticket.is_none() { ctx.get("task").unwrap_or_default() } else { "" };
        let body = format!("{}\n\n{task}", body.trim()).trim().to_owned();
        Ok(PrDraft { title: Some(title), body: Some(body), draft: Some(pr.draft) })
    }

    pub(crate) async fn link_pr_impl(&self, id: &WorkItemId, review: &Review) -> Result<(), KeltaError> {
        let lock = self.item_lock(id);
        // Busy: a Ship or Finish runs and owns the PR fields; the next poll retries.
        let Ok(_guard) = lock.try_lock() else { return Ok(()) };
        let mut item = self.load(id).await?;
        if item.pr_url.is_some() || item.state != WorkState::Active {
            return Ok(());
        }
        item.pr_url = Some(review.url.clone());
        item.state = WorkState::PrOpen;
        self.save(&mut item).await?;
        let env = self.env(&item.project_id, &item.repo_id)?;
        self.on_pr(&env, &item, &self.load_journal(id), review).await;
        Ok(())
    }

    /// `pr.merged` / `pr.closed` (idempotent): Merged + guarded Done move, or PrClosed.
    pub(crate) async fn pr_ended(&self, review: &Review, merged: bool) -> Result<(), KeltaError> {
        let items = self.store.list_items(None).await?;
        let ours = items.into_iter().filter(|w| w.pr_url.as_deref() == Some(review.url.as_str()));
        for it in ours {
            let lock = self.item_lock(&it.id);
            let _guard = lock.lock().await;
            let mut item = self.load(&it.id).await?;
            if item.state.pr_done() || matches!(item.state, WorkState::Finished | WorkState::Failed { .. }) {
                continue;
            }
            item.state = if merged {
                WorkState::Merged { detail: self.move_done(&item).await }
            } else {
                WorkState::PrClosed
            };
            self.save(&mut item).await?;
        }
        Ok(())
    }

    /// On merge, move the ticket to Done only when exactly one transition fits and needs no
    /// fields (FLOW §4.6). Returns why it was not moved, for the phase detail.
    async fn move_done(&self, item: &WorkItem) -> Option<String> {
        let t = item.ticket.as_ref()?;
        let env = match self.env(&item.project_id, &item.repo_id) {
            Ok(e) => e,
            Err(e) => return Some(format!("ticket not moved: {}", e.message)),
        };
        let target = env
            .project
            .tracker
            .as_ref()
            .and_then(|b| b.status_map.done.clone())
            .or_else(|| env.settings.work.on_merge.transition_to.clone())?;
        let tracker = match env.core.tracker_for(&t.account).await {
            Ok(x) => x,
            Err(e) => return Some(format!("ticket not moved: {}", e.message)),
        };
        match saga::transition_ticket_strict(
            &env.core,
            tracker.as_ref(),
            t,
            &target,
            &item.project_id,
            &item.id,
        )
        .await
        {
            Ok(_) => None,
            Err(e) if e.code == ErrorCode::Conflict => Some(CHOOSE_DONE.into()),
            Err(e) => Some(format!("ticket not moved: {}", e.message)),
        }
    }

    /// `git <args>` (a push) in a visible transient pane, awaited through `session.exited`; returns
    /// the exit code.
    pub(crate) async fn push_pane(
        &self,
        env: &Env,
        item: &mut WorkItem,
        j: &Journal,
        args: Vec<String>,
    ) -> Result<i32, KeltaError> {
        let mut rx = env.core.subscribe();
        let info = env
            .core
            .session_spawn(SpawnRequest {
                id: None,
                project_id: item.project_id.clone(),
                kind: SessionKind::Custom,
                name: Some("git push".into()),
                program: Some("git".into()),
                args,
                cwd: Some(item.worktree.clone()),
                env: BTreeMap::new(),
                cols: COLS,
                rows: ROWS,
                work_item_id: Some(item.id.clone()),
                restore: RestorePolicy::None,
                close_on_exit: CloseOnExit::OnSuccess,
                template_id: None,
            })
            .await?;
        let title = Self::tab_title(item, j);
        if let Err(e) = self
            .open_pane(
                env,
                item,
                &title,
                PaneContent::Terminal { session_id: info.id.clone() },
                Placement::SplitDown,
                true,
            )
            .await
        {
            tracing::warn!(error = %e.message, "could not show the push pane");
        }
        await_exit(&env.core, &mut rx, &info.id, PUSH_TIMEOUT).await
    }

    /// FLOW §4.3 step 4: after Link to ticket, a Ship/Push adds the key to the PR title unless it has
    /// one. Clears `pr_title_needs_key` once done or impossible; a failed rename retries next push.
    pub(crate) async fn add_title_key(&self, env: &Env, item: &WorkItem, review: Option<Review>) {
        let (true, Some(t), Some(binding)) = (item.pr_title_needs_key, &item.ticket, &env.repo.code_host)
        else {
            return;
        };
        let renamed = async {
            let host = env.core.code_host_for(&binding.account).await?;
            let review = match review {
                Some(r) => Some(r),
                None => host.find_for_branch(&binding.repo, &item.branch).await?,
            };
            let regex = &env.settings.reviews.ticket_key_regex;
            match review.and_then(|r| Some((plan::pr_title_with_key(&r.title, &t.key, regex)?, r.r#ref))) {
                Some((title, r)) => host.update_title(&r, &title).await,
                None => Ok(()),
            }
        }
        .await;
        if let Err(e) = renamed
            && e.code != ErrorCode::Unsupported
        {
            env.core.toast(Toast::warn(format!("{}: PR title not updated — {}", t.key, e.message)));
            return;
        }
        if let Err(e) = self.update(&item.id, |w| std::mem::replace(&mut w.pr_title_needs_key, false)).await {
            tracing::warn!(error = %e.message, "could not clear pr_title_needs_key");
        }
    }

    /// `work.on_pr`: transition (status_map.review overrides) + comment; non-fatal.
    pub(crate) async fn on_pr(&self, env: &Env, item: &WorkItem, j: &Journal, review: &Review) {
        let Some(t) = &item.ticket else { return };
        let Ok(tracker) = env.core.tracker_for(&t.account).await else { return };
        let on = &env.settings.work.on_pr;
        let target = env
            .project
            .tracker
            .as_ref()
            .and_then(|b| b.status_map.review.clone())
            .or_else(|| on.transition_to.clone());
        if let Some(target) = target
            && let Err(e) =
                saga::transition_ticket(&env.core, tracker.as_ref(), t, &target, &item.project_id, &item.id)
                    .await
        {
            env.core.toast(Toast::warn(format!("{}: transition failed — {}", t.key, e.message)));
        }
        if let Some(c) = &on.comment {
            let mut ctx = self.item_ctx(env, item, j);
            plan::add_review(&mut ctx, review);
            if let Ok(body) = render(c, &ctx, Mode::Lenient)
                && !body.trim().is_empty()
            {
                match tracker.comment(t, &body).await {
                    Ok(()) => env.core.publish(
                        BusEvent::new(
                            bus::TICKET_COMMENTED,
                            serde_json::json!({ "ticket": t, "markdown": body }),
                        )
                        .with_project(item.project_id.clone())
                        .with_work_item(item.id.clone()),
                    ),
                    Err(e) => {
                        env.core.toast(Toast::warn(format!("{}: comment failed — {}", t.key, e.message)))
                    }
                }
            }
        }
    }

    // ---- link ------------------------------------------------------------------------------

    /// `work_link` (FLOW §4.3 step 4): a scratch item becomes ticket-kind; the branch never changes.
    pub(crate) async fn link_impl(
        &self,
        id: &WorkItemId,
        ticket: TicketRef,
        apply_side_effects: bool,
    ) -> Result<WorkItem, KeltaError> {
        let lock = self.item_lock(id);
        let _guard = lock.try_lock().map_err(|_| KeltaError::conflict("work item is busy"))?;
        let mut item = self.load(id).await?;
        match item.kind {
            WorkKind::Branch => {}
            WorkKind::Review => return Err(KeltaError::conflict("Review checkout: read-only")),
            WorkKind::Ticket => return Err(KeltaError::conflict("work item already has a ticket")),
        }
        if item.state == WorkState::Finished {
            return Err(KeltaError::conflict("work item is finished"));
        }
        let items = self.store.list_items(Some(&item.project_id)).await?;
        if plan::existing_for(&items, &WorkSource::Ticket { ticket: ticket.clone() })
            .is_some_and(|w| w.id != item.id)
        {
            return Err(KeltaError::conflict(format!("{} already has a work item", ticket.key)));
        }
        let env = self.env(&item.project_id, &item.repo_id)?;
        let tracker = env.core.tracker_for(&ticket.account).await?;
        let detail = tracker.get(&ticket).await?;
        let mut j = self.load_journal(id);
        // Same file the ticket saga writes, so the next resume's CONTEXT.md points at a real ticket.md.
        // shortcut: the open tab keeps its `wip` title until the next start (no tab-rename API), add one if it confuses.
        let run = self.ensure_claude_run(&item, &mut j)?;
        files::write_private(
            &run.join(crate::claude::TICKET_FILE),
            files::ticket_markdown(&detail).as_bytes(),
        )?;
        j.ticket =
            Some(plan::TicketSnap::from_ticket(&detail.ticket, tracker.branch_key(&ticket), tracker.kind()));
        item.kind = WorkKind::Ticket;
        item.ticket = Some(detail.ticket.r#ref.clone());
        item.pr_title_needs_key = item.pr_url.is_some();
        self.save_journal(id, &j)?;
        self.save(&mut item).await?;
        if apply_side_effects {
            let ctx = self.item_ctx(&env, &item, &j);
            if let Some(p) = j.plan.as_mut() {
                p.side_effects = plan::side_effects(&env.settings, &env.project, true, &ctx);
            }
            j.effects_done.clear();
            // Failures are toasted inside, like the saga step (non-fatal).
            self.step_effects(&env, &mut item, &mut j).await?;
            self.save_journal(id, &j)?;
            if item.pr_url.is_some()
                && let Some(binding) = env.repo.code_host.clone()
            {
                let host = env.core.code_host_for(&binding.account).await?;
                if let Some(review) = host.find_for_branch(&binding.repo, &item.branch).await? {
                    self.on_pr(&env, &item, &j, &review).await;
                }
            }
        }
        Ok(item)
    }

    // ---- finish ------------------------------------------------------------------------------

    pub(crate) async fn finish_impl(
        &self,
        id: &WorkItemId,
        opts: FinishOpts,
    ) -> Result<WorkItem, KeltaError> {
        let lock = self.item_lock(id);
        let _guard = lock.try_lock().map_err(|_| self.busy(id))?;
        let item = self.load(id).await?;
        if item.state == WorkState::Finished {
            return Ok(item);
        }
        let env = self.env(&item.project_id, &item.repo_id)?;
        let j = self.load_journal(id);
        let ev = BusEvent::new(
            bus::WORK_BEFORE_FINISH,
            serde_json::json!({ "work_item_id": item.id, "opts": opts }),
        )
        .with_project(item.project_id.clone())
        .with_work_item(item.id.clone());
        self.blocking(&env.core, ev).await?;

        let repo = env.repo.path.clone();
        let is_worktree = item.worktree.is_dir()
            && !git::same_path(&item.worktree, &repo)
            && git::worktree_at(&repo, &item.worktree).await?.is_some();
        let mut copies: Vec<String> = Vec::new();
        // Clean and nothing unpushed (or merged upstream): a merged branch may be force-deleted.
        let mut verified_clean = false;
        let merged = matches!(item.state, WorkState::Merged { .. });
        if opts.remove_worktree && is_worktree {
            if item.pr_url.is_some() {
                // Best effort: see a merge that happened on the code host since the last fetch.
                let timeout = Duration::from_secs(u64::from(env.settings.worktree.fetch_timeout_secs.max(1)));
                if let Err(e) = git::fetch(&repo, &env.repo.remote, &[&item.base], timeout).await {
                    tracing::warn!(error = %e.message, "fetch before finish failed");
                }
            }
            let report = self.dirty_report(&env, &item, &j).await?;
            copies = report.copies;
            if !opts.force && (!report.files.is_empty() || report.unpushed > 0) {
                let msg = match (report.files.is_empty(), report.unpushed) {
                    (false, 0) => format!("{} has uncommitted changes", item.worktree.display()),
                    (true, n) => format!("{} has {n} unpushed commit(s)", item.branch),
                    (false, n) => format!(
                        "{} has uncommitted changes and {n} unpushed commit(s)",
                        item.worktree.display()
                    ),
                };
                return Err(KeltaError::new(ErrorCode::Dirty, msg).with_detail(serde_json::json!({
                    "files": report.files,
                    "unpushed": report.unpushed > 0,
                    "unpushed_commits": report.unpushed,
                })));
            }
            verified_clean = report.files.is_empty() && report.unpushed == 0;
        }

        if !env.settings.worktree.teardown.trim().is_empty()
            && item.worktree.is_dir()
            && let Err(e) = self.teardown(&env, &item).await
        {
            if !opts.force {
                return Err(e);
            }
            env.core.toast(Toast::warn(e.message));
        }

        for sid in &item.session_ids {
            if env.core.session_get(sid).is_some_and(|s| s.lifecycle != Lifecycle::Exited)
                && let Err(e) = env.core.session_kill(sid, opts.force).await
            {
                tracing::warn!(session = %sid, error = %e.message, "kill on finish failed");
            }
            if self.http_sessions.lock().remove(sid)
                && let Some(h) = self.host()
            {
                h.release_http();
            }
        }

        if opts.remove_worktree && is_worktree {
            // Delete our include copies so git's own clean check (no --force) still guards files
            // written after the dirty report (e.g. by Claude before it was killed).
            for f in &copies {
                let _ = std::fs::remove_file(item.worktree.join(f));
            }
            git::worktree_remove(&repo, &item.worktree, opts.force).await?;
            env.core.publish(
                BusEvent::new(
                    bus::WORKTREE_REMOVED,
                    serde_json::json!({ "path": item.worktree, "branch": item.branch, "repo_id": item.repo_id }),
                )
                .with_project(item.project_id.clone())
                .with_work_item(item.id.clone()),
            );
        }
        if opts.delete_branch
            && git::local_branch_exists(&repo, &item.branch).await.unwrap_or(false)
            // `-d` refuses squash-merged branches; their work is verified to be in the base.
            && let Err(e) =
                git::delete_branch(&repo, &item.branch, opts.force || (merged && verified_clean)).await
        {
            env.core.toast(Toast::warn(format!("Branch {} kept: {}", item.branch, e.message)));
        }
        if let Some(t) = &item.ticket {
            // Merged / closed: the merge listener already made the unambiguous move; any other
            // Done status is an explicit choice, never a default.
            let target = if item.state.pr_done() {
                opts.transition_to.clone()
            } else {
                opts.transition_to
                    .clone()
                    .or_else(|| env.project.tracker.as_ref().and_then(|b| b.status_map.done.clone()))
                    .or_else(|| item.pr_url.as_ref().and(env.settings.work.on_merge.transition_to.clone()))
            };
            if let Some(target) = target {
                match env.core.tracker_for(&t.account).await {
                    Ok(tracker) => {
                        if let Err(e) = saga::transition_ticket(
                            &env.core,
                            tracker.as_ref(),
                            t,
                            &target,
                            &item.project_id,
                            &item.id,
                        )
                        .await
                        {
                            env.core
                                .toast(Toast::warn(format!("{}: transition failed — {}", t.key, e.message)));
                        }
                    }
                    Err(e) => env.core.toast(Toast::warn(format!("{}: {}", t.key, e.message))),
                }
            }
        }
        crate::review::delete_refs(&repo, id).await;
        for dir in [&j.claude_run, &j.editor_run].into_iter().flatten() {
            let _ = std::fs::remove_dir_all(dir);
        }
        let _ = std::fs::remove_file(self.journal_path(id));
        let item = self
            .update(id, |w| {
                w.state = WorkState::Finished;
                w.port_base = None;
                w.review_due = false;
                w.claude_replied = false;
                true
            })
            .await?;
        env.core.publish(
            BusEvent::new(bus::WORK_FINISHED, serde_json::json!({ "work_item_id": item.id, "opts": opts }))
                .with_project(item.project_id.clone())
                .with_work_item(item.id.clone()),
        );
        Ok(item)
    }

    /// `worktree.teardown` in a pane below (kept open when it fails), before sessions are killed.
    async fn teardown(&self, env: &Env, item: &WorkItem) -> Result<(), KeltaError> {
        let cmd = &env.settings.worktree.teardown;
        let mut rx = env.core.subscribe();
        let info = self
            .spawn_script(env, item, "teardown", std::slice::from_ref(cmd), CloseOnExit::OnSuccess)
            .await?;
        let pane = OpenPaneRequest {
            content: PaneContent::Terminal { session_id: info.id.clone() },
            placement: Placement::SplitDown,
            focus: false,
            tab_title: None,
            work_item_id: Some(item.id.clone()),
        };
        if let Err(e) = env.core.layout_open(&item.project_id, pane).await {
            tracing::warn!(error = %e.message, "teardown pane");
        }
        match await_exit(&env.core, &mut rx, &info.id, Duration::from_secs(600)).await? {
            0 => Ok(()),
            code => Err(KeltaError::upstream(format!(
                "teardown `{cmd}` failed (exit {code}); fix it, or Finish with force to skip it"
            ))),
        }
    }

    pub(crate) async fn finish_merged_impl(
        &self,
        ids: &[WorkItemId],
    ) -> Result<FinishMergedReport, KeltaError> {
        let mut report = FinishMergedReport::default();
        // Only the ids the dialog listed: one confirmation finishes exactly what it showed.
        for id in ids {
            let skip = |reason: String| SkippedItem { id: id.clone(), reason };
            let item = match self.load(id).await {
                Ok(item) => item,
                Err(e) => {
                    report.skipped.push(skip(e.message));
                    continue;
                }
            };
            match &item.state {
                WorkState::Merged { detail: None } => {}
                WorkState::Merged { detail: Some(why) } => {
                    report.skipped.push(skip(why.clone()));
                    continue;
                }
                _ => {
                    report.skipped.push(skip("no longer merged".into()));
                    continue;
                }
            }
            let opts =
                FinishOpts { remove_worktree: true, delete_branch: true, force: false, transition_to: None };
            match self.finish_impl(id, opts).await {
                Ok(done) => report.finished.push(done),
                Err(e) => report.skipped.push(skip(e.message)),
            }
        }
        Ok(report)
    }

    pub(crate) async fn dirty_report(
        &self,
        env: &Env,
        item: &WorkItem,
        j: &Journal,
    ) -> Result<DirtyReport, KeltaError> {
        let entries = git::dirty_files(&item.worktree).await?;
        // Only files Kelta copied (journal) and the user left byte-identical are ours to delete.
        let (mut files, mut copies) = (Vec::new(), Vec::new());
        for f in entries {
            let ours = j.include_copies.contains(&f)
                && std::fs::read(item.worktree.join(&f))
                    .ok()
                    .is_some_and(|b| std::fs::read(env.repo.path.join(&f)).is_ok_and(|src| src == b));
            if ours { copies.push(f) } else { files.push(f) }
        }
        let mut unpushed = git::unpushed_count(&item.worktree, &item.base).await?;
        // Squash/rebase-merged PR whose remote branch was pruned: the work is in the remote base.
        let remote_base = format!("refs/remotes/{}/{}", env.repo.remote, item.base);
        if unpushed > 0
            && git::ref_exists(&item.worktree, &remote_base).await?
            && git::changes_merged(&item.worktree, &remote_base).await?
        {
            unpushed = 0;
        }
        Ok(DirtyReport { files, copies, unpushed })
    }

    // ---- status ------------------------------------------------------------------------------

    pub(crate) async fn status_impl(&self, id: &WorkItemId) -> Result<GitStatus, KeltaError> {
        // Re-read a recorded rebase on every status read (window focus, FLOW §4.4).
        let item = self.refresh_rebase(id).await?;
        let env = self.env(&item.project_id, &item.repo_id)?;
        self.git_status(&env, &item).await
    }

    // ---- editors -----------------------------------------------------------------------------

    /// nvim socket of an editor session (session meta, then its work item).
    async fn editor_socket(&self, info: &SessionInfo) -> Option<PathBuf> {
        if let Some(s) = info.editor.as_ref().and_then(|e| e.socket.clone()) {
            return Some(s);
        }
        self.for_session(&info.id).await.and_then(|w| w.nvim_socket)
    }

    async fn editor_session(
        &self,
        core: &Arc<dyn CoreApi>,
        target: &EditorTarget,
    ) -> Result<SessionInfo, KeltaError> {
        match target {
            EditorTarget::Session { id } => {
                core.session_get(id).ok_or_else(|| KeltaError::not_found(format!("session {id}")))
            }
            EditorTarget::WorkItem { id } => {
                let item = self.load(id).await?;
                item.session_ids
                    .iter()
                    .filter_map(|s| core.session_get(s))
                    .find(|s| {
                        matches!(s.kind, SessionKind::Editor { .. }) && s.lifecycle != Lifecycle::Exited
                    })
                    .ok_or_else(|| KeltaError::not_found(format!("work item {id} has no editor session")))
            }
        }
    }

    pub(crate) async fn editor_open_impl(
        &self,
        target: EditorTarget,
        path: &Path,
        line: Option<u32>,
    ) -> Result<(), KeltaError> {
        let core = self.api()?;
        let info = self.editor_session(&core, &target).await?;
        let SessionKind::Editor { adapter } = &info.kind else {
            return Err(KeltaError::invalid(format!("session {} is not an editor", info.id)));
        };
        let settings = core.settings(Some(&info.project_id));
        let preset = editor::preset(&settings.editor, Some(adapter))
            .cloned()
            .ok_or_else(|| KeltaError::invalid(format!("unknown editor preset {adapter}")))?;
        let file = if path.is_absolute() { path.to_path_buf() } else { info.cwd.join(path) };
        match preset.open {
            EditorOpenMode::Rpc => {
                let sock = self.editor_socket(&info).await.ok_or_else(|| {
                    KeltaError::not_found(format!("no nvim socket for session {}", info.id))
                })?;
                let mut c = NvimClient::connect(&sock).await?;
                c.edit(&file, line, true).await
            }
            EditorOpenMode::Keys => {
                let tpl =
                    preset.open_keys.clone().unwrap_or_else(|| "<C-\\><C-N>:edit +{line} {file}<CR>".into());
                let name = file.to_string_lossy();
                // Control bytes would be typed straight into the editor.
                if name.chars().any(char::is_control) {
                    return Err(KeltaError::invalid("file name contains control characters"));
                }
                let mut ctx = editor::ctx(None, ".", Some(&file), line, &info.cwd, &info.id.sid8());
                ctx.set("file", editor::ex_escape(&name));
                let keys = render(&tpl, &ctx, Mode::Lenient)?;
                core.session_write(&info.id, &editor::vim_keys(&keys)).await
            }
            EditorOpenMode::Command => {
                let tpl = preset.open_cmd.clone().unwrap_or_default();
                let ctx = editor::ctx(None, ".", Some(&file), line, &info.cwd, &info.id.sid8());
                editor::run_detached(&editor::render_args(&tpl, &ctx)?, &info.cwd).await
            }
            EditorOpenMode::None => {
                Err(KeltaError::unsupported(format!("editor {} cannot open files remotely", preset.id)))
            }
        }
    }

    pub(crate) async fn editor_diff_impl(
        &self,
        target: EditorTarget,
        old: &Path,
        proposed: &Path,
        close: bool,
    ) -> Result<(), KeltaError> {
        let core = self.api()?;
        let info = self.editor_session(&core, &target).await?;
        let sock = self
            .editor_socket(&info)
            .await
            .ok_or_else(|| KeltaError::unsupported("the IDE diff needs an nvim (RPC) editor session"))?;
        let path = |p: &Path| rmpv::Value::from(p.to_string_lossy().as_ref());
        let mut c = NvimClient::connect(&sock).await?;
        c.exec_lua(crate::nvim::LUA_DIFF, vec![path(old), path(proposed), rmpv::Value::from(close)])
            .await
            .map(|_| ())
    }

    pub(crate) async fn editor_quickfix_impl(
        &self,
        target: EditorTarget,
        files: &[PathBuf],
    ) -> Result<(), KeltaError> {
        let core = self.api()?;
        let info = self.editor_session(&core, &target).await?;
        let sock = self
            .editor_socket(&info)
            .await
            .ok_or_else(|| KeltaError::unsupported("the quickfix list needs an nvim (RPC) editor session"))?;
        let files: Vec<rmpv::Value> =
            files.iter().map(|f| rmpv::Value::from(info.cwd.join(f).to_string_lossy().as_ref())).collect();
        let mut c = NvimClient::connect(&sock).await?;
        c.exec_lua(
            crate::nvim::LUA_QUICKFIX,
            vec![rmpv::Value::from("Files Claude touched"), rmpv::Value::Array(files)],
        )
        .await
        .map(|_| ())
    }

    pub(crate) async fn send_selection_impl(
        &self,
        editor_session: &SessionId,
        claude_session: &SessionId,
    ) -> Result<(), KeltaError> {
        let core = self.api()?;
        let ed = core
            .session_get(editor_session)
            .ok_or_else(|| KeltaError::not_found(format!("session {editor_session}")))?;
        let cl = core
            .session_get(claude_session)
            .ok_or_else(|| KeltaError::not_found(format!("session {claude_session}")))?;
        let sock = self
            .editor_socket(&ed)
            .await
            .ok_or_else(|| KeltaError::unsupported("send selection needs an nvim (RPC) editor session"))?;
        let mut c = NvimClient::connect(&sock).await?;
        let sel = c.selection().await?;
        if sel.path.is_empty() {
            return Err(KeltaError::invalid("the editor buffer has no file"));
        }
        let text = selection_ref(Path::new(&sel.path), &cl.cwd, sel.l1, sel.l2);
        let bytes = if cl.kind == SessionKind::Claude { format!("\x1b[200~{text}\x1b[201~") } else { text };
        core.session_write(&cl.id, bytes.as_bytes()).await
    }

    /// Reaction to `claude.file_edited` (`editor.follow_claude_edits`).
    pub(crate) async fn follow_edit(&self, claude: &SessionId, path: &Path) -> Result<(), KeltaError> {
        let core = self.api()?;
        let Some(cl) = core.session_get(claude) else { return Ok(()) };
        let settings = core.settings(Some(&cl.project_id));
        let mode = settings.editor.follow_claude_edits;
        if mode == kelta_proto::settings::FollowEdits::Off {
            return Ok(());
        }
        let mut sockets: Vec<PathBuf> = Vec::new();
        if let Some(w) = self.for_session(claude).await {
            sockets.extend(w.nvim_socket);
        }
        if sockets.is_empty() {
            for s in core.session_list(Some(&cl.project_id)) {
                if matches!(s.kind, SessionKind::Editor { .. })
                    && s.lifecycle != Lifecycle::Exited
                    && let Some(sock) = s.editor.and_then(|e| e.socket)
                {
                    sockets.push(sock);
                }
            }
        }
        let file = if path.is_absolute() { path.to_path_buf() } else { cl.cwd.join(path) };
        for sock in sockets {
            let mut c = match NvimClient::connect(&sock).await {
                Ok(c) => c,
                Err(_) => continue,
            };
            if mode == kelta_proto::settings::FollowEdits::Open {
                let _ = c.edit(&file, None, false).await;
            }
            let _ = c.checktime().await;
        }
        Ok(())
    }

    pub(crate) async fn quit_hook_impl(&self) -> Result<(), KeltaError> {
        let core = self.api()?;
        let mut targets: Vec<(PathBuf, PathBuf)> = Vec::new();
        let items = self.store.list_items(None).await.unwrap_or_default();
        let sessions_dir = self.dirs.data.join("sessions");
        for s in core.session_list(None) {
            let SessionKind::Editor { adapter } = &s.kind else { continue };
            if s.lifecycle != Lifecycle::Live {
                continue;
            }
            let settings = core.settings(Some(&s.project_id));
            let Some(preset) = editor::preset(&settings.editor, Some(adapter)) else { continue };
            if preset.restore != EditorRestore::Mksession || preset.open != EditorOpenMode::Rpc {
                continue;
            }
            let item = items.iter().find(|w| w.session_ids.contains(&s.id));
            let sock = s
                .editor
                .as_ref()
                .and_then(|e| e.socket.clone())
                .or_else(|| item.and_then(|w| w.nvim_socket.clone()));
            let Some(sock) = sock else { continue };
            let file = item
                .and_then(|w| self.load_journal(&w.id).editor_session_file)
                .unwrap_or_else(|| sessions_dir.join(format!("{}.vim", s.id.as_str())));
            targets.push((sock, file));
        }
        if targets.is_empty() {
            return Ok(());
        }
        files::private_dir(&sessions_dir)?;
        let jobs = targets.into_iter().map(|(sock, file)| async move {
            let mut c = NvimClient::connect(&sock).await?;
            c.mksession(&file).await
        });
        // one-shot: quit must not hang on an unresponsive editor.
        let results = tokio::time::timeout(Duration::from_secs(3), futures::future::join_all(jobs))
            .await
            .unwrap_or_default();
        for r in results {
            if let Err(e) = r {
                tracing::warn!(error = %e.message, "nvim mksession on quit failed");
            }
        }
        Ok(())
    }

    pub(crate) async fn startup_impl(&self) -> Result<(), KeltaError> {
        let core = self.api()?;
        files::private_dir(&self.dirs.data)?;
        let ctl = self.dirs.stable_ctl();
        files::write_private(
            &self.dirs.data.join("lazygit-kelta.yml"),
            files::lazygit_config(&ctl).as_bytes(),
        )?;
        let items = self.store.list_items(None).await?;
        let mut repos: Vec<PathBuf> = Vec::new();
        for w in &items {
            if let Some(p) = core.project(&w.project_id)
                && let Some(r) = p.repos.iter().find(|r| r.id == w.repo_id)
                && r.path.is_dir()
                && !repos.contains(&r.path)
            {
                repos.push(r.path.clone());
            }
        }
        // Review refs of finished (or deleted) items go with the prune, so their objects can be collected.
        let keep: HashSet<String> =
            items.iter().filter(|w| w.state != WorkState::Finished).map(|w| w.id.to_string()).collect();
        for r in repos {
            if let Err(e) = git::worktree_prune(&r).await {
                tracing::warn!(repo = %r.display(), error = %e.message, "git worktree prune failed");
            }
            if let Err(e) = crate::review::prune_refs(&r, &keep).await {
                tracing::warn!(repo = %r.display(), error = %e.message, "review refs prune failed");
            }
        }
        // Sagas interrupted by a quit/crash become Failed so the UI offers Retry / Skip.
        for w in items.into_iter().filter(|w| matches!(w.state, WorkState::Starting | WorkState::Planned)) {
            let lock = self.item_lock(&w.id);
            let Ok(_guard) = lock.try_lock() else { continue };
            let mut item = self.load(&w.id).await?;
            let step = item
                .steps
                .iter()
                .find(|s| !matches!(s.status, StepStatus::Done | StepStatus::Skipped))
                .map(|s| s.step.clone())
                .unwrap_or_else(|| "persist".into());
            let msg = "interrupted (Kelta quit during start)".to_owned();
            self.set_step(&mut item, &step, StepStatus::Failed, Some(msg.clone())).await?;
            item.state = WorkState::Failed { step, message: msg };
            self.save(&mut item).await?;
        }
        Ok(())
    }

    /// Release the HTTP consumer of an exited Claude session.
    pub(crate) fn on_session_exited(&self, sid: &SessionId) {
        if self.http_sessions.lock().remove(sid)
            && let Some(h) = self.host()
        {
            h.release_http();
        }
    }
}

pub(crate) struct DirtyReport {
    pub files: Vec<String>,
    /// Untracked `worktree.include` copies (ours, removed with the worktree).
    pub copies: Vec<String>,
    pub unpushed: u32,
}

/// `@path#Lx-y ` (path relative to the Claude session's cwd when inside it).
pub fn selection_ref(file: &Path, cwd: &Path, l1: u32, l2: u32) -> String {
    let rel = match file.strip_prefix(cwd) {
        Ok(p) => p.to_path_buf(),
        Err(_) => {
            let (cf, cc) = (git::canon(file), git::canon(cwd));
            cf.strip_prefix(&cc).map(Path::to_path_buf).unwrap_or_else(|_| file.to_path_buf())
        }
    };
    let lines = if l1 >= l2 { format!("L{l1}") } else { format!("L{l1}-{l2}") };
    format!("@{}#{lines} ", rel.to_string_lossy())
}

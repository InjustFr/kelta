//! # kelta-work (L6)
//!
//! Work orchestration (SPEC §3.1-3.3): StartWorkPlan building, journaled start saga, git CLI ops,
//! worktrees, Claude launcher (argv, `--settings` hooks, `mcp.json`, `context.md`), editor adapters
//! (nvim msgpack-RPC, vim keys, emacsclient, helix, external), create PR, finish, review start.
//!
//! Wiring beyond the frozen constructor: core calls [`WorkService::set_host`] at boot with the lazy
//! HTTP server port (`kelta_server::Server::ensure_http`) and the blocking trigger runner
//! (`kelta_plugins::PluginHost::run_blocking`). Without a host (tests), MCP config is omitted and
//! blocking pre-events are only published.

pub mod claude;
pub mod editor;
pub mod files;
pub mod git;
pub mod layout;
pub mod nvim;
pub mod plan;
pub mod template;

mod fixloop;
mod listener;
mod ops;
mod rebase;
mod saga;
mod signals;
mod status;

pub use ops::selection_ref;
pub use plan::pr_title_with_key;

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Weak};

use async_trait::async_trait;
use kelta_proto::api::{CoreApi, WorkStore};
use kelta_proto::codehost::{Feedback, PrDraft, Review};
use kelta_proto::dirs::Dirs;
use kelta_proto::error::KeltaError;
use kelta_proto::events::{BusEvent, bus};
use kelta_proto::ext::BlockingOutcome;
use kelta_proto::ids::{ProjectId, SessionId, WorkItemId};
use kelta_proto::model::{
    EditorTarget, FinishMergedReport, FinishOpts, GitStatus, RebaseOp, SendFile, SessionInfo, ShipOrigin,
    StartWorkPlan, StepStatus, WORK_STEPS, WorkItem, WorkSource, WorkStepStatus,
};
use kelta_proto::tracker::TicketRef;
use parking_lot::{Mutex, RwLock};

/// Services owned by other lanes that the saga needs (wired by core after construction).
#[async_trait]
pub trait WorkHost: Send + Sync {
    /// `kelta_server::Server::ensure_http` (consumer refcount +1) → port.
    async fn ensure_http(&self) -> Result<u16, KeltaError>;
    /// `kelta_server::Server::release_http` (consumer refcount −1).
    fn release_http(&self);
    /// `kelta_plugins::PluginHost::run_blocking` for `*.before_*` events.
    async fn run_blocking(&self, ev: &BusEvent) -> Result<BlockingOutcome, KeltaError>;
}

pub struct WorkService {
    core: Weak<dyn CoreApi>,
    store: Arc<dyn WorkStore>,
    dirs: Dirs,
    me: Weak<WorkService>,
    host: RwLock<Option<Arc<dyn WorkHost>>>,
    /// `claude --version` per resolved binary path.
    versions: Mutex<HashMap<PathBuf, Option<semver::Version>>>,
    /// Serializes saga/finish/PR operations per work item.
    item_locks: Mutex<HashMap<WorkItemId, Arc<tokio::sync::Mutex<()>>>>,
    /// Items whose PR Claude is creating through MCP (names the holder of a busy item lock).
    shipping: Mutex<HashSet<WorkItemId>>,
    /// Serializes the existing-item check + insert of `start` (no twin items for one source).
    start_lock: tokio::sync::Mutex<()>,
    /// Serializes store writes; held only around load-modify-save (`save`, `update`).
    write_lock: tokio::sync::Mutex<()>,
    /// Last `git fetch` per repo (`work_status_all` floor).
    fetched: Mutex<HashMap<PathBuf, std::time::Instant>>,
    /// Worktree fingerprint at the last `UserPromptSubmit` per item: a `Stop` changed code iff it moved.
    prompt_marks: Mutex<HashMap<WorkItemId, u64>>,
    /// Bus listener (follow_claude_edits, HTTP consumer release); started on first need.
    listener: Mutex<Option<tokio::task::JoinHandle<()>>>,
    /// Claude sessions holding an HTTP server consumer.
    http_sessions: Mutex<HashSet<SessionId>>,
    /// Test hook: abort the saga right after this step is journaled as done (simulated crash).
    crash_after: Mutex<Option<String>>,
}

impl WorkService {
    pub fn new(core: Weak<dyn CoreApi>, store: Arc<dyn WorkStore>, dirs: Dirs) -> Arc<Self> {
        Arc::new_cyclic(|me| Self {
            core,
            store,
            dirs,
            me: me.clone(),
            host: RwLock::new(None),
            versions: Mutex::new(HashMap::new()),
            item_locks: Mutex::new(HashMap::new()),
            shipping: Mutex::new(HashSet::new()),
            start_lock: tokio::sync::Mutex::new(()),
            write_lock: tokio::sync::Mutex::new(()),
            fetched: Mutex::new(HashMap::new()),
            prompt_marks: Mutex::new(HashMap::new()),
            listener: Mutex::new(None),
            http_sessions: Mutex::new(HashSet::new()),
            crash_after: Mutex::new(None),
        })
    }

    pub fn core(&self) -> Option<Arc<dyn CoreApi>> {
        self.core.upgrade()
    }

    pub fn store(&self) -> &Arc<dyn WorkStore> {
        &self.store
    }

    pub fn dirs(&self) -> &Dirs {
        &self.dirs
    }

    /// Wire the HTTP server / trigger engine (core calls this after constructing every service).
    pub fn set_host(&self, host: Arc<dyn WorkHost>) {
        *self.host.write() = Some(host);
    }

    /// Test hook: make the next saga stop (as if Kelta crashed) right after `step` is journaled.
    #[doc(hidden)]
    pub fn set_crash_after(&self, step: Option<&str>) {
        *self.crash_after.lock() = step.map(str::to_owned);
    }

    /// `work_plan`.
    pub async fn plan(&self, project: &ProjectId, source: WorkSource) -> Result<StartWorkPlan, KeltaError> {
        self.build_plan(project, source).await
    }

    /// `work_start` (progress via `work.updated`).
    pub async fn start(&self, plan: StartWorkPlan) -> Result<WorkItem, KeltaError> {
        self.ensure_listener();
        let guard = self.start_lock.lock().await;
        // Re-check: another start for this source may have created its item since `plan`.
        let items = self.store.list_items(Some(&plan.project_id)).await?;
        // A scratch source's name may be empty: match on the branch the plan resolved.
        let source = match &plan.source {
            WorkSource::Branch { task, repo, .. } => {
                WorkSource::Branch { name: plan.branch.clone(), task: task.clone(), repo: repo.clone() }
            }
            other => other.clone(),
        };
        let found = plan::existing_for(&items, &source).map(|w| w.id.clone());
        if plan.existing.is_none()
            && found.is_some()
            && matches!(&plan.source, WorkSource::Branch { task: Some(_), .. })
        {
            return Err(KeltaError::conflict(format!(
                "Branch {} has a work item. Edit the branch name or the task's first line.",
                plan.branch
            )));
        }
        if let Some(existing) = plan.existing.clone().or(found) {
            drop(guard);
            return self.resume(&existing).await;
        }
        let id = self.create_item(plan).await?;
        // Hold the item lock before releasing `start_lock` so a racing start sees it busy.
        let lock = self.item_lock(&id);
        let _item = lock.lock().await;
        drop(guard);
        self.run_saga_locked(&id).await
    }

    /// `work_list`.
    pub async fn list(&self, project: Option<&ProjectId>) -> Result<Vec<WorkItem>, KeltaError> {
        let mut items = self.store.list_items(project).await?;
        for it in &mut items {
            it.steps = self.merged_steps(&it.id, &it.steps).await?;
        }
        items.sort_by(|a, b| b.created_at.cmp(&a.created_at));
        Ok(items)
    }

    /// `work_resume`.
    pub async fn resume(&self, id: &WorkItemId) -> Result<WorkItem, KeltaError> {
        self.ensure_listener();
        self.resume_item(id, None).await
    }

    /// `work_retry_step` (`step` = a saga step id; `skip:<step>` skips it instead).
    pub async fn retry_step(&self, id: &WorkItemId, step: &str) -> Result<WorkItem, KeltaError> {
        self.ensure_listener();
        self.retry(id, step).await
    }

    /// `work_create_pr` (UI Ship) and the MCP `create_pr` tool.
    pub async fn create_pr(
        &self,
        id: &WorkItemId,
        draft: PrDraft,
        origin: ShipOrigin,
    ) -> Result<WorkItem, KeltaError> {
        if origin == ShipOrigin::Mcp {
            self.shipping.lock().insert(id.clone());
        }
        let r = self.create_pr_impl(id, draft, origin).await;
        if origin == ShipOrigin::Mcp {
            self.shipping.lock().remove(id);
        }
        r
    }

    /// `work_pr_draft`: the title, body and draft flag Ship would use (prefills the dialog).
    pub async fn pr_draft(&self, id: &WorkItemId) -> Result<PrDraft, KeltaError> {
        self.pr_draft_impl(id).await
    }

    /// `work_link`: attach a ticket to a scratch item (optionally applying `on_start` / `on_pr`).
    pub async fn link(
        &self,
        id: &WorkItemId,
        ticket: TicketRef,
        apply_side_effects: bool,
    ) -> Result<WorkItem, KeltaError> {
        self.link_impl(id, ticket, apply_side_effects).await
    }

    /// `work_finish`.
    pub async fn finish(&self, id: &WorkItemId, opts: FinishOpts) -> Result<WorkItem, KeltaError> {
        self.finish_impl(id, opts).await
    }

    /// `work_finish_merged`: finish the listed items that are still merged, clean and need no
    /// Done choice; the rest are skipped with a reason.
    pub async fn finish_merged(&self, ids: &[WorkItemId]) -> Result<FinishMergedReport, KeltaError> {
        self.finish_merged_impl(ids).await
    }

    /// Branch join (FLOW §3.1): an open PR found for this item's branch becomes its PR
    /// (`pr_url`, `PrOpen`, `work.on_pr`), once.
    pub async fn link_pr(&self, id: &WorkItemId, review: &Review) -> Result<(), KeltaError> {
        self.link_pr_impl(id, review).await
    }

    /// `work_send`: brief files into the private run dir, then `prompt` into the item's previous
    /// Claude conversation; `threads` (review thread ids handed over) are remembered on success.
    pub async fn send(
        &self,
        id: &WorkItemId,
        prompt: &str,
        files: Vec<SendFile>,
        threads: Option<Vec<String>>,
    ) -> Result<WorkItem, KeltaError> {
        self.ensure_listener();
        self.send_impl(id, prompt, files, threads).await
    }

    /// `work_feedback`: unresolved threads, review summaries and failed checks of the item's PR.
    pub async fn feedback(&self, id: &WorkItemId) -> Result<Feedback, KeltaError> {
        self.feedback_impl(id).await
    }

    /// `work_rerequest_review` → the logins asked again.
    pub async fn rerequest_review(&self, id: &WorkItemId) -> Result<Vec<String>, KeltaError> {
        self.rerequest_impl(id).await
    }

    /// `work_resolve_sent_threads`: resolve the threads the last Fix with Claude handed over.
    pub async fn resolve_sent_threads(&self, id: &WorkItemId) -> Result<WorkItem, KeltaError> {
        self.resolve_sent_impl(id).await
    }

    /// `work_rebase`.
    pub async fn rebase(&self, id: &WorkItemId, op: RebaseOp) -> Result<WorkItem, KeltaError> {
        self.ensure_listener();
        self.rebase_impl(id, op).await
    }

    /// `work_push` (`force` only over an own rewrite, FLOW §4.4 step 5).
    pub async fn push(&self, id: &WorkItemId, force: bool) -> Result<WorkItem, KeltaError> {
        self.push_impl(id, force).await
    }

    /// `work_status` (on demand).
    pub async fn status(&self, id: &WorkItemId) -> Result<GitStatus, KeltaError> {
        self.status_impl(id).await
    }

    /// `work_status_all`: fresh git status of every unfinished item (one fetch per repo, 5 min floor).
    pub async fn status_all(&self) -> Result<BTreeMap<WorkItemId, GitStatus>, KeltaError> {
        self.status_all_impl().await
    }

    /// `work_diff`: spawns the review diff session (editor with `editor.review_args`, else a shell).
    pub async fn diff(&self, id: &WorkItemId) -> Result<SessionInfo, KeltaError> {
        self.diff_impl(id).await
    }

    /// Work item owning a session (for `CoreApi::work_for_session`).
    pub async fn for_session(&self, id: &SessionId) -> Option<WorkItem> {
        let items = self.store.list_items(None).await.ok()?;
        let mut item = items.into_iter().find(|w| w.session_ids.contains(id))?;
        if let Ok(steps) = self.merged_steps(&item.id, &item.steps).await {
            item.steps = steps;
        }
        Some(item)
    }

    /// `editor_open`.
    pub async fn editor_open(
        &self,
        target: EditorTarget,
        path: &Path,
        line: Option<u32>,
    ) -> Result<(), KeltaError> {
        self.editor_open_impl(target, path, line).await
    }

    /// `editor_diff` (Claude IDE bridge `openDiff`).
    pub async fn editor_diff(
        &self,
        target: EditorTarget,
        old: &Path,
        proposed: &Path,
        close: bool,
    ) -> Result<(), KeltaError> {
        self.editor_diff_impl(target, old, proposed, close).await
    }

    /// `editor_send_selection`: `@path#Lx-y` into the Claude session.
    pub async fn send_selection(
        &self,
        editor_session: &SessionId,
        claude_session: &SessionId,
    ) -> Result<(), KeltaError> {
        self.send_selection_impl(editor_session, claude_session).await
    }

    /// App quit: nvim `:wall | mksession!` for editor sessions.
    pub async fn quit_hook(&self) -> Result<(), KeltaError> {
        self.quit_hook_impl().await
    }

    /// App start: `git worktree prune`, generate `<data>/lazygit-kelta.yml`.
    pub async fn startup(&self) -> Result<(), KeltaError> {
        self.ensure_listener();
        self.startup_impl().await
    }

    // ---- shared internals -----------------------------------------------------------------

    pub(crate) fn api(&self) -> Result<Arc<dyn CoreApi>, KeltaError> {
        self.core.upgrade().ok_or_else(|| KeltaError::internal("core is shutting down"))
    }

    pub(crate) fn host(&self) -> Option<Arc<dyn WorkHost>> {
        self.host.read().clone()
    }

    pub(crate) fn item_lock(&self, id: &WorkItemId) -> Arc<tokio::sync::Mutex<()>> {
        self.item_locks.lock().entry(id.clone()).or_default().clone()
    }

    /// Conflict for a held item lock, naming Claude's MCP ship when that is the holder.
    pub(crate) fn busy(&self, id: &WorkItemId) -> KeltaError {
        if self.shipping.lock().contains(id) {
            KeltaError::conflict("Claude is shipping this item.")
        } else {
            KeltaError::conflict("work item is busy")
        }
    }

    /// Steps from the store merged into saga order (missing → pending).
    pub(crate) async fn merged_steps(
        &self,
        id: &WorkItemId,
        fallback: &[WorkStepStatus],
    ) -> Result<Vec<WorkStepStatus>, KeltaError> {
        let stored = self.store.steps(id).await?;
        let pick = |name: &str| {
            stored
                .iter()
                .find(|s| s.step == name)
                .or_else(|| fallback.iter().find(|s| s.step == name))
                .cloned()
                .unwrap_or_else(|| WorkStepStatus {
                    step: name.to_owned(),
                    status: StepStatus::Pending,
                    detail: None,
                    updated_at: kelta_proto::now_rfc3339(),
                })
        };
        Ok(WORK_STEPS.iter().map(|s| pick(s)).collect())
    }

    pub(crate) async fn load(&self, id: &WorkItemId) -> Result<WorkItem, KeltaError> {
        let mut item =
            self.store.get_item(id).await?.ok_or_else(|| KeltaError::not_found(format!("work item {id}")))?;
        item.steps = self.merged_steps(id, &item.steps).await?;
        Ok(item)
    }

    /// Persist + publish `work.updated`. The hook-owned flags (`review_due`, `claude_replied`) are
    /// never written here: `item` takes the stored ones, so a long operation's final save cannot
    /// undo a hook that arrived while it ran (FLOW §2.3). Only [`Self::update`] writes them.
    pub(crate) async fn save(&self, item: &mut WorkItem) -> Result<(), KeltaError> {
        {
            let _w = self.write_lock.lock().await;
            if let Some(cur) = self.store.get_item(&item.id).await? {
                item.review_due = cur.review_due;
                item.claude_replied = cur.claude_replied;
            }
            self.store.put_item(item).await?;
        }
        self.publish_updated(item);
        Ok(())
    }

    /// Field-level write: re-load under the write lock, apply `f`, save when it returns true, publish.
    pub(crate) async fn update(
        &self,
        id: &WorkItemId,
        f: impl FnOnce(&mut WorkItem) -> bool,
    ) -> Result<WorkItem, KeltaError> {
        let (mut item, changed) = {
            let _w = self.write_lock.lock().await;
            let mut item = self
                .store
                .get_item(id)
                .await?
                .ok_or_else(|| KeltaError::not_found(format!("work item {id}")))?;
            let changed = f(&mut item);
            if changed {
                self.store.put_item(&item).await?;
            }
            (item, changed)
        };
        item.steps = self.merged_steps(id, &item.steps).await?;
        if changed {
            self.publish_updated(&item);
        }
        Ok(item)
    }

    fn publish_updated(&self, item: &WorkItem) {
        if let Ok(core) = self.api() {
            core.publish(
                BusEvent::new(bus::WORK_UPDATED, serde_json::json!({ "work": item }))
                    .with_project(item.project_id.clone())
                    .with_work_item(item.id.clone()),
            );
        }
    }

    pub(crate) async fn set_step(
        &self,
        item: &mut WorkItem,
        step: &str,
        status: StepStatus,
        detail: Option<String>,
    ) -> Result<(), KeltaError> {
        self.store.set_step(&item.id, step, status, detail.clone()).await?;
        let now = kelta_proto::now_rfc3339();
        match item.steps.iter_mut().find(|s| s.step == step) {
            Some(s) => {
                s.status = status;
                s.detail = detail;
                s.updated_at = now;
            }
            None => {
                item.steps.push(WorkStepStatus { step: step.to_owned(), status, detail, updated_at: now })
            }
        }
        self.save(item).await
    }

    /// Starts the bus listener (`pr.merged`, hooks, …) if it is not running.
    pub fn ensure_listener(&self) {
        let mut l = self.listener.lock();
        if l.as_ref().is_some_and(|h| !h.is_finished()) {
            return;
        }
        let Ok(core) = self.api() else { return };
        let Ok(rt) = tokio::runtime::Handle::try_current() else { return };
        let rx = core.subscribe();
        *l = Some(rt.spawn(listener::run(self.me.clone(), rx)));
    }
}

impl Drop for WorkService {
    fn drop(&mut self) {
        if let Some(h) = self.listener.lock().take() {
            h.abort();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kelta_proto::testing::{FakeCore, MemWorkStore};

    #[tokio::test]
    async fn empty_list_and_missing_item() {
        let core = FakeCore::new();
        let weak: Weak<dyn CoreApi> = Arc::downgrade(&(core.clone() as Arc<dyn CoreApi>));
        let w = WorkService::new(weak, Arc::new(MemWorkStore::new()), Dirs::under(&std::env::temp_dir()));
        assert!(w.core().is_some());
        assert!(w.list(None).await.unwrap().is_empty());
        let e = w.resume(&WorkItemId::new("nope")).await.unwrap_err();
        assert_eq!(e.code, kelta_proto::ErrorCode::NotFound);
    }
}

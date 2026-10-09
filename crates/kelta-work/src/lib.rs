//! # kelta-work (L6)
//!
//! Work orchestration (SPEC §3.1-3.3): StartWorkPlan building, journaled start saga, git CLI ops,
//! worktrees, Claude launcher (argv, `--settings` hooks, `mcp.json`, `context.md`), editor adapters
//! (nvim msgpack-RPC, vim keys, emacsclient, helix, external), create PR, finish, review start.
//!
//! Wiring beyond the frozen constructor: [`WorkService::set_host`] gives the service the lazy HTTP
//! server port (`kelta_server::Server::ensure_http`) and the blocking trigger runner
//! (`kelta_plugins::PluginHost::run_blocking`). Without a host, MCP config is omitted and blocking
//! pre-events are only published (see `docs/contract-requests/L6.md`).

pub mod claude;
pub mod editor;
pub mod files;
pub mod git;
pub mod layout;
pub mod nvim;
pub mod plan;
pub mod template;

mod listener;
mod ops;
mod saga;

pub use ops::selection_ref;

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Weak};

use async_trait::async_trait;
use kelta_proto::api::{CoreApi, WorkStore};
use kelta_proto::codehost::PrDraft;
use kelta_proto::dirs::Dirs;
use kelta_proto::error::KeltaError;
use kelta_proto::events::BusEvent;
use kelta_proto::ext::BlockingOutcome;
use kelta_proto::ids::{ProjectId, SessionId, WorkItemId};
use kelta_proto::model::{
    EditorTarget, FinishOpts, GitStatus, StartWorkPlan, StepStatus, WORK_STEPS, WorkItem, WorkSource,
    WorkStepStatus,
};
use parking_lot::{Mutex, RwLock};

/// Bus event carrying a work item update; core relays it to the UI as `UiEvent::WorkUpdated`
/// (`CoreApi` has no direct UI emit — see the L6 contract request).
pub const WORK_UPDATED: &str = "work.updated";

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
    /// Serializes the existing-item check + insert of `start` (no twin items for one source).
    start_lock: tokio::sync::Mutex<()>,
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
            start_lock: tokio::sync::Mutex::new(()),
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
        let existing =
            plan.existing.clone().or_else(|| plan::existing_for(&items, &plan.source).map(|w| w.id.clone()));
        if let Some(existing) = existing {
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
        self.resume_item(id).await
    }

    /// `work_retry_step` (`step` = a saga step id; `skip:<step>` skips it instead).
    pub async fn retry_step(&self, id: &WorkItemId, step: &str) -> Result<WorkItem, KeltaError> {
        self.ensure_listener();
        self.retry(id, step).await
    }

    /// `work_create_pr`.
    pub async fn create_pr(&self, id: &WorkItemId, draft: PrDraft) -> Result<WorkItem, KeltaError> {
        self.create_pr_impl(id, draft).await
    }

    /// `work_finish`.
    pub async fn finish(&self, id: &WorkItemId, opts: FinishOpts) -> Result<WorkItem, KeltaError> {
        self.finish_impl(id, opts).await
    }

    /// `work_status` (on demand).
    pub async fn status(&self, id: &WorkItemId) -> Result<GitStatus, KeltaError> {
        self.status_impl(id).await
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

    /// Persist + publish `work.updated`.
    pub(crate) async fn save(&self, item: &WorkItem) -> Result<(), KeltaError> {
        self.store.put_item(item).await?;
        if let Ok(core) = self.api() {
            core.publish(
                BusEvent::new(WORK_UPDATED, serde_json::json!({ "work": item }))
                    .with_project(item.project_id.clone())
                    .with_work_item(item.id.clone()),
            );
        }
        Ok(())
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

    pub(crate) fn ensure_listener(&self) {
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

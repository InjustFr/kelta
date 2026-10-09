//! # kelta-work (L6)
//!
//! Work orchestration (SPEC §3.1-3.3): StartWorkPlan building, journaled start saga, git CLI ops,
//! worktrees, Claude launcher (argv, `--settings` hooks, `mcp.json`, `context.md`), editor adapters
//! (nvim msgpack-RPC, vim keys, emacsclient, helix, external), create PR, finish, review start.
//!
//! SCAFFOLD STUB: every fallible method returns `Unsupported("not implemented: <fn>")`.

use std::path::Path;
use std::sync::{Arc, Weak};

use kelta_proto::api::{CoreApi, WorkStore};
use kelta_proto::codehost::PrDraft;
use kelta_proto::dirs::Dirs;
use kelta_proto::error::KeltaError;
use kelta_proto::ids::{ProjectId, SessionId, WorkItemId};
use kelta_proto::model::{EditorTarget, FinishOpts, GitStatus, StartWorkPlan, WorkItem, WorkSource};

pub struct WorkService {
    core: Weak<dyn CoreApi>,
    store: Arc<dyn WorkStore>,
    dirs: Dirs,
}

impl WorkService {
    pub fn new(core: Weak<dyn CoreApi>, store: Arc<dyn WorkStore>, dirs: Dirs) -> Arc<Self> {
        Arc::new(Self { core, store, dirs })
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

    /// `work_plan`.
    pub async fn plan(&self, _project: &ProjectId, _source: WorkSource) -> Result<StartWorkPlan, KeltaError> {
        Err(KeltaError::not_implemented("WorkService::plan"))
    }

    /// `work_start` (progress via `work.updated`).
    pub async fn start(&self, _plan: StartWorkPlan) -> Result<WorkItem, KeltaError> {
        Err(KeltaError::not_implemented("WorkService::start"))
    }

    /// `work_list`.
    pub async fn list(&self, _project: Option<&ProjectId>) -> Result<Vec<WorkItem>, KeltaError> {
        Err(KeltaError::not_implemented("WorkService::list"))
    }

    /// `work_resume`.
    pub async fn resume(&self, _id: &WorkItemId) -> Result<WorkItem, KeltaError> {
        Err(KeltaError::not_implemented("WorkService::resume"))
    }

    /// `work_retry_step`.
    pub async fn retry_step(&self, _id: &WorkItemId, _step: &str) -> Result<WorkItem, KeltaError> {
        Err(KeltaError::not_implemented("WorkService::retry_step"))
    }

    /// `work_create_pr`.
    pub async fn create_pr(&self, _id: &WorkItemId, _draft: PrDraft) -> Result<WorkItem, KeltaError> {
        Err(KeltaError::not_implemented("WorkService::create_pr"))
    }

    /// `work_finish`.
    pub async fn finish(&self, _id: &WorkItemId, _opts: FinishOpts) -> Result<WorkItem, KeltaError> {
        Err(KeltaError::not_implemented("WorkService::finish"))
    }

    /// `work_status` (on demand).
    pub async fn status(&self, _id: &WorkItemId) -> Result<GitStatus, KeltaError> {
        Err(KeltaError::not_implemented("WorkService::status"))
    }

    /// Work item owning a session (for `CoreApi::work_for_session`).
    pub async fn for_session(&self, _id: &SessionId) -> Option<WorkItem> {
        None
    }

    /// `editor_open`.
    pub async fn editor_open(
        &self,
        _target: EditorTarget,
        _path: &Path,
        _line: Option<u32>,
    ) -> Result<(), KeltaError> {
        Err(KeltaError::not_implemented("WorkService::editor_open"))
    }

    /// `editor_send_selection`: `@path#Lx-y` into the Claude session.
    pub async fn send_selection(
        &self,
        _editor_session: &SessionId,
        _claude_session: &SessionId,
    ) -> Result<(), KeltaError> {
        Err(KeltaError::not_implemented("WorkService::send_selection"))
    }

    /// App quit: nvim `:wall | mksession!` for editor sessions.
    pub async fn quit_hook(&self) -> Result<(), KeltaError> {
        Ok(())
    }

    /// App start: `git worktree prune`, generate `<data>/lazygit-kelta.yml`.
    pub async fn startup(&self) -> Result<(), KeltaError> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kelta_proto::testing::{FakeCore, MemWorkStore};

    #[tokio::test]
    async fn stub_is_unsupported() {
        let core = FakeCore::new();
        let weak: Weak<dyn CoreApi> = Arc::downgrade(&(core.clone() as Arc<dyn CoreApi>));
        let w = WorkService::new(weak, Arc::new(MemWorkStore::new()), Dirs::under(&std::env::temp_dir()));
        assert!(w.core().is_some());
        let e = w.list(None).await.unwrap_err();
        assert_eq!(e.code, kelta_proto::ErrorCode::Unsupported);
    }
}

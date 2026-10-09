//! `work` commands — work items (start work saga, PRs, finish) (owner: L6, ARCHITECTURE §6).
//!
//! Thin delegation to `kelta_work::WorkService` (through `Core::work()`).

use std::sync::Arc;

use kelta_core::Core;
use kelta_proto::prelude::*;
use tauri::State;

use super::Res;

#[tauri::command(rename_all = "snake_case")]
pub async fn work_plan(
    core: State<'_, Arc<Core>>,
    project_id: ProjectId,
    source: WorkSource,
) -> Res<StartWorkPlan> {
    core.work().plan(&project_id, source).await
}

#[tauri::command(rename_all = "snake_case")]
pub async fn work_start(core: State<'_, Arc<Core>>, plan: StartWorkPlan) -> Res<WorkItem> {
    core.work().start(plan).await
}

#[tauri::command(rename_all = "snake_case")]
pub async fn work_list(core: State<'_, Arc<Core>>, project_id: Option<ProjectId>) -> Res<Vec<WorkItem>> {
    core.work().list(project_id.as_ref()).await
}

#[tauri::command(rename_all = "snake_case")]
pub async fn work_resume(core: State<'_, Arc<Core>>, id: WorkItemId) -> Res<WorkItem> {
    core.work().resume(&id).await
}

/// `step` is a saga step id; `skip:<step>` marks it skipped and continues the saga.
#[tauri::command(rename_all = "snake_case")]
pub async fn work_retry_step(core: State<'_, Arc<Core>>, id: WorkItemId, step: String) -> Res<WorkItem> {
    core.work().retry_step(&id, &step).await
}

#[tauri::command(rename_all = "snake_case")]
pub async fn work_create_pr(core: State<'_, Arc<Core>>, id: WorkItemId, draft: PrDraft) -> Res<WorkItem> {
    core.work().create_pr(&id, draft).await
}

#[tauri::command(rename_all = "snake_case")]
pub async fn work_finish(core: State<'_, Arc<Core>>, id: WorkItemId, opts: FinishOpts) -> Res<WorkItem> {
    core.work().finish(&id, opts).await
}

#[tauri::command(rename_all = "snake_case")]
pub async fn work_status(core: State<'_, Arc<Core>>, id: WorkItemId) -> Res<GitStatus> {
    core.work().status(&id).await
}

//! `work` commands — work items (start work saga, PRs, finish) (owner: L6, ARCHITECTURE §6).
//!
//! SCAFFOLD STUBS: return `Unsupported("not implemented: <command>")`. Keep the signatures (argument
//! names are the snake_case keys the UI sends); replace the bodies.
#![allow(unused_variables, unused_imports)]

use std::path::PathBuf;
use std::sync::Arc;

use kelta_core::Core;
use kelta_proto::prelude::*;
use serde_json::Value;
use tauri::State;
use tauri::ipc::{Channel, InvokeResponseBody};

use super::{Res, not_implemented};

#[tauri::command(rename_all = "snake_case")]
pub async fn work_plan(
    core: State<'_, Arc<Core>>,
    project_id: ProjectId,
    source: WorkSource,
) -> Res<StartWorkPlan> {
    not_implemented("work_plan")
}

#[tauri::command(rename_all = "snake_case")]
pub async fn work_start(core: State<'_, Arc<Core>>, plan: StartWorkPlan) -> Res<WorkItem> {
    not_implemented("work_start")
}

#[tauri::command(rename_all = "snake_case")]
pub async fn work_list(core: State<'_, Arc<Core>>, project_id: Option<ProjectId>) -> Res<Vec<WorkItem>> {
    not_implemented("work_list")
}

#[tauri::command(rename_all = "snake_case")]
pub async fn work_resume(core: State<'_, Arc<Core>>, id: WorkItemId) -> Res<WorkItem> {
    not_implemented("work_resume")
}

#[tauri::command(rename_all = "snake_case")]
pub async fn work_retry_step(core: State<'_, Arc<Core>>, id: WorkItemId, step: String) -> Res<WorkItem> {
    not_implemented("work_retry_step")
}

#[tauri::command(rename_all = "snake_case")]
pub async fn work_create_pr(core: State<'_, Arc<Core>>, id: WorkItemId, draft: PrDraft) -> Res<WorkItem> {
    not_implemented("work_create_pr")
}

#[tauri::command(rename_all = "snake_case")]
pub async fn work_finish(core: State<'_, Arc<Core>>, id: WorkItemId, opts: FinishOpts) -> Res<WorkItem> {
    not_implemented("work_finish")
}

#[tauri::command(rename_all = "snake_case")]
pub async fn work_status(core: State<'_, Arc<Core>>, id: WorkItemId) -> Res<GitStatus> {
    not_implemented("work_status")
}

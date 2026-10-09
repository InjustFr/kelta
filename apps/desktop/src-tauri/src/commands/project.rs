//! `project` commands — projects (owner: L3, ARCHITECTURE §6). Closing or switching a project
//! never touches sessions unless `kill_sessions` is set.

use std::path::PathBuf;
use std::sync::Arc;

use kelta_core::Core;
use kelta_proto::prelude::*;
use tauri::State;

use super::Res;

#[tauri::command(rename_all = "snake_case")]
pub async fn project_list(core: State<'_, Arc<Core>>) -> Res<Vec<ProjectInfo>> {
    Ok(core.project_list())
}

#[tauri::command(rename_all = "snake_case")]
pub async fn project_detect(core: State<'_, Arc<Core>>, path: PathBuf) -> Res<ProjectDraft> {
    core.project_detect(&path)
}

#[tauri::command(rename_all = "snake_case")]
pub async fn project_create(core: State<'_, Arc<Core>>, draft: ProjectDraft) -> Res<ProjectInfo> {
    core.project_create(&draft)
}

#[tauri::command(rename_all = "snake_case")]
pub async fn project_update(
    core: State<'_, Arc<Core>>,
    id: ProjectId,
    patch: ProjectPatch,
) -> Res<ProjectInfo> {
    core.project_update(&id, &patch)
}

#[tauri::command(rename_all = "snake_case")]
pub async fn project_remove(core: State<'_, Arc<Core>>, id: ProjectId, kill_sessions: bool) -> Res<()> {
    core.project_remove(&id, kill_sessions)
}

#[tauri::command(rename_all = "snake_case")]
pub async fn project_open(core: State<'_, Arc<Core>>, id: ProjectId) -> Res<ProjectInfo> {
    core.project_open(&id)
}

#[tauri::command(rename_all = "snake_case")]
pub async fn project_close(
    core: State<'_, Arc<Core>>,
    id: ProjectId,
    kill_sessions: bool,
) -> Res<ProjectInfo> {
    core.project_close(&id, kill_sessions)
}

#[tauri::command(rename_all = "snake_case")]
pub async fn project_activate(core: State<'_, Arc<Core>>, id: ProjectId) -> Res<ProjectInfo> {
    core.project_activate(&id)
}

#[tauri::command(rename_all = "snake_case")]
pub async fn project_reorder(core: State<'_, Arc<Core>>, ids: Vec<ProjectId>) -> Res<()> {
    core.project_reorder(&ids)
}

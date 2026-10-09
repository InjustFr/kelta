//! `project` commands — projects (owner: L3, ARCHITECTURE §6).
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
pub async fn project_list(core: State<'_, Arc<Core>>) -> Res<Vec<ProjectInfo>> {
    not_implemented("project_list")
}

#[tauri::command(rename_all = "snake_case")]
pub async fn project_detect(core: State<'_, Arc<Core>>, path: PathBuf) -> Res<ProjectDraft> {
    not_implemented("project_detect")
}

#[tauri::command(rename_all = "snake_case")]
pub async fn project_create(core: State<'_, Arc<Core>>, draft: ProjectDraft) -> Res<ProjectInfo> {
    not_implemented("project_create")
}

#[tauri::command(rename_all = "snake_case")]
pub async fn project_update(
    core: State<'_, Arc<Core>>,
    id: ProjectId,
    patch: ProjectPatch,
) -> Res<ProjectInfo> {
    not_implemented("project_update")
}

#[tauri::command(rename_all = "snake_case")]
pub async fn project_remove(core: State<'_, Arc<Core>>, id: ProjectId, kill_sessions: bool) -> Res<()> {
    not_implemented("project_remove")
}

#[tauri::command(rename_all = "snake_case")]
pub async fn project_open(core: State<'_, Arc<Core>>, id: ProjectId) -> Res<ProjectInfo> {
    not_implemented("project_open")
}

#[tauri::command(rename_all = "snake_case")]
pub async fn project_close(
    core: State<'_, Arc<Core>>,
    id: ProjectId,
    kill_sessions: bool,
) -> Res<ProjectInfo> {
    not_implemented("project_close")
}

#[tauri::command(rename_all = "snake_case")]
pub async fn project_activate(core: State<'_, Arc<Core>>, id: ProjectId) -> Res<ProjectInfo> {
    not_implemented("project_activate")
}

#[tauri::command(rename_all = "snake_case")]
pub async fn project_reorder(core: State<'_, Arc<Core>>, ids: Vec<ProjectId>) -> Res<()> {
    not_implemented("project_reorder")
}

//! `settings` commands — settings layers, validation, repo trust, account test (owner: L4, ARCHITECTURE §6).
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
pub async fn settings_schema(core: State<'_, Arc<Core>>) -> Res<Value> {
    not_implemented("settings_schema")
}

#[tauri::command(rename_all = "snake_case")]
pub async fn settings_effective(
    core: State<'_, Arc<Core>>,
    project_id: Option<ProjectId>,
) -> Res<EffectiveSettings> {
    not_implemented("settings_effective")
}

#[tauri::command(rename_all = "snake_case")]
pub async fn settings_layer_get(
    core: State<'_, Arc<Core>>,
    layer: Layer,
    project_id: Option<ProjectId>,
    repo_id: Option<String>,
) -> Res<LayerDoc> {
    not_implemented("settings_layer_get")
}

#[tauri::command(rename_all = "snake_case")]
pub async fn settings_set(
    core: State<'_, Arc<Core>>,
    layer: Layer,
    project_id: Option<ProjectId>,
    repo_id: Option<String>,
    path: String,
    value: Value,
) -> Res<EffectiveSettings> {
    not_implemented("settings_set")
}

#[tauri::command(rename_all = "snake_case")]
pub async fn settings_reset(
    core: State<'_, Arc<Core>>,
    layer: Layer,
    project_id: Option<ProjectId>,
    repo_id: Option<String>,
    path: String,
) -> Res<EffectiveSettings> {
    not_implemented("settings_reset")
}

#[tauri::command(rename_all = "snake_case")]
pub async fn settings_validate(
    core: State<'_, Arc<Core>>,
    layer: Layer,
    text: String,
) -> Res<Vec<ValidationIssue>> {
    not_implemented("settings_validate")
}

#[tauri::command(rename_all = "snake_case")]
pub async fn settings_write_raw(
    core: State<'_, Arc<Core>>,
    layer: Layer,
    project_id: Option<ProjectId>,
    repo_id: Option<String>,
    text: String,
) -> Res<EffectiveSettings> {
    not_implemented("settings_write_raw")
}

#[tauri::command(rename_all = "snake_case")]
pub async fn settings_open_file(
    core: State<'_, Arc<Core>>,
    layer: Layer,
    project_id: Option<ProjectId>,
    repo_id: Option<String>,
) -> Res<SessionInfo> {
    not_implemented("settings_open_file")
}

#[tauri::command(rename_all = "snake_case")]
pub async fn repo_trust(
    core: State<'_, Arc<Core>>,
    project_id: ProjectId,
    repo_id: String,
    trust: bool,
) -> Res<TrustInfo> {
    not_implemented("repo_trust")
}

#[tauri::command(rename_all = "snake_case")]
pub async fn account_test(core: State<'_, Arc<Core>>, account_id: AccountId) -> Res<AccountTestResult> {
    not_implemented("account_test")
}

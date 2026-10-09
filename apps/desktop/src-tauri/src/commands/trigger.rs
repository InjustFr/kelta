//! `trigger` commands — triggers (owner: L8, ARCHITECTURE §6).

use std::sync::Arc;

use kelta_core::Core;
use kelta_proto::prelude::*;
use serde_json::Value;
use tauri::State;

use super::Res;
use super::plugin::plugin_host;

#[tauri::command(rename_all = "snake_case")]
pub async fn trigger_list(
    core: State<'_, Arc<Core>>,
    project_id: Option<ProjectId>,
) -> Res<Vec<TriggerInfo>> {
    plugin_host(&core).triggers(project_id.as_ref()).await
}

#[tauri::command(rename_all = "snake_case")]
pub async fn trigger_test(core: State<'_, Arc<Core>>, trigger_id: String, payload: Value) -> Res<TriggerRun> {
    plugin_host(&core).trigger_test(&trigger_id, payload).await
}

#[tauri::command(rename_all = "snake_case")]
pub async fn trigger_log(core: State<'_, Arc<Core>>, limit: u32) -> Res<Vec<TriggerRun>> {
    plugin_host(&core).trigger_log(limit).await
}

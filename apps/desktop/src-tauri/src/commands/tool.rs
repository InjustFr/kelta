//! `tool` commands — tools (owner: L8, ARCHITECTURE §6).

use std::sync::Arc;

use kelta_core::Core;
use kelta_proto::prelude::*;
use tauri::State;

use super::Res;

#[tauri::command(rename_all = "snake_case")]
pub async fn tool_list(core: State<'_, Arc<Core>>, project_id: ProjectId) -> Res<Vec<ToolInfo>> {
    core.plugins().tools(&project_id).await
}

#[tauri::command(rename_all = "snake_case")]
pub async fn tool_check(core: State<'_, Arc<Core>>, tool_id: ToolId) -> Res<ToolCheck> {
    core.plugins().tool_check(&tool_id).await
}

#[tauri::command(rename_all = "snake_case")]
pub async fn tool_open(
    core: State<'_, Arc<Core>>,
    project_id: ProjectId,
    tool_id: ToolId,
    ctx: TemplateCtx,
    placement: Placement,
) -> Res<ToolHandle> {
    core.plugins().tool_open(&project_id, &tool_id, ctx, placement).await
}

#[tauri::command(rename_all = "snake_case")]
pub async fn tool_close(core: State<'_, Arc<Core>>, instance_id: ToolInstanceId) -> Res<()> {
    core.plugins().tool_close(&instance_id).await
}

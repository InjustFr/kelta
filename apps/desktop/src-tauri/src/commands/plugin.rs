//! `plugin` commands — plugins, screens, plugin_call, contributed commands (owner: L8, ARCHITECTURE §6).

use std::sync::Arc;

use kelta_core::Core;
use kelta_proto::prelude::*;
use serde_json::Value;
use tauri::State;

use super::Res;

#[tauri::command(rename_all = "snake_case")]
pub async fn plugin_list(core: State<'_, Arc<Core>>) -> Res<Vec<PluginInfo>> {
    core.plugins().plugins().await
}

#[tauri::command(rename_all = "snake_case")]
pub async fn plugin_inspect(core: State<'_, Arc<Core>>, source: String) -> Res<PluginInstallPreview> {
    core.plugins().inspect(&source).await
}

#[tauri::command(rename_all = "snake_case")]
pub async fn plugin_install(
    core: State<'_, Arc<Core>>,
    source: String,
    sha256: String,
    grant: Vec<String>,
) -> Res<PluginInfo> {
    let info = core.plugins().install(&source, &sha256, grant).await?;
    core.sync_plugin_schemas();
    Ok(info)
}

#[tauri::command(rename_all = "snake_case")]
pub async fn plugin_uninstall(core: State<'_, Arc<Core>>, id: PluginId) -> Res<()> {
    core.plugins().uninstall(&id).await?;
    core.sync_plugin_schemas();
    Ok(())
}

#[tauri::command(rename_all = "snake_case")]
pub async fn plugin_enable(core: State<'_, Arc<Core>>, id: PluginId, enabled: bool) -> Res<()> {
    core.plugins().enable(&id, enabled).await?;
    core.sync_plugin_schemas();
    Ok(())
}

#[tauri::command(rename_all = "snake_case")]
pub async fn plugin_grant(
    core: State<'_, Arc<Core>>,
    id: PluginId,
    permissions: Vec<String>,
) -> Res<PluginInfo> {
    core.plugins().grant(&id, permissions).await
}

#[tauri::command(rename_all = "snake_case")]
pub async fn plugin_screen_open(
    core: State<'_, Arc<Core>>,
    plugin_id: PluginId,
    screen_id: String,
    project_id: Option<ProjectId>,
    params: Value,
) -> Res<ScreenOpenResult> {
    core.plugins().screen_open(&plugin_id, &screen_id, project_id.as_ref(), params).await
}

#[tauri::command(rename_all = "snake_case")]
pub async fn plugin_screen_close(core: State<'_, Arc<Core>>, instance_id: ScreenInstanceId) -> Res<()> {
    core.plugins().screen_close(&instance_id).await
}

#[tauri::command(rename_all = "snake_case")]
pub async fn plugin_call(
    core: State<'_, Arc<Core>>,
    instance_id: ScreenInstanceId,
    method: PluginMethod,
    params: Value,
) -> Res<Value> {
    let origin = CallOrigin::Screen { instance_id: instance_id.clone() };
    core.plugins().call(&instance_id, method, params, origin).await
}

#[tauri::command(rename_all = "snake_case")]
pub async fn command_run(core: State<'_, Arc<Core>>, command_id: String, ctx: TemplateCtx) -> Res<()> {
    core.plugins().command_run(&command_id, ctx).await
}

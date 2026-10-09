//! `plugin` commands — plugins, screens, plugin_call, contributed commands (owner: L8, ARCHITECTURE §6).
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
pub async fn plugin_list(core: State<'_, Arc<Core>>) -> Res<Vec<PluginInfo>> {
    not_implemented("plugin_list")
}

#[tauri::command(rename_all = "snake_case")]
pub async fn plugin_inspect(core: State<'_, Arc<Core>>, source: String) -> Res<PluginInstallPreview> {
    not_implemented("plugin_inspect")
}

#[tauri::command(rename_all = "snake_case")]
pub async fn plugin_install(
    core: State<'_, Arc<Core>>,
    source: String,
    sha256: String,
    grant: Vec<String>,
) -> Res<PluginInfo> {
    not_implemented("plugin_install")
}

#[tauri::command(rename_all = "snake_case")]
pub async fn plugin_uninstall(core: State<'_, Arc<Core>>, id: PluginId) -> Res<()> {
    not_implemented("plugin_uninstall")
}

#[tauri::command(rename_all = "snake_case")]
pub async fn plugin_enable(core: State<'_, Arc<Core>>, id: PluginId, enabled: bool) -> Res<()> {
    not_implemented("plugin_enable")
}

#[tauri::command(rename_all = "snake_case")]
pub async fn plugin_grant(
    core: State<'_, Arc<Core>>,
    id: PluginId,
    permissions: Vec<String>,
) -> Res<PluginInfo> {
    not_implemented("plugin_grant")
}

#[tauri::command(rename_all = "snake_case")]
pub async fn plugin_screen_open(
    core: State<'_, Arc<Core>>,
    plugin_id: PluginId,
    screen_id: String,
    project_id: Option<ProjectId>,
    params: Value,
) -> Res<ScreenOpenResult> {
    not_implemented("plugin_screen_open")
}

#[tauri::command(rename_all = "snake_case")]
pub async fn plugin_screen_close(core: State<'_, Arc<Core>>, instance_id: ScreenInstanceId) -> Res<()> {
    not_implemented("plugin_screen_close")
}

#[tauri::command(rename_all = "snake_case")]
pub async fn plugin_call(
    core: State<'_, Arc<Core>>,
    instance_id: ScreenInstanceId,
    method: PluginMethod,
    params: Value,
) -> Res<Value> {
    not_implemented("plugin_call")
}

#[tauri::command(rename_all = "snake_case")]
pub async fn command_run(core: State<'_, Arc<Core>>, command_id: String, ctx: TemplateCtx) -> Res<()> {
    not_implemented("command_run")
}

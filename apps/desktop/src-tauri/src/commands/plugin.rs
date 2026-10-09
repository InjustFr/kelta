//! `plugin` commands — plugins, screens, plugin_call, contributed commands (owner: L8, ARCHITECTURE §6).

use std::sync::Arc;

use kelta_core::Core;
use kelta_plugins::{PluginHost, Wiring};
use kelta_proto::api::{PluginSettingsSource, SettingsSource};
use kelta_proto::prelude::*;
use serde_json::Value;
use tauri::State;

use super::Res;

/// The plugin host, wired on first use with the services `CoreApi` does not expose: the UI bridge
/// (`plugin.event` relay, web tool handles), project configs (`projects.list`) and the Global-layer
/// writer for `settings.set` (see docs/contract-requests/L8.md).
pub(crate) fn plugin_host(core: &Arc<Core>) -> &Arc<PluginHost> {
    let host = core.plugins();
    if !host.is_wired() {
        let settings: Arc<dyn SettingsSource> = core.config().clone();
        let config = core.config().clone();
        host.wire(Wiring {
            ui: Some(core.bridge().clone()),
            settings: Some(settings),
            settings_writer: Some(Arc::new(move |id: &PluginId, key: &str, value: Value| {
                config.layer_set(Layer::Global, None, None, &format!("plugins.{id}.{key}"), value).map(|_| ())
            })),
        });
        sync_plugin_schemas(core);
    }
    host.start();
    host
}

/// Hand the enabled plugins' settings schema fragments to kelta-config (Plugin-defaults layer).
pub(crate) fn sync_plugin_schemas(core: &Arc<Core>) {
    core.config().set_plugin_schemas(core.plugins().fragments());
}

#[tauri::command(rename_all = "snake_case")]
pub async fn plugin_list(core: State<'_, Arc<Core>>) -> Res<Vec<PluginInfo>> {
    plugin_host(&core).plugins().await
}

#[tauri::command(rename_all = "snake_case")]
pub async fn plugin_inspect(core: State<'_, Arc<Core>>, source: String) -> Res<PluginInstallPreview> {
    plugin_host(&core).inspect(&source).await
}

#[tauri::command(rename_all = "snake_case")]
pub async fn plugin_install(
    core: State<'_, Arc<Core>>,
    source: String,
    sha256: String,
    grant: Vec<String>,
) -> Res<PluginInfo> {
    let info = plugin_host(&core).install(&source, &sha256, grant).await?;
    sync_plugin_schemas(&core);
    Ok(info)
}

#[tauri::command(rename_all = "snake_case")]
pub async fn plugin_uninstall(core: State<'_, Arc<Core>>, id: PluginId) -> Res<()> {
    plugin_host(&core).uninstall(&id).await?;
    sync_plugin_schemas(&core);
    Ok(())
}

#[tauri::command(rename_all = "snake_case")]
pub async fn plugin_enable(core: State<'_, Arc<Core>>, id: PluginId, enabled: bool) -> Res<()> {
    plugin_host(&core).enable(&id, enabled).await?;
    sync_plugin_schemas(&core);
    Ok(())
}

#[tauri::command(rename_all = "snake_case")]
pub async fn plugin_grant(
    core: State<'_, Arc<Core>>,
    id: PluginId,
    permissions: Vec<String>,
) -> Res<PluginInfo> {
    plugin_host(&core).grant(&id, permissions).await
}

#[tauri::command(rename_all = "snake_case")]
pub async fn plugin_screen_open(
    core: State<'_, Arc<Core>>,
    plugin_id: PluginId,
    screen_id: String,
    project_id: Option<ProjectId>,
    params: Value,
) -> Res<ScreenOpenResult> {
    plugin_host(&core).screen_open(&plugin_id, &screen_id, project_id.as_ref(), params).await
}

#[tauri::command(rename_all = "snake_case")]
pub async fn plugin_screen_close(core: State<'_, Arc<Core>>, instance_id: ScreenInstanceId) -> Res<()> {
    plugin_host(&core).screen_close(&instance_id).await
}

#[tauri::command(rename_all = "snake_case")]
pub async fn plugin_call(
    core: State<'_, Arc<Core>>,
    instance_id: ScreenInstanceId,
    method: PluginMethod,
    params: Value,
) -> Res<Value> {
    let origin = CallOrigin::Screen { instance_id: instance_id.clone() };
    plugin_host(&core).call(&instance_id, method, params, origin).await
}

#[tauri::command(rename_all = "snake_case")]
pub async fn command_run(core: State<'_, Arc<Core>>, command_id: String, ctx: TemplateCtx) -> Res<()> {
    plugin_host(&core).command_run(&command_id, ctx).await
}

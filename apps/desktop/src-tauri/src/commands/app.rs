//! `app` commands — app lifecycle, events channel, external links, perf (owner: L3, ARCHITECTURE §6).

use std::sync::Arc;

use kelta_core::Core;
use kelta_proto::prelude::*;
use tauri::ipc::Channel;
use tauri::{AppHandle, State};

use super::Res;

use crate::window::bridge::TauriBridge;

#[tauri::command(rename_all = "snake_case")]
pub async fn app_info(core: State<'_, Arc<Core>>) -> Res<AppInfo> {
    let mut info = core.app_info();
    if let Some(d) = crate::window::effective_decorations() {
        info.decorations = d;
    }
    Ok(info)
}

/// One subscription per window; the bridge fans out `UiEvent`s to every channel.
#[tauri::command(rename_all = "snake_case")]
pub async fn events_subscribe(
    bridge: State<'_, Arc<TauriBridge>>,
    channel: Channel<UiEvent>,
) -> Res<SubscribeResult> {
    Ok(SubscribeResult { sub_id: bridge.subscribe(channel) })
}

/// The UI rendered its first frame: clears the launch crash guard, records the bench mark, binds
/// the runtime and kicks deferred startup work.
#[tauri::command(rename_all = "snake_case")]
pub async fn app_ready(app: AppHandle, core: State<'_, Arc<Core>>, t_ms: f64) -> Res<()> {
    crate::platform::launch_succeeded();
    crate::window::bench::app_ready();
    crate::platform::update::auto_check(&app);
    core.app_ready(t_ms).await
}

/// Updates (Settings → General, the restart banner).
#[tauri::command(rename_all = "snake_case")]
pub async fn app_update_status(app: AppHandle) -> Res<UpdateStatus> {
    Ok(crate::platform::update::status(&app))
}

/// "Check now": checks, installs what it finds, then returns the status.
#[tauri::command(rename_all = "snake_case")]
pub async fn app_update_check(app: AppHandle) -> Res<UpdateStatus> {
    crate::platform::update::check_and_install(&app).await;
    Ok(crate::platform::update::status(&app))
}

/// Restart through the normal exit (`Core::shutdown`: keltad keeps the kept sessions running).
#[tauri::command(rename_all = "snake_case")]
pub async fn app_restart(app: AppHandle) -> Res<()> {
    app.request_restart();
    Ok(())
}

/// The quit dialog's answer (#305): quit, keeping keltad's sessions running or stopping them all.
#[tauri::command(rename_all = "snake_case")]
pub async fn app_quit(app: AppHandle, core: State<'_, Arc<Core>>, stop: bool) -> Res<()> {
    if stop {
        core.stop_sessions_on_quit();
    }
    crate::window::quit(&app);
    Ok(())
}

/// The window close the UI intercepted (it saved its layout): ask keep / stop, or close the window.
#[tauri::command(rename_all = "snake_case")]
pub async fn app_close(app: AppHandle) -> Res<()> {
    crate::window::close_requested(&app);
    Ok(())
}

/// kelta-bench metric measured in the UI (no-op unless `KELTA_BENCH_MARKS` is set).
#[tauri::command(rename_all = "snake_case")]
pub async fn bench_mark(key: String, value: f64) -> Res<()> {
    crate::window::bench::mark(&key, value);
    Ok(())
}

#[tauri::command(rename_all = "snake_case")]
pub async fn open_external(core: State<'_, Arc<Core>>, url: String) -> Res<()> {
    core.open_external(&url).await
}

#[tauri::command(rename_all = "snake_case")]
pub async fn perf_snapshot(core: State<'_, Arc<Core>>) -> Res<PerfSnapshot> {
    Ok(core.perf_snapshot())
}

#[tauri::command(rename_all = "snake_case")]
pub async fn notify_test(core: State<'_, Arc<Core>>) -> Res<()> {
    core.notify_test().await
}

/// Kelta's MCP in Claude Code (Settings → Claude): `None` = Claude Code not found.
#[tauri::command(rename_all = "snake_case")]
pub async fn claude_mcp_status(core: State<'_, Arc<Core>>) -> Res<Option<bool>> {
    Ok(core.claude_mcp_status().await)
}

/// Settings → Claude (#277): the mode sessions whose profile forces none start in; `None` = Claude's default.
#[tauri::command(rename_all = "snake_case")]
pub async fn claude_permission_mode(core: State<'_, Arc<Core>>) -> Res<Option<PermissionMode>> {
    Ok(core.claude_permission_mode())
}

#[tauri::command(rename_all = "snake_case")]
pub async fn claude_mcp_install(core: State<'_, Arc<Core>>) -> Res<()> {
    core.claude_mcp_install().await
}

/// "Set up with Claude" (#343): installs Kelta's MCP if needed, then starts the setup session in Home.
#[tauri::command(rename_all = "snake_case")]
pub async fn claude_setup(core: State<'_, Arc<Core>>) -> Res<SessionInfo> {
    core.claude_setup().await
}

#[tauri::command(rename_all = "snake_case")]
pub async fn claude_mcp_remove(core: State<'_, Arc<Core>>) -> Res<()> {
    core.claude_mcp_remove().await
}

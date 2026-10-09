//! `app` commands — app lifecycle, events channel, external links, perf (owner: L3, ARCHITECTURE §6).

use std::sync::Arc;

use kelta_core::Core;
use kelta_proto::prelude::*;
use tauri::State;
use tauri::ipc::Channel;

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
pub async fn app_ready(core: State<'_, Arc<Core>>, t_ms: f64) -> Res<()> {
    crate::platform::launch_succeeded();
    crate::window::bench::app_ready();
    core.app_ready(t_ms).await
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

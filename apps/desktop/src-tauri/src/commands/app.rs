//! `app` commands — app lifecycle, events channel, external links, perf (owner: L3, ARCHITECTURE §6).
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

use crate::window::bridge::TauriBridge;

#[tauri::command(rename_all = "snake_case")]
pub async fn app_info(core: State<'_, Arc<Core>>) -> Res<AppInfo> {
    Ok(core.app_info())
}

/// One subscription per window; the bridge fans out `UiEvent`s to every channel.
#[tauri::command(rename_all = "snake_case")]
pub async fn events_subscribe(
    bridge: State<'_, Arc<TauriBridge>>,
    channel: Channel<UiEvent>,
) -> Res<SubscribeResult> {
    Ok(SubscribeResult { sub_id: bridge.subscribe(channel) })
}

#[tauri::command(rename_all = "snake_case")]
pub async fn app_ready(core: State<'_, Arc<Core>>, t_ms: f64) -> Res<()> {
    not_implemented("app_ready")
}

#[tauri::command(rename_all = "snake_case")]
pub async fn open_external(core: State<'_, Arc<Core>>, url: String) -> Res<()> {
    not_implemented("open_external")
}

#[tauri::command(rename_all = "snake_case")]
pub async fn perf_snapshot(core: State<'_, Arc<Core>>) -> Res<PerfSnapshot> {
    not_implemented("perf_snapshot")
}

#[tauri::command(rename_all = "snake_case")]
pub async fn notify_test(core: State<'_, Arc<Core>>) -> Res<()> {
    not_implemented("notify_test")
}

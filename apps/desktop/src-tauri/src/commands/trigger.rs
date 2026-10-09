//! `trigger` commands — triggers (owner: L8, ARCHITECTURE §6).
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
pub async fn trigger_list(
    core: State<'_, Arc<Core>>,
    project_id: Option<ProjectId>,
) -> Res<Vec<TriggerInfo>> {
    not_implemented("trigger_list")
}

#[tauri::command(rename_all = "snake_case")]
pub async fn trigger_test(core: State<'_, Arc<Core>>, trigger_id: String, payload: Value) -> Res<TriggerRun> {
    not_implemented("trigger_test")
}

#[tauri::command(rename_all = "snake_case")]
pub async fn trigger_log(core: State<'_, Arc<Core>>, limit: u32) -> Res<Vec<TriggerRun>> {
    not_implemented("trigger_log")
}

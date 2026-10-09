//! `tool` commands — tools (owner: L8, ARCHITECTURE §6).
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
pub async fn tool_list(core: State<'_, Arc<Core>>, project_id: ProjectId) -> Res<Vec<ToolInfo>> {
    not_implemented("tool_list")
}

#[tauri::command(rename_all = "snake_case")]
pub async fn tool_check(core: State<'_, Arc<Core>>, tool_id: ToolId) -> Res<ToolCheck> {
    not_implemented("tool_check")
}

#[tauri::command(rename_all = "snake_case")]
pub async fn tool_open(
    core: State<'_, Arc<Core>>,
    project_id: ProjectId,
    tool_id: ToolId,
    ctx: TemplateCtx,
    placement: Placement,
) -> Res<ToolHandle> {
    not_implemented("tool_open")
}

#[tauri::command(rename_all = "snake_case")]
pub async fn tool_close(core: State<'_, Arc<Core>>, instance_id: ToolInstanceId) -> Res<()> {
    not_implemented("tool_close")
}

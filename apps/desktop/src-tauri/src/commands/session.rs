//! `session` commands — sessions and terminal channel (owner: L3, ARCHITECTURE §6).
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

use super::{Res, not_implemented, parse_session_write};

#[tauri::command(rename_all = "snake_case")]
pub async fn session_spawn(core: State<'_, Arc<Core>>, req: SpawnRequest) -> Res<SessionInfo> {
    not_implemented("session_spawn")
}

#[tauri::command(rename_all = "snake_case")]
pub async fn session_spawn_template(
    core: State<'_, Arc<Core>>,
    project_id: ProjectId,
    template_id: String,
    ctx: TemplateCtx,
    placement: Placement,
) -> Res<Vec<SessionInfo>> {
    not_implemented("session_spawn_template")
}

#[tauri::command(rename_all = "snake_case")]
pub async fn session_attach(
    core: State<'_, Arc<Core>>,
    id: SessionId,
    cols: u16,
    rows: u16,
    channel: Channel<InvokeResponseBody>,
) -> Res<AttachInfo> {
    not_implemented("session_attach")
}

#[tauri::command(rename_all = "snake_case")]
pub async fn session_detach(core: State<'_, Arc<Core>>, id: SessionId, generation: u32) -> Res<()> {
    not_implemented("session_detach")
}

/// Binary input: raw body + `x-kelta-session-id` header, or JSON `{id, data}` (see
/// `commands::parse_session_write`). Fire-and-forget from the UI.
#[tauri::command(rename_all = "snake_case")]
pub async fn session_write(core: State<'_, Arc<Core>>, request: tauri::ipc::Request<'_>) -> Res<()> {
    let (id, data) = parse_session_write(&request)?;
    not_implemented("session_write")
}

#[tauri::command(rename_all = "snake_case")]
pub async fn session_resize(core: State<'_, Arc<Core>>, id: SessionId, cols: u16, rows: u16) -> Res<()> {
    not_implemented("session_resize")
}

#[tauri::command(rename_all = "snake_case")]
pub async fn session_ack(core: State<'_, Arc<Core>>, id: SessionId, generation: u32, bytes: u32) -> Res<()> {
    not_implemented("session_ack")
}

#[tauri::command(rename_all = "snake_case")]
pub async fn session_kill(core: State<'_, Arc<Core>>, id: SessionId, force: bool) -> Res<()> {
    not_implemented("session_kill")
}

#[tauri::command(rename_all = "snake_case")]
pub async fn session_restart(core: State<'_, Arc<Core>>, id: SessionId) -> Res<SessionInfo> {
    not_implemented("session_restart")
}

#[tauri::command(rename_all = "snake_case")]
pub async fn session_rename(core: State<'_, Arc<Core>>, id: SessionId, name: String) -> Res<SessionInfo> {
    not_implemented("session_rename")
}

#[tauri::command(rename_all = "snake_case")]
pub async fn session_list(
    core: State<'_, Arc<Core>>,
    project_id: Option<ProjectId>,
) -> Res<Vec<SessionInfo>> {
    not_implemented("session_list")
}

#[tauri::command(rename_all = "snake_case")]
pub async fn session_mark_seen(core: State<'_, Arc<Core>>, id: SessionId) -> Res<()> {
    not_implemented("session_mark_seen")
}

#[tauri::command(rename_all = "snake_case")]
pub async fn session_link(
    core: State<'_, Arc<Core>>,
    id: SessionId,
    work_item_id: Option<WorkItemId>,
    ticket: Option<TicketRef>,
) -> Res<SessionInfo> {
    not_implemented("session_link")
}

#[tauri::command(rename_all = "snake_case")]
pub async fn session_text_tail(core: State<'_, Arc<Core>>, id: SessionId, max_lines: u32) -> Res<String> {
    not_implemented("session_text_tail")
}

#[tauri::command(rename_all = "snake_case")]
pub async fn terminal_set_palette(core: State<'_, Arc<Core>>, palette: TerminalPalette) -> Res<()> {
    not_implemented("terminal_set_palette")
}

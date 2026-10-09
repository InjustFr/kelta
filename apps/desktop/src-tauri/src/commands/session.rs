//! `session` commands — sessions and terminal channel (owner: L3, ARCHITECTURE §6).

use std::sync::Arc;

use kelta_core::Core;
use kelta_proto::prelude::*;
use tauri::State;
use tauri::ipc::{Channel, InvokeResponseBody};

use super::{Res, parse_session_write};

/// Frames of one attached view → the `session_attach` channel (raw bytes, first byte = tag).
struct ChannelSink(Channel<InvokeResponseBody>);

impl FrameSink for ChannelSink {
    fn send(&mut self, frame: Vec<u8>) -> bool {
        self.0.send(InvokeResponseBody::Raw(frame)).is_ok()
    }
}

#[tauri::command(rename_all = "snake_case")]
pub async fn session_spawn(core: State<'_, Arc<Core>>, req: SpawnRequest) -> Res<SessionInfo> {
    core.session_spawn(req).await
}

#[tauri::command(rename_all = "snake_case")]
pub async fn session_spawn_template(
    core: State<'_, Arc<Core>>,
    project_id: ProjectId,
    template_id: String,
    ctx: TemplateCtx,
    placement: Placement,
) -> Res<Vec<SessionInfo>> {
    core.session_spawn_template(&project_id, &template_id, ctx, placement).await
}

/// Dormant sessions spawn here (lazy restore).
#[tauri::command(rename_all = "snake_case")]
pub async fn session_attach(
    core: State<'_, Arc<Core>>,
    id: SessionId,
    cols: u16,
    rows: u16,
    channel: Channel<InvokeResponseBody>,
) -> Res<AttachInfo> {
    core.session_attach(&id, cols, rows, Box::new(ChannelSink(channel))).await
}

#[tauri::command(rename_all = "snake_case")]
pub async fn session_detach(core: State<'_, Arc<Core>>, id: SessionId, generation: u32) -> Res<()> {
    core.session_detach(&id, generation);
    Ok(())
}

/// Binary input: raw body + `x-kelta-session-id` header, or JSON `{id, data}` (see
/// `commands::parse_session_write`). Fire-and-forget from the UI.
#[tauri::command(rename_all = "snake_case")]
pub async fn session_write(core: State<'_, Arc<Core>>, request: tauri::ipc::Request<'_>) -> Res<()> {
    let (id, data) = parse_session_write(&request)?;
    core.session_write(&id, &data).await
}

#[tauri::command(rename_all = "snake_case")]
pub async fn session_resize(core: State<'_, Arc<Core>>, id: SessionId, cols: u16, rows: u16) -> Res<()> {
    core.session_resize(&id, cols, rows)
}

#[tauri::command(rename_all = "snake_case")]
pub async fn session_ack(core: State<'_, Arc<Core>>, id: SessionId, generation: u32, bytes: u32) -> Res<()> {
    core.session_ack(&id, generation, bytes);
    Ok(())
}

#[tauri::command(rename_all = "snake_case")]
pub async fn session_kill(core: State<'_, Arc<Core>>, id: SessionId, force: bool) -> Res<()> {
    core.session_kill(&id, force).await
}

#[tauri::command(rename_all = "snake_case")]
pub async fn session_restart(core: State<'_, Arc<Core>>, id: SessionId) -> Res<SessionInfo> {
    core.session_restart(&id).await
}

#[tauri::command(rename_all = "snake_case")]
pub async fn session_rename(core: State<'_, Arc<Core>>, id: SessionId, name: String) -> Res<SessionInfo> {
    core.session_rename(&id, &name)
}

#[tauri::command(rename_all = "snake_case")]
pub async fn session_list(
    core: State<'_, Arc<Core>>,
    project_id: Option<ProjectId>,
) -> Res<Vec<SessionInfo>> {
    Ok(core.list_sessions(project_id.as_ref()))
}

#[tauri::command(rename_all = "snake_case")]
pub async fn session_mark_seen(core: State<'_, Arc<Core>>, id: SessionId) -> Res<()> {
    core.session_mark_seen(&id)
}

#[tauri::command(rename_all = "snake_case")]
pub async fn session_link(
    core: State<'_, Arc<Core>>,
    id: SessionId,
    work_item_id: Option<WorkItemId>,
    ticket: Option<TicketRef>,
) -> Res<SessionInfo> {
    core.session_link(&id, work_item_id, ticket)
}

#[tauri::command(rename_all = "snake_case")]
pub async fn session_text_tail(core: State<'_, Arc<Core>>, id: SessionId, max_lines: u32) -> Res<String> {
    core.session_text_tail(&id, max_lines)
}

#[tauri::command(rename_all = "snake_case")]
pub async fn session_history_search(
    core: State<'_, Arc<Core>>,
    project_id: ProjectId,
    session_id: Option<SessionId>,
    query: String,
    limit: u32,
) -> Res<Vec<HistoryHit>> {
    core.session_history_search(&project_id, session_id.as_ref(), &query, limit)
}

/// Pushed on theme change (OSC 4/10/11/12 replies).
#[tauri::command(rename_all = "snake_case")]
pub async fn terminal_set_palette(core: State<'_, Arc<Core>>, palette: TerminalPalette) -> Res<()> {
    core.terminal().set_palette(palette);
    Ok(())
}

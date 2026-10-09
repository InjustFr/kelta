//! `tracker` commands — tickets (owner: L3, ARCHITECTURE §6).

use std::sync::Arc;

use kelta_core::Core;
use kelta_proto::prelude::*;
use serde_json::Value;
use tauri::State;

use super::Res;

#[tauri::command(rename_all = "snake_case")]
pub async fn tracker_list(
    core: State<'_, Arc<Core>>,
    scope: Scope,
    view_id: Option<String>,
    cursor: Option<Cursor>,
    refresh: bool,
) -> Res<TicketPage> {
    core.tracker_list(scope, view_id, cursor, refresh).await
}

#[tauri::command(rename_all = "snake_case")]
pub async fn tracker_get(core: State<'_, Arc<Core>>, ticket: TicketRef) -> Res<TicketDetail> {
    core.tracker_get(&ticket).await
}

#[tauri::command(rename_all = "snake_case")]
pub async fn tracker_columns(core: State<'_, Arc<Core>>, project_id: ProjectId) -> Res<Vec<Column>> {
    core.tracker_columns(&project_id).await
}

#[tauri::command(rename_all = "snake_case")]
pub async fn tracker_transitions(core: State<'_, Arc<Core>>, ticket: TicketRef) -> Res<Vec<Transition>> {
    core.tracker_transitions(&ticket).await
}

/// `NeedsFields` errors carry `detail.fields`.
#[tauri::command(rename_all = "snake_case")]
pub async fn tracker_transition(
    core: State<'_, Arc<Core>>,
    ticket: TicketRef,
    transition_id: String,
    fields: Option<Value>,
) -> Res<Ticket> {
    core.tracker_transition(&ticket, &transition_id, fields).await
}

/// Column → transition; ambiguous → `Conflict` with `detail.candidates`.
#[tauri::command(rename_all = "snake_case")]
pub async fn tracker_move(core: State<'_, Arc<Core>>, ticket: TicketRef, column_id: String) -> Res<Ticket> {
    core.tracker_move(&ticket, &column_id).await
}

#[tauri::command(rename_all = "snake_case")]
pub async fn tracker_comment(core: State<'_, Arc<Core>>, ticket: TicketRef, markdown: String) -> Res<()> {
    core.tracker_comment(&ticket, &markdown).await
}

#[tauri::command(rename_all = "snake_case")]
pub async fn tracker_assign(
    core: State<'_, Arc<Core>>,
    ticket: TicketRef,
    assignee: Assignee,
) -> Res<Ticket> {
    core.tracker_assign(&ticket, assignee).await
}

#[tauri::command(rename_all = "snake_case")]
pub async fn tracker_search(core: State<'_, Arc<Core>>, scope: Scope, text: String) -> Res<Vec<TicketItem>> {
    core.tracker_search(scope, &text).await
}

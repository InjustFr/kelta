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
    who: Option<Who>,
) -> Res<TicketPage> {
    core.tracker_list(scope, view_id, who, cursor, refresh).await
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
    core.tracker_transition(&ticket, &transition_id, fields, None).await
}

/// Column → transition; ambiguous → `Conflict` with `detail.candidates`. `project_id` picks the
/// columns (default: the project bound to the ticket's account).
#[tauri::command(rename_all = "snake_case")]
pub async fn tracker_move(
    core: State<'_, Arc<Core>>,
    ticket: TicketRef,
    column_id: String,
    project_id: Option<ProjectId>,
) -> Res<Ticket> {
    core.tracker_move(&ticket, &column_id, project_id.as_ref()).await
}

#[tauri::command(rename_all = "snake_case")]
pub async fn tracker_comment(core: State<'_, Arc<Core>>, ticket: TicketRef, markdown: String) -> Res<()> {
    core.tracker_comment(&ticket, &markdown, None).await
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
pub async fn tracker_assignable_users(
    core: State<'_, Arc<Core>>,
    ticket: TicketRef,
    query: String,
) -> Res<Vec<User>> {
    core.tracker_assignable_users(&ticket, &query).await
}

#[tauri::command(rename_all = "snake_case")]
pub async fn tracker_priorities(core: State<'_, Arc<Core>>, ticket: TicketRef) -> Res<Vec<String>> {
    core.tracker_priorities(&ticket).await
}

#[tauri::command(rename_all = "snake_case")]
pub async fn tracker_set_priority(
    core: State<'_, Arc<Core>>,
    ticket: TicketRef,
    priority: String,
) -> Res<Ticket> {
    core.tracker_set_priority(&ticket, &priority).await
}

#[tauri::command(rename_all = "snake_case")]
pub async fn tracker_refine(
    core: State<'_, Arc<Core>>,
    ticket: TicketRef,
    project_id: Option<ProjectId>,
) -> Res<String> {
    core.tracker_refine(&ticket, project_id.as_ref()).await
}

#[tauri::command(rename_all = "snake_case")]
pub async fn tracker_search(core: State<'_, Arc<Core>>, scope: Scope, text: String) -> Res<Vec<TicketItem>> {
    core.tracker_search(scope, &text).await
}

#[tauri::command(rename_all = "snake_case")]
pub async fn tracker_sources(
    core: State<'_, Arc<Core>>,
    account_id: AccountId,
    query: String,
) -> Res<Vec<SourceHit>> {
    core.tracker_sources(&account_id, &query).await
}

/// My Next up list, snoozes and seen tickets (#145): Kelta-local, never a tracker call.
#[tauri::command(rename_all = "snake_case")]
pub async fn next_up_list(core: State<'_, Arc<Core>>) -> Res<NextUp> {
    core.next_up_list().await
}

#[tauri::command(rename_all = "snake_case")]
pub async fn next_up_put(core: State<'_, Arc<Core>>, item: NextUpItem) -> Res<()> {
    core.next_up_put(item).await
}

#[tauri::command(rename_all = "snake_case")]
pub async fn next_up_remove(core: State<'_, Arc<Core>>, ticket: TicketRef) -> Res<()> {
    core.next_up_remove(ticket).await
}

/// Clears the tickets' `New` badge.
#[tauri::command(rename_all = "snake_case")]
pub async fn ticket_seen(core: State<'_, Arc<Core>>, tickets: Vec<TicketRef>) -> Res<()> {
    core.ticket_seen(tickets).await
}

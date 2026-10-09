//! `tracker` commands — tickets (owner: L3, ARCHITECTURE §6).
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
pub async fn tracker_list(
    core: State<'_, Arc<Core>>,
    scope: Scope,
    view_id: Option<String>,
    cursor: Option<Cursor>,
    refresh: bool,
) -> Res<TicketPage> {
    not_implemented("tracker_list")
}

#[tauri::command(rename_all = "snake_case")]
pub async fn tracker_get(core: State<'_, Arc<Core>>, ticket: TicketRef) -> Res<TicketDetail> {
    not_implemented("tracker_get")
}

#[tauri::command(rename_all = "snake_case")]
pub async fn tracker_columns(core: State<'_, Arc<Core>>, project_id: ProjectId) -> Res<Vec<Column>> {
    not_implemented("tracker_columns")
}

#[tauri::command(rename_all = "snake_case")]
pub async fn tracker_transitions(core: State<'_, Arc<Core>>, ticket: TicketRef) -> Res<Vec<Transition>> {
    not_implemented("tracker_transitions")
}

#[tauri::command(rename_all = "snake_case")]
pub async fn tracker_transition(
    core: State<'_, Arc<Core>>,
    ticket: TicketRef,
    transition_id: String,
    fields: Option<Value>,
) -> Res<Ticket> {
    not_implemented("tracker_transition")
}

#[tauri::command(rename_all = "snake_case")]
pub async fn tracker_move(core: State<'_, Arc<Core>>, ticket: TicketRef, column_id: String) -> Res<Ticket> {
    not_implemented("tracker_move")
}

#[tauri::command(rename_all = "snake_case")]
pub async fn tracker_comment(core: State<'_, Arc<Core>>, ticket: TicketRef, markdown: String) -> Res<()> {
    not_implemented("tracker_comment")
}

#[tauri::command(rename_all = "snake_case")]
pub async fn tracker_assign(
    core: State<'_, Arc<Core>>,
    ticket: TicketRef,
    assignee: Assignee,
) -> Res<Ticket> {
    not_implemented("tracker_assign")
}

#[tauri::command(rename_all = "snake_case")]
pub async fn tracker_search(core: State<'_, Arc<Core>>, scope: Scope, text: String) -> Res<Vec<TicketItem>> {
    not_implemented("tracker_search")
}

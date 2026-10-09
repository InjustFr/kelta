//! `editor` commands — editor integration (owner: L6, ARCHITECTURE §6).
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
pub async fn editor_open(
    core: State<'_, Arc<Core>>,
    target: EditorTarget,
    path: PathBuf,
    line: Option<u32>,
) -> Res<()> {
    not_implemented("editor_open")
}

#[tauri::command(rename_all = "snake_case")]
pub async fn editor_send_selection(
    core: State<'_, Arc<Core>>,
    editor_session: SessionId,
    claude_session: SessionId,
) -> Res<()> {
    not_implemented("editor_send_selection")
}

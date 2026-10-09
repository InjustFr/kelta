//! `clipboard` commands — clipboard via arboard (CLIPBOARD + Linux PRIMARY) (owner: L3, ARCHITECTURE §6).

use std::sync::Arc;

use kelta_core::Core;
use kelta_proto::prelude::*;
use tauri::State;

use super::Res;

#[tauri::command(rename_all = "snake_case")]
pub async fn clipboard_read(core: State<'_, Arc<Core>>, kind: ClipboardKind) -> Res<String> {
    core.clipboard_read(kind).await
}

#[tauri::command(rename_all = "snake_case")]
pub async fn clipboard_write(core: State<'_, Arc<Core>>, kind: ClipboardKind, text: String) -> Res<()> {
    core.clipboard_write(kind, text).await
}

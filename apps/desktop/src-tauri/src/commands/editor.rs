//! `editor` commands — editor integration (owner: L6, ARCHITECTURE §6).
//!
//! Thin delegation to `kelta_work::WorkService` (through `Core::work()`).

use std::path::PathBuf;
use std::sync::Arc;

use kelta_core::Core;
use kelta_proto::prelude::*;
use tauri::State;

use super::Res;

#[tauri::command(rename_all = "snake_case")]
pub async fn editor_open(
    core: State<'_, Arc<Core>>,
    target: EditorTarget,
    path: PathBuf,
    line: Option<u32>,
) -> Res<()> {
    core.work().editor_open(target, &path, line).await
}

#[tauri::command(rename_all = "snake_case")]
pub async fn editor_send_selection(
    core: State<'_, Arc<Core>>,
    editor_session: SessionId,
    claude_session: SessionId,
) -> Res<()> {
    core.work().send_selection(&editor_session, &claude_session).await
}

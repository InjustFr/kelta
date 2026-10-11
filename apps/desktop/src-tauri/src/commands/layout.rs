//! `layout` commands — layouts (optimistic `rev`) (owner: L3, ARCHITECTURE §6).

use std::sync::Arc;

use kelta_core::Core;
use kelta_proto::prelude::*;
use tauri::State;

use super::Res;

#[tauri::command(rename_all = "snake_case")]
pub async fn layout_get(core: State<'_, Arc<Core>>, project_id: ProjectId) -> Res<Layout> {
    core.layout_get(&project_id)
}

/// Stale `rev` → `Conflict` with `detail.rev` (the UI refetches).
#[tauri::command(rename_all = "snake_case")]
pub async fn layout_save(core: State<'_, Arc<Core>>, layout: Layout) -> Res<LayoutSaveResult> {
    core.layout_save(layout)
}

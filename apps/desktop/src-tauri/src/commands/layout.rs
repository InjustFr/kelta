//! `layout` commands — layouts (optimistic `rev`) (owner: L3, ARCHITECTURE §6).

use std::sync::Arc;

use kelta_core::Core;
use kelta_proto::prelude::*;
use tauri::{State, Window};

use super::Res;
use crate::window::bridge::TauriBridge;

#[tauri::command(rename_all = "snake_case")]
pub async fn layout_get(core: State<'_, Arc<Core>>, project_id: ProjectId) -> Res<Layout> {
    core.layout_get(&project_id)
}

/// Stale `rev` → `Conflict` with `detail.rev` (the UI refetches). The other windows (#342) get the
/// saved layout as `layout.changed`.
#[tauri::command(rename_all = "snake_case")]
pub async fn layout_save(
    window: Window,
    core: State<'_, Arc<Core>>,
    bridge: State<'_, Arc<TauriBridge>>,
    layout: Layout,
) -> Res<LayoutSaveResult> {
    let saved = core.layout_save(layout.clone())?;
    let layout = Layout { rev: saved.rev, ..layout };
    bridge.emit_except(
        window.label(),
        UiEvent::LayoutChanged { project_id: layout.project_id.clone(), layout },
    );
    Ok(saved)
}

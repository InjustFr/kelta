//! `layout` commands — layouts (optimistic `rev`) (owner: L3, ARCHITECTURE §6).
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
pub async fn layout_get(core: State<'_, Arc<Core>>, project_id: ProjectId) -> Res<Layout> {
    not_implemented("layout_get")
}

#[tauri::command(rename_all = "snake_case")]
pub async fn layout_save(core: State<'_, Arc<Core>>, layout: Layout) -> Res<LayoutSaveResult> {
    not_implemented("layout_save")
}

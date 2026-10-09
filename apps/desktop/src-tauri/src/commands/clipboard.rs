//! `clipboard` commands — clipboard via arboard (CLIPBOARD + Linux PRIMARY) (owner: L3, ARCHITECTURE §6).
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
pub async fn clipboard_read(core: State<'_, Arc<Core>>, kind: ClipboardKind) -> Res<String> {
    not_implemented("clipboard_read")
}

#[tauri::command(rename_all = "snake_case")]
pub async fn clipboard_write(core: State<'_, Arc<Core>>, kind: ClipboardKind, text: String) -> Res<()> {
    not_implemented("clipboard_write")
}

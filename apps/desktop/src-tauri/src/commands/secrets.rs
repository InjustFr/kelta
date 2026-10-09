//! `secrets` commands — secret refs (values are never echoed or logged) (owner: L4, ARCHITECTURE §6).
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
pub async fn secret_set(core: State<'_, Arc<Core>>, secret_ref: String, value: String) -> Res<()> {
    not_implemented("secret_set")
}

#[tauri::command(rename_all = "snake_case")]
pub async fn secret_delete(core: State<'_, Arc<Core>>, secret_ref: String) -> Res<()> {
    not_implemented("secret_delete")
}

#[tauri::command(rename_all = "snake_case")]
pub async fn secret_backends_status(core: State<'_, Arc<Core>>) -> Res<Vec<SecretBackendStatus>> {
    not_implemented("secret_backends_status")
}

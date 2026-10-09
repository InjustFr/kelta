//! `secrets` commands — secret refs (values are never echoed or logged) (owner: L4, ARCHITECTURE §6).
//!
//! The only place a token value crosses IPC is the inbound `secret_set.value`; it is handed to
//! the resolver and dropped. Responses carry no secret material.

use std::sync::Arc;

use kelta_core::Core;
use kelta_proto::prelude::*;
use tauri::State;

use super::Res;

#[tauri::command(rename_all = "snake_case")]
pub async fn secret_set(core: State<'_, Arc<Core>>, secret_ref: String, value: String) -> Res<()> {
    core.secret_resolver().set(&SecretRef::new(secret_ref), &value).await
}

#[tauri::command(rename_all = "snake_case")]
pub async fn secret_delete(core: State<'_, Arc<Core>>, secret_ref: String) -> Res<()> {
    core.secret_resolver().delete(&SecretRef::new(secret_ref)).await
}

#[tauri::command(rename_all = "snake_case")]
pub async fn secret_backends_status(core: State<'_, Arc<Core>>) -> Res<Vec<SecretBackendStatus>> {
    Ok(core.secret_resolver().backends_status().await)
}

//! `secrets` commands — secret refs (values are never echoed or logged) (owner: L4, ARCHITECTURE §6).
//!
//! The only places secret material crosses IPC are the inbound `secret_set.value` and
//! `secret_unlock.passphrase`; both are handed to the resolver and dropped (the passphrase
//! zeroized). Responses carry no secret material.

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

/// Unlock the encrypted secrets file for this run (`create`: make it when missing). UI only:
/// the ctl socket has no equivalent, so kelta-ctl never handles the passphrase.
#[tauri::command(rename_all = "snake_case")]
pub async fn secret_unlock(core: State<'_, Arc<Core>>, passphrase: String, create: bool) -> Res<()> {
    core.secrets().unlock(passphrase, create).await
}

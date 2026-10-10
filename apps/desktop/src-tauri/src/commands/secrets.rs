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

/// "Sign in with browser": a device code for GitHub / GitLab. The response carries only the
/// user code and URL to show; the device code stays in the core.
#[tauri::command(rename_all = "snake_case")]
pub async fn oauth_device_start(
    core: State<'_, Arc<Core>>,
    kind: AccountKind,
    base_url: String,
    secret_ref: String,
) -> Res<OAuthDevicePrompt> {
    core.oauth_device_start(kind, &base_url, SecretRef::new(secret_ref)).await
}

/// Resolves once the user approved `user_code` and the tokens are stored at the start's
/// `secret_ref`; `timeout` when the code expired, `cancelled` when denied.
#[tauri::command(rename_all = "snake_case")]
pub async fn oauth_device_finish(core: State<'_, Arc<Core>>, user_code: String) -> Res<()> {
    core.oauth_device_finish(&user_code).await
}

/// Stop a sign-in the user walked away from: its pending `oauth_device_finish` ends `cancelled`.
#[tauri::command(rename_all = "snake_case")]
pub async fn oauth_device_cancel(core: State<'_, Arc<Core>>, user_code: String) -> Res<()> {
    core.oauth_device_cancel(&user_code);
    Ok(())
}

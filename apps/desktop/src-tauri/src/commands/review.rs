//! `review` commands — PR/MR reviews (owner: L3, ARCHITECTURE §6).
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
pub async fn review_list(
    core: State<'_, Arc<Core>>,
    scope: Scope,
    kind: ReviewKind,
    refresh: bool,
) -> Res<ReviewPage> {
    not_implemented("review_list")
}

#[tauri::command(rename_all = "snake_case")]
pub async fn review_get(core: State<'_, Arc<Core>>, review: ReviewRef) -> Res<ReviewDetail> {
    not_implemented("review_get")
}

#[tauri::command(rename_all = "snake_case")]
pub async fn review_approve(core: State<'_, Arc<Core>>, review: ReviewRef, head_sha: String) -> Res<()> {
    not_implemented("review_approve")
}

#[tauri::command(rename_all = "snake_case")]
pub async fn review_comment(core: State<'_, Arc<Core>>, review: ReviewRef, body: String) -> Res<()> {
    not_implemented("review_comment")
}

#[tauri::command(rename_all = "snake_case")]
pub async fn review_request_changes(core: State<'_, Arc<Core>>, review: ReviewRef, body: String) -> Res<()> {
    not_implemented("review_request_changes")
}

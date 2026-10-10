//! `review` commands — PR/MR reviews (owner: L3, ARCHITECTURE §6).

use std::sync::Arc;

use kelta_core::Core;
use kelta_proto::prelude::*;
use tauri::State;

use super::Res;

#[tauri::command(rename_all = "snake_case")]
pub async fn review_list(
    core: State<'_, Arc<Core>>,
    scope: Scope,
    kind: ReviewKind,
    refresh: bool,
) -> Res<ReviewPage> {
    core.review_page(scope, kind, refresh).await
}

#[tauri::command(rename_all = "snake_case")]
pub async fn review_get(core: State<'_, Arc<Core>>, review: ReviewRef) -> Res<ReviewDetail> {
    core.review_get(&review).await
}

/// `Conflict` when the head moved since `head_sha` was shown.
#[tauri::command(rename_all = "snake_case")]
pub async fn review_approve(core: State<'_, Arc<Core>>, review: ReviewRef, head_sha: String) -> Res<()> {
    core.review_approve(&review, &head_sha).await
}

#[tauri::command(rename_all = "snake_case")]
pub async fn review_comment(core: State<'_, Arc<Core>>, review: ReviewRef, body: String) -> Res<()> {
    core.review_comment(&review, &body).await
}

#[tauri::command(rename_all = "snake_case")]
pub async fn review_request_changes(core: State<'_, Arc<Core>>, review: ReviewRef, body: String) -> Res<()> {
    core.review_request_changes(&review, &body).await
}

/// Nudges the reviewers of my PR: re-request (`comment` null) or post `comment`; refused for 24 h after the last.
#[tauri::command(rename_all = "snake_case")]
pub async fn review_nudge(core: State<'_, Arc<Core>>, review: ReviewRef, comment: Option<String>) -> Res<()> {
    core.review_nudge(&review, comment.as_deref()).await
}

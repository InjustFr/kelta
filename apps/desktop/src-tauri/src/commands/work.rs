//! `work` commands — work items (start work saga, PRs, finish) (owner: L6, ARCHITECTURE §6).
//!
//! Thin delegation to `kelta_work::WorkService` (through `Core::work()`).

use std::collections::BTreeMap;
use std::sync::Arc;

use kelta_core::Core;
use kelta_proto::prelude::*;
use tauri::State;

use super::Res;

#[tauri::command(rename_all = "snake_case")]
pub async fn work_plan(
    core: State<'_, Arc<Core>>,
    project_id: ProjectId,
    source: WorkSource,
) -> Res<StartWorkPlan> {
    core.work().plan(&project_id, source).await
}

#[tauri::command(rename_all = "snake_case")]
pub async fn work_start(core: State<'_, Arc<Core>>, plan: StartWorkPlan) -> Res<WorkItem> {
    core.work().start(plan).await
}

#[tauri::command(rename_all = "snake_case")]
pub async fn work_list(core: State<'_, Arc<Core>>, project_id: Option<ProjectId>) -> Res<Vec<WorkItem>> {
    core.work().list(project_id.as_ref()).await
}

#[tauri::command(rename_all = "snake_case")]
pub async fn work_resume(core: State<'_, Arc<Core>>, id: WorkItemId) -> Res<WorkItem> {
    core.work().resume(&id).await
}

/// `step` is a saga step id; `skip:<step>` marks it skipped and continues the saga.
#[tauri::command(rename_all = "snake_case")]
pub async fn work_retry_step(core: State<'_, Arc<Core>>, id: WorkItemId, step: String) -> Res<WorkItem> {
    core.work().retry_step(&id, &step).await
}

/// Ship from the UI (`origin: ui`, counts as review, FLOW §2.3; the MCP `create_pr` tool is the `mcp` caller).
#[tauri::command(rename_all = "snake_case")]
pub async fn work_create_pr(core: State<'_, Arc<Core>>, id: WorkItemId, draft: PrDraft) -> Res<WorkItem> {
    core.work().create_pr(&id, draft, ShipOrigin::Ui).await?;
    core.work().mark_reviewed(&id).await
}

/// Title, body and draft flag Ship would use (prefills the Ship dialog).
#[tauri::command(rename_all = "snake_case")]
pub async fn work_pr_draft(core: State<'_, Arc<Core>>, id: WorkItemId) -> Res<PrDraft> {
    core.work().pr_draft(&id).await
}

#[tauri::command(rename_all = "snake_case")]
pub async fn work_finish_merged(core: State<'_, Arc<Core>>, ids: Vec<WorkItemId>) -> Res<FinishMergedReport> {
    core.work().finish_merged(&ids).await
}

/// Missed merges / closes of work-item PRs (Now open; also run on startup).
#[tauri::command(rename_all = "snake_case")]
pub async fn work_check_prs(core: State<'_, Arc<Core>>) -> Res<()> {
    core.check_work_prs().await
}

#[tauri::command(rename_all = "snake_case")]
pub async fn work_finish(core: State<'_, Arc<Core>>, id: WorkItemId, opts: FinishOpts) -> Res<WorkItem> {
    core.work().finish(&id, opts).await
}

#[tauri::command(rename_all = "snake_case")]
pub async fn work_status(core: State<'_, Arc<Core>>, id: WorkItemId) -> Res<GitStatus> {
    core.work().status(&id).await
}

/// Prompt (and brief files) into the item's previous Claude conversation (FLOW §4.2).
#[tauri::command(rename_all = "snake_case")]
pub async fn work_send(
    core: State<'_, Arc<Core>>,
    id: WorkItemId,
    prompt: String,
    files: Vec<SendFile>,
    threads: Option<Vec<String>>,
) -> Res<WorkItem> {
    core.work().send(&id, &prompt, files, threads).await
}

#[tauri::command(rename_all = "snake_case")]
pub async fn work_feedback(core: State<'_, Arc<Core>>, id: WorkItemId) -> Res<Feedback> {
    core.work().feedback(&id).await
}

#[tauri::command(rename_all = "snake_case")]
pub async fn work_rerequest_review(core: State<'_, Arc<Core>>, id: WorkItemId) -> Res<Vec<String>> {
    core.work().rerequest_review(&id).await
}

#[tauri::command(rename_all = "snake_case")]
pub async fn work_resolve_sent_threads(core: State<'_, Arc<Core>>, id: WorkItemId) -> Res<WorkItem> {
    core.work().resolve_sent_threads(&id).await
}

#[tauri::command(rename_all = "snake_case")]
pub async fn work_rebase(core: State<'_, Arc<Core>>, id: WorkItemId, op: RebaseOp) -> Res<WorkItem> {
    core.work().rebase(&id, op).await
}

#[tauri::command(rename_all = "snake_case")]
pub async fn work_push(core: State<'_, Arc<Core>>, id: WorkItemId, force: bool) -> Res<WorkItem> {
    core.work().push(&id, force).await
}

#[tauri::command(rename_all = "snake_case")]
pub async fn work_link(
    core: State<'_, Arc<Core>>,
    id: WorkItemId,
    ticket: TicketRef,
    apply_side_effects: bool,
) -> Res<WorkItem> {
    core.work().link(&id, ticket, apply_side_effects).await
}

/// Every unfinished item, keyed by id (one fetch per repo, 5 min floor).
#[tauri::command(rename_all = "snake_case")]
pub async fn work_status_all(core: State<'_, Arc<Core>>) -> Res<BTreeMap<WorkItemId, GitStatus>> {
    core.work().status_all().await
}

/// The review diff session (the UI places it zoomed in the work tab). `delta`: only what changed
/// since the last review; `from`: a review item's reviewed PR head.
#[tauri::command(rename_all = "snake_case")]
pub async fn work_diff(
    core: State<'_, Arc<Core>>,
    id: WorkItemId,
    delta: Option<bool>,
    from: Option<String>,
) -> Res<SessionInfo> {
    core.work().diff(&id, delta.unwrap_or(false), from.as_deref()).await
}

#[tauri::command(rename_all = "snake_case")]
pub async fn work_mark_reviewed(core: State<'_, Arc<Core>>, id: WorkItemId) -> Res<WorkItem> {
    core.work().mark_reviewed(&id).await
}

/// Merge when ready: the host's native auto-merge, then Finish once the merge is seen (#143).
#[tauri::command(rename_all = "snake_case")]
pub async fn work_arm_merge(
    core: State<'_, Arc<Core>>,
    id: WorkItemId,
    method: MergeMethod,
) -> Res<WorkItem> {
    core.work().arm_merge(&id, method).await
}

#[tauri::command(rename_all = "snake_case")]
pub async fn work_disarm_merge(core: State<'_, Arc<Core>>, id: WorkItemId) -> Res<WorkItem> {
    core.work().disarm_merge(&id).await
}

/// Louis's `next:` note on the item (null or blank clears it).
#[tauri::command(rename_all = "snake_case")]
pub async fn work_set_note(
    core: State<'_, Arc<Core>>,
    id: WorkItemId,
    note: Option<String>,
) -> Res<WorkItem> {
    core.work().set_note(&id, note).await
}

/// Louis left the item's tab (the return strip's clock).
#[tauri::command(rename_all = "snake_case")]
pub async fn work_left(core: State<'_, Arc<Core>>, id: WorkItemId) -> Res<WorkItem> {
    core.work().left(&id).await
}

/// Start a queued item now, over `claude.max_live` (#141).
#[tauri::command(rename_all = "snake_case")]
pub async fn work_start_now(core: State<'_, Arc<Core>>, id: WorkItemId) -> Res<WorkItem> {
    core.work().start_now(&id).await
}

/// The queued item starts next.
#[tauri::command(rename_all = "snake_case")]
pub async fn work_queue_front(core: State<'_, Arc<Core>>, id: WorkItemId) -> Res<WorkItem> {
    core.work().queue_front(&id).await
}

//! SQLite row types (ARCHITECTURE §10). The schema and migrations live in `kelta-core::store`;
//! these structs mirror one table row each so every lane and the fakes agree on the persisted shape.
//! JSON columns are kept as `String` (serialized with serde_json).

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::ids::{PluginId, ProjectId, SessionId, WorkItemId};

/// Current schema version (`schema_version(v)`); bumped by L3 migrations.
pub const SCHEMA_VERSION: u32 = 12;

/// Table names.
pub mod tables {
    pub const SCHEMA_VERSION: &str = "schema_version";
    pub const PROJECTS_OPEN: &str = "projects_open";
    pub const LAYOUTS: &str = "layouts";
    pub const SESSIONS: &str = "sessions";
    pub const WORK_ITEMS: &str = "work_items";
    pub const WORK_STEPS: &str = "work_steps";
    pub const SEEN_REVIEWS: &str = "seen_reviews";
    pub const PROVIDER_CACHE: &str = "provider_cache";
    pub const PLUGIN_GRANTS: &str = "plugin_grants";
    pub const PLUGIN_KV: &str = "plugin_kv";
    pub const REPO_TRUST: &str = "repo_trust";
    pub const TRIGGER_LOG: &str = "trigger_log";
    pub const UI_STATE: &str = "ui_state";
    pub const NOTES: &str = "notes";
    pub const NUDGES: &str = "nudges";

    pub const ALL: &[&str] = &[
        SCHEMA_VERSION,
        PROJECTS_OPEN,
        LAYOUTS,
        SESSIONS,
        WORK_ITEMS,
        WORK_STEPS,
        SEEN_REVIEWS,
        PROVIDER_CACHE,
        PLUGIN_GRANTS,
        PLUGIN_KV,
        REPO_TRUST,
        TRIGGER_LOG,
        UI_STATE,
        NOTES,
        NUDGES,
    ];
}

/// `trigger_log` is capped at this many rows.
pub const TRIGGER_LOG_CAP: u32 = 1000;

/// `projects_open(project_id PK, ord, active)` — open set + order; config lives in TOML.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectOpenRow {
    pub project_id: ProjectId,
    pub ord: u32,
    pub active: bool,
}

/// `layouts(project_id PK, json, rev, updated_at)`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LayoutRow {
    pub project_id: ProjectId,
    /// `Layout` JSON.
    pub json: String,
    pub rev: u64,
    pub updated_at: String,
}

/// `sessions(id PK, project_id, kind_json, spec_json, name, work_item_id, restore_json, cwd, lifecycle, text_tail, updated_at)`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionRow {
    pub id: SessionId,
    pub project_id: ProjectId,
    /// `SessionKind` JSON.
    pub kind_json: String,
    /// `SpawnRequest` JSON (relaunch spec).
    pub spec_json: String,
    pub name: String,
    pub work_item_id: Option<WorkItemId>,
    /// `RestorePolicy` JSON.
    pub restore_json: String,
    pub cwd: PathBuf,
    /// `dormant` | `live` | `exited`.
    pub lifecycle: String,
    /// ≤ 200 lines of plain text.
    pub text_tail: Option<String>,
    pub updated_at: String,
}

/// `work_items(id PK, project_id, kind, ticket_json, review_json, repo_id, worktree, branch, base,
/// claude_uuid, nvim_socket, tab_id, pr_url, state_json, created_at, updated_at)`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkItemRow {
    pub id: WorkItemId,
    pub project_id: ProjectId,
    /// `ticket` | `review` | `branch`.
    pub kind: String,
    pub ticket_json: Option<String>,
    pub review_json: Option<String>,
    pub repo_id: String,
    pub worktree: PathBuf,
    pub branch: String,
    pub base: String,
    pub claude_uuid: Option<String>,
    pub nvim_socket: Option<PathBuf>,
    pub tab_id: Option<String>,
    pub pr_url: Option<String>,
    /// `WorkState` JSON.
    pub state_json: String,
    pub created_at: String,
    pub updated_at: String,
}

/// `work_steps(work_item_id, step, status, detail, updated_at, PK(work_item_id, step))`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkStepRow {
    pub work_item_id: WorkItemId,
    pub step: String,
    /// `pending` | `running` | `done` | `failed` | `skipped`.
    pub status: String,
    pub detail: Option<String>,
    pub updated_at: String,
}

/// `seen_reviews(account, repo, number, head_sha, first_seen, reviewed_sha, PK(account, repo, number))`;
/// `reviewed_sha` (the head Louis approved in Kelta) is written by `seen_review_stamp` only.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SeenReviewRow {
    pub account: String,
    pub repo: String,
    pub number: u64,
    pub head_sha: String,
    pub first_seen: String,
}

/// `provider_cache(key PK, etag, body_json, fetched_at)`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderCacheRow {
    pub key: String,
    pub etag: Option<String>,
    pub body_json: String,
    pub fetched_at: String,
}

/// `plugin_grants(plugin_id, permission, granted_at, manifest_sha256, PK(plugin_id, permission))`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PluginGrantRow {
    pub plugin_id: PluginId,
    pub permission: String,
    pub granted_at: String,
    pub manifest_sha256: String,
}

/// `plugin_kv(plugin_id, key, value, PK(plugin_id, key))` — screens' `kv.*` (PLUGINS §7); value is JSON text.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PluginKvRow {
    pub plugin_id: PluginId,
    pub key: String,
    pub value: String,
}

/// `repo_trust(path PK, sha256, trusted_at)`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RepoTrustRow {
    pub path: PathBuf,
    pub sha256: String,
    pub trusted_at: String,
}

/// `trigger_log(id PK, ts, trigger_id, event, ok, detail, depth)` — capped at [`TRIGGER_LOG_CAP`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TriggerLogRow {
    pub id: i64,
    pub ts: String,
    pub trigger_id: String,
    pub event: String,
    pub ok: bool,
    pub detail: Option<String>,
    pub depth: u8,
}

/// `ui_state(key PK, value)` — webgl probe result, onboarding done, window geometry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UiStateRow {
    pub key: String,
    pub value: String,
}

/// Well-known `ui_state` keys.
pub mod ui_state_keys {
    pub const WEBGL_PROBE: &str = "webgl_probe";
    pub const ONBOARDING_DONE: &str = "onboarding_done";
    pub const WINDOW_GEOMETRY: &str = "window_geometry";
}

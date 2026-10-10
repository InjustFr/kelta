//! Service traits — the inter-lane contract (ARCHITECTURE §4, §8.1, §8.2).
//!
//! `ProviderFactory` lives in `kelta_http::provider` because its signature takes `kelta_http::HttpCtx`
//! (proto cannot depend on kelta-http without a cycle). See `docs/contract-requests/S0.md`.

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Arc;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use ts_rs::TS;

use crate::codehost::ReviewItem;
use crate::codehost::{
    CodeHostKind, Feedback, PrCreate, PrDraft, Review, ReviewDetail, ReviewKind, ReviewQuery, ReviewRef,
};
use crate::ctl::CtlCommand;
use crate::error::KeltaError;
use crate::events::{BusEvent, Notification, Toast, UiEvent};
use crate::ext::{ProxiedRequest, ProxiedResponse, ToolHandle};
use crate::ids::{AccountId, PluginId, ProjectId, SessionId, ToolId, WorkItemId};
use crate::ipc::WindowState;
use crate::model::{
    AttachInfo, EditorTarget, OpenPaneRequest, PaneRef, Placement, ProjectInfo, Scope, SessionInfo,
    ShipOrigin, SpawnRequest, StatusChange, StepStatus, TemplateCtx, WorkItem, WorkStepStatus,
};
use crate::secret::{Secret, SecretBackendStatus, SecretCtx, SecretRef};
use crate::settings::TrackerView;
use crate::settings::{ProjectConfig, Settings, TrackerBinding};
use crate::term::{
    HistoryHit, KillSignal, PtySpawnSpec, TerminalEvent, TerminalLimits, TerminalPalette, TerminalStats,
};
use crate::tracker::{
    Assignee, Column, Cursor, Page, Ticket, TicketDetail, TicketRef, TrackerCaps, TrackerKind, Transition,
    User,
};

// ---- terminal (impl: kelta-term) ---------------------------------------------------------------

/// `detach` generation matching any view; real generations start at 1.
pub const ANY_VIEW: u32 = 0;

pub trait TerminalHost: Send + Sync {
    /// Id inside spec.
    fn spawn(&self, spec: PtySpawnSpec) -> Result<(), KeltaError>;
    fn attach(
        &self,
        id: &SessionId,
        cols: u16,
        rows: u16,
        sink: Box<dyn FrameSink>,
    ) -> Result<AttachInfo, KeltaError>;
    /// [`ANY_VIEW`] drops whatever view is attached (the window is gone).
    fn detach(&self, id: &SessionId, generation: u32);
    fn write(&self, id: &SessionId, bytes: &[u8]) -> Result<(), KeltaError>;
    fn resize(&self, id: &SessionId, cols: u16, rows: u16) -> Result<(), KeltaError>;
    fn ack(&self, id: &SessionId, generation: u32, bytes: u32);
    /// Hup|Term|Kill, to the process group.
    fn kill(&self, id: &SessionId, signal: KillSignal) -> Result<(), KeltaError>;
    /// OSC 4/10/11/12 replies.
    fn set_palette(&self, palette: TerminalPalette);
    /// Scrollback per kind, memory cap.
    fn set_limits(&self, limits: TerminalLimits);
    /// Plain text (search, persistence).
    fn text_tail(&self, id: &SessionId, max_lines: u32) -> Result<String, KeltaError>;
    /// Last lines of the on-disk history log (§9.6); works for sessions the host no longer runs.
    fn history_tail(&self, id: &SessionId, max_lines: u32) -> Result<String, KeltaError>;
    /// Case-insensitive substring search over the history logs of `ids`, newest `limit` hits.
    fn history_search(
        &self,
        ids: &[SessionId],
        query: &str,
        limit: u32,
    ) -> Result<Vec<HistoryHit>, KeltaError>;
    /// Delete a session's history log (the session row is gone).
    fn history_delete(&self, id: &SessionId);
    /// Per-session bytes, lines, inflight.
    fn stats(&self) -> TerminalStats;
    /// The host outlives the app (keltad): quit leaves restorable sessions running.
    fn persistent(&self) -> bool {
        false
    }
    /// After an app restart, route a still-running session's events to `events`. Returns the
    /// environment it was spawned with (hook tokens), `Ok(None)` when no such session is running,
    /// `Err` when the host could not tell (it may still be running).
    fn adopt(
        &self,
        _id: &SessionId,
        _events: Arc<dyn TerminalEvents>,
    ) -> Result<Option<BTreeMap<String, String>>, KeltaError> {
        Ok(None)
    }
}

/// Receives encoded frames for one attached view. `false` = channel closed → auto-detach.
pub trait FrameSink: Send {
    fn send(&mut self, frame: Vec<u8>) -> bool;
}

pub trait TerminalEvents: Send + Sync {
    fn on_event(&self, id: &SessionId, ev: TerminalEvent);
}

// ---- providers (impl: kelta-trackers / kelta-codehosts) ----------------------------------------

#[async_trait]
pub trait Tracker: Send + Sync {
    fn kind(&self) -> TrackerKind;
    fn caps(&self) -> TrackerCaps;
    async fn me(&self) -> Result<User, KeltaError>;
    async fn list(&self, view: &TrackerView, cursor: Option<Cursor>) -> Result<Page<Ticket>, KeltaError>;
    /// body_md + body_html (sanitized), last 20 comments.
    async fn get(&self, t: &TicketRef) -> Result<TicketDetail, KeltaError>;
    async fn columns(&self, b: &TrackerBinding) -> Result<Vec<Column>, KeltaError>;
    async fn transitions(&self, t: &TicketRef) -> Result<Vec<Transition>, KeltaError>;
    async fn transition(
        &self,
        t: &TicketRef,
        transition_id: &str,
        fields: Option<Value>,
    ) -> Result<Ticket, KeltaError>;
    async fn comment(&self, t: &TicketRef, markdown: &str) -> Result<(), KeltaError>;
    async fn assign(&self, t: &TicketRef, who: Assignee) -> Result<Ticket, KeltaError>;
    fn browser_url(&self, t: &TicketRef) -> String;
    /// `"SHOP-123"` | `"4567"` | `"gh-12"` | `"gl-12"`.
    fn branch_key(&self, t: &TicketRef) -> String;
}

#[async_trait]
pub trait CodeHost: Send + Sync {
    fn kind(&self) -> CodeHostKind;
    async fn me(&self) -> Result<User, KeltaError>;
    /// Cheap gate; default `Ok(true)`.
    async fn changed_since_last(&self) -> Result<bool, KeltaError> {
        Ok(true)
    }
    async fn list_reviews(&self, q: &ReviewQuery) -> Result<Vec<Review>, KeltaError>;
    async fn get(&self, r: &ReviewRef) -> Result<ReviewDetail, KeltaError>;
    /// `approve`, `comment` and `request_changes` also publish my pending review (see
    /// `add_pending_comment`), so its line comments go out with the decision.
    async fn approve(&self, r: &ReviewRef, head_sha: &str) -> Result<(), KeltaError>;
    async fn comment(&self, r: &ReviewRef, body: &str) -> Result<(), KeltaError>;
    async fn request_changes(&self, r: &ReviewRef, body: &str) -> Result<(), KeltaError>;
    /// Adds a line comment (new-side `line` of `path`) to my pending (draft) review, creating it.
    async fn add_pending_comment(
        &self,
        _r: &ReviewRef,
        _path: &str,
        _line: u32,
        _body: &str,
    ) -> Result<(), KeltaError> {
        Err(KeltaError::unsupported("pending review comments"))
    }
    async fn create(&self, d: &PrCreate) -> Result<Review, KeltaError>;
    /// Renames a PR (the ticket key added after Link to ticket, FLOW §4.3 step 4).
    async fn update_title(&self, _r: &ReviewRef, _title: &str) -> Result<(), KeltaError> {
        Err(KeltaError::unsupported("this code host cannot rename pull requests"))
    }
    async fn find_for_branch(&self, repo: &str, branch: &str) -> Result<Option<Review>, KeltaError>;
    /// `pull/N/head:…` | `merge-requests/N/head:…`.
    fn fetch_refspec(&self, r: &ReviewRef, local_branch: &str) -> String;
    fn repo_from_remote(&self, url: &str) -> Option<String>;
    /// Unresolved threads, review summaries and failed checks of a PR (Fix with Claude).
    async fn feedback(&self, _r: &ReviewRef) -> Result<Feedback, KeltaError> {
        Err(KeltaError::unsupported("this code host cannot read review feedback"))
    }
    /// Ask everyone who reviewed to review again; returns their logins.
    async fn rerequest_review(&self, _r: &ReviewRef) -> Result<Vec<String>, KeltaError> {
        Err(KeltaError::unsupported("this code host cannot re-request reviews"))
    }
    /// Resolve review threads by id (`FeedbackThread::id`).
    async fn resolve_threads(&self, _r: &ReviewRef, _ids: &[String]) -> Result<(), KeltaError> {
        Err(KeltaError::unsupported("this code host cannot resolve threads"))
    }
}

// ---- config / secrets (impl: kelta-config / kelta-secrets) ------------------------------------

pub trait SettingsSource: Send + Sync {
    /// Fully merged, validated.
    fn effective(&self, project: Option<&ProjectId>) -> Arc<Settings>;
    fn project(&self, id: &ProjectId) -> Option<Arc<ProjectConfig>>;
    fn projects(&self) -> Vec<Arc<ProjectConfig>>;
}

#[async_trait]
pub trait SecretResolver: Send + Sync {
    async fn resolve(&self, r: &SecretRef, ctx: &SecretCtx) -> Result<Secret, KeltaError>;
    /// keyring refs only.
    async fn set(&self, r: &SecretRef, value: &str) -> Result<(), KeltaError>;
    async fn delete(&self, r: &SecretRef) -> Result<(), KeltaError>;
    async fn backends_status(&self) -> Vec<SecretBackendStatus>;
    fn invalidate(&self, r: &SecretRef);
}

/// Implemented by `PluginHost`; handed by core to `ConfigService::set_plugin_schemas`.
pub trait PluginSettingsSource: Send + Sync {
    /// `(plugin id, flat JSON Schema)` of every enabled plugin with settings.
    fn fragments(&self) -> Vec<(PluginId, Value)>;
}

// ---- core API (impl: kelta-core) consumed by work, server, plugins ------------------------------

#[async_trait]
pub trait CoreApi: Send + Sync {
    // sessions & layout
    async fn session_spawn(&self, req: SpawnRequest) -> Result<SessionInfo, KeltaError>;
    async fn session_write(&self, id: &SessionId, bytes: &[u8]) -> Result<(), KeltaError>;
    async fn session_kill(&self, id: &SessionId, force: bool) -> Result<(), KeltaError>;
    fn session_get(&self, id: &SessionId) -> Option<SessionInfo>;
    fn session_list(&self, project: Option<&ProjectId>) -> Vec<SessionInfo>;
    async fn session_apply_hook(&self, id: &SessionId, change: StatusChange) -> Result<(), KeltaError>;
    /// New tab / split / focus.
    async fn layout_open(&self, project: &ProjectId, req: OpenPaneRequest) -> Result<PaneRef, KeltaError>;
    // projects & settings
    fn project(&self, id: &ProjectId) -> Option<ProjectInfo>;
    fn settings(&self, project: Option<&ProjectId>) -> Arc<Settings>;
    // providers
    async fn tracker_for(&self, account: &AccountId) -> Result<Arc<dyn Tracker>, KeltaError>;
    async fn code_host_for(&self, account: &AccountId) -> Result<Arc<dyn CodeHost>, KeltaError>;
    async fn review_list(&self, scope: Scope, kind: ReviewKind) -> Result<Vec<ReviewItem>, KeltaError>;
    /// Tracker writes with core-owned side effects (cache invalidation, `ticket.transitioned` /
    /// `ticket.commented`); `session` = the session acting (MCP), added as event context.
    async fn ticket_transition(
        &self,
        ticket: &TicketRef,
        transition_id: &str,
        fields: Option<Value>,
        session: Option<&SessionId>,
    ) -> Result<Ticket, KeltaError>;
    async fn ticket_comment(
        &self,
        ticket: &TicketRef,
        markdown: &str,
        session: Option<&SessionId>,
    ) -> Result<(), KeltaError>;
    // work & editor (core delegates to kelta-work)
    async fn work_for_session(&self, id: &SessionId) -> Option<WorkItem>;
    async fn work_create_pr(
        &self,
        id: &WorkItemId,
        draft: PrDraft,
        origin: ShipOrigin,
    ) -> Result<WorkItem, KeltaError>;
    /// Review feedback of the work item's PR (MCP `get_review_feedback`).
    async fn work_feedback(&self, id: &WorkItemId) -> Result<Feedback, KeltaError>;
    async fn editor_open(
        &self,
        target: EditorTarget,
        path: &Path,
        line: Option<u32>,
    ) -> Result<(), KeltaError>;
    /// Claude IDE bridge `openDiff`: `old` next to `proposed` in the editor (nvim RPC only);
    /// `close` closes that diff again.
    async fn editor_diff(
        &self,
        target: EditorTarget,
        old: &Path,
        proposed: &Path,
        close: bool,
    ) -> Result<(), KeltaError>;
    // tools / plugins / bus / ui
    async fn tool_open(
        &self,
        project: &ProjectId,
        tool: &ToolId,
        ctx: TemplateCtx,
        placement: Placement,
    ) -> Result<ToolHandle, KeltaError>;
    fn publish(&self, ev: BusEvent);
    fn subscribe(&self) -> tokio::sync::broadcast::Receiver<BusEvent>;
    async fn notify(&self, n: Notification) -> Result<(), KeltaError>;
    fn toast(&self, t: Toast);
    /// Plugin `net:` (allowlist checked by caller).
    async fn http_fetch(&self, req: ProxiedRequest) -> Result<ProxiedResponse, KeltaError>;
    /// `PATH` of the user's login shell (what pty sessions search); `None` when unknown.
    fn login_path(&self) -> Option<String> {
        None
    }
    /// Dispatch of ctl socket commands.
    async fn ctl(&self, cmd: CtlCommand) -> Result<Value, KeltaError>;
}

// ---- UI bridge (impl: apps/desktop window/bridge.rs) consumed by kelta-core ---------------------

pub trait UiBridge: Send + Sync {
    /// Fan-out to all `events_subscribe` channels; dropped if none.
    fn emit(&self, ev: UiEvent);
    /// macOS dock badge; Linux no-op.
    fn set_badge(&self, needs_input: u32);
    /// Linux urgency hint / macOS bounce (informational).
    fn request_attention(&self);
    /// tauri-plugin-notification.
    fn notify(&self, n: Notification) -> Result<(), KeltaError>;
    /// `{exists, visible, focused}`.
    fn window_state(&self) -> WindowState;
    /// Hang/crash recovery (§12.4).
    fn reload_webview(&self, safe: bool);
    /// WebKit helper pids for `perf_snapshot` (macOS WebContent etc.); default none.
    fn webview_pids(&self) -> Vec<u32> {
        Vec::new()
    }
}

// ---- stores (impl: kelta-core over SQLite; in-memory fakes in `testing`) ------------------------

#[async_trait]
pub trait WorkStore: Send + Sync {
    async fn put_item(&self, item: &WorkItem) -> Result<(), KeltaError>;
    async fn get_item(&self, id: &WorkItemId) -> Result<Option<WorkItem>, KeltaError>;
    async fn list_items(&self, project: Option<&ProjectId>) -> Result<Vec<WorkItem>, KeltaError>;
    async fn delete_item(&self, id: &WorkItemId) -> Result<(), KeltaError>;
    async fn set_step(
        &self,
        id: &WorkItemId,
        step: &str,
        status: StepStatus,
        detail: Option<String>,
    ) -> Result<(), KeltaError>;
    async fn steps(&self, id: &WorkItemId) -> Result<Vec<WorkStepStatus>, KeltaError>;
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct PluginGrant {
    pub permission: String,
    pub granted_at: String,
    pub manifest_sha256: String,
}

#[async_trait]
pub trait GrantStore: Send + Sync {
    async fn grants(&self, plugin: &PluginId) -> Result<Vec<PluginGrant>, KeltaError>;
    /// Adds grants (existing ones are kept, their manifest hash updated).
    async fn grant(
        &self,
        plugin: &PluginId,
        permissions: &[String],
        manifest_sha256: &str,
    ) -> Result<(), KeltaError>;
    async fn revoke_all(&self, plugin: &PluginId) -> Result<(), KeltaError>;

    // Plugin KV (`plugin_kv`, PLUGINS §7 `kv.*`): per-plugin state, so it lives next to the grants.
    /// The stored JSON text of `key`.
    async fn kv_get(&self, plugin: &PluginId, key: &str) -> Result<Option<String>, KeltaError>;
    /// Upsert. `InvalidArgument` (nothing written) when the plugin's total key + value bytes would
    /// exceed `quota`.
    async fn kv_set(
        &self,
        plugin: &PluginId,
        key: &str,
        value: String,
        quota: usize,
    ) -> Result<(), KeltaError>;
    async fn kv_delete(&self, plugin: &PluginId, key: &str) -> Result<(), KeltaError>;
    /// Keys, sorted.
    async fn kv_keys(&self, plugin: &PluginId) -> Result<Vec<String>, KeltaError>;
    /// Every key of `plugin` (uninstall).
    async fn kv_clear(&self, plugin: &PluginId) -> Result<(), KeltaError>;
}

/// The `kv_set` quota error.
pub fn kv_quota_error(quota: usize) -> KeltaError {
    KeltaError::invalid(format!("kv.set: plugin storage is limited to {quota} bytes"))
}

#[async_trait]
pub trait TrustStore: Send + Sync {
    /// SHA-256 recorded for a repo-local config path, if trusted.
    async fn trusted_hash(&self, path: &Path) -> Result<Option<String>, KeltaError>;
    /// `Some(hash)` trusts, `None` revokes.
    async fn set_trust(&self, path: &Path, sha256: Option<String>) -> Result<(), KeltaError>;
}

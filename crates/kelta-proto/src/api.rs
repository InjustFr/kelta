//! Service traits — the inter-lane contract (ARCHITECTURE §4, §8.1, §8.2).
//!
//! `ProviderFactory` lives in `kelta_http::provider` because its signature takes `kelta_http::HttpCtx`
//! (proto cannot depend on kelta-http without a cycle). See `docs/contract-requests/S0.md`.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use ts_rs::TS;

use crate::codehost::{
    CodeHostKind, Feedback, PrCreate, PrDraft, Review, ReviewDetail, ReviewKind, ReviewQuery, ReviewRef,
};
use crate::codehost::{MergeMethod, ReviewItem};
use crate::ctl::CtlCommand;
use crate::error::KeltaError;
use crate::events::{BusEvent, Notification, Toast, UiEvent};
use crate::ext::{ProxiedRequest, ProxiedResponse, ToolHandle};
use crate::ids::{AccountId, PluginId, ProjectId, SessionId, ToolId, WorkItemId};
use crate::ipc::{AccountTestResult, WindowState};
use crate::model::{
    AttachInfo, ClaudeUsage, EditorTarget, OpenPaneRequest, PaneRef, Placement, ProjectDraft, ProjectInfo,
    ReviewNote, RunClaim, RunState, Scope, SessionInfo, ShipOrigin, SpawnRequest, StatusChange, StepStatus,
    TemplateCtx, TicketStep, WorkItem, WorkStepStatus, WorkflowStart,
};
use crate::secret::{Secret, SecretBackendStatus, SecretCtx, SecretRef};
use crate::settings::TrackerView;
use crate::settings::{BlockedMarker, Layer, ProjectConfig, Settings, SourceField, TrackerBinding};
use crate::term::{
    HistoryHit, KillSignal, PtySpawnSpec, TerminalEvent, TerminalLimits, TerminalPalette, TerminalStats,
};
use crate::tracker::{
    Assignee, Column, Cursor, Page, SourceHit, Ticket, TicketDetail, TicketItem, TicketPage, TicketRef,
    TrackerCaps, TrackerKind, Transition, User,
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
    /// Quit with "stop all sessions" (#305): kill what the host still runs and end it (keltad).
    fn shutdown(&self) {}
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
    /// Ticket sources (boards, projects, filters...) matching `query`, as ready views.
    async fn sources(&self, _query: &str) -> Result<Vec<SourceHit>, KeltaError> {
        Err(KeltaError::unsupported("this tracker cannot list sources"))
    }
    /// First page of `view`'s tickets matching the free `text`, searched by the provider (not the cache).
    async fn search(&self, _view: &TrackerView, _text: &str) -> Result<Vec<Ticket>, KeltaError> {
        Err(KeltaError::unsupported("this tracker cannot search"))
    }

    /// The fields `view`'s tickets can show (`Source.fields`, #351): id and the tracker's name.
    async fn fields(&self, _view: &TrackerView) -> Result<Vec<SourceField>, KeltaError> {
        Err(KeltaError::unsupported("this tracker cannot list its fields"))
    }

    /// Users `t` can be assigned to whose name or login matches `query` (empty = the first ones) (Tickets T8).
    async fn assignable_users(&self, _t: &TicketRef, _query: &str) -> Result<Vec<User>, KeltaError> {
        Err(KeltaError::unsupported("this tracker cannot list assignable users"))
    }

    /// Priority names `t` can take, highest first (Tickets T8).
    async fn priorities(&self, _t: &TicketRef) -> Result<Vec<String>, KeltaError> {
        Err(KeltaError::unsupported("this tracker cannot set priorities"))
    }

    /// Sets `t`'s priority to one of [`Self::priorities`], by name.
    async fn set_priority(&self, _t: &TicketRef, _priority: &str) -> Result<Ticket, KeltaError> {
        Err(KeltaError::unsupported("this tracker cannot set priorities"))
    }

    /// Replaces `t`'s description (MCP `update_ticket_body`).
    async fn set_body(&self, _t: &TicketRef, _body_md: &str) -> Result<(), KeltaError> {
        Err(KeltaError::unsupported("this tracker cannot edit ticket descriptions"))
    }

    /// Sets or clears `marker` on `t` (flag or label; kelta-core moves statuses itself) (#348).
    async fn set_blocked(
        &self,
        _t: &TicketRef,
        _marker: &BlockedMarker,
        _blocked: bool,
    ) -> Result<Ticket, KeltaError> {
        Err(KeltaError::unsupported("this tracker cannot set this blocked marker"))
    }

    /// Closes `t`, on trackers where closing is apart from the board status (MCP `close_ticket`).
    async fn close(&self, _t: &TicketRef) -> Result<Ticket, KeltaError> {
        Err(KeltaError::unsupported("this tracker has no close apart from its statuses"))
    }

    /// Files a new ticket in the project (repo, team...) that `project` lists (Tickets T11).
    async fn create(
        &self,
        _project: &TrackerView,
        _title: &str,
        _body_md: &str,
    ) -> Result<Ticket, KeltaError> {
        Err(KeltaError::unsupported("this tracker cannot create tickets"))
    }
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
    /// Every open PR of `repos` (`ReviewKind::Team`: no `my_state` / `reviewed_head`, `waiting_on`
    /// filled like authored). A separate call so a host that lacks it never answers with my PRs.
    async fn list_open(&self, _repos: &[String], _include_drafts: bool) -> Result<Vec<Review>, KeltaError> {
        Err(KeltaError::unsupported("can't list team PRs"))
    }
    async fn get(&self, r: &ReviewRef) -> Result<ReviewDetail, KeltaError>;
    /// `approve`, `comment` and `request_changes` also publish my pending review (see
    /// `add_pending_comment`), so its line comments go out with the decision.
    async fn approve(&self, r: &ReviewRef, head_sha: &str) -> Result<(), KeltaError>;
    async fn comment(&self, r: &ReviewRef, body: &str) -> Result<(), KeltaError>;
    async fn request_changes(&self, r: &ReviewRef, body: &str) -> Result<(), KeltaError>;
    /// A plain conversation comment that leaves my pending review / draft notes unpublished.
    async fn post_note(&self, r: &ReviewRef, body: &str) -> Result<(), KeltaError> {
        self.comment(r, body).await
    }
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
    /// Ask `who` (`Review::waiting_on` entries) to review again, or everyone who reviewed when
    /// `who` is empty; returns the logins asked.
    async fn rerequest_review(&self, _r: &ReviewRef, _who: &[String]) -> Result<Vec<String>, KeltaError> {
        Err(KeltaError::unsupported("this code host cannot re-request reviews"))
    }
    /// Resolve review threads by id (`FeedbackThread::id`).
    async fn resolve_threads(&self, _r: &ReviewRef, _ids: &[String]) -> Result<(), KeltaError> {
        Err(KeltaError::unsupported("this code host cannot resolve threads"))
    }
    /// Native auto-merge: the host merges once approvals and checks pass.
    async fn arm_auto_merge(&self, _r: &ReviewRef, _method: MergeMethod) -> Result<(), KeltaError> {
        Err(KeltaError::unsupported("this code host cannot merge when ready"))
    }
    async fn disarm_auto_merge(&self, _r: &ReviewRef) -> Result<(), KeltaError> {
        Err(KeltaError::unsupported("this code host cannot merge when ready"))
    }
    /// Mark ready for review (`draft` false) or convert back to a draft.
    async fn set_draft(&self, _r: &ReviewRef, _draft: bool) -> Result<(), KeltaError> {
        Err(KeltaError::unsupported("this code host cannot change a pull request's draft state"))
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
    /// Last statusline snapshot of a Claude session (`kelta-ctl statusline`).
    async fn session_set_usage(&self, _id: &SessionId, _usage: ClaudeUsage) -> Result<(), KeltaError> {
        Ok(())
    }
    /// MCP `ask` (#356): show the question on the session (needs input) until its next input.
    /// `body_md` comes from the session: core renders and sanitizes it.
    fn session_ask(
        &self,
        id: &SessionId,
        question: String,
        options: Vec<String>,
        body_md: Option<String>,
    ) -> Result<(), KeltaError>;
    /// Every hook of a session, for its Claude `Workflow` runs (#272): core re-reads the runs the
    /// hook concerns and emits `workflow.updated`; and for its task list (#406, `ClaudeMeta.tasks`).
    /// Never fails: the files are Claude internals.
    fn session_claude_runs_hook(&self, _id: &SessionId, _payload: &crate::hooks::HookPayload) {}
    /// The last permission mode a Claude session switched to (#277): sessions whose profile sets
    /// none start in it; `None` = none seen, Claude's own default applies.
    fn claude_permission_mode(&self) -> Option<crate::settings::PermissionMode> {
        None
    }
    /// New tab / split / focus.
    async fn layout_open(&self, project: &ProjectId, req: OpenPaneRequest) -> Result<PaneRef, KeltaError>;
    // projects & settings
    fn project(&self, id: &ProjectId) -> Option<ProjectInfo>;
    fn settings(&self, project: Option<&ProjectId>) -> Arc<Settings>;
    /// Every project, Home included (MCP setup tools, #343).
    fn project_list(&self) -> Vec<ProjectInfo> {
        Vec::new()
    }
    /// MCP `add_project`: `project_detect` + `project_create`, as the New project sheet with its
    /// suggestions; the draft carries the detected code host / tracker hints.
    fn project_add(&self, _path: &Path) -> Result<(ProjectInfo, ProjectDraft), KeltaError> {
        Err(KeltaError::unsupported("project_add"))
    }
    /// MCP `set_setting`: `settings_set` at `layer` (Global or Project), the same validated write as
    /// Settings; `null` resets the key.
    fn settings_write(
        &self,
        _layer: Layer,
        _project: Option<&ProjectId>,
        _path: &str,
        _value: Value,
    ) -> Result<(), KeltaError> {
        Err(KeltaError::unsupported("settings_write"))
    }
    /// Settings → Accounts "Test" and MCP `test_account`: the account's provider `me()`; provider
    /// failures are data, not errors.
    async fn account_test(&self, _account: &AccountId) -> Result<AccountTestResult, KeltaError> {
        Err(KeltaError::unsupported("account_test"))
    }
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
    /// Kelta's Tickets list (cached, merged views, with `work_item_id`, `prs`, `caps`); `view_id`
    /// narrows to one view, `refresh` skips the cache (MCP `list_tickets`).
    async fn ticket_list(
        &self,
        _scope: Scope,
        _view_id: Option<String>,
        _cursor: Option<Cursor>,
        _refresh: bool,
    ) -> Result<TicketPage, KeltaError> {
        Ok(TicketPage::default())
    }
    /// My local tags (#349; MCP `list_tickets` filter `tag`, read-only).
    async fn local_tags(&self) -> Result<crate::model::LocalTags, KeltaError> {
        Ok(crate::model::LocalTags::default())
    }
    /// The listed ticket `key` names (on `account` when given) with its projects, without the
    /// list's enrichment (MCP by-key tools).
    async fn ticket_by_key(
        &self,
        key: &str,
        account: Option<&AccountId>,
    ) -> Result<Option<TicketItem>, KeltaError> {
        let page = self.ticket_list(Scope::All, None, None, false).await?;
        Ok(page
            .items
            .into_iter()
            .find(|i| i.ticket.r#ref.key == key && account.is_none_or(|a| &i.ticket.r#ref.account == a)))
    }
    async fn ticket_set_body(&self, ticket: &TicketRef, body_md: &str) -> Result<(), KeltaError> {
        self.tracker_for(&ticket.account).await?.set_body(ticket, body_md).await
    }
    async fn ticket_close(&self, ticket: &TicketRef) -> Result<Ticket, KeltaError> {
        self.tracker_for(&ticket.account).await?.close(ticket).await
    }
    /// `ticket.assigned` + cache invalidation, like `ticket_transition`.
    async fn ticket_assign(&self, _ticket: &TicketRef, _who: Assignee) -> Result<Ticket, KeltaError> {
        Err(KeltaError::unsupported("ticket_assign"))
    }
    /// A ticket with its `prs` and `caps`, like Kelta's detail view.
    async fn ticket_get(&self, _ticket: &TicketRef) -> Result<TicketDetail, KeltaError> {
        Err(KeltaError::unsupported("ticket_get"))
    }
    // work & editor (core delegates to kelta-work)
    async fn work_for_session(&self, id: &SessionId) -> Option<WorkItem>;
    /// The ticket MCP `get_ticket` reads for `id`: its work item's, or the ticket a refine runs on (T10).
    async fn ticket_for_session(&self, id: &SessionId) -> Option<TicketRef> {
        self.work_for_session(id).await.and_then(|w| w.ticket)
    }
    async fn work_get(&self, _id: &WorkItemId) -> Option<WorkItem> {
        None
    }
    /// Work items of `project`, every project for `None`; newest first.
    async fn work_list(&self, _project: Option<&ProjectId>) -> Result<Vec<WorkItem>, KeltaError> {
        Ok(Vec::new())
    }
    async fn work_create_pr(
        &self,
        id: &WorkItemId,
        draft: PrDraft,
        origin: ShipOrigin,
    ) -> Result<WorkItem, KeltaError>;
    /// A PR from the review cache, fetched when cold (plugin action `{pr.*}` vars from a ref, #267).
    async fn review(&self, _r: &ReviewRef) -> Option<Review> {
        None
    }
    /// A PR from the review feed cache by URL, never fetched (`context.md`); `None` when not cached.
    async fn cached_review(&self, _url: &str) -> Option<Review> {
        None
    }
    /// Review feedback of the work item's PR (MCP `get_review_feedback`).
    async fn work_feedback(&self, id: &WorkItemId) -> Result<Feedback, KeltaError>;
    /// The `fix_checks` trigger action (#252): `review`'s failed checks to the Claude of the work item
    /// owning it, once per head SHA and failed-check set; `Ok` says what was sent or why nothing was.
    async fn work_fix_checks(&self, _review: &Review) -> Result<String, KeltaError> {
        Err(KeltaError::unsupported("work_fix_checks"))
    }
    /// The `run_workflow` trigger action (#488) and MCP tool (#500): `workflow_run` on `ticket` (claims it; a live run on it
    /// comes back `running`), else `workflow_run_on` of the `on = "global"` entry in `project`.
    async fn workflow_start(
        &self,
        _project: Option<&ProjectId>,
        _workflow: &str,
        _ticket: Option<TicketRef>,
    ) -> Result<WorkflowStart, KeltaError> {
        Err(KeltaError::unsupported("workflow_start"))
    }
    /// MCP `claim_ticket` (#235): a work item for `ticket` adopting `worktree` (default: the project's
    /// primary repo) and `branch`, claimed by `run`. `Conflict` naming the holder when it is taken.
    /// `adopt` false: the caller's Claude is never given to the item (a multi-ticket orchestrator, #497).
    async fn work_claim(
        &self,
        _project: &ProjectId,
        _ticket: TicketRef,
        _worktree: Option<PathBuf>,
        _branch: Option<String>,
        _run: RunClaim,
        _adopt: bool,
    ) -> Result<WorkItem, KeltaError> {
        Err(KeltaError::unsupported("work_claim"))
    }
    /// MCP `report_progress`: the live claim on ticket `key`; `None` keeps the phase / note / sha.
    async fn work_report(
        &self,
        _key: &str,
        _phase: Option<String>,
        _state: RunState,
        _note: Option<String>,
        _sha: Option<String>,
        _caller: Option<&SessionId>,
    ) -> Result<WorkItem, KeltaError> {
        Err(KeltaError::unsupported("work_report"))
    }
    /// MCP `release_ticket`: finishes the claimed item without touching its worktree or branch.
    /// A claim owned by a session answers only to that session (`caller` `None`: the global endpoint).
    async fn work_release(
        &self,
        _key: &str,
        _sha: Option<String>,
        _caller: Option<&SessionId>,
    ) -> Result<WorkItem, KeltaError> {
        Err(KeltaError::unsupported("work_release"))
    }
    /// MCP `link_pr` (#453): the work item of `ticket` adopting `pr`'s head branch, created without
    /// Claude or reused; `bool` is false when it was linked already (no change); `Conflict` naming the item that holds the ticket or the PR.
    async fn work_link_pr(
        &self,
        _project: &ProjectId,
        _ticket: TicketRef,
        _pr: Review,
    ) -> Result<(WorkItem, bool), KeltaError> {
        Err(KeltaError::unsupported("work_link_pr"))
    }
    /// Manual step move (#463, MCP `move_ticket_step`): to `step_id`, `None` = the step after the current one.
    async fn ticket_step_set(
        &self,
        _ticket: &TicketRef,
        _step_id: Option<&str>,
    ) -> Result<TicketStep, KeltaError> {
        Err(KeltaError::unsupported("ticket_step_set"))
    }
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
    /// Writes `text` to the system clipboard (`work_send` to a Claude without Kelta's hooks).
    async fn clipboard_copy(&self, _text: String) -> Result<(), KeltaError> {
        Err(KeltaError::unsupported("clipboard"))
    }
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
    /// `{exists, visible, focused}`, any-of over every window (main and detached).
    fn window_state(&self) -> WindowState;
    /// The main window alone shows the active tab; detached windows show their own tab.
    fn main_window_visible(&self) -> bool {
        self.window_state().visible
    }
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
    async fn set_step(
        &self,
        id: &WorkItemId,
        step: &str,
        status: StepStatus,
        detail: Option<String>,
    ) -> Result<(), KeltaError>;
    async fn steps(&self, id: &WorkItemId) -> Result<Vec<WorkStepStatus>, KeltaError>;
    /// Review notes of an item, by path then line.
    async fn notes(&self, id: &WorkItemId) -> Result<Vec<ReviewNote>, KeltaError>;
    /// Insert (`id` 0) or update a note; returns its id.
    async fn put_note(&self, note: &ReviewNote) -> Result<i64, KeltaError>;
    /// `claim_ticket` in one step (#235): puts `item` per [`WorkItem::claim_over`] against the unfinished
    /// item of its ticket; `Ok(Err(holder))` when that item holds the ticket.
    async fn claim_item(&self, item: &WorkItem) -> Result<Result<WorkItem, Box<WorkItem>>, KeltaError>;
    /// Where `ticket` stands in its pipeline (#461), by account and key.
    async fn ticket_step(&self, ticket: &TicketRef) -> Result<Option<TicketStep>, KeltaError>;
    /// Insert or replace the ticket's row.
    async fn put_ticket_step(&self, step: &TicketStep) -> Result<(), KeltaError>;
    /// Every stored ticket step (#462, `ticket_steps`). SQLite keys rows by account and key, so `ticket.id` is empty there.
    async fn ticket_steps(&self) -> Result<Vec<TicketStep>, KeltaError>;
}

/// The unfinished item of `item`'s ticket, other than `item` itself (`WorkStore::claim_item`).
pub fn claim_holder<'a>(items: impl IntoIterator<Item = &'a WorkItem>, item: &WorkItem) -> Option<WorkItem> {
    let t = item.ticket.as_ref()?;
    items
        .into_iter()
        .find(|w| {
            w.id != item.id
                && w.state != crate::model::WorkState::Finished
                && w.ticket.as_ref().is_some_and(|x| x.account == t.account && x.key == t.key)
        })
        .cloned()
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

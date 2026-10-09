//! UI events (ARCHITECTURE §6.2), bus events (§6.3, PLUGINS §6), toasts and notifications.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::codehost::ReviewRef;
use crate::ctl::CtlCommand;
use crate::ext::Urgency;
use crate::ids::{AccountId, ProjectId, ScreenInstanceId, SessionId, WorkItemId};
use crate::model::{Attention, Layout, OpenPaneRequest, ProjectInfo, Scope, SessionInfo, WorkItem};
use crate::settings::Layer;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, TS, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ToastLevel {
    #[default]
    Info,
    Warn,
    Error,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
pub struct ToastAction {
    pub label: String,
    /// ActionId or a command string handled by the UI action registry.
    pub command: String,
    #[serde(default)]
    pub args: Option<serde_json::Value>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
pub struct Toast {
    pub level: ToastLevel,
    pub text: String,
    #[serde(default)]
    pub action: Option<ToastAction>,
}

impl Toast {
    pub fn info(text: impl Into<String>) -> Self {
        Self { level: ToastLevel::Info, text: text.into(), action: None }
    }
    pub fn warn(text: impl Into<String>) -> Self {
        Self { level: ToastLevel::Warn, text: text.into(), action: None }
    }
    pub fn error(text: impl Into<String>) -> Self {
        Self { level: ToastLevel::Error, text: text.into(), action: None }
    }
}

/// Desktop notification request (core → `UiBridge::notify`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
pub struct Notification {
    pub title: String,
    pub body: Option<String>,
    pub urgency: Urgency,
    /// Click target.
    pub project_id: Option<ProjectId>,
    pub session_id: Option<SessionId>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum AccountStatus {
    Ok,
    NeedsAuth,
    RateLimited,
    Offline,
    Error,
}

/// The single UI event channel (`events_subscribe`). Serde tag `type`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(tag = "type")]
pub enum UiEvent {
    #[serde(rename = "session.updated")]
    SessionUpdated { session: SessionInfo },
    #[serde(rename = "session.removed")]
    SessionRemoved { id: SessionId },
    #[serde(rename = "attention.changed")]
    AttentionChanged {
        project_id: ProjectId,
        level: Attention,
        needs_input_count: u32,
        total_needs_input: u32,
    },
    #[serde(rename = "project.updated")]
    ProjectUpdated { project: ProjectInfo },
    #[serde(rename = "project.removed")]
    ProjectRemoved { id: ProjectId },
    /// Only for backend-initiated changes.
    #[serde(rename = "layout.changed")]
    LayoutChanged { project_id: ProjectId, layout: Layout },
    #[serde(rename = "tickets.changed")]
    TicketsChanged { scope: Scope },
    #[serde(rename = "reviews.changed")]
    ReviewsChanged { scope: Scope, new_keys: Vec<ReviewRef> },
    #[serde(rename = "work.updated")]
    WorkUpdated { work: WorkItem },
    #[serde(rename = "settings.changed")]
    SettingsChanged { layers: Vec<Layer>, paths: Vec<String>, requires_restart: Vec<String> },
    #[serde(rename = "account.status")]
    AccountStatusChanged {
        account_id: AccountId,
        status: AccountStatus,
        #[serde(default)]
        detail: Option<String>,
    },
    #[serde(rename = "toast")]
    Toast { toast: Toast },
    /// Relayed bus events granted to a screen.
    #[serde(rename = "plugin.event")]
    PluginEvent { instance_id: ScreenInstanceId, name: String, payload: serde_json::Value },
    #[serde(rename = "ctl.command")]
    CtlCommand { cmd: CtlCommand },
    /// Backend asks the UI to open a pane.
    #[serde(rename = "ui.open")]
    UiOpen { request: OpenPaneRequest, project_id: ProjectId },
}

impl UiEvent {
    /// The `type` tag.
    pub fn name(&self) -> &'static str {
        match self {
            Self::SessionUpdated { .. } => "session.updated",
            Self::SessionRemoved { .. } => "session.removed",
            Self::AttentionChanged { .. } => "attention.changed",
            Self::ProjectUpdated { .. } => "project.updated",
            Self::ProjectRemoved { .. } => "project.removed",
            Self::LayoutChanged { .. } => "layout.changed",
            Self::TicketsChanged { .. } => "tickets.changed",
            Self::ReviewsChanged { .. } => "reviews.changed",
            Self::WorkUpdated { .. } => "work.updated",
            Self::SettingsChanged { .. } => "settings.changed",
            Self::AccountStatusChanged { .. } => "account.status",
            Self::Toast { .. } => "toast",
            Self::PluginEvent { .. } => "plugin.event",
            Self::CtlCommand { .. } => "ctl.command",
            Self::UiOpen { .. } => "ui.open",
        }
    }
}

/// Trigger recursion bookkeeping carried by bus events emitted from trigger actions.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct TriggerChain {
    pub depth: u8,
    pub origin_triggers: Vec<String>,
}

/// Internal bus event (`tokio::sync::broadcast`, capacity [`BUS_CAPACITY`]).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
pub struct BusEvent {
    pub name: String,
    /// RFC 3339.
    pub ts: String,
    pub project_id: Option<ProjectId>,
    pub session_id: Option<SessionId>,
    pub work_item_id: Option<WorkItemId>,
    pub payload: serde_json::Value,
    pub chain: TriggerChain,
}

pub const BUS_CAPACITY: usize = 1024;

impl BusEvent {
    pub fn new(name: impl Into<String>, payload: serde_json::Value) -> Self {
        Self {
            name: name.into(),
            ts: crate::now_rfc3339(),
            project_id: None,
            session_id: None,
            work_item_id: None,
            payload,
            chain: TriggerChain::default(),
        }
    }

    pub fn with_project(mut self, id: ProjectId) -> Self {
        self.project_id = Some(id);
        self
    }

    pub fn with_session(mut self, id: SessionId) -> Self {
        self.session_id = Some(id);
        self
    }

    pub fn with_work_item(mut self, id: WorkItemId) -> Self {
        self.work_item_id = Some(id);
        self
    }

    pub fn with_chain(mut self, chain: TriggerChain) -> Self {
        self.chain = chain;
        self
    }

    /// `*.before_*` events allow blocking triggers.
    pub fn is_blocking(&self) -> bool {
        bus::BLOCKING.contains(&self.name.as_str())
    }
}

/// Bus event names (PLUGINS §6).
pub mod bus {
    pub const APP_STARTED: &str = "app.started";
    pub const APP_FOCUS_CHANGED: &str = "app.focus_changed";
    pub const PROJECT_OPENED: &str = "project.opened";
    pub const PROJECT_ACTIVATED: &str = "project.activated";
    pub const PROJECT_CLOSED: &str = "project.closed";
    pub const SESSION_SPAWNED: &str = "session.spawned";
    pub const SESSION_STATUS_CHANGED: &str = "session.status_changed";
    pub const SESSION_EXITED: &str = "session.exited";
    pub const SESSION_BELL: &str = "session.bell";
    pub const SESSION_TITLE_CHANGED: &str = "session.title_changed";
    pub const CLAUDE_HOOK: &str = "claude.hook";
    pub const CLAUDE_FILE_EDITED: &str = "claude.file_edited";
    pub const TICKET_BEFORE_START: &str = "ticket.before_start";
    pub const TICKET_STARTED: &str = "ticket.started";
    pub const TICKET_TRANSITIONED: &str = "ticket.transitioned";
    pub const TICKET_COMMENTED: &str = "ticket.commented";
    pub const TICKET_ASSIGNED: &str = "ticket.assigned";
    pub const WORKTREE_CREATED: &str = "worktree.created";
    pub const WORKTREE_REMOVED: &str = "worktree.removed";
    pub const WORK_BEFORE_FINISH: &str = "work.before_finish";
    pub const WORK_FINISHED: &str = "work.finished";
    /// Work item or saga step changed (`{work}`); core relays it as `UiEvent::WorkUpdated`.
    pub const WORK_UPDATED: &str = "work.updated";
    pub const PR_BEFORE_CREATE: &str = "pr.before_create";
    pub const PR_CREATED: &str = "pr.created";
    pub const PR_REVIEW_REQUESTED: &str = "pr.review_requested";
    pub const PR_UPDATED: &str = "pr.updated";
    pub const PR_CI_CHANGED: &str = "pr.ci_changed";
    pub const PR_APPROVED: &str = "pr.approved";
    pub const PR_CHANGES_REQUESTED: &str = "pr.changes_requested";
    pub const PR_MERGED: &str = "pr.merged";
    pub const TOOL_OPENED: &str = "tool.opened";
    pub const TOOL_EXITED: &str = "tool.exited";
    pub const SETTINGS_CHANGED: &str = "settings.changed";
    /// Prefix of external events (`kelta-ctl emit custom.x`).
    pub const CUSTOM_PREFIX: &str = "custom.";
    /// Marker published to a lagged subscriber so it resyncs.
    pub const LAGGED: &str = "lagged";

    /// Blocking pre-events.
    pub const BLOCKING: &[&str] = &[TICKET_BEFORE_START, WORK_BEFORE_FINISH, PR_BEFORE_CREATE];

    /// Every catalogued name (excluding `custom.*`).
    pub const ALL: &[&str] = &[
        APP_STARTED,
        APP_FOCUS_CHANGED,
        PROJECT_OPENED,
        PROJECT_ACTIVATED,
        PROJECT_CLOSED,
        SESSION_SPAWNED,
        SESSION_STATUS_CHANGED,
        SESSION_EXITED,
        SESSION_BELL,
        SESSION_TITLE_CHANGED,
        CLAUDE_HOOK,
        CLAUDE_FILE_EDITED,
        TICKET_BEFORE_START,
        TICKET_STARTED,
        TICKET_TRANSITIONED,
        TICKET_COMMENTED,
        TICKET_ASSIGNED,
        WORKTREE_CREATED,
        WORKTREE_REMOVED,
        WORK_BEFORE_FINISH,
        WORK_FINISHED,
        WORK_UPDATED,
        PR_BEFORE_CREATE,
        PR_CREATED,
        PR_REVIEW_REQUESTED,
        PR_UPDATED,
        PR_CI_CHANGED,
        PR_APPROVED,
        PR_CHANGES_REQUESTED,
        PR_MERGED,
        TOOL_OPENED,
        TOOL_EXITED,
        SETTINGS_CHANGED,
    ];
}

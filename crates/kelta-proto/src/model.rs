//! Domain model (ARCHITECTURE §5, §5.1): projects, sessions, layout, work items.

use std::collections::BTreeMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::codehost::{ReviewKind, ReviewRef};
use crate::ids::{
    PaneId, PluginId, ProjectId, ScreenInstanceId, SessionId, TabId, ToolId, ToolInstanceId, WorkItemId,
};
use crate::settings::{
    ClaudeEffort, ClaudeHost, CodeHostBinding, PermissionMode, StartOption, TrackerBinding, TransitionTarget,
};
use crate::tracker::{TicketGroupBy, TicketRef, TicketSort, Who};

// ---------------------------------------------------------------------------------------------
// Projects
// ---------------------------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
pub struct ProjectInfo {
    pub id: ProjectId,
    pub name: String,
    /// As stored: `theme:<slot>` (resolved against the active theme by the UI only) or a `#rrggbb` hex.
    pub color: Option<String>,
    pub icon: Option<String>,
    pub repos: Vec<RepoInfo>,
    pub tracker: Option<TrackerBinding>,
    pub open: bool,
    pub active: bool,
    pub attention: AttentionSummary,
    /// `true` for the built-in Home project.
    pub builtin: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct RepoInfo {
    pub id: String,
    pub path: PathBuf,
    pub primary: bool,
    pub remote: String,
    pub base: String,
    pub code_host: Option<CodeHostBinding>,
    /// Path exists on disk and is a git repo.
    pub exists: bool,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct AttentionSummary {
    pub level: Attention,
    pub needs_input: u32,
}

/// `project_detect` result, editable in the "New project" sheet and passed to `project_create`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, TS)]
pub struct ProjectDraft {
    pub suggested_id: ProjectId,
    pub name: String,
    #[serde(default)]
    pub color: Option<String>,
    #[serde(default)]
    pub icon: Option<String>,
    pub repos: Vec<RepoDraft>,
    pub code_host_hints: Vec<CodeHostHint>,
    pub tracker_hints: Vec<TrackerHint>,
    #[serde(default)]
    pub tracker: Option<TrackerBinding>,
    #[serde(default)]
    pub default_template: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct RepoDraft {
    pub id: String,
    pub path: PathBuf,
    pub primary: bool,
    pub remote: String,
    pub base: String,
    #[serde(default)]
    pub remote_url: Option<String>,
    #[serde(default)]
    pub code_host: Option<CodeHostBinding>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct CodeHostHint {
    pub repo_id: String,
    /// `"github"` | `"gitlab"` | `"bitbucket"` | `"gitea"`.
    pub kind: String,
    /// Host, e.g. `github.com`, `gitlab.acme.example`.
    pub host: String,
    /// `"acme/shop"`.
    pub repo: String,
    /// Configured account matching the host, if any.
    #[serde(default)]
    pub account: Option<crate::ids::AccountId>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct TrackerHint {
    /// `"jira"` | `"redmine"` | `"github"` | `"gitlab"` | `"gitea"`.
    pub kind: String,
    /// Human explanation, e.g. "branch names contain SHOP-123".
    pub reason: String,
    /// Jira project key / GitHub repo etc.
    #[serde(default)]
    pub key: Option<String>,
    #[serde(default)]
    pub account: Option<crate::ids::AccountId>,
}

/// Partial update for `project_update`; `None` = unchanged.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, TS)]
pub struct ProjectPatch {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub color: Option<String>,
    #[serde(default)]
    pub icon: Option<String>,
    #[serde(default)]
    pub default_template: Option<String>,
    #[serde(default)]
    pub repos: Option<Vec<RepoDraft>>,
    #[serde(default)]
    pub tracker: Option<TrackerBinding>,
    /// `true` removes the tracker binding (takes precedence over `tracker`).
    #[serde(default)]
    pub remove_tracker: bool,
}

// ---------------------------------------------------------------------------------------------
// Sessions
// ---------------------------------------------------------------------------------------------

/// Serialized `{"type":"shell"}`, `{"type":"editor","adapter":"nvim"}`, `{"type":"tool","tool_id":"lazygit"}`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum SessionKind {
    Shell,
    Claude,
    Editor { adapter: String },
    Tool { tool_id: ToolId },
    Setup,
    Custom,
}

impl SessionInfo {
    /// Its Claude runs outside Kelta (#347).
    pub fn hosted(&self) -> bool {
        self.claude.as_ref().is_some_and(|c| c.host.is_some())
    }

    /// Typing into a hosted Claude's pane is dropped silently (the wrapper never reads stdin).
    pub fn refuse_hosted(&self) -> Result<(), crate::error::KeltaError> {
        if self.hosted() {
            return Err(crate::error::KeltaError::conflict(
                "Claude runs outside Kelta: it cannot receive prompts from here.",
            ));
        }
        Ok(())
    }
}

impl SessionKind {
    /// Name used in settings maps (`terminal.scrollback`, `terminal.shift_enter`).
    pub fn name(&self) -> &'static str {
        match self {
            Self::Shell => "shell",
            Self::Claude => "claude",
            Self::Editor { .. } => "editor",
            Self::Tool { .. } => "tool",
            Self::Setup => "setup",
            Self::Custom => "custom",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum SessionStatus {
    #[default]
    Starting,
    Running,
    Working,
    NeedsInput,
    WaitingUser,
    Done,
    Error,
    Exited,
    Unknown,
}

/// Ordered; max-aggregated. Serialized as a string (`"none"`, `"activity"`, ...).
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Hash,
    PartialOrd,
    Ord,
    Default,
    Serialize,
    Deserialize,
    TS,
    schemars::JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum Attention {
    #[default]
    None = 0,
    Activity = 1,
    Done = 2,
    Error = 3,
    NeedsInput = 4,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum Lifecycle {
    Dormant,
    #[default]
    Live,
    Exited,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum StatusSource {
    Hook,
    Heuristic,
    #[default]
    None,
}

/// Serialized `{"kind":"claude_resume","uuid":"..."}` etc.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize, TS)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RestorePolicy {
    #[default]
    None,
    Relaunch,
    ShellInCwd,
    ClaudeResume {
        uuid: String,
        /// The Claude profile it was launched from (#318): a resume re-reads its forced mode, else
        /// takes the remembered one. `None` = unknown (a raw spawn or an older session).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[ts(optional = nullable)]
        profile: Option<String>,
    },
    Editor {
        session_file: Option<PathBuf>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, TS, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum CloseOnExit {
    #[default]
    Never,
    OnSuccess,
    Always,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
pub struct SpawnRequest {
    /// Caller-chosen session id (files naming it are written before the spawn). `None` = core
    /// generates one; a given id must be unused (`Conflict` otherwise).
    #[serde(default)]
    pub id: Option<SessionId>,
    pub project_id: ProjectId,
    pub kind: SessionKind,
    #[serde(default)]
    pub name: Option<String>,
    /// `None` = `$SHELL` (or `terminal.shell`).
    #[serde(default)]
    pub program: Option<String>,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub cwd: Option<PathBuf>,
    #[serde(default)]
    pub env: BTreeMap<String, String>,
    pub cols: u16,
    pub rows: u16,
    #[serde(default)]
    pub work_item_id: Option<WorkItemId>,
    #[serde(default)]
    pub restore: RestorePolicy,
    #[serde(default)]
    pub close_on_exit: CloseOnExit,
    #[serde(default)]
    pub template_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
pub struct SessionInfo {
    pub id: SessionId,
    pub project_id: ProjectId,
    pub kind: SessionKind,
    pub name: String,
    pub title: Option<String>,
    pub cwd: PathBuf,
    pub status: SessionStatus,
    pub status_source: StatusSource,
    pub attention: Attention,
    pub seen: bool,
    pub lifecycle: Lifecycle,
    pub pid: Option<u32>,
    pub exit_code: Option<i32>,
    pub work_item_id: Option<WorkItemId>,
    pub claude: Option<ClaudeMeta>,
    pub editor: Option<EditorMeta>,
    pub cols: u16,
    pub rows: u16,
    /// RFC 3339.
    pub created_at: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, TS)]
pub struct ClaudeMeta {
    pub session_uuid: String,
    pub model: Option<String>,
    /// Last assistant message preview (≤ 200 chars).
    pub preview: Option<String>,
    pub files_touched: Vec<PathBuf>,
    pub hooks_active: bool,
    /// Claude's task list, from its `TodoWrite` / `TaskCreate` / `TaskUpdate` hooks (#406).
    #[serde(default)]
    pub tasks: Vec<ClaudeTask>,
    /// Last statusline snapshot (in memory only); `None` until Claude first refreshes it.
    #[serde(default)]
    #[ts(optional = nullable)]
    pub usage: Option<ClaudeUsage>,
    /// The open MCP `ask` question (#356), in memory only; the next input to the session clears it.
    #[serde(default)]
    #[ts(optional = nullable)]
    /// Boxed: `UiEvent::SessionUpdated` stays under clippy's large-variant limit.
    pub question: Option<Box<ClaudeQuestion>>,
    /// Set when it runs outside Kelta (#347): its pane only waits for it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub host: Option<ClaudeHost>,
}

/// One entry of Claude's task list (#406).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct ClaudeTask {
    /// `TaskCreate`'s id, which `TaskUpdate` names; `None` for `TodoWrite` items.
    pub id: Option<String>,
    pub title: String,
    pub state: ClaudeTaskState,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum ClaudeTaskState {
    Pending,
    InProgress,
    Completed,
}

/// MCP `ask` (#356): a question with one button per option, answered by typing the option.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct ClaudeQuestion {
    pub question: String,
    pub options: Vec<String>,
    /// `body_md` rendered and sanitized in Rust (`kelta_http::markdown::to_html`).
    pub body_html: Option<String>,
}

/// What Claude Code's statusline reports (`HookPayload::usage`), plus Kelta's cost bookkeeping.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, TS)]
pub struct ClaudeUsage {
    /// `context_window.used_percentage`; `None` before the first answer.
    pub context_pct: Option<f64>,
    /// `cost.total_cost_usd` of this Claude process (a resumed one starts from the restored cost).
    pub cost_usd: f64,
    pub lines_added: u64,
    pub lines_removed: u64,
    /// `rate_limits.*`; absent for API-key accounts.
    pub five_hour: Option<RateWindow>,
    pub seven_day: Option<RateWindow>,
    /// Spend not yet added to `WorkItem.cost_usd` (core moves it there on SessionEnd and quit).
    #[serde(default)]
    pub unsaved_usd: f64,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, TS)]
pub struct RateWindow {
    pub used_percentage: f64,
    /// Unix seconds.
    pub resets_at: i64,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct EditorMeta {
    pub adapter: String,
    pub socket: Option<PathBuf>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct AttachInfo {
    pub generation: u32,
    pub cols: u16,
    pub rows: u16,
}

/// Output of `kelta_server::hooks::map` (ARCHITECTURE §7.6).
///
/// Clarification (scaffold): for hook events that leave the status unchanged (e.g. `PostToolUse`),
/// `status` is [`SessionStatus::Unknown`] and core keeps the previous status.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct StatusChange {
    pub status: SessionStatus,
    pub preview: Option<String>,
    pub file_edited: Option<PathBuf>,
    /// `hook_event_name` (+ `:<notification_type>` for Notification).
    pub raw_event: String,
    /// Claude's conversation uuid from the hook payload (changes on `/clear`); core resumes it.
    #[serde(default)]
    pub session_uuid: Option<String>,
}

// ---------------------------------------------------------------------------------------------
// Scope
// ---------------------------------------------------------------------------------------------

/// `{"kind":"project","id":"shop"}` | `{"kind":"source","id":"product"}` | `{"kind":"all"}`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Scope {
    Project {
        id: ProjectId,
    },
    /// One `[[sources]]` entry (tickets; reviews of the repos of its projects).
    Source {
        id: String,
    },
    #[default]
    All,
}

// ---------------------------------------------------------------------------------------------
// Layout (§5.1)
// ---------------------------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
pub struct Layout {
    pub project_id: ProjectId,
    pub tabs: Vec<Tab>,
    pub active_tab: Option<TabId>,
    /// Optimistic concurrency revision; stale rev on `layout_save` → `Conflict`.
    pub rev: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
pub struct Tab {
    pub id: TabId,
    pub title: String,
    pub work_item_id: Option<WorkItemId>,
    pub root: LayoutNode,
    pub focused_pane: Option<PaneId>,
    pub zoomed_pane: Option<PaneId>,
    /// Shown in its own window instead of the main one (#342).
    #[serde(default)]
    pub detached: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SplitDir {
    Row,
    Column,
}

/// `{"type":"split","dir":"row","ratios":[0.5,0.5],"children":[...]}` | `{"type":"pane","id":..,"content":{..}}`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum LayoutNode {
    Split {
        dir: SplitDir,
        /// Sum 1.0, each ≥ 0.05.
        ratios: Vec<f32>,
        children: Vec<LayoutNode>,
    },
    Pane {
        id: PaneId,
        content: PaneContent,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum TicketsMode {
    #[default]
    List,
    Board,
}

/// Row layout of the Reviews and Now lists (#383); `None` = comfortable.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum RowDensity {
    /// Two lines: title, then chips and meta.
    #[default]
    Comfortable,
    /// One 32px line.
    Compact,
}

/// Pane content; serde tag `kind`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PaneContent {
    Terminal {
        session_id: SessionId,
    },
    /// A Claude session's workflow runs (#392), split beside its terminal.
    ClaudeRuns {
        session_id: SessionId,
    },
    Web {
        tool_instance_id: ToolInstanceId,
    },
    PluginScreen {
        plugin_id: PluginId,
        screen_id: String,
        instance_id: ScreenInstanceId,
        params: serde_json::Value,
    },
    Tickets {
        scope: Scope,
        view_id: Option<String>,
        /// `[[sources]]` id the list narrows to (`view_id` then narrows inside it); `None` = every
        /// source of the scope.
        #[serde(default)]
        #[ts(optional = nullable)]
        source_id: Option<String>,
        mode: TicketsMode,
        #[serde(default)]
        who: Option<Who>,
        /// `None` = flow.
        #[serde(default)]
        #[ts(optional = nullable)]
        group: Option<TicketGroupBy>,
        /// `None` = priority.
        #[serde(default)]
        #[ts(optional = nullable)]
        sort: Option<TicketSort>,
        /// Assignee user id narrowing the list client-side; `None` = everyone of the who.
        #[serde(default)]
        #[ts(optional = nullable)]
        person: Option<String>,
    },
    TicketDetail {
        ticket: TicketRef,
    },
    Reviews {
        scope: Scope,
        /// The tab shown before #401 put every section on one list; kept so saved layouts load.
        #[serde(default)]
        #[ts(optional = nullable)]
        tab: Option<ReviewKind>,
        #[serde(default)]
        #[ts(optional = nullable)]
        density: Option<RowDensity>,
    },
    ReviewDetail {
        review: ReviewRef,
    },
    Inbox {
        #[serde(default)]
        #[ts(optional = nullable)]
        density: Option<RowDensity>,
    },
    WorkItem {
        id: WorkItemId,
    },
    Settings {
        section: Option<String>,
    },
    Diagnostics,
    Welcome,
    Empty,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, TS, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Placement {
    #[default]
    NewTab,
    SplitRight,
    SplitDown,
    ReplaceFocused,
    /// Focus an existing pane with the same content if any.
    Focused,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
pub struct OpenPaneRequest {
    pub content: PaneContent,
    pub placement: Placement,
    pub focus: bool,
    #[serde(default)]
    pub tab_title: Option<String>,
    #[serde(default)]
    pub work_item_id: Option<WorkItemId>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct PaneRef {
    pub project_id: ProjectId,
    pub tab_id: TabId,
    pub pane_id: PaneId,
}

// ---------------------------------------------------------------------------------------------
// Work items (SPEC §3.1)
// ---------------------------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum WorkKind {
    #[default]
    Ticket,
    Review,
    Branch,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum WorkState {
    #[default]
    Planned,
    Starting,
    /// Worktree ready, waiting for a Claude slot (`claude.max_live`); lowest `pos` starts first.
    Queued {
        pos: i32,
    },
    Active,
    PrOpen,
    /// The PR was merged on the host. `detail`: why the ticket was not moved to Done yet
    /// ("choose Done status", or a failed transition); `None` = moved, or nothing to move.
    Merged {
        #[serde(default)]
        detail: Option<String>,
    },
    /// The PR was closed without merge.
    PrClosed,
    Finished,
    Failed {
        step: String,
        message: String,
    },
}

impl WorkState {
    /// The PR is merged or closed: only Finish is left to do.
    pub fn pr_done(&self) -> bool {
        matches!(self, Self::Merged { .. } | Self::PrClosed)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum StepStatus {
    Pending,
    Running,
    Done,
    Failed,
    Skipped,
}

/// Saga step ids in order (L6 journals each in `work_steps`).
pub const WORK_STEPS: &[&str] = &[
    "before_start",
    "fetch_ticket",
    "fetch_base",
    "worktree",
    "include_files",
    "claude_files",
    "layout",
    "setup",
    "editor",
    "claude",
    "tracker_side_effects",
    "persist",
];

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct WorkStepStatus {
    pub step: String,
    pub status: StepStatus,
    pub detail: Option<String>,
    pub updated_at: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, TS)]
pub struct WorkItem {
    pub id: WorkItemId,
    pub project_id: ProjectId,
    pub kind: WorkKind,
    pub ticket: Option<TicketRef>,
    pub review: Option<ReviewRef>,
    pub repo_id: String,
    pub worktree: PathBuf,
    pub branch: String,
    pub base: String,
    pub claude_uuid: Option<String>,
    pub nvim_socket: Option<PathBuf>,
    pub session_ids: Vec<SessionId>,
    pub tab_id: Option<TabId>,
    pub pr_url: Option<String>,
    pub state: WorkState,
    pub steps: Vec<WorkStepStatus>,
    pub created_at: String,
    /// Scratch items: first line of the task, 72 chars max (tab, rows, PR title).
    #[serde(default)]
    pub title: Option<String>,
    /// Set by `work_link` on an item with a PR: the next Ship/Push prefixes the ticket key to the PR
    /// title unless it already carries one (`kelta_work::pr_title_with_key`), then clears it.
    #[serde(default)]
    pub pr_title_needs_key: bool,
    /// Claude stopped with changes Louis has not looked at (FLOW §2.3). Set only from a real `Stop`
    /// hook; cleared by `UserPromptSubmit`, UI Ship/Push, Finish and `work_mark_reviewed`.
    #[serde(default)]
    pub review_due: bool,
    /// Claude stopped without changes (ended its turn in prose). Cleared by `UserPromptSubmit`, Finish and `work_seen` (its tab shown, #489).
    #[serde(default)]
    pub claude_replied: bool,
    /// When Claude last stopped or asked for input (RFC 3339), from hooks only; orders Now's rows.
    #[serde(default)]
    #[ts(optional = nullable)]
    pub claude_at: Option<String>,
    /// Review thread ids handed to Claude by the last Fix with Claude (resolved on request).
    #[serde(default)]
    #[ts(as = "Option<Vec<String>>", optional)]
    pub sent_threads: Vec<String>,
    /// Newest review comment time (host's RFC 3339) shown in the last Fix with Claude send: a comment
    /// newer than it is new (#208). `None`: never sent, every unresolved thread awaiting me is new.
    #[serde(default)]
    #[ts(optional = nullable)]
    pub feedback_at: Option<String>,
    /// Kelta-driven rebase in progress, or done and awaiting its force push.
    #[serde(default)]
    #[ts(optional = nullable)]
    pub rebase: Option<Box<RebaseState>>,
    /// Claude spend of the item's ended sessions (USD), summed across sessions and resumes.
    #[serde(default)]
    pub cost_usd: f64,
    /// Claude's full final message of its last `Stop` (the session preview keeps 200 chars).
    #[serde(default)]
    #[ts(optional = nullable)]
    pub claude_message: Option<String>,
    /// Shape of `refs/kelta/wi/<id>/reviewed..last` at the last `Stop`; `None` when empty or reviewed.
    #[serde(default)]
    #[ts(optional = nullable)]
    pub delta: Option<ReviewDelta>,
    /// Louis's own `next:` note (`work_set_note`).
    #[serde(default)]
    #[ts(optional = nullable)]
    pub next_note: Option<String>,
    /// When Louis last left the item's tab (RFC 3339, `work_left`); drives the return strip.
    #[serde(default)]
    #[ts(optional = nullable)]
    pub left_at: Option<String>,
    /// First of the item's `PORT_BLOCK` ports (`[ports] range`); `None` = range off, or finished.
    #[serde(default)]
    #[ts(optional = nullable)]
    pub port_base: Option<u16>,
    /// Merge when ready armed (`work_arm_merge`): the host merges the PR, then Kelta runs Finish once.
    #[serde(default)]
    pub auto_finish: bool,
    /// When Park (`work_park`, #142) stopped the item's Claude; cleared when that Claude spawns again.
    #[serde(default)]
    #[ts(optional = nullable)]
    pub parked_at: Option<String>,
    /// Unsaved nvim buffers that kept nvim running at the last park (0 = nvim parked too, or none).
    #[serde(default)]
    pub nvim_kept: u32,
    /// Claimed by a background run (MCP `claim_ticket`, #235): the item adopts the run's worktree and
    /// branch, which Finish never removes.
    #[serde(default)]
    #[ts(optional = nullable)]
    pub run: Option<RunClaim>,
    /// `[[sources]]` id the ticket was started from (rule 4 of SPEC §3.1); `None` for older items.
    #[serde(default)]
    #[ts(optional = nullable)]
    pub ticket_source: Option<String>,
}

/// A background run's claim on a ticket and how far it got (MCP `claim_ticket` / `report_progress`).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct RunClaim {
    /// Workflow name (`kelta-ticket-pass`).
    pub name: Option<String>,
    pub run_id: Option<String>,
    /// The claiming Claude session; `None` from the global `/mcp`.
    pub session_id: Option<SessionId>,
    /// Free text from the workflow (its phase titles).
    pub phase: Option<String>,
    pub state: RunState,
    pub note: Option<String>,
    pub sha: Option<String>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum RunState {
    #[default]
    Running,
    Blocked,
    Failed,
    Done,
    /// The claiming session is no longer live while the run is `running`: computed when shown
    /// (`work_list`, `work.updated`), not stored by new code.
    Orphaned,
}

/// What changed since Louis's last look (the Ready for review chip). Lockfiles, generated files
/// (`linguist-generated`, `-diff`) and `reviews.ignore_globs` count in `generated` only.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct ReviewDelta {
    /// Added + deleted lines of the real files.
    pub lines: u32,
    pub files: u32,
    /// Real files that are tests.
    pub tests: u32,
    /// Added + deleted lines of lockfiles and generated files.
    pub generated: u32,
    /// Every file, for `+N/−M since you reviewed`.
    pub insertions: u32,
    pub deletions: u32,
}

/// `work_delta_files`: one file of `reviewed..last` (renames keep their new path; a binary file
/// counts 0 lines). `generated`: a lockfile or generated file, folded into one row by the UI.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct DeltaFile {
    pub path: String,
    pub insertions: u32,
    pub deletions: u32,
    pub test: bool,
    pub generated: bool,
}

/// Lifecycle of a review note: `sent` with the next message to Claude, then `touched` /
/// `untouched` after its next `Stop` (a heuristic: a change within 5 lines of the anchor).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum NoteState {
    #[default]
    Open,
    Sent,
    Touched,
    Untouched,
    Resolved,
}

/// Louis's review note on a work item, anchored at `path:line_start-line_end` (#133).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct ReviewNote {
    /// 0 until stored.
    pub id: i64,
    pub work_item_id: WorkItemId,
    /// Relative to the worktree.
    pub path: String,
    pub line_start: u32,
    pub line_end: u32,
    pub body: String,
    /// `nvim` | `pr`.
    pub source: String,
    /// Id of the note on its source (a PR comment); `None` for nvim notes.
    pub ext_ref: Option<String>,
    pub state: NoteState,
    /// RFC 3339.
    pub sent_at: Option<String>,
}

/// `work_notes`: the notes of an item and what changed since they were last sent.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct ReviewNotes {
    pub worktree: PathBuf,
    pub notes: Vec<ReviewNote>,
    /// `+N/−M since feedback` (worktree against the snapshot taken when the notes were sent).
    pub since: Option<ReviewDelta>,
}

/// A ticket in my Next up list, or snoozed (#145). Local to Kelta, never written to a tracker.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
pub struct NextUpItem {
    pub project_id: ProjectId,
    pub ticket: TicketRef,
    /// Fractional order, lowest first; `None` = not in the list, only snoozed.
    pub rank: Option<f64>,
    /// RFC 3339; hidden from Next up and grooming until then.
    pub snoozed_until: Option<String>,
}

/// A workflow a run can use: a saved Claude Code workflow (#233), a `*.js` file whose
/// `export const meta` names it, or a `[[workflows]]` entry of the config (#355).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct WorkflowInfo {
    /// What `workflow_run` takes: the meta `name`, or the config `id`.
    pub name: String,
    /// The config `label`; the `name` for a `*.js` workflow.
    pub label: String,
    pub description: String,
    /// The `*.js` file; `None` for a config workflow.
    pub path: Option<PathBuf>,
    pub source: WorkflowSource,
}

/// Where a workflow lives: the project's primary repo (`.claude/workflows/`), `~/.claude/workflows/`
/// or the config (`[[workflows]]`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum WorkflowSource {
    Repo,
    Home,
    Config,
}

/// A workflow run started, or the one already running on the same ticket or PR (`running`), which
/// the UI opens instead (#355).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
pub struct WorkflowStart {
    pub session: SessionInfo,
    pub running: bool,
}

/// A Claude Code session Kelta did not start (another terminal, a multiplexer, a script), seen
/// through the user-scope hook relay (`kelta-ctl hook --outside`, #345). Keyed by Claude's session id.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
pub struct OutsideSession {
    /// Claude's session uuid.
    pub id: String,
    pub cwd: Option<PathBuf>,
    /// The first prompt's first line, else the cwd's last component.
    pub title: String,
    pub status: SessionStatus,
    /// The Claude process, when the relay found it: the entry goes when it exits.
    pub pid: Option<u32>,
    /// Checked-out branch of the cwd's repo when first seen.
    pub branch: Option<String>,
    /// A listed ticket whose `branch_key` the branch or cwd carries.
    pub ticket: Option<TicketRef>,
    /// Last `Stop` preview.
    pub preview: Option<String>,
    pub updated_at: String,
}

/// A Claude Code `Workflow` run inside a Kelta session (#272), read from Claude's undocumented
/// on-disk files under `~/.claude/projects/<slug>/<claude session>/`; every field is best effort.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
pub struct ClaudeRun {
    /// `wf_<id>`.
    pub run_id: String,
    pub session_id: SessionId,
    /// Workflow name (`kelta-ticket-pass`), from the final record or the script file name.
    pub name: Option<String>,
    /// `running` until the final record exists, then its `status` (`completed`, `killed`, ...).
    pub status: String,
    /// In first-seen order; agents in spawn order.
    pub phases: Vec<ClaudeRunPhase>,
    pub duration_ms: Option<u64>,
    pub total_tokens: Option<u64>,
    pub total_tool_calls: Option<u64>,
    /// The run's result (pretty JSON unless a string), truncated.
    pub result: Option<String>,
    pub logs: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct ClaudeRunPhase {
    pub title: String,
    pub agents: Vec<ClaudeRunAgent>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct ClaudeRunAgent {
    pub id: String,
    /// The agent's label (`implement-#202`).
    pub label: String,
    pub done: bool,
}

/// One line of an agent transcript's tail (`claude_run_tail`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct TranscriptLine {
    /// `text` (assistant prose), `tool` (a tool call) or `result` (a tool result; `error` when failed).
    pub kind: String,
    pub text: String,
}

/// `next_up_list`: the list and snoozes, plus the tickets I have seen (`account:key`, the `New` badge).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
pub struct NextUp {
    pub items: Vec<NextUpItem>,
    pub seen: Vec<String>,
}

/// `tags_get` / `tags_set` (#349): my local tags and what carries them, kept in Kelta's store and never
/// sent to a tracker or code host. Items are keyed `account:key` (tickets) and `account:repo#number`
/// (pull requests); their lists hold tag names.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct LocalTags {
    pub tags: Vec<LocalTag>,
    pub tickets: BTreeMap<String, Vec<String>>,
    pub prs: BTreeMap<String, Vec<String>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct LocalTag {
    pub name: String,
    /// `theme:<slot>` or `#rrggbb`, like a project colour; `None` = neutral.
    pub color: Option<String>,
}

/// Ports per work item: `KELTA_PORT` plus `KELTA_PORT_1..9`.
pub const PORT_BLOCK: u16 = 10;

impl WorkItem {
    /// #497: `sid` holds this item: its run's session or, with no run, one of its own sessions (the
    /// Claude that Start work gave it).
    pub fn claimed_by(&self, sid: Option<&SessionId>) -> bool {
        sid.is_some_and(|s| match &self.run {
            Some(r) => r.session_id.as_ref() == Some(s),
            None => self.session_ids.contains(s),
        })
    }

    /// `claim_ticket` of `self` against `held`, an unfinished item of the same ticket: an orphaned
    /// claim, or one of the same run (equal `run_id`s), is taken over (same id, the new run's
    /// worktree); so is a `blocked` or `failed` one, whose lane has ended, and the holder's own
    /// re-claim (#497: `claimed_by` the claiming session, run ids equal or one missing; the lanes of
    /// one orchestrator share a session, so two different run ids still conflict). Anything else wins
    /// and comes back as `Err`.
    pub fn claim_over(self, held: Option<WorkItem>) -> Result<WorkItem, Box<WorkItem>> {
        let Some(mut h) = held else { return Ok(self) };
        let new = self.run.as_ref();
        let held_id = h.run.as_ref().and_then(|r| r.run_id.as_ref());
        let new_id = new.and_then(|n| n.run_id.as_ref());
        let ended = h
            .run
            .as_ref()
            .is_some_and(|r| matches!(r.state, RunState::Orphaned | RunState::Blocked | RunState::Failed));
        let same_run = held_id.is_some() && held_id == new_id;
        let mine = h.claimed_by(new.and_then(|n| n.session_id.as_ref()))
            && !matches!((held_id, new_id), (Some(a), Some(b)) if a != b);
        if !(ended || same_run || mine) {
            return Err(Box::new(h));
        }
        (h.repo_id, h.worktree, h.branch, h.base, h.run) =
            (self.repo_id, self.worktree, self.branch, self.base, self.run);
        Ok(h)
    }

    /// Env of every process of the item: `KELTA_PORT`, `KELTA_PORT_1..9` (when it has a block) and
    /// `COMPOSE_PROJECT_NAME=kelta-<key-slug>` so parallel worktrees get their own containers.
    pub fn env(&self) -> BTreeMap<String, String> {
        let key = self.ticket.as_ref().map_or(self.branch.as_str(), |t| t.key.as_str());
        let mut slug = String::new();
        for c in key.chars().map(|c| c.to_ascii_lowercase()) {
            if c.is_ascii_alphanumeric() {
                slug.push(c);
            } else if !slug.is_empty() && !slug.ends_with('-') {
                slug.push('-');
            }
        }
        let mut env = BTreeMap::from([(
            "COMPOSE_PROJECT_NAME".to_owned(),
            format!("kelta-{}", slug.trim_end_matches('-')),
        )]);
        if let Some(base) = self.port_base {
            env.insert("KELTA_PORT".into(), base.to_string());
            for i in 1..PORT_BLOCK {
                env.insert(format!("KELTA_PORT_{i}"), (base + i).to_string());
            }
        }
        env
    }
}

/// `work_rebase` bookkeeping (FLOW §4.4). `pre_head` is the last HEAD that contained the remote
/// tip `remote_sha`, so a force push can prove it only rewrites the user's own commits.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct RebaseState {
    /// Ref rebased onto (`origin/main`, `origin/feat/x`).
    pub onto: String,
    pub pre_head: String,
    /// Remote branch tip before the rebase; `None` when the branch was never pushed.
    pub remote_sha: Option<String>,
    /// Conflicted files (worktree-relative); empty when the rebase is not stopped.
    pub conflicts: Vec<PathBuf>,
    pub step: u32,
    pub total: u32,
}

/// What `work_rebase{op: start}` rebases onto.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum RebaseOnto {
    /// `<remote>/<base>`.
    Base,
    /// `<remote>/<branch>`: take commits pushed by others (suggestions, Update branch).
    RemoteBranch,
}

/// `{"kind":"start","onto":"base","no_fetch":false}` | `{"kind":"continue"}` | `{"kind":"abort"}`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RebaseOp {
    Start {
        onto: RebaseOnto,
        /// Rebase onto the last fetched ref (after a failed fetch).
        #[serde(default)]
        no_fetch: bool,
    },
    Continue,
    Abort,
}

/// A file `work_send` writes into the item's private Claude run dir.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct SendFile {
    /// Plain file name (`feedback.md`).
    pub name: String,
    pub content: String,
}

/// `{"kind":"ticket","ticket":{..}}` | `{"kind":"review","review":{..}}` |
/// `{"kind":"branch","name":"..","task":"..","repo":".."}` (scratch work, FLOW §2.1).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum WorkSource {
    Ticket {
        ticket: TicketRef,
    },
    Review {
        review: ReviewRef,
    },
    /// Empty `name` → `work.scratch_branch_template` with `{slug}` from the task's first line.
    /// `task` is the first Claude prompt (`{task}`); `repo` defaults to the project's primary repo.
    Branch {
        name: String,
        #[serde(default)]
        task: Option<String>,
        #[serde(default)]
        repo: Option<String>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum BranchChoice {
    #[default]
    Reuse,
    Suffix,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct BranchExists {
    pub has_worktree: bool,
    pub choice: BranchChoice,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct ClaudePlan {
    pub profile: String,
    pub model: String,
    pub effort: ClaudeEffort,
    /// `None` = the remembered mode, else Claude's own default (no `--permission-mode`).
    pub permission_mode: Option<PermissionMode>,
    /// Rendered prompt (editable).
    pub prompt: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct SideEffects {
    pub assign_me: bool,
    pub transition_to: Option<TransitionTarget>,
    pub comment: Option<String>,
    pub run_setup: bool,
}

/// Where a ticket stands in its pipeline (#461): SQLite `ticket_steps`, bus `ticket.step`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct TicketStep {
    pub ticket: TicketRef,
    /// `[[sources]]` id the steps came from (`WorkItem.ticket_source`).
    pub source_id: Option<String>,
    pub step_id: String,
    /// RFC 3339.
    pub entered_at: String,
    /// The trigger that entered the step, as JSON (`"pr_ready"`, `{"status":{"name":"PO Review"}}`), or `manual` (#463).
    pub by: String,
}

/// The live example under a template setting (#479, `template_preview`): the template rendered for
/// the sample ticket KEL-214 the way Start work renders it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct TemplatePreview {
    pub text: String,
    /// Branch name templates only: `false` when git refuses the rendered name and `text` is the
    /// cleaned-up name Kelta uses instead.
    pub git_ok: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct StartWorkPlan {
    pub project_id: ProjectId,
    pub source: WorkSource,
    pub repo_id: String,
    /// Repos the user may pick from (picker shown when > 1).
    pub repo_choices: Vec<String>,
    /// Projects the user may pick from (picker shown when > 1), filled by `Core::work_plan`.
    #[serde(default)]
    pub project_choices: Vec<ProjectId>,
    /// `project_id` is only the active project: the sheet always shows so the user confirms it.
    #[serde(default)]
    pub project_guessed: bool,
    /// `[[sources]]` id of the ticket, set by `Core::work_plan`; copied to the work item at start.
    #[serde(default)]
    #[ts(optional = nullable)]
    pub ticket_source: Option<String>,
    pub base: String,
    pub branch: String,
    pub branch_exists: Option<BranchExists>,
    pub worktree_path: PathBuf,
    pub template_id: String,
    pub claude: ClaudePlan,
    pub side_effects: SideEffects,
    /// Existing work item → the sheet becomes "Resume".
    pub existing: Option<WorkItemId>,
    /// Own PR made outside Kelta: the item is a scratch item on the PR's head branch, linked to
    /// this PR URL (FLOW §2.1).
    #[serde(default)]
    #[ts(optional = nullable)]
    pub adopt_pr: Option<String>,
    /// My open PRs naming the ticket (#453): `adopt_pr` is the first; the sheet picks among several.
    #[serde(default)]
    pub pr_choices: Vec<PrChoice>,
    /// Start another PR (#456): a new item beside the ticket's unfinished ones, never a Resume.
    #[serde(default)]
    pub another: bool,
    /// Options picked per ticket (#357), set by `Core::work_plan`; `claude.prompt` has them applied.
    #[serde(default)]
    #[ts(optional = nullable)]
    pub options: Option<PlanOptions>,
}

/// An open PR of mine a ticket's Start work can resume: its head branch on `repo_id`, from `base`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct PrChoice {
    pub url: String,
    pub number: u64,
    pub title: String,
    pub repo_id: String,
    pub branch: String,
    pub base: String,
}

/// The start options of a plan: the route's, else the template's.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct PlanOptions {
    /// What remembers the picks: `route:<source>:<index>` or `template:<id>`.
    pub key: String,
    pub declared: Vec<StartOption>,
    /// Picked values, at most one per option.
    pub picked: Vec<String>,
    /// The prompt before the options, with `{options}` where they go (none = at the end).
    pub prompt: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct FinishOpts {
    pub remove_worktree: bool,
    pub delete_branch: bool,
    pub force: bool,
    #[serde(default)]
    pub transition_to: Option<TransitionTarget>,
}

/// Who asked for a PR (`work_create_pr`, FLOW §4.5): the UI (Ship) or Claude through the MCP
/// `create_pr` tool. Only a UI Ship clears `WorkItem.review_due`; an MCP ship sets it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum ShipOrigin {
    Ui,
    Mcp,
}

/// `work_finish_merged` result.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, TS)]
pub struct FinishMergedReport {
    pub finished: Vec<WorkItem>,
    /// Merged items left for a single Finish (dirty worktree, Done status to choose, busy).
    pub skipped: Vec<SkippedItem>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct SkippedItem {
    pub id: WorkItemId,
    pub reason: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct GitStatus {
    /// Commits on HEAD not in `<remote>/<base>`.
    pub ahead: u32,
    /// Commits on `<remote>/<base>` not in HEAD.
    pub behind: u32,
    pub dirty: bool,
    pub unpushed: bool,
    /// Own rewrite: the remote tip is in the pre-rebase HEAD but not in HEAD (force push allowed).
    #[serde(default)]
    #[ts(as = "Option<bool>", optional)]
    pub diverged: bool,
    /// Commits on `<remote>/<branch>` the local work does not have (suggestions, Update branch).
    #[serde(default)]
    #[ts(as = "Option<u32>", optional)]
    pub remote_new: u32,
    /// Diffstat from the merge base with `<remote>/<base>` to the working tree (untracked files
    /// count in `files` only).
    #[serde(default)]
    pub files: u32,
    #[serde(default)]
    pub insertions: u32,
    #[serde(default)]
    pub deletions: u32,
    /// The worktree directory is gone (deleted outside Kelta).
    #[serde(default)]
    pub missing: bool,
}

/// `{"kind":"session","id":..}` | `{"kind":"work_item","id":..}`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum EditorTarget {
    Session { id: SessionId },
    WorkItem { id: WorkItemId },
}

/// Placeholder context passed from the UI (tools, templates, commands); see SETTINGS §6.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct TemplateCtx {
    #[serde(default)]
    pub repo_id: Option<String>,
    #[serde(default)]
    pub cwd: Option<PathBuf>,
    #[serde(default)]
    pub session_id: Option<SessionId>,
    #[serde(default)]
    pub work_item_id: Option<WorkItemId>,
    #[serde(default)]
    pub ticket: Option<TicketRef>,
    #[serde(default)]
    pub review: Option<ReviewRef>,
    /// Extra string placeholders.
    #[serde(default)]
    pub extra: BTreeMap<String, String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(id: &str, sid: Option<&str>, run_id: Option<&str>, worktree: &str) -> WorkItem {
        WorkItem {
            id: WorkItemId::new(id),
            worktree: worktree.into(),
            run: Some(RunClaim {
                session_id: sid.map(SessionId::new),
                run_id: run_id.map(Into::into),
                ..RunClaim::default()
            }),
            ..WorkItem::default()
        }
    }

    #[test]
    fn claim_over_gives_the_holder_session_its_item_but_not_another_lane() {
        let held = |sid, run_id| item("held", sid, run_id, "/old");
        let new = |sid, run_id| item("new", sid, run_id, "/new");
        // The holder's own re-claim: same run id, or either one missing.
        for (h, n) in [(Some("r1"), Some("r1")), (Some("r1"), None), (None, Some("r1"))] {
            let got = new(Some("a"), n).claim_over(Some(held(Some("a"), h))).unwrap();
            assert_eq!((got.id, got.worktree), (WorkItemId::new("held"), "/new".into()));
        }
        // The item's own Claude (an item with no run) claims it: `run` is set.
        let mut start = item("held", None, None, "/old");
        (start.run, start.session_ids) = (None, vec![SessionId::new("a")]);
        let got = new(Some("a"), None).claim_over(Some(start.clone())).unwrap();
        assert!(got.run.is_some());
        assert!(new(Some("b"), None).claim_over(Some(start)).is_err());
        // Another lane of one session, another session, and unowned claims still conflict.
        assert!(new(Some("a"), Some("r2")).claim_over(Some(held(Some("a"), Some("r1")))).is_err());
        assert!(new(Some("b"), None).claim_over(Some(held(Some("a"), None))).is_err());
        assert!(new(None, None).claim_over(Some(held(None, None))).is_err());
    }
}

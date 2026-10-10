//! Domain model (ARCHITECTURE §5, §5.1): projects, sessions, layout, work items.

use std::collections::BTreeMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::codehost::ReviewRef;
use crate::ids::{
    PaneId, PluginId, ProjectId, ScreenInstanceId, SessionId, TabId, ToolId, ToolInstanceId, WorkItemId,
};
use crate::settings::{ClaudeEffort, CodeHostBinding, PermissionMode, TrackerBinding, TransitionTarget};
use crate::tracker::{TicketGroupBy, TicketRef, TicketSort, Who};

// ---------------------------------------------------------------------------------------------
// Projects
// ---------------------------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
pub struct ProjectInfo {
    pub id: ProjectId,
    pub name: String,
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
    /// Last statusline snapshot (in memory only); `None` until Claude first refreshes it.
    #[serde(default)]
    #[ts(optional = nullable)]
    pub usage: Option<ClaudeUsage>,
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

/// `{"kind":"project","id":"shop"}` | `{"kind":"all"}`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Scope {
    Project { id: ProjectId },
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

/// Pane content; serde tag `kind`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PaneContent {
    Terminal {
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
    },
    ReviewDetail {
        review: ReviewRef,
    },
    Inbox,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum WorkKind {
    Ticket,
    Review,
    Branch,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum WorkState {
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

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
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
    /// Claude stopped without changes (ended its turn in prose). Cleared by `UserPromptSubmit`, Finish.
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

/// `next_up_list`: the list and snoozes, plus the tickets I have seen (`account:key`, the `New` badge).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
pub struct NextUp {
    pub items: Vec<NextUpItem>,
    pub seen: Vec<String>,
}

/// Ports per work item: `KELTA_PORT` plus `KELTA_PORT_1..9`.
pub const PORT_BLOCK: u16 = 10;

impl WorkItem {
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
    pub permission_mode: PermissionMode,
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct StartWorkPlan {
    pub project_id: ProjectId,
    pub source: WorkSource,
    pub repo_id: String,
    /// Repos the user may pick from (picker shown when > 1).
    pub repo_choices: Vec<String>,
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

//! Tracker domain types (ARCHITECTURE §8.1). The `Tracker` trait lives in [`crate::api`].

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::codehost::PrLink;
use crate::error::KeltaError;
use crate::ids::{AccountId, ProjectId, WorkItemId};
use crate::settings::TrackerView;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum TrackerKind {
    Jira,
    Redmine,
    GithubIssues,
    GitlabIssues,
    GiteaIssues,
    Linear,
    /// A process (KPP) plugin provider.
    Plugin,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct TrackerCaps {
    pub board_columns: bool,
    pub assign: bool,
    pub comment: bool,
    pub transitions_need_fetch: bool,
    pub projects_v2: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct User {
    /// Provider user id (Jira accountId, Redmine id, GitHub/GitLab login or numeric id).
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub login: Option<String>,
    #[serde(default)]
    pub avatar_url: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, TS)]
pub struct TicketRef {
    pub account: AccountId,
    /// Human key: `"SHOP-123"`, `"4567"`, `"acme/shop#12"`.
    pub key: String,
    /// Provider id (may equal key).
    pub id: String,
}

/// Whose tickets a list shows. On a view or a `tracker_list` call; `None` keeps the view's
/// provider fields (`assigned_to`, `scope`, `jql`) as they are.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Who {
    Mine,
    Unassigned,
    Anyone,
}

/// Tickets pane grouping (`PaneContent::Tickets.group`; `None` = flow).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum TicketGroupBy {
    /// Doing / Waiting / Ready / Backlog / Done.
    Flow,
    Status,
    Priority,
    Sprint,
    Assignee,
    Source,
    None,
}

/// Order within a tickets group (`PaneContent::Tickets.sort`; `None` = priority).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum TicketSort {
    Priority,
    Updated,
    /// Time in the current status (`Ticket.status_since`).
    Age,
    Key,
}

/// Jira sprint, Linear cycle, GitHub Projects iteration, GitLab iteration/milestone, Redmine version.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct Sprint {
    pub id: String,
    pub name: String,
    pub active: bool,
    /// RFC 3339 or `YYYY-MM-DD`.
    #[serde(default)]
    pub ends_at: Option<String>,
}

/// One ticket source offered by `Tracker::sources`, ready to append to `TrackerBinding.views`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct SourceHit {
    /// `board` | `project` | `filter` | `query` | `repo` | `team` | `sprint` ...
    pub kind: String,
    pub label: String,
    pub detail: Option<String>,
    pub view: TrackerView,
}

/// Status category; serialized `todo | in_progress | in_review | done | unknown` (also used in config).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize, TS, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum StatusCategory {
    Todo,
    InProgress,
    InReview,
    Done,
    #[default]
    Unknown,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct Status {
    pub id: String,
    pub name: String,
    pub category: StatusCategory,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct Ticket {
    #[serde(rename = "ref")]
    pub r#ref: TicketRef,
    pub title: String,
    pub url: String,
    pub status: Status,
    #[serde(default)]
    pub kind: Option<String>,
    #[serde(default)]
    pub assignee: Option<User>,
    #[serde(default)]
    pub labels: Vec<String>,
    #[serde(default)]
    pub priority: Option<String>,
    /// RFC 3339.
    pub updated_at: String,
    /// Provider project key/path, used to match tickets to Kelta projects.
    #[serde(default)]
    pub project_hint: Option<String>,
    /// Provider priority as a rank, 0 = highest; `None` = no priority.
    #[serde(default)]
    pub priority_rank: Option<u8>,
    /// RFC 3339: when the ticket entered its current status (falls back to `updated_at`).
    #[serde(default)]
    pub status_since: Option<String>,
    #[serde(default)]
    pub sprint: Option<Sprint>,
    /// Provider estimate as displayed (`3`, `5 pts`, `2h`).
    #[serde(default)]
    pub estimate: Option<String>,
    /// Due date, `YYYY-MM-DD` or RFC 3339.
    #[serde(default)]
    pub due: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum BodyFormat {
    Adf,
    JiraWiki,
    Textile,
    Markdown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct Comment {
    pub author: User,
    pub created_at: String,
    /// Sanitized HTML (rendered in Rust, ARCHITECTURE D12).
    pub body_html: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
pub struct TicketDetail {
    pub ticket: Ticket,
    pub body_md: String,
    /// Sanitized HTML.
    pub body_html: String,
    pub body_format: BodyFormat,
    /// Last 20 comments, oldest first.
    pub comments: Vec<Comment>,
    #[serde(default)]
    pub parent: Option<TicketRef>,
    /// Sub-tasks (TICKETS.md T7): providers fill each `ticket`, kelta-core the rest of the item.
    #[serde(default)]
    pub children: Vec<TicketItem>,
    /// Filled by kelta-core (TICKETS.md T1); providers leave it empty.
    #[serde(default)]
    pub prs: Vec<PrLink>,
    /// The ticket account's caps, filled by kelta-core (a standalone detail has no list item).
    #[serde(default)]
    pub caps: TrackerCaps,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct Transition {
    pub id: String,
    pub name: String,
    pub to: Status,
    pub needs_fields: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct Column {
    pub id: String,
    pub name: String,
    pub category: StatusCategory,
    pub order: u32,
    /// Status names that belong to this column (besides the category).
    pub match_names: Vec<String>,
}

/// Opaque pagination cursor; serialized `{"kind": "offset", "value": 50}`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum Cursor {
    Offset(u32),
    Token(String),
    Page(u32),
    After(String),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
pub struct Page<T> {
    pub items: Vec<T>,
    pub next: Option<Cursor>,
}

impl<T> Default for Page<T> {
    fn default() -> Self {
        Self { items: Vec::new(), next: None }
    }
}

/// Who to assign: `{"kind":"me"}`, `{"kind":"user","id":"..."}`, `{"kind":"none"}`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Assignee {
    Me,
    User { id: String },
    None,
}

/// Per-account failure carried next to aggregated items (ARCHITECTURE §12.1).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
pub struct AccountError {
    pub account_id: AccountId,
    pub error: KeltaError,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, TS)]
pub struct TicketItem {
    pub ticket: Ticket,
    /// Kelta projects this ticket belongs to (empty = "Other").
    pub project_ids: Vec<ProjectId>,
    #[serde(default)]
    pub work_item_id: Option<WorkItemId>,
    /// Ids of the binding views (sources) that returned this ticket.
    #[serde(default)]
    pub view_ids: Vec<String>,
    /// Linked PRs: the work item's first, then key matches from the polled review feeds.
    #[serde(default)]
    pub prs: Vec<PrLink>,
    /// What the ticket's tracker can do (actions it cannot are disabled in the UI).
    #[serde(default)]
    pub caps: TrackerCaps,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, TS)]
pub struct TicketPage {
    pub items: Vec<TicketItem>,
    pub next: Option<Cursor>,
    /// Served from cache while a refresh is pending / failed.
    pub stale: bool,
    pub errors: Vec<AccountError>,
}

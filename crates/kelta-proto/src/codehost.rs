//! Code-host domain types (ARCHITECTURE §8.2). The `CodeHost` trait lives in [`crate::api`].

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::ids::{AccountId, ProjectId};
use crate::tracker::{AccountError, User};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum CodeHostKind {
    Github,
    Gitlab,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, TS)]
pub struct ReviewRef {
    pub account: AccountId,
    /// `"acme/shop"` | `"grp/sub/proj"`.
    pub repo: String,
    /// PR number / MR iid.
    pub number: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum CiState {
    Success,
    Failure,
    Pending,
    Error,
    #[default]
    None,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum ReviewDecision {
    Approved,
    ChangesRequested,
    ReviewRequired,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum MyReviewState {
    Pending,
    Approved,
    ChangesRequested,
    Commented,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum ReviewKind {
    ReviewRequested,
    Authored,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct Review {
    #[serde(rename = "ref")]
    pub r#ref: ReviewRef,
    pub title: String,
    pub url: String,
    pub author: User,
    pub draft: bool,
    pub head_sha: String,
    pub source_branch: String,
    pub target_branch: String,
    pub ci: CiState,
    #[serde(default)]
    pub decision: Option<ReviewDecision>,
    #[serde(default)]
    pub my_state: Option<MyReviewState>,
    #[serde(default)]
    pub mergeable: Option<bool>,
    /// Commit my last submitted review was left on; with `head_sha` it tells "updated since your review".
    #[serde(default)]
    pub reviewed_head: Option<String>,
    #[serde(default)]
    pub labels: Vec<String>,
    pub kind: ReviewKind,
    pub updated_at: String,
    /// Ticket keys matched by `reviews.ticket_key_regex` over branch + title.
    #[serde(default)]
    pub linked_tickets: Vec<String>,
    #[serde(default)]
    pub additions: Option<u32>,
    #[serde(default)]
    pub deletions: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct ReviewQuery {
    pub kind: ReviewKind,
    pub include_team: bool,
    pub include_drafts: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct Reviewer {
    pub user: User,
    #[serde(default)]
    pub state: Option<MyReviewState>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct CiCheck {
    pub name: String,
    pub state: CiState,
    #[serde(default)]
    pub url: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct FileChange {
    pub path: String,
    pub additions: u32,
    pub deletions: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct ReviewDetail {
    pub review: Review,
    /// Sanitized HTML description.
    pub body_html: String,
    pub reviewers: Vec<Reviewer>,
    pub checks: Vec<CiCheck>,
    pub files: Vec<FileChange>,
    /// Line comments waiting in my pending (draft) review; published by approve / comment / request changes.
    #[serde(default)]
    pub pending_comments: u32,
}

/// Create-PR request sent to a `CodeHost`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct PrCreate {
    pub repo: String,
    pub head: String,
    pub base: String,
    pub title: String,
    pub body: String,
    pub draft: bool,
}

/// User-editable PR draft (`work_create_pr`); `None` fields fall back to `work.pr.*` templates.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct PrDraft {
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub body: Option<String>,
    #[serde(default)]
    pub draft: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
pub struct ReviewItem {
    pub review: Review,
    pub project_ids: Vec<ProjectId>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, TS)]
pub struct ReviewPage {
    pub items: Vec<ReviewItem>,
    pub stale: bool,
    pub errors: Vec<AccountError>,
}

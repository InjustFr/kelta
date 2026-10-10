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
    Bitbucket,
    Gitea,
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
    /// Head commit the latest decisive review (changes requested / approved) was left on.
    #[serde(default)]
    #[ts(optional = nullable)]
    pub decision_head: Option<String>,
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

/// Review feedback on a PR for Fix with Claude (FLOW §4.2): unresolved threads, review summaries
/// with a body, failed checks with their log tail.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct Feedback {
    pub threads: Vec<FeedbackThread>,
    pub reviews: Vec<FeedbackReview>,
    pub failed_checks: Vec<FailedCheck>,
    /// Logins that reviewed (re-request review targets).
    pub reviewers: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct FeedbackThread {
    /// Host id used to resolve the thread (GraphQL node id / GitLab discussion id).
    pub id: String,
    pub author: String,
    pub path: Option<String>,
    pub line: Option<u32>,
    /// The thread's comments, oldest first, as `author: body` Markdown.
    pub body_md: String,
    pub url: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct FeedbackReview {
    pub author: String,
    pub state: Option<MyReviewState>,
    pub body_md: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct FailedCheck {
    pub name: String,
    pub url: Option<String>,
    /// Last 40 log lines when the host exposes them.
    pub log_tail: Option<String>,
}

impl Feedback {
    /// Markdown brief for Claude (MCP `get_review_feedback`; the Fix sheet writes the same layout).
    pub fn to_markdown(&self) -> String {
        let mut s = String::from("# Review feedback\n");
        if self.threads.is_empty() && self.reviews.is_empty() && self.failed_checks.is_empty() {
            s.push_str("\nNo unresolved threads, review summaries or failed checks.\n");
        }
        if !self.threads.is_empty() {
            s.push_str("\n## Unresolved threads\n");
            for t in &self.threads {
                let at = match (&t.path, t.line) {
                    (Some(p), Some(l)) => format!(" on `{p}:{l}`"),
                    (Some(p), None) => format!(" on `{p}`"),
                    _ => String::new(),
                };
                s.push_str(&format!("\n### {}{at}\n\n{}\n\n<{}>\n", t.author, t.body_md.trim(), t.url));
            }
        }
        if !self.reviews.is_empty() {
            s.push_str("\n## Reviews\n");
            for r in &self.reviews {
                let state = match r.state {
                    Some(MyReviewState::ChangesRequested) => " (changes requested)",
                    Some(MyReviewState::Approved) => " (approved)",
                    _ => "",
                };
                s.push_str(&format!("\n### {}{state}\n\n{}\n", r.author, r.body_md.trim()));
            }
        }
        if !self.failed_checks.is_empty() {
            s.push_str("\n## Failed checks\n");
            for c in &self.failed_checks {
                s.push_str(&format!("\n### {}\n", c.name));
                if let Some(u) = &c.url {
                    s.push_str(&format!("\n<{u}>\n"));
                }
                if let Some(t) = &c.log_tail {
                    s.push_str(&format!("\n```text\n{}\n```\n", t.trim_end()));
                }
            }
        }
        s
    }
}

#[cfg(test)]
mod feedback_tests {
    use super::*;

    #[test]
    fn markdown_lists_every_part() {
        let f = Feedback {
            threads: vec![FeedbackThread {
                id: "t".into(),
                author: "bob".into(),
                path: Some("src/a.rs".into()),
                line: Some(3),
                body_md: "bob: rename".into(),
                url: "https://h/1".into(),
            }],
            reviews: vec![FeedbackReview {
                author: "ann".into(),
                state: Some(MyReviewState::ChangesRequested),
                body_md: "close".into(),
            }],
            failed_checks: vec![FailedCheck {
                name: "test".into(),
                url: None,
                log_tail: Some("boom".into()),
            }],
            reviewers: vec![],
        };
        let md = f.to_markdown();
        for part in
            ["### bob on `src/a.rs:3`", "### ann (changes requested)", "### test", "```text\nboom\n```"]
        {
            assert!(md.contains(part), "{part} in {md}");
        }
        assert!(Feedback::default().to_markdown().contains("No unresolved"));
    }
}

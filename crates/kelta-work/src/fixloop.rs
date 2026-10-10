//! Feedback loop (FLOW §4.2): send a prompt (and brief files) into the item's previous Claude
//! conversation, read the PR's review feedback, re-request review, resolve sent threads.

use std::sync::Arc;

use kelta_proto::api::{CodeHost, CoreApi};
use kelta_proto::codehost::{Feedback, MergeMethod, ReviewRef};
use kelta_proto::error::KeltaError;
use kelta_proto::ids::WorkItemId;
use kelta_proto::model::{
    Lifecycle, SendFile, SessionInfo, SessionKind, SessionStatus, StatusSource, WorkItem, WorkKind, WorkState,
};

use crate::saga::{Env, Journal};
use crate::template::{Mode, render};
use crate::{WorkService, files};

/// Files Kelta itself writes into the run dir; a brief must not replace them.
const RESERVED: &[&str] = &[crate::claude::CONTEXT_FILE, crate::claude::TICKET_FILE];

/// The item's Claude session unless it exited.
pub(crate) fn claude_of(core: &Arc<dyn CoreApi>, item: &WorkItem, j: &Journal) -> Option<SessionInfo> {
    j.claude_session
        .as_ref()
        .and_then(|s| core.session_get(s))
        .or_else(|| {
            core.session_list(Some(&item.project_id))
                .into_iter()
                .find(|s| s.work_item_id.as_ref() == Some(&item.id) && s.kind == SessionKind::Claude)
        })
        .filter(|s| s.lifecycle != Lifecycle::Exited)
}

fn busy(s: &SessionInfo) -> bool {
    s.lifecycle == Lifecycle::Live && matches!(s.status, SessionStatus::Working | SessionStatus::NeedsInput)
}

/// Ship / push / rebase wait for Claude's turn to end (FLOW §4.5): never act on a half-done turn.
pub(crate) fn refuse_if_busy(
    env: &Env,
    item: &WorkItem,
    j: &Journal,
    action: &str,
) -> Result<(), KeltaError> {
    match claude_of(&env.core, item, j) {
        Some(s) if busy(&s) => {
            Err(KeltaError::conflict(format!("Claude is working in this worktree. {action} when it stops."))
                .with_detail(serde_json::json!({ "reason": "claude_busy" })))
        }
        _ => Ok(()),
    }
}

/// Typing into Claude is only safe when hooks say it is idle at its prompt: a paste into a
/// permission prompt would answer it (FLOW §11).
fn check_idle(s: &SessionInfo) -> Result<(), KeltaError> {
    let reason = if busy(s) {
        "claude_busy"
    } else if s.status_source != StatusSource::Hook {
        return Err(KeltaError::conflict("Kelta can't tell whether Claude is idle (status hooks inactive).")
            .with_detail(serde_json::json!({ "reason": "hooks_inactive" })));
    } else if matches!(s.status, SessionStatus::Done | SessionStatus::WaitingUser) {
        return Ok(());
    } else {
        "claude_busy"
    };
    Err(KeltaError::conflict("Claude is busy; send when it stops.")
        .with_detail(serde_json::json!({ "reason": reason })))
}

/// Bracketed paste of `prompt` into a live, idle Claude, then submit.
pub(crate) async fn paste_prompt(
    core: &Arc<dyn CoreApi>,
    s: &SessionInfo,
    prompt: &str,
) -> Result<(), KeltaError> {
    check_idle(s)?;
    // No control bytes: an ESC could close the bracket and run keys in Claude's TUI.
    let clean: String = prompt.chars().filter(|c| !c.is_control() || *c == '\n' || *c == '\t').collect();
    core.session_write(&s.id, format!("\x1b[200~{clean}\x1b[201~").as_bytes()).await?;
    // shortcut: Enter is a separate write right after the paste; if Claude ever folds it into the
    // paste, add a write barrier (wait for the echo) here.
    core.session_write(&s.id, b"\r").await
}

fn valid_name(name: &str) -> bool {
    name.strip_suffix(".md").is_some_and(|stem| {
        !stem.is_empty() && stem.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    }) && !RESERVED.contains(&name)
}

impl WorkService {
    /// `work_send`: brief files into the private run dir, then the prompt into the previous
    /// conversation (paste when live and idle, `claude --resume <uuid> -- <prompt>` otherwise).
    pub(crate) async fn send_impl(
        &self,
        id: &WorkItemId,
        prompt: &str,
        sent: Vec<SendFile>,
        threads: Option<Vec<String>>,
    ) -> Result<WorkItem, KeltaError> {
        if prompt.trim().is_empty() {
            return Err(KeltaError::invalid("the prompt is empty"));
        }
        if let Some(f) = sent.iter().find(|f| !valid_name(&f.name)) {
            return Err(KeltaError::invalid(format!(
                "invalid brief file name `{}` (expected name.md)",
                f.name
            )));
        }
        let item = self.load(id).await?;
        if item.claude_uuid.is_none() {
            return Err(KeltaError::invalid("this work item has no Claude conversation"));
        }
        let env = self.env(&item.project_id, &item.repo_id)?;
        let mut j = self.load_journal(id);
        // Refuse before writing anything (re-checked right before the paste).
        if let Some(s) = claude_of(&env.core, &item, &j).filter(|s| s.lifecycle == Lifecycle::Live) {
            check_idle(&s)?;
        }
        let run = self.ensure_claude_run(&item, &mut j)?;
        self.save_journal(id, &j)?;
        for f in &sent {
            files::write_private(&run.join(&f.name), f.content.as_bytes())?;
        }
        let mut ctx = self.item_ctx(&env, &item, &j);
        if let Some(f) = sent.first() {
            ctx.set("file", run.join(&f.name).to_string_lossy().into_owned());
        }
        if let Some(url) = &item.pr_url {
            ctx.set("pr.url", url.clone());
        }
        if let Some(r) = &item.rebase {
            ctx.set("onto", r.onto.clone());
            ctx.set("step", r.step.to_string());
            ctx.set("total", r.total.to_string());
        }
        let text = render(prompt, &ctx, Mode::Lenient)?;
        let mut item = self.resume_item(id, Some(text)).await?;
        if let Some(t) = threads {
            item = self
                .update(id, |w| {
                    w.sent_threads = t;
                    true
                })
                .await?;
        }
        Ok(item)
    }

    /// Code host + PR of a work item (its review ref, or its bound repo + `pr_url`).
    async fn pr_of(&self, item: &WorkItem) -> Result<(Arc<dyn CodeHost>, ReviewRef), KeltaError> {
        let env = self.env(&item.project_id, &item.repo_id)?;
        if let Some(r) = &item.review {
            return Ok((env.core.code_host_for(&r.account).await?, r.clone()));
        }
        let url = item
            .pr_url
            .as_deref()
            .ok_or_else(|| KeltaError::invalid("this work item has no pull request"))?;
        let binding = env.repo.code_host.clone().ok_or_else(|| {
            KeltaError::unsupported(format!("repo {} has no code host binding", env.repo.id))
        })?;
        let number = pr_number(url).ok_or_else(|| KeltaError::invalid(format!("no PR number in {url}")))?;
        let host = env.core.code_host_for(&binding.account).await?;
        Ok((host, ReviewRef { account: binding.account, repo: binding.repo, number }))
    }

    pub(crate) async fn feedback_impl(&self, id: &WorkItemId) -> Result<Feedback, KeltaError> {
        let item = self.load(id).await?;
        let (host, r) = self.pr_of(&item).await?;
        host.feedback(&r).await
    }

    pub(crate) async fn rerequest_impl(&self, id: &WorkItemId) -> Result<Vec<String>, KeltaError> {
        let item = self.load(id).await?;
        if item.kind == WorkKind::Review {
            return Err(KeltaError::conflict("Review checkout: read-only"));
        }
        let (host, r) = self.pr_of(&item).await?;
        host.rerequest_review(&r).await
    }

    /// `work_arm_merge` (`Some(method)`) / `work_disarm_merge` (`None`): the host's native auto-merge;
    /// a refusal carries the host's message and leaves `auto_finish` as it was.
    pub(crate) async fn arm_merge_impl(
        &self,
        id: &WorkItemId,
        method: Option<MergeMethod>,
    ) -> Result<WorkItem, KeltaError> {
        let item = self.load(id).await?;
        if item.kind == WorkKind::Review {
            return Err(KeltaError::conflict("Review checkout: not your pull request"));
        }
        if item.state != WorkState::PrOpen {
            return Err(KeltaError::conflict("only an open pull request can be merged when ready"));
        }
        let (host, r) = self.pr_of(&item).await?;
        match method {
            Some(m) => host.arm_auto_merge(&r, m).await?,
            None => host.disarm_auto_merge(&r).await?,
        }
        let on = method.is_some();
        self.update(id, |w| std::mem::replace(&mut w.auto_finish, on) != on).await
    }

    pub(crate) async fn resolve_sent_impl(&self, id: &WorkItemId) -> Result<WorkItem, KeltaError> {
        let item = self.load(id).await?;
        if item.sent_threads.is_empty() {
            return Err(KeltaError::invalid("no review threads were sent to Claude"));
        }
        let (host, r) = self.pr_of(&item).await?;
        host.resolve_threads(&r, &item.sent_threads).await?;
        self.update(id, |w| {
            w.sent_threads.clear();
            true
        })
        .await
    }
}

/// `…/pull/74`, `…/-/merge_requests/12#note_3` → the number.
fn pr_number(url: &str) -> Option<u64> {
    url.split(['#', '?']).next()?.trim_end_matches('/').rsplit('/').next()?.parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn brief_names_and_pr_numbers() {
        assert!(valid_name("feedback.md") && valid_name("conflicts.md"));
        for bad in ["../x.md", "a/b.md", ".md", "x.txt", "ticket.md", "context.md", ".hidden.md"] {
            assert!(!valid_name(bad), "{bad}");
        }
        assert_eq!(pr_number("https://github.com/a/b/pull/74"), Some(74));
        assert_eq!(pr_number("https://gitlab.com/g/p/-/merge_requests/12#note_3"), Some(12));
        assert_eq!(pr_number("https://x/y"), None);
    }

    #[test]
    fn idle_only_with_hook_status() {
        let mut s = kelta_proto::samples::session_info();
        s.lifecycle = Lifecycle::Live;
        s.status_source = StatusSource::Hook;
        for (st, ok) in [
            (SessionStatus::Done, true),
            (SessionStatus::WaitingUser, true),
            (SessionStatus::Working, false),
            (SessionStatus::NeedsInput, false),
            (SessionStatus::Running, false),
        ] {
            s.status = st;
            assert_eq!(check_idle(&s).is_ok(), ok, "{st:?}");
        }
        s.status = SessionStatus::Done;
        s.status_source = StatusSource::Heuristic;
        let e = check_idle(&s).unwrap_err();
        assert_eq!(e.detail.unwrap()["reason"], "hooks_inactive");
    }
}

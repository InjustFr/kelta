//! Feedback loop (FLOW §4.2): send a prompt (and brief files) into the item's previous Claude
//! conversation, read the PR's review feedback, re-request review, resolve sent threads.

use std::sync::Arc;

use kelta_proto::api::{CodeHost, CoreApi};
use kelta_proto::codehost::{Feedback, MergeMethod, Review, ReviewRef};
use kelta_proto::error::KeltaError;
use kelta_proto::events::Toast;
use kelta_proto::ids::WorkItemId;
use kelta_proto::model::{
    Lifecycle, SendFile, SessionInfo, SessionKind, SessionStatus, StatusSource, WorkItem, WorkKind, WorkState,
};

use crate::notes::{Route, route};
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

pub(crate) fn busy(s: &SessionInfo) -> bool {
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
    s.refuse_hosted()?;
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

/// A send to this live Claude goes to the clipboard: Kelta cannot tell when it is idle.
fn copies(s: &SessionInfo) -> bool {
    s.hosted() || (route(Some(s)) == Route::Paste && check_idle(s).is_err())
}

fn valid_name(name: &str) -> bool {
    name.strip_suffix(".md").is_some_and(|stem| {
        !stem.is_empty() && stem.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    }) && !RESERVED.contains(&name)
}

/// Remove the `name-N.md` copies held sends wrote into `run` (`send_impl`): once the turn that read
/// them ended with nothing held, no prompt names them any more (#319).
pub(crate) fn clear_held_copies(run: &std::path::Path) {
    let copy = |n: &str| {
        valid_name(n)
            && n.trim_end_matches(".md")
                .rsplit_once('-')
                .is_some_and(|(_, i)| i.parse::<u32>().is_ok_and(|i| i >= 2))
    };
    for e in std::fs::read_dir(run).into_iter().flatten().flatten() {
        if e.file_name().to_str().is_some_and(copy) {
            let _ = std::fs::remove_file(e.path());
        }
    }
}

impl WorkService {
    /// `work_send`: brief files into the private run dir, then the prompt into the previous
    /// conversation through [`WorkService::deliver_prompt`] (pasted, held until `Stop`, or resumed),
    /// or onto the clipboard when Kelta cannot tell when Claude is idle.
    pub(crate) async fn send_impl(
        &self,
        id: &WorkItemId,
        prompt: &str,
        sent: Vec<SendFile>,
        threads: Option<Vec<String>>,
        feedback_at: Option<String>,
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
        // Never refused: a live Claude Kelta cannot paste into (hooks inactive, so no `Stop` to hold
        // the message for) gets the prompt on the clipboard instead.
        let copy = claude_of(&env.core, &item, &j).is_some_and(|s| copies(&s));
        let run = self.ensure_claude_run(&item, &mut j)?;
        self.save_journal(id, &j)?;
        let mut first = None;
        for f in &sent {
            // A held prompt may still need an existing brief: this send writes its own (`name-2.md`, ...).
            let path = (1..)
                .map(|i| match i {
                    1 => f.name.clone(),
                    i => format!("{}-{i}.md", f.name.trim_end_matches(".md")),
                })
                .filter(|n| valid_name(n))
                .map(|n| run.join(n))
                .find(|p| j.pending_prompt.is_none() || !p.exists())
                .unwrap_or_else(|| run.join(&f.name));
            files::write_private(&path, f.content.as_bytes())?;
            first.get_or_insert(path);
        }
        let mut ctx = self.item_ctx(&env, &item, &j);
        if let Some(p) = first {
            ctx.set("file", p.to_string_lossy().into_owned());
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
        if copy {
            env.core.clipboard_copy(text).await?;
            env.core.toast(Toast::info(
                "This Claude isn't connected to Kelta: the feedback is copied, paste it when Claude is ready.",
            ));
        } else {
            self.deliver_prompt(id, &text).await?;
        }
        if threads.is_none() && feedback_at.is_none() {
            return self.load(id).await;
        }
        self.update(id, |w| {
            if let Some(t) = threads {
                w.sent_threads = t;
            }
            // The watermark never moves back: an older sheet sent late keeps the newer one.
            w.feedback_at = kelta_proto::later_rfc3339(w.feedback_at.take(), feedback_at);
            true
        })
        .await
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

    /// The `fix_checks` trigger action (#252): the failed checks of `review` to its work item's Claude,
    /// as Enter on Checks failed sends them, once per head SHA and failed-check set. `Ok(reason)`
    /// when nothing is sent.
    pub(crate) async fn fix_checks_impl(&self, review: &Review) -> Result<String, KeltaError> {
        let skip = |why: &str| {
            tracing::debug!(pr = %review.url, "fix_checks skipped: {why}");
            Ok(format!("skipped: {why}"))
        };
        let items = self.store.list_items(None).await?;
        let Some(item) = items
            .into_iter()
            .find(|w| w.state == WorkState::PrOpen && w.pr_url.as_deref() == Some(review.url.as_str()))
        else {
            return skip("no work item owns this PR");
        };
        if item.claude_uuid.is_none() {
            return skip("the work item has no Claude");
        }
        let env = self.env(&item.project_id, &item.repo_id)?;
        let j = self.load_journal(&item.id);
        if claude_of(&env.core, &item, &j).is_some_and(|s| copies(&s)) {
            return skip("Claude's status hooks are inactive");
        }
        let label = item.ticket.as_ref().map_or(item.branch.as_str(), |t| t.key.as_str());
        if let Some(budget) = env.settings.claude.budget_usd {
            let unsaved: f64 = env
                .core
                .session_list(Some(&item.project_id))
                .iter()
                .filter(|s| s.work_item_id.as_ref() == Some(&item.id))
                .filter_map(|s| s.claude.as_ref()?.usage.as_ref().map(|u| u.unsaved_usd))
                .sum();
            if item.cost_usd + unsaved > budget {
                env.core.toast(Toast::warn(format!(
                    "{label}: CI failed. Not sent to Claude: the item is over its ${budget:.2} budget."
                )));
                return Ok("skipped: over budget".into());
            }
        }
        let failed = self.feedback_impl(&item.id).await?.failed_checks;
        if failed.is_empty() {
            return skip("no failed check");
        }
        let mut names: Vec<&str> = failed.iter().map(|c| c.name.as_str()).collect();
        names.sort_unstable();
        let sent = format!("{}:{}", review.head_sha, names.join(","));
        if j.checks_sent.as_deref() == Some(sent.as_str()) {
            return skip("already sent for this head");
        }
        let n = failed.len();
        let brief = Feedback { failed_checks: failed, ..Feedback::default() }.to_markdown();
        let prompt = env.settings.claude.prompt_templates.get("checks").cloned().unwrap_or_default();
        let file = SendFile { name: "feedback.md".into(), content: brief };
        self.send_impl(&item.id, &prompt, vec![file], None, None).await?;
        // send_impl saved the journal: reload, so its run dir / held prompt are kept.
        let mut j = self.load_journal(&item.id);
        j.checks_sent = Some(sent);
        self.save_journal(&item.id, &j)?;
        let what = format!("{n} failed check{}", if n == 1 { "" } else { "s" });
        env.core.toast(Toast::info(format!("{label}: sent {what} to Claude.")));
        Ok(format!("sent {what}"))
    }

    pub(crate) async fn rerequest_impl(&self, id: &WorkItemId) -> Result<Vec<String>, KeltaError> {
        let item = self.load(id).await?;
        if item.kind == WorkKind::Review {
            return Err(KeltaError::conflict("Review checkout: read-only"));
        }
        let (host, r) = self.pr_of(&item).await?;
        host.rerequest_review(&r, &[]).await
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
        s.status_source = StatusSource::Hook;
        s.claude.get_or_insert_with(Default::default).host =
            Some(kelta_proto::settings::ClaudeHost::Terminal);
        assert!(check_idle(&s).is_err() && copies(&s));
        s.status = SessionStatus::Working; // busy: route is AtStop, still a copy
        assert!(copies(&s));
    }
}

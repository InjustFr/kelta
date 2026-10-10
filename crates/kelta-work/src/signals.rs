//! Durable Claude signals on work items (FLOW §2.3): `review_due` / `claude_replied` from real
//! `Stop` hooks (changes = worktree moved since the last prompt), cleared by `UserPromptSubmit` and Mark reviewed; `claude_uuid` follows the hook
//! session id (B3). Everything travels in `work.updated`.

use kelta_proto::error::KeltaError;
use kelta_proto::events::Notification;
use kelta_proto::ext::Urgency;
use kelta_proto::hooks::{HookPayload, names};
use kelta_proto::ids::{SessionId, WorkItemId};
use kelta_proto::model::{SessionKind, WorkItem, WorkKind, WorkState};

use crate::{WorkService, git};

/// Short name of an item in notifications: ticket key, `#n` for a review, else the branch.
pub(crate) fn item_key(item: &WorkItem) -> String {
    match (&item.ticket, &item.review) {
        (Some(t), _) => t.key.clone(),
        (None, Some(r)) => format!("#{}", r.number),
        _ => item.branch.clone(),
    }
}

impl WorkService {
    /// A `claude.hook` of `sid`. The bus only carries real hooks, so heuristic sessions (hooks
    /// inactive) never get here and never set a flag.
    pub(crate) async fn on_claude_hook(&self, sid: &SessionId, hook: HookPayload) -> Result<(), KeltaError> {
        let event = hook.hook_event_name.as_str();
        // A new session id comes with SessionStart (/clear, in-Claude /resume); tool hooks are noise.
        if ![names::SESSION_START, names::USER_PROMPT_SUBMIT, names::STOP].contains(&event) {
            return Ok(());
        }
        let core = self.api()?;
        if core.session_get(sid).is_none_or(|s| s.kind != SessionKind::Claude) {
            return Ok(());
        }
        let Some(item) = self.for_session(sid).await else { return Ok(()) };
        if item.state == WorkState::Finished {
            return Ok(());
        }
        // Review checkouts are someone else's code: no "to review" signal for them.
        let own = item.kind != WorkKind::Review;
        let changes = match event {
            names::USER_PROMPT_SUBMIT if own => {
                // A missing worktree just leaves no mark (Stop then falls back).
                match git::fingerprint(&item.worktree).await {
                    Ok(fp) => self.prompt_marks.lock().insert(item.id.clone(), fp),
                    Err(_) => self.prompt_marks.lock().remove(&item.id),
                };
                None
            }
            // Changes since the prompt; without a mark (Kelta restarted) anything ahead of base or dirty.
            names::STOP if own => {
                let mark = self.prompt_marks.lock().get(&item.id).copied();
                match mark {
                    Some(m) => Some(git::fingerprint(&item.worktree).await? != m),
                    None => {
                        let env = self.env(&item.project_id, &item.repo_id)?;
                        let st = self.git_status(&env, &item).await?;
                        Some(st.ahead > 0 || st.dirty)
                    }
                }
            }
            _ => None,
        };
        let uuid = hook.session_id.filter(|u| !u.is_empty());
        let item = self
            .update(&item.id, |w| {
                let before = (w.review_due, w.claude_replied, w.claude_uuid.clone());
                if uuid.is_some() {
                    w.claude_uuid.clone_from(&uuid);
                }
                match (event, changes) {
                    (names::USER_PROMPT_SUBMIT, _) => (w.review_due, w.claude_replied) = (false, false),
                    (names::STOP, Some(true)) => (w.review_due, w.claude_replied) = (true, false),
                    (names::STOP, Some(false)) => w.claude_replied = true,
                    _ => {}
                }
                before != (w.review_due, w.claude_replied, w.claude_uuid.clone())
            })
            .await?;
        // Core leaves the "finished" notification of a work item's Claude to us.
        if event == names::STOP && core.settings(Some(&item.project_id)).notifications.claude_done {
            let key = item_key(&item);
            let title = match changes {
                Some(true) => format!("{key} ready to review"),
                Some(false) => format!("{key}: Claude replied"),
                None => format!("{key}: Claude finished"),
            };
            let body = hook.last_assistant_message.map(|m| m.chars().take(200).collect());
            // Core drops it when the session's pane is visible in the focused window.
            core.notify(Notification {
                title,
                body,
                urgency: Urgency::Normal,
                project_id: Some(item.project_id.clone()),
                session_id: Some(sid.clone()),
            })
            .await?;
        }
        Ok(())
    }

    /// `work_mark_reviewed`: Louis looked at Claude's changes.
    pub async fn mark_reviewed(&self, id: &WorkItemId) -> Result<WorkItem, KeltaError> {
        self.update(id, |w| std::mem::replace(&mut w.review_due, false)).await
    }
}

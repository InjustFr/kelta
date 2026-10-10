//! Durable Claude signals on work items (FLOW §2.3): `review_due` / `claude_replied` from real
//! `Stop` hooks (changes = a non-empty delta since Louis's last review, `review.rs`), cleared by
//! `UserPromptSubmit` and Mark reviewed; `claude_uuid` follows the hook
//! session id (B3). Everything travels in `work.updated`.

use kelta_proto::error::KeltaError;
use kelta_proto::events::Notification;
use kelta_proto::ext::Urgency;
use kelta_proto::hooks::{HookPayload, names};
use kelta_proto::ids::{SessionId, WorkItemId};
use kelta_proto::model::{SessionKind, WorkItem, WorkKind, WorkState};

use crate::WorkService;

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
        // Claude asks: the hooks kelta-server maps to NeedsInput.
        let asks = event == names::PERMISSION_REQUEST
            || (event == names::NOTIFICATION
                && matches!(
                    hook.notification_type.as_deref(),
                    Some("permission_prompt" | "elicitation_dialog" | "agent_needs_input")
                ));
        // A new session id comes with SessionStart (/clear, in-Claude /resume); tool hooks are noise.
        if !asks && ![names::SESSION_START, names::USER_PROMPT_SUBMIT, names::STOP].contains(&event) {
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
        // Stop: snapshot, then what changed since Louis's last look (`None` = nothing new).
        let delta = match event {
            names::STOP if own => {
                let env = self.env(&item.project_id, &item.repo_id)?;
                Some(self.stop_delta(&env, &item).await?)
            }
            _ => None,
        };
        let uuid = hook.session_id.filter(|u| !u.is_empty());
        let message = hook.last_assistant_message;
        let item = self
            .update(&item.id, |w| {
                let before = (w.review_due, w.claude_replied, w.claude_uuid.clone());
                if uuid.is_some() {
                    w.claude_uuid.clone_from(&uuid);
                }
                if asks || event == names::STOP {
                    w.claude_at = Some(kelta_proto::now_rfc3339());
                }
                if event == names::STOP {
                    w.claude_message.clone_from(&message);
                }
                match (event, delta) {
                    (names::USER_PROMPT_SUBMIT, _) => (w.review_due, w.claude_replied) = (false, false),
                    (names::STOP, Some(d)) => {
                        (w.review_due, w.claude_replied, w.delta) = (d.is_some(), d.is_none(), d);
                    }
                    _ => {}
                }
                asks || event == names::STOP
                    || before != (w.review_due, w.claude_replied, w.claude_uuid.clone())
            })
            .await?;
        // Core leaves the "finished" notification of a work item's Claude to us.
        if event == names::STOP && core.settings(Some(&item.project_id)).notifications.claude_done {
            let key = item_key(&item);
            let title = match delta {
                Some(Some(_)) => format!("{key} ready to review"),
                Some(None) => format!("{key}: Claude replied"),
                None => format!("{key}: Claude finished"),
            };
            let body = message.map(|m| m.chars().take(200).collect());
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

    /// `work_mark_reviewed`: Louis looked at Claude's changes; `reviewed` = `last`, so the next
    /// delta starts here (best effort: a git failure still clears the flag).
    pub async fn mark_reviewed(&self, id: &WorkItemId) -> Result<WorkItem, KeltaError> {
        let item = self.load(id).await?;
        if item.kind != WorkKind::Review
            && item.worktree.is_dir()
            && let Err(e) = self.stamp_reviewed(&item).await
        {
            tracing::warn!(work_item = %id, error = %e.message, "reviewed ref not stamped");
        }
        self.update(id, |w| {
            let changed = w.review_due || w.delta.is_some();
            (w.review_due, w.delta) = (false, None);
            changed
        })
        .await
    }
}

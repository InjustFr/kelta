//! Claude status machine (ARCHITECTURE §7.6) and hook ingestion shared by the ctl socket and the
//! `/hook/<sid>` HTTP transport.

use std::sync::Arc;

use kelta_proto::api::CoreApi;
use kelta_proto::error::KeltaError;
use kelta_proto::events::{BusEvent, bus};
use kelta_proto::hooks::{HookPayload, names};
use kelta_proto::ids::SessionId;
use kelta_proto::model::{SessionStatus, StatusChange};

/// Max chars of the `Stop` preview (`last_assistant_message`).
pub const PREVIEW_CHARS: usize = 200;

/// Tools whose `PostToolUse` reports an edited file.
const EDIT_TOOLS: &[&str] = &["Edit", "Write", "MultiEdit"];

/// Map a hook payload to a status change; `None` for ignored events.
///
/// | Hook | Status |
/// |---|---|
/// | SessionStart | Running |
/// | UserPromptSubmit | Working |
/// | PermissionRequest; Notification `permission_prompt`/`elicitation_dialog`/`agent_needs_input` | NeedsInput |
/// | Notification `idle_prompt` | WaitingUser |
/// | Stop | Done (preview = `last_assistant_message`, 200 chars) |
/// | StopFailure | Error |
/// | SessionEnd | Exited |
/// | PostToolUse `Edit`/`Write`/`MultiEdit` | Unknown (unchanged) + `file_edited` |
pub fn map(payload: &HookPayload) -> Option<StatusChange> {
    let event = payload.hook_event_name.as_str();
    let change = |status: SessionStatus| StatusChange {
        status,
        preview: None,
        file_edited: None,
        raw_event: event.to_owned(),
        session_uuid: payload.session_id.clone().filter(|u| !u.is_empty()),
    };
    match event {
        names::SESSION_START => Some(change(SessionStatus::Running)),
        names::USER_PROMPT_SUBMIT => Some(change(SessionStatus::Working)),
        names::PERMISSION_REQUEST => Some(change(SessionStatus::NeedsInput)),
        names::NOTIFICATION => {
            let kind = payload.notification_type.as_deref()?;
            let status = match kind {
                "permission_prompt" | "elicitation_dialog" | "agent_needs_input" => SessionStatus::NeedsInput,
                "idle_prompt" => SessionStatus::WaitingUser,
                _ => return None,
            };
            Some(StatusChange { raw_event: format!("{event}:{kind}"), ..change(status) })
        }
        names::STOP => Some(StatusChange {
            preview: payload.last_assistant_message.as_deref().and_then(preview),
            ..change(SessionStatus::Done)
        }),
        names::STOP_FAILURE => Some(change(SessionStatus::Error)),
        names::SESSION_END => Some(change(SessionStatus::Exited)),
        names::POST_TOOL_USE => {
            let tool = payload.tool_name.as_deref()?;
            if !EDIT_TOOLS.contains(&tool) {
                return None;
            }
            let file = payload.edited_file()?;
            Some(StatusChange { file_edited: Some(file), ..change(SessionStatus::Unknown) })
        }
        _ => None,
    }
}

/// First [`PREVIEW_CHARS`] chars of the trimmed message; `None` when empty.
fn preview(msg: &str) -> Option<String> {
    let t = msg.trim();
    if t.is_empty() {
        return None;
    }
    let mut out: String = t.chars().take(PREVIEW_CHARS).collect();
    if t.chars().nth(PREVIEW_CHARS).is_some() {
        out.push('…');
    }
    Some(out)
}

/// The value Claude's hook `matcher` is matched against, per event.
fn matcher_value(p: &HookPayload) -> Option<String> {
    match p.hook_event_name.as_str() {
        names::NOTIFICATION => p.notification_type.clone(),
        names::PERMISSION_REQUEST | names::POST_TOOL_USE | "PreToolUse" => p.tool_name.clone(),
        names::SESSION_START => p.source.clone(),
        names::SESSION_END => p.reason.clone(),
        _ => None,
    }
}

/// Ingest one hook for `sid` (already authenticated): apply the status change to core and publish
/// `claude.hook` (always) and `claude.file_edited` (edits).
pub async fn ingest(
    core: &Arc<dyn CoreApi>,
    sid: &SessionId,
    payload: HookPayload,
) -> Result<(), KeltaError> {
    // Statusline refreshes are frequent: they only update the session's usage, no bus event.
    if payload.hook_event_name == names::STATUS {
        return match payload.usage() {
            Some(u) => core.session_set_usage(sid, u).await,
            None => Ok(()),
        };
    }
    let change = map(&payload);
    let project = core.session_get(sid).map(|s| s.project_id);
    let with_ctx = |ev: BusEvent| {
        let ev = ev.with_session(sid.clone());
        match &project {
            Some(p) => ev.with_project(p.clone()),
            None => ev,
        }
    };

    let mut result = Ok(());
    if let Some(change) = &change {
        result = core.session_apply_hook(sid, change.clone()).await;
    }

    let raw = serde_json::to_value(&payload).unwrap_or(serde_json::Value::Null);
    core.publish(with_ctx(BusEvent::new(
        bus::CLAUDE_HOOK,
        serde_json::json!({
            "event": payload.hook_event_name,
            "matcher_value": matcher_value(&payload),
            "payload": raw,
        }),
    )));

    if let Some(path) = change.and_then(|c| c.file_edited) {
        core.publish(with_ctx(BusEvent::new(
            bus::CLAUDE_FILE_EDITED,
            serde_json::json!({ "path": path, "tool": payload.tool_name }),
        )));
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preview_truncates_on_chars() {
        let long = "é".repeat(300);
        let p = preview(&long).unwrap();
        assert_eq!(p.chars().count(), PREVIEW_CHARS + 1);
        assert_eq!(preview("  "), None);
        assert_eq!(preview(" ok ").as_deref(), Some("ok"));
    }

    #[test]
    fn unknown_and_unmatched_events_are_ignored() {
        let mut p = HookPayload { hook_event_name: "PreToolUse".into(), ..Default::default() };
        assert!(map(&p).is_none());
        p.hook_event_name = names::NOTIFICATION.into();
        assert!(map(&p).is_none());
        p.notification_type = Some("auth_success".into());
        assert!(map(&p).is_none());
        p.hook_event_name = names::POST_TOOL_USE.into();
        p.tool_name = Some("Bash".into());
        assert!(map(&p).is_none());
    }
}

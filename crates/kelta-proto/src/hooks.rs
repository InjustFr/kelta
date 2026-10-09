//! Claude Code hook payloads (tolerant serde: unknown fields are kept in `extra`, every field optional).

use std::collections::BTreeMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, TS)]
#[serde(default)]
pub struct HookPayload {
    /// `SessionStart`, `UserPromptSubmit`, `PermissionRequest`, `Notification`, `PostToolUse`,
    /// `Stop`, `StopFailure`, `SessionEnd`, ...
    pub hook_event_name: String,
    /// Claude's session uuid.
    pub session_id: Option<String>,
    pub transcript_path: Option<PathBuf>,
    pub cwd: Option<PathBuf>,
    /// Notification: `permission_prompt` | `idle_prompt` | `elicitation_dialog` | `agent_needs_input`.
    pub notification_type: Option<String>,
    pub message: Option<String>,
    /// Stop.
    pub last_assistant_message: Option<String>,
    /// PreToolUse/PostToolUse/PermissionRequest.
    pub tool_name: Option<String>,
    pub tool_input: Option<serde_json::Value>,
    /// UserPromptSubmit.
    pub prompt: Option<String>,
    /// SessionStart: `startup` | `resume` | `clear` | `compact`.
    pub source: Option<String>,
    /// SessionEnd reason.
    pub reason: Option<String>,
    /// Everything else, preserved.
    #[serde(flatten)]
    pub extra: BTreeMap<String, serde_json::Value>,
}

/// Hook event names Kelta registers (PLUGINS §8).
pub mod names {
    pub const SESSION_START: &str = "SessionStart";
    pub const USER_PROMPT_SUBMIT: &str = "UserPromptSubmit";
    pub const PERMISSION_REQUEST: &str = "PermissionRequest";
    pub const NOTIFICATION: &str = "Notification";
    pub const POST_TOOL_USE: &str = "PostToolUse";
    pub const STOP: &str = "Stop";
    pub const STOP_FAILURE: &str = "StopFailure";
    pub const SESSION_END: &str = "SessionEnd";

    pub const ALL: &[&str] = &[
        SESSION_START,
        USER_PROMPT_SUBMIT,
        PERMISSION_REQUEST,
        NOTIFICATION,
        POST_TOOL_USE,
        STOP,
        STOP_FAILURE,
        SESSION_END,
    ];

    /// Notification matcher Kelta installs.
    pub const NOTIFICATION_MATCHER: &str =
        "permission_prompt|idle_prompt|elicitation_dialog|agent_needs_input";
    /// PostToolUse matcher Kelta installs.
    pub const EDIT_TOOLS_MATCHER: &str = "Edit|Write|MultiEdit";
}

impl HookPayload {
    /// `tool_input.file_path` for Edit/Write/MultiEdit.
    pub fn edited_file(&self) -> Option<PathBuf> {
        self.tool_input.as_ref().and_then(|v| v.get("file_path")).and_then(|v| v.as_str()).map(PathBuf::from)
    }
}

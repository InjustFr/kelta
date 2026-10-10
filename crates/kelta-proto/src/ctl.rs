//! Control socket protocol (ARCHITECTURE §2, §7.3, SPEC §8): one JSON object per line.
//!
//! Request: `{"v":1,"cmd":"hook","session":"…","token":"…","payload":{…}}`
//! Response: `{"ok":true,"result":…}` | `{"ok":false,"error":{KeltaError}}`.
//! `kelta-ctl` re-implements this wire format with serde_json only (it does not link kelta-proto).

use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::error::KeltaError;
use crate::hooks::HookPayload;
use crate::ids::{ProjectId, SessionId};

/// Protocol version.
pub const CTL_PROTOCOL_VERSION: u32 = 1;
/// Max request line size (hook payloads are capped at 1 MiB by kelta-ctl).
pub const CTL_MAX_LINE: usize = 1024 * 1024 + 4096;
/// Socket file name under the runtime dir.
pub const CTL_SOCKET_NAME: &str = "ctl.sock";

/// Commands accepted on the ctl socket. Serde tag `cmd`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(tag = "cmd", rename_all = "snake_case")]
pub enum CtlCommand {
    /// Claude hook relay (token-checked).
    Hook {
        session: SessionId,
        token: String,
        payload: Box<HookPayload>,
    },
    Toggle,
    Palette,
    Open {
        path: PathBuf,
    },
    FocusProject {
        id: ProjectId,
    },
    /// Ticket key or URL.
    Start {
        ticket: String,
        #[serde(default)]
        project: Option<ProjectId>,
    },
    /// Scratch work item from a task (`kelta-ctl start --task`), same saga as New work item.
    StartTask {
        task: String,
        #[serde(default)]
        project: Option<ProjectId>,
    },
    New {
        template: String,
        #[serde(default)]
        cwd: Option<PathBuf>,
        #[serde(default)]
        project: Option<ProjectId>,
    },
    /// Only `custom.*` names are accepted.
    Emit {
        name: String,
        payload: serde_json::Value,
    },
    Trust {
        repo: PathBuf,
    },
    EditorOpen {
        file: PathBuf,
        #[serde(default)]
        line: Option<u32>,
    },
    PluginInstall {
        source: String,
    },
    Version,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
pub struct CtlRequest {
    pub v: u32,
    #[serde(flatten)]
    pub command: CtlCommand,
}

impl CtlRequest {
    pub fn new(command: CtlCommand) -> Self {
        Self { v: CTL_PROTOCOL_VERSION, command }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
pub struct CtlResponse {
    pub ok: bool,
    #[serde(default)]
    pub result: Option<serde_json::Value>,
    #[serde(default)]
    pub error: Option<KeltaError>,
}

impl CtlResponse {
    pub fn ok(result: serde_json::Value) -> Self {
        Self { ok: true, result: Some(result), error: None }
    }

    pub fn err(error: KeltaError) -> Self {
        Self { ok: false, result: None, error: Some(error) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hook_line_shape() {
        let line = r#"{"v":1,"cmd":"hook","session":"s1","token":"t","payload":{"hook_event_name":"Stop","last_assistant_message":"done","extra_field":1}}"#;
        let req: CtlRequest = serde_json::from_str(line).unwrap();
        match &req.command {
            CtlCommand::Hook { session, payload, .. } => {
                assert_eq!(session.as_str(), "s1");
                assert_eq!(payload.hook_event_name, "Stop");
                assert!(payload.extra.contains_key("extra_field"));
            }
            other => panic!("unexpected {other:?}"),
        }
        let toggle: CtlRequest = serde_json::from_str(r#"{"v":1,"cmd":"toggle"}"#).unwrap();
        assert_eq!(toggle.command, CtlCommand::Toggle);
    }
}

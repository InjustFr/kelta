//! The Claude permission mode sessions start in when their profile forces none (#277): the last mode
//! a session switched to, seen in its hooks' `permission_mode` and kept in `ui_state`.

use kelta_proto::error::KeltaError;
use kelta_proto::hooks::HookPayload;
use kelta_proto::ids::SessionId;
use kelta_proto::settings::PermissionMode;

use crate::Core;
use crate::store::q;

const KEY: &str = "claude.permission_mode";

/// `dontAsk` / `bypassPermissions` are never remembered (#304): one session's danger is not every later one's.
fn remembered(m: &PermissionMode) -> bool {
    !matches!(m, PermissionMode::DontAsk | PermissionMode::BypassPermissions)
}

fn parse(s: &str) -> Option<PermissionMode> {
    serde_json::from_value(serde_json::Value::String(s.to_owned())).ok()
}

/// The `--permission-mode` of a Claude argv.
pub(crate) fn launched_with(args: &[String]) -> Option<PermissionMode> {
    args.windows(2).find(|w| w[0] == "--permission-mode").and_then(|w| parse(&w[1]))
}

impl Core {
    pub(crate) fn load_permission_mode(&self) -> Result<(), KeltaError> {
        let v = self.store.call_blocking(|c| q::ui_state_get(c, KEY))?;
        *self.claude_mode.lock() = v.as_deref().and_then(parse).filter(remembered);
        Ok(())
    }

    /// Remembers the hook's mode when it differs from the session's previous one (its launch flag at
    /// first): a forced plan mode or an unchanged session repeating itself is not the user's choice.
    /// shortcut: a mode Claude fell back to at launch (auto unavailable) is remembered
    /// like a switch; tell it apart from a key press if that fallback overwrites real choices.
    pub(crate) fn remember_permission_mode(&self, sid: &SessionId, p: &HookPayload) {
        let Some(mode) = p.extra.get("permission_mode").and_then(|v| v.as_str()).and_then(parse) else {
            return;
        };
        {
            let mut s = self.sessions.lock();
            let Some(e) = s.get_mut(sid) else { return };
            if e.permission_mode == Some(mode) {
                return;
            }
            e.permission_mode = Some(mode);
        }
        if !remembered(&mode) {
            return;
        }
        let mut cur = self.claude_mode.lock();
        if *cur != Some(mode) {
            *cur = Some(mode);
            let v = kelta_work::claude::enum_str(&mode);
            self.store.exec("permission_mode", move |c| q::ui_state_set(c, KEY, &v));
        }
    }
}

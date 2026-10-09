//! # kelta-server (L7)
//!
//! Local surfaces (ARCHITECTURE §2, §7.6, §11.1, PLUGINS §8): ctl socket server (line JSON,
//! peer-uid + per-session hook token checks), hook ingestion → `hooks::map`, lazy loopback axum
//! server (MCP at `/mcp/<sid>`, http hooks at `/hook/<sid>`, web-tool proxy at `/proxy/`).
//!
//! SCAFFOLD STUB: every fallible method returns `Unsupported("not implemented: <fn>")`.

use std::path::PathBuf;
use std::sync::{Arc, Weak};

use kelta_proto::api::CoreApi;
use kelta_proto::dirs::Dirs;
use kelta_proto::error::KeltaError;
use kelta_proto::ids::SessionId;

pub struct Server {
    core: Weak<dyn CoreApi>,
    dirs: Dirs,
}

impl Server {
    pub fn new(core: Weak<dyn CoreApi>, dirs: Dirs) -> Arc<Self> {
        Arc::new(Self { core, dirs })
    }

    pub fn core(&self) -> Option<Arc<dyn CoreApi>> {
        self.core.upgrade()
    }

    pub fn dirs(&self) -> &Dirs {
        &self.dirs
    }

    /// Bind `<runtime>/ctl.sock` (0600 in a 0700 dir) and serve; returns the socket path.
    pub async fn start_ctl(&self) -> Result<PathBuf, KeltaError> {
        Err(KeltaError::not_implemented("Server::start_ctl"))
    }

    /// Start the loopback HTTP server if needed and take a consumer reference; returns the port.
    pub async fn ensure_http(&self) -> Result<u16, KeltaError> {
        Err(KeltaError::not_implemented("Server::ensure_http"))
    }

    /// Release a consumer reference; the server stops at 0 (no idle timer).
    pub fn release_http(&self) {}

    /// Per-session tokens for hooks (ctl + `/hook/<sid>`) and MCP (`/mcp/<sid>`).
    pub fn register_session(&self, _sid: &SessionId, _hook_token: &str, _mcp_token: Option<&str>) {}

    pub fn unregister_session(&self, _sid: &SessionId) {}
}

/// Claude status machine (ARCHITECTURE §7.6), a pure function.
pub mod hooks {
    use kelta_proto::hooks::HookPayload;
    use kelta_proto::model::StatusChange;

    /// Map a hook payload to a status change; `None` for ignored events. Stub: `None`.
    pub fn map(_payload: &HookPayload) -> Option<StatusChange> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kelta_proto::testing::FakeCore;

    #[tokio::test]
    async fn stub_is_unsupported() {
        let core = FakeCore::new();
        let weak: Weak<dyn CoreApi> = Arc::downgrade(&(core.clone() as Arc<dyn CoreApi>));
        let s = Server::new(weak, Dirs::under(&std::env::temp_dir()));
        assert_eq!(s.ensure_http().await.unwrap_err().code, kelta_proto::ErrorCode::Unsupported);
        assert!(hooks::map(&kelta_proto::hooks::HookPayload::default()).is_none());
        let _ = kelta_plugins::proxy::router();
    }
}

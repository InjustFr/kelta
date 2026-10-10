//! # kelta-server (L7)
//!
//! Local surfaces (ARCHITECTURE §2, §7.6, §11.1, PLUGINS §8): ctl socket server (line JSON,
//! peer-uid + per-session hook token checks), hook ingestion → `hooks::map`, lazy loopback axum
//! server (MCP at `/mcp/<sid>`, http hooks at `/hook/<sid>`), Claude IDE bridges (`ide`).

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Weak};

use kelta_proto::api::CoreApi;
use kelta_proto::dirs::Dirs;
use kelta_proto::error::KeltaError;
use kelta_proto::ids::SessionId;
use parking_lot::Mutex;
use tokio::task::AbortHandle;

mod auth;
mod ctl;
pub mod hooks;
mod http;
mod ide;
mod mcp;

struct CtlRunning {
    path: PathBuf,
    task: AbortHandle,
}

pub struct Server {
    core: Weak<dyn CoreApi>,
    dirs: Dirs,
    tokens: Arc<auth::TokenTable>,
    ctl: Mutex<Option<CtlRunning>>,
    http: Mutex<http::HttpState>,
    ide: Mutex<HashMap<SessionId, ide::Bridge>>,
}

impl Server {
    pub fn new(core: Weak<dyn CoreApi>, dirs: Dirs) -> Arc<Self> {
        Arc::new(Self {
            core,
            dirs,
            tokens: Arc::new(auth::TokenTable::default()),
            ctl: Mutex::new(None),
            http: Mutex::new(http::HttpState::default()),
            ide: Mutex::new(HashMap::new()),
        })
    }

    pub fn core(&self) -> Option<Arc<dyn CoreApi>> {
        self.core.upgrade()
    }

    pub fn dirs(&self) -> &Dirs {
        &self.dirs
    }

    /// Bind `<runtime>/ctl.sock` (0600 in a 0700 dir) and serve; returns the socket path.
    /// Idempotent: a second call returns the path of the running socket.
    pub async fn start_ctl(&self) -> Result<PathBuf, KeltaError> {
        let mut guard = self.ctl.lock();
        if let Some(r) = guard.as_ref() {
            return Ok(r.path.clone());
        }
        let path = self.dirs.ctl_socket();
        let listener = ctl::bind(&path)?;
        let cx = Arc::new(ctl::CtlCtx { core: self.core.clone(), tokens: self.tokens.clone() });
        let task = tokio::spawn(ctl::accept_loop(listener, cx)).abort_handle();
        tracing::info!(path = %path.display(), "ctl socket listening");
        *guard = Some(CtlRunning { path: path.clone(), task });
        Ok(path)
    }

    /// Start the loopback HTTP server if needed and take a consumer reference; returns the port.
    pub async fn ensure_http(&self) -> Result<u16, KeltaError> {
        http::ensure(&self.http, self.core.clone(), self.tokens.clone())
    }

    /// Release a consumer reference; the server stops at 0 (no idle timer).
    pub fn release_http(&self) {
        http::release(&self.http);
    }

    /// Port of the running HTTP server; `None` while no consumer holds it.
    pub fn http_port(&self) -> Option<u16> {
        http::port(&self.http)
    }

    /// Per-session tokens for hooks (ctl + `/hook/<sid>`) and MCP (`/mcp/<sid>`).
    pub fn register_session(&self, sid: &SessionId, hook_token: &str, mcp_token: Option<&str>) {
        self.tokens.register(sid, hook_token, mcp_token);
    }

    pub fn unregister_session(&self, sid: &SessionId) {
        self.tokens.unregister(sid);
        self.ide_close(sid);
    }

    /// Start the Claude IDE bridge of `sid`: a loopback WebSocket plus `<claude_dir>/ide/<port>.lock`
    /// (stale Kelta locks there are removed first); returns the port. Replaces a previous bridge.
    pub fn ide_open(
        &self,
        sid: &SessionId,
        claude_dir: &Path,
        folders: Vec<PathBuf>,
    ) -> Result<u16, KeltaError> {
        let (bridge, port) =
            ide::open(self.core.clone(), sid.clone(), claude_dir, folders, &self.dirs.runtime)?;
        self.ide.lock().insert(sid.clone(), bridge);
        Ok(port)
    }

    /// Stop the bridge of `sid` and remove its lock file.
    pub fn ide_close(&self, sid: &SessionId) {
        self.ide.lock().remove(sid);
    }

    /// App quit: the process exits without running destructors.
    pub fn ide_close_all(&self) {
        self.ide.lock().clear();
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        if let Some(r) = self.ctl.get_mut().take() {
            r.task.abort();
            let _ = std::fs::remove_file(&r.path);
        }
        http::stop(&self.http);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kelta_proto::testing::FakeCore;

    #[tokio::test]
    async fn refcount_starts_and_stops() {
        let core = FakeCore::new();
        let dyn_core: Arc<dyn CoreApi> = core.clone();
        let tmp = tempfile::tempdir().unwrap();
        let s = Server::new(Arc::downgrade(&dyn_core), Dirs::under(tmp.path()));
        assert_eq!(s.http_port(), None);
        let p1 = s.ensure_http().await.unwrap();
        let p2 = s.ensure_http().await.unwrap();
        assert_eq!(p1, p2);
        s.release_http();
        assert_eq!(s.http_port(), Some(p1));
        s.release_http();
        assert_eq!(s.http_port(), None);
        s.release_http();
        assert_eq!(s.http_port(), None);
    }
}

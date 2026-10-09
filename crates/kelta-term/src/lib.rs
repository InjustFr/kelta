//! # kelta-term (L1)
//!
//! `TerminalHost` implementation: PTY backend (portable-pty | rustix), login-env exec, reader
//! threads, headless alacritty model, query responder, snapshot encoder, flow control and the
//! scrollback memory budget (ARCHITECTURE §7.1-§7.4, §9.3, §9.5).
//!
//! SCAFFOLD STUB: every fallible method returns `Unsupported("not implemented: <fn>")`.

use std::sync::Arc;
use std::time::Duration;

use kelta_proto::api::{FrameSink, TerminalHost};
use kelta_proto::error::KeltaError;
use kelta_proto::ids::SessionId;
use kelta_proto::model::AttachInfo;
use kelta_proto::term::{KillSignal, LoginEnv, PtySpawnSpec, TerminalLimits, TerminalPalette, TerminalStats};
use parking_lot::Mutex;

/// The PTY-backed terminal host.
pub struct PtyTerminalHost {
    env: LoginEnv,
    limits: Mutex<TerminalLimits>,
    palette: Mutex<TerminalPalette>,
}

impl PtyTerminalHost {
    pub fn new(env: LoginEnv, limits: TerminalLimits) -> Self {
        Self { env, limits: Mutex::new(limits), palette: Mutex::new(TerminalPalette::default()) }
    }

    /// Convenience for composition.
    pub fn new_arc(env: LoginEnv, limits: TerminalLimits) -> Arc<Self> {
        Arc::new(Self::new(env, limits))
    }

    /// The login environment this host was created with.
    pub fn login_env(&self) -> &LoginEnv {
        &self.env
    }
}

impl TerminalHost for PtyTerminalHost {
    fn spawn(&self, _spec: PtySpawnSpec) -> Result<(), KeltaError> {
        Err(KeltaError::not_implemented("PtyTerminalHost::spawn"))
    }

    fn attach(
        &self,
        _id: &SessionId,
        _cols: u16,
        _rows: u16,
        _sink: Box<dyn FrameSink>,
    ) -> Result<AttachInfo, KeltaError> {
        Err(KeltaError::not_implemented("PtyTerminalHost::attach"))
    }

    fn detach(&self, _id: &SessionId, _generation: u32) {}

    fn write(&self, _id: &SessionId, _bytes: &[u8]) -> Result<(), KeltaError> {
        Err(KeltaError::not_implemented("PtyTerminalHost::write"))
    }

    fn resize(&self, _id: &SessionId, _cols: u16, _rows: u16) -> Result<(), KeltaError> {
        Err(KeltaError::not_implemented("PtyTerminalHost::resize"))
    }

    fn ack(&self, _id: &SessionId, _generation: u32, _bytes: u32) {}

    fn kill(&self, _id: &SessionId, _signal: KillSignal) -> Result<(), KeltaError> {
        Err(KeltaError::not_implemented("PtyTerminalHost::kill"))
    }

    fn set_palette(&self, palette: TerminalPalette) {
        *self.palette.lock() = palette;
    }

    fn set_limits(&self, limits: TerminalLimits) {
        *self.limits.lock() = limits;
    }

    fn text_tail(&self, _id: &SessionId, _max_lines: u32) -> Result<String, KeltaError> {
        Err(KeltaError::not_implemented("PtyTerminalHost::text_tail"))
    }

    fn stats(&self) -> TerminalStats {
        TerminalStats::default()
    }
}

/// Resolve the user's login environment (§7.1). Stub: the inherited process environment.
pub fn resolve_login_env(_timeout: Duration) -> LoginEnv {
    LoginEnv::inherited()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stub_returns_unsupported() {
        let h = PtyTerminalHost::new(resolve_login_env(Duration::from_secs(1)), TerminalLimits::default());
        let e = h.write(&SessionId::new("x"), b"a").unwrap_err();
        assert_eq!(e.code, kelta_proto::ErrorCode::Unsupported);
    }
}

//! `KeltaError`: the single error type crossing crate boundaries and the IPC wire
//! (ARCHITECTURE §4, §12.1). `IpcError` on the wire == `KeltaError`.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// Error category. Serialized in `snake_case` (`"not_found"`, `"needs_auth"`, ...).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    NotFound,
    InvalidArgument,
    Conflict,
    PermissionDenied,
    NeedsAuth,
    RateLimited,
    Network,
    Upstream,
    Timeout,
    Unsupported,
    Untrusted,
    NeedsFields,
    Dirty,
    Cancelled,
    Internal,
}

impl ErrorCode {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::NotFound => "not_found",
            Self::InvalidArgument => "invalid_argument",
            Self::Conflict => "conflict",
            Self::PermissionDenied => "permission_denied",
            Self::NeedsAuth => "needs_auth",
            Self::RateLimited => "rate_limited",
            Self::Network => "network",
            Self::Upstream => "upstream",
            Self::Timeout => "timeout",
            Self::Unsupported => "unsupported",
            Self::Untrusted => "untrusted",
            Self::NeedsFields => "needs_fields",
            Self::Dirty => "dirty",
            Self::Cancelled => "cancelled",
            Self::Internal => "internal",
        }
    }
}

impl std::fmt::Display for ErrorCode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// The error returned by every fallible service method and Tauri command.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS, thiserror::Error)]
#[error("{code}: {message}")]
pub struct KeltaError {
    pub code: ErrorCode,
    pub message: String,
    /// Structured detail, e.g. `{"fields": [...]}` for `NeedsFields`, `{"candidates": [...]}` for an
    /// ambiguous `tracker_move`, `{"files": [...]}` for `Dirty`, `{"permission": "..."}` for `PermissionDenied`.
    #[serde(default)]
    pub detail: Option<serde_json::Value>,
    #[serde(default)]
    #[ts(type = "number | null")]
    pub retry_after_ms: Option<u64>,
}

/// Crate-wide result alias.
pub type Result<T, E = KeltaError> = std::result::Result<T, E>;

impl KeltaError {
    pub fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        Self { code, message: message.into(), detail: None, retry_after_ms: None }
    }

    /// The exact error every scaffold stub returns: `Unsupported`, `"not implemented: <fn>"`.
    pub fn not_implemented(func: &str) -> Self {
        Self::new(ErrorCode::Unsupported, format!("not implemented: {func}"))
    }

    pub fn not_found(message: impl Into<String>) -> Self {
        Self::new(ErrorCode::NotFound, message)
    }

    pub fn invalid(message: impl Into<String>) -> Self {
        Self::new(ErrorCode::InvalidArgument, message)
    }

    pub fn conflict(message: impl Into<String>) -> Self {
        Self::new(ErrorCode::Conflict, message)
    }

    pub fn permission_denied(permission: impl Into<String>) -> Self {
        let permission = permission.into();
        Self::new(ErrorCode::PermissionDenied, format!("missing permission: {permission}"))
            .with_detail(serde_json::json!({ "permission": permission }))
    }

    pub fn needs_auth(message: impl Into<String>) -> Self {
        Self::new(ErrorCode::NeedsAuth, message)
    }

    pub fn rate_limited(message: impl Into<String>, retry_after_ms: Option<u64>) -> Self {
        let mut e = Self::new(ErrorCode::RateLimited, message);
        e.retry_after_ms = retry_after_ms;
        e
    }

    pub fn network(message: impl Into<String>) -> Self {
        Self::new(ErrorCode::Network, message)
    }

    pub fn upstream(message: impl Into<String>) -> Self {
        Self::new(ErrorCode::Upstream, message)
    }

    pub fn timeout(message: impl Into<String>) -> Self {
        Self::new(ErrorCode::Timeout, message)
    }

    pub fn unsupported(message: impl Into<String>) -> Self {
        Self::new(ErrorCode::Unsupported, message)
    }

    pub fn untrusted(message: impl Into<String>) -> Self {
        Self::new(ErrorCode::Untrusted, message)
    }

    pub fn cancelled(message: impl Into<String>) -> Self {
        Self::new(ErrorCode::Cancelled, message)
    }

    pub fn internal(message: impl Into<String>) -> Self {
        Self::new(ErrorCode::Internal, message)
    }

    pub fn with_detail(mut self, detail: serde_json::Value) -> Self {
        self.detail = Some(detail);
        self
    }

    pub fn with_retry_after_ms(mut self, ms: u64) -> Self {
        self.retry_after_ms = Some(ms);
        self
    }

    pub fn is(&self, code: ErrorCode) -> bool {
        self.code == code
    }
}

impl From<serde_json::Error> for KeltaError {
    fn from(e: serde_json::Error) -> Self {
        Self::invalid(format!("json: {e}"))
    }
}

impl From<std::io::Error> for KeltaError {
    fn from(e: std::io::Error) -> Self {
        let code = match e.kind() {
            std::io::ErrorKind::NotFound => ErrorCode::NotFound,
            std::io::ErrorKind::PermissionDenied => ErrorCode::PermissionDenied,
            std::io::ErrorKind::TimedOut => ErrorCode::Timeout,
            std::io::ErrorKind::InvalidInput | std::io::ErrorKind::InvalidData => ErrorCode::InvalidArgument,
            _ => ErrorCode::Internal,
        };
        Self::new(code, e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stub_error_shape() {
        let e = KeltaError::not_implemented("session_spawn");
        assert_eq!(e.code, ErrorCode::Unsupported);
        assert_eq!(e.message, "not implemented: session_spawn");
        let v = serde_json::to_value(&e).unwrap();
        assert_eq!(v["code"], "unsupported");
    }
}

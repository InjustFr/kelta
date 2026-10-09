//! Per-session tokens and constant-time comparison (ARCHITECTURE §11.1). Tokens are never logged.

use std::collections::HashMap;

use kelta_proto::ids::SessionId;
use parking_lot::RwLock;

/// Constant-time equality for equal-length inputs (the length itself is not secret).
pub(crate) fn ct_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let diff = a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y));
    std::hint::black_box(diff) == 0
}

#[derive(Clone)]
struct Tokens {
    hook: String,
    mcp: Option<String>,
}

/// Which token a request must present.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum TokenKind {
    Hook,
    Mcp,
}

#[derive(Default)]
pub(crate) struct TokenTable {
    map: RwLock<HashMap<SessionId, Tokens>>,
}

impl TokenTable {
    pub(crate) fn register(&self, sid: &SessionId, hook: &str, mcp: Option<&str>) {
        self.map.write().insert(sid.clone(), Tokens { hook: hook.to_owned(), mcp: mcp.map(str::to_owned) });
    }

    pub(crate) fn unregister(&self, sid: &SessionId) {
        self.map.write().remove(sid);
    }

    /// True iff `sid` is registered and `presented` equals its token of `kind` (non-empty).
    pub(crate) fn check(&self, sid: &SessionId, kind: TokenKind, presented: &str) -> bool {
        let map = self.map.read();
        let Some(t) = map.get(sid) else {
            // Burn a comparison so unknown sessions are not distinguishable by timing.
            let _ = ct_eq(presented.as_bytes(), presented.as_bytes());
            return false;
        };
        let expected = match kind {
            TokenKind::Hook => Some(t.hook.as_str()),
            TokenKind::Mcp => t.mcp.as_deref(),
        };
        match expected {
            Some(e) if !e.is_empty() => ct_eq(e.as_bytes(), presented.as_bytes()),
            _ => false,
        }
    }
}

/// `Authorization: Bearer <token>` → token.
pub(crate) fn bearer(value: Option<&str>) -> Option<&str> {
    let v = value?.trim();
    let (scheme, token) = v.split_once(' ')?;
    scheme.eq_ignore_ascii_case("bearer").then(|| token.trim()).filter(|t| !t.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn token_checks() {
        let t = TokenTable::default();
        let sid = SessionId::new("s1");
        t.register(&sid, "hook-tok", None);
        assert!(t.check(&sid, TokenKind::Hook, "hook-tok"));
        assert!(!t.check(&sid, TokenKind::Hook, "hook-toK"));
        assert!(!t.check(&sid, TokenKind::Hook, ""));
        assert!(!t.check(&sid, TokenKind::Mcp, "hook-tok"));
        assert!(!t.check(&SessionId::new("s2"), TokenKind::Hook, "hook-tok"));
        t.register(&sid, "", Some("m"));
        assert!(!t.check(&sid, TokenKind::Hook, ""));
        assert!(t.check(&sid, TokenKind::Mcp, "m"));
        t.unregister(&sid);
        assert!(!t.check(&sid, TokenKind::Mcp, "m"));
    }

    #[test]
    fn bearer_parsing() {
        assert_eq!(bearer(Some("Bearer abc")), Some("abc"));
        assert_eq!(bearer(Some("bearer  abc ")), Some("abc"));
        assert_eq!(bearer(Some("Basic abc")), None);
        assert_eq!(bearer(Some("Bearer ")), None);
        assert_eq!(bearer(None), None);
    }
}

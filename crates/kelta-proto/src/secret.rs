//! Secret references and in-memory secrets (SETTINGS §5).

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::ids::AccountId;

/// A reference to a secret, never the secret itself:
/// `keyring:<name>` | `gh-cli` | `glab-cli` | `command:<argv>` | `env:<VAR>`.
#[derive(
    Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Default, Serialize, Deserialize, TS, JsonSchema,
)]
#[serde(transparent)]
pub struct SecretRef(pub String);

/// Parsed form of a [`SecretRef`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SecretSource {
    Keyring(String),
    GhCli,
    GlabCli,
    /// argv string, split shell-words by the resolver.
    Command(String),
    Env(String),
}

impl SecretRef {
    pub fn new(s: impl Into<String>) -> Self {
        Self(s.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn parse(&self) -> Option<SecretSource> {
        let s = self.0.trim();
        match s {
            "gh-cli" => return Some(SecretSource::GhCli),
            "glab-cli" => return Some(SecretSource::GlabCli),
            _ => {}
        }
        let (kind, rest) = s.split_once(':')?;
        if rest.is_empty() {
            return None;
        }
        match kind {
            "keyring" => Some(SecretSource::Keyring(rest.to_owned())),
            "command" => Some(SecretSource::Command(rest.to_owned())),
            "env" => Some(SecretSource::Env(rest.to_owned())),
            _ => None,
        }
    }

    pub fn is_keyring(&self) -> bool {
        matches!(self.parse(), Some(SecretSource::Keyring(_)))
    }
}

impl std::fmt::Display for SecretRef {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// Keyring service name for `keyring:` refs.
pub const KEYRING_SERVICE: &str = "dev.kelta";

/// A resolved secret value. Not `Serialize`, redacted in `Debug`, zeroed on drop.
pub struct Secret(String);

impl Secret {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    /// Borrow the secret value. Never log it.
    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl Clone for Secret {
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}

impl std::fmt::Debug for Secret {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Secret(***)")
    }
}

impl Drop for Secret {
    fn drop(&mut self) {
        let mut bytes = std::mem::take(&mut self.0).into_bytes();
        for b in bytes.iter_mut() {
            *b = 0;
        }
        std::hint::black_box(&bytes);
    }
}

/// Context for resolution (host for `gh-cli`/`glab-cli`, account for diagnostics).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SecretCtx {
    pub account: Option<AccountId>,
    /// Host of the account `base_url` (e.g. `github.com`, `gitlab.acme.example`).
    pub host: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct SecretBackendStatus {
    /// `keychain` | `secret-service` | `gh-cli` | `glab-cli` | `command` | `env`.
    pub backend: String,
    pub available: bool,
    pub detail: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_refs() {
        assert_eq!(
            SecretRef::new("keyring:jira-acme").parse(),
            Some(SecretSource::Keyring("jira-acme".into()))
        );
        assert_eq!(SecretRef::new("gh-cli").parse(), Some(SecretSource::GhCli));
        assert_eq!(
            SecretRef::new("command:pass show x").parse(),
            Some(SecretSource::Command("pass show x".into()))
        );
        assert_eq!(SecretRef::new("env:").parse(), None);
        assert_eq!(format!("{:?}", Secret::new("hunter2")), "Secret(***)");
    }
}

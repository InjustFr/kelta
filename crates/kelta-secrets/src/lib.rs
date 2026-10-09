//! # kelta-secrets (L4)
//!
//! `SecretRef` resolution chain (SETTINGS §5): `keyring:` (macOS Keychain / Linux Secret Service via
//! keyring-core), `gh-cli`, `glab-cli`, `command:`, `env:`; 5 s timeouts off-thread, in-memory cache,
//! backend status.
//!
//! SCAFFOLD STUB: every fallible method returns `Unsupported("not implemented: <fn>")`.

use std::sync::Arc;

use async_trait::async_trait;
use kelta_proto::api::{SecretResolver, SettingsSource};
use kelta_proto::error::KeltaError;
use kelta_proto::secret::{Secret, SecretBackendStatus, SecretCtx, SecretRef};

pub struct Secrets {
    settings: Arc<dyn SettingsSource>,
}

impl Secrets {
    pub fn new(settings: Arc<dyn SettingsSource>) -> Arc<Self> {
        Arc::new(Self { settings })
    }

    pub fn settings(&self) -> &Arc<dyn SettingsSource> {
        &self.settings
    }
}

#[async_trait]
impl SecretResolver for Secrets {
    async fn resolve(&self, _r: &SecretRef, _ctx: &SecretCtx) -> Result<Secret, KeltaError> {
        Err(KeltaError::not_implemented("Secrets::resolve"))
    }

    async fn set(&self, _r: &SecretRef, _value: &str) -> Result<(), KeltaError> {
        Err(KeltaError::not_implemented("Secrets::set"))
    }

    async fn delete(&self, _r: &SecretRef) -> Result<(), KeltaError> {
        Err(KeltaError::not_implemented("Secrets::delete"))
    }

    async fn backends_status(&self) -> Vec<SecretBackendStatus> {
        Vec::new()
    }

    fn invalidate(&self, _r: &SecretRef) {}
}

#[cfg(test)]
mod tests {
    use super::*;
    use kelta_proto::testing::FakeSettings;

    #[tokio::test]
    async fn stub_is_unsupported() {
        let s = Secrets::new(FakeSettings::defaults());
        let e = s.resolve(&SecretRef::new("env:X"), &SecretCtx::default()).await.unwrap_err();
        assert_eq!(e.code, kelta_proto::ErrorCode::Unsupported);
    }
}

//! # kelta-codehosts (L5)
//!
//! Code-host providers (ARCHITECTURE §8.2): GitHub (aliased GraphQL search, notifications gate,
//! REST actions, GHE) and GitLab (REST, version-gated draft/wip, approvals, todos gate).
//!
//! SCAFFOLD STUB: the factory returns `Unsupported("not implemented: <fn>")`.

use std::sync::Arc;

use kelta_http::{HttpCtx, ProviderFactory};
use kelta_proto::api::{CodeHost, SecretResolver, Tracker};
use kelta_proto::error::KeltaError;
use kelta_proto::settings::AccountConfig;

/// Builds `CodeHost`s for `github` and `gitlab` accounts.
#[derive(Debug, Default, Clone, Copy)]
pub struct CodeHostFactory;

impl ProviderFactory for CodeHostFactory {
    fn tracker(
        &self,
        _account: &AccountConfig,
        _http: HttpCtx,
        _secrets: Arc<dyn SecretResolver>,
    ) -> Result<Arc<dyn Tracker>, KeltaError> {
        Err(KeltaError::unsupported("CodeHostFactory does not build trackers"))
    }

    fn code_host(
        &self,
        _account: &AccountConfig,
        _http: HttpCtx,
        _secrets: Arc<dyn SecretResolver>,
    ) -> Result<Arc<dyn CodeHost>, KeltaError> {
        Err(KeltaError::not_implemented("CodeHostFactory::code_host"))
    }
}

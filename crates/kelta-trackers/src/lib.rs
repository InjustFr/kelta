//! # kelta-trackers (L5)
//!
//! Tracker providers (ARCHITECTURE §8.1): Jira Cloud + Data Center (flavor detection, ADF walker),
//! Redmine, GitHub Issues (+ Projects v2), GitLab Issues.
//!
//! SCAFFOLD STUB: the factory returns `Unsupported("not implemented: <fn>")`.

use std::sync::Arc;

use kelta_http::{HttpCtx, ProviderFactory};
use kelta_proto::api::{CodeHost, SecretResolver, Tracker};
use kelta_proto::error::KeltaError;
use kelta_proto::settings::AccountConfig;

/// Builds `Tracker`s for `jira`, `redmine`, `github`, `gitlab` accounts.
#[derive(Debug, Default, Clone, Copy)]
pub struct TrackerFactory;

impl ProviderFactory for TrackerFactory {
    fn tracker(
        &self,
        _account: &AccountConfig,
        _http: HttpCtx,
        _secrets: Arc<dyn SecretResolver>,
    ) -> Result<Arc<dyn Tracker>, KeltaError> {
        Err(KeltaError::not_implemented("TrackerFactory::tracker"))
    }

    fn code_host(
        &self,
        _account: &AccountConfig,
        _http: HttpCtx,
        _secrets: Arc<dyn SecretResolver>,
    ) -> Result<Arc<dyn CodeHost>, KeltaError> {
        Err(KeltaError::unsupported("TrackerFactory does not build code hosts"))
    }
}

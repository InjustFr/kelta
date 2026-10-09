//! `ProviderFactory` (ARCHITECTURE §4). Lives here rather than in `kelta-proto::api` because its
//! signature takes [`HttpCtx`]; implemented by `kelta_trackers::TrackerFactory` and
//! `kelta_codehosts::CodeHostFactory` (core picks the factory by account kind).

use std::sync::Arc;

use kelta_proto::api::{CodeHost, SecretResolver, Tracker};
use kelta_proto::error::KeltaError;
use kelta_proto::settings::AccountConfig;

use crate::HttpCtx;

pub trait ProviderFactory: Send + Sync {
    fn tracker(
        &self,
        account: &AccountConfig,
        http: HttpCtx,
        secrets: Arc<dyn SecretResolver>,
    ) -> Result<Arc<dyn Tracker>, KeltaError>;

    fn code_host(
        &self,
        account: &AccountConfig,
        http: HttpCtx,
        secrets: Arc<dyn SecretResolver>,
    ) -> Result<Arc<dyn CodeHost>, KeltaError>;
}

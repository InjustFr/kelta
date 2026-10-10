//! # kelta-codehosts (L5)
//!
//! Code-host providers (ARCHITECTURE §8.2): GitHub (aliased GraphQL search, notifications gate,
//! REST actions, GHE), GitLab (REST, version-gated draft/wip, approvals, todos gate), Bitbucket Cloud
//! (REST 2.0, workspace-scoped) and Gitea/Forgejo (REST v1).
//!
//! `reviews.ticket_key_regex` reaches the providers through [`set_ticket_key_regex`] (the
//! `ProviderFactory` signature carries no settings); the default is the SETTINGS.md default.

use std::sync::Arc;

use kelta_http::{HttpCtx, ProviderFactory};
use kelta_proto::api::{CodeHost, SecretResolver, Tracker};
use kelta_proto::error::KeltaError;
use kelta_proto::settings::{AccountConfig, AccountKind};

pub mod bitbucket;
mod common;
pub mod gitea;
pub mod github;
pub mod gitlab;

pub use bitbucket::BitbucketHost;
pub use common::{linked_tickets, parse_remote, set_ticket_key_regex};
pub use gitea::GiteaHost;
pub use github::GithubHost;
pub use gitlab::GitlabHost;

/// Builds `CodeHost`s for `github`, `gitlab`, `bitbucket` and `gitea` accounts.
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
        account: &AccountConfig,
        http: HttpCtx,
        secrets: Arc<dyn SecretResolver>,
    ) -> Result<Arc<dyn CodeHost>, KeltaError> {
        match account.kind {
            AccountKind::Github => Ok(Arc::new(GithubHost::new(account, http, secrets)?)),
            AccountKind::Gitlab => Ok(Arc::new(GitlabHost::new(account, http, secrets)?)),
            AccountKind::Bitbucket => Ok(Arc::new(BitbucketHost::new(account, http, secrets)?)),
            AccountKind::Gitea => Ok(Arc::new(GiteaHost::new(account, http, secrets)?)),
            AccountKind::Jira
            | AccountKind::Redmine
            | AccountKind::Linear
            | AccountKind::PluginTracker
            | AccountKind::PluginCodehost => {
                Err(KeltaError::unsupported(format!("{:?} accounts have no code host", account.kind)))
            }
        }
    }
}

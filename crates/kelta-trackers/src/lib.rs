//! # kelta-trackers (L5)
//!
//! Tracker providers (ARCHITECTURE §8.1): Jira Cloud + Data Center (flavor detection, ADF walker),
//! Redmine, GitHub Issues (+ Projects v2), GitLab Issues, Gitea/Forgejo Issues, Linear.
//!
//! Every provider resolves its secret per request through [`kelta_http::Authed`] (the resolver
//! caches), talks only through the per-account [`kelta_http::HttpCtx`], and never hard-codes
//! workflow ids: moves are resolved at runtime from `transitions()`.

use std::sync::Arc;

use kelta_http::{HttpCtx, ProviderFactory};
use kelta_proto::api::{CodeHost, SecretResolver, Tracker};
use kelta_proto::error::KeltaError;
use kelta_proto::settings::{AccountConfig, AccountKind};

pub mod adf;
mod common;
pub mod gitea;
pub mod github;
pub mod gitlab;
pub mod jira;
pub mod linear;
pub mod redmine;

pub use gitea::GiteaIssues;
pub use github::GithubIssues;
pub use gitlab::GitlabIssues;
pub use jira::JiraTracker;
pub use linear::LinearTracker;
pub use redmine::RedmineTracker;

/// Builds `Tracker`s for `jira`, `redmine`, `github`, `gitlab`, `gitea`, `linear` accounts.
#[derive(Debug, Default, Clone, Copy)]
pub struct TrackerFactory;

impl ProviderFactory for TrackerFactory {
    fn tracker(
        &self,
        account: &AccountConfig,
        http: HttpCtx,
        secrets: Arc<dyn SecretResolver>,
    ) -> Result<Arc<dyn Tracker>, KeltaError> {
        Ok(match account.kind {
            AccountKind::Jira => Arc::new(JiraTracker::new(account, http, secrets)?),
            AccountKind::Redmine => Arc::new(RedmineTracker::new(account, http, secrets)?),
            AccountKind::Github => Arc::new(GithubIssues::new(account, http, secrets)?),
            AccountKind::Gitlab => Arc::new(GitlabIssues::new(account, http, secrets)?),
            AccountKind::Linear => Arc::new(LinearTracker::new(account, http, secrets)?),
            AccountKind::Gitea => Arc::new(GiteaIssues::new(account, http, secrets)?),
            AccountKind::Bitbucket => {
                return Err(KeltaError::unsupported("Bitbucket Cloud removed its issue tracker; use Jira"));
            }
        })
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

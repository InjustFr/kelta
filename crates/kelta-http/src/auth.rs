//! Provider authentication: resolve the account secret per request, apply the right header,
//! invalidate the cached secret on 401 (SETTINGS §5: "invalidated on settings change or 401").

use std::sync::Arc;

use kelta_proto::api::SecretResolver;
use kelta_proto::error::{ErrorCode, KeltaError};
use kelta_proto::secret::{Secret, SecretCtx, SecretRef};
use kelta_proto::settings::{AccountConfig, AuthKind};
use serde::de::DeserializeOwned;

use crate::{HttpCtx, HttpRequest, HttpResponse};

/// How the secret is presented.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AuthScheme {
    /// `Authorization: Bearer <secret>`.
    Bearer,
    /// `Authorization: Basic base64(<user>:<secret>)` (Jira Cloud: email + API token).
    Basic { user: String },
    /// `<name>: <secret>` (`X-Redmine-API-Key`, `PRIVATE-TOKEN`).
    Header(String),
    /// Bearer token from the device flow, refreshed before it expires ([`crate::oauth`]).
    OAuth,
}

impl AuthScheme {
    /// The scheme an account asks for explicitly through `auth`, if any.
    pub fn from_account(account: &AccountConfig) -> Option<Self> {
        let user = || account.email.clone().or_else(|| account.user.clone()).unwrap_or_default();
        account.auth.map(|a| match a {
            AuthKind::Basic => Self::Basic { user: user() },
            AuthKind::Bearer => Self::Bearer,
            AuthKind::ApiKey => Self::Header("X-Redmine-API-Key".into()),
            AuthKind::Token => Self::Bearer,
            AuthKind::Oauth => Self::OAuth,
        })
    }
}

/// `HttpCtx` + account credentials. Cheap to clone.
#[derive(Clone)]
pub struct Authed {
    http: HttpCtx,
    secrets: Arc<dyn SecretResolver>,
    secret: Option<SecretRef>,
    secret_ctx: SecretCtx,
    scheme: AuthScheme,
    headers: Vec<(String, String)>,
}

impl Authed {
    pub fn new(
        http: HttpCtx,
        secrets: Arc<dyn SecretResolver>,
        secret: Option<SecretRef>,
        base_url: Option<&str>,
        scheme: AuthScheme,
    ) -> Self {
        let secret_ctx = SecretCtx {
            account: Some(http.account_id().clone()),
            host: base_url.and_then(crate::util::url_host),
        };
        Self { http, secrets, secret, secret_ctx, scheme, headers: Vec::new() }
    }

    /// Header added to every request (`Accept`, API version, ...).
    pub fn with_header(mut self, k: impl Into<String>, v: impl Into<String>) -> Self {
        self.headers.push((k.into(), v.into()));
        self
    }

    pub fn with_scheme(mut self, scheme: AuthScheme) -> Self {
        self.scheme = scheme;
        self
    }

    pub fn scheme(&self) -> &AuthScheme {
        &self.scheme
    }

    pub fn http(&self) -> &HttpCtx {
        &self.http
    }

    /// The resolved secret (never log it).
    pub async fn token(&self) -> Result<Secret, KeltaError> {
        let Some(r) = &self.secret else {
            return Err(KeltaError::needs_auth(format!(
                "account {} has no secret configured",
                self.http.account_id()
            )));
        };
        if self.scheme == AuthScheme::OAuth
            && let Some(fresh) =
                crate::oauth::refreshed(self.http.client(), &*self.secrets, r, &self.secret_ctx).await?
        {
            return Ok(fresh);
        }
        self.secrets.resolve(r, &self.secret_ctx).await
    }

    /// Resolve the secret and add the auth + default headers to `req`.
    pub async fn prepare(&self, mut req: HttpRequest) -> Result<HttpRequest, KeltaError> {
        let token = self.token().await?;
        let token = token.expose();
        req = match &self.scheme {
            AuthScheme::Bearer | AuthScheme::OAuth => req.bearer(token),
            AuthScheme::Basic { user } => req.basic(user, token),
            AuthScheme::Header(name) => req.header(name.clone(), token),
        };
        for (k, v) in &self.headers {
            req = req.header(k.clone(), v.clone());
        }
        Ok(req)
    }

    pub async fn send_json<T: DeserializeOwned>(
        &self,
        req: HttpRequest,
    ) -> Result<HttpResponse<T>, KeltaError> {
        let req = self.prepare(req).await?;
        self.http.send_json(req).await.inspect_err(|e| self.on_error(e))
    }

    pub async fn send_text(&self, req: HttpRequest) -> Result<HttpResponse<String>, KeltaError> {
        let req = self.prepare(req).await?;
        self.http.send_text(req).await.inspect_err(|e| self.on_error(e))
    }

    fn on_error(&self, e: &KeltaError) {
        if e.code == ErrorCode::NeedsAuth
            && let Some(r) = &self.secret
        {
            self.secrets.invalidate(r);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{HttpClient, HttpPolicy};
    use kelta_proto::ids::AccountId;
    use kelta_proto::testing::FakeSecrets;

    fn authed(secrets: Arc<FakeSecrets>, scheme: AuthScheme) -> Authed {
        let ctx = HttpCtx::new(HttpClient::new("t"), AccountId::new("a"), HttpPolicy::default());
        Authed::new(
            ctx,
            secrets,
            Some(SecretRef::new("env:X")),
            Some("https://gitlab.acme.example/x"),
            scheme,
        )
    }

    #[tokio::test]
    async fn applies_schemes() {
        let s = FakeSecrets::with(&[("env:X", "tok")]);
        let r = authed(s.clone(), AuthScheme::Header("PRIVATE-TOKEN".into()))
            .prepare(HttpRequest::get("http://h/"))
            .await
            .unwrap();
        assert!(r.headers.contains(&("PRIVATE-TOKEN".to_owned(), "tok".to_owned())));
        let r = authed(s, AuthScheme::Basic { user: "me@x.io".into() })
            .with_header("Accept", "application/json")
            .prepare(HttpRequest::get("http://h/"))
            .await
            .unwrap();
        assert!(r.headers.iter().any(|(k, v)| k == "Authorization" && v.starts_with("Basic ")));
        assert!(r.headers.iter().any(|(k, _)| k == "Accept"));
    }

    #[tokio::test]
    async fn missing_secret_is_needs_auth() {
        let a = authed(FakeSecrets::new(), AuthScheme::Bearer);
        assert_eq!(a.prepare(HttpRequest::get("http://h/")).await.unwrap_err().code, ErrorCode::NeedsAuth);
    }

    #[test]
    fn scheme_from_account_auth() {
        let acc: AccountConfig = toml_free_account(AuthKind::Basic);
        assert_eq!(AuthScheme::from_account(&acc), Some(AuthScheme::Basic { user: "a@b.c".into() }));
    }

    fn toml_free_account(auth: AuthKind) -> AccountConfig {
        serde_json::from_value(serde_json::json!({
            "kind": "jira", "base_url": "https://x", "email": "a@b.c",
            "auth": serde_json::to_value(auth).unwrap(),
        }))
        .unwrap()
    }
}

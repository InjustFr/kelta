//! OAuth 2.0 device authorization grant (RFC 8628) for GitHub and GitLab, and the refresh of
//! expiring tokens (SETTINGS §5). Tokens go straight to the secret backend: never logged, never
//! returned. Only the user code and the verification URL leave this module.
//!
//! An OAuth account (`auth = "oauth"`) keeps its access token at its `secret` ref and, next to it
//! at `<secret>.oauth`, a JSON [`Grant`]: client id, token URL, refresh token and expiry.

use std::time::Duration;

use kelta_proto::api::SecretResolver;
use kelta_proto::error::{ErrorCode, KeltaError};
use kelta_proto::secret::{Secret, SecretCtx, SecretRef};
use kelta_proto::settings::AccountKind;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use tokio::time::Instant;

use crate::HttpClient;
use crate::ctx::map_reqwest_error;

const DEVICE_GRANT: &str = "urn:ietf:params:oauth:grant-type:device_code";
/// Refresh this long before the access token expires.
const REFRESH_MARGIN_SECS: i64 = 60;

/// Serializes refreshes: GitLab rotates refresh tokens, so two concurrent refreshes would revoke
/// each other.
static REFRESH: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

/// Web root of the account: GitHub's API host maps to `github.com`, GHE drops `/api/v3`,
/// GitLab drops `/api/v4`.
fn web_root(kind: AccountKind, base_url: &str) -> Result<String, KeltaError> {
    let base = base_url.trim().trim_end_matches('/');
    let root = match kind {
        AccountKind::Github if base == "https://api.github.com" => "https://github.com",
        AccountKind::Github => base.strip_suffix("/api/v3").unwrap_or(base),
        AccountKind::Gitlab => base.strip_suffix("/api/v4").unwrap_or(base),
        _ => return Err(KeltaError::invalid("browser sign-in is available for GitHub and GitLab only")),
    };
    Ok(root.to_owned())
}

/// The host `oauth.client_ids` is keyed by (`github.com`, `gitlab.com`, `gitlab.acme.example`).
pub fn client_host(kind: AccountKind, base_url: &str) -> Result<String, KeltaError> {
    crate::util::url_host(&web_root(kind, base_url)?)
        .ok_or_else(|| KeltaError::invalid(format!("invalid server URL `{base_url}`")))
}

/// A started device authorization. The device code never leaves the backend.
#[derive(Debug)]
pub struct DeviceAuth {
    pub user_code: String,
    pub verification_uri: String,
    pub expires_in: u64,
    device_code: Secret,
    interval: u64,
    grant: Grant,
}

/// Stored at `<secret>.oauth`.
#[derive(Serialize, Deserialize)]
struct Grant {
    client_id: String,
    token_url: String,
    refresh_token: Option<String>,
    /// Unix seconds; `None` = does not expire (GitHub OAuth Apps).
    expires_at: Option<i64>,
}

impl std::fmt::Debug for Grant {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Grant").field("token_url", &self.token_url).finish_non_exhaustive()
    }
}

impl Grant {
    fn expiring(&self) -> bool {
        self.expires_at.is_some_and(|t| t - REFRESH_MARGIN_SECS <= now_unix())
    }
}

/// Every field any of the three endpoints may answer (RFC 8628 §3.2, §3.5; RFC 6749 §5).
#[derive(Deserialize, Default)]
#[serde(default)]
struct Resp {
    error: Option<String>,
    error_description: Option<String>,
    device_code: Option<String>,
    user_code: Option<String>,
    verification_uri: Option<String>,
    expires_in: Option<u64>,
    interval: Option<u64>,
    access_token: Option<String>,
    refresh_token: Option<String>,
}

fn now_unix() -> i64 {
    time::OffsetDateTime::now_utc().unix_timestamp()
}

fn sign_in_again(why: &str) -> KeltaError {
    KeltaError::needs_auth(format!("{why}: sign in again"))
}

/// The derived ref holding the [`Grant`] of an OAuth secret.
pub fn grant_ref(secret: &SecretRef) -> SecretRef {
    SecretRef::new(format!("{}.oauth", secret.as_str()))
}

/// POST a form, read the JSON answer whatever the status (RFC errors come as 400 + JSON).
async fn post_form<T: DeserializeOwned>(
    http: &HttpClient,
    url: &str,
    form: &[(&str, &str)],
) -> Result<T, KeltaError> {
    let res = http
        .reqwest()
        .post(url)
        .header("Accept", "application/json")
        .form(form)
        .send()
        .await
        .map_err(map_reqwest_error)?;
    let status = res.status().as_u16();
    let body = res.bytes().await.map_err(map_reqwest_error)?;
    serde_json::from_slice(&body).map_err(|_| {
        KeltaError::upstream(format!(
            "unexpected answer from {} (HTTP {status})",
            kelta_proto::redact::redact_url(url)
        ))
    })
}

fn oauth_error(r: &Resp) -> KeltaError {
    let code = r.error.as_deref().unwrap_or("unknown_error");
    let hint = match code {
        "device_flow_disabled" => " (enable Device Flow in the GitHub OAuth App settings)",
        "invalid_client" | "incorrect_client_credentials" => {
            " (check oauth.client_ids; GitLab applications must not be confidential)"
        }
        _ => "",
    };
    let desc = r.error_description.as_deref().map(|d| format!(": {d}")).unwrap_or_default();
    KeltaError::new(ErrorCode::Upstream, format!("sign-in failed: {code}{desc}{hint}"))
}

/// Ask for a device code (RFC 8628 §3.1). `base_url` is the account's (API) base URL.
pub async fn device_start(
    http: &HttpClient,
    kind: AccountKind,
    base_url: &str,
    client_id: &str,
) -> Result<DeviceAuth, KeltaError> {
    let root = web_root(kind, base_url)?;
    let (device_url, token_url, scope) = match kind {
        // scopes = what the providers call: issues/PRs, org teams, Projects v2 status moves
        AccountKind::Github => (
            format!("{root}/login/device/code"),
            format!("{root}/login/oauth/access_token"),
            "repo read:org project",
        ),
        _ => (format!("{root}/oauth/authorize_device"), format!("{root}/oauth/token"), "api"),
    };
    let r: Resp = post_form(http, &device_url, &[("client_id", client_id), ("scope", scope)]).await?;
    if r.error.is_some() {
        return Err(oauth_error(&r));
    }
    let (Some(device_code), Some(user_code), Some(verification_uri)) =
        (r.device_code, r.user_code, r.verification_uri)
    else {
        return Err(KeltaError::upstream("sign-in failed: incomplete device authorization answer"));
    };
    Ok(DeviceAuth {
        user_code,
        verification_uri,
        expires_in: r.expires_in.unwrap_or(900),
        device_code: Secret::new(device_code),
        interval: r.interval.unwrap_or(5).max(1),
        grant: Grant { client_id: client_id.to_owned(), token_url, refresh_token: None, expires_at: None },
    })
}

/// Poll until the user answers (RFC 8628 §3.4-3.5), then store the tokens at `secret`.
pub async fn device_finish(
    http: &HttpClient,
    secrets: &dyn SecretResolver,
    secret: &SecretRef,
    auth: DeviceAuth,
) -> Result<(), KeltaError> {
    let expired = || KeltaError::new(ErrorCode::Timeout, "the sign-in code expired: start again");
    let deadline = Instant::now() + Duration::from_secs(auth.expires_in);
    let mut interval = auth.interval;
    loop {
        // one-shot: RFC 8628 wait between polls, re-armed only while the user has not answered
        tokio::time::sleep(Duration::from_secs(interval)).await;
        if Instant::now() >= deadline {
            return Err(expired());
        }
        let form = [
            ("client_id", auth.grant.client_id.as_str()),
            ("device_code", auth.device_code.expose()),
            ("grant_type", DEVICE_GRANT),
        ];
        let r: Resp = match post_form(http, &auth.grant.token_url, &form).await {
            Ok(r) => r,
            // RFC 8628 §3.5: keep polling through transient failures; Upstream = a non-JSON body (a 5xx page)
            Err(e) if matches!(e.code, ErrorCode::Network | ErrorCode::Timeout | ErrorCode::Upstream) => {
                continue;
            }
            Err(e) => return Err(e),
        };
        match r.error.as_deref() {
            None => return store(secrets, secret, auth.grant, r).await.map(drop),
            Some("authorization_pending") => {}
            Some("slow_down") => interval = r.interval.unwrap_or(0).max(interval + 5),
            // GitHub's docs name it both ways
            Some("expired_token" | "token_expired") => return Err(expired()),
            Some("access_denied") => {
                return Err(KeltaError::new(ErrorCode::Cancelled, "sign-in was denied in the browser"));
            }
            Some(_) => return Err(oauth_error(&r)),
        }
    }
}

/// Write the access token at `secret` and the grant next to it. Returns the access token.
async fn store(
    secrets: &dyn SecretResolver,
    secret: &SecretRef,
    mut grant: Grant,
    r: Resp,
) -> Result<Secret, KeltaError> {
    let access = Secret::new(
        r.access_token
            .filter(|t| !t.is_empty())
            .ok_or_else(|| KeltaError::upstream("sign-in failed: no access token in the answer"))?,
    );
    // refresh tokens rotate; keep the previous one if the server did not send a new one
    if r.refresh_token.is_some() {
        grant.refresh_token = r.refresh_token;
    }
    grant.expires_at = r.expires_in.map(|s| now_unix() + s as i64);
    secrets.set(secret, access.expose()).await?;
    let blob = Secret::new(serde_json::to_string(&grant).map_err(|e| KeltaError::internal(e.to_string()))?);
    secrets.set(&grant_ref(secret), blob.expose()).await?;
    tracing::info!(refresh = grant.refresh_token.is_some(), "oauth tokens stored");
    Ok(access)
}

async fn read_grant(secrets: &dyn SecretResolver, secret: &SecretRef, ctx: &SecretCtx) -> Option<Grant> {
    let blob = secrets.resolve(&grant_ref(secret), ctx).await.ok()?;
    serde_json::from_str(blob.expose()).ok()
}

/// For an OAuth secret: a refreshed access token when the stored one expires within a minute,
/// `None` when it is still good (or has no grant, e.g. a token stored by hand). A failed refresh
/// is `NeedsAuth`.
pub async fn refreshed(
    http: &HttpClient,
    secrets: &dyn SecretResolver,
    secret: &SecretRef,
    ctx: &SecretCtx,
) -> Result<Option<Secret>, KeltaError> {
    if !read_grant(secrets, secret, ctx).await.is_some_and(|g| g.expiring()) {
        return Ok(None);
    }
    let _one_at_a_time = REFRESH.lock().await;
    // another request may have refreshed while this one waited
    let Some(grant) = read_grant(secrets, secret, ctx).await.filter(Grant::expiring) else {
        return Ok(None);
    };
    let Some(refresh_token) = grant.refresh_token.as_deref() else {
        return Err(sign_in_again("the OAuth token expired"));
    };
    let form = [
        ("client_id", grant.client_id.as_str()),
        ("grant_type", "refresh_token"),
        ("refresh_token", refresh_token),
    ];
    // offline or a 5xx page: the refresh token is still good, the next request retries
    let r: Resp = post_form(http, &grant.token_url, &form).await?;
    if let Some(code) = &r.error {
        tracing::warn!(error = %code, "oauth refresh refused");
        return Err(sign_in_again("the OAuth token could not be refreshed"));
    }
    store(secrets, secret, grant, r).await.map(Some)
}

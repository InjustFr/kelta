//! "Sign in with browser" (device flow, SETTINGS §5): the device code stays here between
//! `oauth_device_start` and `oauth_device_finish`, so it never crosses IPC; the tokens go to the
//! secret backend.

use std::sync::Arc;

use kelta_http::oauth::{self, DeviceAuth};
use kelta_proto::error::KeltaError;
use kelta_proto::secret::{OAuthDevicePrompt, SecretRef, SecretSource};
use kelta_proto::settings::AccountKind;
use tokio::sync::Notify;

use crate::Core;

pub(crate) type Pending = (DeviceAuth, SecretRef);

impl Core {
    /// Ask GitHub / GitLab for a device code. `secret` (a `keyring:` or `file:` ref) receives the
    /// token once [`Core::oauth_device_finish`] sees the user approve.
    pub async fn oauth_device_start(
        &self,
        kind: AccountKind,
        base_url: &str,
        secret: SecretRef,
    ) -> Result<OAuthDevicePrompt, KeltaError> {
        if !matches!(secret.parse(), Some(SecretSource::Keyring(_) | SecretSource::File(_))) {
            return Err(KeltaError::invalid(
                "browser sign-in stores the token in a keyring: or file: reference",
            ));
        }
        let host = oauth::client_host(kind, base_url)?;
        let settings = self.cfg.effective(None);
        let client_id = settings.oauth.client_ids.get(&host).map(|s| s.trim()).filter(|s| !s.is_empty());
        let Some(client_id) = client_id else {
            return Err(KeltaError::invalid(format!(
                "no OAuth app for {host}: register one and set oauth.client_ids.\"{host}\""
            )));
        };
        let auth = oauth::device_start(self.http(), kind, base_url, client_id).await?;
        let prompt = OAuthDevicePrompt {
            user_code: auth.user_code.clone(),
            verification_uri: auth.verification_uri.clone(),
            expires_in: auth.expires_in,
        };
        self.oauth.lock().insert(prompt.user_code.clone(), (auth, secret));
        Ok(prompt)
    }

    /// Wait for the user to approve `user_code` in the browser, then store the tokens.
    /// `cancelled` when [`Core::oauth_device_cancel`] stops it first.
    pub async fn oauth_device_finish(&self, user_code: &str) -> Result<(), KeltaError> {
        let (auth, secret) = self
            .oauth
            .lock()
            .remove(user_code)
            .ok_or_else(|| KeltaError::not_found("no sign-in in progress for this code"))?;
        let cancel = Arc::new(Notify::new());
        self.oauth_polling.lock().insert(user_code.to_owned(), cancel.clone());
        let secrets = self.secret_resolver();
        let res = tokio::select! {
            r = oauth::device_finish(self.http(), &*secrets, &secret, auth) => r,
            () = cancel.notified() => Err(KeltaError::cancelled("sign-in cancelled")),
        };
        self.oauth_polling.lock().remove(user_code);
        res
    }

    /// Drop a sign-in the user walked away from (the wizard closed): stops its polling.
    pub fn oauth_device_cancel(&self, user_code: &str) {
        self.oauth.lock().remove(user_code);
        if let Some(cancel) = self.oauth_polling.lock().remove(user_code) {
            // notify_one keeps a permit, so a cancel that lands before the poll awaits still counts
            cancel.notify_one();
        }
    }
}

//! Kelta's MCP for every Claude Code session (ticket #234): the global `/mcp` registered at user
//! scope with `claude mcp add`, offered at startup when Claude Code is found, like an IDE's MCP.

use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;
use std::time::Duration;

use kelta_proto::api::CoreApi;
use kelta_proto::error::KeltaError;
use kelta_proto::events::{Toast, ToastAction};
use kelta_proto::secret::{SecretCtx, SecretRef};

use crate::Core;

/// Server name in Claude Code's MCP list.
const NAME: &str = "kelta";
/// The install token, apart from the per-session tokens.
const TOKEN_REF: &str = "keyring:kelta-mcp";
/// UI actions of the offer toast (registered in the settings view).
pub const INSTALL_ACTION: &str = "claude.mcp_install";
pub const NEVER_ACTION: &str = "claude.mcp_never";

/// Port of a registered global `/mcp` URL.
fn registered_port(url: &str) -> Option<u16> {
    let rest = url.split("http://127.0.0.1:").nth(1)?;
    let (port, path) = rest.split_once('/')?;
    path.starts_with("mcp").then(|| port.parse().ok()).flatten()
}

/// A user-scope registration as Claude Code keeps it: `(url, Authorization header value)`.
type Registration = (String, String);

/// The `kelta` entry of a Claude Code config file (`mcpServers.kelta`), read without running
/// `claude` (a ~100 MB Node process).
fn registration_in(config: &[u8]) -> Option<Registration> {
    let v: serde_json::Value = serde_json::from_slice(config).ok()?;
    let e = v.get("mcpServers")?.get(NAME)?;
    let url = e.get("url")?.as_str()?.to_owned();
    let auth = e.pointer("/headers/Authorization").and_then(|a| a.as_str()).unwrap_or_default();
    Some((url, auth.to_owned()))
}

impl Core {
    fn claude_bin(&self) -> Option<PathBuf> {
        let bin = self.cfg.effective(None).claude.binary.clone();
        crate::spawn_env::resolve_program(&bin, self.login_env.path(), &std::env::temp_dir()).ok()
    }

    /// `claude mcp <args>` in the login environment; stdout, or an error when it exits non-zero.
    async fn claude_mcp(&self, bin: &Path, args: &[&str]) -> Result<String, KeltaError> {
        let home = self.home_dir();
        let argv = std::iter::once("mcp").chain(args.iter().copied());
        // one-shot: bounded `claude mcp add|remove` call.
        let out =
            crate::ctl::claude_oneshot(bin, argv, &self.login_env.vars, &home, None, Duration::from_secs(30))
                .await?;
        if !out.status.success() {
            let err = String::from_utf8_lossy(if out.stderr.is_empty() { &out.stdout } else { &out.stderr });
            return Err(KeltaError::upstream(format!("claude mcp {}: {}", args[0], err.trim())));
        }
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    }

    /// The user-scope registration in `$CLAUDE_CONFIG_DIR/.claude.json` (else `~/.claude.json`).
    async fn claude_mcp_registration(&self) -> Option<Registration> {
        let dir =
            self.login_env.get("CLAUDE_CONFIG_DIR").map(PathBuf::from).unwrap_or_else(|| self.home_dir());
        registration_in(&tokio::fs::read(dir.join(".claude.json")).await.ok()?)
    }

    /// Startup: offer the install when Claude Code is found and `kelta` is not registered; serve an
    /// existing registration (consented to earlier), re-registering it when its port or token changed.
    pub async fn claude_mcp_startup(&self) {
        let Some(bin) = self.claude_bin() else { return };
        match self.claude_mcp_registration().await {
            Some(current) => {
                if let Err(e) = self.claude_mcp_serve(&bin, Some(&current)).await {
                    tracing::warn!(error = %e.message, "kelta MCP registration not updated");
                }
            }
            None if self.cfg.effective(None).claude.mcp_offer => {
                let action = |label: &str, command: &str| ToastAction {
                    label: label.into(),
                    command: command.into(),
                    args: None,
                };
                let mut t =
                    Toast::info("Claude Code detected. Let your Claude sessions use Kelta's ticket tools?");
                t.action = Some(action("Install", INSTALL_ACTION));
                t.secondary = Some(action("Never", NEVER_ACTION));
                self.toast(t);
            }
            None => {}
        }
    }

    /// Open the global `/mcp` and make the user-scope registration point at it. `current` = an
    /// existing registration.
    async fn claude_mcp_serve(&self, bin: &Path, current: Option<&Registration>) -> Result<(), KeltaError> {
        let r = SecretRef::new(TOKEN_REF);
        let token = match self.resolver.resolve(&r, &SecretCtx::default()).await {
            Ok(s) => s.expose().to_owned(),
            Err(_) => {
                let t = crate::spawn_env::token();
                // Unsaved, it still serves this run; the next start registers a new one.
                if let Err(e) = self.resolver.set(&r, &t).await {
                    tracing::warn!(error = %e.message, "kelta MCP install token not saved");
                }
                t
            }
        };
        self.server.set_install_token(Some(&token));
        let port = match self.server.http_port().filter(|_| self.claude_mcp_ref.load(Ordering::SeqCst)) {
            Some(p) => p,
            None => {
                let p = self.server.ensure_http_at(current.and_then(|(url, _)| registered_port(url))).await?;
                self.claude_mcp_ref.store(true, Ordering::SeqCst);
                p
            }
        };
        let url = format!("http://127.0.0.1:{port}/mcp");
        let bearer = format!("Bearer {token}");
        if current.is_some_and(|(u, auth)| *u == url && *auth == bearer) {
            return Ok(());
        }
        let header = format!("Authorization: {bearer}");
        let _ = self.claude_mcp(bin, &["remove", "--scope", "user", NAME]).await;
        self.claude_mcp(
            bin,
            &["add", "--scope", "user", "--transport", "http", NAME, &url, "--header", &header],
        )
        .await?;
        tracing::info!(port, "kelta MCP registered in Claude Code (user scope)");
        Ok(())
    }

    /// Settings → Claude: `None` = Claude Code not found, else whether `kelta` is registered.
    pub async fn claude_mcp_status(&self) -> Option<bool> {
        self.claude_bin()?;
        Some(self.claude_mcp_registration().await.is_some())
    }

    /// Install (toast or Settings → Claude): `claude mcp add --scope user`.
    pub async fn claude_mcp_install(&self) -> Result<(), KeltaError> {
        let bin = self
            .claude_bin()
            .ok_or_else(|| KeltaError::not_found("Claude Code not found in your login PATH"))?;
        self.claude_mcp_serve(&bin, None).await
    }

    /// Remove: `claude mcp remove`, then revoke the install token and close the global `/mcp`.
    pub async fn claude_mcp_remove(&self) -> Result<(), KeltaError> {
        if let Some(bin) = self.claude_bin() {
            self.claude_mcp(&bin, &["remove", "--scope", "user", NAME]).await?;
        }
        if let Err(e) = self.resolver.delete(&SecretRef::new(TOKEN_REF)).await {
            tracing::warn!(error = %e.message, "kelta MCP install token not deleted");
        }
        self.server.set_install_token(None);
        if self.claude_mcp_ref.swap(false, Ordering::SeqCst) {
            self.server.release_http();
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn port_of_registration() {
        assert_eq!(registered_port("http://127.0.0.1:45678/mcp"), Some(45678));
        assert_eq!(registered_port("http://127.0.0.1:45678/mcp/s1"), Some(45678));
        assert_eq!(registered_port("http://127.0.0.1:45678/other"), None);
    }

    #[test]
    fn registration_from_the_config_file() {
        let cfg = br#"{"numStartups":3,"mcpServers":{"other":{"type":"stdio"},"kelta":{"type":"http","url":"http://127.0.0.1:45678/mcp","headers":{"Authorization":"Bearer t"}}}}"#;
        assert_eq!(registration_in(cfg), Some(("http://127.0.0.1:45678/mcp".into(), "Bearer t".into())));
        assert_eq!(registration_in(br#"{"mcpServers":{"other":{"url":"x"}}}"#), None);
        assert_eq!(registration_in(b"{}"), None);
        assert_eq!(registration_in(b"not json"), None);
    }
}

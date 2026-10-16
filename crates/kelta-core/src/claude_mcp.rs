//! Kelta's MCP for every Claude Code session (ticket #234): the global `/mcp` registered at user
//! scope in Claude Code's config (`mcpServers.kelta`), offered at startup when Claude Code is found, like an IDE's MCP.

use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use kelta_proto::api::CoreApi;
use kelta_proto::error::KeltaError;
use kelta_proto::events::{Toast, ToastAction};
use serde_json::value::RawValue;

use crate::Core;

/// Server name in Claude Code's MCP list.
const NAME: &str = "kelta";
/// UI actions of the offer toast (registered in the settings view).
pub const INSTALL_ACTION: &str = "claude.mcp_install";
pub const NEVER_ACTION: &str = "claude.mcp_never";

/// Claude Code's user config: `.claude.json` in a session's config dir when `CLAUDE_CONFIG_DIR` is
/// set, else in `$HOME` (beside `~/.claude`, not in it).
fn claude_config_file(env: &kelta_proto::term::LoginEnv) -> PathBuf {
    let dir = env.get("CLAUDE_CONFIG_DIR").filter(|d| !d.is_empty());
    let dir = dir.and_then(|_| crate::spawn_env::claude_config_dir(&env.vars));
    dir.unwrap_or_else(|| crate::projects::home_dir(env)).join(".claude.json")
}

/// The install token (apart from the per-session tokens): the one Claude Code already holds in
/// plain text in its registration, else a new one. Never in the Keychain (#275): it would add
/// prompts, no secrecy.
fn install_token(current: Option<&Registration>) -> String {
    current
        .and_then(|(_, auth)| auth.strip_prefix("Bearer "))
        .filter(|t| !t.is_empty())
        .map_or_else(crate::spawn_env::token, str::to_owned)
}

/// Port of a Kelta MCP URL (the global `/mcp` or a session's `/mcp/<sid>`).
pub(crate) fn registered_port(url: &str) -> Option<u16> {
    let rest = url.split("http://127.0.0.1:").nth(1)?;
    let (port, path) = rest.split_once('/')?;
    path.starts_with("mcp").then(|| port.parse().ok()).flatten()
}

/// A user-scope registration as Claude Code keeps it: `(url, Authorization header value)`.
pub(crate) type Registration = (String, String);

/// The `kelta` entry of a Claude Code config file (`mcpServers.kelta`), read without running
/// `claude` (a ~100 MB Node process).
fn registration_in(config: &[u8]) -> Option<Registration> {
    let v: serde_json::Value = serde_json::from_slice(config).ok()?;
    let e = v.get("mcpServers")?.get(NAME)?;
    let url = e.get("url")?.as_str()?.to_owned();
    let auth = e.pointer("/headers/Authorization").and_then(|a| a.as_str()).unwrap_or_default();
    Some((url, auth.to_owned()))
}

/// A JSON object in file order, its values left as written. Not `serde_json::Value`: without the
/// workspace-wide `preserve_order` feature (it changes other crates' output) it re-sorts every key of the
/// user's config (#310).
#[derive(Default)]
pub(crate) struct Ordered(pub(crate) Vec<(String, Json)>);

/// A value of an [`Ordered`] object: as read, or rebuilt by Kelta and so indented at its depth (#322).
#[derive(serde::Serialize)]
#[serde(untagged)]
pub(crate) enum Json {
    Raw(Box<RawValue>),
    Object(Ordered),
    Array(Vec<Json>),
    Value(serde_json::Value),
}

impl<'de> serde::Deserialize<'de> for Ordered {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct V;
        impl<'de> serde::de::Visitor<'de> for V {
            type Value = Ordered;
            fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                f.write_str("a JSON object")
            }
            fn visit_map<A: serde::de::MapAccess<'de>>(self, mut m: A) -> Result<Ordered, A::Error> {
                let mut v = Vec::new();
                while let Some((k, raw)) = m.next_entry()? {
                    v.push((k, Json::Raw(raw)));
                }
                Ok(Ordered(v))
            }
        }
        d.deserialize_map(V)
    }
}

impl serde::Serialize for Ordered {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.collect_map(self.0.iter().map(|(k, v)| (k, v)))
    }
}

impl Ordered {
    /// `key` as read from the file.
    pub(crate) fn get(&self, key: &str) -> Option<&RawValue> {
        self.0.iter().find_map(|(k, v)| match v {
            Json::Raw(raw) if k == key => Some(&**raw),
            _ => None,
        })
    }

    /// Replaces `key` in place, else appends it.
    pub(crate) fn set(&mut self, key: &str, value: Json) {
        match self.0.iter_mut().find(|(k, _)| k == key) {
            Some((_, v)) => *v = value,
            None => self.0.push((key.to_owned(), value)),
        }
    }

    pub(crate) fn remove(&mut self, key: &str) -> bool {
        let n = self.0.len();
        self.0.retain(|(k, _)| k != key);
        self.0.len() != n
    }
}

/// Set (`Some`) or drop (`None`) `mcpServers.kelta` in a Claude Code config file, keeping the other
/// keys in their order: written beside it, 0600 (it holds the token), then renamed over it. Not
/// `claude mcp add`: its argv would show the token to any local user in `ps` (#294).
fn write_registration(path: &Path, reg: Option<&Registration>) -> Result<(), KeltaError> {
    // A symlinked config (dotfiles) is updated where it points.
    let path = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_owned());
    let invalid = |e: serde_json::Error| KeltaError::invalid(format!("{}: {e}", path.display()));
    let mut v: Ordered = match std::fs::read(&path) {
        // Unparsable or not an object: refuse rather than clobber Claude Code's settings.
        Ok(b) => serde_json::from_slice(&b).map_err(invalid)?,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ordered::default(),
        Err(e) => return Err(e.into()),
    };
    let mut servers: Ordered = match v.get("mcpServers") {
        Some(raw) => serde_json::from_str(raw.get())
            .map_err(|_| KeltaError::invalid(format!("{}: mcpServers is not an object", path.display())))?,
        None => Ordered::default(),
    };
    match reg {
        Some((url, auth)) => {
            let entry = serde_json::json!({"type": "http", "url": url, "headers": {"Authorization": auth}});
            servers.set(NAME, Json::Value(entry));
        }
        None if !servers.remove(NAME) => return Ok(()),
        None => {}
    }
    v.set("mcpServers", Json::Object(servers));
    let mut tmp = path.clone().into_os_string();
    tmp.push(format!(".kelta-{}", std::process::id()));
    let tmp = PathBuf::from(tmp);
    crate::templates::write_private(&tmp, &serde_json::to_string_pretty(&v)?)?;
    std::fs::rename(&tmp, &path).inspect_err(|_| {
        let _ = std::fs::remove_file(&tmp);
    })?;
    Ok(())
}

/// Claude Code's lock on its config (proper-lockfile): a `<config>.lock` directory, refreshed while
/// held, stale after 10 s. Kelta holds it across its read-modify-write so a concurrent Claude Code save
/// re-reads Kelta's change instead of overwriting it (#322). Removed on drop.
pub(crate) struct ConfigLock(PathBuf);

impl ConfigLock {
    /// `None` when the config's directory does not exist yet: no Claude Code to race with.
    pub(crate) async fn acquire(config: &Path) -> Result<Option<Self>, KeltaError> {
        const STALE: Duration = Duration::from_secs(10);
        let mut dir = config.as_os_str().to_owned();
        dir.push(".lock");
        let dir = PathBuf::from(dir);
        let deadline = Instant::now() + STALE;
        loop {
            match std::fs::create_dir(&dir) {
                Ok(()) => return Ok(Some(Self(dir))),
                Err(e) if e.kind() == ErrorKind::NotFound => return Ok(None),
                Err(e) if e.kind() != ErrorKind::AlreadyExists => return Err(e.into()),
                Err(_) => {}
            }
            let age = std::fs::metadata(&dir).and_then(|m| m.modified()).ok().and_then(|t| t.elapsed().ok());
            if age.is_some_and(|a| a > STALE) {
                // Left by a crashed Claude Code: proper-lockfile takes it over the same way.
                let _ = std::fs::remove_dir(&dir);
            } else if Instant::now() > deadline {
                return Err(KeltaError::conflict(format!("{} is held by another process", dir.display())));
            } else {
                // Bounded wait for a lock Claude Code holds for milliseconds, not a polling loop.
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
        }
    }
}

impl Drop for ConfigLock {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir(&self.0);
    }
}

/// Write `reg` under Claude Code's lock unless the config already holds it; true when it wrote. Also the
/// check after a registration: a concurrent save that dropped it gets it back (#322).
async fn ensure_registration(path: &Path, reg: &Registration) -> Result<bool, KeltaError> {
    let _lock = ConfigLock::acquire(path).await?;
    if std::fs::read(path).ok().and_then(|b| registration_in(&b)).as_ref() == Some(reg) {
        return Ok(false);
    }
    write_registration(path, Some(reg))?;
    Ok(true)
}

/// Delay of the check after a registration write.
const RECHECK: Duration = Duration::from_secs(5);

impl Core {
    pub(crate) fn claude_bin(&self) -> Option<PathBuf> {
        let bin = self.cfg.effective(None).claude.binary.clone();
        crate::spawn_env::resolve_program(&bin, self.login_env.path(), &std::env::temp_dir()).ok()
    }

    /// Claude Code's user config: `$CLAUDE_CONFIG_DIR/.claude.json`, else `~/.claude.json`.
    fn claude_config(&self) -> PathBuf {
        claude_config_file(&self.login_env)
    }

    /// The user-scope registration in Claude Code's user config.
    async fn claude_mcp_registration(&self) -> Option<Registration> {
        registration_in(&tokio::fs::read(self.claude_config()).await.ok()?)
    }

    /// Startup: offer the install when Claude Code is found and `kelta` is not registered; serve an
    /// existing registration (consented to earlier), re-registering it when its port or token changed.
    pub async fn claude_mcp_startup(&self) {
        if self.claude_bin().is_none() {
            return;
        }
        match self.claude_mcp_registration().await {
            Some(current) => {
                if let Err(e) = self.claude_mcp_serve(Some(&current)).await {
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
    async fn claude_mcp_serve(&self, current: Option<&Registration>) -> Result<(), KeltaError> {
        let mut held = self.claude_mcp_ref.lock().await;
        let token = install_token(current);
        self.server.set_install_token(Some(&token));
        let port = match self.server.http_port().filter(|_| held.is_some()) {
            Some(p) => p,
            None => self.server.ensure_http_at(current.and_then(|(url, _)| registered_port(url))).await?,
        };
        let reg = (format!("http://127.0.0.1:{port}/mcp"), format!("Bearer {token}"));
        *held = Some(reg.clone());
        if current == Some(&reg) || !ensure_registration(&self.claude_config(), &reg).await? {
            return Ok(());
        }
        tracing::info!(port, "kelta MCP registered in Claude Code (user scope)");
        let me = self.me.clone();
        // one-shot: armed by a registration write, re-armed only by the next one.
        self.claude_mcp_recheck.arm(&self.rt, RECHECK, async move {
            let Some(core) = me.upgrade() else { return };
            let held = core.claude_mcp_ref.lock().await;
            // Removed since: nothing to restore.
            let Some(reg) = held.as_ref() else { return };
            match ensure_registration(&core.claude_config(), reg).await {
                Ok(true) => tracing::info!("kelta MCP registration dropped by a concurrent save, restored"),
                Ok(false) => {}
                Err(e) => tracing::warn!(error = %e.message, "kelta MCP registration not checked"),
            }
        });
        Ok(())
    }

    /// Settings → Claude: `None` = Claude Code not found, else whether `kelta` is registered.
    pub async fn claude_mcp_status(&self) -> Option<bool> {
        self.claude_bin()?;
        Some(self.claude_mcp_registration().await.is_some())
    }

    /// Install (toast or Settings → Claude): registers `kelta` at user scope.
    pub async fn claude_mcp_install(&self) -> Result<(), KeltaError> {
        self.claude_bin().ok_or_else(|| KeltaError::not_found("Claude Code not found in your login PATH"))?;
        self.claude_mcp_serve(None).await
    }

    /// Remove: drop the registration (the token goes with it), then revoke it and close the global
    /// `/mcp`. A `dev.kelta`/`kelta-mcp` Keychain item left by builds before #275 is not deleted:
    /// touching it would prompt.
    pub async fn claude_mcp_remove(&self) -> Result<(), KeltaError> {
        let mut held = self.claude_mcp_ref.lock().await;
        let config = self.claude_config();
        let lock = ConfigLock::acquire(&config).await?;
        write_registration(&config, None)?;
        drop(lock);
        self.claude_mcp_recheck.cancel();
        self.server.set_install_token(None);
        if held.take().is_some() {
            self.server.release_http();
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_file_follows_the_session_config_dir() {
        let env = |dir: Option<&str>| kelta_proto::term::LoginEnv {
            vars: [("HOME", Some("/h")), ("CLAUDE_CONFIG_DIR", dir)]
                .into_iter()
                .filter_map(|(k, v)| Some((k.to_owned(), v?.to_owned())))
                .collect(),
            ..Default::default()
        };
        let tilde = env(Some("~/x"));
        assert_eq!(claude_config_file(&tilde), PathBuf::from("/h/x/.claude.json"));
        assert_eq!(
            claude_config_file(&tilde),
            crate::spawn_env::claude_config_dir(&tilde.vars).unwrap().join(".claude.json")
        );
        assert_eq!(claude_config_file(&env(Some(""))), PathBuf::from("/h/.claude.json"));
        assert_eq!(claude_config_file(&env(None)), PathBuf::from("/h/.claude.json"));
    }

    #[test]
    fn port_of_registration() {
        assert_eq!(registered_port("http://127.0.0.1:45678/mcp"), Some(45678));
        assert_eq!(registered_port("http://127.0.0.1:45678/mcp/s1"), Some(45678));
        assert_eq!(registered_port("http://127.0.0.1:45678/other"), None);
    }

    #[test]
    fn token_from_the_registration() {
        let reg = |auth: &str| ("http://127.0.0.1:1/mcp".to_owned(), auth.to_owned());
        assert_eq!(install_token(Some(&reg("Bearer abc"))), "abc");
        for fresh in
            [install_token(None), install_token(Some(&reg(""))), install_token(Some(&reg("Bearer ")))]
        {
            assert!(fresh.len() >= 32, "{fresh}");
        }
    }

    #[test]
    fn registration_from_the_config_file() {
        let cfg = br#"{"numStartups":3,"mcpServers":{"other":{"type":"stdio"},"kelta":{"type":"http","url":"http://127.0.0.1:45678/mcp","headers":{"Authorization":"Bearer t"}}}}"#;
        assert_eq!(registration_in(cfg), Some(("http://127.0.0.1:45678/mcp".into(), "Bearer t".into())));
        assert_eq!(registration_in(br#"{"mcpServers":{"other":{"url":"x"}}}"#), None);
        assert_eq!(registration_in(b"{}"), None);
        assert_eq!(registration_in(b"not json"), None);
    }

    #[test]
    fn registration_written_to_the_config_file() {
        use std::os::unix::fs::PermissionsExt;
        let tmp = tempfile::tempdir().unwrap();
        let real = tmp.path().join("dotfiles.json");
        std::fs::write(&real, r#"{"numStartups":3,"mcpServers":{"other":{"type":"stdio"}}}"#).unwrap();
        std::fs::set_permissions(&real, std::fs::Permissions::from_mode(0o644)).unwrap();
        let link = tmp.path().join(".claude.json");
        std::os::unix::fs::symlink(&real, &link).unwrap();
        let reg = ("http://127.0.0.1:45678/mcp".to_owned(), "Bearer t".to_owned());

        write_registration(&link, Some(&reg)).unwrap();
        assert_eq!(registration_in(&std::fs::read(&link).unwrap()), Some(reg));
        assert!(std::fs::symlink_metadata(&link).unwrap().file_type().is_symlink());
        assert_eq!(std::fs::metadata(&real).unwrap().permissions().mode() & 0o777, 0o600);
        let v: serde_json::Value = serde_json::from_slice(&std::fs::read(&real).unwrap()).unwrap();
        assert_eq!(v["numStartups"], 3);
        assert_eq!(v["mcpServers"]["other"]["type"], "stdio");

        write_registration(&link, None).unwrap();
        let v: serde_json::Value = serde_json::from_slice(&std::fs::read(&real).unwrap()).unwrap();
        assert_eq!(v, serde_json::json!({"numStartups":3,"mcpServers":{"other":{"type":"stdio"}}}));
        assert_eq!(std::fs::read_dir(tmp.path()).unwrap().count(), 2, "no temp file left");

        // Unparsable: left as it is. Missing: created; removing from a missing file writes nothing.
        std::fs::write(&real, "not json").unwrap();
        assert!(write_registration(&link, None).is_err());
        assert_eq!(std::fs::read(&real).unwrap(), b"not json");
        let fresh = tmp.path().join("cfg/.claude.json");
        write_registration(&fresh, None).unwrap();
        assert!(!fresh.exists());
        write_registration(&fresh, Some(&("u".into(), "a".into()))).unwrap();
        assert_eq!(registration_in(&std::fs::read(&fresh).unwrap()), Some(("u".into(), "a".into())));
    }

    #[test]
    fn registration_keeps_the_config_key_order() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join(".claude.json");
        let keys = |p: &Path| {
            let v: Ordered = serde_json::from_slice(&std::fs::read(p).unwrap()).unwrap();
            let s: Ordered = serde_json::from_str(v.get("mcpServers").unwrap().get()).unwrap();
            let names = |o: &Ordered| o.0.iter().map(|(k, _)| k.clone()).collect::<Vec<_>>();
            (names(&v), names(&s))
        };
        std::fs::write(&path, r#"{"zeta":1,"mcpServers":{"z":{},"a":{}},"alpha":2}"#).unwrap();
        let before =
            (vec!["zeta".to_owned(), "mcpServers".into(), "alpha".into()], vec!["z".to_owned(), "a".into()]);
        assert_eq!(keys(&path), before, "read in file order");
        write_registration(&path, Some(&("u".into(), "a".into()))).unwrap();
        let (top, servers) = keys(&path);
        assert_eq!((top, &servers[..2]), (before.0.clone(), &before.1[..]));
        write_registration(&path, None).unwrap();
        assert_eq!(keys(&path), before);
    }

    #[test]
    fn registration_keeps_the_config_indentation() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join(".claude.json");
        // As Claude Code writes it (`JSON.stringify(c, null, 2)`); keys sorted so `to_string_pretty` agrees.
        let doc = |servers: serde_json::Value| {
            serde_json::to_string_pretty(&serde_json::json!({
                "a": 1, "mcpServers": servers, "projects": {"/x": {"allowedTools": ["Read"]}}
            }))
            .unwrap()
        };
        let other = serde_json::json!({"command": "x", "type": "stdio"});
        let before = doc(serde_json::json!({"a-other": other}));
        std::fs::write(&path, &before).unwrap();
        let entry = serde_json::json!({"headers": {"Authorization": "Bearer t"}, "type": "http", "url": "u"});

        write_registration(&path, Some(&("u".into(), "Bearer t".into()))).unwrap();
        let after = std::fs::read_to_string(&path).unwrap();
        assert_eq!(after, doc(serde_json::json!({"a-other": other, "kelta": entry})), "{after}");
        write_registration(&path, None).unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), before);
    }

    #[tokio::test]
    async fn registration_dropped_by_a_concurrent_save_is_restored() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join(".claude.json");
        let lock = tmp.path().join(".claude.json.lock");
        let reg = ("http://127.0.0.1:45678/mcp".to_owned(), "Bearer t".to_owned());
        std::fs::write(&path, r#"{"numStartups":3}"#).unwrap();
        assert!(ensure_registration(&path, &reg).await.unwrap());
        assert!(!ensure_registration(&path, &reg).await.unwrap(), "up to date: not rewritten");

        // Claude Code saves the copy it read before Kelta's write: the next check puts the entry back.
        std::fs::write(&path, r#"{"numStartups":4,"mcpServers":{}}"#).unwrap();
        // ... once Claude Code's lock (proper-lockfile's `<config>.lock` directory) is released.
        std::fs::create_dir(&lock).unwrap();
        let release = {
            let lock = lock.clone();
            tokio::spawn(async move {
                tokio::time::sleep(Duration::from_millis(200)).await;
                std::fs::remove_dir(lock).unwrap();
            })
        };
        assert!(ensure_registration(&path, &reg).await.unwrap());
        release.await.unwrap();
        assert_eq!(registration_in(&std::fs::read(&path).unwrap()), Some(reg.clone()));
        let v: serde_json::Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(v["numStartups"], 4, "Claude Code's save kept");
        assert!(!lock.exists(), "Kelta's lock released");

        // A lock left by a crashed Claude Code (older than 10 s) is taken over.
        std::fs::write(&path, "{}").unwrap();
        std::fs::create_dir(&lock).unwrap();
        let old = std::time::SystemTime::now() - Duration::from_secs(60);
        std::fs::File::open(&lock).unwrap().set_modified(old).unwrap();
        assert!(ensure_registration(&path, &reg).await.unwrap());
        assert!(!lock.exists());
    }
}

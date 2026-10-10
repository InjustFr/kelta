//! # kelta-secrets (L4)
//!
//! `SecretRef` resolution chain (SETTINGS §5): `keyring:` (macOS Keychain / Linux Secret Service
//! via keyring-core), `file:` (passphrase-encrypted file, unlocked once per run), `gh-cli`, `glab-cli`, `command:`, `env:`. Resolution runs off the caller's
//! thread with a 5 s timeout, results are cached in memory only (zeroized on drop) and the cache
//! is dropped whenever the effective settings change, on [`SecretResolver::invalidate`] (401) and
//! on `set` / `delete`. Secret values are never logged, serialized or put in error messages.

mod file;
mod glab;
mod words;

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use kelta_proto::api::{SecretResolver, SettingsSource};
use kelta_proto::error::{ErrorCode, KeltaError};
use kelta_proto::secret::{KEYRING_SERVICE, Secret, SecretBackendStatus, SecretCtx, SecretRef, SecretSource};
use kelta_proto::settings::Settings;
use keyring_core::CredentialStore;
use parking_lot::{Mutex, RwLock};

pub use glab::{token_from_status as glab_token_from_status, token_from_yaml as glab_token_from_yaml};
pub use words::split as split_command;

/// Timeout of every external lookup (SETTINGS §5).
pub const RESOLVE_TIMEOUT: Duration = Duration::from_secs(5);

/// Extra directories searched for `gh` / `glab` / `command:` programs when `PATH` (possibly the
/// minimal Dock-launch one on macOS) does not contain them.
const FALLBACK_DIRS: &[&str] = &["/opt/homebrew/bin", "/usr/local/bin", "/usr/bin", "/bin"];

/// Test and embedding hooks.
#[derive(Default)]
pub struct SecretsOptions {
    /// Credential store used for `keyring:` refs (default: the platform store, created lazily).
    pub store: Option<Arc<CredentialStore>>,
    /// Complete environment of child processes and `env:` lookups (default: this process).
    pub env: Option<BTreeMap<String, String>>,
    /// Lookup timeout (default [`RESOLVE_TIMEOUT`]).
    pub timeout: Option<Duration>,
    /// Encrypted secrets file for `file:` refs (default: none, `file:` refs are unsupported).
    pub file: Option<PathBuf>,
}

/// Name of the encrypted secrets file in the data dir.
pub const SECRETS_FILE: &str = "secrets.enc";

struct Cache {
    /// The settings snapshot the entries were resolved under; a different `Arc` means the
    /// settings changed. Holding it keeps the address unique.
    generation: Option<Arc<Settings>>,
    map: HashMap<String, Secret>,
}

pub struct Secrets {
    settings: Arc<dyn SettingsSource>,
    store: Mutex<Option<Arc<CredentialStore>>>,
    env: RwLock<Option<BTreeMap<String, String>>>,
    timeout: Duration,
    cache: Mutex<Cache>,
    file: Option<Arc<file::SecretFile>>,
}

fn wipe(v: &mut [u8]) {
    for b in v.iter_mut() {
        *b = 0;
    }
    std::hint::black_box(&*v);
}

fn ref_kind(src: &SecretSource) -> &'static str {
    match src {
        SecretSource::Keyring(_) => "keyring",
        SecretSource::File(_) => "file",
        SecretSource::GhCli => "gh-cli",
        SecretSource::GlabCli => "glab-cli",
        SecretSource::Command(_) => "command",
        SecretSource::Env(_) => "env",
    }
}

#[derive(Debug)]
enum RunError {
    NotFound,
    Timeout,
    Io(String),
}

struct Captured {
    code: Option<i32>,
    stdout: Vec<u8>,
    stderr: Vec<u8>,
}

impl Drop for Captured {
    fn drop(&mut self) {
        wipe(&mut self.stdout);
        wipe(&mut self.stderr);
    }
}

fn map_keyring(e: &keyring_core::Error, what: &str) -> KeltaError {
    use keyring_core::Error as E;
    match e {
        E::NoEntry => KeltaError::needs_auth(format!("no token stored for {what}")),
        E::NoStorageAccess(_) => KeltaError::new(
            ErrorCode::Upstream,
            "secret storage is locked or unavailable (unlock it, or use a command: / env: secret)",
        ),
        E::PlatformFailure(_) => KeltaError::new(ErrorCode::Upstream, "secret storage failed"),
        other => KeltaError::internal(format!("secret storage error: {other}")),
    }
}

impl Secrets {
    pub fn new(settings: Arc<dyn SettingsSource>) -> Arc<Self> {
        Self::with_options(settings, SecretsOptions::default())
    }

    pub fn with_options(settings: Arc<dyn SettingsSource>, opts: SecretsOptions) -> Arc<Self> {
        Arc::new(Self {
            settings,
            store: Mutex::new(opts.store),
            env: RwLock::new(opts.env),
            timeout: opts.timeout.unwrap_or(RESOLVE_TIMEOUT),
            cache: Mutex::new(Cache { generation: None, map: HashMap::new() }),
            file: opts.file.map(|p| Arc::new(file::SecretFile::new(p))),
        })
    }

    pub fn settings(&self) -> &Arc<dyn SettingsSource> {
        &self.settings
    }

    /// Use the login environment (full `PATH`) for child processes and `env:` refs.
    pub fn set_env(&self, env: BTreeMap<String, String>) {
        *self.env.write() = Some(env);
        self.clear_cache();
    }

    fn env_var(&self, key: &str) -> Option<String> {
        match &*self.env.read() {
            Some(m) => m.get(key).cloned(),
            None => std::env::var(key).ok(),
        }
    }

    // ---- cache -----------------------------------------------------------------------------

    fn clear_cache(&self) {
        self.cache.lock().map.clear();
    }

    fn cache_key(r: &SecretRef, ctx: &SecretCtx) -> String {
        format!("{}\u{1}{}", r.as_str(), ctx.host.as_deref().unwrap_or(""))
    }

    fn cached(&self, key: &str) -> Option<Secret> {
        let current = self.settings.effective(None);
        let mut c = self.cache.lock();
        let same = c.generation.as_ref().is_some_and(|g| Arc::ptr_eq(g, &current));
        if !same {
            c.map.clear();
            c.generation = Some(current);
        }
        c.map.get(key).cloned()
    }

    fn remember(&self, key: String, secret: &Secret) {
        self.cache.lock().map.insert(key, secret.clone());
    }

    // ---- processes -------------------------------------------------------------------------

    fn find_program(&self, name: &str) -> Option<PathBuf> {
        let is_exec = |p: &Path| {
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                p.is_file() && std::fs::metadata(p).is_ok_and(|m| m.permissions().mode() & 0o111 != 0)
            }
            #[cfg(not(unix))]
            {
                p.is_file()
            }
        };
        if name.contains('/') {
            let p = PathBuf::from(name);
            return is_exec(&p).then_some(p);
        }
        let path = self.env_var("PATH").unwrap_or_default();
        for dir in std::env::split_paths(&path) {
            let c = dir.join(name);
            if is_exec(&c) {
                return Some(c);
            }
        }
        FALLBACK_DIRS.iter().map(|d| Path::new(d).join(name)).find(|c| is_exec(c))
    }

    async fn run(&self, program: &Path, args: &[String]) -> Result<Captured, RunError> {
        let mut cmd = tokio::process::Command::new(program);
        cmd.args(args)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .kill_on_drop(true);
        if let Some(env) = &*self.env.read() {
            cmd.env_clear().envs(env);
        }
        // one-shot: bound every external secret lookup (SETTINGS §5)
        match tokio::time::timeout(self.timeout, cmd.output()).await {
            Err(_) => Err(RunError::Timeout),
            Ok(Err(e)) if e.kind() == std::io::ErrorKind::NotFound => Err(RunError::NotFound),
            Ok(Err(e)) => Err(RunError::Io(e.kind().to_string())),
            Ok(Ok(o)) => Ok(Captured { code: o.status.code(), stdout: o.stdout, stderr: o.stderr }),
        }
    }

    fn run_error(what: &str, e: RunError) -> KeltaError {
        match e {
            RunError::NotFound => KeltaError::unsupported(format!("`{what}` was not found in PATH")),
            RunError::Timeout => KeltaError::timeout(format!("`{what}` did not answer within 5 s")),
            RunError::Io(kind) => KeltaError::internal(format!("could not run `{what}` ({kind})")),
        }
    }

    /// First non-empty trimmed line of stdout as a secret.
    fn secret_from_stdout(out: &Captured, what: &str) -> Result<Secret, KeltaError> {
        let text = String::from_utf8_lossy(&out.stdout);
        let line = text.lines().map(str::trim).find(|l| !l.is_empty());
        match line {
            Some(l) => Ok(Secret::new(l)),
            None => Err(KeltaError::needs_auth(format!("`{what}` returned an empty token"))),
        }
    }

    // ---- sources ---------------------------------------------------------------------------

    async fn resolve_command(&self, argv: &str) -> Result<Secret, KeltaError> {
        let words =
            words::split(argv).map_err(|e| KeltaError::invalid(format!("invalid command secret: {e}")))?;
        let Some((prog, args)) = words.split_first() else {
            return Err(KeltaError::invalid("empty command secret"));
        };
        let program = self.find_program(prog).ok_or_else(|| Self::run_error(prog, RunError::NotFound))?;
        let out = self.run(&program, args).await.map_err(|e| Self::run_error(prog, e))?;
        if out.code != Some(0) {
            return Err(KeltaError::needs_auth(format!(
                "`{prog}` failed (exit {})",
                out.code.map_or_else(|| "signal".to_owned(), |c| c.to_string())
            )));
        }
        Self::secret_from_stdout(&out, prog)
    }

    fn resolve_env(&self, var: &str) -> Result<Secret, KeltaError> {
        match self.env_var(var).filter(|v| !v.is_empty()) {
            Some(v) => Ok(Secret::new(v)),
            None => Err(KeltaError::needs_auth(format!("environment variable {var} is not set"))),
        }
    }

    async fn resolve_gh(&self, ctx: &SecretCtx) -> Result<Secret, KeltaError> {
        let mut host = ctx.host.clone().unwrap_or_else(|| "github.com".to_owned());
        if host == "api.github.com" {
            host = "github.com".to_owned();
        }
        let program = self.find_program("gh").ok_or_else(|| Self::run_error("gh", RunError::NotFound))?;
        let args = ["auth".to_owned(), "token".to_owned(), "--hostname".to_owned(), host.clone()];
        let out = self.run(&program, &args).await.map_err(|e| Self::run_error("gh", e))?;
        if out.code != Some(0) {
            return Err(KeltaError::needs_auth(format!(
                "gh is not logged in to {host} (run `gh auth login`)"
            )));
        }
        Self::secret_from_stdout(&out, "gh auth token")
    }

    async fn resolve_glab(&self, ctx: &SecretCtx) -> Result<Secret, KeltaError> {
        let host = ctx.host.clone().unwrap_or_else(|| "gitlab.com".to_owned());
        let paths = glab::config_paths(&|k| self.env_var(k));
        for p in paths {
            let Ok(text) = std::fs::read_to_string(&p) else { continue };
            let found = glab::token_from_yaml(&text, &host);
            wipe_string(text);
            if let Some(t) = found {
                let s = Secret::new(t.as_str());
                wipe_string(t);
                return Ok(s);
            }
        }
        let program = self.find_program("glab").ok_or_else(|| {
            KeltaError::needs_auth(format!(
                "no glab token for {host} in config.yml and `glab` was not found in PATH"
            ))
        })?;
        let args = [
            "auth".to_owned(),
            "status".to_owned(),
            "--show-token".to_owned(),
            "--hostname".to_owned(),
            host.clone(),
        ];
        let out = self.run(&program, &args).await.map_err(|e| Self::run_error("glab", e))?;
        // glab prints its status on stderr
        let mut combined = String::from_utf8_lossy(&out.stdout).into_owned();
        combined.push('\n');
        combined.push_str(&String::from_utf8_lossy(&out.stderr));
        let token = glab::token_from_status(&combined);
        wipe_string(combined);
        match token {
            Some(t) if out.code == Some(0) => {
                let s = Secret::new(t.as_str());
                wipe_string(t);
                Ok(s)
            }
            _ => Err(KeltaError::needs_auth(format!(
                "glab is not logged in to {host} (run `glab auth login`)"
            ))),
        }
    }

    // ---- encrypted file --------------------------------------------------------------------

    fn secret_file(&self) -> Result<&Arc<file::SecretFile>, KeltaError> {
        self.file.as_ref().ok_or_else(|| KeltaError::unsupported("no encrypted secrets file in this process"))
    }

    /// Unlock the encrypted secrets file for this run (`create`: make it if it does not exist).
    /// UI-only (Tauri IPC): kelta-ctl has no way to send a passphrase.
    pub async fn unlock(&self, passphrase: String, create: bool) -> Result<(), KeltaError> {
        let passphrase = zeroize::Zeroizing::new(passphrase);
        if passphrase.is_empty() {
            return Err(KeltaError::invalid("the passphrase is empty"));
        }
        let file = self.secret_file()?.clone();
        tokio::task::spawn_blocking(move || file.unlock(passphrase.as_bytes(), create))
            .await
            .map_err(|_| KeltaError::internal("unlock task failed"))??;
        tracing::info!(kind = "file", "encrypted secrets file unlocked");
        Ok(())
    }

    // ---- keyring ---------------------------------------------------------------------------

    fn credential_store(&self) -> Result<Arc<CredentialStore>, KeltaError> {
        let mut g = self.store.lock();
        if let Some(s) = &*g {
            return Ok(s.clone());
        }
        let s = platform_store()?;
        *g = Some(s.clone());
        Ok(s)
    }

    /// Run a blocking keyring operation off-thread under the lookup timeout.
    async fn keyring_op<T, F>(&self, name: &str, op: F) -> Result<T, KeltaError>
    where
        T: Send + 'static,
        F: FnOnce(keyring_core::Entry) -> keyring_core::Result<T> + Send + 'static,
    {
        let store = self.credential_store()?;
        let user = name.to_owned();
        let label = name.to_owned();
        let job = tokio::task::spawn_blocking(move || {
            let entry = store.build(KEYRING_SERVICE, &user, None)?;
            op(entry)
        });
        // one-shot: the Secret Service can block on a locked collection prompt
        match tokio::time::timeout(self.timeout, job).await {
            Err(_) => Err(KeltaError::timeout("secret storage did not answer within 5 s (is it locked?)")),
            Ok(Err(_)) => Err(KeltaError::internal("secret storage task failed")),
            Ok(Ok(Err(e))) => Err(map_keyring(&e, &label)),
            Ok(Ok(Ok(v))) => Ok(v),
        }
    }
}

/// Zero a `String`'s buffer before it is freed.
fn wipe_string(s: String) {
    let mut bytes = s.into_bytes();
    wipe(&mut bytes);
}

fn platform_store() -> Result<Arc<CredentialStore>, KeltaError> {
    #[cfg(target_os = "macos")]
    {
        let s =
            apple_native_keyring_store::keychain::Store::new().map_err(|e| map_keyring(&e, "keychain"))?;
        let s: Arc<CredentialStore> = s;
        Ok(s)
    }
    #[cfg(target_os = "linux")]
    {
        let s =
            zbus_secret_service_keyring_store::Store::new().map_err(|e| map_keyring(&e, "secret service"))?;
        let s: Arc<CredentialStore> = s;
        Ok(s)
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        Err(KeltaError::unsupported("no credential store on this platform"))
    }
}

#[cfg(target_os = "linux")]
async fn secret_service_probe() -> SecretBackendStatus {
    use zbus::Connection;
    const NAME: &str = "org.freedesktop.secrets";
    let backend = "secret-service".to_owned();
    let conn = match Connection::session().await {
        Ok(c) => c,
        Err(_) => {
            return SecretBackendStatus {
                backend,
                available: false,
                detail: Some("no D-Bus session bus".into()),
            };
        }
    };
    let owned = conn
        .call_method(
            Some("org.freedesktop.DBus"),
            "/org/freedesktop/DBus",
            Some("org.freedesktop.DBus"),
            "NameHasOwner",
            &(NAME,),
        )
        .await
        .ok()
        .and_then(|r| r.body().deserialize::<bool>().ok())
        .unwrap_or(false);
    if owned {
        return SecretBackendStatus {
            backend,
            available: true,
            detail: Some("org.freedesktop.secrets provider running".into()),
        };
    }
    let activatable = conn
        .call_method(
            Some("org.freedesktop.DBus"),
            "/org/freedesktop/DBus",
            Some("org.freedesktop.DBus"),
            "ListActivatableNames",
            &(),
        )
        .await
        .ok()
        .and_then(|r| r.body().deserialize::<Vec<String>>().ok())
        .is_some_and(|names| names.iter().any(|n| n == NAME));
    if activatable {
        SecretBackendStatus {
            backend,
            available: true,
            detail: Some("org.freedesktop.secrets provider will start on demand".into()),
        }
    } else {
        SecretBackendStatus {
            backend,
            available: false,
            detail: Some("no org.freedesktop.secrets provider (gnome-keyring, KeePassXC, KWallet)".into()),
        }
    }
}

#[async_trait]
impl SecretResolver for Secrets {
    async fn resolve(&self, r: &SecretRef, ctx: &SecretCtx) -> Result<Secret, KeltaError> {
        let src = r.parse().ok_or_else(|| {
            KeltaError::invalid(
                "invalid secret reference (use keyring:<name>, gh-cli, glab-cli, command:<argv> or env:<VAR>)",
            )
        })?;
        let key = Self::cache_key(r, ctx);
        if let Some(s) = self.cached(&key) {
            return Ok(s);
        }
        tracing::debug!(kind = ref_kind(&src), account = ?ctx.account, "resolving secret");
        let secret = match &src {
            SecretSource::Keyring(name) => {
                let v = self.keyring_op(name, |e| e.get_password()).await?;
                let s = Secret::new(v.as_str());
                wipe_string(v);
                s
            }
            SecretSource::File(name) => self.secret_file()?.get(name)?,
            SecretSource::GhCli => self.resolve_gh(ctx).await?,
            SecretSource::GlabCli => self.resolve_glab(ctx).await?,
            SecretSource::Command(argv) => self.resolve_command(argv).await?,
            SecretSource::Env(var) => self.resolve_env(var)?,
        };
        self.remember(key, &secret);
        Ok(secret)
    }

    async fn set(&self, r: &SecretRef, value: &str) -> Result<(), KeltaError> {
        if value.is_empty() {
            return Err(KeltaError::invalid("refusing to store an empty token"));
        }
        let kind = match r.parse() {
            Some(SecretSource::Keyring(name)) => {
                let value = value.to_owned();
                self.keyring_op(&name, move |e| e.set_password(&value)).await?;
                "keyring"
            }
            Some(SecretSource::File(name)) => {
                self.secret_file()?.update(&name, Some(value))?;
                "file"
            }
            _ => return Err(KeltaError::invalid("only keyring: and file: references can be written")),
        };
        self.invalidate(r);
        tracing::info!(kind, "secret stored");
        Ok(())
    }

    async fn delete(&self, r: &SecretRef) -> Result<(), KeltaError> {
        let name = match r.parse() {
            Some(SecretSource::Keyring(name)) => name,
            Some(SecretSource::File(name)) => {
                let res = self.secret_file().and_then(|f| f.update(&name, None));
                self.invalidate(r);
                return res;
            }
            _ => return Err(KeltaError::invalid("only keyring: and file: references can be deleted")),
        };
        let res = self
            .keyring_op(&name, |e| match e.delete_credential() {
                Err(keyring_core::Error::NoEntry) => Ok(()),
                other => other,
            })
            .await;
        self.invalidate(r);
        res
    }

    async fn backends_status(&self) -> Vec<SecretBackendStatus> {
        let mut out = Vec::new();
        #[cfg(target_os = "macos")]
        {
            let available = self.credential_store().is_ok();
            out.push(SecretBackendStatus {
                backend: "keychain".into(),
                available,
                detail: Some(if available {
                    "macOS Keychain".into()
                } else {
                    "Keychain is not accessible".into()
                }),
            });
        }
        #[cfg(target_os = "linux")]
        {
            out.push(secret_service_probe().await);
        }
        #[cfg(not(any(target_os = "macos", target_os = "linux")))]
        {
            let available = self.store.lock().is_some();
            out.push(SecretBackendStatus { backend: "keyring".into(), available, detail: None });
        }
        if let Some(f) = &self.file {
            out.push(f.status());
        }
        for (backend, program, hint) in [
            ("gh-cli", "gh", "install the GitHub CLI and run `gh auth login`"),
            ("glab-cli", "glab", "install the GitLab CLI and run `glab auth login`"),
        ] {
            let found = self.find_program(program);
            let (available, detail) = match found {
                Some(p) => (true, Some(p.display().to_string())),
                None => (false, Some(format!("`{program}` not found in PATH: {hint}"))),
            };
            out.push(SecretBackendStatus { backend: backend.into(), available, detail });
        }
        out.push(SecretBackendStatus {
            backend: "command".into(),
            available: true,
            detail: Some("any program that prints the token, e.g. `command:pass show jira/acme`".into()),
        });
        out.push(SecretBackendStatus {
            backend: "env".into(),
            available: true,
            detail: Some("`env:VAR` reads the token from the environment".into()),
        });
        out
    }

    fn invalidate(&self, r: &SecretRef) {
        let prefix = format!("{}\u{1}", r.as_str());
        self.cache.lock().map.retain(|k, _| !k.starts_with(&prefix));
    }
}

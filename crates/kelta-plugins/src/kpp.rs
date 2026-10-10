//! Process (KPP) provider plugins (PLUGINS §9): a plugin's `[provider]` program speaks JSON-RPC 2.0
//! over stdio, one JSON message per line, and serves `Tracker` / `CodeHost` accounts.
//!
//! Lifecycle: one process per plugin, spawned on the first call (never at startup), kept until the
//! plugin is disabled, uninstalled, re-granted or updated, or Kelta quits. No idle timer: an idle
//! provider costs its own RSS only. A crash fails the in-flight calls; the next call respawns it,
//! immediately after the first crash, then after 0.5, 1, 2 … 30 s (reset by a successful call).
//! A malformed or oversized message is a protocol violation: the process is killed like a crash.
//! So is a timeout during which the process sent nothing at all (hung); a slow call is just failed.
//! stderr goes to a 64 KiB ring (its tail is attached to crash errors).

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Weak};
use std::time::{Duration, Instant};

use async_trait::async_trait;
use kelta_http::{HttpCtx, ProviderFactory};
use kelta_proto::api::{CodeHost, SecretResolver, Tracker};
use kelta_proto::codehost::{CodeHostKind, PrCreate, Review, ReviewDetail, ReviewQuery, ReviewRef};
use kelta_proto::error::{ErrorCode, KeltaError};
use kelta_proto::ext::{Permission, ProviderDef, ProviderKind};
use kelta_proto::ids::{AccountId, PluginId};
use kelta_proto::secret::SecretCtx;
use kelta_proto::settings::{AccountConfig, AccountKind, TrackerBinding, TrackerView};
use kelta_proto::tracker::{
    Assignee, Column, Cursor, Page, Ticket, TicketDetail, TicketRef, TrackerCaps, TrackerKind, Transition,
    User,
};
use parking_lot::Mutex;
use serde::Deserialize as _;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::process::{ChildStdin, ChildStdout};
use tokio::sync::oneshot;

use crate::PluginHost;
use crate::util::{ByteRing, signal_group};

/// stderr ring per provider process.
pub const LOG_CAP: usize = 64 * 1024;
/// Largest message read from a provider.
pub const MAX_MESSAGE: usize = 8 << 20;
const DEFAULT_TIMEOUT: Duration = Duration::from_secs(30);
const BACKOFF_MAX: Duration = Duration::from_secs(30);

type Reply = oneshot::Sender<Result<Value, KeltaError>>;

struct Conn {
    pid: u32,
    stdin: tokio::sync::Mutex<ChildStdin>,
    pending: Mutex<HashMap<u64, Reply>>,
    /// Set (before the pending calls are failed) when stdout closed or the protocol broke.
    exited_at: Mutex<Option<Instant>>,
    /// When stdout last delivered a line: tells a hung process from a slow call.
    last_reply: Mutex<Option<Instant>>,
}

#[derive(Default)]
struct State {
    conn: Option<Arc<Conn>>,
    /// Earliest respawn after a crash.
    retry_at: Option<Instant>,
}

/// One provider process (spawned lazily, respawned after crashes).
pub struct KppProcess {
    sha256: String,
    program: PathBuf,
    args: Vec<String>,
    dir: PathBuf,
    path: Option<String>,
    timeout: Duration,
    log: Arc<Mutex<ByteRing>>,
    state: Mutex<State>,
    next_id: AtomicU64,
    crashes: AtomicU32,
    spawns: AtomicU32,
}

impl KppProcess {
    /// `dir` = plugin dir (cwd; base of a `command` with `/`), `path` = `PATH` for the program
    /// (the login shell's: a GUI app's own `PATH` misses nvm/brew), `sha256` = manifest hash.
    pub fn new(def: &ProviderDef, dir: &Path, sha256: &str, path: Option<String>) -> Self {
        let program =
            if def.command.contains('/') { dir.join(&def.command) } else { PathBuf::from(&def.command) };
        Self {
            sha256: sha256.to_owned(),
            program,
            args: def.args.clone(),
            dir: dir.to_owned(),
            path,
            timeout: def.timeout_ms.map_or(DEFAULT_TIMEOUT, Duration::from_millis),
            log: Arc::new(Mutex::new(ByteRing::new(LOG_CAP))),
            state: Mutex::new(State::default()),
            next_id: AtomicU64::new(1),
            crashes: AtomicU32::new(0),
            spawns: AtomicU32::new(0),
        }
    }

    pub fn sha256(&self) -> &str {
        &self.sha256
    }

    /// Pid of the running process.
    pub fn pid(&self) -> Option<u32> {
        let st = self.state.lock();
        st.conn.as_ref().filter(|c| c.exited_at.lock().is_none()).map(|c| c.pid)
    }

    /// Processes started so far.
    pub fn spawns(&self) -> u32 {
        self.spawns.load(Ordering::Relaxed)
    }

    pub fn log_tail(&self, lines: usize) -> String {
        self.log.lock().tail_lines(lines)
    }

    /// Kill the process group (disable / uninstall / quit). A later call spawns a fresh one.
    pub fn kill(&self) {
        let mut st = self.state.lock();
        if let Some(c) = st.conn.take() {
            signal_group(c.pid, rustix::process::Signal::KILL);
        }
        st.retry_at = None;
        self.crashes.store(0, Ordering::Relaxed);
    }

    fn conn(&self) -> Result<Arc<Conn>, KeltaError> {
        let mut st = self.state.lock();
        if let Some(c) = &st.conn {
            let exited = *c.exited_at.lock();
            let Some(at) = exited else { return Ok(c.clone()) };
            let n = self.crashes.fetch_add(1, Ordering::Relaxed) + 1;
            // first crash: respawn at once; then 0.5 s, 1 s, 2 s … 30 s
            let wait = if n <= 1 {
                Duration::ZERO
            } else {
                Duration::from_millis(500u64.saturating_mul(1 << (n - 2).min(16))).min(BACKOFF_MAX)
            };
            st.retry_at = Some(at + wait);
            st.conn = None;
        }
        if let Some(at) = st.retry_at {
            let now = Instant::now();
            if now < at {
                let ms = u64::try_from((at - now).as_millis()).unwrap_or(u64::MAX);
                let mut e = KeltaError::upstream(format!("provider crashed; restarting in {ms} ms"))
                    .with_detail(json!({ "log": self.log_tail(20) }));
                e.retry_after_ms = Some(ms);
                return Err(e);
            }
        }
        let c = self.spawn()?;
        st.conn = Some(c.clone());
        st.retry_at = None;
        Ok(c)
    }

    fn spawn(&self) -> Result<Arc<Conn>, KeltaError> {
        let mut cmd = tokio::process::Command::new(&self.program);
        // The provider gets secrets per call only: no inherited environment (env: secret refs).
        cmd.args(&self.args).current_dir(&self.dir).env_clear();
        for k in ["HOME", "USER", "LANG", "TMPDIR"] {
            if let Some(v) = std::env::var_os(k) {
                cmd.env(k, v);
            }
        }
        if let Some(p) = self.path.clone().or_else(|| std::env::var("PATH").ok()) {
            cmd.env("PATH", p);
        }
        cmd.stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .process_group(0)
            .kill_on_drop(true);
        let mut child = cmd.spawn().map_err(|e| {
            let what = self.program.display();
            if e.kind() == std::io::ErrorKind::NotFound {
                KeltaError::not_found(format!("provider program `{what}` not found"))
            } else {
                KeltaError::internal(format!("cannot start provider `{what}`: {e}"))
            }
        })?;
        let pid = child.id().ok_or_else(|| KeltaError::internal("provider exited immediately"))?;
        let (Some(stdin), Some(stdout)) = (child.stdin.take(), child.stdout.take()) else {
            return Err(KeltaError::internal("provider stdio not piped"));
        };
        if let Some(err) = child.stderr.take() {
            tokio::spawn(crate::web::pump(err, self.log.clone(), None));
        }
        tokio::spawn(async move {
            let _ = child.wait().await;
        });
        self.spawns.fetch_add(1, Ordering::Relaxed);
        let conn = Arc::new(Conn {
            pid,
            stdin: tokio::sync::Mutex::new(stdin),
            pending: Mutex::new(HashMap::new()),
            exited_at: Mutex::new(None),
            last_reply: Mutex::new(None),
        });
        tokio::spawn(read_loop(stdout, conn.clone(), self.log.clone()));
        Ok(conn)
    }

    /// One JSON-RPC call (`params` is an object), bounded by the per-call timeout.
    pub async fn call(&self, method: &str, params: Value) -> Result<Value, KeltaError> {
        let conn = self.conn()?;
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let (tx, rx) = oneshot::channel();
        conn.pending.lock().insert(id, tx);
        if conn.exited_at.lock().is_some() {
            conn.pending.lock().remove(&id);
            return Err(KeltaError::upstream("provider exited"));
        }
        let mut line =
            serde_json::to_vec(&json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params }))?;
        line.push(b'\n');
        let start = Instant::now();
        let exchange = async {
            let mut w = conn.stdin.lock().await;
            if let Err(e) = async {
                w.write_all(&line).await?;
                w.flush().await
            }
            .await
            {
                return Err(KeltaError::upstream(format!("provider stdin: {e}")));
            }
            drop(w);
            rx.await.unwrap_or_else(|_| Err(KeltaError::upstream("provider exited")))
        };
        // one-shot: per-call deadline armed by this call
        let out = match tokio::time::timeout(self.timeout, exchange).await {
            Ok(r) => r,
            Err(_) => {
                // silent for the whole call: hung, not slow. Kill it so the next call respawns
                // (crash backoff) instead of timing out forever.
                if conn.last_reply.lock().is_none_or(|t| t < start) {
                    signal_group(conn.pid, rustix::process::Signal::KILL);
                    conn.exited_at.lock().get_or_insert_with(Instant::now);
                }
                Err(KeltaError::timeout(format!(
                    "provider did not answer `{method}` within {} ms",
                    self.timeout.as_millis()
                )))
            }
        };
        conn.pending.lock().remove(&id);
        if out.is_ok() {
            self.crashes.store(0, Ordering::Relaxed);
        }
        out
    }
}

impl Drop for KppProcess {
    fn drop(&mut self) {
        self.kill();
    }
}

async fn read_loop(out: ChildStdout, conn: Arc<Conn>, log: Arc<Mutex<ByteRing>>) {
    let mut r = BufReader::new(out);
    let mut buf = Vec::new();
    let why = loop {
        buf.clear();
        match (&mut r).take(MAX_MESSAGE as u64 + 1).read_until(b'\n', &mut buf).await {
            Ok(0) => break "exited".to_owned(),
            Err(e) => break format!("stdout failed: {e}"),
            Ok(_) => {}
        }
        *conn.last_reply.lock() = Some(Instant::now());
        if buf.len() > MAX_MESSAGE {
            break format!("sent a message over {MAX_MESSAGE} bytes");
        }
        let line = buf.trim_ascii();
        if line.is_empty() {
            continue;
        }
        let msg: Value = match serde_json::from_slice(line) {
            Ok(v) => v,
            Err(e) => break format!("sent a malformed message ({e})"),
        };
        // notifications and unknown ids are ignored
        let Some(id) = msg.get("id").and_then(Value::as_u64) else { continue };
        let Some(tx) = conn.pending.lock().remove(&id) else { continue };
        let _ = tx.send(response(&msg));
    };
    signal_group(conn.pid, rustix::process::Signal::KILL);
    *conn.exited_at.lock() = Some(Instant::now());
    let err = KeltaError::upstream(format!("provider {why}"))
        .with_detail(json!({ "log": log.lock().tail_lines(20) }));
    for (_, tx) in conn.pending.lock().drain() {
        let _ = tx.send(Err(err.clone()));
    }
}

fn response(msg: &Value) -> Result<Value, KeltaError> {
    match msg.get("error") {
        Some(e) => Err(rpc_error(e)),
        None => Ok(msg.get("result").cloned().unwrap_or(Value::Null)),
    }
}

/// JSON-RPC error → `KeltaError`: `data.code` (an `ErrorCode`) wins, then `data.status` (an HTTP
/// status, mapped like the built-in providers: 401 → NeedsAuth), then -32601 → Unsupported.
fn rpc_error(e: &Value) -> KeltaError {
    let message: String =
        e.get("message").and_then(Value::as_str).unwrap_or("provider error").chars().take(300).collect();
    let data = e.get("data");
    if let Some(code) = data.and_then(|d| d.get("code")).and_then(|c| ErrorCode::deserialize(c).ok()) {
        return KeltaError::new(code, message);
    }
    if let Some(status) = data.and_then(|d| d.get("status")).and_then(Value::as_u64) {
        return kelta_http::status_error(u16::try_from(status).unwrap_or(500), &BTreeMap::new(), &message);
    }
    if e.get("code").and_then(Value::as_i64) == Some(-32601) {
        return KeltaError::unsupported(message);
    }
    KeltaError::upstream(message)
}

// ---- typed adapters ----------------------------------------------------------------------------

/// Where an adapter gets its process: straight (`kpp-check`) or through the host, which checks
/// that the plugin is enabled and holds the `provider` grant on every call.
#[derive(Clone)]
pub enum Source {
    Direct(Arc<KppProcess>),
    Host(Weak<PluginHost>, PluginId),
}

/// Per-account call context: the account config and its secret go with every call.
struct Rpc {
    source: Source,
    account_id: AccountId,
    account: AccountConfig,
    secrets: Arc<dyn SecretResolver>,
}

impl Rpc {
    async fn call<T: DeserializeOwned>(&self, method: &str, params: Value) -> Result<T, KeltaError> {
        let proc_ = match &self.source {
            Source::Direct(p) => p.clone(),
            Source::Host(host, id) => {
                let host = host.upgrade().ok_or_else(|| KeltaError::internal("plugin host is gone"))?;
                host.kpp_process(id).await?
            }
        };
        let secret_ref = self.account.effective_secret();
        let secret = match &secret_ref {
            Some(r) => {
                let ctx = SecretCtx {
                    account: Some(self.account_id.clone()),
                    host: self.account.base_url.as_deref().and_then(kelta_http::util::url_host),
                };
                Some(self.secrets.resolve(r, &ctx).await?)
            }
            None => None,
        };
        let mut params = params;
        params["account_id"] = json!(self.account_id);
        params["account"] = serde_json::to_value(&self.account)?;
        params["secret"] = json!(secret.as_ref().map(|s| s.expose()));
        let v = proc_.call(method, params).await.inspect_err(|e| {
            if e.code == ErrorCode::NeedsAuth
                && let Some(r) = &secret_ref
            {
                self.secrets.invalidate(r);
            }
        })?;
        serde_json::from_value(v)
            .map_err(|e| KeltaError::upstream(format!("provider `{method}`: bad result: {e}")))
    }

    /// The plugin cannot speak for another account.
    fn own_ticket(&self, mut t: Ticket) -> Ticket {
        t.r#ref.account = self.account_id.clone();
        t
    }

    fn own_review(&self, mut r: Review) -> Review {
        r.r#ref.account = self.account_id.clone();
        r
    }

    fn expand(&self, tpl: &str, vars: &[(&str, &str)]) -> String {
        let base = self.account.base_url.as_deref().unwrap_or("").trim_end_matches('/');
        let web = self.account.web_url.as_deref().map_or(base, |w| w.trim_end_matches('/'));
        let mut s = tpl.replace("{base_url}", base).replace("{web_url}", web);
        for (k, v) in vars {
            s = s.replace(&format!("{{{k}}}"), v);
        }
        s
    }
}

pub struct KppTracker {
    rpc: Rpc,
    def: ProviderDef,
}

impl KppTracker {
    pub fn new(
        source: Source,
        def: ProviderDef,
        account_id: AccountId,
        account: AccountConfig,
        secrets: Arc<dyn SecretResolver>,
    ) -> Self {
        Self { rpc: Rpc { source, account_id, account, secrets }, def }
    }
}

#[async_trait]
impl Tracker for KppTracker {
    fn kind(&self) -> TrackerKind {
        TrackerKind::Plugin
    }
    fn caps(&self) -> TrackerCaps {
        self.def.caps.clone()
    }
    async fn me(&self) -> Result<User, KeltaError> {
        self.rpc.call("tracker.me", json!({})).await
    }
    async fn list(&self, view: &TrackerView, cursor: Option<Cursor>) -> Result<Page<Ticket>, KeltaError> {
        let mut p: Page<Ticket> =
            self.rpc.call("tracker.list", json!({ "view": view, "cursor": cursor })).await?;
        p.items = p.items.into_iter().map(|t| self.rpc.own_ticket(t)).collect();
        Ok(p)
    }
    async fn get(&self, t: &TicketRef) -> Result<TicketDetail, KeltaError> {
        let mut d: TicketDetail = self.rpc.call("tracker.get", json!({ "ticket": t })).await?;
        d.ticket = self.rpc.own_ticket(d.ticket);
        if let Some(p) = &mut d.parent {
            p.account = self.rpc.account_id.clone();
        }
        for c in &mut d.children {
            c.ticket = self.rpc.own_ticket(std::mem::take(&mut c.ticket));
        }
        d.body_html = if d.body_html.is_empty() {
            kelta_http::markdown::to_html(&d.body_md)
        } else {
            kelta_http::markdown::sanitize(&d.body_html)
        };
        d.comments.truncate(20);
        for c in &mut d.comments {
            c.body_html = kelta_http::markdown::sanitize(&c.body_html);
        }
        Ok(d)
    }
    async fn columns(&self, b: &TrackerBinding) -> Result<Vec<Column>, KeltaError> {
        self.rpc.call("tracker.columns", json!({ "binding": b })).await
    }
    async fn transitions(&self, t: &TicketRef) -> Result<Vec<Transition>, KeltaError> {
        self.rpc.call("tracker.transitions", json!({ "ticket": t })).await
    }
    async fn transition(
        &self,
        t: &TicketRef,
        transition_id: &str,
        fields: Option<Value>,
    ) -> Result<Ticket, KeltaError> {
        let params = json!({ "ticket": t, "transition_id": transition_id, "fields": fields });
        Ok(self.rpc.own_ticket(self.rpc.call("tracker.transition", params).await?))
    }
    async fn comment(&self, t: &TicketRef, markdown: &str) -> Result<(), KeltaError> {
        if !self.def.caps.comment {
            return Err(KeltaError::unsupported("this tracker plugin does not comment"));
        }
        let _: Value = self.rpc.call("tracker.comment", json!({ "ticket": t, "markdown": markdown })).await?;
        Ok(())
    }
    async fn assign(&self, t: &TicketRef, who: Assignee) -> Result<Ticket, KeltaError> {
        if !self.def.caps.assign {
            return Err(KeltaError::unsupported("this tracker plugin does not assign"));
        }
        Ok(self.rpc.own_ticket(self.rpc.call("tracker.assign", json!({ "ticket": t, "who": who })).await?))
    }
    fn browser_url(&self, t: &TicketRef) -> String {
        self.rpc.expand(
            self.def.browser_url.as_deref().unwrap_or("{web_url}/{key}"),
            &[("key", &t.key), ("id", &t.id)],
        )
    }
    fn branch_key(&self, t: &TicketRef) -> String {
        self.rpc.expand(self.def.branch_key.as_deref().unwrap_or("{key}"), &[("key", &t.key), ("id", &t.id)])
    }
}

pub struct KppCodeHost {
    rpc: Rpc,
    def: ProviderDef,
}

impl KppCodeHost {
    pub fn new(
        source: Source,
        def: ProviderDef,
        account_id: AccountId,
        account: AccountConfig,
        secrets: Arc<dyn SecretResolver>,
    ) -> Self {
        Self { rpc: Rpc { source, account_id, account, secrets }, def }
    }
}

#[async_trait]
impl CodeHost for KppCodeHost {
    fn kind(&self) -> CodeHostKind {
        CodeHostKind::Plugin
    }
    async fn me(&self) -> Result<User, KeltaError> {
        self.rpc.call("codehost.me", json!({})).await
    }
    async fn changed_since_last(&self) -> Result<bool, KeltaError> {
        match self.rpc.call("codehost.changed_since_last", json!({})).await {
            Err(e) if e.code == ErrorCode::Unsupported => Ok(true),
            r => r,
        }
    }
    async fn list_reviews(&self, q: &ReviewQuery) -> Result<Vec<Review>, KeltaError> {
        let list: Vec<Review> = self.rpc.call("codehost.list_reviews", json!({ "query": q })).await?;
        Ok(list.into_iter().map(|r| self.rpc.own_review(r)).collect())
    }
    async fn get(&self, r: &ReviewRef) -> Result<ReviewDetail, KeltaError> {
        let mut d: ReviewDetail = self.rpc.call("codehost.get", json!({ "review": r })).await?;
        d.review = self.rpc.own_review(d.review);
        d.body_html = kelta_http::markdown::sanitize(&d.body_html);
        Ok(d)
    }
    async fn approve(&self, r: &ReviewRef, head_sha: &str) -> Result<(), KeltaError> {
        let _: Value =
            self.rpc.call("codehost.approve", json!({ "review": r, "head_sha": head_sha })).await?;
        Ok(())
    }
    async fn comment(&self, r: &ReviewRef, body: &str) -> Result<(), KeltaError> {
        let _: Value = self.rpc.call("codehost.comment", json!({ "review": r, "body": body })).await?;
        Ok(())
    }
    async fn request_changes(&self, r: &ReviewRef, body: &str) -> Result<(), KeltaError> {
        let _: Value =
            self.rpc.call("codehost.request_changes", json!({ "review": r, "body": body })).await?;
        Ok(())
    }
    async fn add_pending_comment(
        &self,
        r: &ReviewRef,
        path: &str,
        line: u32,
        body: &str,
    ) -> Result<(), KeltaError> {
        let params = json!({ "review": r, "path": path, "line": line, "body": body });
        let _: Value = self.rpc.call("codehost.add_pending_comment", params).await?;
        Ok(())
    }
    async fn create(&self, d: &PrCreate) -> Result<Review, KeltaError> {
        Ok(self.rpc.own_review(self.rpc.call("codehost.create", json!({ "pr": d })).await?))
    }
    async fn find_for_branch(&self, repo: &str, branch: &str) -> Result<Option<Review>, KeltaError> {
        let r: Option<Review> =
            self.rpc.call("codehost.find_for_branch", json!({ "repo": repo, "branch": branch })).await?;
        Ok(r.map(|r| self.rpc.own_review(r)))
    }
    fn fetch_refspec(&self, r: &ReviewRef, local_branch: &str) -> String {
        let n = r.number.to_string();
        let tpl = self.def.fetch_refspec.as_deref().unwrap_or("pull/{number}/head:{branch}");
        self.rpc.expand(tpl, &[("number", &n), ("repo", &r.repo), ("branch", local_branch)])
    }
    fn repo_from_remote(&self, _url: &str) -> Option<String> {
        // shortcut: plugin code hosts map no remote to a repo (no core caller in v0.1), add a
        // `[provider]` template or a call when remote detection needs it.
        None
    }
}

// ---- factory ------------------------------------------------------------------------------------

/// Wraps the built-in factory: `plugin_tracker` / `plugin_codehost` accounts resolve to the
/// plugin named by `account.plugin`, everything else goes to `inner`.
pub struct KppFactory {
    inner: Arc<dyn ProviderFactory>,
    host: Weak<PluginHost>,
}

impl KppFactory {
    pub fn new(inner: Arc<dyn ProviderFactory>, host: Weak<PluginHost>) -> Self {
        Self { inner, host }
    }

    fn provider(
        &self,
        account: &AccountConfig,
        kind: ProviderKind,
    ) -> Result<(PluginId, ProviderDef), KeltaError> {
        let id = account
            .plugin
            .as_deref()
            .ok_or_else(|| KeltaError::invalid("plugin account without `plugin = \"<plugin id>\"`"))?;
        let host = self.host.upgrade().ok_or_else(|| KeltaError::internal("plugin host is gone"))?;
        let reg = host.registry();
        let e =
            reg.get(id).ok_or_else(|| KeltaError::not_found(format!("plugin `{id}` is not installed")))?;
        // shortcut: the templates are read when the account's provider is built; a plugin update
        // reaches them on the next rebuild (account change or 30 min idle).
        let def =
            e.manifest().and_then(|m| m.provider.clone()).filter(|d| d.kind == kind).ok_or_else(|| {
                KeltaError::invalid(format!("plugin `{id}` has no {kind:?} provider").to_lowercase())
            })?;
        Ok((e.id.clone(), def))
    }
}

impl ProviderFactory for KppFactory {
    fn tracker(
        &self,
        account: &AccountConfig,
        http: HttpCtx,
        secrets: Arc<dyn SecretResolver>,
    ) -> Result<Arc<dyn Tracker>, KeltaError> {
        if account.kind != AccountKind::PluginTracker {
            return self.inner.tracker(account, http, secrets);
        }
        let (id, def) = self.provider(account, ProviderKind::Tracker)?;
        let source = Source::Host(self.host.clone(), id);
        Ok(Arc::new(KppTracker::new(source, def, http.account_id().clone(), account.clone(), secrets)))
    }

    fn code_host(
        &self,
        account: &AccountConfig,
        http: HttpCtx,
        secrets: Arc<dyn SecretResolver>,
    ) -> Result<Arc<dyn CodeHost>, KeltaError> {
        if account.kind != AccountKind::PluginCodehost {
            return self.inner.code_host(account, http, secrets);
        }
        let (id, def) = self.provider(account, ProviderKind::Codehost)?;
        let source = Source::Host(self.host.clone(), id);
        Ok(Arc::new(KppCodeHost::new(source, def, http.account_id().clone(), account.clone(), secrets)))
    }
}

impl PluginHost {
    /// The provider process of an enabled plugin holding the `provider` grant (created lazily;
    /// replaced when the manifest changed).
    pub async fn kpp_process(&self, id: &PluginId) -> Result<Arc<KppProcess>, KeltaError> {
        let e = self.active_entry(id.as_str())?;
        self.granted(&e).await?.require(Permission::Provider)?;
        let def = e
            .manifest()
            .and_then(|m| m.provider.clone())
            .ok_or_else(|| KeltaError::invalid(format!("plugin `{id}` has no [provider]")))?;
        let mut map = self.kpp.lock();
        if let Some(p) = map.get(id)
            && p.sha256() == e.sha256()
        {
            return Ok(p.clone());
        }
        let path = self.core().and_then(|c| c.login_path());
        let p = Arc::new(KppProcess::new(&def, &e.dir, e.sha256(), path));
        if let Some(old) = map.insert(id.clone(), p.clone()) {
            old.kill();
        }
        Ok(p)
    }

    /// Kill a plugin's provider process, if any.
    pub(crate) fn kpp_stop(&self, id: &PluginId) {
        if let Some(p) = self.kpp.lock().remove(id) {
            p.kill();
        }
    }
}

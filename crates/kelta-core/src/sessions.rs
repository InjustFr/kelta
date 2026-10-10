//! SessionRegistry (ARCHITECTURE §7.2, §7.5, §7.6): SpawnRequest → PtySpawnSpec, Dormant/lazy
//! restore on attach, restore policies, quit flow, hooks-inactive one-shot + heuristic status,
//! status/attention from `TerminalEvents` and hooks, OSC 52 policy, `seen` handling.

use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Weak};
use std::time::{Duration, Instant};

use kelta_proto::api::{FrameSink, TerminalEvents};
use kelta_proto::error::{ErrorCode, KeltaError};
use kelta_proto::events::{BusEvent, Notification, Toast, UiEvent, bus};
use kelta_proto::ext::Urgency;
use kelta_proto::ids::{ProjectId, SessionId, WorkItemId};
use kelta_proto::model::{
    AttachInfo, Attention, ClaudeMeta, CloseOnExit, EditorMeta, Lifecycle, RestorePolicy, SessionInfo,
    SessionKind, SessionStatus, SpawnRequest, StatusChange, StatusSource,
};
use kelta_proto::settings::{BellMode, HookTransport, Osc52, RestoreMode, SessionHost};
use kelta_proto::store::SessionRow;
use kelta_proto::term::{ClipboardKind, HistoryHit, KillSignal, PtySpawnSpec, TerminalEvent};
use kelta_proto::tracker::TicketRef;

use crate::Core;
use crate::projects::repo_path;
use crate::rt::OneShot;
use crate::spawn_env::{self, EnvInputs};
use crate::status::{self, Decision, Heuristic, NotifyKind, SessState};
use crate::store::q;

/// No `SessionStart` within this delay → hooks inactive, heuristic status.
pub const HOOKS_GRACE: Duration = Duration::from_secs(10);
/// Heuristic: quiet after output → Done.
pub const QUIET_AFTER_OUTPUT: Duration = Duration::from_secs(3);
/// Quit: SIGHUP → this grace → SIGKILL.
pub const QUIT_GRACE: Duration = Duration::from_secs(2);
/// A `--resume` Claude exiting non-zero this fast is retried with `--continue`.
pub const RESUME_FAIL_WINDOW: Duration = Duration::from_secs(5);
/// Persisted text tail (lines).
pub const TEXT_TAIL_LINES: u32 = 200;

#[derive(Default)]
pub struct SessionTimers {
    pub hooks: OneShot,
    pub quiet: OneShot,
}

impl SessionTimers {
    fn cancel(&self) {
        self.hooks.cancel();
        self.quiet.cancel();
    }
}

pub struct SessionEntry {
    pub info: SessionInfo,
    /// The original request (resolved cwd), replayed by restore policies.
    pub spec: SpawnRequest,
    pub restore: RestorePolicy,
    pub hook_token: String,
    pub mcp_token: Option<String>,
    pub ticket: Option<TicketRef>,
    /// Persisted text tail (Dormant sessions).
    pub text_tail: Option<String>,
    pub spawned_at: Option<Instant>,
    /// Spawned with `--resume` (fallback to `--continue` on a quick failure).
    pub resume_attempt: bool,
    pub kill_requested: bool,
    pub restarting: bool,
    /// Live-claimed by `restore_session` but the PTY may not exist yet; attach waits on `exits`.
    pub restoring: bool,
    pub http_ref: bool,
    /// Hooks inactive → heuristic status.
    pub heuristic: bool,
    pub timers: Arc<SessionTimers>,
}

impl SessionEntry {
    fn state(&self, visible: bool) -> SessState {
        SessState { status: self.info.status, attention: self.info.attention, seen: self.info.seen, visible }
    }

    fn hooks_active(&self) -> bool {
        self.info.claude.as_ref().is_some_and(|c| c.hooks_active)
    }

    fn row(&self, lifecycle: &str) -> Result<SessionRow, KeltaError> {
        Ok(SessionRow {
            id: self.info.id.clone(),
            project_id: self.info.project_id.clone(),
            kind_json: serde_json::to_string(&self.info.kind)?,
            spec_json: serde_json::to_string(&self.spec)?,
            name: self.info.name.clone(),
            work_item_id: self.info.work_item_id.clone(),
            restore_json: serde_json::to_string(&self.restore)?,
            cwd: self.info.cwd.clone(),
            lifecycle: lifecycle.to_owned(),
            text_tail: self.text_tail.clone(),
            updated_at: kelta_proto::now_rfc3339(),
        })
    }
}

fn lifecycle_str(l: Lifecycle) -> &'static str {
    match l {
        Lifecycle::Dormant => "dormant",
        Lifecycle::Live => "live",
        Lifecycle::Exited => "exited",
    }
}

/// Forwards `TerminalEvents` from reader threads to core.
struct EventSink {
    core: Weak<Core>,
}

impl TerminalEvents for EventSink {
    fn on_event(&self, id: &SessionId, ev: TerminalEvent) {
        if let Some(core) = self.core.upgrade() {
            core.on_terminal_event(id, ev);
        }
    }
}

/// What to exec (restore policies rewrite the original request).
pub struct Launch {
    pub program: Option<String>,
    pub args: Vec<String>,
    pub resume_attempt: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpawnMode {
    New,
    Restore,
    Restart,
}

/// Default restore policy when the request says `None` (ARCHITECTURE §7.5 "Next start").
pub fn default_restore(
    kind: &SessionKind,
    requested: &RestorePolicy,
    claude_uuid: Option<&str>,
    nvim_session: PathBuf,
) -> RestorePolicy {
    if *requested != RestorePolicy::None {
        return requested.clone();
    }
    match kind {
        SessionKind::Shell => RestorePolicy::ShellInCwd,
        SessionKind::Claude => match claude_uuid {
            Some(u) => RestorePolicy::ClaudeResume { uuid: u.to_owned() },
            None => RestorePolicy::Relaunch,
        },
        SessionKind::Editor { adapter } if adapter == "nvim" => {
            RestorePolicy::Editor { session_file: Some(nvim_session) }
        }
        SessionKind::Editor { .. } | SessionKind::Tool { .. } => RestorePolicy::Relaunch,
        SessionKind::Setup | SessionKind::Custom => RestorePolicy::None,
    }
}

/// Launch for a restore policy.
pub fn restore_launch(
    spec: &SpawnRequest,
    policy: &RestorePolicy,
    claude_binary: &str,
    use_continue: bool,
) -> Launch {
    let exists = |p: &Path| p.exists();
    match policy {
        RestorePolicy::ShellInCwd => Launch { program: None, args: Vec::new(), resume_attempt: false },
        RestorePolicy::ClaudeResume { uuid } => Launch {
            program: Some(spec.program.clone().unwrap_or_else(|| claude_binary.to_owned())),
            args: spawn_env::claude_restore_args(&spec.args, uuid, use_continue, &exists),
            resume_attempt: !use_continue,
        },
        RestorePolicy::Editor { session_file } => Launch {
            program: spec.program.clone(),
            args: spawn_env::editor_restore_args(&spec.args, session_file.as_deref(), &exists),
            resume_attempt: false,
        },
        RestorePolicy::Relaunch | RestorePolicy::None => {
            Launch { program: spec.program.clone(), args: spec.args.clone(), resume_attempt: false }
        }
    }
}

fn default_name(kind: &SessionKind, program: Option<&str>, shell: &Path) -> String {
    match kind {
        SessionKind::Shell => match program {
            Some(p) => Path::new(p)
                .file_name()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_else(|| p.to_owned()),
            None => {
                shell.file_name().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "shell".into())
            }
        },
        SessionKind::Claude => "claude".into(),
        SessionKind::Editor { adapter } => adapter.clone(),
        SessionKind::Tool { tool_id } => tool_id.to_string(),
        SessionKind::Setup => "setup".into(),
        SessionKind::Custom => program
            .and_then(|p| Path::new(p).file_name().map(|s| s.to_string_lossy().into_owned()))
            .unwrap_or_else(|| "session".into()),
    }
}

/// OSC 52 reply: `ESC ] 52 ; c|p ; <base64> BEL`.
pub fn osc52_reply(kind: ClipboardKind, text: &str) -> Vec<u8> {
    let sel = if kind == ClipboardKind::Primary { 'p' } else { 'c' };
    format!("\x1b]52;{sel};{}\x07", base64(text.as_bytes())).into_bytes()
}

fn base64(input: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(input.len().div_ceil(3) * 4);
    for chunk in input.chunks(3) {
        let b = [chunk[0], chunk.get(1).copied().unwrap_or(0), chunk.get(2).copied().unwrap_or(0)];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        out.push(T[(n >> 18) as usize & 63] as char);
        out.push(T[(n >> 12) as usize & 63] as char);
        out.push(if chunk.len() > 1 { T[(n >> 6) as usize & 63] as char } else { '=' });
        out.push(if chunk.len() > 2 { T[n as usize & 63] as char } else { '=' });
    }
    out
}

fn tail_lines(text: &str, max: u32) -> String {
    let lines: Vec<&str> = text.lines().collect();
    lines[lines.len().saturating_sub(max as usize)..].join("\n")
}

impl Core {
    // =========================================================================================
    // Startup / persistence
    // =========================================================================================

    pub(crate) fn load_sessions(&self) -> Result<(), KeltaError> {
        let rows = self.store.call_blocking(|c| q::sessions(c))?;
        let mode = self.cfg.effective(None).app.restore_mode;
        let mut kept: BTreeMap<SessionId, SessionEntry> = BTreeMap::new();
        for row in rows {
            let restore: RestorePolicy = serde_json::from_str(&row.restore_json).unwrap_or_default();
            let spec: Option<SpawnRequest> = serde_json::from_str(&row.spec_json).ok();
            let kind: Option<SessionKind> = serde_json::from_str(&row.kind_json).ok();
            let (Some(spec), Some(kind)) = (spec, kind) else {
                self.delete_row(&row.id);
                continue;
            };
            if restore == RestorePolicy::None
                || mode == RestoreMode::None
                || row.lifecycle == "exited"
                || !self.project_exists(&row.project_id)
            {
                self.delete_row(&row.id);
                continue;
            }
            let claude = matches!(kind, SessionKind::Claude).then(|| ClaudeMeta {
                session_uuid: match &restore {
                    RestorePolicy::ClaudeResume { uuid } => uuid.clone(),
                    _ => String::new(),
                },
                model: spawn_env::arg_value(&spec.args, "--model"),
                ..ClaudeMeta::default()
            });
            let editor = match &kind {
                SessionKind::Editor { adapter } => Some(EditorMeta {
                    adapter: adapter.clone(),
                    socket: spawn_env::arg_value(&spec.args, "--listen").map(PathBuf::from),
                }),
                _ => None,
            };
            let info = SessionInfo {
                id: row.id.clone(),
                project_id: row.project_id.clone(),
                kind,
                name: row.name.clone(),
                title: None,
                cwd: row.cwd.clone(),
                status: SessionStatus::Unknown,
                status_source: StatusSource::None,
                attention: Attention::None,
                seen: true,
                lifecycle: Lifecycle::Dormant,
                pid: None,
                exit_code: None,
                work_item_id: row.work_item_id.clone(),
                claude,
                editor,
                cols: spec.cols,
                rows: spec.rows,
                created_at: row.updated_at.clone(),
            };
            let entry = SessionEntry {
                info,
                spec,
                restore,
                hook_token: spawn_env::token(),
                mcp_token: None,
                ticket: None,
                text_tail: row.text_tail.clone(),
                spawned_at: None,
                resume_attempt: false,
                kill_requested: false,
                restarting: false,
                restoring: false,
                http_ref: false,
                heuristic: false,
                timers: Arc::new(SessionTimers::default()),
            };
            if let Ok(r) = entry.row("dormant") {
                self.store.exec("session_dormant", move |c| q::session_put(c, &r));
            }
            kept.insert(row.id, entry);
        }
        let ids: HashSet<SessionId> = kept.keys().cloned().collect();
        *self.sessions.lock() = kept;
        self.prune_layouts(&ids);
        Ok(())
    }

    /// keltad: sessions that kept running while the app was closed come back Live (same process,
    /// same hook token) instead of respawning from Dormant; what keltad holds that core does not
    /// know any more is killed.
    pub(crate) fn adopt_live_sessions(&self) {
        if !self.terminal.persistent() {
            return;
        }
        let ids: Vec<SessionId> = self.sessions.lock().keys().cloned().collect();
        let mut adopted = HashSet::new();
        let mut failed = false;
        for id in ids {
            let env = match self.terminal.adopt(&id, Arc::new(EventSink { core: self.me.clone() })) {
                Ok(Some(env)) => env,
                Ok(None) => continue,
                Err(e) => {
                    tracing::warn!(error = %e, session = %id, "keltad adopt failed");
                    failed = true;
                    continue;
                }
            };
            let hook = env.get("KELTA_HOOK_TOKEN").cloned().unwrap_or_default();
            // shortcut: KELTA_MCP_URL keeps the previous run's port, so MCP / http hooks of an adopted Claude fail until it restarts; pin the port to fix.
            let mcp = env.get("KELTA_MCP_TOKEN").cloned();
            if let Some(e) = self.sessions.lock().get_mut(&id) {
                e.info.lifecycle = Lifecycle::Live;
                e.hook_token = hook.clone();
                e.mcp_token = mcp.clone();
            }
            self.server.register_session(&id, &hook, mcp.as_deref());
            self.persist_session(&id);
            adopted.insert(id);
        }
        // A failed adopt may be a slow reply for a session core keeps: never kill on a guess.
        if failed {
            return;
        }
        for s in self.terminal.stats().sessions {
            if !adopted.contains(&s.id) {
                let _ = self.terminal.kill(&s.id, KillSignal::Kill);
            }
        }
    }

    fn delete_row(&self, id: &SessionId) {
        self.terminal.history_delete(id);
        let id = id.clone();
        self.store.exec("session_delete", move |c| q::session_delete(c, &id));
    }

    pub(crate) fn persist_session(&self, id: &SessionId) {
        let row = {
            let s = self.sessions.lock();
            s.get(id).map(|e| e.row(lifecycle_str(e.info.lifecycle)))
        };
        match row {
            Some(Ok(r)) => self.store.exec("session_put", move |c| q::session_put(c, &r)),
            Some(Err(e)) => tracing::warn!(error = %e, "cannot serialize session"),
            None => {}
        }
    }

    /// `restore_mode = eager`: spawn every Dormant session at startup.
    pub(crate) async fn eager_restore(&self) {
        if self.cfg.effective(None).app.restore_mode != RestoreMode::Eager {
            return;
        }
        let dormant: Vec<(SessionId, u16, u16)> = self
            .sessions
            .lock()
            .values()
            .filter(|e| e.info.lifecycle == Lifecycle::Dormant)
            .map(|e| (e.info.id.clone(), e.info.cols, e.info.rows))
            .collect();
        for (id, cols, rows) in dormant {
            if let Err(e) = self.restore_session(&id, cols, rows, false).await {
                tracing::warn!(error = %e, session = %id, "eager restore failed");
            }
        }
    }

    // =========================================================================================
    // Queries
    // =========================================================================================

    pub fn list_sessions(&self, project: Option<&ProjectId>) -> Vec<SessionInfo> {
        let mut v: Vec<SessionInfo> = self
            .sessions
            .lock()
            .values()
            .filter(|e| project.is_none_or(|p| &e.info.project_id == p))
            .map(|e| e.info.clone())
            .collect();
        v.sort_by(|a, b| a.created_at.cmp(&b.created_at).then_with(|| a.id.cmp(&b.id)));
        v
    }

    fn get_info(&self, id: &SessionId) -> Result<SessionInfo, KeltaError> {
        self.sessions
            .lock()
            .get(id)
            .map(|e| e.info.clone())
            .ok_or_else(|| KeltaError::not_found(format!("session {id}")))
    }

    /// Sessions on screen: active project's active tab, window visible.
    pub fn visible_sessions(&self) -> HashSet<SessionId> {
        let w = self.bridge.window_state();
        if !(w.exists && w.visible) {
            return HashSet::new();
        }
        let active = self.active_project();
        self.layouts
            .lock()
            .get(&active)
            .map(|l| crate::layout::visible_sessions(l).into_iter().collect())
            .unwrap_or_default()
    }

    pub(crate) fn is_visible(&self, id: &SessionId) -> bool {
        self.visible_sessions().contains(id)
    }

    /// Any Claude session Working/NeedsInput (quit confirmation, `app.confirm_quit_with_running`).
    pub fn quit_needs_confirm(&self) -> bool {
        let cfg = self.cfg.effective(None);
        // Same rule as quit_flow: keltad keeps restorable sessions running, the rest are killed.
        let survives = self.quit_persistent() && cfg.app.restore_mode != RestoreMode::None;
        cfg.app.confirm_quit_with_running
            && self.sessions.lock().values().any(|e| {
                !(survives && e.restore != RestorePolicy::None)
                    && matches!(e.info.kind, SessionKind::Claude)
                    && e.info.lifecycle == Lifecycle::Live
                    && matches!(e.info.status, SessionStatus::Working | SessionStatus::NeedsInput)
            })
    }

    // =========================================================================================
    // Spawn
    // =========================================================================================

    /// Reserves `id`'s sid8 under one lock so concurrent spawns / templates never share a runtime
    /// dir. False when the id or its sid8 is already in use.
    fn reserve_session_id(&self, id: &SessionId) -> bool {
        let mut taken = self.sid8_taken.lock();
        let sid8 = id.sid8();
        if taken.contains(&sid8) || self.sessions.lock().keys().any(|k| k == id || k.sid8() == sid8) {
            return false;
        }
        taken.insert(sid8);
        true
    }

    fn new_session_id(&self) -> SessionId {
        loop {
            let id = SessionId::generate();
            if self.reserve_session_id(&id) {
                return id;
            }
        }
    }

    pub(crate) async fn spawn_session(&self, req: SpawnRequest) -> Result<SessionInfo, KeltaError> {
        let id = match req.id.clone() {
            Some(id) if uuid::Uuid::parse_str(id.as_str()).is_err() => {
                return Err(KeltaError::invalid(format!("session id {id} is not a uuid")));
            }
            Some(id) if !self.reserve_session_id(&id) => {
                return Err(KeltaError::conflict(format!("session id {id} is already in use")));
            }
            Some(id) => id,
            None => self.new_session_id(),
        };
        let launch = Launch { program: req.program.clone(), args: req.args.clone(), resume_attempt: false };
        let res = self.spawn_with(id.clone(), req, launch, SpawnMode::New).await;
        // A failed spawn frees its sid8 so the caller can retry with the same id.
        let unused = res.is_err() && !self.sessions.lock().contains_key(&id);
        if unused {
            self.sid8_taken.lock().remove(&id.sid8());
        }
        res
    }

    /// Spawn with a pre-chosen id (templates write per-session files first).
    pub(crate) async fn spawn_session_with_id(
        &self,
        id: SessionId,
        req: SpawnRequest,
    ) -> Result<SessionInfo, KeltaError> {
        let launch = Launch { program: req.program.clone(), args: req.args.clone(), resume_attempt: false };
        self.spawn_with(id, req, launch, SpawnMode::New).await
    }

    pub(crate) fn fresh_session_id(&self) -> SessionId {
        self.new_session_id()
    }

    async fn ticket_key(&self, ticket: Option<&TicketRef>, work_item: Option<&WorkItemId>) -> Option<String> {
        if let Some(t) = ticket {
            return Some(t.key.clone());
        }
        let w = work_item?;
        let w = w.clone();
        self.store
            .call(move |c| q::work_get(c, &w))
            .await
            .ok()
            .flatten()
            .and_then(|w| w.ticket)
            .map(|t| t.key)
    }

    pub(crate) async fn spawn_with(
        &self,
        id: SessionId,
        mut spec: SpawnRequest,
        mut launch: Launch,
        mode: SpawnMode,
    ) -> Result<SessionInfo, KeltaError> {
        if !self.project_exists(&spec.project_id) {
            return Err(KeltaError::not_found(format!("project {}", spec.project_id)));
        }
        let settings = self.cfg.effective(Some(&spec.project_id));
        let home = self.home_dir();
        let default_cwd = self.project_cwd(&spec.project_id, None);
        let cwd = match &spec.cwd {
            Some(c) => {
                let p = repo_path(&c.to_string_lossy(), &home);
                if p.is_dir() {
                    p
                } else if mode == SpawnMode::New {
                    return Err(KeltaError::not_found(format!("directory {} does not exist", p.display())));
                } else {
                    default_cwd
                }
            }
            None => default_cwd,
        };
        spec.cwd = Some(cwd.clone());
        if spec.cols == 0 || spec.rows == 0 {
            spec.cols = spec.cols.max(80);
            spec.rows = spec.rows.max(24);
        }

        // Claude session uuid (resume needs it).
        let is_claude = matches!(spec.kind, SessionKind::Claude);
        let mut claude_uuid = None;
        if is_claude {
            claude_uuid = spawn_env::claude_uuid_from_args(&spec.args).or_else(|| match &spec.restore {
                RestorePolicy::ClaudeResume { uuid } => Some(uuid.clone()),
                _ => None,
            });
            let selects =
                spec.args.iter().any(|a| matches!(a.as_str(), "--continue" | "-c" | "--resume" | "-r"));
            if claude_uuid.is_none() && !selects && mode == SpawnMode::New {
                let u = uuid::Uuid::new_v4().to_string();
                spec.args.splice(0..0, ["--session-id".to_owned(), u.clone()]);
                launch.args.splice(0..0, ["--session-id".to_owned(), u.clone()]);
                claude_uuid = Some(u);
            }
        }

        // Existing entry (restore / restart) keeps identity and metadata.
        let (ticket, prev_restore, created_at, name) = {
            let s = self.sessions.lock();
            match s.get(&id) {
                Some(e) => (
                    e.ticket.clone(),
                    Some(e.restore.clone()),
                    Some(e.info.created_at.clone()),
                    Some(e.info.name.clone()),
                ),
                None => (None, None, None, None),
            }
        };
        let restore = match (&prev_restore, mode) {
            (Some(r), SpawnMode::Restore | SpawnMode::Restart) => r.clone(),
            _ => default_restore(
                &spec.kind,
                &spec.restore,
                claude_uuid.as_deref(),
                self.dirs.data.join("sessions").join(format!("{id}.vim")),
            ),
        };
        let ticket_key = self.ticket_key(ticket.as_ref(), spec.work_item_id.as_ref()).await;

        // Lazy HTTP server for MCP / http hooks.
        let mut http_ref = false;
        let mut mcp_url = None;
        if is_claude && (settings.claude.mcp || settings.claude.hook_transport == HookTransport::Http) {
            match self.server.ensure_http().await {
                Ok(port) => {
                    http_ref = true;
                    self.http_refs.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                    mcp_url = Some(format!("http://127.0.0.1:{port}/mcp/{id}"));
                }
                Err(e) => tracing::debug!(error = %e, "http server unavailable"),
            }
        }
        let hook_token = spawn_env::token();
        let mcp_token = is_claude.then(spawn_env::token);
        let env = spawn_env::assemble_env(&EnvInputs {
            login: &self.login_env.vars,
            terminal_env: &settings.terminal.env,
            project_env: &settings.env,
            request_env: &spec.env,
            version: kelta_proto::VERSION,
            session_id: id.as_str(),
            project_id: spec.project_id.as_str(),
            ctl_sock: &self.dirs.ctl_socket(),
            hook_token: &hook_token,
            ticket: ticket_key.as_deref(),
            mcp_token: mcp_token.as_deref(),
            mcp_url: mcp_url.as_deref(),
        });
        let shell = spawn_env::default_shell(&settings.terminal.shell, &self.login_env.vars);
        let resolved = match &launch.program {
            None => Ok((
                shell.clone(),
                std::iter::once("-l".to_owned()).chain(launch.args.iter().cloned()).collect::<Vec<_>>(),
            )),
            Some(p) => spawn_env::resolve_program(p, env.get("PATH").map(String::as_str), &cwd)
                .map(|path| (path, launch.args.clone())),
        };
        let (program, args) = match resolved {
            Ok(x) => x,
            Err(e) => {
                self.release_http_ref(http_ref);
                return Err(e);
            }
        };

        let info = SessionInfo {
            id: id.clone(),
            project_id: spec.project_id.clone(),
            kind: spec.kind.clone(),
            name: spec
                .name
                .clone()
                .filter(|n| !n.trim().is_empty())
                .or(name)
                .unwrap_or_else(|| default_name(&spec.kind, spec.program.as_deref(), &shell)),
            title: None,
            cwd: cwd.clone(),
            status: SessionStatus::Starting,
            status_source: StatusSource::None,
            attention: Attention::None,
            seen: true,
            lifecycle: Lifecycle::Live,
            pid: None,
            exit_code: None,
            work_item_id: spec.work_item_id.clone(),
            claude: is_claude.then(|| ClaudeMeta {
                session_uuid: claude_uuid.clone().unwrap_or_default(),
                model: spawn_env::arg_value(&args, "--model"),
                preview: None,
                files_touched: Vec::new(),
                hooks_active: false,
            }),
            editor: match &spec.kind {
                SessionKind::Editor { adapter } => Some(EditorMeta {
                    adapter: adapter.clone(),
                    socket: spawn_env::arg_value(&args, "--listen").map(PathBuf::from),
                }),
                _ => None,
            },
            cols: spec.cols,
            rows: spec.rows,
            created_at: match mode {
                SpawnMode::Restore => created_at.unwrap_or_else(kelta_proto::now_rfc3339),
                _ => kelta_proto::now_rfc3339(),
            },
        };
        let scrollback = settings.terminal.scrollback.for_kind(spec.kind.name());
        let pty = PtySpawnSpec {
            id: id.clone(),
            program,
            args,
            cwd,
            env,
            cols: spec.cols,
            rows: spec.rows,
            scrollback_lines: scrollback,
            kind: spec.kind.clone(),
            events: Arc::new(EventSink { core: self.me.clone() }),
        };
        let previous = {
            let mut s = self.sessions.lock();
            // killed/removed while this restore/restart awaited: do not resurrect it
            if mode != SpawnMode::New && !s.contains_key(&id) {
                drop(s);
                self.release_http_ref(http_ref);
                return Err(KeltaError::not_found(format!("session {id}")));
            }
            let entry = SessionEntry {
                info: info.clone(),
                spec: spec.clone(),
                restore,
                hook_token: hook_token.clone(),
                mcp_token: mcp_token.clone(),
                ticket,
                text_tail: s.get(&id).and_then(|e| e.text_tail.clone()),
                spawned_at: Some(Instant::now()),
                resume_attempt: launch.resume_attempt,
                kill_requested: false,
                restarting: false,
                restoring: s.get(&id).is_some_and(|e| e.restoring),
                http_ref,
                heuristic: false,
                timers: Arc::new(SessionTimers::default()),
            };
            s.insert(id.clone(), entry)
        };
        if let Err(e) = self.terminal.spawn(pty) {
            let mut s = self.sessions.lock();
            match previous {
                Some(mut p) => {
                    p.info.lifecycle =
                        if mode == SpawnMode::Restore { Lifecycle::Dormant } else { p.info.lifecycle };
                    s.insert(id.clone(), p);
                }
                None => {
                    s.remove(&id);
                }
            }
            drop(s);
            self.release_http_ref(http_ref);
            return Err(e);
        }
        self.server.register_session(&id, &hook_token, mcp_token.as_deref());
        self.persist_session(&id);
        let info = self.get_info(&id)?;
        self.emit(UiEvent::SessionUpdated { session: info.clone() });
        self.publish_ev(
            BusEvent::new(bus::SESSION_SPAWNED, serde_json::json!({ "session": info }))
                .with_project(info.project_id.clone())
                .with_session(id.clone()),
        );
        if is_claude {
            self.arm_hooks_timer(&id);
        }
        self.refresh_attention();
        Ok(info)
    }

    fn release_http_ref(&self, had: bool) {
        if had {
            self.http_refs.fetch_sub(1, std::sync::atomic::Ordering::SeqCst);
            self.server.release_http();
        }
    }

    /// Dormant → Live (lazy restore on attach, eager restore, `--continue` fallback).
    pub(crate) async fn restore_session(
        &self,
        id: &SessionId,
        cols: u16,
        rows: u16,
        use_continue: bool,
    ) -> Result<SessionInfo, KeltaError> {
        let (spec, policy) = {
            let mut s = self.sessions.lock();
            let e = s.get_mut(id).ok_or_else(|| KeltaError::not_found(format!("session {id}")))?;
            // the --continue fallback runs from Dormant too, so it never bypasses this guard
            if e.restoring || e.info.lifecycle == Lifecycle::Live {
                return Ok(e.info.clone());
            }
            // claim (a concurrent attach sees `restoring` and waits)
            e.info.lifecycle = Lifecycle::Live;
            e.restoring = true;
            let mut spec = e.spec.clone();
            spec.cwd = Some(e.info.cwd.clone());
            spec.cols = if cols > 0 { cols } else { e.info.cols };
            spec.rows = if rows > 0 { rows } else { e.info.rows };
            (spec, e.restore.clone())
        };
        let launch = self.relaunch(id, &spec, &policy, use_continue).await;
        let res = self.spawn_with(id.clone(), spec, launch, SpawnMode::Restore).await;
        if let Some(x) = self.sessions.lock().get_mut(id) {
            x.restoring = false;
            if res.is_err() {
                x.info.lifecycle = Lifecycle::Dormant;
            }
        }
        self.exits.notify_waiters();
        res
    }

    // =========================================================================================
    // Terminal passthroughs
    // =========================================================================================

    /// Launch for a restore or restart under the same id.
    async fn relaunch(
        &self,
        id: &SessionId,
        spec: &SpawnRequest,
        policy: &RestorePolicy,
        use_continue: bool,
    ) -> Launch {
        let binary = self.cfg.effective(Some(&spec.project_id)).claude.binary.clone();
        let mut launch = restore_launch(spec, policy, &binary, use_continue);
        // A work item's Claude needs its per-session files regenerated (the runtime dir does not
        // survive a reboot): kelta-work builds the full argv.
        if matches!(policy, RestorePolicy::ClaudeResume { .. }) && spec.work_item_id.is_some() {
            match self.work.claude_restore_request(id, use_continue).await {
                Ok(Some(req)) => {
                    launch = Launch { program: req.program, args: req.args, resume_attempt: !use_continue };
                }
                Ok(None) => {}
                Err(e) => {
                    tracing::warn!(error = %e, session = %id, "work item Claude restore files not written")
                }
            }
        }
        launch
    }

    /// `session_attach`: Dormant sessions spawn here (lazy restore).
    pub async fn session_attach(
        &self,
        id: &SessionId,
        cols: u16,
        rows: u16,
        sink: Box<dyn FrameSink>,
    ) -> Result<AttachInfo, KeltaError> {
        self.rt.capture();
        // a restore in flight (eager restore, double attach) → wait for its PTY
        let lifecycle = loop {
            let notified = self.exits.notified();
            tokio::pin!(notified);
            notified.as_mut().enable();
            let (lifecycle, restoring) = {
                let s = self.sessions.lock();
                let e = s.get(id).ok_or_else(|| KeltaError::not_found(format!("session {id}")))?;
                (e.info.lifecycle, e.restoring)
            };
            if !restoring {
                break lifecycle;
            }
            notified.await;
        };
        if lifecycle == Lifecycle::Dormant {
            self.restore_session(id, cols, rows, false).await?;
        }
        let info = self.terminal.attach(id, cols, rows, sink)?;
        if let Some(e) = self.sessions.lock().get_mut(id) {
            e.info.cols = info.cols;
            e.info.rows = info.rows;
        }
        Ok(info)
    }

    pub fn session_detach(&self, id: &SessionId, generation: u32) {
        self.terminal.detach(id, generation);
    }

    /// The window is gone: drop every view so sessions release their view-side memory (background mode).
    pub fn detach_all_views(&self) {
        let ids: Vec<SessionId> = self.sessions.lock().keys().cloned().collect();
        for id in &ids {
            self.terminal.detach(id, kelta_proto::api::ANY_VIEW);
        }
    }

    pub(crate) fn write_session(&self, id: &SessionId, bytes: &[u8]) -> Result<(), KeltaError> {
        let lifecycle = self.get_info(id)?.lifecycle;
        if lifecycle != Lifecycle::Live {
            return Err(KeltaError::new(ErrorCode::Conflict, format!("session {id} is not running")));
        }
        self.terminal.write(id, bytes)
    }

    pub fn session_resize(&self, id: &SessionId, cols: u16, rows: u16) -> Result<(), KeltaError> {
        let lifecycle = self.get_info(id)?.lifecycle;
        if let Some(e) = self.sessions.lock().get_mut(id) {
            e.info.cols = cols;
            e.info.rows = rows;
        }
        if lifecycle == Lifecycle::Live { self.terminal.resize(id, cols, rows) } else { Ok(()) }
    }

    pub fn session_ack(&self, id: &SessionId, generation: u32, bytes: u32) {
        self.terminal.ack(id, generation, bytes);
    }

    // =========================================================================================
    // Kill / remove / restart
    // =========================================================================================

    pub(crate) fn kill_session(&self, id: &SessionId, force: bool) -> Result<(), KeltaError> {
        let lifecycle = {
            let mut s = self.sessions.lock();
            let e = s.get_mut(id).ok_or_else(|| KeltaError::not_found(format!("session {id}")))?;
            e.kill_requested = true;
            e.info.lifecycle
        };
        if lifecycle != Lifecycle::Live {
            self.remove_session(id);
            return Ok(());
        }
        let sig = if force { KillSignal::Kill } else { KillSignal::Hup };
        match self.terminal.kill(id, sig) {
            Ok(()) => Ok(()),
            Err(e) if e.code == ErrorCode::NotFound => {
                self.remove_session(id);
                Ok(())
            }
            Err(e) => Err(e),
        }
    }

    pub(crate) fn remove_session(&self, id: &SessionId) {
        let Some(entry) = self.sessions.lock().remove(id) else { return };
        entry.timers.cancel();
        self.server.unregister_session(id);
        self.release_http_ref(entry.http_ref);
        self.delete_row(id);
        let changed: Vec<ProjectId> = {
            let mut layouts = self.layouts.lock();
            layouts
                .iter_mut()
                .filter_map(|(p, l)| {
                    crate::layout::remove_session(l, id).then(|| {
                        l.rev += 1;
                        p.clone()
                    })
                })
                .collect()
        };
        for p in changed {
            self.layout_changed(&p);
        }
        self.emit(UiEvent::SessionRemoved { id: id.clone() });
        self.refresh_attention();
    }

    /// Wait (≤ `QUIT_GRACE`) until the session left `Live`.
    async fn wait_exit(&self, ids: &[SessionId]) -> bool {
        let deadline = tokio::time::Instant::now() + QUIT_GRACE;
        loop {
            let notified = self.exits.notified();
            tokio::pin!(notified);
            notified.as_mut().enable();
            let pending = {
                let s = self.sessions.lock();
                ids.iter().any(|i| s.get(i).is_some_and(|e| e.info.lifecycle == Lifecycle::Live))
            };
            if !pending {
                return true;
            }
            tokio::select! {
                _ = &mut notified => {}
                // one-shot: quit / restart grace, armed by the kill request.
                _ = tokio::time::sleep_until(deadline) => return false,
            }
        }
    }

    /// `session_restart`: same id, same request (a work item's Claude gets its restore argv).
    pub async fn session_restart(&self, id: &SessionId) -> Result<SessionInfo, KeltaError> {
        self.rt.capture();
        let (spec, lifecycle, restore) = {
            let mut s = self.sessions.lock();
            let e = s.get_mut(id).ok_or_else(|| KeltaError::not_found(format!("session {id}")))?;
            if e.info.lifecycle == Lifecycle::Live {
                e.restarting = true;
            }
            (e.spec.clone(), e.info.lifecycle, e.restore.clone())
        };
        match lifecycle {
            Lifecycle::Dormant => return self.restore_session(id, 0, 0, false).await,
            Lifecycle::Live => {
                let _ = self.terminal.kill(id, KillSignal::Hup);
                if !self.wait_exit(std::slice::from_ref(id)).await {
                    let _ = self.terminal.kill(id, KillSignal::Kill);
                    self.wait_exit(std::slice::from_ref(id)).await;
                }
            }
            Lifecycle::Exited => {}
        }
        // Claude: `--resume <uuid>` (replaying `--session-id` + the first prompt would be refused).
        let policy = if matches!(restore, RestorePolicy::ClaudeResume { .. }) {
            restore
        } else {
            RestorePolicy::Relaunch
        };
        let launch = self.relaunch(id, &spec, &policy, false).await;
        self.spawn_with(id.clone(), spec, launch, SpawnMode::Restart).await
    }

    // =========================================================================================
    // Metadata
    // =========================================================================================

    fn update_info(
        &self,
        id: &SessionId,
        f: impl FnOnce(&mut SessionEntry),
    ) -> Result<SessionInfo, KeltaError> {
        let info = {
            let mut s = self.sessions.lock();
            let e = s.get_mut(id).ok_or_else(|| KeltaError::not_found(format!("session {id}")))?;
            f(e);
            e.info.clone()
        };
        self.persist_session(id);
        self.emit(UiEvent::SessionUpdated { session: info.clone() });
        Ok(info)
    }

    pub fn session_rename(&self, id: &SessionId, name: &str) -> Result<SessionInfo, KeltaError> {
        let name = name.trim();
        if name.is_empty() {
            return Err(KeltaError::invalid("session name cannot be empty"));
        }
        self.update_info(id, |e| {
            e.info.name = name.to_owned();
            e.spec.name = Some(name.to_owned());
        })
    }

    pub fn session_link(
        &self,
        id: &SessionId,
        work_item_id: Option<WorkItemId>,
        ticket: Option<TicketRef>,
    ) -> Result<SessionInfo, KeltaError> {
        self.update_info(id, |e| {
            if let Some(w) = work_item_id {
                e.info.work_item_id = Some(w.clone());
                e.spec.work_item_id = Some(w);
            }
            if ticket.is_some() {
                e.ticket = ticket;
            }
        })
    }

    pub fn session_mark_seen(&self, id: &SessionId) -> Result<(), KeltaError> {
        let (state, source) = {
            let s = self.sessions.lock();
            let e = s.get(id).ok_or_else(|| KeltaError::not_found(format!("session {id}")))?;
            (e.state(true), e.info.status_source)
        };
        self.apply_decision(id, status::mark_seen(&state), source, None);
        Ok(())
    }

    pub fn session_text_tail(&self, id: &SessionId, max_lines: u32) -> Result<String, KeltaError> {
        let (lifecycle, stored) = {
            let s = self.sessions.lock();
            let e = s.get(id).ok_or_else(|| KeltaError::not_found(format!("session {id}")))?;
            (e.info.lifecycle, e.text_tail.clone())
        };
        if lifecycle == Lifecycle::Dormant {
            // The on-disk history log is newer than the tail stored at quit (and survives a crash).
            let disk = self.terminal.history_tail(id, max_lines).unwrap_or_default();
            if !disk.is_empty() {
                return Ok(disk);
            }
            return Ok(tail_lines(&stored.unwrap_or_default(), max_lines));
        }
        match self.terminal.text_tail(id, max_lines) {
            Ok(t) => Ok(t),
            Err(e) => stored.map(|t| tail_lines(&t, max_lines)).ok_or(e),
        }
    }

    /// Search the on-disk history of one session, or of every session of `project_id`.
    pub fn session_history_search(
        &self,
        project_id: &ProjectId,
        session_id: Option<&SessionId>,
        query: &str,
        limit: u32,
    ) -> Result<Vec<HistoryHit>, KeltaError> {
        let ids: Vec<SessionId> = {
            let s = self.sessions.lock();
            if let Some(id) = session_id
                && !s.get(id).is_some_and(|e| &e.info.project_id == project_id)
            {
                return Err(KeltaError::not_found(format!("session {id} in project {project_id}")));
            }
            s.values()
                .filter(|e| &e.info.project_id == project_id && session_id.is_none_or(|id| &e.info.id == id))
                .map(|e| e.info.id.clone())
                .collect()
        };
        self.terminal.history_search(&ids, query, limit.clamp(1, 1000))
    }

    // =========================================================================================
    // Status / attention
    // =========================================================================================

    /// Fold a decision into the session and emit what changed.
    pub(crate) fn apply_decision(
        &self,
        id: &SessionId,
        d: Decision,
        source: StatusSource,
        preview: Option<String>,
    ) {
        let out = {
            let mut s = self.sessions.lock();
            let Some(e) = s.get_mut(id) else { return };
            let prev = e.info.status;
            let mut changed =
                e.info.status != d.status || e.info.attention != d.attention || e.info.seen != d.seen;
            e.info.status = d.status;
            e.info.attention = d.attention;
            e.info.seen = d.seen;
            if prev != d.status && e.info.status_source != source {
                e.info.status_source = source;
                changed = true;
            }
            if let (Some(h), Some(c)) = (d.hooks_active, e.info.claude.as_mut())
                && c.hooks_active != h
            {
                c.hooks_active = h;
                changed = true;
            }
            if let (Some(p), Some(c)) = (preview.clone(), e.info.claude.as_mut())
                && c.preview.as_ref() != Some(&p)
            {
                c.preview = Some(p);
                changed = true;
            }
            (prev, e.info.clone(), changed)
        };
        let (prev, info, changed) = out;
        if changed {
            self.emit(UiEvent::SessionUpdated { session: info.clone() });
        }
        if prev != info.status {
            let src = match source {
                StatusSource::Hook => "hook",
                StatusSource::Heuristic => "heuristic",
                StatusSource::None => "none",
            };
            self.publish_ev(
                BusEvent::new(
                    bus::SESSION_STATUS_CHANGED,
                    serde_json::json!({ "status": info.status, "previous": prev, "source": src, "preview": preview }),
                )
                .with_project(info.project_id.clone())
                .with_session(info.id.clone()),
            );
        }
        if let Some(kind) = d.notify {
            self.maybe_notify(kind, &info, preview.as_deref());
        }
        self.refresh_attention();
    }

    /// Apply the notification rules for a session event.
    pub(crate) fn maybe_notify(&self, kind: NotifyKind, info: &SessionInfo, body: Option<&str>) {
        // kelta-work sends "KEY ready to review" / "Claude replied" for hook-driven work item Claude.
        if kind == NotifyKind::ClaudeDone
            && info.work_item_id.is_some()
            && info.status_source == StatusSource::Hook
        {
            return;
        }
        let settings = self.cfg.effective(Some(&info.project_id));
        let visible = self.is_visible(&info.id);
        let window = self.bridge.window_state();
        if !crate::notifier::should_notify(
            kind,
            &settings.notifications,
            window,
            visible,
            crate::notifier::local_minute_of_day(),
        ) {
            return;
        }
        let project = self.project_info(&info.project_id).map(|p| p.name).unwrap_or_default();
        let (title, urgency) = match kind {
            NotifyKind::ClaudeNeedsInput => (format!("{} needs input", info.name), Urgency::Critical),
            NotifyKind::ClaudeDone => (format!("{} finished", info.name), Urgency::Normal),
            NotifyKind::BellBackground => (format!("Bell in {}", info.name), Urgency::Low),
            _ => (info.name.clone(), Urgency::Normal),
        };
        let body = body.map(str::to_owned).or_else(|| (!project.is_empty()).then(|| project.clone()));
        self.deliver(Notification {
            title,
            body,
            urgency,
            project_id: Some(info.project_id.clone()),
            session_id: Some(info.id.clone()),
        });
    }

    /// Desktop notification, falling back to an in-app toast when the daemon is missing.
    pub(crate) fn deliver(&self, n: Notification) {
        if let Err(e) = self.bridge.notify(n.clone()) {
            tracing::debug!(error = %e, "notification failed; toast instead");
            let text = match &n.body {
                Some(b) => format!("{} — {b}", n.title),
                None => n.title.clone(),
            };
            self.emit(UiEvent::Toast { toast: Toast::info(text) });
        }
    }

    /// `CoreApi::notify` (plugins, MCP `notify`): enabled + quiet hours.
    pub(crate) fn notify_rules(&self, n: Notification) -> Result<(), KeltaError> {
        let settings = self.cfg.effective(n.project_id.as_ref());
        let ns = &settings.notifications;
        if !ns.enabled
            || crate::notifier::in_quiet_hours(&ns.quiet_hours, crate::notifier::local_minute_of_day())
        {
            return Ok(());
        }
        // About a session Louis is looking at right now: nothing to tell.
        let w = self.bridge.window_state();
        if ns.only_when_unfocused
            && w.exists
            && w.visible
            && w.focused
            && n.session_id.as_ref().is_some_and(|s| self.is_visible(s))
        {
            return Ok(());
        }
        self.deliver(n);
        Ok(())
    }

    /// `session_apply_hook`.
    pub(crate) fn apply_hook(&self, id: &SessionId, change: StatusChange) -> Result<(), KeltaError> {
        let visible = self.is_visible(id);
        let (state, timers, project) = {
            let mut s = self.sessions.lock();
            let e = s.get_mut(id).ok_or_else(|| KeltaError::not_found(format!("session {id}")))?;
            if let (Some(f), Some(c)) = (&change.file_edited, e.info.claude.as_mut())
                && !c.files_touched.contains(f)
            {
                c.files_touched.push(f.clone());
                if c.files_touched.len() > 100 {
                    c.files_touched.remove(0);
                }
            }
            e.heuristic = false;
            (e.state(visible), e.timers.clone(), e.info.project_id.clone())
        };
        let d = status::apply_hook(&state, &change);
        if d.hooks_active == Some(true) {
            timers.hooks.cancel();
            timers.quiet.cancel();
        }
        if let Some(f) = &change.file_edited {
            let tool = change.raw_event.split_once(':').map(|(_, t)| t.to_owned());
            self.publish_ev(
                BusEvent::new(bus::CLAUDE_FILE_EDITED, serde_json::json!({ "path": f, "tool": tool }))
                    .with_project(project)
                    .with_session(id.clone()),
            );
        }
        self.apply_decision(id, d, StatusSource::Hook, change.preview.clone());
        Ok(())
    }

    /// Update the Claude uuid learned from a hook payload (resume policy follows it).
    pub(crate) fn learn_claude_uuid(&self, id: &SessionId, uuid: &str) {
        let changed = {
            let mut s = self.sessions.lock();
            let Some(e) = s.get_mut(id) else { return };
            let Some(c) = e.info.claude.as_mut() else { return };
            if c.session_uuid == uuid {
                false
            } else {
                c.session_uuid = uuid.to_owned();
                if matches!(e.restore, RestorePolicy::ClaudeResume { .. } | RestorePolicy::Relaunch) {
                    e.restore = RestorePolicy::ClaudeResume { uuid: uuid.to_owned() };
                }
                true
            }
        };
        if changed {
            self.persist_session(id);
        }
    }

    fn arm_hooks_timer(&self, id: &SessionId) {
        let Some(timers) = self.sessions.lock().get(id).map(|e| e.timers.clone()) else { return };
        let weak = self.me.clone();
        let sid = id.clone();
        timers.hooks.arm(&self.rt, HOOKS_GRACE, async move {
            if let Some(core) = weak.upgrade() {
                core.hooks_timeout(&sid);
            }
        });
    }

    /// No SessionStart within the grace: hooks inactive → heuristic status.
    pub(crate) fn hooks_timeout(&self, id: &SessionId) {
        let info = {
            let mut s = self.sessions.lock();
            let Some(e) = s.get_mut(id) else { return };
            if e.hooks_active() || e.info.lifecycle != Lifecycle::Live {
                return;
            }
            e.heuristic = true;
            e.info.status_source = StatusSource::Heuristic;
            if let Some(c) = e.info.claude.as_mut() {
                c.hooks_active = false;
            }
            e.info.clone()
        };
        self.emit(UiEvent::SessionUpdated { session: info });
    }

    fn heuristic(&self, id: &SessionId, ev: Heuristic) {
        let visible = self.is_visible(id);
        let Some((state, timers)) =
            self.sessions.lock().get(id).map(|e| (e.state(visible), e.timers.clone()))
        else {
            return;
        };
        let d = status::apply_heuristic(&state, ev);
        if ev == Heuristic::Output {
            let weak = self.me.clone();
            let sid = id.clone();
            timers.quiet.arm(&self.rt, QUIET_AFTER_OUTPUT, async move {
                if let Some(core) = weak.upgrade() {
                    core.heuristic(&sid, Heuristic::Quiet);
                }
            });
        }
        self.apply_decision(id, d, StatusSource::Heuristic, None);
    }

    // =========================================================================================
    // Terminal events
    // =========================================================================================

    pub(crate) fn on_terminal_event(&self, id: &SessionId, ev: TerminalEvent) {
        let Some((project, kind, heuristic, hooked)) = self
            .sessions
            .lock()
            .get(id)
            .map(|e| (e.info.project_id.clone(), e.info.kind.clone(), e.heuristic, e.hooks_active()))
        else {
            return;
        };
        let is_claude = matches!(kind, SessionKind::Claude);
        match ev {
            TerminalEvent::Title(t) => {
                if let Ok(info) = self.update_title(id, &t) {
                    self.publish_ev(
                        BusEvent::new(bus::SESSION_TITLE_CHANGED, serde_json::json!({ "title": t }))
                            .with_project(info.project_id)
                            .with_session(id.clone()),
                    );
                }
            }
            TerminalEvent::Cwd(p) => {
                let info = {
                    let mut s = self.sessions.lock();
                    let Some(e) = s.get_mut(id) else { return };
                    if e.info.cwd == p {
                        return;
                    }
                    e.info.cwd = p.clone();
                    e.info.clone()
                };
                let sid = id.clone();
                self.store.exec("session_cwd", move |c| q::session_set_cwd(c, &sid, &p));
                self.emit(UiEvent::SessionUpdated { session: info });
            }
            TerminalEvent::Bell => {
                self.publish_ev(
                    BusEvent::new(bus::SESSION_BELL, serde_json::json!({}))
                        .with_project(project.clone())
                        .with_session(id.clone()),
                );
                if is_claude && heuristic {
                    self.heuristic(id, Heuristic::Alert);
                } else if self.cfg.effective(Some(&project)).terminal.bell == BellMode::Attention {
                    let visible = self.is_visible(id);
                    let Some(state) = self.sessions.lock().get(id).map(|e| e.state(visible)) else { return };
                    let source =
                        self.sessions.lock().get(id).map(|e| e.info.status_source).unwrap_or_default();
                    self.apply_decision(id, status::apply_bell(&state), source, None);
                }
            }
            TerminalEvent::Notify { title, body } => {
                if is_claude && heuristic {
                    self.heuristic(id, Heuristic::Alert);
                    return;
                }
                let visible = self.is_visible(id);
                let Some((state, source, info)) = self
                    .sessions
                    .lock()
                    .get(id)
                    .map(|e| (e.state(visible), e.info.status_source, e.info.clone()))
                else {
                    return;
                };
                let mut d = status::apply_bell(&state);
                d.notify = None;
                self.apply_decision(id, d, source, None);
                let settings = self.cfg.effective(Some(&project));
                if crate::notifier::should_notify(
                    NotifyKind::Program,
                    &settings.notifications,
                    self.bridge.window_state(),
                    visible,
                    crate::notifier::local_minute_of_day(),
                ) {
                    self.deliver(Notification {
                        title: title.unwrap_or_else(|| info.name.clone()),
                        body: Some(body),
                        urgency: Urgency::Normal,
                        project_id: Some(project),
                        session_id: Some(id.clone()),
                    });
                }
            }
            TerminalEvent::ClipboardStore { kind: ck, text } => {
                if self.cfg.effective(Some(&project)).terminal.osc52 != Osc52::Off {
                    let clip = self.clip.clone();
                    self.rt.spawn(async move {
                        if let Err(e) = clip.write(ck, &text) {
                            tracing::debug!(error = %e, "osc52 store failed");
                        }
                    });
                }
            }
            TerminalEvent::ClipboardLoad { kind: ck } => {
                if self.cfg.effective(Some(&project)).terminal.osc52 == Osc52::ReadWrite {
                    let clip = self.clip.clone();
                    let term = self.terminal.clone();
                    let sid = id.clone();
                    self.rt.spawn(async move {
                        let text = clip.read(ck).unwrap_or_default();
                        if let Err(e) = term.write(&sid, &osc52_reply(ck, &text)) {
                            tracing::debug!(error = %e, "osc52 reply failed");
                        }
                    });
                }
            }
            TerminalEvent::Activity => {
                if is_claude && heuristic && !hooked {
                    self.heuristic(id, Heuristic::Output);
                } else {
                    let visible = self.is_visible(id);
                    let Some((state, source)) =
                        self.sessions.lock().get(id).map(|e| (e.state(visible), e.info.status_source))
                    else {
                        return;
                    };
                    self.apply_decision(id, status::apply_activity(&state, is_claude), source, None);
                }
            }
            TerminalEvent::Exited { code, signal } => self.on_exited(id, code, signal),
            TerminalEvent::AckTimeout { .. } => self.on_ack_timeout(),
            TerminalEvent::MemoryCapReached { cap_mb } => self.emit(UiEvent::Toast {
                toast: Toast::warn(format!(
                    "Terminal scrollback reached the {cap_mb} MB cap (terminal.memory_cap_mb): \
                     the least recently viewed sessions keep their last {} lines.",
                    kelta_term::SHRINK_FLOOR
                )),
            }),
        }
    }

    fn update_title(&self, id: &SessionId, t: &str) -> Result<SessionInfo, KeltaError> {
        let info = {
            let mut s = self.sessions.lock();
            let e = s.get_mut(id).ok_or_else(|| KeltaError::not_found(format!("session {id}")))?;
            e.info.title = (!t.is_empty()).then(|| t.to_owned());
            e.info.clone()
        };
        self.emit(UiEvent::SessionUpdated { session: info.clone() });
        Ok(info)
    }

    fn on_ack_timeout(&self) {
        let w = self.bridge.window_state();
        if !(w.exists && w.visible) {
            return;
        }
        {
            let mut last = self.last_reload.lock();
            if last.is_some_and(|t| t.elapsed() < Duration::from_secs(30)) {
                return;
            }
            *last = Some(Instant::now());
        }
        self.bridge.reload_webview(true);
        self.emit(UiEvent::Toast { toast: Toast::warn("A plugin screen stopped responding and was closed") });
    }

    fn on_exited(&self, id: &SessionId, code: Option<i32>, signal: Option<i32>) {
        let quitting = self.quitting.load(std::sync::atomic::Ordering::SeqCst);
        enum Next {
            Nothing,
            Remove,
            Keep,
            Continue(u16, u16),
        }
        let (next, info, http_ref) = {
            let mut s = self.sessions.lock();
            let Some(e) = s.get_mut(id) else { return };
            e.timers.cancel();
            e.info.lifecycle = Lifecycle::Exited;
            e.info.status = SessionStatus::Exited;
            e.info.exit_code = code.or(signal.map(|_| -1));
            if e.info.attention == Attention::NeedsInput {
                e.info.attention = Attention::None;
            }
            let http_ref = std::mem::take(&mut e.http_ref);
            let next = if quitting {
                Next::Nothing
            } else if e.restarting {
                e.restarting = false;
                Next::Nothing
            } else if e.resume_attempt
                && !e.kill_requested
                && code != Some(0)
                && e.spawned_at.is_some_and(|t| t.elapsed() < RESUME_FAIL_WINDOW)
            {
                e.resume_attempt = false;
                e.info.lifecycle = Lifecycle::Dormant;
                Next::Continue(e.info.cols, e.info.rows)
            } else if e.kill_requested
                || e.spec.close_on_exit == CloseOnExit::Always
                || (e.spec.close_on_exit == CloseOnExit::OnSuccess && code == Some(0))
            {
                Next::Remove
            } else {
                Next::Keep
            };
            (next, e.info.clone(), http_ref)
        };
        self.exits.notify_waiters();
        self.release_http_ref(http_ref);
        if quitting {
            return;
        }
        self.server.unregister_session(id);
        self.publish_ev(
            BusEvent::new(bus::SESSION_EXITED, serde_json::json!({ "code": code, "signal": signal }))
                .with_project(info.project_id.clone())
                .with_session(id.clone()),
        );
        match next {
            Next::Nothing => {}
            Next::Remove => self.remove_session(id),
            Next::Keep => {
                self.persist_session(id);
                self.emit(UiEvent::SessionUpdated { session: info });
            }
            Next::Continue(cols, rows) => {
                tracing::info!(session = %id, "claude --resume refused; retrying with --continue");
                let weak = self.me.clone();
                let sid = id.clone();
                self.rt.spawn(async move {
                    if let Some(core) = weak.upgrade()
                        && let Err(e) = core.restore_session(&sid, cols, rows, true).await
                    {
                        tracing::warn!(error = %e, "claude --continue fallback failed");
                    }
                });
            }
        }
        self.refresh_attention();
    }

    // =========================================================================================
    // Quit
    // =========================================================================================

    /// The next run adopts keltad's sessions: the host is keltad and the (restart-only)
    /// `terminal.session_host` still says so, else kept sessions would run twice.
    fn quit_persistent(&self) -> bool {
        self.terminal.persistent() && self.cfg.effective(None).terminal.session_host == SessionHost::Daemon
    }

    pub(crate) async fn quit_flow(&self) -> Result<(), KeltaError> {
        self.quitting.store(true, std::sync::atomic::Ordering::SeqCst);
        if let Err(e) = self.work.quit_hook().await {
            tracing::warn!(error = %e, "work quit hook failed");
        }
        // keltad: kept sessions stay running and are adopted on the next start.
        let persistent = self.quit_persistent();
        let restoring = self.cfg.effective(None).app.restore_mode != RestoreMode::None;
        let live: Vec<(SessionId, bool)> = self
            .sessions
            .lock()
            .values()
            .filter(|e| e.info.lifecycle != Lifecycle::Dormant)
            .map(|e| {
                let keep = e.info.lifecycle == Lifecycle::Live && e.restore != RestorePolicy::None;
                (e.info.id.clone(), keep && (restoring || !persistent))
            })
            .collect();
        let mut to_kill = Vec::new();
        for (id, keep) in &live {
            if *keep {
                let tail = self.terminal.text_tail(id, TEXT_TAIL_LINES).ok();
                let row = {
                    let mut s = self.sessions.lock();
                    s.get_mut(id).map(|e| {
                        if tail.is_some() {
                            e.text_tail = tail;
                        }
                        e.row("dormant")
                    })
                };
                if let Some(Ok(r)) = row {
                    self.store.exec("session_dormant", move |c| q::session_put(c, &r));
                }
            } else {
                self.delete_row(id);
            }
            let is_live = self.sessions.lock().get(id).is_some_and(|e| e.info.lifecycle == Lifecycle::Live);
            if is_live && !(persistent && *keep) {
                to_kill.push(id.clone());
            }
        }
        for id in &to_kill {
            let _ = self.terminal.kill(id, KillSignal::Hup);
        }
        if !self.wait_exit(&to_kill).await {
            for id in &to_kill {
                let live = self.sessions.lock().get(id).is_some_and(|e| e.info.lifecycle == Lifecycle::Live);
                if live {
                    let _ = self.terminal.kill(id, KillSignal::Kill);
                }
            }
        }
        if persistent {
            // close what keltad would otherwise hold for nobody (exited and killed sessions)
            let kept: HashSet<&SessionId> = live.iter().filter(|(_, k)| *k).map(|(id, _)| id).collect();
            for s in self.terminal.stats().sessions {
                if !kept.contains(&s.id) {
                    let _ = self.terminal.kill(&s.id, KillSignal::Kill);
                }
            }
        }
        // Sessions that do not survive the quit lose their history log; their readers wrote the
        // screen at exit (after `delete_row` above), so delete again now.
        // shortcut: a reader still exiting after the SIGKILL fallback can leave an orphan log (the
        // global cap removes it eventually); sweep logs without a session row at startup if they pile up.
        for (id, keep) in &live {
            if !keep {
                self.terminal.history_delete(id);
            }
        }
        self.store.flush().await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_matches_rfc4648() {
        assert_eq!(base64(b""), "");
        assert_eq!(base64(b"f"), "Zg==");
        assert_eq!(base64(b"fo"), "Zm8=");
        assert_eq!(base64(b"foo"), "Zm9v");
        assert_eq!(base64(b"foobar"), "Zm9vYmFy");
        assert_eq!(osc52_reply(ClipboardKind::Clipboard, "hi"), b"\x1b]52;c;aGk=\x07".to_vec());
    }
}

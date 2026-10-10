//! # kelta-core (L3)
//!
//! Composition root and `CoreApi` implementation: ProjectRegistry, SessionRegistry, layouts, Store
//! (SQLite + migrations, `WorkStore`/`GrantStore`/`TrustStore`), EventBus, attention, Scheduler,
//! ProviderRegistry + aggregation + caches + seen_reviews, notifier, clipboard, ctl dispatch,
//! perf snapshot.
//!
//! Every lane service is constructed with `Weak<dyn CoreApi>` and consumed only through the scaffold
//! signatures. [`Core::start_with`] lets tests inject the `kelta_proto::testing` fakes.

pub mod attention;
pub mod bus;
pub mod clipboard;
pub mod ctl;
pub mod detect;
pub mod feeds;
pub mod layout;
pub mod layout_store;
pub mod notifier;
pub mod oauth;
pub mod perf;
pub mod projects;
pub mod providers;
pub mod rt;
pub mod scheduler;
pub mod sessions;
pub mod spawn_env;
pub mod status;
pub mod store;
pub mod templates;

use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Weak};
use std::time::Duration;

use async_trait::async_trait;
use kelta_config::ConfigService;
use kelta_http::{HttpClient, ProviderFactory};
use kelta_plugins::{PluginHost, Wiring};
use kelta_proto::api::{
    CodeHost, CoreApi, GrantStore, PluginSettingsSource, SecretResolver, SettingsSource, TerminalHost,
    Tracker, TrustStore, UiBridge, WorkStore,
};
use kelta_proto::codehost::{PrDraft, ReviewItem, ReviewKind};
use kelta_proto::ctl::CtlCommand;
use kelta_proto::dirs::{CliArgs, Dirs};
use kelta_proto::error::KeltaError;
use kelta_proto::events::{BUS_CAPACITY, BusEvent, Notification, Toast, UiEvent};
use kelta_proto::ext::{BlockingOutcome, ProxiedRequest, ProxiedResponse, ToolHandle};
use kelta_proto::ids::{AccountId, ProjectId, SessionId, ToolId, WorkItemId};
use kelta_proto::ipc::AppInfo;
use kelta_proto::model::{
    EditorTarget, OpenPaneRequest, PaneRef, Placement, ProjectDraft, ProjectInfo, ProjectPatch, Scope,
    SessionInfo, SpawnRequest, StatusChange, TemplateCtx, WorkItem,
};
use kelta_proto::settings::{Layer, ProjectConfig, RuntimeOverrides, SessionHost, Settings, SettingsDiff};
use kelta_proto::term::{LoginEnv, TerminalLimits};
use kelta_proto::tracker::{Ticket, TicketRef};
use kelta_secrets::{SECRETS_FILE, Secrets, SecretsOptions};
use kelta_server::Server;
use kelta_term::PtyTerminalHost;
use kelta_term::daemon::DaemonTerminalHost;
use kelta_work::{WorkHost, WorkService};
use parking_lot::Mutex;
use tokio::sync::broadcast;

use crate::clipboard::{ClipboardBackend, SystemClipboard};
use crate::rt::Rt;
use crate::store::Store;

/// Inputs of [`Core::start`].
pub struct CoreConfig {
    pub dirs: Dirs,
    pub cli: CliArgs,
    pub bridge: Arc<dyn UiBridge>,
}

/// Settings + project-file CRUD as core consumes them. Implemented by `ConfigService`; tests
/// provide an in-memory implementation.
pub trait ConfigBackend: SettingsSource {
    fn project_create(&self, draft: &ProjectDraft) -> Result<Arc<ProjectConfig>, KeltaError>;
    fn project_update(&self, id: &ProjectId, patch: &ProjectPatch) -> Result<Arc<ProjectConfig>, KeltaError>;
    fn project_remove(&self, id: &ProjectId) -> Result<(), KeltaError>;
    fn watch(&self, on_change: Box<dyn Fn(SettingsDiff) + Send + Sync>);
}

impl ConfigBackend for ConfigService {
    fn project_create(&self, draft: &ProjectDraft) -> Result<Arc<ProjectConfig>, KeltaError> {
        ConfigService::project_create(self, draft)
    }
    fn project_update(&self, id: &ProjectId, patch: &ProjectPatch) -> Result<Arc<ProjectConfig>, KeltaError> {
        ConfigService::project_update(self, id, patch)
    }
    fn project_remove(&self, id: &ProjectId) -> Result<(), KeltaError> {
        ConfigService::project_remove(self, id)
    }
    fn watch(&self, on_change: Box<dyn Fn(SettingsDiff) + Send + Sync>) {
        ConfigService::watch(self, on_change)
    }
}

/// Injection points of [`Core::start_with`]; `None` = the real implementation.
pub struct CoreDeps {
    pub dirs: Dirs,
    pub cli: CliArgs,
    pub bridge: Arc<dyn UiBridge>,
    pub config: Option<Arc<dyn ConfigBackend>>,
    pub terminal: Option<Arc<dyn TerminalHost>>,
    pub secrets: Option<Arc<dyn SecretResolver>>,
    pub trackers: Option<Arc<dyn ProviderFactory>>,
    pub code_hosts: Option<Arc<dyn ProviderFactory>>,
    pub login_env: Option<LoginEnv>,
    pub clipboard: Option<Arc<dyn ClipboardBackend>>,
    /// Use an in-memory database instead of `<data>/kelta.db`.
    pub in_memory_store: bool,
    /// Install the stable `kelta-ctl` copy at startup (off in tests).
    pub install_ctl: bool,
    /// Bind the ctl socket and run `WorkService::startup` (off in tests).
    pub start_services: bool,
}

impl CoreDeps {
    pub fn new(dirs: Dirs, cli: CliArgs, bridge: Arc<dyn UiBridge>) -> Self {
        Self {
            dirs,
            cli,
            bridge,
            config: None,
            terminal: None,
            secrets: None,
            trackers: None,
            code_hosts: None,
            login_env: None,
            clipboard: None,
            in_memory_store: false,
            install_ctl: true,
            start_services: true,
        }
    }
}

/// The application core. Commands reach services through the accessors.
pub struct Core {
    me: Weak<Core>,
    dirs: Dirs,
    cli: CliArgs,
    bridge: Arc<dyn UiBridge>,
    config: Arc<ConfigService>,
    cfg: Arc<dyn ConfigBackend>,
    secrets: Arc<Secrets>,
    resolver: Arc<dyn SecretResolver>,
    terminal: Arc<dyn TerminalHost>,
    login_env: LoginEnv,
    http: HttpClient,
    trackers: Arc<dyn ProviderFactory>,
    code_hosts: Arc<dyn ProviderFactory>,
    work: Arc<WorkService>,
    server: Arc<Server>,
    plugins: Arc<PluginHost>,
    store: Arc<Store>,
    bus: broadcast::Sender<BusEvent>,
    pub(crate) rt: Rt,
    pub(crate) clip: Arc<dyn ClipboardBackend>,
    pub(crate) projects: Mutex<projects::ProjectsState>,
    pub(crate) layouts: Mutex<std::collections::HashMap<ProjectId, kelta_proto::model::Layout>>,
    pub(crate) sessions: Mutex<std::collections::BTreeMap<SessionId, sessions::SessionEntry>>,
    /// shortcut: sid8s handed out are never freed (8 bytes per spawn), prune on remove if spawns reach 1e5+.
    pub(crate) sid8_taken: Mutex<std::collections::HashSet<String>>,
    pub(crate) attention: Mutex<attention::AttentionState>,
    pub(crate) providers: providers::ProviderRegistry,
    pub(crate) scheduler: scheduler::Scheduler,
    pub(crate) feeds: feeds::Feeds,
    pub(crate) exits: tokio::sync::Notify,
    pub(crate) quitting: AtomicBool,
    pub(crate) http_refs: std::sync::atomic::AtomicU32,
    pub(crate) last_reload: Mutex<Option<std::time::Instant>>,
    pub(crate) claude_ver: Mutex<Option<kelta_proto::ipc::ToolVersion>>,
    /// Device-flow sign-ins between start and finish, by user code.
    pub(crate) oauth: Mutex<std::collections::HashMap<String, oauth::Pending>>,
    started: AtomicBool,
    start_services: bool,
    install_ctl: bool,
}

/// `terminal.session_host`: keltad (sessions survive quit) or PTYs in this process.
fn terminal_host(dirs: &Dirs, settings: &Settings, login_env: &LoginEnv) -> Arc<dyn TerminalHost> {
    let limits = TerminalLimits::from_settings(&settings.terminal);
    if settings.terminal.session_host == SessionHost::Daemon {
        // Launched from a stable copy: the AppImage mount (or an updated bundle) goes away on quit.
        let daemon = ctl::bundled("keltad")
            .and_then(|src| ctl::install_stable_bin(&src, &dirs.bin, kelta_proto::VERSION))
            .and_then(|exe| {
                DaemonTerminalHost::connect_or_launch(
                    &dirs.keltad_socket(),
                    &exe,
                    &dirs.logs.join("keltad.log"),
                    &dirs.data.join("history"),
                )
            });
        match daemon {
            Ok(d) => {
                d.set_limits(limits);
                return d;
            }
            Err(e) => tracing::warn!(error = %e, "keltad unavailable; sessions run in-process"),
        }
    }
    Arc::new(PtyTerminalHost::with_history_dir(
        login_env.clone(),
        limits,
        kelta_term::backend::default_backend(),
        dirs.data.join("history"),
    ))
}

impl Core {
    /// Construct every service (each lane service receives `Weak<dyn CoreApi>`).
    pub fn start(cfg: CoreConfig) -> Result<Arc<Core>, KeltaError> {
        Self::start_with(CoreDeps::new(cfg.dirs, cfg.cli, cfg.bridge))
    }

    /// [`Core::start`] with injectable services (tests).
    pub fn start_with(deps: CoreDeps) -> Result<Arc<Core>, KeltaError> {
        let CoreDeps {
            dirs,
            cli,
            bridge,
            config: cfg_override,
            terminal,
            secrets: resolver,
            trackers,
            code_hosts,
            login_env,
            clipboard,
            in_memory_store,
            install_ctl,
            start_services,
        } = deps;
        let overrides = RuntimeOverrides::from_env_and_cli(std::env::vars(), &cli);
        let config = ConfigService::load(&dirs, overrides)?;
        let cfg: Arc<dyn ConfigBackend> = match cfg_override {
            Some(c) => c,
            None => config.clone(),
        };
        let settings_source: Arc<dyn SettingsSource> = cfg.clone();
        let secrets = Secrets::with_options(
            settings_source.clone(),
            SecretsOptions { file: Some(dirs.data.join(SECRETS_FILE)), ..SecretsOptions::default() },
        );
        let resolver: Arc<dyn SecretResolver> = resolver.unwrap_or_else(|| secrets.clone());
        let settings = settings_source.effective(None);
        let login_env = login_env.unwrap_or_else(|| kelta_term::resolve_login_env(Duration::from_secs(3)));
        let terminal: Arc<dyn TerminalHost> = match terminal {
            Some(t) => t,
            None => terminal_host(&dirs, &settings, &login_env),
        };
        let store = if in_memory_store {
            Store::open_in_memory()?
        } else {
            match Store::open(&dirs.db_path()) {
                Ok(s) => s,
                Err(e) => {
                    tracing::error!(error = %e, "cannot open the state database; using an in-memory store");
                    Store::open_in_memory()?
                }
            }
        };
        // Repo trust lives in SQLite (D10): load it before anything reads the repo layers.
        futures::executor::block_on(config.set_trust_store(store.clone()));
        let (bus, _) = broadcast::channel(BUS_CAPACITY);
        let http = HttpClient::new(&kelta_http::default_user_agent());
        let trackers: Arc<dyn ProviderFactory> =
            trackers.unwrap_or_else(|| Arc::new(kelta_trackers::TrackerFactory));
        let code_hosts: Arc<dyn ProviderFactory> =
            code_hosts.unwrap_or_else(|| Arc::new(kelta_codehosts::CodeHostFactory));
        let clip: Arc<dyn ClipboardBackend> =
            clipboard.unwrap_or_else(|| Arc::new(SystemClipboard::default()));
        let rt = Rt::new();
        let gauge = rt.timers.clone();

        let core = Arc::new_cyclic(|weak: &Weak<Core>| {
            let api: Weak<dyn CoreApi> = weak.clone();
            let work_store: Arc<dyn WorkStore> = store.clone();
            let grant_store: Arc<dyn GrantStore> = store.clone();
            let refresher: Weak<dyn scheduler::Refresher> = weak.clone();
            let plugins = PluginHost::new(api.clone(), dirs.clone(), grant_store);
            // plugin_tracker / plugin_codehost accounts are served by provider plugins (KPP)
            let kpp = |inner: &Arc<dyn ProviderFactory>| -> Arc<dyn ProviderFactory> {
                Arc::new(kelta_plugins::kpp::KppFactory::new(inner.clone(), Arc::downgrade(&plugins)))
            };
            Core {
                me: weak.clone(),
                work: WorkService::new(api.clone(), work_store, dirs.clone()),
                server: Server::new(api, dirs.clone()),
                providers: providers::ProviderRegistry::new(
                    http.clone(),
                    kpp(&trackers),
                    kpp(&code_hosts),
                    resolver.clone(),
                ),
                plugins,
                scheduler: scheduler::Scheduler::new(refresher, gauge),
                dirs,
                cli,
                bridge,
                config,
                cfg,
                secrets,
                resolver,
                terminal,
                login_env,
                http,
                trackers,
                code_hosts,
                store,
                bus,
                rt,
                clip,
                projects: Mutex::new(projects::ProjectsState::default()),
                layouts: Mutex::new(std::collections::HashMap::new()),
                sessions: Mutex::new(std::collections::BTreeMap::new()),
                sid8_taken: Mutex::default(),
                attention: Mutex::new(attention::AttentionState::default()),
                feeds: feeds::Feeds::default(),
                exits: tokio::sync::Notify::new(),
                quitting: AtomicBool::new(false),
                http_refs: std::sync::atomic::AtomicU32::new(0),
                last_reload: Mutex::new(None),
                claude_ver: Mutex::new(None),
                oauth: Mutex::default(),
                started: AtomicBool::new(false),
                start_services,
                install_ctl,
            }
        });
        core.boot()?;
        Ok(core)
    }

    /// Synchronous startup: state from SQLite (before window creation), config watch, Dormant
    /// sessions; async services are queued until the runtime is reachable.
    fn boot(self: &Arc<Self>) -> Result<(), KeltaError> {
        self.load_projects()?;
        self.load_layouts()?;
        self.load_sessions()?;
        self.adopt_live_sessions();
        let weak = self.me.clone();
        self.cfg.watch(Box::new(move |diff| {
            if let Some(core) = weak.upgrade() {
                core.on_settings_changed(diff);
            }
        }));
        self.work.set_host(Arc::new(CoreWorkHost(self.me.clone())));
        let settings: Arc<dyn SettingsSource> = self.cfg.clone();
        let config = self.config.clone();
        self.plugins.wire(Wiring {
            ui: Some(self.bridge.clone()),
            settings: Some(settings),
            settings_writer: Some(Arc::new(move |id: &kelta_proto::ids::PluginId, key: &str, value| {
                config.layer_set(Layer::Global, None, None, &format!("plugins.{id}.{key}"), value).map(|_| ())
            })),
        });
        self.sync_plugin_schemas();
        apply_ticket_key_regex(&self.cfg.effective(None));
        let weak = self.me.clone();
        self.rt.spawn(async move {
            if let Some(core) = weak.upgrade() {
                core.start_async().await;
            }
        });
        Ok(())
    }

    /// Async startup (runs once the runtime is reachable).
    async fn start_async(self: Arc<Self>) {
        if self.started.swap(true, Ordering::SeqCst) {
            return;
        }
        self.plugins.start();
        self.scheduler.run(&self.rt);
        self.resubscribe();
        if self.start_services {
            if let Err(e) = self.server.start_ctl().await {
                tracing::warn!(error = %e, "ctl socket not started");
            }
            if let Err(e) = self.work.startup().await {
                tracing::warn!(error = %e, "work startup hook failed");
            }
            let probe =
                ctl::probe_claude(self.login_env.clone(), self.cfg.effective(None).claude.clone()).await;
            *self.claude_ver.lock() = probe;
        }
        if self.install_ctl {
            let dirs = self.dirs.clone();
            let r = self.rt.blocking(move || ctl::install_stable_ctl(&dirs)).await;
            if let Err(e) = r {
                tracing::warn!(error = %e, "kelta-ctl stable copy not installed");
            }
        }
        self.publish(BusEvent::new(
            kelta_proto::events::bus::APP_STARTED,
            serde_json::json!({ "version": kelta_proto::VERSION }),
        ));
        self.eager_restore().await;
    }

    // ---- accessors used by commands -------------------------------------------------------------

    pub fn dirs(&self) -> &Dirs {
        &self.dirs
    }
    pub fn cli(&self) -> &CliArgs {
        &self.cli
    }
    pub fn bridge(&self) -> &Arc<dyn UiBridge> {
        &self.bridge
    }
    pub fn config(&self) -> &Arc<ConfigService> {
        &self.config
    }
    pub fn secrets(&self) -> &Arc<Secrets> {
        &self.secrets
    }
    pub fn secret_resolver(&self) -> Arc<dyn SecretResolver> {
        self.resolver.clone()
    }
    pub fn terminal(&self) -> Arc<dyn TerminalHost> {
        self.terminal.clone()
    }
    pub fn http(&self) -> &HttpClient {
        &self.http
    }
    pub fn tracker_factory(&self) -> &Arc<dyn ProviderFactory> {
        &self.trackers
    }
    pub fn code_host_factory(&self) -> &Arc<dyn ProviderFactory> {
        &self.code_hosts
    }
    pub fn work(&self) -> &Arc<WorkService> {
        &self.work
    }
    pub fn server(&self) -> &Arc<Server> {
        &self.server
    }
    pub fn plugins(&self) -> &Arc<PluginHost> {
        &self.plugins
    }
    /// The SQLite store (also `WorkStore`/`GrantStore`/`TrustStore`).
    pub fn store(&self) -> &Arc<Store> {
        &self.store
    }
    /// `TrustStore` view of the store (repo trust, used by settings commands).
    pub fn trust_store(&self) -> Arc<dyn TrustStore> {
        self.store.clone()
    }
    /// The settings source core reads (the `ConfigService` in production).
    pub fn settings_source(&self) -> Arc<dyn SettingsSource> {
        self.cfg.clone()
    }
    pub fn login_env(&self) -> &LoginEnv {
        &self.login_env
    }
    /// Hand the enabled plugins' settings schema fragments to kelta-config (Plugin-defaults
    /// layer). Called at boot and after plugin install / uninstall / enable.
    pub fn sync_plugin_schemas(&self) {
        self.config.set_plugin_schemas(self.plugins.fragments());
    }
    /// Scheduler state (subscriptions, armed deadline, paused accounts).
    pub fn scheduler_snapshot(&self) -> scheduler::SchedulerSnapshot {
        self.scheduler.snapshot()
    }

    /// `app_info`.
    pub fn app_info(&self) -> AppInfo {
        AppInfo {
            version: kelta_proto::VERSION.to_owned(),
            platform: kelta_proto::dirs::Os::current().as_str().to_owned(),
            arch: std::env::consts::ARCH.to_owned(),
            data_dir: self.dirs.data.clone(),
            config_dir: self.dirs.config.clone(),
            runtime_dir: self.dirs.runtime.clone(),
            claude: self.claude_ver.lock().clone(),
            safe_graphics: self.cli.safe_graphics,
            // The desktop shell resolves `auto` against the compositor.
            decorations: self.cfg.effective(None).window.decorations,
        }
    }

    /// `app_ready`: binds the runtime, kicks deferred startup work (the desktop shell clears its
    /// launch crash guard).
    pub async fn app_ready(&self, t_ms: f64) -> Result<(), KeltaError> {
        self.rt.capture();
        tracing::info!(t_ms, "app ready");
        self.on_window_changed();
        self.scheduler.kick(None);
        Ok(())
    }

    /// `open_external` (http/https/mailto only).
    pub async fn open_external(&self, url: &str) -> Result<(), KeltaError> {
        self.rt.capture();
        ctl::open_external(url).await
    }

    /// `notify_test`: bypasses the rules, reports the daemon error.
    pub async fn notify_test(&self) -> Result<(), KeltaError> {
        self.rt.capture();
        self.bridge.notify(Notification {
            title: "Kelta".into(),
            body: Some("Test notification".into()),
            urgency: kelta_proto::ext::Urgency::Normal,
            project_id: None,
            session_id: None,
        })
    }

    pub(crate) fn emit(&self, ev: UiEvent) {
        self.bridge.emit(ev);
    }

    /// Settings hot reload: terminal limits, providers, subscriptions, projects.
    pub(crate) fn on_settings_changed(&self, diff: SettingsDiff) {
        let settings = self.cfg.effective(None);
        self.terminal.set_limits(TerminalLimits::from_settings(&settings.terminal));
        self.emit(UiEvent::SettingsChanged {
            layers: diff.layers.clone(),
            paths: diff.paths.clone(),
            requires_restart: diff.requires_restart.clone(),
        });
        self.publish(BusEvent::new(
            kelta_proto::events::bus::SETTINGS_CHANGED,
            serde_json::json!({ "paths": diff.paths, "layers": diff.layers }),
        ));
        self.providers.invalidate_changed(&settings.accounts);
        apply_ticket_key_regex(&settings);
        self.scheduler.resume_all();
        self.emit_all_projects();
        self.resubscribe();
    }

    /// Window focus / visibility changed (bus `app.focus_changed` or `app_ready`).
    pub fn on_window_changed(&self) {
        let w = self.bridge.window_state();
        self.scheduler.window_changed(w);
    }

    /// Called by the desktop shell on quit: nvim mksession, persist Dormant sessions, SIGHUP →
    /// 2 s → SIGKILL.
    pub async fn shutdown(&self) -> Result<(), KeltaError> {
        self.rt.capture();
        self.quit_flow().await
    }
}

#[async_trait]
impl CoreApi for Core {
    fn login_path(&self) -> Option<String> {
        self.login_env.path().map(str::to_owned)
    }
    async fn session_spawn(&self, req: SpawnRequest) -> Result<SessionInfo, KeltaError> {
        self.rt.capture();
        self.spawn_session(req).await
    }
    async fn session_write(&self, id: &SessionId, bytes: &[u8]) -> Result<(), KeltaError> {
        self.write_session(id, bytes)
    }
    async fn session_kill(&self, id: &SessionId, force: bool) -> Result<(), KeltaError> {
        self.rt.capture();
        self.kill_session(id, force)
    }
    fn session_get(&self, id: &SessionId) -> Option<SessionInfo> {
        self.sessions.lock().get(id).map(|e| e.info.clone())
    }
    fn session_list(&self, project: Option<&ProjectId>) -> Vec<SessionInfo> {
        self.list_sessions(project)
    }
    async fn session_apply_hook(&self, id: &SessionId, change: StatusChange) -> Result<(), KeltaError> {
        self.rt.capture();
        self.apply_hook(id, change)
    }
    async fn layout_open(&self, project: &ProjectId, req: OpenPaneRequest) -> Result<PaneRef, KeltaError> {
        self.rt.capture();
        self.open_pane(project, req)
    }
    fn project(&self, id: &ProjectId) -> Option<ProjectInfo> {
        self.project_info(id)
    }
    fn settings(&self, project: Option<&ProjectId>) -> Arc<Settings> {
        self.cfg.effective(project)
    }
    async fn tracker_for(&self, account: &AccountId) -> Result<Arc<dyn Tracker>, KeltaError> {
        self.rt.capture();
        self.providers.tracker(account, &self.cfg.effective(None).accounts)
    }
    async fn code_host_for(&self, account: &AccountId) -> Result<Arc<dyn CodeHost>, KeltaError> {
        self.rt.capture();
        self.providers.code_host(account, &self.cfg.effective(None).accounts)
    }
    async fn review_list(&self, scope: Scope, kind: ReviewKind) -> Result<Vec<ReviewItem>, KeltaError> {
        self.rt.capture();
        Ok(self.review_page(scope, kind, false).await?.items)
    }
    async fn ticket_transition(
        &self,
        ticket: &TicketRef,
        transition_id: &str,
        fields: Option<serde_json::Value>,
        session: Option<&SessionId>,
    ) -> Result<Ticket, KeltaError> {
        self.tracker_transition(ticket, transition_id, fields, session).await
    }
    async fn ticket_comment(
        &self,
        ticket: &TicketRef,
        markdown: &str,
        session: Option<&SessionId>,
    ) -> Result<(), KeltaError> {
        self.tracker_comment(ticket, markdown, session).await
    }
    async fn work_for_session(&self, id: &SessionId) -> Option<WorkItem> {
        self.work.for_session(id).await
    }
    async fn work_create_pr(&self, id: &WorkItemId, draft: PrDraft) -> Result<WorkItem, KeltaError> {
        self.work.create_pr(id, draft).await
    }
    async fn work_feedback(&self, id: &WorkItemId) -> Result<kelta_proto::codehost::Feedback, KeltaError> {
        self.work.feedback(id).await
    }
    async fn editor_open(
        &self,
        target: EditorTarget,
        path: &Path,
        line: Option<u32>,
    ) -> Result<(), KeltaError> {
        self.work.editor_open(target, path, line).await
    }
    async fn editor_diff(
        &self,
        target: EditorTarget,
        old: &Path,
        proposed: &Path,
        close: bool,
    ) -> Result<(), KeltaError> {
        self.work.editor_diff(target, old, proposed, close).await
    }
    async fn tool_open(
        &self,
        project: &ProjectId,
        tool: &ToolId,
        ctx: TemplateCtx,
        placement: Placement,
    ) -> Result<ToolHandle, KeltaError> {
        self.plugins.tool_open(project, tool, ctx, placement).await
    }
    fn publish(&self, ev: BusEvent) {
        bus::publish(self, ev);
    }
    fn subscribe(&self) -> broadcast::Receiver<BusEvent> {
        self.bus.subscribe()
    }
    async fn notify(&self, n: Notification) -> Result<(), KeltaError> {
        self.rt.capture();
        self.notify_rules(n)
    }
    fn toast(&self, t: Toast) {
        self.emit(UiEvent::Toast { toast: t });
    }
    async fn http_fetch(&self, req: ProxiedRequest) -> Result<ProxiedResponse, KeltaError> {
        self.rt.capture();
        ctl::http_fetch(req).await
    }
    async fn ctl(&self, cmd: CtlCommand) -> Result<serde_json::Value, KeltaError> {
        self.rt.capture();
        self.dispatch_ctl(cmd).await
    }
}

/// `reviews.ticket_key_regex` → kelta-codehosts (process-wide; an invalid pattern keeps the previous).
fn apply_ticket_key_regex(settings: &Settings) {
    if let Err(e) = kelta_codehosts::set_ticket_key_regex(&settings.reviews.ticket_key_regex) {
        tracing::warn!(error = %e, "reviews.ticket_key_regex ignored");
    }
}

/// kelta-work's view of the HTTP server and the blocking trigger runner (weak: no cycle).
struct CoreWorkHost(Weak<Core>);

#[async_trait]
impl WorkHost for CoreWorkHost {
    async fn ensure_http(&self) -> Result<u16, KeltaError> {
        let core = self.0.upgrade().ok_or_else(|| KeltaError::internal("core is shutting down"))?;
        core.server.ensure_http().await
    }
    fn release_http(&self) {
        if let Some(core) = self.0.upgrade() {
            core.server.release_http();
        }
    }
    async fn run_blocking(&self, ev: &BusEvent) -> Result<BlockingOutcome, KeltaError> {
        let core = self.0.upgrade().ok_or_else(|| KeltaError::internal("core is shutting down"))?;
        core.plugins.run_blocking(ev).await
    }
}

/// Convenience: `Arc<Core>` as the `CoreApi` trait object.
pub fn as_api(core: &Arc<Core>) -> Arc<dyn CoreApi> {
    core.clone()
}

//! # kelta-core (L3)
//!
//! Composition root and `CoreApi` implementation: ProjectRegistry, SessionRegistry, layouts, Store
//! (SQLite + migrations, `WorkStore`/`GrantStore`/`TrustStore`), EventBus, attention, Scheduler,
//! ProviderRegistry + aggregation + caches + seen_reviews, notifier, clipboard, ctl dispatch,
//! perf snapshot.
//!
//! SCAFFOLD STUB: `Core::start` wires the stub services; every `CoreApi` method returns
//! `Unsupported("not implemented: <fn>")` (or an empty value) except `app_info`.

use std::path::Path;
use std::sync::{Arc, Weak};
use std::time::Duration;

use async_trait::async_trait;
use kelta_config::ConfigService;
use kelta_http::{HttpClient, ProviderFactory};
use kelta_plugins::PluginHost;
use kelta_proto::api::{
    CodeHost, CoreApi, GrantStore, PluginGrant, SecretResolver, SettingsSource, TerminalHost, Tracker,
    TrustStore, UiBridge, WorkStore,
};
use kelta_proto::codehost::{PrDraft, ReviewItem, ReviewKind};
use kelta_proto::ctl::CtlCommand;
use kelta_proto::dirs::{CliArgs, Dirs};
use kelta_proto::error::KeltaError;
use kelta_proto::events::{BUS_CAPACITY, BusEvent, Notification, Toast};
use kelta_proto::ext::{ProxiedRequest, ProxiedResponse, ToolHandle};
use kelta_proto::ids::{AccountId, PluginId, ProjectId, SessionId, ToolId, WorkItemId};
use kelta_proto::ipc::AppInfo;
use kelta_proto::model::{
    EditorTarget, OpenPaneRequest, PaneRef, Placement, ProjectInfo, Scope, SessionInfo, SpawnRequest,
    StatusChange, StepStatus, TemplateCtx, WorkItem, WorkStepStatus,
};
use kelta_proto::settings::{RuntimeOverrides, Settings};
use kelta_proto::term::TerminalLimits;
use kelta_secrets::Secrets;
use kelta_server::Server;
use kelta_term::PtyTerminalHost;
use kelta_work::WorkService;
use tokio::sync::broadcast;

/// Inputs of [`Core::start`].
pub struct CoreConfig {
    pub dirs: Dirs,
    pub cli: CliArgs,
    pub bridge: Arc<dyn UiBridge>,
}

/// The application core. Commands reach services through the accessors.
pub struct Core {
    dirs: Dirs,
    cli: CliArgs,
    bridge: Arc<dyn UiBridge>,
    config: Arc<ConfigService>,
    secrets: Arc<Secrets>,
    terminal: Arc<PtyTerminalHost>,
    http: HttpClient,
    trackers: Arc<dyn ProviderFactory>,
    code_hosts: Arc<dyn ProviderFactory>,
    work: Arc<WorkService>,
    server: Arc<Server>,
    plugins: Arc<PluginHost>,
    bus: broadcast::Sender<BusEvent>,
}

impl Core {
    /// Construct every service (each lane service receives `Weak<dyn CoreApi>`).
    pub fn start(cfg: CoreConfig) -> Result<Arc<Core>, KeltaError> {
        let CoreConfig { dirs, cli, bridge } = cfg;
        let overrides = RuntimeOverrides::from_env_and_cli(std::env::vars(), &cli);
        let config = ConfigService::load(&dirs, overrides)?;
        let settings_source: Arc<dyn SettingsSource> = config.clone();
        let secrets = Secrets::new(settings_source.clone());
        let settings = settings_source.effective(None);
        let terminal = PtyTerminalHost::new_arc(
            kelta_term::resolve_login_env(Duration::from_secs(3)),
            TerminalLimits::from_settings(&settings.terminal),
        );
        let store = Arc::new(NullStore);
        let (bus, _) = broadcast::channel(BUS_CAPACITY);
        let core = Arc::new_cyclic(|weak: &Weak<Core>| {
            let api: Weak<dyn CoreApi> = weak.clone();
            Core {
                work: WorkService::new(api.clone(), store.clone(), dirs.clone()),
                server: Server::new(api.clone(), dirs.clone()),
                plugins: PluginHost::new(api, dirs.clone(), store.clone()),
                dirs,
                cli,
                bridge,
                config,
                secrets,
                terminal,
                http: HttpClient::new(&kelta_http::default_user_agent()),
                trackers: Arc::new(kelta_trackers::TrackerFactory),
                code_hosts: Arc::new(kelta_codehosts::CodeHostFactory),
                bus,
            }
        });
        Ok(core)
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
        self.secrets.clone()
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

    /// `app_info` (functional in the scaffold).
    pub fn app_info(&self) -> AppInfo {
        AppInfo {
            version: kelta_proto::VERSION.to_owned(),
            platform: kelta_proto::dirs::Os::current().as_str().to_owned(),
            arch: std::env::consts::ARCH.to_owned(),
            data_dir: self.dirs.data.clone(),
            config_dir: self.dirs.config.clone(),
            runtime_dir: self.dirs.runtime.clone(),
            claude: None,
            safe_graphics: self.cli.safe_graphics,
        }
    }

    /// Called by the desktop shell on quit (L3: confirm, nvim mksession, persist Dormant sessions).
    pub async fn shutdown(&self) -> Result<(), KeltaError> {
        Ok(())
    }
}

fn ni<T>(f: &str) -> Result<T, KeltaError> {
    Err(KeltaError::not_implemented(f))
}

#[async_trait]
impl CoreApi for Core {
    async fn session_spawn(&self, _req: SpawnRequest) -> Result<SessionInfo, KeltaError> {
        ni("Core::session_spawn")
    }
    async fn session_write(&self, _id: &SessionId, _bytes: &[u8]) -> Result<(), KeltaError> {
        ni("Core::session_write")
    }
    async fn session_kill(&self, _id: &SessionId, _force: bool) -> Result<(), KeltaError> {
        ni("Core::session_kill")
    }
    fn session_get(&self, _id: &SessionId) -> Option<SessionInfo> {
        None
    }
    fn session_list(&self, _project: Option<&ProjectId>) -> Vec<SessionInfo> {
        Vec::new()
    }
    async fn session_apply_hook(&self, _id: &SessionId, _change: StatusChange) -> Result<(), KeltaError> {
        ni("Core::session_apply_hook")
    }
    async fn layout_open(&self, _project: &ProjectId, _req: OpenPaneRequest) -> Result<PaneRef, KeltaError> {
        ni("Core::layout_open")
    }
    fn project(&self, _id: &ProjectId) -> Option<ProjectInfo> {
        None
    }
    fn settings(&self, project: Option<&ProjectId>) -> Arc<Settings> {
        self.config.effective(project)
    }
    async fn tracker_for(&self, _account: &AccountId) -> Result<Arc<dyn Tracker>, KeltaError> {
        ni("Core::tracker_for")
    }
    async fn code_host_for(&self, _account: &AccountId) -> Result<Arc<dyn CodeHost>, KeltaError> {
        ni("Core::code_host_for")
    }
    async fn review_list(&self, _scope: Scope, _kind: ReviewKind) -> Result<Vec<ReviewItem>, KeltaError> {
        ni("Core::review_list")
    }
    async fn work_for_session(&self, id: &SessionId) -> Option<WorkItem> {
        self.work.for_session(id).await
    }
    async fn work_create_pr(&self, id: &WorkItemId, draft: PrDraft) -> Result<WorkItem, KeltaError> {
        self.work.create_pr(id, draft).await
    }
    async fn editor_open(
        &self,
        target: EditorTarget,
        path: &Path,
        line: Option<u32>,
    ) -> Result<(), KeltaError> {
        self.work.editor_open(target, path, line).await
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
        let _ = self.bus.send(ev);
    }
    fn subscribe(&self) -> broadcast::Receiver<BusEvent> {
        self.bus.subscribe()
    }
    async fn notify(&self, _n: Notification) -> Result<(), KeltaError> {
        ni("Core::notify")
    }
    fn toast(&self, t: Toast) {
        self.bridge.emit(kelta_proto::events::UiEvent::Toast { toast: t });
    }
    async fn http_fetch(&self, _req: ProxiedRequest) -> Result<ProxiedResponse, KeltaError> {
        ni("Core::http_fetch")
    }
    async fn ctl(&self, _cmd: CtlCommand) -> Result<serde_json::Value, KeltaError> {
        ni("Core::ctl")
    }
}

/// Placeholder store until L3's SQLite `Store` exists: reads are empty, writes Unsupported.
struct NullStore;

#[async_trait]
impl WorkStore for NullStore {
    async fn put_item(&self, _item: &WorkItem) -> Result<(), KeltaError> {
        ni("Store::put_item")
    }
    async fn get_item(&self, _id: &WorkItemId) -> Result<Option<WorkItem>, KeltaError> {
        Ok(None)
    }
    async fn list_items(&self, _project: Option<&ProjectId>) -> Result<Vec<WorkItem>, KeltaError> {
        Ok(Vec::new())
    }
    async fn delete_item(&self, _id: &WorkItemId) -> Result<(), KeltaError> {
        ni("Store::delete_item")
    }
    async fn set_step(
        &self,
        _id: &WorkItemId,
        _step: &str,
        _status: StepStatus,
        _detail: Option<String>,
    ) -> Result<(), KeltaError> {
        ni("Store::set_step")
    }
    async fn steps(&self, _id: &WorkItemId) -> Result<Vec<WorkStepStatus>, KeltaError> {
        Ok(Vec::new())
    }
}

#[async_trait]
impl GrantStore for NullStore {
    async fn grants(&self, _plugin: &PluginId) -> Result<Vec<PluginGrant>, KeltaError> {
        Ok(Vec::new())
    }
    async fn grant(&self, _plugin: &PluginId, _permissions: &[String], _sha: &str) -> Result<(), KeltaError> {
        ni("Store::grant")
    }
    async fn revoke_all(&self, _plugin: &PluginId) -> Result<(), KeltaError> {
        ni("Store::revoke_all")
    }
}

#[async_trait]
impl TrustStore for NullStore {
    async fn trusted_hash(&self, _path: &Path) -> Result<Option<String>, KeltaError> {
        Ok(None)
    }
    async fn set_trust(&self, _path: &Path, _sha256: Option<String>) -> Result<(), KeltaError> {
        ni("Store::set_trust")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kelta_proto::testing::FakeUiBridge;

    #[tokio::test]
    async fn start_wires_stubs() {
        let tmp = tempfile::tempdir().unwrap();
        let core = Core::start(CoreConfig {
            dirs: Dirs::under(tmp.path()),
            cli: CliArgs::default(),
            bridge: FakeUiBridge::new(),
        })
        .unwrap();
        assert_eq!(core.app_info().version, kelta_proto::VERSION);
        let e = core.session_list_err().await;
        assert_eq!(e.code, kelta_proto::ErrorCode::Unsupported);
        assert!(core.work().core().is_some());
    }

    impl Core {
        async fn session_list_err(&self) -> KeltaError {
            self.session_write(&SessionId::new("x"), b"").await.unwrap_err()
        }
    }
}

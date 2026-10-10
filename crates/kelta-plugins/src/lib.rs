//! # kelta-plugins (L8)
//!
//! Extensibility (PLUGINS.md): manifest load/validate/install, registry, grants, `plugin_call`
//! permission gate, `kelta-plugin://` handler, tools registry + web-tool lifecycle + header-stripping
//! proxy, trigger engine. Nothing here keeps a background runtime alive: the only long-lived task is
//! the bus subscription of the trigger engine (event-driven), plus per-web-tool readers that exist
//! only while a web tool runs, and provider (KPP) processes, spawned on first use (`kpp`).

pub mod actions;
mod context;
pub mod install;
pub mod kpp;
pub mod manifest;
pub mod matcher;
pub mod perms;
pub mod proxy;
pub mod registry;
mod screens;
pub mod template;
mod tools;
pub mod triggers;
pub mod uri;
pub mod util;
pub mod web;

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Weak};

use kelta_proto::api::{CoreApi, GrantStore, PluginSettingsSource, SettingsSource, UiBridge};
use kelta_proto::dirs::Dirs;
use kelta_proto::error::KeltaError;
use kelta_proto::events::{BusEvent, UiEvent};
use kelta_proto::ext::{
    BlockingOutcome, CallOrigin, CommandDef, PluginInfo, PluginInstallPreview, PluginMethod,
    ScreenOpenResult, ToolCheck, ToolHandle, ToolInfo, TriggerInfo, TriggerRun,
};
use kelta_proto::ids::{PluginId, ProjectId, ScreenInstanceId, ToolId, ToolInstanceId};
use kelta_proto::model::{Placement, TemplateCtx};
use kelta_proto::settings::Settings;
use parking_lot::{Mutex, RwLock};
use serde_json::Value;

use crate::perms::Granted;
use crate::registry::{Entry, Registry};

/// Writes `plugins.<id>.<key>` at the Global layer (wired by core to kelta-config).
pub type SettingsWriter = Arc<dyn Fn(&PluginId, &str, Value) -> Result<(), KeltaError> + Send + Sync>;

/// Optional host services that `CoreApi` does not expose. Core wires them at boot
/// (`Core::start`); every feature degrades gracefully without them.
#[derive(Clone, Default)]
pub struct Wiring {
    /// UI events (`plugin.event` relay to screens, web tool handles) and window focus.
    pub ui: Option<Arc<dyn UiBridge>>,
    /// Project configs (`projects.list` for global screens).
    pub settings: Option<Arc<dyn SettingsSource>>,
    /// `settings.set` of a plugin's own namespace.
    pub settings_writer: Option<SettingsWriter>,
}

pub struct PluginHost {
    core: Weak<dyn CoreApi>,
    dirs: Dirs,
    grants: Arc<dyn GrantStore>,
    me: Weak<PluginHost>,
    registry: RwLock<Option<Arc<Registry>>>,
    wiring: RwLock<Wiring>,
    /// Plugins activated by a command, a screen or a project open (PLUGINS §4 `activation`).
    activated: Mutex<HashSet<PluginId>>,
    screens: Mutex<HashMap<ScreenInstanceId, screens::Screen>>,
    tools: tools::State,
    engine: triggers::Engine,
    bus_task: Mutex<Option<tokio::task::AbortHandle>>,
    /// Provider (KPP) processes by plugin.
    kpp: Mutex<HashMap<PluginId, Arc<kpp::KppProcess>>>,
}

impl PluginHost {
    pub fn new(core: Weak<dyn CoreApi>, dirs: Dirs, grants: Arc<dyn GrantStore>) -> Arc<Self> {
        Arc::new_cyclic(|me| Self {
            core,
            dirs,
            grants,
            me: me.clone(),
            registry: RwLock::new(None),
            wiring: RwLock::new(Wiring::default()),
            activated: Mutex::new(HashSet::new()),
            screens: Mutex::new(HashMap::new()),
            tools: tools::State::default(),
            engine: triggers::Engine::default(),
            bus_task: Mutex::new(None),
            kpp: Mutex::new(HashMap::new()),
        })
    }

    pub fn core(&self) -> Option<Arc<dyn CoreApi>> {
        self.core.upgrade()
    }

    pub fn dirs(&self) -> &Dirs {
        &self.dirs
    }

    pub fn grant_store(&self) -> &Arc<dyn GrantStore> {
        &self.grants
    }

    /// Wire optional host services (fields left `None` keep their current value). Idempotent.
    pub fn wire(&self, w: Wiring) {
        let mut cur = self.wiring.write();
        if w.ui.is_some() {
            cur.ui = w.ui;
        }
        if w.settings.is_some() {
            cur.settings = w.settings;
        }
        if w.settings_writer.is_some() {
            cur.settings_writer = w.settings_writer;
        }
    }

    pub fn is_wired(&self) -> bool {
        self.wiring.read().ui.is_some()
    }

    /// Start the trigger engine's bus subscription (idempotent; needs a tokio runtime and a live
    /// core). Core calls it at startup; public entry points call it too, so tests need no core.
    pub fn start(&self) {
        let mut slot = self.bus_task.lock();
        if slot.as_ref().is_some_and(|t| !t.is_finished()) {
            return;
        }
        let Ok(rt) = tokio::runtime::Handle::try_current() else { return };
        let Some(core) = self.core() else { return };
        let mut rx = core.subscribe();
        drop(core);
        let me = self.me.clone();
        let task = rt.spawn(async move {
            loop {
                match rx.recv().await {
                    Ok(ev) => {
                        let Some(host) = me.upgrade() else { break };
                        host.on_event(&ev).await;
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                        tracing::warn!(skipped = n, "trigger engine lagged behind the bus");
                        let Some(host) = me.upgrade() else { break };
                        let marker = BusEvent::new(
                            kelta_proto::events::bus::LAGGED,
                            serde_json::json!({ "missed": n }),
                        );
                        host.handle_event(&marker).await;
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                }
            }
        });
        *slot = Some(task.abort_handle());
    }

    // ---- shared plumbing -----------------------------------------------------------------------

    pub(crate) fn core_or_err(&self) -> Result<Arc<dyn CoreApi>, KeltaError> {
        self.core().ok_or_else(|| KeltaError::internal("core is shutting down"))
    }

    pub(crate) fn settings(&self, project: Option<&ProjectId>) -> Arc<Settings> {
        self.core().map(|c| c.settings(project)).unwrap_or_else(|| Arc::new(Settings::defaults()))
    }

    pub(crate) fn wiring(&self) -> Wiring {
        self.wiring.read().clone()
    }

    pub(crate) fn emit_ui(&self, ev: UiEvent) {
        if let Some(ui) = self.wiring.read().ui.clone() {
            ui.emit(ev);
        }
    }

    /// Current registry (discovered lazily, refreshed after installs and settings changes).
    pub fn registry(&self) -> Arc<Registry> {
        if let Some(r) = self.registry.read().clone() {
            return r;
        }
        self.refresh()
    }

    /// Re-scan the plugin directories.
    pub fn refresh(&self) -> Arc<Registry> {
        let dev = self.settings(None).plugins.dev_paths.clone();
        let reg = Arc::new(registry::discover(&self.dirs.plugins_dir(), &dev));
        *self.registry.write() = Some(reg.clone());
        reg
    }

    pub(crate) fn invalidate(&self) {
        *self.registry.write() = None;
    }

    /// Enabled = not disabled in `<data>/plugins/.state.json` nor in `plugins.disabled`.
    pub fn is_enabled(&self, id: &PluginId) -> bool {
        let state = registry::load_state(&self.dirs.plugins_dir());
        if state.disabled.contains(id.as_str()) {
            return false;
        }
        !self.settings(None).plugins.disabled.contains(id)
    }

    /// Valid, compatible and enabled plugins.
    pub(crate) fn active(&self) -> Vec<Arc<Entry>> {
        let reg = self.registry();
        let state = registry::load_state(&self.dirs.plugins_dir());
        let disabled = self.settings(None).plugins.disabled.clone();
        reg.entries
            .iter()
            .filter(|e| e.loadable() && !state.disabled.contains(e.id.as_str()) && !disabled.contains(&e.id))
            .cloned()
            .collect()
    }

    pub(crate) fn active_entry(&self, id: &str) -> Result<Arc<Entry>, KeltaError> {
        let reg = self.registry();
        let e =
            reg.get(id).ok_or_else(|| KeltaError::not_found(format!("plugin `{id}` is not installed")))?;
        if !e.loadable() {
            return Err(KeltaError::invalid(format!(
                "plugin `{id}` is not loaded: {}",
                e.all_problems().join("; ")
            )));
        }
        if !self.is_enabled(&e.id) {
            return Err(KeltaError::invalid(format!("plugin `{id}` is disabled")));
        }
        Ok(e.clone())
    }

    /// Effective grants (`declared ∩ granted`).
    pub async fn granted(&self, entry: &Entry) -> Result<Granted, KeltaError> {
        let declared = entry.manifest().map(|m| m.permissions.clone()).unwrap_or_default();
        let grants = self.grants.grants(&entry.id).await?;
        Ok(Granted::new(&declared, &grants))
    }

    pub(crate) fn mark_activated(&self, id: &PluginId) {
        self.activated.lock().insert(id.clone());
    }

    pub(crate) fn is_activated(&self, id: &PluginId) -> bool {
        self.activated.lock().contains(id)
    }

    // ---- tools ---------------------------------------------------------------------------------

    /// `tool_list`.
    pub async fn tools(&self, project: &ProjectId) -> Result<Vec<ToolInfo>, KeltaError> {
        self.start();
        Ok(self.tool_infos(project))
    }

    /// `tool_open`.
    pub async fn tool_open(
        &self,
        project: &ProjectId,
        tool: &ToolId,
        ctx: TemplateCtx,
        placement: Placement,
    ) -> Result<ToolHandle, KeltaError> {
        self.start();
        self.open_tool(project, tool, ctx, placement).await
    }

    /// `tool_close`.
    pub async fn tool_close(&self, instance: &ToolInstanceId) -> Result<(), KeltaError> {
        self.close_tool(instance).await
    }

    /// `tool_check`.
    pub async fn tool_check(&self, tool: &ToolId) -> Result<ToolCheck, KeltaError> {
        self.start();
        self.check_tool(tool).await
    }

    // ---- plugins -------------------------------------------------------------------------------

    /// `plugin_list`.
    pub async fn plugins(&self) -> Result<Vec<PluginInfo>, KeltaError> {
        self.start();
        let reg = self.refresh();
        let mut out = Vec::with_capacity(reg.entries.len());
        for e in &reg.entries {
            out.push(self.info(e).await?);
        }
        Ok(out)
    }

    pub(crate) async fn info(&self, e: &Entry) -> Result<PluginInfo, KeltaError> {
        let granted = if e.parsed.is_some() { self.granted(e).await?.strings() } else { Vec::new() };
        let mut problems = e.all_problems();
        if let Some(m) = e.manifest() {
            let missing: Vec<&String> = m.permissions.iter().filter(|p| !granted.contains(p)).collect();
            if !missing.is_empty() {
                problems.push(format!(
                    "permissions not granted: {}",
                    missing.iter().map(|s| s.as_str()).collect::<Vec<_>>().join(", ")
                ));
            }
        }
        Ok(match e.manifest() {
            Some(m) => PluginInfo {
                id: m.id.clone(),
                name: m.name.clone(),
                version: m.version.clone(),
                description: m.description.clone(),
                enabled: self.is_enabled(&e.id),
                permissions: m.permissions.clone(),
                granted,
                problems,
                dir: e.dir.clone(),
                dev: e.dev,
                contributes: m.contributes.clone(),
            },
            None => PluginInfo {
                id: e.id.clone(),
                name: e.id.to_string(),
                version: String::new(),
                description: String::new(),
                enabled: self.is_enabled(&e.id),
                permissions: Vec::new(),
                granted,
                problems,
                dir: e.dir.clone(),
                dev: e.dev,
                contributes: Default::default(),
            },
        })
    }

    /// `plugin_inspect` (dir | git url | tar path).
    pub async fn inspect(&self, source: &str) -> Result<PluginInstallPreview, KeltaError> {
        self.start();
        install::inspect(self, source).await
    }

    /// `plugin_install`.
    pub async fn install(
        &self,
        source: &str,
        sha256: &str,
        grant: Vec<String>,
    ) -> Result<PluginInfo, KeltaError> {
        self.start();
        install::install(self, source, sha256, grant).await
    }

    /// `plugin_uninstall`.
    pub async fn uninstall(&self, id: &PluginId) -> Result<(), KeltaError> {
        self.start();
        install::uninstall(self, id).await
    }

    /// `plugin_enable`.
    pub async fn enable(&self, id: &PluginId, enabled: bool) -> Result<(), KeltaError> {
        self.start();
        let reg = self.registry();
        if reg.get(id.as_str()).is_none() {
            return Err(KeltaError::not_found(format!("plugin `{id}` is not installed")));
        }
        if enabled && self.settings(None).plugins.disabled.contains(id) {
            return Err(KeltaError::conflict(format!(
                "plugin `{id}` is disabled by `plugins.disabled` in config.toml"
            )));
        }
        let dir = self.dirs.plugins_dir();
        let mut state = registry::load_state(&dir);
        if enabled {
            state.disabled.remove(id.as_str());
        } else {
            state.disabled.insert(id.to_string());
        }
        registry::save_state(&dir, &state)?;
        if !enabled {
            self.deactivate(id).await;
        }
        self.invalidate();
        Ok(())
    }

    /// Close screens and stop web tools and the provider process of a plugin that is disabled or removed.
    pub(crate) async fn deactivate(&self, id: &PluginId) {
        self.kpp_stop(id);
        self.screens.lock().retain(|_, s| &s.plugin != id);
        self.activated.lock().remove(id);
        self.close_plugin_tools(id).await;
    }

    /// `plugin_grant`: sets the exact granted set (must be declared by the manifest).
    pub async fn grant(&self, id: &PluginId, permissions: Vec<String>) -> Result<PluginInfo, KeltaError> {
        self.start();
        let reg = self.registry();
        let e = reg
            .get(id.as_str())
            .ok_or_else(|| KeltaError::not_found(format!("plugin `{id}` is not installed")))?;
        let m = e
            .manifest()
            .ok_or_else(|| KeltaError::invalid(format!("plugin `{id}` has an invalid manifest")))?;
        if let Some(bad) = permissions.iter().find(|p| !m.permissions.contains(p)) {
            return Err(KeltaError::invalid(format!("`{bad}` is not requested by the plugin manifest")));
        }
        self.grants.revoke_all(id).await?;
        self.kpp_stop(id);
        if !permissions.is_empty() {
            self.grants.grant(id, &permissions, e.sha256()).await?;
        }
        self.info(e).await
    }

    // ---- screens -------------------------------------------------------------------------------

    /// `plugin_screen_open`.
    pub async fn screen_open(
        &self,
        plugin: &PluginId,
        screen_id: &str,
        project: Option<&ProjectId>,
        params: Value,
    ) -> Result<ScreenOpenResult, KeltaError> {
        self.start();
        self.open_screen(plugin, screen_id, project, params)
    }

    /// `plugin_screen_close`.
    pub async fn screen_close(&self, instance: &ScreenInstanceId) -> Result<(), KeltaError> {
        self.screens.lock().remove(instance);
        Ok(())
    }

    /// `plugin_call` (permission-gated, PLUGINS §5/§7).
    pub async fn call(
        &self,
        instance: &ScreenInstanceId,
        method: PluginMethod,
        params: Value,
        caller: CallOrigin,
    ) -> Result<Value, KeltaError> {
        self.start();
        self.dispatch_call(instance, method, params, caller).await
    }

    // ---- triggers / commands -------------------------------------------------------------------

    /// `trigger_list`.
    pub async fn triggers(&self, project: Option<&ProjectId>) -> Result<Vec<TriggerInfo>, KeltaError> {
        self.start();
        Ok(self.resolve_triggers(project).into_iter().map(|t| t.info()).collect())
    }

    /// `trigger_test`.
    pub async fn trigger_test(&self, trigger_id: &str, payload: Value) -> Result<TriggerRun, KeltaError> {
        self.start();
        self.test_trigger(trigger_id, payload).await
    }

    /// `trigger_log`.
    pub async fn trigger_log(&self, limit: u32) -> Result<Vec<TriggerRun>, KeltaError> {
        self.start();
        Ok(self.engine.log(limit as usize))
    }

    /// Bus event fan-in for the trigger engine (non-blocking events). Safe to call in addition to
    /// the host's own bus subscription: duplicates are dropped.
    pub async fn on_event(&self, ev: &BusEvent) {
        self.start();
        self.handle_event(ev).await;
    }

    /// Blocking pre-events (`*.before_*`).
    pub async fn run_blocking(&self, ev: &BusEvent) -> Result<BlockingOutcome, KeltaError> {
        self.start();
        self.blocking(ev).await
    }

    /// Contributed + configured palette commands.
    pub async fn commands(&self) -> Result<Vec<CommandDef>, KeltaError> {
        self.start();
        Ok(self.command_defs(None).into_iter().map(|c| c.def).collect())
    }

    /// `command_run`.
    pub async fn command_run(&self, command_id: &str, ctx: TemplateCtx) -> Result<(), KeltaError> {
        self.start();
        self.run_command(command_id, ctx).await
    }
}

impl PluginSettingsSource for PluginHost {
    fn fragments(&self) -> Vec<(PluginId, Value)> {
        self.active()
            .into_iter()
            .filter_map(|e| e.settings_schema.clone().map(|s| (e.id.clone(), s)))
            .collect()
    }
}

impl Drop for PluginHost {
    fn drop(&mut self) {
        if let Some(t) = self.bus_task.lock().take() {
            t.abort();
        }
        self.tools.kill_all();
        for p in self.kpp.lock().values() {
            p.kill();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kelta_proto::testing::{FakeCore, MemGrantStore};

    #[tokio::test]
    async fn empty_host_lists_nothing() {
        let core = FakeCore::new();
        let weak: Weak<dyn CoreApi> = Arc::downgrade(&(core.clone() as Arc<dyn CoreApi>));
        let tmp = tempfile::tempdir().unwrap();
        let host = PluginHost::new(weak, Dirs::under(tmp.path()), Arc::new(MemGrantStore::new()));
        assert!(host.plugins().await.unwrap().is_empty());
        assert!(host.fragments().is_empty());
        let resp = uri::handle(&host, axum::http::Request::new(Vec::new()));
        assert_eq!(resp.status(), axum::http::StatusCode::NOT_FOUND);
        let _ = proxy::router();
    }
}

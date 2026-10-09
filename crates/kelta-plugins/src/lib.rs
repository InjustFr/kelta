//! # kelta-plugins (L8)
//!
//! Extensibility (PLUGINS.md): manifest load/validate/install, registry, grants, `plugin_call`
//! permission gate, `kelta-plugin://` handler, tools registry + web-tool lifecycle + header-stripping
//! proxy, trigger engine.
//!
//! SCAFFOLD STUB: every fallible method returns `Unsupported("not implemented: <fn>")`.

use std::sync::{Arc, Weak};

use kelta_proto::api::{CoreApi, GrantStore, PluginSettingsSource};
use kelta_proto::dirs::Dirs;
use kelta_proto::error::KeltaError;
use kelta_proto::events::BusEvent;
use kelta_proto::ext::{
    BlockingOutcome, CallOrigin, CommandDef, PluginInfo, PluginInstallPreview, PluginMethod,
    ScreenOpenResult, ToolCheck, ToolHandle, ToolInfo, TriggerInfo, TriggerRun,
};
use kelta_proto::ids::{PluginId, ProjectId, ScreenInstanceId, ToolId, ToolInstanceId};
use kelta_proto::model::{Placement, TemplateCtx};
use serde_json::Value;

pub struct PluginHost {
    core: Weak<dyn CoreApi>,
    dirs: Dirs,
    grants: Arc<dyn GrantStore>,
}

impl PluginHost {
    pub fn new(core: Weak<dyn CoreApi>, dirs: Dirs, grants: Arc<dyn GrantStore>) -> Arc<Self> {
        Arc::new(Self { core, dirs, grants })
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

    // ---- tools ---------------------------------------------------------------------------------

    /// `tool_list`.
    pub async fn tools(&self, _project: &ProjectId) -> Result<Vec<ToolInfo>, KeltaError> {
        Err(KeltaError::not_implemented("PluginHost::tools"))
    }

    /// `tool_open`.
    pub async fn tool_open(
        &self,
        _project: &ProjectId,
        _tool: &ToolId,
        _ctx: TemplateCtx,
        _placement: Placement,
    ) -> Result<ToolHandle, KeltaError> {
        Err(KeltaError::not_implemented("PluginHost::tool_open"))
    }

    /// `tool_close`.
    pub async fn tool_close(&self, _instance: &ToolInstanceId) -> Result<(), KeltaError> {
        Err(KeltaError::not_implemented("PluginHost::tool_close"))
    }

    /// `tool_check`.
    pub async fn tool_check(&self, _tool: &ToolId) -> Result<ToolCheck, KeltaError> {
        Err(KeltaError::not_implemented("PluginHost::tool_check"))
    }

    // ---- plugins -------------------------------------------------------------------------------

    /// `plugin_list`.
    pub async fn plugins(&self) -> Result<Vec<PluginInfo>, KeltaError> {
        Err(KeltaError::not_implemented("PluginHost::plugins"))
    }

    /// `plugin_inspect` (dir | git url | tar path).
    pub async fn inspect(&self, _source: &str) -> Result<PluginInstallPreview, KeltaError> {
        Err(KeltaError::not_implemented("PluginHost::inspect"))
    }

    /// `plugin_install`.
    pub async fn install(
        &self,
        _source: &str,
        _sha256: &str,
        _grant: Vec<String>,
    ) -> Result<PluginInfo, KeltaError> {
        Err(KeltaError::not_implemented("PluginHost::install"))
    }

    /// `plugin_uninstall`.
    pub async fn uninstall(&self, _id: &PluginId) -> Result<(), KeltaError> {
        Err(KeltaError::not_implemented("PluginHost::uninstall"))
    }

    /// `plugin_enable`.
    pub async fn enable(&self, _id: &PluginId, _enabled: bool) -> Result<(), KeltaError> {
        Err(KeltaError::not_implemented("PluginHost::enable"))
    }

    /// `plugin_grant`.
    pub async fn grant(&self, _id: &PluginId, _permissions: Vec<String>) -> Result<PluginInfo, KeltaError> {
        Err(KeltaError::not_implemented("PluginHost::grant"))
    }

    // ---- screens -------------------------------------------------------------------------------

    /// `plugin_screen_open`.
    pub async fn screen_open(
        &self,
        _plugin: &PluginId,
        _screen_id: &str,
        _project: Option<&ProjectId>,
        _params: Value,
    ) -> Result<ScreenOpenResult, KeltaError> {
        Err(KeltaError::not_implemented("PluginHost::screen_open"))
    }

    /// `plugin_screen_close`.
    pub async fn screen_close(&self, _instance: &ScreenInstanceId) -> Result<(), KeltaError> {
        Err(KeltaError::not_implemented("PluginHost::screen_close"))
    }

    /// `plugin_call` (permission-gated, PLUGINS §5/§7).
    pub async fn call(
        &self,
        _instance: &ScreenInstanceId,
        _method: PluginMethod,
        _params: Value,
        _caller: CallOrigin,
    ) -> Result<Value, KeltaError> {
        Err(KeltaError::not_implemented("PluginHost::call"))
    }

    // ---- triggers / commands -------------------------------------------------------------------

    /// `trigger_list`.
    pub async fn triggers(&self, _project: Option<&ProjectId>) -> Result<Vec<TriggerInfo>, KeltaError> {
        Err(KeltaError::not_implemented("PluginHost::triggers"))
    }

    /// `trigger_test`.
    pub async fn trigger_test(&self, _trigger_id: &str, _payload: Value) -> Result<TriggerRun, KeltaError> {
        Err(KeltaError::not_implemented("PluginHost::trigger_test"))
    }

    /// `trigger_log`.
    pub async fn trigger_log(&self, _limit: u32) -> Result<Vec<TriggerRun>, KeltaError> {
        Err(KeltaError::not_implemented("PluginHost::trigger_log"))
    }

    /// Bus event fan-in for the trigger engine (non-blocking events).
    pub async fn on_event(&self, _ev: &BusEvent) {}

    /// Blocking pre-events (`*.before_*`).
    pub async fn run_blocking(&self, _ev: &BusEvent) -> Result<BlockingOutcome, KeltaError> {
        Ok(BlockingOutcome::Proceed { patch: None })
    }

    /// Contributed + configured palette commands.
    pub async fn commands(&self) -> Result<Vec<CommandDef>, KeltaError> {
        Err(KeltaError::not_implemented("PluginHost::commands"))
    }

    /// `command_run`.
    pub async fn command_run(&self, _command_id: &str, _ctx: TemplateCtx) -> Result<(), KeltaError> {
        Err(KeltaError::not_implemented("PluginHost::command_run"))
    }
}

impl PluginSettingsSource for PluginHost {
    fn fragments(&self) -> Vec<(PluginId, Value)> {
        Vec::new()
    }
}

/// `kelta-plugin://<id>/<path>` scheme handler (canonicalized files inside the plugin dir + CSP).
pub mod uri {
    use axum::http::{Request, Response, StatusCode, header};

    use super::PluginHost;

    /// Content-Security-Policy for plugin screens (ARCHITECTURE §11.3).
    pub fn csp(plugin_id: &str) -> String {
        format!(
            "default-src 'self' kelta-plugin://{plugin_id}; script-src kelta-plugin://{plugin_id}; \
             style-src kelta-plugin://{plugin_id} 'unsafe-inline'; img-src kelta-plugin://{plugin_id} data:; \
             connect-src 'none'; frame-ancestors 'self'"
        )
    }

    /// Serve a request. Stub: 501.
    pub fn handle(_host: &PluginHost, _req: Request<Vec<u8>>) -> Response<Vec<u8>> {
        let mut resp = Response::new(b"not implemented: uri::handle".to_vec());
        *resp.status_mut() = StatusCode::NOT_IMPLEMENTED;
        resp.headers_mut()
            .insert(header::CONTENT_TYPE, header::HeaderValue::from_static("text/plain; charset=utf-8"));
        resp
    }
}

/// Header-stripping reverse proxy for web tools, mounted by kelta-server at `/proxy/`.
pub mod proxy {
    use axum::Router;
    use axum::http::StatusCode;
    use axum::routing::any;

    /// Stub: every request answers 501.
    pub fn router() -> Router {
        Router::new()
            .fallback(any(|| async { (StatusCode::NOT_IMPLEMENTED, "not implemented: proxy::router") }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kelta_proto::testing::{FakeCore, MemGrantStore};

    #[tokio::test]
    async fn stub_is_unsupported() {
        let core = FakeCore::new();
        let weak: Weak<dyn CoreApi> = Arc::downgrade(&(core.clone() as Arc<dyn CoreApi>));
        let host = PluginHost::new(weak, Dirs::under(&std::env::temp_dir()), Arc::new(MemGrantStore::new()));
        assert_eq!(host.plugins().await.unwrap_err().code, kelta_proto::ErrorCode::Unsupported);
        let resp = uri::handle(&host, axum::http::Request::new(Vec::new()));
        assert_eq!(resp.status(), axum::http::StatusCode::NOT_IMPLEMENTED);
        let _ = proxy::router();
    }
}

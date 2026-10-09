//! # kelta-config (L4)
//!
//! Layered settings (SETTINGS.md): load/merge/provenance, schema + semantic validation,
//! comment-preserving `toml_edit` writes, hot reload, repo trust helpers, project file CRUD and
//! the early `[linux.graphics]` reader.
//!
//! SCAFFOLD STUB: `load` returns compiled defaults only; mutating methods return
//! `Unsupported("not implemented: <fn>")`. `schema()` is functional.

use std::path::Path;
use std::sync::Arc;

use kelta_proto::api::SettingsSource;
use kelta_proto::dirs::Dirs;
use kelta_proto::error::KeltaError;
use kelta_proto::ids::{PluginId, ProjectId};
use kelta_proto::model::{ProjectDraft, ProjectPatch};
use kelta_proto::settings::{
    EffectiveSettings, Layer, LayerDoc, ProjectConfig, RuntimeOverrides, Settings, SettingsDiff,
    ValidationIssue,
};
use parking_lot::RwLock;
use serde_json::Value;

/// Change callback registered with [`ConfigService::watch`].
pub type OnChange = Box<dyn Fn(SettingsDiff) + Send + Sync>;

pub struct ConfigService {
    dirs: Dirs,
    overrides: RuntimeOverrides,
    global: RwLock<Arc<Settings>>,
    plugin_schemas: RwLock<Vec<(PluginId, Value)>>,
    on_change: RwLock<Vec<OnChange>>,
}

impl ConfigService {
    /// Load every layer. Stub: compiled defaults only.
    pub fn load(dirs: &Dirs, overrides: RuntimeOverrides) -> Result<Arc<Self>, KeltaError> {
        Ok(Arc::new(Self {
            dirs: dirs.clone(),
            overrides,
            global: RwLock::new(Arc::new(Settings::defaults())),
            plugin_schemas: RwLock::new(Vec::new()),
            on_change: RwLock::new(Vec::new()),
        }))
    }

    pub fn dirs(&self) -> &Dirs {
        &self.dirs
    }

    pub fn overrides(&self) -> &RuntimeOverrides {
        &self.overrides
    }

    /// Register a change callback (invoked after a successful hot reload or write).
    pub fn watch(&self, on_change: OnChange) {
        self.on_change.write().push(on_change);
    }

    /// Plugin-defaults layer + validation of `plugins.<id>` (from `PluginSettingsSource`).
    pub fn set_plugin_schemas(&self, fragments: Vec<(PluginId, Value)>) {
        *self.plugin_schemas.write() = fragments;
    }

    /// `settings_effective`: merged value + winning layer per dotted path.
    pub fn effective_doc(&self, _project: Option<&ProjectId>) -> Result<EffectiveSettings, KeltaError> {
        Err(KeltaError::not_implemented("ConfigService::effective_doc"))
    }

    /// `settings_layer_get`.
    pub fn layer_get(
        &self,
        _layer: Layer,
        _project: Option<&ProjectId>,
        _repo_id: Option<&str>,
    ) -> Result<LayerDoc, KeltaError> {
        Err(KeltaError::not_implemented("ConfigService::layer_get"))
    }

    /// `settings_set`: write one dotted key at a layer (toml_edit, atomic).
    pub fn layer_set(
        &self,
        _layer: Layer,
        _project: Option<&ProjectId>,
        _repo_id: Option<&str>,
        _path: &str,
        _value: Value,
    ) -> Result<EffectiveSettings, KeltaError> {
        Err(KeltaError::not_implemented("ConfigService::layer_set"))
    }

    /// `settings_reset`: remove one dotted key at a layer.
    pub fn layer_reset(
        &self,
        _layer: Layer,
        _project: Option<&ProjectId>,
        _repo_id: Option<&str>,
        _path: &str,
    ) -> Result<EffectiveSettings, KeltaError> {
        Err(KeltaError::not_implemented("ConfigService::layer_reset"))
    }

    /// `settings_validate`: validate a whole TOML document for a layer.
    pub fn layer_validate(&self, _layer: Layer, _text: &str) -> Result<Vec<ValidationIssue>, KeltaError> {
        Err(KeltaError::not_implemented("ConfigService::layer_validate"))
    }

    /// `settings_write_raw`: validate then replace a layer file.
    pub fn layer_write_raw(
        &self,
        _layer: Layer,
        _project: Option<&ProjectId>,
        _repo_id: Option<&str>,
        _text: &str,
    ) -> Result<EffectiveSettings, KeltaError> {
        Err(KeltaError::not_implemented("ConfigService::layer_write_raw"))
    }

    /// Writes `projects/<id>.toml`.
    pub fn project_create(&self, _draft: &ProjectDraft) -> Result<Arc<ProjectConfig>, KeltaError> {
        Err(KeltaError::not_implemented("ConfigService::project_create"))
    }

    pub fn project_update(
        &self,
        _id: &ProjectId,
        _patch: &ProjectPatch,
    ) -> Result<Arc<ProjectConfig>, KeltaError> {
        Err(KeltaError::not_implemented("ConfigService::project_update"))
    }

    /// Moves the file to `projects/.trash/`.
    pub fn project_remove(&self, _id: &ProjectId) -> Result<(), KeltaError> {
        Err(KeltaError::not_implemented("ConfigService::project_remove"))
    }

    /// Flattened settings JSON Schema (incl. `plugins.<id>` fragments in L4).
    pub fn schema() -> Value {
        kelta_proto::schema::settings_schema()
    }
}

impl SettingsSource for ConfigService {
    fn effective(&self, _project: Option<&ProjectId>) -> Arc<Settings> {
        self.global.read().clone()
    }

    fn project(&self, _id: &ProjectId) -> Option<Arc<ProjectConfig>> {
        None
    }

    fn projects(&self) -> Vec<Arc<ProjectConfig>> {
        Vec::new()
    }
}

/// Reads needed before any GTK/WebKit init (single-threaded `platform::pre_init`).
pub mod early {
    use super::Path;
    use kelta_proto::settings::LinuxGraphics;

    /// `[linux.graphics]` from `<config_dir>/config.toml` with a minimal parse. Stub: defaults.
    pub fn linux_graphics(_config_dir: &Path) -> LinuxGraphics {
        LinuxGraphics::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stub_loads_defaults() {
        let tmp = std::env::temp_dir();
        let svc = ConfigService::load(&Dirs::under(&tmp), RuntimeOverrides::default()).unwrap();
        assert_eq!(svc.effective(None).terminal.max_live_views, 4);
        assert!(ConfigService::schema().is_object());
        assert_eq!(
            svc.layer_get(Layer::Global, None, None).unwrap_err().code,
            kelta_proto::ErrorCode::Unsupported
        );
    }
}

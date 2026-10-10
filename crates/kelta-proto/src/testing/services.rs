//! `FakeSecrets`, `FakeSettings`, `FakeUiBridge`.

use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use parking_lot::Mutex;

use crate::api::{SecretResolver, SettingsSource, UiBridge};
use crate::error::KeltaError;
use crate::events::{Notification, UiEvent};
use crate::ids::ProjectId;
use crate::ipc::WindowState;
use crate::secret::{Secret, SecretBackendStatus, SecretCtx, SecretRef};
use crate::settings::{ProjectConfig, ProjectFile, Settings};

/// Map-backed secrets; every ref kind resolves from the map (keyed by the full ref string).
#[derive(Default)]
pub struct FakeSecrets {
    values: Mutex<HashMap<String, String>>,
    resolved: Mutex<Vec<String>>,
    invalidated: Mutex<Vec<String>>,
    failing_sets: Mutex<Vec<String>>,
}

impl FakeSecrets {
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    pub fn with(pairs: &[(&str, &str)]) -> Arc<Self> {
        let s = Self::default();
        for (k, v) in pairs {
            s.values.lock().insert((*k).to_owned(), (*v).to_owned());
        }
        Arc::new(s)
    }

    pub fn insert(&self, r: &str, value: &str) {
        self.values.lock().insert(r.to_owned(), value.to_owned());
    }

    /// Refs resolved so far (never values).
    pub fn resolved(&self) -> Vec<String> {
        self.resolved.lock().clone()
    }

    pub fn invalidated(&self) -> Vec<String> {
        self.invalidated.lock().clone()
    }

    /// Make every later `set` of `r` fail (a locked keychain).
    pub fn fail_set(&self, r: &str) {
        self.failing_sets.lock().push(r.to_owned());
    }
}

#[async_trait]
impl SecretResolver for FakeSecrets {
    async fn resolve(&self, r: &SecretRef, _ctx: &SecretCtx) -> Result<Secret, KeltaError> {
        self.resolved.lock().push(r.0.clone());
        self.values
            .lock()
            .get(&r.0)
            .map(|v| Secret::new(v.clone()))
            .ok_or_else(|| KeltaError::needs_auth(format!("no secret for {r}")))
    }

    async fn set(&self, r: &SecretRef, value: &str) -> Result<(), KeltaError> {
        if !r.is_keyring() {
            return Err(KeltaError::invalid("only keyring: refs can be set"));
        }
        if self.failing_sets.lock().contains(&r.0) {
            return Err(KeltaError::internal(format!("cannot write {r}")));
        }
        self.values.lock().insert(r.0.clone(), value.to_owned());
        Ok(())
    }

    async fn delete(&self, r: &SecretRef) -> Result<(), KeltaError> {
        self.values.lock().remove(&r.0);
        Ok(())
    }

    async fn backends_status(&self) -> Vec<SecretBackendStatus> {
        vec![SecretBackendStatus { backend: "fake".into(), available: true, detail: None }]
    }

    fn invalidate(&self, r: &SecretRef) {
        self.invalidated.lock().push(r.0.clone());
    }
}

/// Settings from TOML strings (global + optional project files). Projects use their
/// `[project]` table; project overrides are applied by replacing whole top-level sections
/// present in the project file (a simplification of the real layered merge).
pub struct FakeSettings {
    global: Mutex<Arc<Settings>>,
    projects: Mutex<Vec<(Arc<ProjectConfig>, Arc<Settings>)>>,
}

impl FakeSettings {
    pub fn defaults() -> Arc<Self> {
        Arc::new(Self {
            global: Mutex::new(Arc::new(Settings::defaults())),
            projects: Mutex::new(Vec::new()),
        })
    }

    /// Parse a global `config.toml` (missing keys = defaults).
    pub fn from_toml(text: &str) -> Result<Arc<Self>, KeltaError> {
        let s: Settings =
            toml::from_str(text).map_err(|e| KeltaError::invalid(format!("settings toml: {e}")))?;
        Ok(Arc::new(Self { global: Mutex::new(Arc::new(s)), projects: Mutex::new(Vec::new()) }))
    }

    /// Add a project from a `projects/<id>.toml` text.
    pub fn add_project_toml(&self, text: &str) -> Result<(), KeltaError> {
        let file: ProjectFile =
            toml::from_str(text).map_err(|e| KeltaError::invalid(format!("project toml: {e}")))?;
        let raw: toml::Table =
            toml::from_str(text).map_err(|e| KeltaError::invalid(format!("project toml: {e}")))?;
        let mut merged = serde_json::to_value(&*self.global.lock().clone())?;
        let overrides = serde_json::to_value(&file.overrides)?;
        if let (Some(m), Some(o)) = (merged.as_object_mut(), overrides.as_object()) {
            for key in raw.keys().filter(|k| k.as_str() != "project") {
                if let Some(v) = o.get(key) {
                    m.insert(key.clone(), v.clone());
                }
            }
        }
        let merged: Settings = serde_json::from_value(merged)?;
        self.projects.lock().push((Arc::new(file.project), Arc::new(merged)));
        Ok(())
    }

    pub fn set_global(&self, s: Settings) {
        *self.global.lock() = Arc::new(s);
    }
}

impl SettingsSource for FakeSettings {
    fn effective(&self, project: Option<&ProjectId>) -> Arc<Settings> {
        if let Some(p) = project
            && let Some((_, s)) = self.projects.lock().iter().find(|(c, _)| &c.id == p)
        {
            return s.clone();
        }
        self.global.lock().clone()
    }

    fn project(&self, id: &ProjectId) -> Option<Arc<ProjectConfig>> {
        self.projects.lock().iter().find(|(c, _)| &c.id == id).map(|(c, _)| c.clone())
    }

    fn projects(&self) -> Vec<Arc<ProjectConfig>> {
        self.projects.lock().iter().map(|(c, _)| c.clone()).collect()
    }
}

/// Records everything core sends to the UI.
pub struct FakeUiBridge {
    events: Mutex<Vec<UiEvent>>,
    badge: Mutex<u32>,
    attention_requests: Mutex<u32>,
    notifications: Mutex<Vec<Notification>>,
    window: Mutex<WindowState>,
    reloads: Mutex<Vec<bool>>,
}

impl FakeUiBridge {
    /// Window exists, visible and focused.
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            events: Mutex::new(Vec::new()),
            badge: Mutex::new(0),
            attention_requests: Mutex::new(0),
            notifications: Mutex::new(Vec::new()),
            window: Mutex::new(WindowState { exists: true, visible: true, focused: true }),
            reloads: Mutex::new(Vec::new()),
        })
    }

    pub fn set_window_state(&self, w: WindowState) {
        *self.window.lock() = w;
    }

    pub fn events(&self) -> Vec<UiEvent> {
        self.events.lock().clone()
    }

    /// Event `type` names in order.
    pub fn event_names(&self) -> Vec<&'static str> {
        self.events.lock().iter().map(UiEvent::name).collect()
    }

    pub fn take_events(&self) -> Vec<UiEvent> {
        std::mem::take(&mut *self.events.lock())
    }

    pub fn badge(&self) -> u32 {
        *self.badge.lock()
    }

    pub fn attention_requests(&self) -> u32 {
        *self.attention_requests.lock()
    }

    pub fn notifications(&self) -> Vec<Notification> {
        self.notifications.lock().clone()
    }

    pub fn reloads(&self) -> Vec<bool> {
        self.reloads.lock().clone()
    }
}

impl UiBridge for FakeUiBridge {
    fn emit(&self, ev: UiEvent) {
        self.events.lock().push(ev);
    }

    fn set_badge(&self, needs_input: u32) {
        *self.badge.lock() = needs_input;
    }

    fn request_attention(&self) {
        *self.attention_requests.lock() += 1;
    }

    fn notify(&self, n: Notification) -> Result<(), KeltaError> {
        self.notifications.lock().push(n);
        Ok(())
    }

    fn window_state(&self) -> WindowState {
        *self.window.lock()
    }

    fn reload_webview(&self, safe: bool) {
        self.reloads.lock().push(safe);
    }
}

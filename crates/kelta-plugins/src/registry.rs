//! Plugin discovery (`<data>/plugins/<id>/` + `plugins.dev_paths`) and the enable/disable state
//! file (`<data>/plugins/.state.json`). Grants are not here: they live in the `GrantStore`.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use kelta_proto::error::KeltaError;
use kelta_proto::ext::PluginManifest;
use kelta_proto::ids::PluginId;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::manifest::{self, ParsedManifest};
use crate::util::io_err;

/// One discovered plugin directory.
#[derive(Debug)]
pub struct Entry {
    pub id: PluginId,
    /// Canonical plugin directory.
    pub dir: PathBuf,
    pub dev: bool,
    pub parsed: Option<ParsedManifest>,
    /// Invalid-manifest errors (the plugin contributes nothing).
    pub errors: Vec<String>,
    /// Non-fatal reasons the plugin is not loaded (API, platform, missing files, duplicate id).
    pub problems: Vec<String>,
    /// Flat JSON Schema of `[contributes.settings]`.
    pub settings_schema: Option<Value>,
}

impl Entry {
    pub fn manifest(&self) -> Option<&PluginManifest> {
        self.parsed.as_ref().map(|p| &p.manifest)
    }

    pub fn sha256(&self) -> &str {
        self.parsed.as_ref().map(|p| p.sha256.as_str()).unwrap_or("")
    }

    /// Valid and compatible (enabled state is checked separately).
    pub fn loadable(&self) -> bool {
        self.parsed.is_some() && self.errors.is_empty() && self.problems.is_empty()
    }

    /// Every reason shown in `PluginInfo.problems`.
    pub fn all_problems(&self) -> Vec<String> {
        self.errors.iter().chain(self.problems.iter()).cloned().collect()
    }
}

/// A snapshot of every discovered plugin.
#[derive(Debug, Default)]
pub struct Registry {
    pub entries: Vec<Arc<Entry>>,
}

impl Registry {
    pub fn get(&self, id: &str) -> Option<&Arc<Entry>> {
        self.entries.iter().find(|e| e.id.as_str() == id)
    }
}

/// Load one plugin directory.
pub fn load_entry(dir: &Path, dev: bool) -> Entry {
    let canonical = dir.canonicalize().unwrap_or_else(|_| dir.to_path_buf());
    let fallback_id = PluginId::new(dir.file_name().and_then(|n| n.to_str()).unwrap_or("unknown").to_owned());
    match manifest::load_dir(&canonical) {
        Ok(parsed) => {
            let mut problems = parsed.problems.clone();
            if !dev && parsed.manifest.id != fallback_id {
                problems.push(format!(
                    "directory name `{}` does not match the manifest id `{}`",
                    fallback_id, parsed.manifest.id
                ));
            }
            let settings_schema = parsed.manifest.contributes.settings.as_ref().and_then(|s| {
                match read_settings_schema(&canonical, &s.schema) {
                    Ok(v) => Some(v),
                    Err(e) => {
                        problems.push(e);
                        None
                    }
                }
            });
            Entry {
                id: parsed.manifest.id.clone(),
                dir: canonical,
                dev,
                parsed: Some(parsed),
                errors: Vec::new(),
                problems,
                settings_schema,
            }
        }
        Err(e) => {
            let mut errors: Vec<String> = e
                .detail
                .as_ref()
                .and_then(|d| d.get("errors"))
                .and_then(Value::as_array)
                .map(|a| a.iter().filter_map(Value::as_str).map(str::to_owned).collect())
                .unwrap_or_default();
            if errors.is_empty() {
                errors.push(e.message.clone());
            }
            Entry {
                id: fallback_id,
                dir: canonical,
                dev,
                parsed: None,
                errors,
                problems: Vec::new(),
                settings_schema: None,
            }
        }
    }
}

fn read_settings_schema(dir: &Path, rel: &str) -> Result<Value, String> {
    let path = dir.join(rel);
    let canonical = path.canonicalize().map_err(|e| format!("settings schema `{rel}`: {e}"))?;
    if !canonical.starts_with(dir) {
        return Err(format!("settings schema `{rel}` is outside the plugin directory"));
    }
    let text = std::fs::read_to_string(&canonical).map_err(|e| format!("settings schema `{rel}`: {e}"))?;
    let v: Value = serde_json::from_str(&text).map_err(|e| format!("settings schema `{rel}`: {e}"))?;
    if v.get("type").and_then(Value::as_str) != Some("object")
        || !v.get("properties").is_some_and(Value::is_object)
    {
        return Err(format!("settings schema `{rel}` must be a flat object schema with `properties`"));
    }
    jsonschema::validator_for(&v).map_err(|e| format!("settings schema `{rel}`: {e}"))?;
    Ok(v)
}

/// Scan the data dir and the dev paths. Dev paths win on duplicate ids (they are explicit).
pub fn discover(plugins_dir: &Path, dev_paths: &[PathBuf]) -> Registry {
    let mut entries: Vec<Entry> = Vec::new();
    for p in dev_paths {
        if p.is_dir() {
            entries.push(load_entry(p, true));
        }
    }
    let mut installed: Vec<PathBuf> = std::fs::read_dir(plugins_dir)
        .map(|rd| {
            rd.filter_map(Result::ok)
                .map(|e| e.path())
                .filter(|p| p.is_dir())
                .filter(|p| p.file_name().and_then(|n| n.to_str()).is_some_and(|n| !n.starts_with('.')))
                .collect()
        })
        .unwrap_or_default();
    installed.sort();
    for p in installed {
        entries.push(load_entry(&p, false));
    }
    let mut seen = BTreeSet::new();
    for e in &mut entries {
        if !seen.insert(e.id.clone()) {
            e.problems.push(format!("duplicate plugin id `{}` (another copy is loaded)", e.id));
        }
    }
    Registry { entries: entries.into_iter().map(Arc::new).collect() }
}

/// Persistent enable/disable state.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct State {
    pub disabled: BTreeSet<String>,
}

pub fn state_path(plugins_dir: &Path) -> PathBuf {
    plugins_dir.join(".state.json")
}

pub fn load_state(plugins_dir: &Path) -> State {
    std::fs::read_to_string(state_path(plugins_dir))
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default()
}

pub fn save_state(plugins_dir: &Path, state: &State) -> Result<(), KeltaError> {
    std::fs::create_dir_all(plugins_dir).map_err(|e| io_err(plugins_dir.display(), e))?;
    let path = state_path(plugins_dir);
    let tmp = plugins_dir.join(".state.json.tmp");
    let text = serde_json::to_vec_pretty(state)?;
    std::fs::write(&tmp, text).map_err(|e| io_err(tmp.display(), e))?;
    std::fs::rename(&tmp, &path).map_err(|e| io_err(path.display(), e))?;
    Ok(())
}

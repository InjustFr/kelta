//! `ConfigService`: layer loading, merge, validation, last-good state and change notification.
//!
//! Every disk read goes through [`ConfigService::build`], which produces a complete candidate
//! [`Inner`]: files whose content hash is unchanged are reused (this is also how the echo of our
//! own atomic writes is ignored), invalid files keep their last good value, and a merged result
//! that fails semantic validation reverts the changed layers. [`ConfigService::commit`] swaps
//! the candidate in and notifies `watch` callbacks.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Weak};

use kelta_proto::api::{SettingsSource, TrustStore};
use kelta_proto::dirs::Dirs;
use kelta_proto::error::KeltaError;
use kelta_proto::ids::{PluginId, ProjectId};
use kelta_proto::settings::{
    EffectiveSettings, Layer, ProjectConfig, REPO_EXEC_KEYS, RuntimeOverrides, Settings, SettingsDiff,
    ValidationIssue,
};
use parking_lot::{Mutex, RwLock};
use serde_json::{Map, Value};

use crate::merge::Merger;
use crate::path::{self, join_path, split_path};
use crate::schema_info::index;
use crate::toml_io::{self, Parsed, read_optional, sha256_hex};
use crate::validate::{self, RawIssue};

/// Change callback registered with [`ConfigService::watch`].
pub type OnChange = Box<dyn Fn(SettingsDiff) + Send + Sync>;
/// Callback receiving the full issue list whenever it changes.
pub type OnIssues = Box<dyn Fn(Vec<ConfigIssue>) + Send + Sync>;

/// A validation problem attributed to a file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigIssue {
    pub file: PathBuf,
    pub issue: ValidationIssue,
}

impl ConfigIssue {
    /// `file:line:col message` as shown in toasts.
    pub fn display(&self) -> String {
        let loc = match (self.issue.line, self.issue.col) {
            (Some(l), Some(c)) => format!(":{l}:{c}"),
            (Some(l), None) => format!(":{l}"),
            _ => String::new(),
        };
        let key = if self.issue.path.is_empty() { String::new() } else { format!("{}: ", self.issue.path) };
        format!("{}{} {}{}", self.file.display(), loc, key, self.issue.message)
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum FileKind {
    Global,
    Keybindings,
    Project,
    Repo,
}

impl FileKind {
    fn layer(self) -> Layer {
        match self {
            Self::Global | Self::Keybindings => Layer::Global,
            Self::Project => Layer::Project,
            Self::Repo => Layer::Repo,
        }
    }
}

/// One TOML file of a layer.
#[derive(Clone, Debug)]
pub(crate) struct FileLayer {
    pub path: PathBuf,
    pub exists: bool,
    pub text: String,
    pub hash: String,
    /// Last good parsed value (a JSON object).
    pub value: Value,
    /// Whether the current text is valid.
    pub valid: bool,
    pub issues: Vec<ValidationIssue>,
}

impl FileLayer {
    fn missing(path: &Path) -> Self {
        Self {
            path: path.to_owned(),
            exists: false,
            text: String::new(),
            hash: sha256_hex(b""),
            value: Value::Object(Map::new()),
            valid: true,
            issues: Vec::new(),
        }
    }
}

#[derive(Clone)]
pub(crate) struct Computed {
    pub settings: Arc<Settings>,
    pub doc: Arc<EffectiveSettings>,
}

#[derive(Clone)]
pub(crate) struct RepoEntry {
    pub repo_id: String,
    pub primary: bool,
    pub file: FileLayer,
}

#[derive(Clone)]
pub(crate) struct ProjectEntry {
    pub file: FileLayer,
    pub config: Arc<ProjectConfig>,
    pub repos: Vec<RepoEntry>,
    pub eff: Computed,
}

#[derive(Clone)]
pub(crate) struct Inner {
    pub global: FileLayer,
    pub keybindings: FileLayer,
    pub projects: BTreeMap<String, ProjectEntry>,
    /// Project files that never parsed (kept for their issues only).
    pub rejected: BTreeMap<PathBuf, FileLayer>,
    pub global_eff: Computed,
    pub plugin_defaults: Value,
}

/// Texts that replace the disk content during a [`ConfigService::build`] (pending writes).
pub(crate) type Overrides = HashMap<PathBuf, String>;

pub struct ConfigService {
    pub(crate) me: Weak<ConfigService>,
    pub(crate) dirs: Dirs,
    pub(crate) overrides: RuntimeOverrides,
    pub(crate) runtime: Value,
    pub(crate) runtime_issues: Vec<ConfigIssue>,
    pub(crate) defaults: Value,
    pub(crate) inner: RwLock<Inner>,
    pub(crate) issues: RwLock<Vec<ConfigIssue>>,
    pub(crate) plugin_schemas: RwLock<Vec<(PluginId, Value)>>,
    pub(crate) on_change: RwLock<Vec<OnChange>>,
    pub(crate) on_issues: RwLock<Vec<OnIssues>>,
    /// Serializes `build` + `commit` (writes and reloads).
    pub(crate) gate: Mutex<()>,
    /// Content hash trusted per repo config path (mirror of the `TrustStore`).
    pub(crate) trusted: RwLock<HashMap<PathBuf, String>>,
    pub(crate) trust_store: RwLock<Option<Arc<dyn TrustStore>>>,
    pub(crate) watcher: Mutex<Option<crate::watch::WatchHandle>>,
}

// ---------------------------------------------------------------------------------------------
// helpers
// ---------------------------------------------------------------------------------------------

fn empty_obj() -> Value {
    Value::Object(Map::new())
}

/// `~` expansion for repo paths.
pub fn expand_home(p: &str) -> PathBuf {
    if let Some(rest) = p.strip_prefix("~/")
        && let Some(home) = std::env::var_os("HOME")
    {
        return PathBuf::from(home).join(rest);
    }
    if p == "~"
        && let Some(home) = std::env::var_os("HOME")
    {
        return PathBuf::from(home);
    }
    PathBuf::from(p)
}

pub(crate) fn deep_merge(base: &mut Value, over: &Value) {
    match (&mut *base, over) {
        (Value::Object(b), Value::Object(o)) => {
            for (k, v) in o {
                match b.get_mut(k) {
                    Some(x) => deep_merge(x, v),
                    None => {
                        b.insert(k.clone(), v.clone());
                    }
                }
            }
        }
        _ => *base = over.clone(),
    }
}

fn plugin_defaults_value(fragments: &[(PluginId, Value)]) -> Value {
    let mut tables = Map::new();
    for (id, schema) in fragments {
        let mut t = Map::new();
        if let Some(props) = schema.get("properties").and_then(Value::as_object) {
            for (k, p) in props {
                // A plugin must never pick its own SecretRef: the user fills secret fields.
                if p.get("x-kelta-secret").and_then(Value::as_bool) != Some(true)
                    && let Some(d) = p.get("default")
                {
                    t.insert(k.clone(), d.clone());
                }
            }
        }
        if !t.is_empty() {
            tables.insert(id.as_str().to_owned(), Value::Object(t));
        }
    }
    if tables.is_empty() {
        return empty_obj();
    }
    let mut root = Map::new();
    root.insert("plugins".into(), Value::Object(tables));
    Value::Object(root)
}

fn strip_exec_keys(v: &Value) -> Value {
    let mut out = v.clone();
    for k in REPO_EXEC_KEYS {
        if let Ok(segs) = split_path(k) {
            path::remove(&mut out, &segs);
        }
    }
    out
}

fn diff_leaves(old: &Value, new: &Value, cur: &mut Vec<String>, out: &mut BTreeSet<String>) {
    match (old, new) {
        (Value::Object(a), Value::Object(b)) => {
            let keys: BTreeSet<&String> = a.keys().chain(b.keys()).collect();
            for k in keys {
                cur.push(k.clone());
                match (a.get(k), b.get(k)) {
                    (Some(x), Some(y)) => diff_leaves(x, y, cur, out),
                    _ => {
                        out.insert(join_path(cur));
                    }
                }
                cur.pop();
            }
        }
        _ => {
            if old != new && !cur.is_empty() {
                out.insert(join_path(cur));
            }
        }
    }
}

fn parse_runtime_value(text: &str) -> Value {
    let wrapped = format!("v = {text}");
    match toml_io::parse(&wrapped) {
        Ok(p) => p.value.get("v").cloned().unwrap_or_else(|| Value::String(text.to_owned())),
        Err(_) => Value::String(text.to_owned()),
    }
}

/// The Runtime layer document and the issues of rejected entries.
fn build_runtime(overrides: &RuntimeOverrides) -> (Value, Vec<ConfigIssue>) {
    let mut rt = empty_obj();
    let mut issues = Vec::new();
    let origin = PathBuf::from("<env/cli>");
    for (key, text) in &overrides.sets {
        match split_path(key) {
            Ok(segs) if !segs.is_empty() => {
                let candidate = {
                    let mut c = rt.clone();
                    let _ = path::set(&mut c, &segs, parse_runtime_value(text));
                    c
                };
                let bad = validate::schema_issues(Layer::Runtime, &candidate);
                if bad.is_empty() {
                    rt = candidate;
                } else {
                    for b in bad {
                        issues.push(ConfigIssue {
                            file: origin.clone(),
                            issue: ValidationIssue {
                                path: key.clone(),
                                message: format!("runtime override ignored: {}", b.message),
                                line: None,
                                col: None,
                            },
                        });
                    }
                }
            }
            _ => issues.push(ConfigIssue {
                file: origin.clone(),
                issue: ValidationIssue {
                    path: key.clone(),
                    message: "runtime override ignored: invalid key".into(),
                    line: None,
                    col: None,
                },
            }),
        }
    }
    if overrides.safe_graphics {
        let _ = path::set(
            &mut rt,
            &["linux".to_owned(), "graphics".to_owned(), "profile".to_owned()],
            Value::String("safe".into()),
        );
    }
    (rt, issues)
}

fn project_config_of(value: &Value) -> Option<ProjectConfig> {
    serde_json::from_value::<ProjectConfig>(value.get("project")?.clone()).ok()
}

fn without_project_table(value: &Value) -> Value {
    let mut v = value.clone();
    if let Value::Object(m) = &mut v {
        m.remove("project");
    }
    v
}

/// Issues of the keybindings file are reported against the file, without the synthetic `keys`.
fn locate_keybindings_path(parsed: &Parsed, mut raw: Vec<RawIssue>) -> Vec<RawIssue> {
    if parsed.value.get("keys").is_none() {
        for i in &mut raw {
            if i.path.first().map(String::as_str) == Some("keys") {
                i.path.remove(0);
            }
        }
    }
    raw
}

impl ConfigService {
    /// Load every layer from disk. Invalid files leave defaults in place and are reported by
    /// [`ConfigService::issues`] (SPEC §5 "Invalid config").
    pub fn load(dirs: &Dirs, overrides: RuntimeOverrides) -> Result<Arc<Self>, KeltaError> {
        let mut dirs = dirs.clone();
        if let Some(c) = &overrides.config_dir {
            dirs.config = c.clone();
        }
        let defaults = serde_json::to_value(Settings::defaults())?;
        let (runtime, runtime_issues) = build_runtime(&overrides);
        let empty = Inner {
            global: FileLayer::missing(&dirs.global_config()),
            keybindings: FileLayer::missing(&dirs.config.join("keybindings.toml")),
            projects: BTreeMap::new(),
            rejected: BTreeMap::new(),
            global_eff: Computed {
                settings: Arc::new(Settings::defaults()),
                doc: Arc::new(EffectiveSettings::default()),
            },
            plugin_defaults: empty_obj(),
        };
        let svc = Arc::new_cyclic(|me| ConfigService {
            me: me.clone(),
            dirs,
            overrides,
            runtime,
            runtime_issues,
            defaults,
            inner: RwLock::new(empty),
            issues: RwLock::new(Vec::new()),
            plugin_schemas: RwLock::new(Vec::new()),
            on_change: RwLock::new(Vec::new()),
            on_issues: RwLock::new(Vec::new()),
            gate: Mutex::new(()),
            trusted: RwLock::new(HashMap::new()),
            trust_store: RwLock::new(None),
            watcher: Mutex::new(None),
        });
        {
            let _g = svc.gate.lock();
            let prev = svc.inner.read().clone();
            let cand = svc.build(&prev, &Overrides::new());
            *svc.issues.write() = svc.collect_issues(&cand);
            *svc.inner.write() = cand;
        }
        Ok(svc)
    }

    pub fn dirs(&self) -> &Dirs {
        &self.dirs
    }

    pub fn overrides(&self) -> &RuntimeOverrides {
        &self.overrides
    }

    /// Current issue list (parse/validation problems of files; last good config stays active).
    pub fn issues(&self) -> Vec<ConfigIssue> {
        self.issues.read().clone()
    }

    /// Register a change callback (invoked after a successful hot reload or write). The first
    /// call also starts the directory watch.
    pub fn watch(&self, on_change: OnChange) {
        self.on_change.write().push(on_change);
        if let Some(me) = self.me.upgrade() {
            let mut w = self.watcher.lock();
            if w.is_none() {
                match crate::watch::start(me) {
                    Ok(h) => *w = Some(h),
                    Err(e) => tracing::warn!(error = %e, "settings watch could not start"),
                }
            }
        }
    }

    /// Register a callback receiving the issue list whenever it changes.
    pub fn watch_issues(&self, on_issues: OnIssues) {
        self.on_issues.write().push(on_issues);
    }

    /// Plugin-defaults layer + validation of `plugins.<id>` (from `PluginSettingsSource`).
    pub fn set_plugin_schemas(&self, fragments: Vec<(PluginId, Value)>) {
        *self.plugin_schemas.write() = fragments;
        let _ = self.reload();
    }

    /// Settings JSON Schema including the `plugins.<id>` fragments.
    pub fn schema_full(&self) -> Value {
        let mut s = index().settings_schema().clone();
        let frags = self.plugin_schemas.read().clone();
        if let Some(props) = s.pointer_mut("/properties/plugins/properties").and_then(Value::as_object_mut) {
            for (id, mut frag) in frags {
                if let Value::Object(m) = &mut frag {
                    m.entry("x-kelta-category").or_insert(Value::String("Plugins".into()));
                }
                props.insert(id.as_str().to_owned(), frag);
            }
        }
        s
    }

    /// Flattened settings JSON Schema (without plugin fragments; see [`Self::schema_full`]).
    pub fn schema() -> Value {
        kelta_proto::schema::settings_schema()
    }

    /// Re-read every file and apply the result. Returns the diff, `None` when nothing changed.
    pub fn reload(&self) -> Option<SettingsDiff> {
        let _g = self.gate.lock();
        let prev = self.inner.read().clone();
        let cand = self.build(&prev, &Overrides::new());
        self.commit(prev, cand)
    }

    // -----------------------------------------------------------------------------------------
    // build
    // -----------------------------------------------------------------------------------------

    fn read_text(path: &Path, ov: &Overrides) -> Result<Option<String>, KeltaError> {
        if let Some(t) = ov.get(path) {
            return Ok(Some(t.clone()));
        }
        read_optional(path)
    }

    /// Load one file, reusing `prev` when the content hash is unchanged.
    pub(crate) fn load_file(
        &self,
        path: &Path,
        kind: FileKind,
        stem: Option<&str>,
        prev: Option<&FileLayer>,
        ov: &Overrides,
    ) -> FileLayer {
        let read = Self::read_text(path, ov);
        let (exists, text) = match read {
            Ok(Some(t)) => (true, t),
            Ok(None) => (false, String::new()),
            Err(e) => {
                let mut f = prev.cloned().unwrap_or_else(|| FileLayer::missing(path));
                f.valid = false;
                f.issues = vec![ValidationIssue {
                    path: String::new(),
                    message: format!("cannot read file: {}", e.message),
                    line: None,
                    col: None,
                }];
                return f;
            }
        };
        let hash = sha256_hex(text.as_bytes());
        if let Some(p) = prev
            && p.exists == exists
            && p.hash == hash
        {
            return p.clone();
        }
        let last_good = prev.map_or_else(empty_obj, |p| p.value.clone());
        if !exists {
            return FileLayer {
                path: path.to_owned(),
                exists,
                text,
                hash,
                value: empty_obj(),
                valid: true,
                issues: vec![],
            };
        }
        let parsed = match toml_io::parse(&text) {
            Ok(p) => p,
            Err(issue) => {
                return FileLayer {
                    path: path.to_owned(),
                    exists,
                    text,
                    hash,
                    value: last_good,
                    valid: false,
                    issues: vec![issue],
                };
            }
        };
        let (value, raw) = if kind == FileKind::Keybindings {
            let inner = match parsed.value.get("keys") {
                Some(k) if parsed.value.as_object().is_some_and(|m| m.len() == 1) => k.clone(),
                _ => parsed.value.clone(),
            };
            let mut wrapped = Map::new();
            wrapped.insert("keys".into(), inner);
            let wrapped = Value::Object(wrapped);
            let raw = locate_keybindings_path(&parsed, validate::layer_issues(Layer::Global, &wrapped, None));
            (wrapped, raw)
        } else {
            let raw = validate::layer_issues(kind.layer(), &parsed.value, stem);
            (parsed.value.clone(), raw)
        };
        if raw.is_empty() {
            FileLayer { path: path.to_owned(), exists, text, hash, value, valid: true, issues: vec![] }
        } else {
            let issues = validate::position(&text, Some(&parsed), raw);
            FileLayer { path: path.to_owned(), exists, text, hash, value: last_good, valid: false, issues }
        }
    }

    fn global_value(inner_global: &FileLayer, keybindings: &FileLayer) -> Value {
        let mut v = inner_global.value.clone();
        if !keybindings.value.as_object().is_none_or(Map::is_empty) {
            deep_merge(&mut v, &keybindings.value);
        }
        v
    }

    /// Merge all layers. `Err` carries semantic issues of the merged result.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn compute(
        &self,
        plugin_defaults: &Value,
        global: &Value,
        project: Option<&Value>,
        repos: &[Value],
        project_default_template: Option<&str>,
    ) -> Result<Computed, Vec<RawIssue>> {
        let by_id = index().by_id_paths();
        let mut base = self.defaults.clone();
        let mut m = Merger::new(by_id, &base, Layer::Default);
        if plugin_defaults.as_object().is_some_and(|o| !o.is_empty()) {
            m.apply(&mut base, plugin_defaults, Layer::Plugin);
        }
        m.apply(&mut base, global, Layer::Global);
        if let Some(p) = project {
            m.apply(&mut base, &without_project_table(p), Layer::Project);
        }
        for r in repos {
            m.apply(&mut base, r, Layer::Repo);
        }
        m.apply(&mut base, &self.runtime, Layer::Runtime);
        let sources = m.finish();

        let mut issues = validate::cross_issues(&base, project_default_template);
        issues.extend(validate::plugin_issues(&base, &self.plugin_schemas.read()));
        if issues.is_empty() {
            match serde_json::from_value::<Settings>(base.clone()) {
                Ok(settings) => {
                    return Ok(Computed {
                        settings: Arc::new(settings),
                        doc: Arc::new(EffectiveSettings { value: base, sources }),
                    });
                }
                Err(e) => issues.push(RawIssue::new(&[], format!("invalid merged settings: {e}"))),
            }
        }
        Err(issues)
    }

    fn repo_value(&self, r: &RepoEntry) -> Value {
        let trusted = self.trusted.read().get(&r.file.path).is_some_and(|h| *h == r.file.hash);
        if trusted { r.file.value.clone() } else { strip_exec_keys(&r.file.value) }
    }

    fn repos_values(&self, repos: &[RepoEntry]) -> Vec<Value> {
        // non-primary first, the primary repo wins on conflict
        let mut order: Vec<&RepoEntry> = repos.iter().filter(|r| !r.primary).collect();
        order.extend(repos.iter().filter(|r| r.primary));
        order.into_iter().filter(|r| r.file.exists).map(|r| self.repo_value(r)).collect()
    }

    fn repo_entries(
        &self,
        cfg: &ProjectConfig,
        prev: Option<&ProjectEntry>,
        ov: &Overrides,
    ) -> Vec<RepoEntry> {
        let any_primary = cfg.repos.iter().any(|r| r.primary);
        cfg.repos
            .iter()
            .enumerate()
            .map(|(i, r)| {
                let path = expand_home(&r.path).join(".kelta").join("config.toml");
                let old = prev.and_then(|p| p.repos.iter().find(|x| x.file.path == path)).map(|x| &x.file);
                RepoEntry {
                    repo_id: r.id.clone(),
                    primary: r.primary || (!any_primary && i == 0),
                    file: self.load_file(&path, FileKind::Repo, None, old, ov),
                }
            })
            .collect()
    }

    fn project_files(&self) -> Vec<(String, PathBuf)> {
        let mut out = Vec::new();
        if let Ok(rd) = std::fs::read_dir(self.dirs.projects_dir()) {
            for e in rd.flatten() {
                let p = e.path();
                if p.extension().and_then(|x| x.to_str()) == Some("toml")
                    && let Some(stem) = p.file_stem().and_then(|s| s.to_str())
                    && !stem.starts_with('.')
                {
                    out.push((stem.to_owned(), p));
                }
            }
        }
        out.sort();
        out
    }

    /// Compute a project's effective settings from its layers.
    fn try_project(
        &self,
        plugin_defaults: &Value,
        global: &Value,
        file: &FileLayer,
        repos: &[RepoEntry],
        cfg: &ProjectConfig,
    ) -> Result<Computed, Vec<RawIssue>> {
        let rv = self.repos_values(repos);
        self.compute(plugin_defaults, global, Some(&file.value), &rv, cfg.default_template.as_deref())
    }

    /// Build a complete candidate state from disk (plus pending `ov` texts).
    pub(crate) fn build(&self, prev: &Inner, ov: &Overrides) -> Inner {
        let plugin_defaults = plugin_defaults_value(&self.plugin_schemas.read());
        let mut global =
            self.load_file(&self.dirs.global_config(), FileKind::Global, None, Some(&prev.global), ov);
        let mut keybindings = self.load_file(
            &self.dirs.config.join("keybindings.toml"),
            FileKind::Keybindings,
            None,
            Some(&prev.keybindings),
            ov,
        );

        // global effective, reverting freshly changed global layers on semantic failure
        let mut global_eff =
            self.compute(&plugin_defaults, &Self::global_value(&global, &keybindings), None, &[], None);
        if let Err(raw) = &global_eff {
            let mut reverted = false;
            for (fresh, old) in [(&mut global, &prev.global), (&mut keybindings, &prev.keybindings)] {
                if fresh.hash != old.hash {
                    fresh.value = old.value.clone();
                    fresh.valid = false;
                    fresh.issues = validate::position(
                        &fresh.text,
                        toml_io::parse(&fresh.text).ok().as_ref(),
                        raw.clone(),
                    );
                    reverted = true;
                }
            }
            if reverted {
                global_eff = self.compute(
                    &plugin_defaults,
                    &Self::global_value(&global, &keybindings),
                    None,
                    &[],
                    None,
                );
            }
        }
        let global_eff = global_eff.unwrap_or_else(|_| Computed {
            settings: Arc::new(Settings::defaults()),
            doc: Arc::new(EffectiveSettings { value: self.defaults.clone(), sources: BTreeMap::new() }),
        });
        let gv = Self::global_value(&global, &keybindings);

        // project files (including ones that only exist in `ov`)
        let mut paths: Vec<(String, PathBuf)> = self.project_files();
        for p in ov.keys() {
            if p.parent() == Some(self.dirs.projects_dir().as_path())
                && p.extension().and_then(|x| x.to_str()) == Some("toml")
                && let Some(stem) = p.file_stem().and_then(|s| s.to_str())
                && !paths.iter().any(|(s, _)| s == stem)
            {
                paths.push((stem.to_owned(), p.clone()));
            }
        }
        paths.sort();

        let mut projects = BTreeMap::new();
        let mut rejected = BTreeMap::new();
        for (stem, path) in paths {
            let old = prev.projects.get(&stem);
            let mut file = self.load_file(&path, FileKind::Project, Some(&stem), old.map(|e| &e.file), ov);
            if !file.exists {
                continue;
            }
            let Some(mut cfg) =
                project_config_of(&file.value).map(Arc::new).or_else(|| old.map(|e| e.config.clone()))
            else {
                rejected.insert(path, file);
                continue;
            };
            let mut repos = self.repo_entries(&cfg, old, ov);
            let eff = match self.try_project(&plugin_defaults, &gv, &file, &repos, &cfg) {
                Ok(c) => c,
                Err(raw) => {
                    // semantic failure: revert the layers that changed since the last good state
                    let mut changed = false;
                    if let Some(pe) = old {
                        if file.hash != pe.file.hash {
                            file.value = pe.file.value.clone();
                            file.valid = false;
                            cfg = pe.config.clone();
                            changed = true;
                        }
                        for (r, o) in repos.iter_mut().zip(pe.repos.iter()) {
                            if r.file.hash != o.file.hash {
                                r.file.value = o.file.value.clone();
                                r.file.valid = false;
                                changed = true;
                            }
                        }
                    }
                    if !changed {
                        // nothing to revert to (new file): reject it and say why
                        file.valid = false;
                        file.issues.extend(validate::position(
                            &file.text,
                            toml_io::parse(&file.text).ok().as_ref(),
                            raw,
                        ));
                    } else {
                        file.issues.push(ValidationIssue {
                            path: String::new(),
                            message: "change rejected: it breaks the merged settings (unknown template, preset or plugin setting)"
                                .into(),
                            line: None,
                            col: None,
                        });
                    }
                    self.try_project(&plugin_defaults, &gv, &file, &repos, &cfg)
                        .unwrap_or_else(|_| old.map_or_else(|| global_eff.clone(), |p| p.eff.clone()))
                }
            };
            projects.insert(stem, ProjectEntry { file, config: cfg, repos, eff });
        }
        Inner { global, keybindings, projects, rejected, global_eff, plugin_defaults }
    }

    fn collect_issues(&self, inner: &Inner) -> Vec<ConfigIssue> {
        let mut out: Vec<ConfigIssue> = self.runtime_issues.clone();
        let mut add = |f: &FileLayer| {
            for i in &f.issues {
                out.push(ConfigIssue { file: f.path.clone(), issue: i.clone() });
            }
        };
        add(&inner.global);
        add(&inner.keybindings);
        for e in inner.projects.values() {
            add(&e.file);
            for r in &e.repos {
                add(&r.file);
            }
        }
        for f in inner.rejected.values() {
            add(f);
        }
        out
    }

    // -----------------------------------------------------------------------------------------
    // commit
    // -----------------------------------------------------------------------------------------

    fn layers_changed(prev: &Inner, next: &Inner) -> Vec<Layer> {
        let mut layers = BTreeSet::new();
        if prev.global.hash != next.global.hash || prev.keybindings.hash != next.keybindings.hash {
            layers.insert(Layer::Global);
        }
        if prev.plugin_defaults != next.plugin_defaults {
            layers.insert(Layer::Plugin);
        }
        let ids: BTreeSet<&String> = prev.projects.keys().chain(next.projects.keys()).collect();
        for id in ids {
            match (prev.projects.get(id), next.projects.get(id)) {
                (Some(a), Some(b)) => {
                    if a.file.hash != b.file.hash {
                        layers.insert(Layer::Project);
                    }
                    let repo_hashes = |e: &ProjectEntry| -> Vec<(PathBuf, String)> {
                        e.repos.iter().map(|r| (r.file.path.clone(), r.file.hash.clone())).collect()
                    };
                    if repo_hashes(a) != repo_hashes(b) {
                        layers.insert(Layer::Repo);
                    }
                }
                _ => {
                    layers.insert(Layer::Project);
                }
            }
        }
        layers.into_iter().collect()
    }

    /// Swap the candidate in; notify callbacks outside the state lock. Caller holds `gate`.
    pub(crate) fn commit(&self, prev: Inner, next: Inner) -> Option<SettingsDiff> {
        let mut paths = BTreeSet::new();
        diff_leaves(&prev.global_eff.doc.value, &next.global_eff.doc.value, &mut Vec::new(), &mut paths);
        let ids: BTreeSet<&String> = prev.projects.keys().chain(next.projects.keys()).collect();
        for id in ids {
            if let (Some(a), Some(b)) = (prev.projects.get(id), next.projects.get(id)) {
                diff_leaves(&a.eff.doc.value, &b.eff.doc.value, &mut Vec::new(), &mut paths);
            }
        }
        let layers = Self::layers_changed(&prev, &next);
        let new_issues = self.collect_issues(&next);
        let issues_changed = *self.issues.read() != new_issues;
        let idx = index();
        let requires_restart: Vec<String> = paths
            .iter()
            .filter(|p| split_path(p).map(|s| idx.info(&s).restart).unwrap_or(false))
            .cloned()
            .collect();
        *self.inner.write() = next;
        *self.issues.write() = new_issues.clone();
        if issues_changed {
            for cb in self.on_issues.read().iter() {
                cb(new_issues.clone());
            }
        }
        if paths.is_empty() && layers.is_empty() && !issues_changed {
            return None;
        }
        let diff = SettingsDiff { layers, paths: paths.into_iter().collect(), requires_restart };
        for cb in self.on_change.read().iter() {
            cb(diff.clone());
        }
        Some(diff)
    }

    // -----------------------------------------------------------------------------------------
    // reads
    // -----------------------------------------------------------------------------------------

    /// `settings_effective`: merged value + winning layer per dotted path.
    pub fn effective_doc(&self, project: Option<&ProjectId>) -> Result<EffectiveSettings, KeltaError> {
        let inner = self.inner.read();
        if let Some(p) = project {
            if let Some(e) = inner.projects.get(p.as_str()) {
                return Ok((*e.eff.doc).clone());
            }
            return Err(KeltaError::not_found(format!("unknown project `{p}`")));
        }
        Ok((*inner.global_eff.doc).clone())
    }

    /// Absolute `<repo>/.kelta/config.toml` paths of every configured repo (watch / trust).
    pub fn repo_config_paths(&self) -> Vec<PathBuf> {
        self.inner
            .read()
            .projects
            .values()
            .flat_map(|p| p.repos.iter().map(|r| r.file.path.clone()))
            .collect()
    }
}

impl SettingsSource for ConfigService {
    fn effective(&self, project: Option<&ProjectId>) -> Arc<Settings> {
        let inner = self.inner.read();
        project
            .and_then(|p| inner.projects.get(p.as_str()))
            .map_or_else(|| inner.global_eff.settings.clone(), |e| e.eff.settings.clone())
    }

    fn project(&self, id: &ProjectId) -> Option<Arc<ProjectConfig>> {
        self.inner.read().projects.get(id.as_str()).map(|e| e.config.clone())
    }

    fn projects(&self) -> Vec<Arc<ProjectConfig>> {
        self.inner.read().projects.values().map(|e| e.config.clone()).collect()
    }
}

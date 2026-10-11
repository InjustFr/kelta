//! Layer reads and writes: `toml_edit` edits, raw writes with validation, project file CRUD
//! (`.trash` on removal) and repo trust.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use parking_lot::MutexGuard;

use kelta_proto::api::TrustStore;
use kelta_proto::dirs::Dirs;
use kelta_proto::error::KeltaError;
use kelta_proto::ids::ProjectId;
use kelta_proto::model::{ProjectDraft, ProjectPatch, RepoDraft};
use kelta_proto::schema::SCHEMA_BASE_URL;
use kelta_proto::settings::{
    EffectiveSettings, Layer, LayerDoc, ProjectConfig, Severity, Source, SourceRoute, TrackerBinding,
    TrustInfo, ValidationIssue, sources_from_trackers,
};
use serde_json::{Value, json};
use toml_edit::DocumentMut;

use crate::edit;
use crate::path::split_path;
use crate::service::{ConfigService, FileKind, FileLayer, Inner, Overrides, expand_home};
use crate::toml_io::{self, read_optional, sha256_hex, write_atomic};
use crate::validate;

/// Where a layer document lives.
#[derive(Debug, Clone)]
pub(crate) struct Target {
    pub path: PathBuf,
    pub kind: FileKind,
}

fn ensure_newline(mut s: String) -> String {
    if !s.is_empty() && !s.ends_with('\n') {
        s.push('\n');
    }
    s
}

/// Only hand-written files are lenient about the colour: an IPC caller gets an error, nothing is written.
fn check_color(doc: &DocumentMut) -> Result<(), KeltaError> {
    let color = ["project".to_owned(), "color".to_owned()];
    if validate::schema_issues(Layer::Project, &toml_io::item_to_json(doc.as_item()))
        .iter()
        .any(|i| i.path == color)
    {
        return Err(KeltaError::invalid(validate::PROJECT_COLOR));
    }
    Ok(())
}

fn issues_error(file: &Path, issues: &[ValidationIssue]) -> KeltaError {
    let first = issues.first();
    let loc = match first.and_then(|i| i.line.zip(i.col)) {
        Some((l, c)) => format!("{}:{l}:{c}", file.display()),
        None => file.display().to_string(),
    };
    let message = match first {
        Some(i) if i.path.is_empty() => format!("{loc} {}", i.message),
        Some(i) => format!("{loc} {}: {}", i.path, i.message),
        None => format!("{loc} invalid"),
    };
    KeltaError::invalid(message).with_detail(json!({ "file": file, "issues": issues }))
}

fn find_file<'a>(inner: &'a Inner, path: &Path) -> Option<&'a FileLayer> {
    if inner.global.path == path {
        return Some(&inner.global);
    }
    if inner.keybindings.path == path {
        return Some(&inner.keybindings);
    }
    for e in inner.projects.values() {
        if e.file.path == path {
            return Some(&e.file);
        }
        if let Some(r) = e.repos.iter().find(|r| r.file.path == path) {
            return Some(&r.file);
        }
    }
    inner.rejected.get(path)
}

fn compress_home(p: &Path) -> String {
    if let Some(home) = std::env::var_os("HOME")
        && let Ok(rest) = p.strip_prefix(PathBuf::from(home))
    {
        return if rest.as_os_str().is_empty() { "~".into() } else { format!("~/{}", rest.display()) };
    }
    p.display().to_string()
}

fn repo_json(r: &RepoDraft, force_primary: bool) -> Value {
    let mut v = json!({
        "id": r.id,
        "path": compress_home(&r.path),
        "primary": r.primary || force_primary,
        "remote": r.remote,
        "base": r.base,
    });
    if let Some(c) = &r.code_host
        && let Ok(cv) = serde_json::to_value(c)
    {
        v["code_host"] = cv;
    }
    v
}

/// A fresh `projects/.trash/<id>-<secs>[-<n>].toml` path.
fn trash_path(dirs: &Dirs, id: &str) -> Result<PathBuf, KeltaError> {
    let trash = dirs.projects_dir().join(".trash");
    std::fs::create_dir_all(&trash)?;
    let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs());
    let mut dest = trash.join(format!("{id}-{secs}.toml"));
    let mut n = 1;
    while dest.exists() {
        dest = trash.join(format!("{id}-{secs}-{n}.toml"));
        n += 1;
    }
    Ok(dest)
}

/// Add one project tool entry to the global list, unless it is an override or already there.
fn merge_project_tool(tools: &mut Vec<Value>, entry: &Value, project: &str, name: &str) {
    let Some(m) = entry.as_object() else { return };
    if m.keys().all(|k| k == "id" || k == "enabled") {
        return;
    }
    let id = m.get("id").and_then(Value::as_str).unwrap_or_default();
    let label = m.get("label").and_then(Value::as_str).unwrap_or(id);
    for n in 0.. {
        let cand = match n {
            0 => entry.clone(),
            _ => {
                let suffix = if n == 1 { project.to_owned() } else { format!("{project}-{n}") };
                let mut c = m.clone();
                c.insert("id".into(), json!(format!("{id}-{suffix}")));
                c.insert("label".into(), json!(format!("{label} ({name})")));
                Value::Object(c)
            }
        };
        match tools.iter().find(|t| t.get("id") == cand.get("id")) {
            None => return tools.push(cand),
            Some(t) if *t == cand => return,
            Some(_) => {}
        }
    }
}

fn repos_json(repos: &[RepoDraft]) -> Value {
    let any_primary = repos.iter().any(|r| r.primary);
    Value::Array(repos.iter().enumerate().map(|(i, r)| repo_json(r, !any_primary && i == 0)).collect())
}

impl ConfigService {
    // -----------------------------------------------------------------------------------------
    // targets
    // -----------------------------------------------------------------------------------------

    pub(crate) fn resolve_target(
        &self,
        layer: Layer,
        project: Option<&ProjectId>,
        repo_id: Option<&str>,
    ) -> Result<Target, KeltaError> {
        let inner = self.inner.read();
        match layer {
            Layer::Global => Ok(Target { path: inner.global.path.clone(), kind: FileKind::Global }),
            Layer::Project => {
                let p = project.ok_or_else(|| KeltaError::invalid("project layer needs a project id"))?;
                let e = inner
                    .projects
                    .get(p.as_str())
                    .ok_or_else(|| KeltaError::not_found(format!("unknown project `{p}`")))?;
                Ok(Target { path: e.file.path.clone(), kind: FileKind::Project })
            }
            Layer::Repo => {
                let p = project.ok_or_else(|| KeltaError::invalid("repo layer needs a project id"))?;
                let e = inner
                    .projects
                    .get(p.as_str())
                    .ok_or_else(|| KeltaError::not_found(format!("unknown project `{p}`")))?;
                let repo = match repo_id {
                    Some(id) => e.repos.iter().find(|r| r.repo_id == id),
                    None => e.repos.iter().find(|r| r.primary),
                }
                .ok_or_else(|| KeltaError::not_found("unknown repo"))?;
                Ok(Target { path: repo.file.path.clone(), kind: FileKind::Repo })
            }
            Layer::Default | Layer::Plugin | Layer::Runtime => {
                Err(KeltaError::invalid(format!("the {layer:?} layer is read-only")))
            }
        }
    }

    fn effective_for(&self, project: Option<&ProjectId>) -> Result<EffectiveSettings, KeltaError> {
        self.effective_doc(project)
    }

    // -----------------------------------------------------------------------------------------
    // reads
    // -----------------------------------------------------------------------------------------

    /// `settings_layer_get`.
    pub fn layer_get(
        &self,
        layer: Layer,
        project: Option<&ProjectId>,
        repo_id: Option<&str>,
    ) -> Result<LayerDoc, KeltaError> {
        match layer {
            Layer::Default => Ok(LayerDoc {
                path: PathBuf::new(),
                value: self.defaults.clone(),
                text: String::new(),
                trusted: None,
            }),
            Layer::Plugin => Ok(LayerDoc {
                path: PathBuf::new(),
                value: self.inner.read().plugin_defaults.clone(),
                text: String::new(),
                trusted: None,
            }),
            Layer::Runtime => Ok(LayerDoc {
                path: PathBuf::new(),
                value: self.runtime.clone(),
                text: String::new(),
                trusted: None,
            }),
            Layer::Global | Layer::Project | Layer::Repo => {
                let t = self.resolve_target(layer, project, repo_id)?;
                let inner = self.inner.read();
                let f = find_file(&inner, &t.path).cloned().unwrap_or_else(|| FileLayer {
                    path: t.path.clone(),
                    exists: false,
                    text: String::new(),
                    hash: sha256_hex(b""),
                    value: json!({}),
                    valid: true,
                    issues: vec![],
                });
                let value = if layer == Layer::Global {
                    let mut v = f.value.clone();
                    let kb = &inner.keybindings.value;
                    if kb.as_object().is_some_and(|m| !m.is_empty()) {
                        crate::service::deep_merge(&mut v, kb);
                    }
                    v
                } else {
                    f.value.clone()
                };
                let trusted = (layer == Layer::Repo && f.exists)
                    .then(|| self.trusted.read().get(&f.path).is_some_and(|h| *h == f.hash));
                Ok(LayerDoc { path: f.path, value, text: f.text, trusted })
            }
        }
    }

    /// `settings_validate`: validate a whole TOML document for a layer (no project context).
    pub fn layer_validate(&self, layer: Layer, text: &str) -> Result<Vec<ValidationIssue>, KeltaError> {
        if !matches!(layer, Layer::Global | Layer::Project | Layer::Repo) {
            return Err(KeltaError::invalid(format!("the {layer:?} layer is read-only")));
        }
        let parsed = match toml_io::parse(text) {
            Ok(p) => p,
            Err(issue) => return Ok(vec![issue]),
        };
        // same lenient reading as the load path: warnings are reported, never blocking
        let mut value = parsed.value.clone();
        let (mut raw, warnings) = validate::lenient_issues(layer, &mut value, None);
        if raw.is_empty() {
            let inner = self.inner.read();
            let pd = inner.plugin_defaults.clone();
            let gv = {
                let mut v = inner.global.value.clone();
                crate::service::deep_merge(&mut v, &inner.keybindings.value);
                v
            };
            drop(inner);
            let res = match layer {
                Layer::Global => self.compute(&pd, &value, None, &[], None),
                Layer::Project => {
                    let dt = parsed
                        .value
                        .pointer("/project/default_template")
                        .and_then(Value::as_str)
                        .map(str::to_owned);
                    self.compute(&pd, &gv, Some(&value), &[], dt.as_deref())
                }
                _ => self.compute(&pd, &gv, None, std::slice::from_ref(&value), None),
            };
            if let Err(e) = res {
                raw.extend(e);
            }
        }
        let mut out = validate::position(text, Some(&parsed), raw);
        out.extend(validate::position_warnings(text, Some(&parsed), warnings));
        Ok(out)
    }

    // -----------------------------------------------------------------------------------------
    // writes
    // -----------------------------------------------------------------------------------------

    /// Validate `new_text` as the content of `target`, write it atomically and apply it.
    pub(crate) fn commit_text(&self, target: &Target, new_text: String) -> Result<(), KeltaError> {
        let g = self.gate.lock();
        self.commit_text_locked(&g, target, new_text)
    }

    /// Same as [`Self::commit_text`] for callers that read the file under the gate (read-modify-write).
    fn commit_text_locked(
        &self,
        _gate: &MutexGuard<'_, ()>,
        target: &Target,
        new_text: String,
    ) -> Result<(), KeltaError> {
        let prev = self.inner.read().clone();
        let mut ov = Overrides::new();
        ov.insert(target.path.clone(), new_text.clone());
        let cand = self.build(&prev, &ov);
        match find_file(&cand, &target.path) {
            Some(f) if f.valid && f.exists => {}
            Some(f) => return Err(issues_error(&target.path, &f.issues)),
            None => {
                // a brand-new project file that was rejected before it could join the state
                return Err(KeltaError::invalid(format!("{} could not be loaded", target.path.display())));
            }
        }
        let unchanged = read_optional(&target.path)?.is_some_and(|t| t == new_text);
        if !unchanged {
            write_atomic(&target.path, &new_text)?;
        }
        let _ = self.commit(prev, cand);
        Ok(())
    }

    /// Parse the current file (empty document when it does not exist yet).
    fn read_doc(&self, target: &Target) -> Result<DocumentMut, KeltaError> {
        let text = read_optional(&target.path)?.unwrap_or_default();
        match text.parse::<DocumentMut>() {
            Ok(d) => Ok(d),
            Err(e) => {
                let (line, col) = e.span().map_or((None, None), |r| {
                    let (l, c) = toml_io::line_col(&text, r.start);
                    (Some(l), Some(c))
                });
                Err(issues_error(
                    &target.path,
                    &[ValidationIssue {
                        path: String::new(),
                        message: format!(
                            "syntax error ({}); fix the file before editing it from settings",
                            e.message()
                        ),
                        line,
                        col,
                        severity: Severity::Error,
                    }],
                ))
            }
        }
    }

    /// Writes under `keys.` go to `keybindings.toml` when that file exists.
    fn redirect(&self, target: Target, segs: Vec<String>) -> Result<(Target, Vec<String>), KeltaError> {
        if target.kind == FileKind::Global && segs.first().map(String::as_str) == Some("keys") {
            let kb = self.inner.read().keybindings.clone();
            if kb.exists {
                let has_keys_table = toml_io::parse(&kb.text).is_ok_and(|p| p.value.get("keys").is_some());
                let segs = if has_keys_table {
                    segs
                } else if segs.len() > 1 {
                    segs[1..].to_vec()
                } else {
                    return Err(KeltaError::invalid(
                        "`keys` as a whole cannot be edited while keybindings.toml exists",
                    ));
                };
                return Ok((Target { path: kb.path, kind: FileKind::Keybindings }, segs));
            }
        }
        Ok((target, segs))
    }

    /// Serialize an edited document; new global / project files get the Taplo `#:schema` line.
    fn finish_text(target: &Target, doc: &DocumentMut) -> String {
        let body = doc.to_string();
        let new_file = !target.path.exists();
        let schema = match target.kind {
            FileKind::Global if new_file => Some("settings.schema.json"),
            FileKind::Project if new_file => Some("project.schema.json"),
            _ => None,
        };
        match schema {
            Some(s) => ensure_newline(format!("#:schema {SCHEMA_BASE_URL}/{s}\n{body}")),
            None => ensure_newline(body),
        }
    }

    /// `settings_set`: write one dotted key at a layer (toml_edit, atomic). `null` resets the key.
    pub fn layer_set(
        &self,
        layer: Layer,
        project: Option<&ProjectId>,
        repo_id: Option<&str>,
        path: &str,
        value: Value,
    ) -> Result<EffectiveSettings, KeltaError> {
        if value.is_null() {
            return self.layer_reset(layer, project, repo_id, path);
        }
        let segs = split_path(path).map_err(KeltaError::invalid)?;
        if segs.is_empty() {
            return Err(KeltaError::invalid("empty settings path"));
        }
        let target = self.resolve_target(layer, project, repo_id)?;
        let (target, segs) = self.redirect(target, segs)?;
        let g = self.gate.lock();
        let mut doc = self.read_doc(&target)?;
        edit::set_path(&mut doc, &segs, &value).map_err(KeltaError::invalid)?;
        self.commit_text_locked(&g, &target, Self::finish_text(&target, &doc))?;
        drop(g);
        self.effective_for(project)
    }

    /// `settings_reset`: remove one dotted key at a layer.
    pub fn layer_reset(
        &self,
        layer: Layer,
        project: Option<&ProjectId>,
        repo_id: Option<&str>,
        path: &str,
    ) -> Result<EffectiveSettings, KeltaError> {
        let segs = split_path(path).map_err(KeltaError::invalid)?;
        let target = self.resolve_target(layer, project, repo_id)?;
        let (target, segs) = self.redirect(target, segs)?;
        let g = self.gate.lock();
        if read_optional(&target.path)?.is_none() {
            drop(g);
            return self.effective_for(project);
        }
        let mut doc = self.read_doc(&target)?;
        if edit::remove_path(&mut doc, &segs) {
            self.commit_text_locked(&g, &target, Self::finish_text(&target, &doc))?;
        }
        drop(g);
        self.effective_for(project)
    }

    /// `settings_write_raw`: validate then replace a layer file.
    pub fn layer_write_raw(
        &self,
        layer: Layer,
        project: Option<&ProjectId>,
        repo_id: Option<&str>,
        text: &str,
    ) -> Result<EffectiveSettings, KeltaError> {
        let target = self.resolve_target(layer, project, repo_id)?;
        self.commit_text(&target, text.to_owned())?;
        self.effective_for(project)
    }

    // -----------------------------------------------------------------------------------------
    // projects
    // -----------------------------------------------------------------------------------------

    fn project_arc(&self, id: &str) -> Result<Arc<ProjectConfig>, KeltaError> {
        self.inner
            .read()
            .projects
            .get(id)
            .map(|e| e.config.clone())
            .ok_or_else(|| KeltaError::internal(format!("project `{id}` was written but did not load")))
    }

    /// Writes `projects/<id>.toml`.
    pub fn project_create(&self, draft: &ProjectDraft) -> Result<Arc<ProjectConfig>, KeltaError> {
        let id = draft.suggested_id.as_str().to_owned();
        if !validate::valid_slug(&id) || id == "home" || id == "inbox" {
            return Err(KeltaError::invalid(format!(
                "`{id}` is not a valid project id (use [a-z0-9-], 1-40 chars; `home` and `inbox` are reserved)"
            )));
        }
        let path = self.dirs.projects_dir().join(format!("{id}.toml"));
        let g = self.gate.lock();
        if path.exists() || self.inner.read().projects.contains_key(&id) {
            return Err(KeltaError::conflict(format!("project `{id}` already exists")));
        }
        let mut doc = DocumentMut::new();
        let set = |doc: &mut DocumentMut, p: &[&str], v: Value| {
            let segs: Vec<String> = p.iter().map(|s| (*s).to_owned()).collect();
            edit::set_path(doc, &segs, &v).map_err(KeltaError::invalid)
        };
        set(&mut doc, &["project", "id"], json!(id))?;
        set(
            &mut doc,
            &["project", "name"],
            json!(if draft.name.is_empty() { id.clone() } else { draft.name.clone() }),
        )?;
        if let Some(c) = &draft.color {
            set(&mut doc, &["project", "color"], json!(c))?;
            check_color(&doc)?;
        }
        if let Some(i) = &draft.icon {
            set(&mut doc, &["project", "icon"], json!(i))?;
        }
        if let Some(t) = &draft.default_template {
            set(&mut doc, &["project", "default_template"], json!(t))?;
        }
        if !draft.repos.is_empty() {
            set(&mut doc, &["project", "repos"], repos_json(&draft.repos))?;
        }
        let text = format!("#:schema {SCHEMA_BASE_URL}/project.schema.json\n{doc}");
        self.commit_text_locked(&g, &Target { path, kind: FileKind::Project }, ensure_newline(text))?;
        if let Some(t) = draft.tracker.as_ref().filter(|t| **t != TrackerBinding::default()) {
            let p = ProjectConfig {
                id: draft.suggested_id.clone(),
                name: draft.name.clone(),
                tracker: Some(t.clone()),
                ..ProjectConfig::default()
            };
            self.update_sources(&g, |s| *s = sources_from_trackers(s, std::slice::from_ref(&p)))?;
        }
        drop(g);
        self.project_arc(&id)
    }

    pub fn project_update(
        &self,
        id: &ProjectId,
        patch: &ProjectPatch,
    ) -> Result<Arc<ProjectConfig>, KeltaError> {
        let target = self.resolve_target(Layer::Project, Some(id), None)?;
        let g = self.gate.lock();
        let mut doc = self.read_doc(&target)?;
        let set = |doc: &mut DocumentMut, key: &str, v: Value| {
            edit::set_path(doc, &["project".to_owned(), key.to_owned()], &v).map_err(KeltaError::invalid)
        };
        if let Some(n) = &patch.name {
            set(&mut doc, "name", json!(n))?;
        }
        if let Some(c) = &patch.color {
            set(&mut doc, "color", json!(c))?;
            check_color(&doc)?;
        }
        if let Some(i) = &patch.icon {
            set(&mut doc, "icon", json!(i))?;
        }
        if let Some(t) = &patch.default_template {
            set(&mut doc, "default_template", json!(t))?;
        }
        if let Some(repos) = &patch.repos {
            if repos.is_empty() {
                edit::remove_path(&mut doc, &["project".to_owned(), "repos".to_owned()]);
            } else {
                set(&mut doc, "repos", repos_json(repos))?;
            }
        }
        // The board lives in `[[sources]]` (written first, so a failure keeps the old block);
        // a deprecated `[project.tracker]` block is superseded either way.
        if patch.remove_tracker {
            self.update_sources(&g, |s| {
                for src in s.iter_mut() {
                    src.projects.retain(|p| p != id);
                    src.routes.retain(|r| &r.project != id);
                }
            })?;
            edit::remove_path(&mut doc, &["project".to_owned(), "tracker".to_owned()]);
        } else if let Some(t) = &patch.tracker {
            let name = self
                .inner
                .read()
                .projects
                .get(id.as_str())
                .map(|e| e.config.name.clone())
                .unwrap_or_default();
            self.update_sources(&g, |s| match s.iter_mut().find(|src| src.projects.contains(id)) {
                Some(src) => {
                    src.tracker = TrackerBinding { repo_rules: Vec::new(), ..t.clone() };
                    src.routes.retain(|r| &r.project != id);
                    src.routes.extend(t.repo_rules.iter().map(|r| SourceRoute {
                        r#match: r.r#match.clone(),
                        project: id.clone(),
                        repo: r.repo.clone(),
                    }));
                }
                None => {
                    let p = ProjectConfig {
                        id: id.clone(),
                        name,
                        tracker: Some(t.clone()),
                        ..ProjectConfig::default()
                    };
                    *s = sources_from_trackers(s, std::slice::from_ref(&p));
                }
            })?;
            edit::remove_path(&mut doc, &["project".to_owned(), "tracker".to_owned()]);
        }
        self.commit_text_locked(&g, &target, Self::finish_text(&target, &doc))?;
        drop(g);
        self.project_arc(id.as_str())
    }

    /// Moves the file to `projects/.trash/`.
    pub fn project_remove(&self, id: &ProjectId) -> Result<(), KeltaError> {
        let target = self.resolve_target(Layer::Project, Some(id), None)?;
        let g = self.gate.lock();
        std::fs::rename(&target.path, trash_path(&self.dirs, id.as_str())?)?;
        drop(g);
        let _ = self.reload();
        Ok(())
    }

    /// Read-modify-write of the global `[[sources]]` under the gate; no write when `f` changes nothing.
    fn update_sources(
        &self,
        g: &MutexGuard<'_, ()>,
        f: impl FnOnce(&mut Vec<Source>),
    ) -> Result<(), KeltaError> {
        let target = self.resolve_target(Layer::Global, None, None)?;
        let mut doc = self.read_doc(&target)?;
        let value = toml_io::parse(&doc.to_string()).map_err(|i| KeltaError::invalid(i.message))?.value;
        let before: Vec<Source> = match value.get("sources") {
            Some(v) => serde_json::from_value(v.clone()).map_err(|e| {
                KeltaError::invalid(format!("fix `sources` in the global settings first: {e}"))
            })?,
            None => Vec::new(),
        };
        let mut sources = before.clone();
        f(&mut sources);
        if sources == before {
            return Ok(());
        }
        let segs = ["sources".to_owned()];
        if sources.is_empty() {
            edit::remove_path(&mut doc, &segs);
        } else {
            let v = edit::prune_empty(&serde_json::to_value(&sources)?);
            edit::set_path(&mut doc, &segs, &v).map_err(KeltaError::invalid)?;
            // `[sources.tracker]` and `[[sources.routes]]` read better than one long inline table
            let aot = doc.get_mut("sources").and_then(toml_edit::Item::as_array_of_tables_mut);
            for t in aot.into_iter().flat_map(|a| a.iter_mut()) {
                if let Some(i) = t.get_mut("tracker")
                    && let Some(inline) = i.as_inline_table().cloned()
                {
                    *i = toml_edit::Item::Table(inline.into_table());
                }
                if let Some(i) = t.get_mut("routes")
                    && let Some(a) = i.as_array().cloned()
                {
                    let tables =
                        a.iter().filter_map(|r| r.as_inline_table().cloned().map(|r| r.into_table()));
                    *i = toml_edit::Item::ArrayOfTables(tables.collect());
                }
            }
        }
        self.commit_text_locked(g, &target, Self::finish_text(&target, &doc))
    }

    /// Moves every loaded project's deprecated `[project.tracker]` to the global `[[sources]]`
    /// (#238). Nothing is removed before its copy exists: each project file is copied to
    /// `projects/.trash/<id>-tracker-<secs>.toml`, then the global file is written, then the blocks
    /// are stripped. Idempotent: a block already in `[[sources]]` only joins it. Returns the
    /// number of blocks moved.
    pub fn migrate_project_trackers(&self) -> Result<usize, KeltaError> {
        let g = self.gate.lock();
        let found: Vec<(PathBuf, ProjectConfig)> = self
            .inner
            .read()
            .projects
            .values()
            // a block naming no account at all is no source: it stays, unused, until edited
            .filter(|e| e.config.tracker.as_ref().is_some_and(|t| t.source_account().is_some()))
            .map(|e| (e.file.path.clone(), (*e.config).clone()))
            .collect();
        if found.is_empty() {
            return Ok(0);
        }
        for (path, p) in &found {
            std::fs::copy(path, trash_path(&self.dirs, &format!("{}-tracker", p.id))?)?;
        }
        let projects: Vec<ProjectConfig> = found.iter().map(|(_, p)| p.clone()).collect();
        self.update_sources(&g, |s| *s = sources_from_trackers(s, &projects))?;
        for (path, _) in &found {
            let target = Target { path: path.clone(), kind: FileKind::Project };
            let mut doc = self.read_doc(&target)?;
            edit::remove_path(&mut doc, &["project".to_owned(), "tracker".to_owned()]);
            self.commit_text_locked(&g, &target, Self::finish_text(&target, &doc))?;
        }
        Ok(found.len())
    }

    /// Tools are personal (#243): move every project file's `[[tools]]` to the global file, once.
    /// A new id is appended unchanged, an identical entry or an `id`/`enabled`-only override is
    /// dropped, a different definition under a taken id becomes `<id>-<project>`. The global file
    /// is validated before it is written, then each project file is copied to `projects/.trash/`
    /// and stripped. Returns the number of project entries moved.
    /// Runs on the loaded layers (a project's `tools` is dropped from its value and left as a
    /// warning on the file), so a load with nothing to migrate re-reads nothing.
    pub(crate) fn migrate_project_tools(&self) -> Result<usize, KeltaError> {
        let g = self.gate.lock();
        let mut projects = Vec::new();
        for (stem, e) in &self.inner.read().projects {
            if !e.file.issues.iter().any(|i| i.path == "tools") {
                continue;
            }
            // an unparsable file or a malformed `tools` is left alone: validation reports it
            let Ok(parsed) = toml_io::parse(&e.file.text) else { continue };
            let Some(Value::Array(entries)) = parsed.value.get("tools") else { continue };
            let name = parsed.value.pointer("/project/name").and_then(Value::as_str).unwrap_or(stem);
            projects.push((stem.clone(), name.to_owned(), e.file.path.clone(), entries.clone()));
        }
        if projects.is_empty() {
            return Ok(0);
        }
        let global = Target { path: self.dirs.global_config(), kind: FileKind::Global };
        let mut gdoc = self.read_doc(&global)?;
        let gvalue = toml_io::parse(&gdoc.to_string()).map_err(|i| KeltaError::invalid(i.message))?.value;
        let mut tools = match gvalue.get("tools") {
            None => Vec::new(),
            Some(Value::Array(a)) => a.clone(),
            Some(_) => return Err(KeltaError::invalid("global `tools` is not a list")),
        };
        let before = tools.len();
        let mut moved = 0;
        for (stem, name, _, entries) in &projects {
            moved += entries.len();
            for e in entries {
                merge_project_tool(&mut tools, e, stem, name);
            }
        }
        if tools.len() != before {
            edit::set_path(&mut gdoc, &["tools".to_owned()], &Value::Array(tools))
                .map_err(KeltaError::invalid)?;
            self.commit_text_locked(&g, &global, Self::finish_text(&global, &gdoc))?;
        }
        for (stem, _, path, _) in &projects {
            std::fs::copy(path, trash_path(&self.dirs, stem)?)?;
            let target = Target { path: path.clone(), kind: FileKind::Project };
            let mut doc = self.read_doc(&target)?;
            edit::remove_path(&mut doc, &["tools".to_owned()]);
            self.commit_text_locked(&g, &target, Self::finish_text(&target, &doc))?;
        }
        Ok(moved)
    }

    // -----------------------------------------------------------------------------------------
    // repo trust
    // -----------------------------------------------------------------------------------------

    /// Attach the persistent trust store and load the trusted hashes of every known repo file.
    /// Core calls this once the store is open, and [`Self::refresh_trust`] after projects change.
    pub async fn set_trust_store(&self, store: Arc<dyn TrustStore>) {
        *self.trust_store.write() = Some(store);
        self.refresh_trust().await;
    }

    /// Re-read trusted hashes for all repo config paths (new projects / repos) and re-apply.
    pub async fn refresh_trust(&self) {
        let store = self.trust_store.read().clone();
        let Some(store) = store else { return };
        for p in self.repo_config_paths() {
            if let Ok(Some(h)) = store.trusted_hash(&p).await {
                self.trusted.write().insert(p, h);
            }
        }
        let _ = self.reload();
    }

    /// `repo_trust`: trust the reviewed content of a repo's `.kelta/config.toml` (`sha256` = hash of
    /// the text the user was shown), or revoke trust (`trust = false`).
    pub async fn repo_trust(
        &self,
        project: &ProjectId,
        repo_id: &str,
        trust: bool,
        sha256: Option<&str>,
    ) -> Result<TrustInfo, KeltaError> {
        let target = self.resolve_target(Layer::Repo, Some(project), Some(repo_id))?;
        let sha256 = match (trust, sha256) {
            (true, None) => {
                return Err(KeltaError::invalid("trusting needs the sha256 of the reviewed file"));
            }
            (true, s) => s,
            (false, _) => None,
        };
        self.trust_file(&target.path, sha256).await
    }

    /// Trust `file` if its current content hashes to `sha256` (`None` revokes). Shared by the
    /// Settings banner and `kelta-ctl trust`; persisted in the trust store when attached.
    pub async fn trust_file(&self, file: &Path, sha256: Option<&str>) -> Result<TrustInfo, KeltaError> {
        let text = read_optional(file)?;
        let hash = sha256_hex(text.as_deref().unwrap_or("").as_bytes());
        if let Some(expected) = sha256 {
            if text.is_none() {
                return Err(KeltaError::not_found(format!("{} does not exist", file.display())));
            }
            if !expected.eq_ignore_ascii_case(&hash) {
                return Err(KeltaError::conflict(format!(
                    "{} changed since it was reviewed; review it again",
                    file.display()
                )));
            }
        }
        let store = self.trust_store.read().clone();
        if let Some(s) = &store {
            s.set_trust(file, sha256.map(|_| hash.clone())).await?;
        }
        {
            let mut t = self.trusted.write();
            match sha256 {
                Some(_) => t.insert(file.to_path_buf(), hash.clone()),
                None => t.remove(file),
            };
        }
        let _ = self.reload();
        Ok(TrustInfo { path: file.to_path_buf(), hash, trusted: sha256.is_some() })
    }

    /// `ProjectConfig` repo config path helper for callers (diagnostics, banners).
    pub fn repo_config_path(&self, project: &ProjectId, repo_id: &str) -> Result<PathBuf, KeltaError> {
        Ok(self.resolve_target(Layer::Repo, Some(project), Some(repo_id))?.path)
    }

    /// Expanded path of a configured repo (`~` resolved).
    pub fn repo_path(&self, project: &ProjectId, repo_id: &str) -> Option<PathBuf> {
        let inner = self.inner.read();
        let e = inner.projects.get(project.as_str())?;
        e.config.repos.iter().find(|r| r.id == repo_id).map(|r| expand_home(&r.path))
    }
}

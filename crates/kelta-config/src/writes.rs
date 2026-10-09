//! Layer reads and writes: `toml_edit` edits, raw writes with validation, project file CRUD
//! (`.trash` on removal) and repo trust.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use parking_lot::MutexGuard;

use kelta_proto::api::TrustStore;
use kelta_proto::error::KeltaError;
use kelta_proto::ids::ProjectId;
use kelta_proto::model::{ProjectDraft, ProjectPatch, RepoDraft};
use kelta_proto::schema::SCHEMA_BASE_URL;
use kelta_proto::settings::{EffectiveSettings, Layer, LayerDoc, ProjectConfig, TrustInfo, ValidationIssue};
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
        let mut raw = validate::layer_issues(layer, &parsed.value, None);
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
                Layer::Global => self.compute(&pd, &parsed.value, None, &[], None),
                Layer::Project => {
                    let dt = parsed
                        .value
                        .pointer("/project/default_template")
                        .and_then(Value::as_str)
                        .map(str::to_owned);
                    self.compute(&pd, &gv, Some(&parsed.value), &[], dt.as_deref())
                }
                _ => self.compute(&pd, &gv, None, std::slice::from_ref(&parsed.value), None),
            };
            if let Err(e) = res {
                raw.extend(e);
            }
        }
        Ok(validate::position(text, Some(&parsed), raw))
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
        if let Some(t) = &draft.tracker {
            let v = edit::prune_empty(&serde_json::to_value(t)?);
            if v.as_object().is_some_and(|m| !m.is_empty()) {
                set(&mut doc, &["project", "tracker"], v)?;
            }
        }
        let text = format!("#:schema {SCHEMA_BASE_URL}/project.schema.json\n{doc}");
        self.commit_text_locked(&g, &Target { path, kind: FileKind::Project }, ensure_newline(text))?;
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
        if patch.remove_tracker {
            edit::remove_path(&mut doc, &["project".to_owned(), "tracker".to_owned()]);
        } else if let Some(t) = &patch.tracker {
            let v = edit::prune_empty(&serde_json::to_value(t)?);
            set(&mut doc, "tracker", v)?;
        }
        self.commit_text_locked(&g, &target, Self::finish_text(&target, &doc))?;
        drop(g);
        self.project_arc(id.as_str())
    }

    /// Moves the file to `projects/.trash/`.
    pub fn project_remove(&self, id: &ProjectId) -> Result<(), KeltaError> {
        let target = self.resolve_target(Layer::Project, Some(id), None)?;
        let trash = self.dirs.projects_dir().join(".trash");
        let g = self.gate.lock();
        std::fs::create_dir_all(&trash)?;
        let secs =
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs());
        let mut dest = trash.join(format!("{}-{secs}.toml", id.as_str()));
        let mut n = 1;
        while dest.exists() {
            dest = trash.join(format!("{}-{secs}-{n}.toml", id.as_str()));
            n += 1;
        }
        std::fs::rename(&target.path, &dest)?;
        drop(g);
        let _ = self.reload();
        Ok(())
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

    /// `repo_trust`: trust (or revoke) the current content of a repo's `.kelta/config.toml`.
    pub async fn repo_trust(
        &self,
        project: &ProjectId,
        repo_id: &str,
        trust: bool,
    ) -> Result<TrustInfo, KeltaError> {
        let target = self.resolve_target(Layer::Repo, Some(project), Some(repo_id))?;
        let text = read_optional(&target.path)?;
        let hash = sha256_hex(text.as_deref().unwrap_or("").as_bytes());
        if trust && text.is_none() {
            return Err(KeltaError::not_found(format!("{} does not exist", target.path.display())));
        }
        let store = self.trust_store.read().clone();
        if let Some(s) = &store {
            s.set_trust(&target.path, trust.then(|| hash.clone())).await?;
        }
        {
            let mut t = self.trusted.write();
            if trust {
                t.insert(target.path.clone(), hash.clone());
            } else {
                t.remove(&target.path);
            }
        }
        let _ = self.reload();
        Ok(TrustInfo { path: target.path, hash, trusted: trust })
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

//! ProjectRegistry (SPEC §2): the built-in Home project + configured projects; open set, order and
//! active project persisted in `projects_open`. Closing or switching never touches processes
//! unless `kill_sessions` is requested.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use kelta_proto::error::KeltaError;
use kelta_proto::events::{BusEvent, UiEvent, bus};
use kelta_proto::ids::ProjectId;
use kelta_proto::model::{AttentionSummary, ProjectDraft, ProjectInfo, ProjectPatch, RepoInfo};
use kelta_proto::settings::ProjectConfig;
use kelta_proto::store::ProjectOpenRow;

use crate::Core;
use crate::store::q;

#[derive(Debug, Default, Clone)]
pub struct ProjectsState {
    /// Open projects in rail order.
    pub open: Vec<ProjectId>,
    pub active: Option<ProjectId>,
}

impl ProjectsState {
    fn rows(&self) -> Vec<ProjectOpenRow> {
        self.open
            .iter()
            .enumerate()
            .map(|(i, p)| ProjectOpenRow {
                project_id: p.clone(),
                ord: i as u32,
                active: self.active.as_ref() == Some(p),
            })
            .collect()
    }
}

pub fn home_dir(login: &kelta_proto::term::LoginEnv) -> PathBuf {
    login
        .get("HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(PathBuf::from))
        .unwrap_or_else(|| PathBuf::from("/"))
}

pub fn repo_path(raw: &str, home: &Path) -> PathBuf {
    PathBuf::from(crate::spawn_env::expand_tilde(raw, Some(&home.to_string_lossy())))
}

impl Core {
    pub(crate) fn load_projects(&self) -> Result<(), KeltaError> {
        let rows = self.store.call_blocking(|c| q::projects_open(c))?;
        let known = self.known_project_ids();
        let mut st = ProjectsState::default();
        for r in rows {
            if known.contains(&r.project_id) && !st.open.contains(&r.project_id) {
                if r.active {
                    st.active = Some(r.project_id.clone());
                }
                st.open.push(r.project_id);
            }
        }
        let home = ProjectId::home();
        if !st.open.contains(&home) {
            st.open.push(home.clone());
        }
        if st.active.is_none() {
            st.active = st.open.first().cloned();
        }
        let rows = st.rows();
        *self.projects.lock() = st;
        self.store.exec("projects_open", move |c| q::projects_open_replace(c, &rows));
        Ok(())
    }

    fn persist_projects(&self) {
        let rows = self.projects.lock().rows();
        self.store.exec("projects_open", move |c| q::projects_open_replace(c, &rows));
    }

    pub(crate) fn known_project_ids(&self) -> BTreeSet<ProjectId> {
        let mut s: BTreeSet<ProjectId> = self.cfg.projects().iter().map(|p| p.id.clone()).collect();
        s.insert(ProjectId::home());
        s
    }

    pub(crate) fn project_exists(&self, id: &ProjectId) -> bool {
        id.as_str() == ProjectId::HOME || self.cfg.project(id).is_some()
    }

    pub fn active_project(&self) -> ProjectId {
        self.projects.lock().active.clone().unwrap_or_else(ProjectId::home)
    }

    pub fn open_projects(&self) -> Vec<ProjectId> {
        self.projects.lock().open.clone()
    }

    pub(crate) fn home_dir(&self) -> PathBuf {
        home_dir(&self.login_env)
    }

    /// Default working directory of a project: primary repo, first repo, else `$HOME`.
    pub(crate) fn project_cwd(&self, id: &ProjectId, repo_id: Option<&str>) -> PathBuf {
        let home = self.home_dir();
        let Some(cfg) = self.cfg.project(id) else { return home };
        let pick = repo_id
            .and_then(|r| cfg.repos.iter().find(|x| x.id == r))
            .or_else(|| cfg.repos.iter().find(|r| r.primary))
            .or_else(|| cfg.repos.first());
        pick.map(|r| repo_path(&r.path, &home)).filter(|p| p.is_dir()).unwrap_or(home)
    }

    fn info_from(&self, cfg: Option<&ProjectConfig>, id: &ProjectId) -> ProjectInfo {
        let st = self.projects.lock().clone();
        let attention = self.project_attention(id);
        let home = self.home_dir();
        match cfg {
            Some(c) => ProjectInfo {
                id: c.id.clone(),
                name: if c.name.is_empty() { c.id.to_string() } else { c.name.clone() },
                color: c.color.clone(),
                icon: c.icon.clone(),
                repos: c
                    .repos
                    .iter()
                    .map(|r| {
                        let path = repo_path(&r.path, &home);
                        RepoInfo {
                            id: r.id.clone(),
                            exists: crate::detect::git_dir(&path).is_some(),
                            path,
                            primary: r.primary,
                            remote: r.remote.clone(),
                            base: r.base.clone(),
                            code_host: r.code_host.clone(),
                        }
                    })
                    .collect(),
                tracker: c.tracker.clone(),
                open: st.open.contains(id),
                active: st.active.as_ref() == Some(id),
                attention,
                builtin: false,
            },
            None => ProjectInfo {
                id: ProjectId::home(),
                name: "Home".into(),
                color: None,
                icon: None,
                repos: Vec::new(),
                tracker: None,
                open: st.open.contains(id),
                active: st.active.as_ref() == Some(id),
                attention,
                builtin: true,
            },
        }
    }

    /// `CoreApi::project`.
    pub fn project_info(&self, id: &ProjectId) -> Option<ProjectInfo> {
        if id.as_str() == ProjectId::HOME {
            return Some(self.info_from(None, id));
        }
        self.cfg.project(id).map(|c| self.info_from(Some(&c), id))
    }

    fn project_attention(&self, id: &ProjectId) -> AttentionSummary {
        self.attention.lock().last.get(id).copied().unwrap_or_default()
    }

    /// `project_list`: open projects in rail order, then Home if closed, then the closed ones by name.
    pub fn project_list(&self) -> Vec<ProjectInfo> {
        let open = self.open_projects();
        let mut out: Vec<ProjectInfo> = open.iter().filter_map(|id| self.project_info(id)).collect();
        let mut rest: Vec<ProjectInfo> = self
            .cfg
            .projects()
            .iter()
            .filter(|c| !open.contains(&c.id))
            .map(|c| self.info_from(Some(c), &c.id))
            .collect();
        rest.sort_by_key(|a| a.name.to_lowercase());
        out.extend(rest);
        out
    }

    pub(crate) fn emit_project(&self, id: &ProjectId) {
        if let Some(p) = self.project_info(id) {
            self.emit(UiEvent::ProjectUpdated { project: p });
        }
    }

    pub(crate) fn emit_all_projects(&self) {
        for p in self.project_list() {
            self.emit(UiEvent::ProjectUpdated { project: p });
        }
    }

    fn require(&self, id: &ProjectId) -> Result<(), KeltaError> {
        if self.project_exists(id) { Ok(()) } else { Err(KeltaError::not_found(format!("project {id}"))) }
    }

    /// `project_detect`.
    pub fn project_detect(&self, path: &Path) -> Result<ProjectDraft, KeltaError> {
        let home = self.home_dir();
        let path = repo_path(&path.to_string_lossy(), &home);
        if !path.is_dir() {
            return Err(KeltaError::not_found(format!("{} is not a directory", path.display())));
        }
        let accounts = self.cfg.effective(None).accounts.clone();
        Ok(crate::detect::detect(&path, &accounts, &self.known_project_ids()))
    }

    /// `project_create`: write `projects/<id>.toml`, open and activate it.
    pub fn project_create(&self, draft: &ProjectDraft) -> Result<ProjectInfo, KeltaError> {
        if !ProjectId::is_valid_slug(draft.suggested_id.as_str())
            || draft.suggested_id.as_str() == ProjectId::HOME
            || draft.suggested_id.as_str() == ProjectId::INBOX
        {
            return Err(KeltaError::invalid(format!("invalid project id `{}`", draft.suggested_id)));
        }
        if self.project_exists(&draft.suggested_id) {
            return Err(KeltaError::conflict(format!("project {} exists", draft.suggested_id)));
        }
        let cfg = self.cfg.project_create(draft)?;
        let id = cfg.id.clone();
        self.project_open(&id)?;
        self.project_activate(&id)
    }

    /// `project_update`.
    pub fn project_update(&self, id: &ProjectId, patch: &ProjectPatch) -> Result<ProjectInfo, KeltaError> {
        if id.as_str() == ProjectId::HOME {
            return Err(KeltaError::invalid("the Home project cannot be edited"));
        }
        self.require(id)?;
        let cfg = self.cfg.project_update(id, patch)?;
        let info = self.info_from(Some(&cfg), id);
        self.emit(UiEvent::ProjectUpdated { project: info.clone() });
        self.resubscribe();
        Ok(info)
    }

    /// `project_remove`: config moved to `projects/.trash/`; sessions kept unless `kill_sessions`.
    pub fn project_remove(&self, id: &ProjectId, kill_sessions: bool) -> Result<(), KeltaError> {
        if id.as_str() == ProjectId::HOME {
            return Err(KeltaError::invalid("the Home project cannot be removed"));
        }
        self.require(id)?;
        if kill_sessions {
            self.kill_project_sessions(id);
        }
        self.cfg.project_remove(id)?;
        self.forget_open(id);
        self.layouts.lock().remove(id);
        let pid = id.clone();
        self.store.exec("layout_delete", move |c| q::layout_delete(c, &pid));
        self.emit(UiEvent::ProjectRemoved { id: id.clone() });
        self.resubscribe();
        Ok(())
    }

    fn forget_open(&self, id: &ProjectId) {
        let new_active = {
            let mut st = self.projects.lock();
            st.open.retain(|p| p != id);
            if st.active.as_ref() == Some(id) {
                st.active = st.open.first().cloned().or_else(|| Some(ProjectId::home()));
                st.active.clone()
            } else {
                None
            }
        };
        self.persist_projects();
        if let Some(a) = new_active {
            self.emit_project(&a);
            self.on_visibility_changed();
        }
    }

    /// `project_open` (rail): never touches sessions.
    pub fn project_open(&self, id: &ProjectId) -> Result<ProjectInfo, KeltaError> {
        self.require(id)?;
        let opened = {
            let mut st = self.projects.lock();
            if st.open.contains(id) {
                false
            } else {
                st.open.push(id.clone());
                true
            }
        };
        if opened {
            self.persist_projects();
            self.publish_project_event(bus::PROJECT_OPENED, id);
            self.resubscribe();
        }
        let info = self.project_info(id).ok_or_else(|| KeltaError::not_found(format!("project {id}")))?;
        self.emit(UiEvent::ProjectUpdated { project: info.clone() });
        Ok(info)
    }

    /// `project_close`: sessions keep running unless `kill_sessions`.
    pub fn project_close(&self, id: &ProjectId, kill_sessions: bool) -> Result<ProjectInfo, KeltaError> {
        self.require(id)?;
        if kill_sessions {
            self.kill_project_sessions(id);
        }
        if id.as_str() != ProjectId::HOME {
            let was_open = self.projects.lock().open.contains(id);
            self.forget_open(id);
            if was_open {
                self.publish_project_event(bus::PROJECT_CLOSED, id);
            }
            self.resubscribe();
        }
        let info = self.project_info(id).ok_or_else(|| KeltaError::not_found(format!("project {id}")))?;
        self.emit(UiEvent::ProjectUpdated { project: info.clone() });
        Ok(info)
    }

    /// `project_activate` (opens it if needed). Never touches processes.
    pub fn project_activate(&self, id: &ProjectId) -> Result<ProjectInfo, KeltaError> {
        self.require(id)?;
        let (prev, opened) = {
            let mut st = self.projects.lock();
            let opened = if st.open.contains(id) {
                false
            } else {
                st.open.push(id.clone());
                true
            };
            (st.active.replace(id.clone()), opened)
        };
        self.persist_projects();
        if opened {
            self.publish_project_event(bus::PROJECT_OPENED, id);
        }
        if prev.as_ref() != Some(id) {
            if let Some(p) = &prev {
                self.emit_project(p);
            }
            self.publish_project_event(bus::PROJECT_ACTIVATED, id);
            self.on_visibility_changed();
        }
        self.resubscribe();
        let info = self.project_info(id).ok_or_else(|| KeltaError::not_found(format!("project {id}")))?;
        self.emit(UiEvent::ProjectUpdated { project: info.clone() });
        Ok(info)
    }

    /// `project_reorder`: the given ids first (in order), other open projects keep their order.
    pub fn project_reorder(&self, ids: &[ProjectId]) -> Result<(), KeltaError> {
        {
            let mut st = self.projects.lock();
            let mut seen = std::collections::HashSet::new();
            let mut next: Vec<ProjectId> =
                ids.iter().filter(|i| st.open.contains(i) && seen.insert(*i)).cloned().collect();
            for p in &st.open {
                if !next.contains(p) {
                    next.push(p.clone());
                }
            }
            st.open = next;
        }
        self.persist_projects();
        Ok(())
    }

    fn publish_project_event(&self, name: &str, id: &ProjectId) {
        self.publish_ev(
            BusEvent::new(name, serde_json::json!({ "project_id": id })).with_project(id.clone()),
        );
    }

    fn kill_project_sessions(&self, id: &ProjectId) {
        let ids: Vec<_> = self.list_sessions(Some(id)).into_iter().map(|s| s.id).collect();
        for sid in ids {
            if let Err(e) = self.kill_session(&sid, false) {
                tracing::warn!(error = %e, session = %sid, "kill on project close failed");
            }
        }
    }

    /// Project ids whose repos contain `path` (longest repo path first).
    pub fn projects_for_path(&self, path: &Path) -> Vec<ProjectId> {
        let home = self.home_dir();
        let mut hits: Vec<(usize, ProjectId)> = Vec::new();
        for p in self.cfg.projects().iter() {
            for r in &p.repos {
                let rp = repo_path(&r.path, &home);
                if path.starts_with(&rp) {
                    hits.push((rp.as_os_str().len(), p.id.clone()));
                }
            }
        }
        hits.sort_by_key(|h| std::cmp::Reverse(h.0));
        hits.into_iter().map(|(_, id)| id).collect()
    }

    pub(crate) fn project_configs(&self) -> Vec<Arc<ProjectConfig>> {
        self.cfg.projects()
    }
}

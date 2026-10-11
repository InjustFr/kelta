//! Layout persistence (ARCHITECTURE §5.1, §10): one `Layout` per project kept in memory, written
//! through to `layouts`; optimistic `rev` (stale → `Conflict` with `detail.rev`); backend-initiated
//! changes emit `layout.changed`.

use std::collections::{HashMap, HashSet};

use kelta_proto::error::KeltaError;
use kelta_proto::events::UiEvent;
use kelta_proto::ids::{ProjectId, SessionId};
use kelta_proto::ipc::LayoutSaveResult;
use kelta_proto::model::{Layout, LayoutNode, OpenPaneRequest, PaneContent, PaneRef, Tab};
use kelta_proto::store::LayoutRow;

use crate::Core;
use crate::layout;
use crate::store::q;

/// Where the tabs of a pre-#270 project with the (now reserved) id `global` go.
const GLOBAL_PROJECT: &str = "global-project";

/// Before #270 a project could have the id `global`, now the views' layout (the config refuses that id
/// since): its tabs, any that is not a view's, move once to `global-project`, so they never mix with the
/// views and come back if the project is renamed so. `true` when tabs moved.
fn split_global_project(map: &mut HashMap<ProjectId, Layout>) -> bool {
    const VIEWS: [&str; 3] = ["tickets", "now", "reviews"];
    let global = ProjectId::new(ProjectId::GLOBAL);
    let moved = ProjectId::new(GLOBAL_PROJECT);
    if map.contains_key(&moved) {
        return false;
    }
    let Some(l) = map.get_mut(&global) else { return false };
    let (views, tabs): (Vec<Tab>, Vec<Tab>) =
        std::mem::take(&mut l.tabs).into_iter().partition(|t| VIEWS.contains(&t.id.as_str()));
    l.tabs = views;
    if tabs.is_empty() {
        return false;
    }
    let active = l.active_tab.take_if(|a| !VIEWS.contains(&a.as_str()));
    l.rev += 1;
    if l.tabs.is_empty() {
        // Never a view: the UI seeds the views as on a first start.
        map.remove(&global);
    }
    map.insert(moved.clone(), Layout { project_id: moved, tabs, active_tab: active, rev: 1 });
    true
}

impl Core {
    pub(crate) fn load_layouts(&self) -> Result<(), KeltaError> {
        let rows = self.store.call_blocking(|c| q::layouts(c))?;
        let mut map = self.layouts.lock();
        for r in rows {
            match serde_json::from_str::<Layout>(&r.json) {
                Ok(mut l) => {
                    l.rev = r.rev;
                    l.project_id = r.project_id.clone();
                    map.insert(r.project_id, l);
                }
                Err(e) => tracing::warn!(error = %e, project = %r.project_id, "dropping unreadable layout"),
            }
        }
        let split = split_global_project(&mut map);
        drop(map);
        if split {
            let global = ProjectId::new(ProjectId::GLOBAL);
            self.persist_layout(&ProjectId::new(GLOBAL_PROJECT));
            if self.layouts.lock().contains_key(&global) {
                self.persist_layout(&global);
            } else {
                self.store.exec("layout_delete", move |c| q::layout_delete(c, &global));
            }
        }
        Ok(())
    }

    /// Drop terminal panes of sessions that no longer exist (startup).
    pub(crate) fn prune_layouts(&self, live: &HashSet<SessionId>) {
        let changed: Vec<ProjectId> = {
            let mut map = self.layouts.lock();
            map.iter_mut()
                .filter_map(|(p, l)| {
                    let gone: Vec<SessionId> =
                        layout::layout_sessions(l).into_iter().filter(|s| !live.contains(s)).collect();
                    let mut any = false;
                    for s in gone {
                        any |= layout::remove_session(l, &s);
                    }
                    any.then(|| {
                        l.rev += 1;
                        p.clone()
                    })
                })
                .collect()
        };
        for p in changed {
            self.persist_layout(&p);
        }
    }

    fn persist_layout(&self, project: &ProjectId) {
        let row = self.layouts.lock().get(project).map(|l| {
            serde_json::to_string(l).map(|json| LayoutRow {
                project_id: project.clone(),
                json,
                rev: l.rev,
                updated_at: kelta_proto::now_rfc3339(),
            })
        });
        match row {
            Some(Ok(r)) => self.store.exec("layout_put", move |c| q::layout_put(c, &r)),
            Some(Err(e)) => tracing::warn!(error = %e, "cannot serialize layout"),
            None => {}
        }
    }

    /// Persist + emit `layout.changed` (backend-initiated changes only) + visibility follow-ups.
    pub(crate) fn layout_changed(&self, project: &ProjectId) {
        self.persist_layout(project);
        if let Some(l) = self.layouts.lock().get(project).cloned() {
            self.emit(UiEvent::LayoutChanged { project_id: project.clone(), layout: l });
        }
        self.on_visibility_changed();
    }

    /// Visible panes changed (layout save, project switch): scheduler subscriptions follow.
    pub(crate) fn on_visibility_changed(&self) {
        self.resubscribe();
    }

    /// A project's layout, or the global views' built-in one (`ProjectId::GLOBAL`).
    fn has_layout(&self, id: &ProjectId) -> bool {
        id.as_str() == ProjectId::GLOBAL || self.project_exists(id)
    }

    /// `layout_get` (empty layout with `rev = 0` for a project never saved).
    pub fn layout_get(&self, project: &ProjectId) -> Result<Layout, KeltaError> {
        if !self.has_layout(project) {
            return Err(KeltaError::not_found(format!("project {project}")));
        }
        Ok(self.layouts.lock().get(project).cloned().unwrap_or_else(|| layout::empty(project)))
    }

    /// `layout_save`: optimistic concurrency on `rev`.
    pub fn layout_save(&self, mut l: Layout) -> Result<LayoutSaveResult, KeltaError> {
        if !self.has_layout(&l.project_id) {
            return Err(KeltaError::not_found(format!("project {}", l.project_id)));
        }
        let rev = {
            let mut map = self.layouts.lock();
            let current = map.get(&l.project_id).map(|x| x.rev).unwrap_or(0);
            if l.rev != current {
                return Err(KeltaError::conflict(format!("stale layout rev {} (current {current})", l.rev))
                    .with_detail(serde_json::json!({ "rev": current })));
            }
            l.rev = current + 1;
            let rev = l.rev;
            map.insert(l.project_id.clone(), l.clone());
            rev
        };
        self.persist_layout(&l.project_id);
        self.on_visibility_changed();
        Ok(LayoutSaveResult { rev })
    }

    /// `CoreApi::layout_open`: new tab / split / replace / focus existing, then `layout.changed`.
    pub(crate) fn open_pane(&self, project: &ProjectId, req: OpenPaneRequest) -> Result<PaneRef, KeltaError> {
        self.open_node(project, layout::pane(req.content.clone()), &req)
    }

    /// Open a whole subtree (templates) at `req.placement`.
    pub(crate) fn open_node(
        &self,
        project: &ProjectId,
        node: LayoutNode,
        req: &OpenPaneRequest,
    ) -> Result<PaneRef, KeltaError> {
        if !self.project_exists(project) {
            return Err(KeltaError::not_found(format!("project {project}")));
        }
        if let PaneContent::Terminal { session_id } = &req.content {
            // Moving a session to another project's layout vacates it there.
            let others: Vec<ProjectId> = self
                .layouts
                .lock()
                .iter_mut()
                .filter(|(p, _)| *p != project)
                .filter_map(|(p, l)| {
                    layout::remove_session(l, session_id).then(|| {
                        l.rev += 1;
                        p.clone()
                    })
                })
                .collect();
            for p in others {
                self.layout_changed(&p);
            }
        }
        let (tab, pane) = {
            let mut map = self.layouts.lock();
            let l = map.entry(project.clone()).or_insert_with(|| layout::empty(project));
            let loc = layout::open_node(l, node, req);
            l.rev += 1;
            loc
        };
        self.layout_changed(project);
        Ok(PaneRef { project_id: project.clone(), tab_id: tab, pane_id: pane })
    }
}

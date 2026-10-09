//! Layout persistence (ARCHITECTURE §5.1, §10): one `Layout` per project kept in memory, written
//! through to `layouts`; optimistic `rev` (stale → `Conflict` with `detail.rev`); backend-initiated
//! changes emit `layout.changed`.

use std::collections::HashSet;

use kelta_proto::error::KeltaError;
use kelta_proto::events::UiEvent;
use kelta_proto::ids::{ProjectId, SessionId};
use kelta_proto::ipc::LayoutSaveResult;
use kelta_proto::model::{Layout, LayoutNode, OpenPaneRequest, PaneContent, PaneRef};
use kelta_proto::store::LayoutRow;

use crate::Core;
use crate::layout;
use crate::store::q;

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

    /// `layout_get` (empty layout with `rev = 0` for a project never saved).
    pub fn layout_get(&self, project: &ProjectId) -> Result<Layout, KeltaError> {
        if !self.project_exists(project) {
            return Err(KeltaError::not_found(format!("project {project}")));
        }
        Ok(self.layouts.lock().get(project).cloned().unwrap_or_else(|| layout::empty(project)))
    }

    /// `layout_save`: optimistic concurrency on `rev`.
    pub fn layout_save(&self, mut l: Layout) -> Result<LayoutSaveResult, KeltaError> {
        if !self.project_exists(&l.project_id) {
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

//! Layout persistence: optimistic `rev` conflicts, backend `layout_open`, a session in at most
//! one pane, removal of killed sessions.

use crate::common::*;
use kelta_proto::ErrorCode;
use kelta_proto::api::CoreApi;
use kelta_proto::ids::ProjectId;
use kelta_proto::model::{
    OpenPaneRequest, PaneContent, Placement, Scope, SessionKind, SpawnRequest, TicketsMode,
};
use kelta_proto::settings::Settings;

fn open(content: PaneContent, placement: Placement) -> OpenPaneRequest {
    OpenPaneRequest { content, placement, focus: true, tab_title: None, work_item_id: None }
}

#[tokio::test]
async fn save_conflicts_on_stale_rev() {
    let tmp = tempfile::tempdir().unwrap();
    let shop = ProjectId::new("shop");
    {
        let h = start(tmp.path(), Settings::defaults(), vec![project("shop", tmp.path())]);
        let l0 = h.core.layout_get(&shop).unwrap();
        assert_eq!((l0.rev, l0.tabs.len()), (0, 0));
        let mut l = l0.clone();
        l.tabs.push(kelta_proto::samples::layout().tabs[0].clone());
        assert_eq!(h.core.layout_save(l.clone()).unwrap().rev, 1);
        let err = h.core.layout_save(l.clone()).unwrap_err();
        assert_eq!(err.code, ErrorCode::Conflict);
        assert_eq!(err.detail.unwrap()["rev"], 1);
        l.rev = 1;
        assert_eq!(h.core.layout_save(l).unwrap().rev, 2);
        assert_eq!(h.core.layout_get(&ProjectId::new("ghost")).unwrap_err().code, ErrorCode::NotFound);
        h.core.store().flush().await.unwrap();
    }
    // persisted (rev and tabs), sessions that no longer exist are pruned
    let h = start(tmp.path(), Settings::defaults(), vec![project("shop", tmp.path())]);
    let l = h.core.layout_get(&shop).unwrap();
    assert!(l.rev >= 2);
    assert!(kelta_core::layout::layout_sessions(&l).is_empty());
}

#[tokio::test]
async fn backend_open_moves_sessions_and_kill_removes_panes() {
    let tmp = tempfile::tempdir().unwrap();
    let h = start(
        tmp.path(),
        Settings::defaults(),
        vec![project("shop", tmp.path()), project("blog", tmp.path())],
    );
    let shop = ProjectId::new("shop");
    let blog = ProjectId::new("blog");
    let s = h
        .core
        .session_spawn(SpawnRequest {
            id: None,
            project_id: shop.clone(),
            kind: SessionKind::Shell,
            name: None,
            program: None,
            args: vec![],
            cwd: None,
            env: Default::default(),
            cols: 80,
            rows: 24,
            work_item_id: None,
            restore: Default::default(),
            close_on_exit: Default::default(),
            template_id: None,
        })
        .await
        .unwrap();
    let term = PaneContent::Terminal { session_id: s.id.clone() };
    let p1 = h.core.layout_open(&shop, open(term.clone(), Placement::NewTab)).await.unwrap();
    let tickets = PaneContent::Tickets {
        scope: Scope::Project { id: shop.clone() },
        view_id: None,
        mode: TicketsMode::Board,
        who: None,
        group: None,
        sort: None,
        person: None,
    };
    let p2 = h.core.layout_open(&shop, open(tickets.clone(), Placement::SplitRight)).await.unwrap();
    assert_eq!(p1.tab_id, p2.tab_id);
    let l = h.core.layout_get(&shop).unwrap();
    assert_eq!(l.rev, 2);
    assert!(h.ui.event_names().iter().filter(|n| **n == "layout.changed").count() >= 2);
    // the UI's copy at rev 1 is stale now
    let mut stale = l.clone();
    stale.rev = 1;
    assert_eq!(h.core.layout_save(stale).unwrap_err().code, ErrorCode::Conflict);
    // Focused placement focuses the existing pane, no new tab
    let p3 = h.core.layout_open(&shop, open(tickets, Placement::Focused)).await.unwrap();
    assert_eq!(p3.pane_id, p2.pane_id);
    // the session moves to blog: vacated in shop
    h.core.layout_open(&blog, open(term, Placement::NewTab)).await.unwrap();
    assert!(kelta_core::layout::layout_sessions(&h.core.layout_get(&shop).unwrap()).is_empty());
    assert_eq!(kelta_core::layout::layout_sessions(&h.core.layout_get(&blog).unwrap()), vec![s.id.clone()]);
    // kill → removed from the registry and the layout
    h.core.session_kill(&s.id, false).await.unwrap();
    assert!(h.core.session_get(&s.id).is_none());
    assert!(h.core.layout_get(&blog).unwrap().tabs.is_empty());
}

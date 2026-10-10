//! Claude slots and the run queue (#141).

use std::time::Duration;

use crate::common::{Fx, has_git, project};
use kelta_proto::api::WorkStore;
use kelta_proto::ids::WorkItemId;
use kelta_proto::model::{SessionKind, StepStatus, WorkItem, WorkSource, WorkState};

async fn start(w: &kelta_work::WorkService, name: &str) -> WorkItem {
    let source = WorkSource::Branch { name: name.into(), task: None, repo: None };
    let plan = w.plan(&project(), source).await.unwrap();
    w.start(plan).await.unwrap()
}

fn state(items: &[WorkItem], id: &WorkItemId) -> WorkState {
    items.iter().find(|w| &w.id == id).unwrap().state.clone()
}

/// Wait (polling, test-only) until `id` is Active.
async fn wait_active(fx: &Fx, id: &WorkItemId) {
    for _ in 0..500 {
        if fx.store.get_item(id).await.unwrap().is_some_and(|w| w.state == WorkState::Active) {
            return;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("{id} never started");
}

fn live_claude(fx: &Fx) -> Vec<kelta_proto::model::SessionInfo> {
    fx.spawned_of(|k| *k == SessionKind::Claude)
        .into_iter()
        .filter(|s| s.lifecycle == kelta_proto::model::Lifecycle::Live)
        .collect()
}

#[tokio::test]
async fn cap_queues_with_worktrees_then_dequeues_in_order() {
    if !has_git() {
        return;
    }
    let fx = Fx::new();
    fx.settings(|s| s.claude.max_live = 2);
    let w = fx.service();
    let mut items = Vec::new();
    for n in 1..=4 {
        items.push(start(&w, &format!("q/{n}")).await);
    }
    let states: Vec<WorkState> = items.iter().map(|i| i.state.clone()).collect();
    assert_eq!(
        states,
        [WorkState::Active, WorkState::Active, WorkState::Queued { pos: 0 }, WorkState::Queued { pos: 1 }]
    );
    // Start work again on a queued item: it keeps its place.
    let again = w.resume(&items[2].id).await.unwrap();
    assert_eq!(again.state, WorkState::Queued { pos: 0 });
    assert_eq!(live_claude(&fx).len(), 2);
    assert_eq!(fx.worktree_count(), 5, "main + one worktree per item, queued ones included");
    let q = &items[2];
    assert!(q.worktree.join("README.md").exists());
    let step = |name: &str| q.steps.iter().find(|s| s.step == name).unwrap().status;
    assert_eq!((step("include_files"), step("claude_files")), (StepStatus::Done, StepStatus::Pending));

    // Move to front: q/4 overtakes q/3.
    let front = w.queue_front(&items[3].id).await.unwrap();
    assert_eq!(front.state, WorkState::Queued { pos: -1 });
    assert!(w.queue_front(&items[0].id).await.is_err(), "only queued items move");

    // Exiting a Claude frees a slot: the front of the queue starts on that event.
    let first = live_claude(&fx).into_iter().find(|s| s.work_item_id.as_ref() == Some(&items[0].id)).unwrap();
    fx.core.exit_session(&first.id, 0);
    wait_active(&fx, &items[3].id).await;
    let all = fx.store.list_items(None).await.unwrap();
    assert_eq!(state(&all, &items[2].id), WorkState::Queued { pos: 0 }, "cap still full");
    assert_eq!(live_claude(&fx).len(), 2);

    // Start now (over cap).
    let now = w.start_now(&items[2].id).await.unwrap();
    assert_eq!(now.state, WorkState::Active, "{:?}", now.steps);
    assert_eq!(live_claude(&fx).len(), 3);
    assert!(w.start_now(&items[2].id).await.is_err(), "no longer queued");
}

#[tokio::test]
async fn queue_survives_a_restart_in_order() {
    if !has_git() {
        return;
    }
    let fx = Fx::new();
    fx.settings(|s| s.claude.max_live = 1);
    let w = fx.service();
    let a = start(&w, "r/a").await;
    let b = start(&w, "r/b").await;
    let c = start(&w, "r/c").await;
    assert_eq!(
        (b.state.clone(), c.state.clone()),
        (WorkState::Queued { pos: 0 }, WorkState::Queued { pos: 1 })
    );
    drop(w);

    // Restart: Claude is gone (the fake keeps it live; exit it without a listener), one slot frees.
    let sid = live_claude(&fx).into_iter().find(|s| s.work_item_id.as_ref() == Some(&a.id)).unwrap().id;
    fx.core.exit_session(&sid, 0);
    let w2 = fx.service();
    w2.startup().await.unwrap();
    wait_active(&fx, &b.id).await;
    let all = fx.store.list_items(None).await.unwrap();
    assert_eq!(state(&all, &c.id), WorkState::Queued { pos: 1 }, "still queued, not marked interrupted");
}

#[tokio::test]
async fn hold_threshold_keeps_items_queued() {
    if !has_git() {
        return;
    }
    let fx = Fx::new();
    fx.settings(|s| {
        s.claude.max_live = 4;
        s.claude.queue_hold_pct = Some(80.0);
    });
    let w = fx.service();
    let a = start(&w, "h/a").await;
    assert_eq!(a.state, WorkState::Active);
    let sid = live_claude(&fx)[0].id.clone();
    let mut s = fx.core.sessions().into_iter().find(|s| s.id == sid).unwrap();
    s.claude.as_mut().unwrap().usage = Some(kelta_proto::model::ClaudeUsage {
        five_hour: Some(kelta_proto::model::RateWindow { used_percentage: 91.0, resets_at: i64::MAX }),
        ..Default::default()
    });
    fx.core.insert_session(s.clone());
    let b = start(&w, "h/b").await;
    assert_eq!(b.state, WorkState::Queued { pos: 0 }, "held above 80% of the 5h window");

    // The window resets: its stale 91% no longer holds, and the next exit drains the queue.
    s.claude.as_mut().unwrap().usage.as_mut().unwrap().five_hour.as_mut().unwrap().resets_at = 0;
    fx.core.insert_session(s);
    fx.core.exit_session(&sid, 0);
    wait_active(&fx, &b.id).await;
}

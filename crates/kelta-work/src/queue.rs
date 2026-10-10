//! Claude slots and the run queue (#141): start work past `claude.max_live` live Claude processes
//! stops after the worktree steps with the item `Queued`; a freed slot (session exit, saga end, app
//! start) runs the rest of the saga for the lowest `pos`. Event-armed only, no timer (§13).

use std::sync::Arc;

use kelta_proto::api::CoreApi;
use kelta_proto::error::KeltaError;
use kelta_proto::ids::WorkItemId;
use kelta_proto::model::{Lifecycle, SessionKind, WorkItem, WorkState};

use crate::WorkService;

/// Dequeue policy: one more Claude may start while a slot is free (`max_live` 0 = no cap) and the
/// 5h usage is under the hold threshold (if any).
pub(crate) fn may_start(
    live: usize,
    max_live: u32,
    five_hour_pct: Option<f64>,
    hold_pct: Option<f64>,
) -> bool {
    let slot = max_live == 0 || live < max_live as usize;
    let held = matches!((five_hour_pct, hold_pct), (Some(u), Some(h)) if u >= h);
    slot && !held
}

/// Queued items in start order (lowest `pos` first, then the oldest).
pub(crate) fn queue_order(items: &[WorkItem]) -> Vec<WorkItemId> {
    let mut q: Vec<(i32, &str, &WorkItemId)> = items
        .iter()
        .filter_map(|w| match w.state {
            WorkState::Queued { pos } => Some((pos, w.created_at.as_str(), &w.id)),
            _ => None,
        })
        .collect();
    q.sort();
    q.into_iter().map(|(_, _, id)| id.clone()).collect()
}

impl WorkService {
    /// Take a Claude slot for `id` (true when it holds one already). A taken slot counts until
    /// the item's saga ends, so items between the gate and their Claude spawn are not overbooked.
    pub(crate) fn admit(&self, core: &Arc<dyn CoreApi>, id: &WorkItemId) -> bool {
        let settings = core.settings(None);
        let sessions = core.session_list(None);
        let mut starting = self.starting.lock();
        if starting.contains(id) {
            return true;
        }
        let live = sessions
            .iter()
            .filter(|s| s.kind == SessionKind::Claude && s.lifecycle == Lifecycle::Live)
            .filter(|s| s.work_item_id.as_ref().is_none_or(|w| !starting.contains(w)))
            .count()
            + starting.len();
        let five_hour = sessions
            .iter()
            .filter_map(|s| s.claude.as_ref()?.usage.as_ref()?.five_hour.as_ref().map(|w| w.used_percentage))
            .reduce(f64::max);
        let ok = may_start(live, settings.claude.max_live, five_hour, settings.claude.queue_hold_pct);
        if ok {
            starting.insert(id.clone());
        }
        ok
    }

    /// The saga of `id` ended (or never ran): free its slot and let the next queued item have it.
    pub(crate) fn release_slot(&self, id: &WorkItemId) {
        if self.starting.lock().remove(id) {
            self.kick_queue();
        }
    }

    pub(crate) async fn next_queue_pos(&self) -> Result<i32, KeltaError> {
        let items = self.store.list_items(None).await?;
        let max = items.iter().filter_map(|w| match w.state {
            WorkState::Queued { pos } => Some(pos),
            _ => None,
        });
        Ok(max.max().map_or(0, |p| p + 1))
    }

    /// Re-check the queue off the caller's task (bus listener, saga end, app start).
    pub(crate) fn kick_queue(&self) {
        let (Some(me), Ok(rt)) = (self.me.upgrade(), tokio::runtime::Handle::try_current()) else { return };
        rt.spawn(async move {
            if let Err(e) = me.dequeue().await {
                tracing::warn!(error = %e.message, "work queue not drained");
            }
        });
    }

    /// Start queued items, oldest first, while slots are free.
    pub(crate) async fn dequeue(&self) -> Result<(), KeltaError> {
        let core = self.api()?;
        for id in queue_order(&self.store.list_items(None).await?) {
            if self.starting.lock().contains(&id) {
                continue;
            }
            if !self.admit(&core, &id) {
                break;
            }
            let Some(me) = self.me.upgrade() else { return Ok(()) };
            tokio::spawn(async move {
                if let Err(e) = me.run_queued(&id, false).await {
                    tracing::warn!(work_item = %id, error = %e.message, "queued work item did not start");
                }
            });
        }
        Ok(())
    }

    /// Run the rest of the saga of a queued item that holds a slot (`force`: take one over the cap).
    async fn run_queued(&self, id: &WorkItemId, force: bool) -> Result<WorkItem, KeltaError> {
        let lock = self.item_lock(id);
        let _guard = if force { lock.try_lock().map_err(|_| self.busy(id))? } else { lock.lock().await };
        let item = self.load(id).await;
        if !item.as_ref().is_ok_and(|w| matches!(w.state, WorkState::Queued { .. })) {
            self.release_slot(id);
            return match item {
                Ok(_) if force => Err(KeltaError::conflict("work item is not queued")),
                other => other,
            };
        }
        if force {
            self.starting.lock().insert(id.clone());
        }
        self.run_saga_locked(id).await
    }

    /// `work_start_now`: start a queued item now, over the cap.
    pub async fn start_now(&self, id: &WorkItemId) -> Result<WorkItem, KeltaError> {
        self.ensure_listener();
        self.run_queued(id, true).await
    }

    /// `work_queue_front`: the queued item starts next.
    pub async fn queue_front(&self, id: &WorkItemId) -> Result<WorkItem, KeltaError> {
        let items = self.store.list_items(None).await?;
        let first = items.iter().filter_map(|w| match w.state {
            WorkState::Queued { pos } => Some(pos),
            _ => None,
        });
        let front = first.min().map_or(0, |p| p - 1);
        let mut queued = false;
        let item = self
            .update(id, |w| {
                queued = matches!(w.state, WorkState::Queued { .. });
                if queued {
                    w.state = WorkState::Queued { pos: front };
                }
                queued
            })
            .await?;
        if !queued {
            return Err(KeltaError::conflict("work item is not queued"));
        }
        Ok(item)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn policy_caps_and_holds() {
        assert!(may_start(1, 2, None, None));
        assert!(!may_start(2, 2, None, None), "cap reached");
        assert!(may_start(9, 0, None, None), "0 = no cap");
        assert!(may_start(0, 2, Some(79.0), Some(80.0)));
        assert!(!may_start(0, 2, Some(80.0), Some(80.0)), "held at the threshold");
        assert!(may_start(0, 2, Some(99.0), None), "no hold configured");
        assert!(may_start(0, 2, None, Some(80.0)), "no usage known");
    }

    #[test]
    fn order_is_pos_then_age() {
        let item = |id: &str, state: WorkState, at: &str| {
            let mut w = kelta_proto::samples::work_item();
            w.id = WorkItemId::new(id);
            w.state = state;
            w.created_at = at.into();
            w
        };
        let items = [
            item("c", WorkState::Queued { pos: 2 }, "2026-01-01T00:00:00Z"),
            item("active", WorkState::Active, "2025-01-01T00:00:00Z"),
            item("b", WorkState::Queued { pos: 1 }, "2026-01-02T00:00:00Z"),
            item("a", WorkState::Queued { pos: 1 }, "2026-01-01T00:00:00Z"),
            item("front", WorkState::Queued { pos: -1 }, "2026-01-03T00:00:00Z"),
        ];
        let ids: Vec<String> = queue_order(&items).iter().map(|i| i.to_string()).collect();
        assert_eq!(ids, ["front", "a", "b", "c"]);
    }
}

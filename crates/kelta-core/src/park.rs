//! Park (#142): stop a work item's Claude (and its nvim, unless it has unsaved buffers) without
//! touching the worktree or the conversation. The sessions go back to Dormant, so the next attach
//! resumes them (`claude --resume <uuid>`, nvim `-S <session>`). Auto-park: one one-shot per Claude
//! session, armed when it turns done + seen and cancelled by any input or status change.

use std::time::Duration;

use kelta_proto::api::CoreApi;
use kelta_proto::error::KeltaError;
use kelta_proto::events::{Toast, UiEvent};
use kelta_proto::ids::{SessionId, WorkItemId};
use kelta_proto::model::{
    Attention, Lifecycle, SessionInfo, SessionKind, SessionStatus, WorkItem, WorkState,
};
use kelta_proto::term::KillSignal;

use crate::Core;
use crate::sessions::SessionTimers;

/// Auto-park candidate: a live work item Claude that is done, seen and asks for nothing.
pub(crate) fn parkable(i: &SessionInfo) -> bool {
    i.kind == SessionKind::Claude
        && i.work_item_id.is_some()
        && i.lifecycle == Lifecycle::Live
        && i.status == SessionStatus::Done
        && i.seen
        && i.attention == Attention::None
}

impl Core {
    /// `work_park`. `auto` (the auto-park timer) parks only a parkable Claude and never an nvim on
    /// screen. A working Claude is never parked.
    pub async fn work_park(&self, id: &WorkItemId, auto: bool) -> Result<WorkItem, KeltaError> {
        self.rt.capture();
        let item = self.work_get(id).await.ok_or_else(|| KeltaError::not_found(format!("work item {id}")))?;
        if matches!(item.state, WorkState::Planned | WorkState::Starting | WorkState::Finished) {
            return Err(KeltaError::conflict("only a started, unfinished work item can be parked"));
        }
        let live: Vec<SessionInfo> = item
            .session_ids
            .iter()
            .filter_map(|s| self.session_get(s))
            .filter(|s| s.lifecycle == Lifecycle::Live)
            .collect();
        let claude: Vec<&SessionInfo> = live.iter().filter(|s| s.kind == SessionKind::Claude).collect();
        if claude.iter().any(|s| s.status == SessionStatus::Working || (auto && !parkable(s))) {
            return Err(KeltaError::conflict("Claude is working"));
        }
        let mut park: Vec<SessionId> = claude.iter().map(|s| s.id.clone()).collect();
        let mut kept = 0;
        for ed in live.iter().filter(|s| matches!(s.kind, SessionKind::Editor { .. })) {
            if auto && self.is_visible(&ed.id) {
                continue;
            }
            match self.work.park_editor(&item, ed).await {
                Ok(0) => park.push(ed.id.clone()),
                Ok(n) => kept += n,
                Err(e) => tracing::info!(session = %ed.id, error = %e.message, "editor kept running on park"),
            }
        }
        if park.is_empty() {
            return Err(KeltaError::conflict("nothing to park"));
        }
        if let Err(e) = self.work.park_teardown(&item).await {
            self.emit(UiEvent::Toast { toast: Toast::warn(format!("Parked anyway: {}", e.message)) });
        }
        self.park_sessions(&park).await;
        self.work.set_parked(id, Some(kept)).await
    }

    /// Stop live sessions and keep them Dormant (SIGHUP → `QUIT_GRACE` → SIGKILL); `on_exited`
    /// sees `parking`, so the exit frees the Claude slot and the HTTP consumer like any exit.
    pub(crate) async fn park_sessions(&self, ids: &[SessionId]) {
        let ids: Vec<SessionId> = {
            let mut s = self.sessions.lock();
            ids.iter()
                .filter(|id| {
                    s.get_mut(*id).is_some_and(|e| {
                        let live = e.info.lifecycle == Lifecycle::Live && !e.restoring;
                        e.parking |= live;
                        live
                    })
                })
                .cloned()
                .collect()
        };
        for id in &ids {
            if let Err(e) = self.terminal.kill(id, KillSignal::Hup) {
                tracing::warn!(session = %id, error = %e.message, "park: kill failed");
                if let Some(e) = self.sessions.lock().get_mut(id) {
                    e.parking = false;
                }
            }
        }
        if !self.wait_exit(&ids).await {
            for id in &ids {
                if self.session_get(id).is_some_and(|s| s.lifecycle == Lifecycle::Live) {
                    let _ = self.terminal.kill(id, KillSignal::Kill);
                }
            }
            self.wait_exit(&ids).await;
        }
    }

    /// A status change: arm auto-park on the transition to parkable, cancel it on any other state.
    pub(crate) fn auto_park_status(&self, info: &SessionInfo, was: bool, timers: &SessionTimers) {
        if !parkable(info) {
            timers.park.cancel();
            return;
        }
        let mins = self.cfg.effective(Some(&info.project_id)).claude.auto_park_after_mins;
        if was || mins == 0 {
            return;
        }
        let weak = self.me.clone();
        let sid = info.id.clone();
        // one-shot: auto-park, armed on done + seen, cancelled by input (`write_session`).
        timers.park.arm(&self.rt, Duration::from_secs(u64::from(mins) * 60), async move {
            let Some(core) = weak.upgrade() else { return };
            let Some(item) = core.session_get(&sid).filter(parkable).and_then(|s| s.work_item_id) else {
                return;
            };
            // Off the timer's task: the park's own exit cancels this session's timers.
            let c = core.clone();
            core.rt.spawn(async move {
                if let Err(e) = c.work_park(&item, true).await {
                    tracing::info!(work_item = %item, error = %e.message, "auto-park skipped");
                }
            });
        });
    }
}

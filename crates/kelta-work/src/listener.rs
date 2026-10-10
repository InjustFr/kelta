//! Bus listener (one task, event-driven): `claude.file_edited` → editor reload/open
//! (`editor.follow_claude_edits`), `session.exited` → release the HTTP consumer of a Claude session,
//! `pr.merged` / `pr.closed` → Merged / PrClosed work items (FLOW §4.6).
//! (`editor.follow_claude_edits`), `claude.hook` → work item signals (FLOW §2.3),
//! `session.exited` → release the HTTP consumer of a Claude session.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Weak;

use kelta_proto::events::{BusEvent, bus};
use kelta_proto::hooks::HookPayload;
use kelta_proto::ids::SessionId;
use tokio::sync::broadcast;

use crate::WorkService;

pub(crate) async fn run(me: Weak<WorkService>, mut rx: broadcast::Receiver<BusEvent>) {
    // Last hook task per session: the next one waits for it, so a prompt never lands before an
    // earlier Stop's slow git read.
    let mut last_hook: HashMap<SessionId, tokio::task::JoinHandle<()>> = HashMap::new();
    loop {
        let ev = match rx.recv().await {
            Ok(ev) => ev,
            Err(broadcast::error::RecvError::Lagged(_)) => continue,
            Err(broadcast::error::RecvError::Closed) => return,
        };
        let Some(svc) = me.upgrade() else { return };
        match ev.name.as_str() {
            bus::CLAUDE_FILE_EDITED => {
                let (Some(sid), Some(path)) = (
                    ev.session_id.clone(),
                    ev.payload.get("path").and_then(|p| p.as_str()).map(PathBuf::from),
                ) else {
                    continue;
                };
                // Editor RPC is bounded (connect 2 s, call 5 s); keep the listener responsive.
                tokio::spawn(async move {
                    if let Err(e) = svc.follow_edit(&sid, &path).await {
                        tracing::debug!(error = %e.message, "follow_claude_edits failed");
                    }
                });
            }
            bus::PR_MERGED | bus::PR_CLOSED => {
                let Some(review) =
                    ev.payload.get("review").cloned().and_then(|r| serde_json::from_value(r).ok())
                else {
                    continue;
                };
                let merged = ev.name == bus::PR_MERGED;
                // Waits on the item lock and the tracker; keep the listener responsive.
                tokio::spawn(async move {
                    if let Err(e) = svc.pr_ended(&review, merged).await {
                        tracing::warn!(error = %e.message, "pr end not applied");
                    }
                });
            }
            bus::CLAUDE_HOOK => {
                let (Some(sid), Some(Ok(hook))) = (
                    ev.session_id.clone(),
                    ev.payload.get("payload").cloned().map(serde_json::from_value::<HookPayload>),
                ) else {
                    continue;
                };
                let stop = ev.payload.get("event").and_then(|e| e.as_str()) == Some("Stop");
                // Esc ends a turn without a Stop; Claude still reports itself idle.
                let idle = hook.notification_type.as_deref() == Some("idle_prompt");
                // Hooks read git status: off the listener task, in order per session.
                let prev = last_hook.remove(&sid);
                let key = sid.clone();
                let task = tokio::spawn(async move {
                    if let Some(prev) = prev {
                        let _ = prev.await;
                    }
                    if let Err(e) = svc.on_claude_hook(&sid, hook).await {
                        tracing::debug!(error = %e.message, "work item hook signal failed");
                    }
                    if stop && let Err(e) = svc.notes_on_stop(&sid).await {
                        tracing::warn!(error = %e.message, "held prompt / review notes on Stop failed");
                    }
                    if idle && let Err(e) = svc.flush_pending(&sid).await {
                        tracing::warn!(error = %e.message, "held prompt on idle failed");
                    }
                    // Claude may have finished (or aborted) a rebase it was asked to resolve.
                    if stop
                        && let Some(item) = svc.for_session(&sid).await
                        && item.rebase.is_some()
                        && let Err(e) = svc.refresh_rebase(&item.id).await
                    {
                        tracing::debug!(error = %e.message, "rebase re-read on Stop failed");
                    }
                });
                last_hook.insert(key, task);
            }
            bus::SESSION_EXITED => {
                if let Some(sid) = &ev.session_id {
                    last_hook.remove(sid);
                    svc.on_session_exited(sid);
                }
            }
            _ => {}
        }
    }
}

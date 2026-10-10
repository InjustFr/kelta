//! Bus listener (one task, event-driven): `claude.file_edited` → editor reload/open
//! (`editor.follow_claude_edits`), `session.exited` → release the HTTP consumer of a Claude session,
//! Claude hooks → the item's `claude_uuid` follows the conversation, `Stop` re-reads the rebase state.

use std::path::PathBuf;
use std::sync::Weak;

use kelta_proto::events::{BusEvent, bus};
use tokio::sync::broadcast;

use crate::WorkService;

pub(crate) async fn run(me: Weak<WorkService>, mut rx: broadcast::Receiver<BusEvent>) {
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
            bus::CLAUDE_HOOK => {
                let Some(sid) = ev.session_id.clone() else { continue };
                let stop = ev.payload.get("event").and_then(|e| e.as_str()) == Some("Stop");
                let uuid =
                    ev.payload.pointer("/payload/session_id").and_then(|u| u.as_str()).map(str::to_owned);
                tokio::spawn(async move {
                    let Some(item) = svc.for_session(&sid).await else { return };
                    // B3: `/clear` or an in-Claude `/resume` changes the conversation; Fix with
                    // Claude must resume the one the user was really in.
                    if let Some(u) = uuid.filter(|u| !u.is_empty() && item.claude_uuid.as_deref() != Some(u))
                        && let Err(e) = svc.set_claude_uuid(&item.id, u).await
                    {
                        tracing::debug!(error = %e.message, "claude_uuid update failed");
                    }
                    // Claude may have finished (or aborted) a rebase it was asked to resolve.
                    if stop
                        && item.rebase.is_some()
                        && let Err(e) = svc.refresh_rebase(&item.id).await
                    {
                        tracing::debug!(error = %e.message, "rebase re-read on Stop failed");
                    }
                });
            }
            bus::SESSION_EXITED => {
                if let Some(sid) = &ev.session_id {
                    svc.on_session_exited(sid);
                }
            }
            _ => {}
        }
    }
}

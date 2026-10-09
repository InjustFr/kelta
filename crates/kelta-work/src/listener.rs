//! Bus listener (one task, event-driven): `claude.file_edited` → editor reload/open
//! (`editor.follow_claude_edits`), `claude.hook` → work item signals (FLOW §2.3),
//! `session.exited` → release the HTTP consumer of a Claude session.

use std::path::PathBuf;
use std::sync::Weak;

use kelta_proto::events::{BusEvent, bus};
use kelta_proto::hooks::HookPayload;
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
                let (Some(sid), Some(Ok(hook))) = (
                    ev.session_id.clone(),
                    ev.payload.get("payload").cloned().map(serde_json::from_value::<HookPayload>),
                ) else {
                    continue;
                };
                // Stop reads git status: off the listener task.
                tokio::spawn(async move {
                    if let Err(e) = svc.on_claude_hook(&sid, hook).await {
                        tracing::debug!(error = %e.message, "work item hook signal failed");
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

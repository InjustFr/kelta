//! EventBus (ARCHITECTURE §6.3): a `tokio::sync::broadcast` (capacity 1024). The trigger engine
//! has its own subscription (`PluginHost::start`, called by core at startup). Two events are also
//! handled by core, synchronously on publish: window focus changes (scheduler intervals) and
//! `work.updated` (relayed to the UI as `UiEvent::WorkUpdated`).

use kelta_proto::events::{BusEvent, UiEvent, bus};
use kelta_proto::model::WorkItem;

use crate::Core;

pub fn publish(core: &Core, ev: BusEvent) {
    match ev.name.as_str() {
        bus::APP_FOCUS_CHANGED => core.on_window_changed(),
        bus::WORK_UPDATED => match ev.payload.get("work").cloned().map(serde_json::from_value::<WorkItem>) {
            Some(Ok(work)) => {
                core.note_work_pr(&work);
                core.emit(UiEvent::WorkUpdated { work: Box::new(work) });
            }
            _ => tracing::warn!("work.updated without a work item"),
        },
        _ => {}
    }
    // No receiver = nobody listening; not an error.
    let _ = core.bus.send(ev);
}

impl Core {
    pub(crate) fn publish_ev(&self, ev: BusEvent) {
        publish(self, ev);
    }
}

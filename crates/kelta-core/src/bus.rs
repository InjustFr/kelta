//! EventBus (ARCHITECTURE §6.3): a `tokio::sync::broadcast` (capacity 1024). One event-driven
//! forwarder task feeds the trigger engine (`PluginHost::on_event`) and reacts to window focus
//! changes; lagging is reported with a `lagged` marker.

use std::sync::Arc;

use kelta_proto::events::{BusEvent, bus};
use tokio::sync::broadcast::error::RecvError;

use crate::Core;

pub fn publish(core: &Core, ev: BusEvent) {
    // No receiver = nobody listening; not an error.
    let _ = core.bus.send(ev);
}

impl Core {
    pub(crate) fn publish_ev(&self, ev: BusEvent) {
        publish(self, ev);
    }

    async fn on_bus_event(&self, ev: &BusEvent) {
        if ev.name == bus::APP_FOCUS_CHANGED {
            self.on_window_changed();
        }
        self.plugins.on_event(ev).await;
    }
}

/// Start the forwarder (once, from the async startup).
pub fn spawn_forwarder(core: &Arc<Core>) {
    let mut rx = core.bus.subscribe();
    let weak = core.me.clone();
    core.rt.spawn(async move {
        loop {
            match rx.recv().await {
                Ok(ev) => {
                    let Some(core) = weak.upgrade() else { break };
                    core.on_bus_event(&ev).await;
                }
                Err(RecvError::Lagged(n)) => {
                    let Some(core) = weak.upgrade() else { break };
                    tracing::warn!(missed = n, "bus forwarder lagged");
                    let marker = BusEvent::new(bus::LAGGED, serde_json::json!({ "missed": n }));
                    core.plugins.on_event(&marker).await;
                }
                Err(RecvError::Closed) => break,
            }
        }
    });
}

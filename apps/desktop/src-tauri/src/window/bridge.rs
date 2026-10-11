//! `UiBridge` implementation (owner: L10, ARCHITECTURE §4): `UiEvent` fan-out to every
//! `events_subscribe` channel, dock badge, attention request, notifications, window state, reload.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use kelta_proto::api::UiBridge;
use kelta_proto::ctl::CtlCommand;
use kelta_proto::error::KeltaError;
use kelta_proto::events::{Notification, UiEvent};
use kelta_proto::ipc::WindowState;
use parking_lot::Mutex;
use tauri::ipc::Channel;
use tauri::{AppHandle, Manager, UserAttentionType};
use tauri_plugin_notification::NotificationExt;

use super::MAIN_WINDOW;

/// Backend-initiated commands and toasts that arrive while the webview does not exist (startup,
/// background mode) are queued and delivered on the next subscription. Bounded so a stuck UI cannot grow memory.
const PENDING_CAP: usize = 32;

/// Subscriber list (id, window label, channel) plus the queue of commands that arrived with no
/// main-window subscriber. Generic over the channel/event types so the ordering logic is testable
/// without a Tauri app.
struct Hub<C, E> {
    subs: Mutex<Vec<(u64, String, C)>>,
    pending: Mutex<Vec<E>>,
}

impl<C: Clone, E: Clone> Hub<C, E> {
    fn new() -> Self {
        Self { subs: Mutex::new(Vec::new()), pending: Mutex::new(Vec::new()) }
    }

    fn subscribe(&self, id: u64, label: &str, ch: C, send: impl Fn(&C, E) -> bool) {
        // Hold subs across drain and push so a concurrent deliver cannot queue in between.
        let mut subs = self.subs.lock();
        if label == MAIN_WINDOW {
            for ev in std::mem::take(&mut *self.pending.lock()) {
                let _ = send(&ch, ev);
            }
        }
        subs.push((id, label.to_owned(), ch));
    }

    /// Sends to the subscribers of the windows `to` accepts.
    fn fan_out(&self, ev: &E, to: impl Fn(&str) -> bool, send: impl Fn(&C, E) -> bool) -> usize {
        let snapshot: Vec<(u64, String, C)> = self.subs.lock().clone();
        let mut dead = Vec::new();
        let mut delivered = 0;
        for (id, _, ch) in snapshot.iter().filter(|(_, label, _)| to(label)) {
            if send(ch, ev.clone()) {
                delivered += 1;
            } else {
                dead.push(*id);
            }
        }
        if !dead.is_empty() {
            self.subs.lock().retain(|(id, _, _)| !dead.contains(id));
        }
        delivered
    }

    /// Send to the main window's subscribers, or queue when there is none (or none accepted it):
    /// commands and toasts are the main window's, never a detached one's (#342).
    fn deliver(&self, ev: E, send: impl Fn(&C, E) -> bool) {
        // Empty-check and queue under the subs lock (shared with subscribe).
        let subs = self.subs.lock();
        if !subs.iter().any(|(_, label, _)| label == MAIN_WINDOW) {
            queue_capped(&mut self.pending.lock(), ev);
            return;
        }
        drop(subs);
        if self.fan_out(&ev, |l| l == MAIN_WINDOW, send) == 0 {
            queue_capped(&mut self.pending.lock(), ev);
        }
    }

    fn clear(&self, label: &str) {
        self.subs.lock().retain(|(_, l, _)| l != label);
    }
}

fn all(_: &str) -> bool {
    true
}

fn send_ev(ch: &Channel<UiEvent>, ev: UiEvent) -> bool {
    ch.send(ev).is_ok()
}

pub struct TauriBridge {
    handle: AppHandle,
    next_sub: AtomicU64,
    hub: Hub<Channel<UiEvent>, UiEvent>,
}

impl TauriBridge {
    pub fn new(handle: AppHandle) -> Arc<Self> {
        Arc::new(Self { handle, next_sub: AtomicU64::new(1), hub: Hub::new() })
    }

    pub fn handle(&self) -> &AppHandle {
        &self.handle
    }

    /// Register a UI event channel (one per window). Queued commands are delivered first.
    pub fn subscribe(&self, label: &str, channel: Channel<UiEvent>) -> u64 {
        let id = self.next_sub.fetch_add(1, Ordering::Relaxed);
        self.hub.subscribe(id, label, channel, send_ev);
        id
    }

    /// Drops the window's channels (the webview that owned them is gone or reloading).
    pub fn clear_subscribers(&self, label: &str) {
        self.hub.clear(label);
    }

    /// An event for every window but `label` (which already knows: it made the change).
    pub fn emit_except(&self, label: &str, ev: UiEvent) {
        let _ = self.hub.fan_out(&ev, |l| l != label, send_ev);
    }

    /// Commands that need a visible UI: bring the window back (recreating it in background mode).
    fn deliver_command(&self, ev: UiEvent) {
        super::raise(&self.handle);
        self.hub.deliver(ev, send_ev);
    }
}

fn queue_capped<E>(p: &mut Vec<E>, ev: E) {
    if p.len() >= PENDING_CAP {
        p.remove(0);
    }
    p.push(ev);
}

/// Which events must wake a window that does not exist.
pub fn needs_window(ev: &UiEvent) -> bool {
    matches!(ev, UiEvent::CtlCommand { .. } | UiEvent::UiOpen { .. } | UiEvent::QuitRequested { .. })
}

impl UiBridge for TauriBridge {
    fn emit(&self, ev: UiEvent) {
        match &ev {
            UiEvent::CtlCommand { cmd: CtlCommand::Emit { name, .. } }
                if name == "custom.bench.close_window" && super::bench::enabled() =>
            {
                if let Some(w) = self.handle.get_webview_window(MAIN_WINDOW) {
                    let _ = w.close();
                }
            }
            // The UI runs the in-app scenarios (ui/src/lib/terminal/bench.ts).
            UiEvent::CtlCommand { cmd: CtlCommand::Emit { name, .. } }
                if name == "custom.bench.run" && super::bench::enabled() =>
            {
                let _ = self.hub.fan_out(&ev, |l| l == MAIN_WINDOW, send_ev);
            }
            // Other bench events are not for the UI (and must not recreate the window).
            UiEvent::CtlCommand { cmd: CtlCommand::Emit { .. } } => {}
            // Toggle is native: it must work when the webview is destroyed or hung.
            UiEvent::CtlCommand { cmd: CtlCommand::Toggle } => super::toggle(&self.handle),
            e if needs_window(e) => self.deliver_command(ev),
            // A boot toast (tools moved, MCP offer) predates the webview's subscription: keep it.
            UiEvent::Toast { .. } => self.hub.deliver(ev, send_ev),
            _ => {
                let _ = self.hub.fan_out(&ev, all, send_ev);
            }
        }
    }

    fn set_badge(&self, needs_input: u32) {
        #[cfg(target_os = "macos")]
        {
            let label = (needs_input > 0).then(|| needs_input.to_string());
            let _ =
                self.handle.run_on_main_thread(move || crate::platform::dock::set_badge(label.as_deref()));
        }
        #[cfg(not(target_os = "macos"))]
        {
            let _ = needs_input;
        }
    }

    fn request_attention(&self) {
        if let Some(w) = self.handle.get_webview_window(MAIN_WINDOW)
            && !w.is_focused().unwrap_or(false)
        {
            let _ = w.request_user_attention(Some(UserAttentionType::Informational));
        }
    }

    fn notify(&self, n: Notification) -> Result<(), KeltaError> {
        let mut b = self.handle.notification().builder().title(n.title);
        if let Some(body) = n.body {
            b = b.body(body);
        }
        b.show().map_err(|e| KeltaError::unsupported(format!("notification failed: {e}")))
    }

    fn window_state(&self) -> WindowState {
        match self.handle.get_webview_window(MAIN_WINDOW) {
            Some(w) => WindowState {
                exists: true,
                visible: w.is_visible().unwrap_or(false),
                focused: w.is_focused().unwrap_or(false),
            },
            None => WindowState::default(),
        }
    }

    fn reload_webview(&self, safe: bool) {
        let Some(w) = self.handle.get_webview_window(MAIN_WINDOW) else { return };
        let target = super::reload_url(w.url().ok(), safe);
        match target {
            Some(url) => {
                let _ = w.navigate(url);
            }
            None => {
                let _ = w.reload();
            }
        }
    }

    fn webview_pids(&self) -> Vec<u32> {
        match self.handle.get_webview_window(MAIN_WINDOW) {
            Some(w) => crate::platform::webview::helper_pids(&w),
            None => Vec::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kelta_proto::ids::ProjectId;

    #[test]
    fn pending_queue_evicts_oldest() {
        let mut p = Vec::new();
        for i in 0..PENDING_CAP + 3 {
            queue_capped(&mut p, UiEvent::ProjectRemoved { id: ProjectId::new(i.to_string()) });
        }
        assert_eq!(p.len(), PENDING_CAP);
        assert!(matches!(&p[0], UiEvent::ProjectRemoved { id } if id.as_str() == "3"));
    }

    #[test]
    fn queued_until_subscribe_then_drained_in_order() {
        let hub: Hub<u8, u32> = Hub::new();
        let sent = std::cell::RefCell::new(Vec::new());
        let send = |c: &u8, e: u32| {
            sent.borrow_mut().push((*c, e));
            true
        };
        hub.deliver(1, send);
        hub.deliver(2, send);
        assert!(sent.borrow().is_empty());
        hub.subscribe(1, "detached-t", 8, send);
        assert!(sent.borrow().is_empty(), "commands wait for the main window");
        hub.subscribe(2, MAIN_WINDOW, 7, send);
        assert_eq!(*sent.borrow(), [(7, 1), (7, 2)]);
        hub.deliver(3, send);
        assert_eq!(sent.borrow().last(), Some(&(7, 3)));
        assert_eq!(hub.fan_out(&9, all, send), 2);
        // a reload drops its own window's channels only
        hub.clear("detached-t");
        assert_eq!(hub.subs.lock().len(), 1);
        // a dead channel is pruned and the event queued for the next subscriber
        hub.deliver(4, |_, _| false);
        assert!(hub.subs.lock().is_empty());
        assert_eq!(*hub.pending.lock(), [4]);
    }

    #[test]
    fn only_window_commands_wake_the_webview() {
        assert!(needs_window(&UiEvent::CtlCommand { cmd: CtlCommand::Palette }));
        assert!(needs_window(&UiEvent::QuitRequested { live: 1, busy: vec![] }));
        assert!(!needs_window(&UiEvent::ProjectRemoved { id: ProjectId::new("p") }));
    }
}

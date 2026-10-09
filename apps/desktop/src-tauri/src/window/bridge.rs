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

/// Backend-initiated commands that arrive while the webview does not exist (background mode) are
/// queued and delivered on the next subscription. Bounded so a stuck UI cannot grow memory.
const PENDING_CAP: usize = 32;

pub struct TauriBridge {
    handle: AppHandle,
    next_sub: AtomicU64,
    subs: Mutex<Vec<(u64, Channel<UiEvent>)>>,
    pending: Mutex<Vec<UiEvent>>,
}

impl TauriBridge {
    pub fn new(handle: AppHandle) -> Arc<Self> {
        Arc::new(Self {
            handle,
            next_sub: AtomicU64::new(1),
            subs: Mutex::new(Vec::new()),
            pending: Mutex::new(Vec::new()),
        })
    }

    pub fn handle(&self) -> &AppHandle {
        &self.handle
    }

    /// Register a UI event channel (one per window). Queued commands are delivered first.
    pub fn subscribe(&self, channel: Channel<UiEvent>) -> u64 {
        let id = self.next_sub.fetch_add(1, Ordering::Relaxed);
        let queued = std::mem::take(&mut *self.pending.lock());
        for ev in queued {
            let _ = channel.send(ev);
        }
        self.subs.lock().push((id, channel));
        id
    }

    /// Drops every channel (the webview that owned them is gone).
    pub fn clear_subscribers(&self) {
        self.subs.lock().clear();
    }

    pub fn subscriber_count(&self) -> usize {
        self.subs.lock().len()
    }

    fn fan_out(&self, ev: &UiEvent) -> usize {
        let snapshot: Vec<(u64, Channel<UiEvent>)> = self.subs.lock().clone();
        if snapshot.is_empty() {
            return 0;
        }
        let mut dead = Vec::new();
        let mut delivered = 0;
        for (id, ch) in &snapshot {
            match ch.send(ev.clone()) {
                Ok(()) => delivered += 1,
                Err(_) => dead.push(*id),
            }
        }
        if !dead.is_empty() {
            self.subs.lock().retain(|(id, _)| !dead.contains(id));
        }
        delivered
    }

    fn queue(&self, ev: UiEvent) {
        let mut p = self.pending.lock();
        if p.len() >= PENDING_CAP {
            p.remove(0);
        }
        p.push(ev);
    }

    /// Commands that need a visible UI: bring the window back (recreating it in background mode).
    fn deliver_command(&self, ev: UiEvent) {
        super::raise(&self.handle);
        if self.fan_out(&ev) == 0 {
            self.queue(ev);
        }
    }
}

/// Which events must wake a window that does not exist.
pub fn needs_window(ev: &UiEvent) -> bool {
    matches!(ev, UiEvent::CtlCommand { .. } | UiEvent::UiOpen { .. })
}

impl UiBridge for TauriBridge {
    fn emit(&self, ev: UiEvent) {
        match &ev {
            // Toggle is native: it must work when the webview is destroyed or hung.
            UiEvent::CtlCommand { cmd: CtlCommand::Toggle } => super::toggle(&self.handle),
            e if needs_window(e) => self.deliver_command(ev),
            _ => {
                let _ = self.fan_out(&ev);
            }
        }
    }

    fn set_badge(&self, needs_input: u32) {
        #[cfg(target_os = "macos")]
        {
            let label = (needs_input > 0).then(|| needs_input.to_string());
            let _ = self.handle.run_on_main_thread(move || crate::platform::dock::set_badge(label.as_deref()));
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
    fn only_window_commands_wake_the_webview() {
        assert!(needs_window(&UiEvent::CtlCommand { cmd: CtlCommand::Palette }));
        assert!(!needs_window(&UiEvent::ProjectRemoved { id: ProjectId::new("p") }));
    }
}

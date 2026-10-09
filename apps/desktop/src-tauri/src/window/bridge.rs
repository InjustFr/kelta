//! `UiBridge` implementation (owner: L10, ARCHITECTURE §4): `UiEvent` fan-out to every
//! `events_subscribe` channel, dock badge, attention request, notifications, window state, reload.
//!
//! SCAFFOLD STUB: subscriptions get ids but events are dropped; other methods are no-ops.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use kelta_proto::api::UiBridge;
use kelta_proto::error::KeltaError;
use kelta_proto::events::{Notification, UiEvent};
use kelta_proto::ipc::WindowState;
use tauri::AppHandle;
use tauri::ipc::Channel;

pub struct TauriBridge {
    handle: AppHandle,
    next_sub: AtomicU64,
}

impl TauriBridge {
    pub fn new(handle: AppHandle) -> Arc<Self> {
        Arc::new(Self { handle, next_sub: AtomicU64::new(1) })
    }

    pub fn handle(&self) -> &AppHandle {
        &self.handle
    }

    /// Register a UI event channel (one per window). Stub: the channel is dropped.
    pub fn subscribe(&self, channel: Channel<UiEvent>) -> u64 {
        drop(channel);
        self.next_sub.fetch_add(1, Ordering::Relaxed)
    }
}

impl UiBridge for TauriBridge {
    fn emit(&self, _ev: UiEvent) {}

    fn set_badge(&self, _needs_input: u32) {}

    fn request_attention(&self) {}

    fn notify(&self, _n: Notification) -> Result<(), KeltaError> {
        Err(KeltaError::not_implemented("TauriBridge::notify"))
    }

    fn window_state(&self) -> WindowState {
        WindowState::default()
    }

    fn reload_webview(&self, _safe: bool) {}
}

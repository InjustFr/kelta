//! Window management (owner: L10, ARCHITECTURE §2, §12.4, §14): window creation (decorations,
//! app_id, geometry restore), macOS app menu, close → background / quit, reopen, second instance,
//! webview crash/hang reload.
//!
//! SCAFFOLD STUB: creates a plain "main" window; event hooks are no-ops.

pub mod bridge;

use std::sync::Arc;

use kelta_core::Core;
use tauri::{AppHandle, Manager, RunEvent, WebviewUrl, WebviewWindowBuilder, Window, WindowEvent};

/// Main window label (capabilities are granted to it only).
pub const MAIN_WINDOW: &str = "main";

/// Called from the Tauri `setup` hook after `Core::start`.
pub fn setup(app: &mut tauri::App, core: Arc<Core>) -> Result<(), Box<dyn std::error::Error>> {
    let _ = core;
    if app.get_webview_window(MAIN_WINDOW).is_none() {
        WebviewWindowBuilder::new(app, MAIN_WINDOW, WebviewUrl::App("index.html".into()))
            .title("Kelta")
            .inner_size(1280.0, 800.0)
            .min_inner_size(640.0, 400.0)
            .build()?;
    }
    Ok(())
}

/// Window events (close → background/quit per `window.close_behavior`, focus → bus).
pub fn on_window_event(window: &Window, event: &WindowEvent) {
    let _ = (window, event);
}

/// App run-loop events (macOS Dock reopen, exit requested → `Core::shutdown`).
pub fn on_run_event(app: &AppHandle, event: &RunEvent) {
    let _ = (app, event);
}

/// A second `kelta [args]` launch forwarded by tauri-plugin-single-instance.
pub fn on_second_instance(app: &AppHandle, argv: Vec<String>, cwd: String) {
    let _ = (argv, cwd);
    if let Some(w) = app.get_webview_window(MAIN_WINDOW) {
        let _ = w.set_focus();
    }
}

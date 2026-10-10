//! Window management (owner: L10, ARCHITECTURE §2, §12.4, §14): window creation (decorations,
//! app_id, geometry restore), macOS app menu, close → background / quit, reopen, second instance,
//! webview crash/hang reload.

pub mod bench;
pub mod bridge;

use std::path::{Path, PathBuf};
use std::sync::Arc;

use kelta_core::Core;
use kelta_proto::api::CoreApi;
use kelta_proto::api::{SettingsSource, UiBridge};
use kelta_proto::ctl::CtlCommand;
use kelta_proto::dirs::CliArgs;
use kelta_proto::events::{BusEvent, UiEvent, bus};
use kelta_proto::settings::Decorations;
use tauri::webview::PageLoadEvent;
use tauri::{
    AppHandle, Manager, PhysicalPosition, PhysicalSize, RunEvent, Url, WebviewUrl, WebviewWindow,
    WebviewWindowBuilder, Window, WindowEvent,
};

use crate::platform::decorations;
use crate::platform::geometry::{self, Geometry};
use bridge::TauriBridge;

/// Main window label (capabilities are granted to it only).
pub const MAIN_WINDOW: &str = "main";

/// Decorations the main window was last built with (reported by `app_info`).
static EFFECTIVE_DECORATIONS: parking_lot::Mutex<Option<decorations::Effective>> =
    parking_lot::Mutex::new(None);

/// `app_info.decorations`: what the window was built with (`None` before the first window).
pub fn effective_decorations() -> Option<Decorations> {
    EFFECTIVE_DECORATIONS.lock().map(|e| match e {
        decorations::Effective::Native => Decorations::Native,
        decorations::Effective::None => Decorations::None,
        decorations::Effective::Custom => Decorations::Custom,
    })
}

fn core(app: &AppHandle) -> Option<Arc<Core>> {
    app.try_state::<Arc<Core>>().map(|s| s.inner().clone())
}

fn env(k: &str) -> Option<String> {
    std::env::var(k).ok()
}

fn background_mode(app: &AppHandle) -> bool {
    core(app).is_some_and(|c| {
        decorations::closes_to_background(
            c.config().effective(None).window.close_behavior,
            cfg!(target_os = "macos"),
        )
    })
}

/// Called from the Tauri `setup` hook after `Core::start`.
pub fn setup(app: &mut tauri::App, core: Arc<Core>) -> Result<(), Box<dyn std::error::Error>> {
    let _ = core;
    bench::started();
    #[cfg(target_os = "macos")]
    {
        app.set_menu(app_menu(app.handle())?)?;
        // The predefined Quit item calls NSApp terminate:, which exits without RunEvent::Exit
        // (no Core::shutdown). Route our own item through app.exit instead.
        app.on_menu_event(|app, ev| {
            if ev.id() == "quit" {
                save_main_geometry(app); // app.exit does not send CloseRequested
                app.exit(0);
            }
        });
    }
    create(app.handle())?;
    Ok(())
}

/// Creates the main window (also used to bring it back after a background-mode close).
fn create(app: &AppHandle) -> Result<WebviewWindow, Box<dyn std::error::Error>> {
    let core = core(app).ok_or("core not started")?;
    let s = core.config().effective(None);
    let eff = decorations::decide(s.window.decorations, cfg!(target_os = "linux"), &env);

    let mut b = WebviewWindowBuilder::new(app, MAIN_WINDOW, WebviewUrl::App("index.html".into()))
        .title("Kelta")
        .inner_size(geometry::DEFAULT_WIDTH, geometry::DEFAULT_HEIGHT)
        .min_inner_size(geometry::MIN_WIDTH, geometry::MIN_HEIGHT)
        .decorations(!eff.undecorated())
        .on_page_load(|w, p| {
            // The UI re-subscribes after every load; channels of the previous page are dead.
            if matches!(p.event(), PageLoadEvent::Started)
                && let Some(b) = w.app_handle().try_state::<Arc<TauriBridge>>()
            {
                b.clear_subscribers();
            }
        });

    let saved = s.window.restore_geometry.then(|| geometry::load(&core.dirs().data)).flatten();
    let maximized = saved.is_some_and(|g| g.maximized);
    if let Some(g) = saved.filter(|g| g.visible_on(&monitors(app))) {
        b = b.inner_size(g.width, g.height).position(g.x, g.y);
    }
    let win = b.build()?;
    *EFFECTIVE_DECORATIONS.lock() = Some(eff);
    if maximized {
        let _ = win.maximize();
    }
    crate::platform::webview::configure(&win);
    tracing::info!(decorations = eff.as_str(), "main window created");
    Ok(win)
}

/// Monitor rectangles in logical pixels.
fn monitors(app: &AppHandle) -> Vec<(f64, f64, f64, f64)> {
    app.available_monitors()
        .unwrap_or_default()
        .iter()
        .map(|m| {
            let f = m.scale_factor();
            let (p, s) = (m.position(), m.size());
            (f64::from(p.x) / f, f64::from(p.y) / f, f64::from(s.width) / f, f64::from(s.height) / f)
        })
        .collect()
}

fn save_main_geometry(app: &AppHandle) {
    if let Some(c) = core(app)
        && c.config().effective(None).window.restore_geometry
        && let Some(w) = app.get_webview_window(MAIN_WINDOW)
    {
        save_geometry(&w.as_ref().window(), &c.dirs().data);
    }
}

fn save_geometry(win: &Window, data_dir: &Path) {
    let (Ok(pos), Ok(size), Ok(f)) = (win.outer_position(), win.inner_size(), win.scale_factor()) else {
        return;
    };
    let maximized = win.is_maximized().unwrap_or(false);
    let prev = geometry::load(data_dir);
    // A maximized window reports the maximized rect: keep the previous normal rect.
    let g = match (maximized, prev) {
        (true, Some(p)) => Geometry { maximized: true, ..p },
        _ => {
            let PhysicalPosition { x, y } = pos;
            let PhysicalSize { width, height } = size;
            Geometry {
                x: f64::from(x) / f,
                y: f64::from(y) / f,
                width: f64::from(width) / f,
                height: f64::from(height) / f,
                maximized,
            }
        }
    };
    geometry::save(data_dir, &g);
}

/// Shows and focuses the window, recreating it after a background-mode close.
pub(crate) fn raise(app: &AppHandle) {
    let Some(w) = app.get_webview_window(MAIN_WINDOW) else {
        if let Err(e) = create(app) {
            tracing::error!("cannot recreate window: {e}");
        }
        return;
    };
    let _ = w.show();
    let _ = w.unminimize();
    // shortcut: focus request only; an XDG activation token is consumed by GTK at process start,
    // so a hotkey-launched second instance cannot pass it to the running one. Upgrade when
    // CtlCommand::Toggle carries the token.
    let _ = w.set_focus();
}

/// `kelta-ctl toggle`: hide when focused, otherwise raise.
pub(crate) fn toggle(app: &AppHandle) {
    match app.get_webview_window(MAIN_WINDOW) {
        Some(w) if w.is_visible().unwrap_or(false) && w.is_focused().unwrap_or(false) => {
            let _ = w.hide();
        }
        _ => raise(app),
    }
}

/// Target of `reload_webview(safe)`: `?safe=1` for the safe reload, no query otherwise. `None`
/// means a plain `reload()` is enough.
pub(crate) fn reload_url(current: Option<Url>, safe: bool) -> Option<Url> {
    let mut url = current?;
    let has_safe = url.query_pairs().any(|(k, _)| k == "safe");
    if safe == has_safe {
        return None;
    }
    let keep: Vec<(String, String)> = url
        .query_pairs()
        .filter(|(k, _)| k != "safe")
        .map(|(k, v)| (k.into_owned(), v.into_owned()))
        .collect();
    url.set_query(None);
    {
        let mut q = url.query_pairs_mut();
        q.extend_pairs(keep.iter().map(|(k, v)| (k.as_str(), v.as_str())));
        if safe {
            q.append_pair("safe", "1");
        }
    }
    if url.query() == Some("") {
        url.set_query(None);
    }
    Some(url)
}

/// Window events (close → background/quit per `window.close_behavior`).
pub fn on_window_event(window: &Window, event: &WindowEvent) {
    if window.label() != MAIN_WINDOW {
        return;
    }
    let app = window.app_handle();
    match event {
        WindowEvent::CloseRequested { .. } => {
            save_main_geometry(app);
        }
        // Scheduler intervals and triggers follow focus (`app.focus_changed`).
        WindowEvent::Focused(focused) => {
            if let Some(c) = core(app) {
                c.publish(BusEvent::new(bus::APP_FOCUS_CHANGED, serde_json::json!({ "focused": focused })));
            }
        }
        // The webview is gone: its event channels are dead; commands queue until the next one.
        WindowEvent::Destroyed => {
            if bench::enabled() {
                bench::mark("window_closed", 1.0);
            }
            if let Some(b) = app.try_state::<Arc<TauriBridge>>() {
                b.clear_subscribers();
            }
            if let Some(c) = core(app) {
                c.detach_all_views();
            }
        }
        _ => {}
    }
}

/// App run-loop events (macOS Dock reopen, background mode keeps the app alive, exit → shutdown).
pub fn on_run_event(app: &AppHandle, event: &RunEvent) {
    match event {
        // No window left and no explicit quit: stay alive in background mode.
        RunEvent::ExitRequested { code: None, api, .. } if background_mode(app) => api.prevent_exit(),
        RunEvent::ExitRequested { .. } => save_main_geometry(app),
        RunEvent::Exit => {
            if let Some(c) = core(app)
                && let Err(e) = tauri::async_runtime::block_on(c.shutdown())
            {
                tracing::error!("shutdown: {e}");
            }
        }
        #[cfg(target_os = "macos")]
        RunEvent::Reopen { has_visible_windows: false, .. } => raise(app),
        _ => {}
    }
}

/// Paths a second `kelta [args]` launch asks to open, made absolute against its cwd.
fn second_instance_paths(argv: &[String], cwd: &str) -> Vec<PathBuf> {
    CliArgs::parse(argv.get(1..).unwrap_or_default())
        .open
        .into_iter()
        .map(|p| if p.is_absolute() { p } else { Path::new(cwd).join(p) })
        .collect()
}

/// A second `kelta [args]` launch forwarded by tauri-plugin-single-instance.
pub fn on_second_instance(app: &AppHandle, argv: Vec<String>, cwd: String) {
    // The second process wrote the crash guard in pre_init and exits without a page load: drop it.
    crate::platform::launch_succeeded();
    raise(app);
    if let Some(b) = app.try_state::<Arc<TauriBridge>>() {
        for path in second_instance_paths(&argv, &cwd) {
            b.emit(UiEvent::CtlCommand { cmd: CtlCommand::Open { path } });
        }
    }
}

/// macOS app menu replacing Tauri's default (Cmd+W/H/M/Q are intentional).
#[cfg(target_os = "macos")]
fn app_menu(app: &AppHandle) -> tauri::Result<tauri::menu::Menu<tauri::Wry>> {
    use tauri::menu::{Menu, MenuItem, PredefinedMenuItem as P, Submenu};
    let kelta = Submenu::with_items(
        app,
        "Kelta",
        true,
        &[
            &P::about(app, None, None)?,
            &P::separator(app)?,
            &P::hide(app, None)?,
            &P::hide_others(app, None)?,
            &P::show_all(app, None)?,
            &P::separator(app)?,
            &MenuItem::with_id(app, "quit", "Quit Kelta", true, Some("CmdOrCtrl+Q"))?,
        ],
    )?;
    let edit = Submenu::with_items(
        app,
        "Edit",
        true,
        &[
            &P::undo(app, None)?,
            &P::redo(app, None)?,
            &P::separator(app)?,
            &P::cut(app, None)?,
            &P::copy(app, None)?,
            &P::paste(app, None)?,
            &P::select_all(app, None)?,
        ],
    )?;
    let window = Submenu::with_items(
        app,
        "Window",
        true,
        &[&P::minimize(app, None)?, &P::maximize(app, None)?, &P::close_window(app, None)?],
    )?;
    Menu::with_items(app, &[&kelta, &edit, &window])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn u(s: &str) -> Option<Url> {
        Url::parse(s).ok()
    }

    #[test]
    fn reload_url_adds_and_drops_safe() {
        let plain = u("tauri://localhost/index.html");
        let safe = u("tauri://localhost/index.html?safe=1");
        assert_eq!(reload_url(plain.clone(), true), safe);
        assert_eq!(reload_url(safe.clone(), false), plain);
        assert_eq!(reload_url(plain, false), None);
        assert_eq!(reload_url(safe, true), None);
        assert_eq!(reload_url(u("http://x/?a=1"), true), u("http://x/?a=1&safe=1"));
        assert_eq!(reload_url(None, true), None);
    }

    #[test]
    fn second_instance_paths_are_absolute() {
        let argv = ["kelta", "--safe-graphics", "repo", "/abs"].map(String::from);
        assert_eq!(
            second_instance_paths(&argv, "/work"),
            [PathBuf::from("/work/repo"), PathBuf::from("/abs")]
        );
    }
}

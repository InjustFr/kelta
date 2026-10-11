//! Window management (owner: L10, ARCHITECTURE §2, §12.4, §14): window creation (decorations,
//! app_id, geometry restore), detached tab windows (#342), macOS app menu, close → background / quit,
//! reopen, second instance, webview crash/hang reload.

pub mod bench;
pub mod bridge;

use std::path::{Path, PathBuf};
use std::sync::Arc;

use kelta_core::Core;
use kelta_core::sessions::QuitPlan;
use kelta_proto::api::CoreApi;
use kelta_proto::api::SettingsSource;
use kelta_proto::api::UiBridge;
use kelta_proto::ctl::CtlCommand;
use kelta_proto::dirs::CliArgs;
use kelta_proto::events::{BusEvent, Toast, UiEvent, bus};
use kelta_proto::ids::{ProjectId, TabId};
use kelta_proto::settings::Decorations;
use tauri::webview::{PageLoadEvent, PageLoadPayload};
use tauri::{
    AppHandle, Manager, PhysicalPosition, PhysicalSize, RunEvent, Url, WebviewUrl, WebviewWindow,
    WebviewWindowBuilder, Window, WindowEvent,
};

use crate::platform::decorations;
use crate::platform::geometry::{self, Geometry};
use bridge::TauriBridge;

/// Main window label (capabilities are granted to it and the detached windows only).
pub const MAIN_WINDOW: &str = "main";

/// Label prefix of the windows that each show one detached tab (#342).
pub const DETACHED_PREFIX: &str = "detached-";

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

pub(crate) fn core(app: &AppHandle) -> Option<Arc<Core>> {
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
        app.on_menu_event(|app, ev| match ev.id().as_ref() {
            "quit" if !quit_asks(app) => quit(app),
            // Like the title-bar button of the focused window: CloseRequested → the UI's close handler.
            "close_window" => {
                let windows = app.webview_windows();
                let focused = windows.values().find(|w| w.is_focused().unwrap_or(false));
                if let Some(w) = focused.or_else(|| windows.get(MAIN_WINDOW)) {
                    let _ = w.close();
                }
            }
            _ => {}
        });
        // Dock Quit and logout send terminate:: ask like Cmd+Q (#311). Terminating now still
        // reaches RunEvent::Exit (tao's applicationWillTerminate), hence Core::shutdown.
        let handle = app.handle().clone();
        crate::platform::dock::on_should_terminate(move || {
            let now = !quit_asks(&handle);
            if now {
                save_geometries(&handle); // no ExitRequested on this path
            }
            now
        });
    }
    create(app.handle())?;
    restore_detached(app.handle());
    Ok(())
}

/// The UI re-subscribes after every load; channels of the window's previous page are dead.
fn on_page_load(w: WebviewWindow, p: PageLoadPayload<'_>) {
    if matches!(p.event(), PageLoadEvent::Started)
        && let Some(b) = w.app_handle().try_state::<Arc<TauriBridge>>()
    {
        b.clear_subscribers(w.label());
    }
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
        .on_page_load(on_page_load);

    // macOS custom chrome: the system traffic lights over the context bar's 72px inset, no title bar.
    #[cfg(target_os = "macos")]
    if eff == decorations::Effective::Custom {
        b = b.decorations(true).title_bar_style(tauri::TitleBarStyle::Overlay).hidden_title(true);
    }

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

/// Window label of a detached tab (labels take `a-zA-Z0-9-/:_`).
fn detached_label(tab: &TabId) -> String {
    let id: String =
        tab.as_str().chars().map(|c| if c.is_ascii_alphanumeric() || c == '-' { c } else { '_' }).collect();
    format!("{DETACHED_PREFIX}{id}")
}

/// Shows the window of a detached tab, creating it where it was last (same rules as the main
/// window's geometry). The page learns its tab from `window.__KELTA_DETACHED__`.
pub(crate) fn open_detached(
    app: &AppHandle,
    project: &ProjectId,
    tab: &TabId,
) -> Result<(), Box<dyn std::error::Error>> {
    let label = detached_label(tab);
    if let Some(w) = app.get_webview_window(&label) {
        let _ = w.unminimize();
        let _ = w.set_focus();
        return Ok(());
    }
    let core = core(app).ok_or("core not started")?;
    let title = core.layout_get(project).ok().and_then(|l| l.tabs.into_iter().find(|t| &t.id == tab));
    let shows = serde_json::json!({ "project_id": project, "tab_id": tab });
    let mut b = WebviewWindowBuilder::new(app, &label, WebviewUrl::App("index.html".into()))
        .title(title.map_or_else(|| "Kelta".to_owned(), |t| format!("{} — Kelta", t.title)))
        .inner_size(geometry::DEFAULT_WIDTH, geometry::DEFAULT_HEIGHT)
        .min_inner_size(geometry::MIN_WIDTH, geometry::MIN_HEIGHT)
        .initialization_script(format!("window.__KELTA_DETACHED__ = {shows};"))
        .on_page_load(on_page_load);
    let saved = core
        .config()
        .effective(None)
        .window
        .restore_geometry
        .then(|| geometry::load_detached(&core.dirs().data).remove(&label))
        .flatten();
    if let Some(g) = saved.filter(|g| g.visible_on(&monitors(app))) {
        b = b.inner_size(g.width, g.height).position(g.x, g.y);
    }
    let win = b.build()?;
    if saved.is_some_and(|g| g.maximized) {
        let _ = win.maximize();
    }
    crate::platform::webview::configure(&win);
    Ok(())
}

/// Reopens the windows of the tabs that were detached at the last quit.
fn restore_detached(app: &AppHandle) {
    let Some(c) = core(app) else { return };
    for p in c.project_list().into_iter().filter(|p| p.open) {
        let Ok(l) = c.layout_get(&p.id) else { continue };
        for t in l.tabs.iter().filter(|t| t.detached) {
            if let Err(e) = open_detached(app, &p.id, &t.id) {
                tracing::warn!(tab = %t.id, "cannot reopen a detached window: {e}");
            }
        }
    }
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

/// Quits through `Core::shutdown` (Cmd+Q, `app_quit`). The detached windows close with it and
/// their tabs stay detached: they come back on the next launch.
pub(crate) fn quit(app: &AppHandle) {
    save_geometries(app); // app.exit does not send CloseRequested
    app.exit(0);
}

/// A user quit with sessions keltad would keep (#305): true when the UI now asks keep / stop and
/// quits through `app_quit`; `app.quit_sessions = "stop"` marks the quit to stop them.
fn quit_asks(app: &AppHandle) -> bool {
    let Some(c) = core(app) else { return false };
    match c.quit_plan() {
        QuitPlan::Keep => false,
        QuitPlan::Stop => {
            c.stop_sessions_on_quit();
            false
        }
        // The bridge raises the window first (background mode) and queues the event until the page listens.
        QuitPlan::Ask { live, busy } => {
            let Some(b) = app.try_state::<Arc<TauriBridge>>() else { return false };
            b.emit(UiEvent::QuitRequested { live, busy });
            true
        }
    }
}

/// The UI's close handler (it prevents the default close, which Tauri would otherwise finish
/// right after the handler whatever Rust does in `CloseRequested`): the quit question, or close.
/// Out of background mode, closing the main window quits even with detached windows open.
pub(crate) fn close_requested(app: &AppHandle) {
    if background_mode(app) {
        if let Some(w) = app.get_webview_window(MAIN_WINDOW) {
            let _ = w.destroy();
        }
    } else if !quit_asks(app) {
        quit(app);
    }
}

fn save_main_geometry(app: &AppHandle) {
    if let Some(c) = core(app)
        && c.config().effective(None).window.restore_geometry
        && let Some(w) = app.get_webview_window(MAIN_WINDOW)
        && let Some(g) = geometry_of(&w.as_ref().window(), geometry::load(&c.dirs().data))
    {
        geometry::save(&c.dirs().data, &g);
    }
}

/// On quit: the main window's geometry and the open detached windows' (the closed ones are dropped).
fn save_geometries(app: &AppHandle) {
    save_main_geometry(app);
    let Some(c) = core(app) else { return };
    if !c.config().effective(None).window.restore_geometry {
        return;
    }
    let data = &c.dirs().data;
    let mut prev = geometry::load_detached(data);
    let map = app
        .webview_windows()
        .into_iter()
        .filter(|(label, _)| label.starts_with(DETACHED_PREFIX))
        .filter_map(|(label, w)| {
            let g = geometry_of(&w.as_ref().window(), prev.remove(&label))?;
            Some((label, g))
        })
        .collect();
    geometry::save_detached(data, &map);
}

/// `prev`: the last saved geometry, kept as the normal rect of a maximized window.
fn geometry_of(win: &Window, prev: Option<Geometry>) -> Option<Geometry> {
    let (Ok(pos), Ok(size), Ok(f)) = (win.outer_position(), win.inner_size(), win.scale_factor()) else {
        return None;
    };
    let maximized = win.is_maximized().unwrap_or(false);
    // A maximized window reports the maximized rect: keep the previous normal rect.
    Some(match (maximized, prev) {
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
    })
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
    let app = window.app_handle();
    if window.label() != MAIN_WINDOW {
        match event {
            // A detached window: its page reattached its tab before closing.
            WindowEvent::Destroyed => {
                if let Some(b) = app.try_state::<Arc<TauriBridge>>() {
                    b.clear_subscribers(window.label());
                }
            }
            // Focus is any-of over the windows (`window_state`).
            WindowEvent::Focused(focused) => {
                if let Some(c) = core(app) {
                    c.publish(BusEvent::new(
                        bus::APP_FOCUS_CHANGED,
                        serde_json::json!({ "focused": focused }),
                    ));
                }
            }
            _ => {}
        }
        return;
    }
    match event {
        // The UI's close handler decides (`close_requested`): Tauri already prevents the close
        // while it listens, and finishes it itself unless the handler prevents the default.
        WindowEvent::CloseRequested { .. } => save_main_geometry(app),
        // Scheduler intervals and triggers follow focus (`app.focus_changed`).
        WindowEvent::Focused(focused) => {
            if let Some(c) = core(app) {
                c.publish(BusEvent::new(bus::APP_FOCUS_CHANGED, serde_json::json!({ "focused": focused })));
            }
            if *focused {
                crate::platform::update::auto_check(app);
            }
        }
        // The webview is gone: its event channels are dead; commands queue until the next one.
        WindowEvent::Destroyed => {
            if bench::enabled() {
                bench::mark("window_closed", 1.0);
            }
            if let Some(b) = app.try_state::<Arc<TauriBridge>>() {
                b.clear_subscribers(MAIN_WINDOW);
            }
            // Detached windows keep showing their sessions (background mode).
            if let Some(c) = core(app)
                && app.webview_windows().keys().all(|l| l == MAIN_WINDOW)
            {
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
        RunEvent::ExitRequested { .. } => save_geometries(app),
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
    let Some(c) = core(app) else { return };
    let paths = second_instance_paths(&argv, &cwd);
    if paths.is_empty() {
        return;
    }
    // Core's ctl `open` detects/creates/activates the project, then tells the UI to focus it.
    tauri::async_runtime::spawn(async move {
        for path in paths {
            if let Err(e) = c.ctl(CtlCommand::Open { path }).await {
                c.toast(Toast::error(format!("Opening the folder failed: {}", e.message)));
            }
        }
    });
}

/// macOS app menu replacing Tauri's default (Cmd+H/M/Q are intentional). AppKit gives menu key
/// equivalents before the webview sees the keydown, so a menu key shadows the keymap: Cmd+W is
/// left to `pane.close` (#324) and Close Window takes Cmd+Shift+W (see `MENU_KEYS`).
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
        &[
            &P::minimize(app, None)?,
            &P::maximize(app, None)?,
            &MenuItem::with_id(app, "close_window", "Close Window", true, Some("CmdOrCtrl+Shift+W"))?,
        ],
    )?;
    Menu::with_items(app, &[&kelta, &edit, &window])
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Key equivalents of `app_menu` that never reach the webview, in keymap chord syntax (the
    /// Edit items are left out: they send copy:/paste:/… to the webview itself).
    const MENU_KEYS: &[&str] = &["cmd+q", "cmd+h", "cmd+alt+h", "cmd+m", "cmd+shift+w"];

    #[test]
    fn app_menu_keys_leave_default_bindings_to_the_keymap() {
        for a in kelta_proto::actions::ACTIONS {
            for k in a.mac {
                assert!(!MENU_KEYS.contains(k), "{k} of {} is eaten by the app menu", a.id);
            }
        }
        assert!(kelta_proto::actions::find("pane.close").is_some_and(|a| a.mac == ["cmd+w"]));
    }

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
    fn detached_labels_fit_the_capability() {
        assert_eq!(detached_label(&TabId::new("tab-1a2b")), "detached-tab-1a2b");
        assert_eq!(detached_label(&TabId::new("t.x/é")), "detached-t_x__");
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

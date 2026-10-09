//! Per-webview platform hooks (ARCHITECTURE §12.4, §13):
//! - Linux: WebKitGTK `CacheModel::DocumentViewer` (smaller resident caches) and a
//!   `web-process-terminated` reload.
//! - macOS: a `webViewWebContentProcessDidTerminate:` handler added to wry's navigation delegate
//!   (reload) and the WebContent pid (via `_webProcessIdentifier`) for `perf_snapshot`.

use std::sync::atomic::{AtomicU32, Ordering};

use tauri::{Runtime, WebviewWindow};

/// Reloads allowed after WebContent terminations before giving up (a crash loop would otherwise
/// spin; the launch crash guard and the diagnostics pane take over).
pub const MAX_AUTO_RELOADS: u32 = 5;

static RELOADS: AtomicU32 = AtomicU32::new(0);

/// True while another automatic reload is still allowed.
pub fn allow_reload() -> bool {
    RELOADS.fetch_add(1, Ordering::Relaxed) < MAX_AUTO_RELOADS
}

/// Called once per created main webview.
#[cfg(target_os = "linux")]
pub fn configure<R: Runtime>(win: &WebviewWindow<R>) {
    let _ = win.with_webview(|wv| {
        use webkit2gtk::{CacheModel, WebContextExt, WebViewExt};
        let view = wv.inner();
        if let Some(ctx) = view.context() {
            ctx.set_cache_model(CacheModel::DocumentViewer);
        }
        view.connect_web_process_terminated(|v, _reason| {
            if allow_reload() {
                v.reload();
            }
        });
    });
}

#[cfg(target_os = "macos")]
pub fn configure<R: Runtime>(win: &WebviewWindow<R>) {
    let _ = win.with_webview(|wv| {
        // SAFETY: runs on the main thread; `inner()` is the live WKWebView of this window.
        unsafe { macos::install_terminate_handler(wv.inner()) };
    });
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
pub fn configure<R: Runtime>(_win: &WebviewWindow<R>) {}

/// Pids of WebKit helper processes belonging to this app.
#[cfg(target_os = "macos")]
pub fn helper_pids<R: Runtime>(win: &WebviewWindow<R>) -> Vec<u32> {
    let (tx, rx) = std::sync::mpsc::channel();
    let sent = win.with_webview(move |wv| {
        // SAFETY: main thread, live WKWebView.
        let pid = unsafe { macos::web_process_id(wv.inner()) };
        let _ = tx.send(pid);
    });
    if sent.is_err() {
        return Vec::new();
    }
    // one-shot: the main thread answers within a frame; never block a worker longer than this
    match rx.recv_timeout(std::time::Duration::from_millis(500)) {
        Ok(pid) if pid > 0 => vec![pid],
        _ => Vec::new(),
    }
}

/// Pids of WebKit helper processes belonging to this app (children named `WebKit*`).
#[cfg(not(target_os = "macos"))]
pub fn helper_pids<R: Runtime>(_win: &WebviewWindow<R>) -> Vec<u32> {
    #[cfg(target_os = "linux")]
    {
        linux_helpers(std::process::id())
    }
    #[cfg(not(target_os = "linux"))]
    {
        Vec::new()
    }
}

#[cfg(target_os = "linux")]
fn linux_helpers(me: u32) -> Vec<u32> {
    let Ok(rd) = std::fs::read_dir("/proc") else { return Vec::new() };
    rd.filter_map(|e| {
        let e = e.ok()?;
        let pid: u32 = e.file_name().to_str()?.parse().ok()?;
        let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
        let (comm, ppid) = parse_stat(&stat)?;
        (ppid == me && comm.starts_with("WebKit")).then_some(pid)
    })
    .collect()
}

/// `(comm, ppid)` from `/proc/<pid>/stat` (the comm may contain spaces and parentheses).
pub fn parse_stat(stat: &str) -> Option<(String, u32)> {
    let open = stat.find('(')?;
    let close = stat.rfind(')')?;
    let comm = stat.get(open + 1..close)?.to_owned();
    let rest = stat.get(close + 1..)?.split_whitespace().collect::<Vec<_>>();
    let ppid = rest.get(1)?.parse().ok()?;
    Some((comm, ppid))
}

#[cfg(target_os = "macos")]
mod macos {
    use std::ffi::c_void;

    use objc2::ffi::class_addMethod;
    use objc2::runtime::{AnyObject, Sel};
    use objc2::{msg_send, sel};

    unsafe extern "C" fn did_terminate(_this: *mut AnyObject, _sel: Sel, web_view: *mut AnyObject) {
        if super::allow_reload() && !web_view.is_null() {
            // SAFETY: WebKit passes the live WKWebView.
            let _: () = unsafe { msg_send![web_view, reload] };
        }
    }

    /// Adds `webViewWebContentProcessDidTerminate:` to the class of the webview's navigation
    /// delegate (wry's), unless it already implements it.
    ///
    /// # Safety
    /// `wk` must be a live `WKWebView*`; call on the main thread.
    pub unsafe fn install_terminate_handler(wk: *mut c_void) {
        let wk = wk.cast::<AnyObject>();
        if wk.is_null() {
            return;
        }
        // SAFETY: caller contract.
        let delegate: *mut AnyObject = unsafe { msg_send![wk, navigationDelegate] };
        if delegate.is_null() {
            return;
        }
        // SAFETY: `delegate` is a live object; its class is valid for the process lifetime.
        let class = unsafe { (*delegate).class() };
        let imp: unsafe extern "C" fn(*mut AnyObject, Sel, *mut AnyObject) = did_terminate;
        // SAFETY: the type encoding `v@:@` matches `did_terminate`'s signature.
        unsafe {
            class_addMethod(
                std::ptr::from_ref(class).cast_mut().cast(),
                sel!(webViewWebContentProcessDidTerminate:),
                std::mem::transmute::<
                    unsafe extern "C" fn(*mut AnyObject, Sel, *mut AnyObject),
                    unsafe extern "C-unwind" fn(),
                >(imp),
                c"v@:@".as_ptr(),
            );
        }
    }

    /// WebContent process id (`-[WKWebView _webProcessIdentifier]`, 0 when not running).
    ///
    /// # Safety
    /// `wk` must be a live `WKWebView*`; call on the main thread.
    pub unsafe fn web_process_id(wk: *mut c_void) -> u32 {
        let wk = wk.cast::<AnyObject>();
        if wk.is_null() {
            return 0;
        }
        // SAFETY: caller contract; the selector is checked first.
        unsafe {
            let responds: bool = msg_send![wk, respondsToSelector: sel!(_webProcessIdentifier)];
            if !responds {
                return 0;
            }
            let pid: i32 = msg_send![wk, _webProcessIdentifier];
            u32::try_from(pid).unwrap_or(0)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_stat_with_awkward_comm() {
        let line = "4242 (WebKitWebProcess) S 1000 4242 4242 0 -1 4194560";
        assert_eq!(parse_stat(line), Some(("WebKitWebProcess".into(), 1000)));
        let odd = "7 (we (ird) name) R 55 7 7 0";
        assert_eq!(parse_stat(odd), Some(("we (ird) name".into(), 55)));
        assert_eq!(parse_stat("garbage"), None);
    }

    #[test]
    fn reload_budget_is_bounded() {
        let allowed = (0..20).filter(|_| allow_reload()).count();
        assert!(allowed <= MAX_AUTO_RELOADS as usize);
    }
}

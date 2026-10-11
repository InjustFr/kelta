//! macOS Dock: tile badge (sessions needing input) and Dock Quit (`terminate:`). Call on the main
//! thread.

use std::sync::OnceLock;

use objc2::ffi::class_addMethod;
use objc2::runtime::{AnyObject, Sel};
use objc2::{class, msg_send, sel};
use objc2_foundation::NSString;

/// Sets the Dock badge label (`None` clears it).
pub fn set_badge(label: Option<&str>) {
    // SAFETY: AppKit calls on the main thread (callers use `run_on_main_thread`); both receivers
    // are valid for the process lifetime and `setBadgeLabel:` accepts nil.
    unsafe {
        let app: *mut AnyObject = msg_send![class!(NSApplication), sharedApplication];
        if app.is_null() {
            return;
        }
        let tile: *mut AnyObject = msg_send![app, dockTile];
        if tile.is_null() {
            return;
        }
        match label {
            Some(text) => {
                let s = NSString::from_str(text);
                let _: () = msg_send![tile, setBadgeLabel: &*s];
            }
            None => {
                let _: () = msg_send![tile, setBadgeLabel: std::ptr::null::<NSString>()];
            }
        }
    }
}

/// Asked by `applicationShouldTerminate:`: true quits now, false cancels the terminate.
static SHOULD_TERMINATE: OnceLock<Box<dyn Fn() -> bool + Send + Sync>> = OnceLock::new();

unsafe extern "C" fn should_terminate(_this: *mut AnyObject, _sel: Sel, _app: *mut AnyObject) -> usize {
    // NSTerminateNow = 1, NSTerminateCancel = 0
    usize::from(SHOULD_TERMINATE.get().is_none_or(|f| f()))
}

/// Routes `NSApp terminate:` (Dock Quit, logout) through `decide` by adding
/// `applicationShouldTerminate:` to tao's app delegate, which does not implement it. Call once,
/// on the main thread, after the event loop is built.
pub fn on_should_terminate(decide: impl Fn() -> bool + Send + Sync + 'static) {
    if SHOULD_TERMINATE.set(Box::new(decide)).is_err() {
        return;
    }
    // SAFETY: main thread; the delegate and its class live for the process lifetime; the type
    // encoding `Q@:@` (NSUInteger return) matches `should_terminate`'s signature.
    unsafe {
        let app: *mut AnyObject = msg_send![class!(NSApplication), sharedApplication];
        if app.is_null() {
            return;
        }
        let delegate: *mut AnyObject = msg_send![app, delegate];
        if delegate.is_null() {
            return;
        }
        let imp: unsafe extern "C" fn(*mut AnyObject, Sel, *mut AnyObject) -> usize = should_terminate;
        class_addMethod(
            std::ptr::from_ref((*delegate).class()).cast_mut().cast(),
            sel!(applicationShouldTerminate:),
            std::mem::transmute::<
                unsafe extern "C" fn(*mut AnyObject, Sel, *mut AnyObject) -> usize,
                unsafe extern "C-unwind" fn(),
            >(imp),
            c"Q@:@".as_ptr(),
        );
    }
}

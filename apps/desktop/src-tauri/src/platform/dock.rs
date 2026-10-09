//! macOS Dock tile badge (sessions needing input). Call on the main thread.

use objc2::runtime::AnyObject;
use objc2::{class, msg_send};
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

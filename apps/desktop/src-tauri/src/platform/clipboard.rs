//! Linux clipboard through GTK (ARCHITECTURE D13): GTK talks to the compositor as the focused client, so
//! GNOME Wayland (no wlr/ext data-control, mutter#524) works where arboard falls back to XWayland.

use std::sync::mpsc::sync_channel;
use std::time::Duration;

use gtk::{gdk, glib};
use kelta_core::clipboard::ClipboardBackend;
use kelta_proto::error::KeltaError;
use kelta_proto::term::ClipboardKind;

const READ_TIMEOUT: Duration = Duration::from_secs(2);

/// Runs every GTK call on the thread that owns the default main context (tao's GTK loop).
pub struct GtkClipboard;

fn selection(kind: ClipboardKind) -> gdk::Atom {
    match kind {
        ClipboardKind::Clipboard => gdk::SELECTION_CLIPBOARD,
        ClipboardKind::Primary => gdk::SELECTION_PRIMARY,
    }
}

impl ClipboardBackend for GtkClipboard {
    fn read(&self, kind: ClipboardKind) -> Result<String, KeltaError> {
        let ctx = glib::MainContext::default();
        // The reply is delivered on the GTK thread: waiting for it there would deadlock.
        if ctx.is_owner() {
            return Err(KeltaError::internal("clipboard read on the GTK main thread"));
        }
        let (tx, rx) = sync_channel(1);
        ctx.invoke(move || {
            gtk::Clipboard::get(&selection(kind)).request_text(move |_, t| {
                let _ = tx.send(t.map(str::to_owned));
            });
        });
        match rx.recv_timeout(READ_TIMEOUT) {
            Ok(Some(t)) => Ok(t),
            Ok(None) => Err(KeltaError::not_found("clipboard is empty")),
            Err(_) => Err(KeltaError::timeout("clipboard read timed out")),
        }
    }

    fn write(&self, kind: ClipboardKind, text: &str) -> Result<(), KeltaError> {
        let text = text.to_owned();
        // From the GTK thread `invoke` runs inline; elsewhere it queues in call order.
        glib::MainContext::default().invoke(move || gtk::Clipboard::get(&selection(kind)).set_text(&text));
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kinds_map_to_gdk_selections() {
        assert!(selection(ClipboardKind::Clipboard) == gdk::SELECTION_CLIPBOARD);
        assert!(selection(ClipboardKind::Primary) == gdk::SELECTION_PRIMARY);
    }

    #[test]
    fn read_on_the_gtk_thread_is_refused() {
        let ctx = glib::MainContext::default();
        let _owner = ctx.acquire().unwrap();
        let e = GtkClipboard.read(ClipboardKind::Clipboard).unwrap_err();
        assert!(e.is(kelta_proto::error::ErrorCode::Internal), "{e:?}");
    }

    #[test]
    #[ignore = "needs a display: xvfb-run cargo nextest run -p kelta-desktop --run-ignored only -E 'test(gtk_round_trip)'"]
    #[allow(clippy::disallowed_methods)] // allowlisted: test, a caller off the GTK thread
    fn gtk_round_trip() {
        gtk::init().unwrap();
        let caller = std::thread::spawn(|| {
            let r = [ClipboardKind::Clipboard, ClipboardKind::Primary].map(|k| {
                GtkClipboard.write(k, &format!("kelta-523 {k:?}")).and_then(|()| GtkClipboard.read(k))
            });
            glib::MainContext::default().invoke(gtk::main_quit);
            r
        });
        gtk::main();
        let [c, p] = caller.join().unwrap();
        assert_eq!(c.unwrap(), "kelta-523 Clipboard");
        assert_eq!(p.unwrap(), "kelta-523 Primary");
    }
}

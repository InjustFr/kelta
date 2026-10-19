//! Linux clipboard through GTK (ARCHITECTURE D13): GTK talks to the compositor as the focused client, so
//! GNOME Wayland (no wlr/ext data-control, mutter#524) works where arboard falls back to XWayland.

use std::sync::mpsc::sync_channel;
use std::time::Duration;

use gtk::{gdk, glib};
use kelta_core::clipboard::ClipboardBackend;
use kelta_proto::error::KeltaError;
use kelta_proto::term::ClipboardKind;

const READ_TIMEOUT: Duration = Duration::from_secs(2);

type Job = Box<dyn FnOnce() + Send>;
type Dispatch = Box<dyn Fn(Job) -> Result<(), KeltaError> + Send + Sync>;

/// Runs every GTK call on tao's main thread. `invoke` would not do: GLib runs it inline on any thread that can
/// take the default context, and tao releases it between loop iterations. `run_on_main_thread` always queues.
pub struct GtkClipboard(Dispatch);

impl GtkClipboard {
    pub fn new(app: tauri::AppHandle) -> Self {
        Self(Box::new(move |job| {
            app.run_on_main_thread(job).map_err(|e| KeltaError::internal(format!("clipboard dispatch: {e}")))
        }))
    }
}

fn selection(kind: ClipboardKind) -> gdk::Atom {
    match kind {
        ClipboardKind::Clipboard => gdk::SELECTION_CLIPBOARD,
        ClipboardKind::Primary => gdk::SELECTION_PRIMARY,
    }
}

impl ClipboardBackend for GtkClipboard {
    fn read(&self, kind: ClipboardKind) -> Result<String, KeltaError> {
        // The reply is delivered on the GTK thread: waiting for it there would deadlock.
        if glib::MainContext::default().is_owner() {
            return Err(KeltaError::internal("clipboard read on the GTK main thread"));
        }
        let (tx, rx) = sync_channel(1);
        (self.0)(Box::new(move || {
            gtk::Clipboard::get(&selection(kind)).request_text(move |_, t| {
                let _ = tx.send(t.map(str::to_owned));
            });
        }))?;
        match rx.recv_timeout(READ_TIMEOUT) {
            Ok(Some(t)) => Ok(t),
            Ok(None) => Err(KeltaError::not_found("clipboard is empty")),
            Err(_) => Err(KeltaError::timeout("clipboard read timed out")),
        }
    }

    fn write(&self, kind: ClipboardKind, text: &str) -> Result<(), KeltaError> {
        let text = text.to_owned();
        (self.0)(Box::new(move || gtk::Clipboard::get(&selection(kind)).set_text(&text)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Test dispatcher: queues on the GLib context, which the test owns so the jobs never run on the caller.
    fn inline() -> Dispatch {
        Box::new(|job| {
            glib::MainContext::default().invoke(job);
            Ok(())
        })
    }

    #[test]
    fn kinds_map_to_gdk_selections() {
        assert!(selection(ClipboardKind::Clipboard) == gdk::SELECTION_CLIPBOARD);
        assert!(selection(ClipboardKind::Primary) == gdk::SELECTION_PRIMARY);
    }

    #[test]
    fn read_on_the_gtk_thread_is_refused() {
        let ctx = glib::MainContext::default();
        let _owner = ctx.acquire().unwrap();
        let e = GtkClipboard(inline()).read(ClipboardKind::Clipboard).unwrap_err();
        assert!(e.is(kelta_proto::error::ErrorCode::Internal), "{e:?}");
    }

    #[test]
    #[ignore = "needs a display: xvfb-run cargo nextest run -p kelta-desktop --run-ignored only -E 'test(gtk_round_trip)'"]
    #[allow(clippy::disallowed_methods)] // allowlisted: test, a caller off the GTK thread
    fn gtk_round_trip() {
        gtk::init().unwrap();
        // Own the context so off-thread `invoke`s queue instead of running inline on the caller thread.
        let clip = GtkClipboard(inline());
        let ctx = glib::MainContext::default();
        let _owner = ctx.acquire().unwrap();
        let caller = std::thread::spawn(move || {
            let r = [ClipboardKind::Clipboard, ClipboardKind::Primary]
                .map(|k| clip.write(k, &format!("kelta-523 {k:?}")).and_then(|()| clip.read(k)));
            glib::MainContext::default().invoke(gtk::main_quit);
            r
        });
        gtk::main();
        let [c, p] = caller.join().unwrap();
        assert_eq!(c.unwrap(), "kelta-523 Clipboard");
        assert_eq!(p.unwrap(), "kelta-523 Primary");
    }
}

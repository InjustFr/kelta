//! Clipboard via `arboard` (ARCHITECTURE D13): CLIPBOARD everywhere, PRIMARY on Linux (emulated
//! in-process on macOS, which has no primary selection).

use kelta_proto::error::KeltaError;
use kelta_proto::term::ClipboardKind;
use parking_lot::Mutex;

pub trait ClipboardBackend: Send + Sync {
    fn read(&self, kind: ClipboardKind) -> Result<String, KeltaError>;
    fn write(&self, kind: ClipboardKind, text: &str) -> Result<(), KeltaError>;
}

fn clip_err(e: arboard::Error) -> KeltaError {
    match e {
        arboard::Error::ContentNotAvailable => KeltaError::not_found("clipboard is empty"),
        arboard::Error::ClipboardNotSupported => KeltaError::unsupported("clipboard not supported"),
        other => KeltaError::internal(format!("clipboard: {other}")),
    }
}

/// The system clipboard. On Linux one `arboard::Clipboard` is kept alive (it owns the selection
/// served to other apps).
#[derive(Default)]
pub struct SystemClipboard {
    #[cfg(target_os = "linux")]
    inner: Mutex<Option<arboard::Clipboard>>,
    /// macOS has no PRIMARY: keep it in-process.
    #[cfg(not(target_os = "linux"))]
    primary: Mutex<String>,
}

#[cfg(target_os = "linux")]
impl SystemClipboard {
    fn with<R>(
        &self,
        f: impl FnOnce(&mut arboard::Clipboard) -> Result<R, arboard::Error>,
    ) -> Result<R, KeltaError> {
        let mut g = self.inner.lock();
        if g.is_none() {
            *g = Some(arboard::Clipboard::new().map_err(clip_err)?);
        }
        match g.as_mut() {
            Some(c) => f(c).map_err(clip_err),
            None => Err(KeltaError::unsupported("clipboard unavailable")),
        }
    }
}

impl ClipboardBackend for SystemClipboard {
    #[cfg(target_os = "linux")]
    fn read(&self, kind: ClipboardKind) -> Result<String, KeltaError> {
        use arboard::{GetExtLinux, LinuxClipboardKind};
        let k = if kind == ClipboardKind::Primary {
            LinuxClipboardKind::Primary
        } else {
            LinuxClipboardKind::Clipboard
        };
        self.with(|c| c.get().clipboard(k).text())
    }

    #[cfg(target_os = "linux")]
    fn write(&self, kind: ClipboardKind, text: &str) -> Result<(), KeltaError> {
        use arboard::{LinuxClipboardKind, SetExtLinux};
        let k = if kind == ClipboardKind::Primary {
            LinuxClipboardKind::Primary
        } else {
            LinuxClipboardKind::Clipboard
        };
        let t = text.to_owned();
        self.with(|c| c.set().clipboard(k).text(t))
    }

    #[cfg(not(target_os = "linux"))]
    fn read(&self, kind: ClipboardKind) -> Result<String, KeltaError> {
        if kind == ClipboardKind::Primary {
            return Ok(self.primary.lock().clone());
        }
        arboard::Clipboard::new().and_then(|mut c| c.get_text()).map_err(clip_err)
    }

    #[cfg(not(target_os = "linux"))]
    fn write(&self, kind: ClipboardKind, text: &str) -> Result<(), KeltaError> {
        if kind == ClipboardKind::Primary {
            *self.primary.lock() = text.to_owned();
            return Ok(());
        }
        arboard::Clipboard::new().and_then(|mut c| c.set_text(text.to_owned())).map_err(clip_err)
    }
}

/// In-memory clipboard (tests, headless).
#[derive(Default)]
pub struct MemClipboard {
    clipboard: Mutex<String>,
    primary: Mutex<String>,
}

impl ClipboardBackend for MemClipboard {
    fn read(&self, kind: ClipboardKind) -> Result<String, KeltaError> {
        Ok(match kind {
            ClipboardKind::Clipboard => self.clipboard.lock().clone(),
            ClipboardKind::Primary => self.primary.lock().clone(),
        })
    }

    fn write(&self, kind: ClipboardKind, text: &str) -> Result<(), KeltaError> {
        match kind {
            ClipboardKind::Clipboard => *self.clipboard.lock() = text.to_owned(),
            ClipboardKind::Primary => *self.primary.lock() = text.to_owned(),
        }
        Ok(())
    }
}

impl crate::Core {
    /// `clipboard_read` (blocking work off the async workers).
    pub async fn clipboard_read(&self, kind: ClipboardKind) -> Result<String, KeltaError> {
        let clip = self.clip.clone();
        self.rt.blocking(move || clip.read(kind)).await
    }

    /// `clipboard_write`.
    pub async fn clipboard_write(&self, kind: ClipboardKind, text: String) -> Result<(), KeltaError> {
        let clip = self.clip.clone();
        self.rt.blocking(move || clip.write(kind, &text)).await
    }
}

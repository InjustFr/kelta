//! Window geometry persistence (`window.restore_geometry`). Stored as `<data>/window-geometry.json`
//! (the `ui_state` table is core-private; see docs/contract-requests/L10.md).

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

pub const FILE: &str = "window-geometry.json";

/// Logical-pixel geometry of the main window.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Geometry {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
    #[serde(default)]
    pub maximized: bool,
}

pub const MIN_WIDTH: f64 = 640.0;
pub const MIN_HEIGHT: f64 = 400.0;
pub const DEFAULT_WIDTH: f64 = 1280.0;
pub const DEFAULT_HEIGHT: f64 = 800.0;

impl Geometry {
    /// Rejects NaN/absurd values so a corrupt file never produces an invisible window.
    pub fn sane(&self) -> bool {
        let finite = [self.x, self.y, self.width, self.height].iter().all(|v| v.is_finite());
        finite
            && self.width >= MIN_WIDTH
            && self.height >= MIN_HEIGHT
            && self.width <= 16384.0
            && self.height <= 16384.0
            && self.x.abs() <= 32768.0
            && self.y.abs() <= 32768.0
    }

    /// True when the top-left corner lies inside one of `monitors` (x, y, w, h in logical px),
    /// so a window saved on a now-disconnected screen is not restored off-screen.
    pub fn visible_on(&self, monitors: &[(f64, f64, f64, f64)]) -> bool {
        monitors.is_empty()
            || monitors.iter().any(|(mx, my, mw, mh)| {
                self.x + 40.0 >= *mx && self.y >= *my && self.x + 40.0 < mx + mw && self.y + 40.0 < my + mh
            })
    }
}

pub fn path(data_dir: &Path) -> PathBuf {
    data_dir.join(FILE)
}

pub fn load(data_dir: &Path) -> Option<Geometry> {
    let text = std::fs::read_to_string(path(data_dir)).ok()?;
    serde_json::from_str::<Geometry>(&text).ok().filter(Geometry::sane)
}

/// Best effort; errors are ignored.
pub fn save(data_dir: &Path, g: &Geometry) {
    if !g.sane() {
        return;
    }
    let _ = std::fs::create_dir_all(data_dir);
    if let Ok(text) = serde_json::to_string(g) {
        let tmp = data_dir.join(format!("{FILE}.tmp"));
        if std::fs::write(&tmp, text).is_ok() {
            let _ = std::fs::rename(&tmp, path(data_dir));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn g(x: f64, y: f64, w: f64, h: f64) -> Geometry {
        Geometry { x, y, width: w, height: h, maximized: false }
    }

    #[test]
    fn round_trips() {
        let tmp = tempfile::tempdir().unwrap();
        let geo = g(10.0, 20.0, 1000.0, 700.0);
        save(tmp.path(), &geo);
        assert_eq!(load(tmp.path()), Some(geo));
    }

    #[test]
    fn rejects_garbage_and_insane_values() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(path(tmp.path()), "not json").unwrap();
        assert_eq!(load(tmp.path()), None);
        assert!(!g(0.0, 0.0, 10.0, 10.0).sane());
        assert!(!g(f64::NAN, 0.0, 800.0, 600.0).sane());
        save(tmp.path(), &g(0.0, 0.0, 1.0, 1.0));
        assert_eq!(load(tmp.path()), None);
    }

    #[test]
    fn off_screen_geometry_is_not_visible() {
        let screens = [(0.0, 0.0, 1920.0, 1080.0)];
        assert!(g(100.0, 100.0, 800.0, 600.0).visible_on(&screens));
        assert!(!g(3000.0, 100.0, 800.0, 600.0).visible_on(&screens));
        assert!(g(3000.0, 100.0, 800.0, 600.0).visible_on(&[]));
    }
}

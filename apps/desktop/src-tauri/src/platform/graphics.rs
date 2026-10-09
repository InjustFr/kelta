//! Linux graphics environment decision table (ARCHITECTURE §14, SETTINGS `[linux.graphics]`).
//!
//! Everything here is a pure function over explicit inputs so the table is unit-tested from fixture
//! `/proc` and `/sys` trees. Applying the result to the process environment is [`apply`].

use std::path::Path;

use kelta_proto::settings::{GdkBackend, GraphicsProfile, LinuxGraphics};

pub const ENV_DMABUF: &str = "WEBKIT_DISABLE_DMABUF_RENDERER";
pub const ENV_COMPOSITING: &str = "WEBKIT_DISABLE_COMPOSITING_MODE";
pub const ENV_EXPLICIT_SYNC: &str = "__NV_DISABLE_EXPLICIT_SYNC";
pub const ENV_GDK_BACKEND: &str = "GDK_BACKEND";

/// Inputs of the decision table.
#[derive(Debug, Clone, Copy)]
pub struct GraphicsInputs {
    /// `[linux.graphics]` read early from `config.toml`.
    pub cfg: LinuxGraphics,
    /// `--safe-graphics` was passed.
    pub safe_flag: bool,
    /// The launch crash guard asks for the safe profile.
    pub guard_safe: bool,
    /// NVIDIA proprietary driver detected.
    pub nvidia: bool,
}

/// True when the safe profile is in effect (every workaround on).
pub fn is_safe(i: &GraphicsInputs) -> bool {
    i.safe_flag || i.guard_safe || i.cfg.profile == GraphicsProfile::Safe
}

/// The environment variables to export, in a stable order. Variables already present in the
/// process environment are never overridden by [`apply`].
pub fn decide(i: &GraphicsInputs) -> Vec<(&'static str, &'static str)> {
    let safe = is_safe(i);
    let auto_nvidia = i.cfg.profile == GraphicsProfile::Auto && i.cfg.auto_nvidia && i.nvidia;
    let mut out = Vec::new();
    if safe || auto_nvidia || i.cfg.disable_dmabuf {
        out.push((ENV_DMABUF, "1"));
    }
    if safe || i.cfg.disable_compositing {
        out.push((ENV_COMPOSITING, "1"));
    }
    if safe || auto_nvidia || i.cfg.nvidia_disable_explicit_sync {
        out.push((ENV_EXPLICIT_SYNC, "1"));
    }
    if safe {
        out.push((ENV_GDK_BACKEND, "x11"));
    } else {
        match i.cfg.gdk_backend {
            GdkBackend::Auto => {}
            GdkBackend::Wayland => out.push((ENV_GDK_BACKEND, "wayland")),
            GdkBackend::X11 => out.push((ENV_GDK_BACKEND, "x11")),
        }
    }
    out
}

/// NVIDIA proprietary driver detection: `<root>/proc/driver/nvidia/version` exists, or the
/// `nvidia_drm` kernel module is loaded (`<root>/sys/module/nvidia_drm`). Nouveau does not match.
pub fn detect_nvidia(root: &Path) -> bool {
    root.join("proc/driver/nvidia/version").is_file() || root.join("sys/module/nvidia_drm").is_dir()
}

/// Exports `vars` unless the user already set them. Returns the variables actually set.
///
/// Must be called single-threaded, before any GTK/WebKit init (`std::env::set_var` is unsafe for
/// that reason).
pub fn apply(vars: &[(&'static str, &'static str)]) -> Vec<(&'static str, &'static str)> {
    let mut set = Vec::new();
    for (k, v) in vars {
        if std::env::var_os(k).is_none() {
            // SAFETY: `pre_init` runs first in `main`, before the async runtime, GTK or any
            // thread of ours exists, so nothing reads the environment concurrently.
            unsafe { std::env::set_var(k, v) };
            set.push((*k, *v));
        }
    }
    set
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(name: &str) -> std::path::PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("src/platform/fixtures").join(name)
    }

    fn inputs() -> GraphicsInputs {
        GraphicsInputs { cfg: LinuxGraphics::default(), safe_flag: false, guard_safe: false, nvidia: false }
    }

    #[test]
    fn detects_nvidia_from_proc_version() {
        assert!(detect_nvidia(&fixture("nvidia")));
    }

    #[test]
    fn detects_nvidia_from_nvidia_drm_module() {
        assert!(detect_nvidia(&fixture("nvidia_drm_only")));
    }

    #[test]
    fn nouveau_and_empty_are_not_nvidia() {
        assert!(!detect_nvidia(&fixture("nouveau")));
        assert!(!detect_nvidia(&fixture("empty")));
        assert!(!detect_nvidia(&fixture("does-not-exist")));
    }

    #[test]
    fn mesa_default_sets_nothing() {
        assert!(decide(&inputs()).is_empty());
    }

    #[test]
    fn auto_nvidia_disables_dmabuf_and_explicit_sync() {
        let i = GraphicsInputs { nvidia: true, ..inputs() };
        assert_eq!(decide(&i), vec![(ENV_DMABUF, "1"), (ENV_EXPLICIT_SYNC, "1")]);
    }

    #[test]
    fn auto_nvidia_off_ignores_detection() {
        let mut i = GraphicsInputs { nvidia: true, ..inputs() };
        i.cfg.auto_nvidia = false;
        assert!(decide(&i).is_empty());
    }

    #[test]
    fn default_profile_ignores_nvidia_but_honours_toggles() {
        let mut i = GraphicsInputs { nvidia: true, ..inputs() };
        i.cfg.profile = GraphicsProfile::Default;
        assert!(decide(&i).is_empty());
        i.cfg.disable_compositing = true;
        assert_eq!(decide(&i), vec![(ENV_COMPOSITING, "1")]);
    }

    #[test]
    fn explicit_toggles_and_backend() {
        let mut i = inputs();
        i.cfg.disable_dmabuf = true;
        i.cfg.nvidia_disable_explicit_sync = true;
        i.cfg.gdk_backend = GdkBackend::Wayland;
        assert_eq!(
            decide(&i),
            vec![(ENV_DMABUF, "1"), (ENV_EXPLICIT_SYNC, "1"), (ENV_GDK_BACKEND, "wayland")]
        );
    }

    #[test]
    fn safe_sources_set_everything() {
        let all = vec![
            (ENV_DMABUF, "1"),
            (ENV_COMPOSITING, "1"),
            (ENV_EXPLICIT_SYNC, "1"),
            (ENV_GDK_BACKEND, "x11"),
        ];
        assert_eq!(decide(&GraphicsInputs { safe_flag: true, ..inputs() }), all);
        assert_eq!(decide(&GraphicsInputs { guard_safe: true, ..inputs() }), all);
        let mut i = inputs();
        i.cfg.profile = GraphicsProfile::Safe;
        i.cfg.gdk_backend = GdkBackend::Wayland;
        assert_eq!(decide(&i), all);
    }

    #[test]
    fn apply_does_not_override_user_env() {
        // Unique names so parallel tests never race on the same variable.
        const USER: &str = "KELTA_TEST_GFX_USER_SET";
        const FRESH: &str = "KELTA_TEST_GFX_FRESH";
        // SAFETY: test-only, names are unique to this test.
        unsafe { std::env::set_var(USER, "keep") };
        let set = apply(&[(USER, "1"), (FRESH, "1")]);
        assert_eq!(set, vec![(FRESH, "1")]);
        assert_eq!(std::env::var(USER).as_deref(), Ok("keep"));
        assert_eq!(std::env::var(FRESH).as_deref(), Ok("1"));
    }
}

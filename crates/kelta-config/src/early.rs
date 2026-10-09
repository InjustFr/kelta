//! Reads needed before any GTK/WebKit init (single-threaded `platform::pre_init`).

use std::path::Path;

use kelta_proto::settings::{GdkBackend, GraphicsProfile, LinuxGraphics};

fn flag(t: &toml::Table, key: &str, default: bool) -> bool {
    t.get(key).and_then(toml::Value::as_bool).unwrap_or(default)
}

fn pick<T>(t: &toml::Table, key: &str, default: T, parse: impl Fn(&str) -> Option<T>) -> T {
    t.get(key).and_then(toml::Value::as_str).and_then(parse).unwrap_or(default)
}

/// `[linux.graphics]` from `<config_dir>/config.toml` with a minimal, lenient parse: every field
/// that does not parse falls back to its default, a missing or broken file yields the defaults.
/// Never panics and never validates anything else in the file.
pub fn linux_graphics(config_dir: &Path) -> LinuxGraphics {
    let d = LinuxGraphics::default();
    let Ok(text) = std::fs::read_to_string(config_dir.join("config.toml")) else { return d };
    let Ok(doc) = text.parse::<toml::Table>() else { return d };
    let Some(g) = doc.get("linux").and_then(|l| l.get("graphics")).and_then(toml::Value::as_table) else {
        return d;
    };
    LinuxGraphics {
        profile: pick(g, "profile", d.profile, |s| match s {
            "auto" => Some(GraphicsProfile::Auto),
            "default" => Some(GraphicsProfile::Default),
            "safe" => Some(GraphicsProfile::Safe),
            _ => None,
        }),
        auto_nvidia: flag(g, "auto_nvidia", d.auto_nvidia),
        disable_dmabuf: flag(g, "disable_dmabuf", d.disable_dmabuf),
        disable_compositing: flag(g, "disable_compositing", d.disable_compositing),
        nvidia_disable_explicit_sync: flag(g, "nvidia_disable_explicit_sync", d.nvidia_disable_explicit_sync),
        gdk_backend: pick(g, "gdk_backend", d.gdk_backend, |s| match s {
            "auto" => Some(GdkBackend::Auto),
            "wayland" => Some(GdkBackend::Wayland),
            "x11" => Some(GdkBackend::X11),
            _ => None,
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lenient_parse() {
        let d = tempfile::tempdir().unwrap();
        assert_eq!(linux_graphics(d.path()), LinuxGraphics::default());
        std::fs::write(
            d.path().join("config.toml"),
            "[linux.graphics]\nprofile = \"safe\"\ndisable_dmabuf = true\nauto_nvidia = \"oops\"\ngdk_backend = \"x11\"\n",
        )
        .unwrap();
        let g = linux_graphics(d.path());
        assert_eq!(g.profile, GraphicsProfile::Safe);
        assert!(g.disable_dmabuf);
        assert!(g.auto_nvidia, "bad value falls back to the default");
        assert_eq!(g.gdk_backend, GdkBackend::X11);
        std::fs::write(d.path().join("config.toml"), "[[[ broken").unwrap();
        assert_eq!(linux_graphics(d.path()), LinuxGraphics::default());
    }
}

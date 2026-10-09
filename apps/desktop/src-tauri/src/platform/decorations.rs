//! Window decoration decision (SPEC §8, SETTINGS `[window]`): a pure function over the setting and
//! the compositor environment so it is unit-tested without a display.

use kelta_proto::settings::{CloseBehavior, Decorations};

/// What the window is actually built with.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Effective {
    /// System title bar.
    Native,
    /// No decorations, no app-drawn chrome (tiling compositors).
    None,
    /// No system decorations; the app provides a drag region and resize handles.
    Custom,
}

impl Effective {
    /// True when the system title bar is not drawn.
    pub fn undecorated(self) -> bool {
        self != Self::Native
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Native => "native",
            Self::None => "none",
            Self::Custom => "custom",
        }
    }
}

fn non_empty(v: Option<String>) -> bool {
    v.is_some_and(|s| !s.is_empty())
}

/// True on a tiling compositor known to ignore client-side title bars.
pub fn tiling_compositor(env: &impl Fn(&str) -> Option<String>) -> bool {
    non_empty(env("HYPRLAND_INSTANCE_SIGNATURE")) || non_empty(env("SWAYSOCK"))
}

/// `auto` = none on Hyprland/Sway (Linux only), native elsewhere.
pub fn decide(setting: Decorations, is_linux: bool, env: &impl Fn(&str) -> Option<String>) -> Effective {
    match setting {
        Decorations::Native => Effective::Native,
        Decorations::None => Effective::None,
        Decorations::Custom => Effective::Custom,
        Decorations::Auto if is_linux && tiling_compositor(env) => Effective::None,
        Decorations::Auto => Effective::Native,
    }
}

/// `window.close_behavior`: auto = background on macOS, quit on Linux.
pub fn closes_to_background(setting: CloseBehavior, is_macos: bool) -> bool {
    match setting {
        CloseBehavior::Background => true,
        CloseBehavior::Quit => false,
        CloseBehavior::Auto => is_macos,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn env<'a>(pairs: &'a [(&'a str, &'a str)]) -> impl Fn(&str) -> Option<String> + 'a {
        let m: HashMap<_, _> = pairs.iter().copied().collect();
        move |k| m.get(k).map(|v| (*v).to_owned())
    }

    #[test]
    fn auto_is_none_on_hyprland_and_sway() {
        let hypr = env(&[("HYPRLAND_INSTANCE_SIGNATURE", "abc")]);
        let sway = env(&[("SWAYSOCK", "/run/user/1000/sway.sock")]);
        assert_eq!(decide(Decorations::Auto, true, &hypr), Effective::None);
        assert_eq!(decide(Decorations::Auto, true, &sway), Effective::None);
    }

    #[test]
    fn auto_is_native_on_gnome_and_macos() {
        let gnome = env(&[("XDG_CURRENT_DESKTOP", "GNOME")]);
        assert_eq!(decide(Decorations::Auto, true, &gnome), Effective::Native);
        let hypr = env(&[("HYPRLAND_INSTANCE_SIGNATURE", "abc")]);
        assert_eq!(decide(Decorations::Auto, false, &hypr), Effective::Native);
    }

    #[test]
    fn empty_variables_do_not_count() {
        let e = env(&[("SWAYSOCK", "")]);
        assert_eq!(decide(Decorations::Auto, true, &e), Effective::Native);
    }

    #[test]
    fn explicit_settings_win_over_the_environment() {
        let hypr = env(&[("HYPRLAND_INSTANCE_SIGNATURE", "abc")]);
        assert_eq!(decide(Decorations::Native, true, &hypr), Effective::Native);
        assert_eq!(decide(Decorations::Custom, true, &hypr), Effective::Custom);
        assert_eq!(decide(Decorations::None, true, &env(&[])), Effective::None);
        assert!(Effective::Custom.undecorated() && !Effective::Native.undecorated());
    }

    #[test]
    fn close_behavior_auto_depends_on_platform() {
        assert!(closes_to_background(CloseBehavior::Auto, true));
        assert!(!closes_to_background(CloseBehavior::Auto, false));
        assert!(closes_to_background(CloseBehavior::Background, false));
        assert!(!closes_to_background(CloseBehavior::Quit, true));
    }
}

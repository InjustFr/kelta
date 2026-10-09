//! Platform glue before any GTK/WebKit init (owner: L10, ARCHITECTURE §14, SPEC §8):
//! `--safe-graphics`, early `[linux.graphics]` read, NVIDIA detection, WebKit env vars, launch crash
//! guard, `mallopt(M_ARENA_MAX, 2)` on glibc.
//!
//! SCAFFOLD STUB: no-op except reading `--safe-graphics`.

/// Result of [`pre_init`].
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PreInit {
    /// Start with the safe graphics profile (flag, or crash guard tripped).
    pub safe_graphics: bool,
}

/// Runs first in `main`, single-threaded. `args` excludes argv[0].
pub fn pre_init(args: &[String]) -> PreInit {
    PreInit { safe_graphics: args.iter().any(|a| a == "--safe-graphics") }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_flag() {
        assert!(pre_init(&["--safe-graphics".to_owned()]).safe_graphics);
        assert!(!pre_init(&[]).safe_graphics);
    }
}

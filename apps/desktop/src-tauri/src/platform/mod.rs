//! Platform glue before any GTK/WebKit init (owner: L10, ARCHITECTURE §14, SPEC §8):
//! `--safe-graphics`, early `[linux.graphics]` read, NVIDIA detection, WebKit env vars, launch crash
//! guard, `mallopt(M_ARENA_MAX, 2)` on glibc.

pub mod graphics;
pub mod guard;

use std::path::PathBuf;
use std::sync::OnceLock;

/// Result of [`pre_init`].
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PreInit {
    /// Start with the safe graphics profile (flag, or crash guard tripped).
    pub safe_graphics: bool,
}

/// What `pre_init` decided, kept for diagnostics.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AppliedGraphics {
    pub safe: bool,
    pub nvidia: bool,
    pub guard_tripped: bool,
    /// Variables this process exported (user-provided ones are not listed).
    pub vars: Vec<(String, String)>,
}

static APPLIED: OnceLock<AppliedGraphics> = OnceLock::new();
static DATA_DIR: OnceLock<PathBuf> = OnceLock::new();

/// Graphics decisions taken by `pre_init` (None before it ran).
pub fn applied_graphics() -> Option<&'static AppliedGraphics> {
    APPLIED.get()
}

/// Called on `app_ready` (and by `commands::app`): the launch succeeded, drop the crash guard.
pub fn launch_succeeded() {
    if let Some(dir) = DATA_DIR.get() {
        guard::clear(dir);
    }
}

fn has_safe_flag(args: &[String]) -> bool {
    args.iter().any(|a| a == "--safe-graphics")
}

/// Runs first in `main`, single-threaded. `args` excludes argv[0].
pub fn pre_init(args: &[String]) -> PreInit {
    let safe_flag = has_safe_flag(args);
    #[cfg(target_os = "linux")]
    {
        linux_pre_init(args, safe_flag)
    }
    #[cfg(not(target_os = "linux"))]
    {
        PreInit { safe_graphics: safe_flag }
    }
}

#[cfg(target_os = "linux")]
fn linux_pre_init(args: &[String], safe_flag: bool) -> PreInit {
    use kelta_proto::dirs::{CliArgs, Dirs, DirsOverrides};

    limit_malloc_arenas();

    let cli = CliArgs::parse(args);
    let dirs = Dirs::from_process_env(&DirsOverrides { config: cli.config_dir.clone(), ..Default::default() }).ok();

    let cfg = dirs.as_ref().map(|d| kelta_config::early::linux_graphics(&d.config)).unwrap_or_default();

    let mut guard_safe = false;
    let mut guard_tripped = false;
    if let Some(d) = &dirs {
        let _ = DATA_DIR.set(d.data.clone());
        let decision = guard::begin(&d.data);
        guard_safe = decision.safe;
        guard_tripped = decision.safe;
        if decision.hint {
            eprintln!(
                "kelta: the last launches did not finish. Running with the safe graphics profile.\n\
                 kelta: try `kelta --safe-graphics`, then Settings > Diagnostics (docs/user/graphics.md)."
            );
        }
    }

    let nvidia = graphics::detect_nvidia(std::path::Path::new("/"));
    let inputs = graphics::GraphicsInputs { cfg, safe_flag, guard_safe, nvidia };
    let safe = graphics::is_safe(&inputs);
    let set = graphics::apply(&graphics::decide(&inputs));
    let _ = APPLIED.set(AppliedGraphics {
        safe,
        nvidia,
        guard_tripped,
        vars: set.into_iter().map(|(k, v)| (k.to_owned(), v.to_owned())).collect(),
    });
    PreInit { safe_graphics: safe }
}

/// `mallopt(M_ARENA_MAX, 2)` on glibc: fewer malloc arenas, smaller RSS with few threads.
#[cfg(all(target_os = "linux", target_env = "gnu"))]
fn limit_malloc_arenas() {
    // SAFETY: mallopt only adjusts allocator tunables and is called before other threads exist.
    unsafe {
        libc::mallopt(libc::M_ARENA_MAX, 2);
    }
}

#[cfg(all(target_os = "linux", not(target_env = "gnu")))]
fn limit_malloc_arenas() {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_flag() {
        assert!(has_safe_flag(&["--safe-graphics".to_owned()]));
        assert!(!has_safe_flag(&["--other".to_owned()]));
    }
}

//! Filesystem locations (ARCHITECTURE §2.1) and CLI arguments shared by every crate.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::{KeltaError, Result};

/// Resolved directories.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Dirs {
    /// `~/.config/kelta` (honours `XDG_CONFIG_HOME` on both platforms).
    pub config: PathBuf,
    /// Linux `~/.local/share/kelta`; macOS `~/Library/Application Support/dev.kelta.Kelta`.
    pub data: PathBuf,
    /// Linux `$XDG_STATE_HOME/kelta`; macOS = data.
    pub state: PathBuf,
    /// Linux `<state>/logs`; macOS `~/Library/Logs/Kelta`.
    pub logs: PathBuf,
    /// `$XDG_RUNTIME_DIR/kelta` (fallback `/tmp/kelta-<uid>`); macOS `/tmp/kelta-<uid>`.
    pub runtime: PathBuf,
    /// `<data>/bin` (stable kelta-ctl copies).
    pub bin: PathBuf,
}

/// Explicit overrides (CLI `--config-dir`, tests).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DirsOverrides {
    pub config: Option<PathBuf>,
    pub data: Option<PathBuf>,
    pub runtime: Option<PathBuf>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Os {
    Macos,
    Linux,
}

impl Os {
    pub const fn current() -> Self {
        if cfg!(target_os = "macos") { Self::Macos } else { Self::Linux }
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Macos => "macos",
            Self::Linux => "linux",
        }
    }
}

/// App identifier.
pub const APP_ID: &str = "dev.kelta.Kelta";

impl Dirs {
    /// Resolve from an environment map and overrides for the current OS and uid.
    pub fn resolve(env: &BTreeMap<String, String>, overrides: &DirsOverrides) -> Result<Self> {
        let uid = rustix::process::getuid().as_raw();
        Self::resolve_for(Os::current(), env, uid, overrides)
    }

    /// Resolve from the process environment.
    pub fn from_process_env(overrides: &DirsOverrides) -> Result<Self> {
        let env: BTreeMap<String, String> = std::env::vars().collect();
        Self::resolve(&env, overrides)
    }

    /// Pure resolution (testable).
    pub fn resolve_for(
        os: Os,
        env: &BTreeMap<String, String>,
        uid: u32,
        overrides: &DirsOverrides,
    ) -> Result<Self> {
        let get = |k: &str| env.get(k).filter(|v| !v.is_empty()).map(PathBuf::from);
        let home = get("HOME").ok_or_else(|| KeltaError::internal("HOME is not set"))?;
        let xdg = |var: &str, default: &[&str]| -> PathBuf {
            get(var).unwrap_or_else(|| default.iter().fold(home.clone(), |p, s| p.join(s)))
        };

        let config =
            overrides.config.clone().unwrap_or_else(|| xdg("XDG_CONFIG_HOME", &[".config"]).join("kelta"));
        let tmp_runtime = PathBuf::from(format!("/tmp/kelta-{uid}"));
        let (data, state, logs, runtime) = match os {
            Os::Linux => {
                let data = xdg("XDG_DATA_HOME", &[".local", "share"]).join("kelta");
                let state = xdg("XDG_STATE_HOME", &[".local", "state"]).join("kelta");
                let logs = state.join("logs");
                let runtime = get("XDG_RUNTIME_DIR").map(|p| p.join("kelta")).unwrap_or(tmp_runtime);
                (data, state, logs, runtime)
            }
            Os::Macos => {
                let data = home.join("Library").join("Application Support").join(APP_ID);
                let logs = home.join("Library").join("Logs").join("Kelta");
                (data.clone(), data, logs, tmp_runtime)
            }
        };
        let data = overrides.data.clone().unwrap_or(data);
        let runtime = overrides.runtime.clone().unwrap_or(runtime);
        let bin = data.join("bin");
        Ok(Self { config, data, state, logs, runtime, bin })
    }

    /// Dirs rooted in one directory (tests).
    pub fn under(root: &Path) -> Self {
        Self {
            config: root.join("config"),
            data: root.join("data"),
            state: root.join("state"),
            logs: root.join("logs"),
            runtime: root.join("run"),
            bin: root.join("data").join("bin"),
        }
    }

    pub fn db_path(&self) -> PathBuf {
        self.data.join("kelta.db")
    }

    pub fn ctl_socket(&self) -> PathBuf {
        self.runtime.join(crate::ctl::CTL_SOCKET_NAME)
    }

    /// `<runtime>/s/<sid8>/`.
    pub fn session_runtime(&self, sid8: &str) -> PathBuf {
        self.runtime.join("s").join(sid8)
    }

    pub fn projects_dir(&self) -> PathBuf {
        self.config.join("projects")
    }

    pub fn global_config(&self) -> PathBuf {
        self.config.join("config.toml")
    }

    pub fn plugins_dir(&self) -> PathBuf {
        self.data.join("plugins")
    }

    /// `<data>/bin/current/kelta-ctl`.
    pub fn stable_ctl(&self) -> PathBuf {
        self.bin.join("current").join("kelta-ctl")
    }
}

/// Parsed command line of the `kelta` binary.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CliArgs {
    pub safe_graphics: bool,
    pub config_dir: Option<PathBuf>,
    /// `--set key=value` (repeatable).
    pub sets: Vec<String>,
    /// Positional paths to open as projects.
    pub open: Vec<PathBuf>,
}

impl CliArgs {
    /// Parse `kelta [--safe-graphics] [--config-dir DIR] [--set k=v]... [PATH]...` (argv[0] excluded).
    pub fn parse(args: &[String]) -> Self {
        let mut out = Self::default();
        let mut it = args.iter();
        while let Some(a) = it.next() {
            match a.as_str() {
                "--safe-graphics" => out.safe_graphics = true,
                "--config-dir" => out.config_dir = it.next().map(PathBuf::from),
                "--set" => {
                    if let Some(v) = it.next() {
                        out.sets.push(v.clone());
                    }
                }
                s if s.starts_with("--config-dir=") => {
                    out.config_dir = Some(PathBuf::from(&s["--config-dir=".len()..]));
                }
                s if s.starts_with("--set=") => out.sets.push(s["--set=".len()..].to_owned()),
                s if s.starts_with('-') => {}
                s => out.open.push(PathBuf::from(s)),
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
        pairs.iter().map(|(k, v)| ((*k).to_owned(), (*v).to_owned())).collect()
    }

    #[test]
    fn linux_and_macos_layouts() {
        let e = env(&[("HOME", "/home/u"), ("XDG_RUNTIME_DIR", "/run/user/1000")]);
        let d = Dirs::resolve_for(Os::Linux, &e, 1000, &DirsOverrides::default()).unwrap();
        assert_eq!(d.config, PathBuf::from("/home/u/.config/kelta"));
        assert_eq!(d.data, PathBuf::from("/home/u/.local/share/kelta"));
        assert_eq!(d.logs, PathBuf::from("/home/u/.local/state/kelta/logs"));
        assert_eq!(d.runtime, PathBuf::from("/run/user/1000/kelta"));

        let e = env(&[("HOME", "/Users/u")]);
        let d = Dirs::resolve_for(Os::Macos, &e, 501, &DirsOverrides::default()).unwrap();
        assert_eq!(d.data, PathBuf::from("/Users/u/Library/Application Support/dev.kelta.Kelta"));
        assert_eq!(d.runtime, PathBuf::from("/tmp/kelta-501"));
        assert_eq!(d.logs, PathBuf::from("/Users/u/Library/Logs/Kelta"));
    }

    #[test]
    fn cli_parse() {
        let a: Vec<String> = ["--safe-graphics", "--set", "app.theme=\"dark\"", "/tmp/x"]
            .iter()
            .map(|s| (*s).into())
            .collect();
        let c = CliArgs::parse(&a);
        assert!(c.safe_graphics);
        assert_eq!(c.sets, vec!["app.theme=\"dark\"".to_owned()]);
        assert_eq!(c.open, vec![PathBuf::from("/tmp/x")]);
    }
}

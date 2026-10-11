//! Terminal contract types (ARCHITECTURE §4, §6.1, §7.3, §7.4).

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::api::TerminalEvents;
use crate::ids::SessionId;
use crate::model::SessionKind;
use crate::settings::{KeyboardProtocol, ScrollbackSettings};

// ---- frames (§6.1) ------------------------------------------------------------------------------

/// Raw PTY bytes → `term.write(bytes, () => ack(n))`.
pub const FRAME_DATA: u8 = 0x01;
/// ANSI repaint (§9.3) → `term.reset()` then write, ack.
pub const FRAME_SNAPSHOT: u8 = 0x02;
/// `i32` LE exit code (`-1` = signal) → exit banner.
pub const FRAME_EXIT: u8 = 0x03;
/// `u8` kitty keyboard flags of the active screen; sent when they change and after a Snapshot
/// when non-zero (a Snapshot resets them to 0 in the view). Not acknowledged.
pub const FRAME_KEYBOARD: u8 = 0x04;

/// Flow control: stop sending Data frames above this many unacked bytes.
pub const HIGH_WATERMARK: u32 = 256 * 1024;
/// Resume (with a Snapshot) below this many unacked bytes.
pub const LOW_WATERMARK: u32 = 64 * 1024;
/// Ack watchdog deadline while `inflight > 0`.
pub const ACK_TIMEOUT_MS: u64 = 5_000;
/// Reader thread stack size.
pub const READER_STACK_SIZE: usize = 256 * 1024;
/// Reader buffer size.
pub const READ_CHUNK: usize = 64 * 1024;

/// Build a frame: tag byte followed by payload.
pub fn encode_frame(tag: u8, payload: &[u8]) -> Vec<u8> {
    let mut v = Vec::with_capacity(payload.len() + 1);
    v.push(tag);
    v.extend_from_slice(payload);
    v
}

/// Exit frame with a little-endian `i32` code.
pub fn exit_frame(code: i32) -> Vec<u8> {
    encode_frame(FRAME_EXIT, &code.to_le_bytes())
}

/// Split a frame into `(tag, payload)`.
pub fn decode_frame(frame: &[u8]) -> Option<(u8, &[u8])> {
    frame.split_first().map(|(t, p)| (*t, p))
}

// ---- query swallowing (§7.4) ----------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum QueryKind {
    Csi,
    Osc,
}

/// A terminal query answered by the Rust model, hence swallowed by xterm.js.
/// CSI entries map to `term.parser.registerCsiHandler({prefix, intermediates, final}, ...)`;
/// OSC entries to `registerOscHandler(ident, data => data === '?' ...)`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct TerminalQuery {
    pub name: &'static str,
    pub kind: QueryKind,
    pub prefix: Option<&'static str>,
    pub intermediates: Option<&'static str>,
    #[serde(rename = "final")]
    pub final_byte: Option<&'static str>,
    pub osc: Option<u16>,
    /// CSI: only swallow when the first param is one of these (empty = any).
    pub params: &'static [u16],
    /// OSC: only swallow the `?` query form.
    pub query_only: bool,
}

const fn csi(
    name: &'static str,
    prefix: Option<&'static str>,
    intermediates: Option<&'static str>,
    final_byte: &'static str,
    params: &'static [u16],
) -> TerminalQuery {
    TerminalQuery {
        name,
        kind: QueryKind::Csi,
        prefix,
        intermediates,
        final_byte: Some(final_byte),
        osc: None,
        params,
        query_only: false,
    }
}

const fn osc(name: &'static str, ident: u16) -> TerminalQuery {
    TerminalQuery {
        name,
        kind: QueryKind::Osc,
        prefix: None,
        intermediates: None,
        final_byte: None,
        osc: Some(ident),
        params: &[],
        query_only: true,
    }
}

/// Initial list (ARCHITECTURE §7.4). L1 records the exact answered set in
/// `docs/contracts/terminal-queries.md` and files a contract request if it differs.
pub const SWALLOWED_QUERIES: &[TerminalQuery] = &[
    csi("DA1", None, None, "c", &[]),
    csi("DA2", Some(">"), None, "c", &[]),
    csi("DSR", None, None, "n", &[5, 6]),
    csi("DECRQM", Some("?"), Some("$"), "p", &[]),
    csi("DECRQM_ANSI", None, Some("$"), "p", &[]),
    csi("XTWINOPS_CHARS", None, None, "t", &[18]),
    csi("KITTY_KEYBOARD", Some("?"), None, "u", &[]),
    osc("OSC4_PALETTE", 4),
    osc("OSC10_FG", 10),
    osc("OSC11_BG", 11),
    osc("OSC12_CURSOR", 12),
];

// ---- runtime types ----------------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum KillSignal {
    Hup,
    Term,
    Kill,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum ClipboardKind {
    #[default]
    Clipboard,
    Primary,
}

/// Events from a terminal session to core (`TerminalEvents::on_event`; keltad sends them as JSON).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", content = "data", rename_all = "snake_case")]
pub enum TerminalEvent {
    Title(String),
    /// OSC 7.
    Cwd(PathBuf),
    Bell,
    /// OSC 9 / 777.
    Notify {
        title: Option<String>,
        body: String,
    },
    ClipboardStore {
        kind: ClipboardKind,
        text: String,
    },
    ClipboardLoad {
        kind: ClipboardKind,
    },
    /// First output since `mark_seen`.
    Activity,
    Exited {
        code: Option<i32>,
        signal: Option<i32>,
    },
    AckTimeout {
        generation: u32,
    },
    /// The global scrollback memory cap (§9.5) trimmed sessions; emitted once per host, on one of
    /// the trimmed sessions.
    MemoryCapReached {
        cap_mb: u32,
    },
}

/// Colours used to answer OSC 4/10/11/12 queries (`#rrggbb`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct TerminalPalette {
    pub foreground: String,
    pub background: String,
    pub cursor: String,
    /// 16 ANSI colours (0-15).
    pub ansi: Vec<String>,
}

impl Default for TerminalPalette {
    fn default() -> Self {
        let ansi = [
            "#000000", "#cd3131", "#0dbc79", "#e5e510", "#2472c8", "#bc3fbc", "#11a8cd", "#e5e5e5",
            "#666666", "#f14c4c", "#23d18b", "#f5f543", "#3b8eea", "#d670d6", "#29b8db", "#ffffff",
        ];
        Self {
            foreground: "#d4d4d4".into(),
            background: "#1e1e1e".into(),
            cursor: "#d4d4d4".into(),
            ansi: ansi.iter().map(|s| (*s).to_owned()).collect(),
        }
    }
}

/// Scrollback per kind + global memory cap (§9.5), and the keyboard protocol of the models.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, TS)]
pub struct TerminalLimits {
    pub scrollback: ScrollbackSettings,
    pub memory_cap_mb: u32,
    /// History lines sent with a snapshot (`terminal.view_scrollback`; 0 = default 1000).
    pub view_scrollback: u32,
    pub keyboard_protocol: KeyboardProtocol,
    /// `terminal.osc52`: `read-write` makes the models answer OSC 52 queries (write is the default).
    pub osc52: crate::settings::Osc52,
    /// On-disk history log (`terminal.history_log`; ARCHITECTURE §9.6).
    pub history_log: bool,
    /// Per-session log cap in MiB (`terminal.history_log_mb`; 0 = default 16).
    pub history_log_mb: u32,
    /// Cap of all logs in MiB (`terminal.history_log_total_mb`; 0 = default 512).
    pub history_log_total_mb: u32,
}

impl TerminalLimits {
    pub fn from_settings(t: &crate::settings::TerminalSettings) -> Self {
        Self {
            scrollback: t.scrollback,
            memory_cap_mb: t.memory_cap_mb,
            view_scrollback: t.view_scrollback,
            keyboard_protocol: t.keyboard_protocol,
            osc52: t.osc52,
            history_log: t.history_log,
            history_log_mb: t.history_log_mb,
            history_log_total_mb: t.history_log_total_mb,
        }
    }
}

/// One matching line of the on-disk history log (`session_history_search`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct HistoryHit {
    pub session_id: SessionId,
    /// Plain text (ANSI stripped, wrapped rows joined).
    pub line: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct SessionTermStats {
    pub id: SessionId,
    pub bytes_in: u64,
    pub history_lines: u32,
    pub cols: u16,
    pub rows: u16,
    pub inflight: u32,
    pub attached: bool,
    /// Estimated model memory (`history_lines × cols × 24 B` + grid).
    pub memory_bytes: u64,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct TerminalStats {
    pub sessions: Vec<SessionTermStats>,
    pub total_memory_bytes: u64,
    pub reader_threads: u32,
}

/// Where the login environment came from (§7.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum LoginEnvSource {
    LoginInteractive,
    Login,
    PathHelper,
    #[default]
    Inherited,
}

/// Login environment resolved once per app start.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct LoginEnv {
    pub vars: BTreeMap<String, String>,
    pub source: LoginEnvSource,
    /// Shell used for the probe.
    pub shell: Option<PathBuf>,
}

impl LoginEnv {
    /// The inherited process environment without the AppImage runtime's variables
    /// ([`strip_appimage_env`]): what a child process of Kelta should start from.
    pub fn inherited() -> Self {
        let mut vars = std::env::vars().collect();
        strip_appimage_env(&mut vars);
        Self { vars, source: LoginEnvSource::Inherited, shell: std::env::var_os("SHELL").map(PathBuf::from) }
    }

    pub fn get(&self, key: &str) -> Option<&str> {
        self.vars.get(key).map(String::as_str)
    }

    pub fn path(&self) -> Option<&str> {
        self.get("PATH")
    }
}

/// Undo what an AppImage runtime and AppImageLauncher set up for Kelta itself (no-op without
/// `APPDIR`): a saved original (`APPIMAGE_ORIGINAL_<VAR>`, `<VAR>_ORIG`) is restored, entries naming
/// the mount or the `.AppImage` file leave path lists (`PATH`, `XDG_DATA_DIRS`, `LD_LIBRARY_PATH`,
/// ...), a variable left with nothing else is removed, and so are the launch-only ones (`APPDIR`,
/// `APPIMAGE`, `OWD`, `ARGV0`, `*_APPIMAGE`, `DESKTOPINTEGRATION`, `GIO_LAUNCHED_DESKTOP_FILE*`).
pub fn strip_appimage_env(vars: &mut BTreeMap<String, String>) {
    let Some(appdir) =
        vars.get("APPDIR").map(|d| d.trim_end_matches('/').to_owned()).filter(|d| !d.is_empty())
    else {
        return;
    };
    let appimage = vars.get("APPIMAGE").cloned().filter(|a| !a.is_empty());
    vars.retain(|k, _| {
        !(matches!(k.as_str(), "APPDIR" | "APPIMAGE" | "OWD" | "ARGV0" | "DESKTOPINTEGRATION")
            || k.ends_with("_APPIMAGE")
            || k.starts_with("GIO_LAUNCHED_DESKTOP_FILE"))
    });
    // `p` as a whole path in `v`: `/x/.mount_K` names `/x/.mount_K/usr`, not `/x/.mount_Kx`.
    let names = |p: &str, v: &str| {
        v.match_indices(p).any(|(i, _)| {
            v[i + p.len()..].chars().next().is_none_or(|c| !(c.is_alphanumeric() || "._-".contains(c)))
        })
    };
    let refers = |v: &str| names(&appdir, v) || appimage.as_deref().is_some_and(|a| names(a, v));
    let keys: Vec<String> = vars.keys().cloned().collect();
    for k in keys {
        let Some(v) = vars.get(&k).cloned() else { continue };
        let saved = format!("{k}_ORIG");
        let original = if let Some(name) = k.strip_prefix("APPIMAGE_ORIGINAL_") {
            let v = vars.remove(&k).unwrap_or_default();
            Some((name.to_owned(), v))
        } else if refers(&v) && vars.contains_key(&saved) {
            vars.remove(&saved).map(|v| (k.clone(), v))
        } else {
            None
        };
        match original {
            Some((name, v)) if v.is_empty() => {
                vars.remove(&name);
            }
            Some((name, v)) => {
                vars.insert(name, v);
            }
            None if refers(&v) => {
                let kept: Vec<&str> = v.split(':').filter(|e| !e.is_empty() && !refers(e)).collect();
                if kept.is_empty() {
                    vars.remove(&k);
                } else {
                    let kept = kept.join(":");
                    vars.insert(k, kept);
                }
            }
            None => {}
        }
    }
}

/// A child process of Kelta: starts from [`LoginEnv::inherited`] (what Kelta was launched with,
/// without the AppImage runtime's variables). Every spawn goes through here (clippy.toml).
#[allow(clippy::disallowed_methods)] // allowlisted: the one place a child process starts
pub fn command(program: impl AsRef<std::ffi::OsStr>) -> std::process::Command {
    let mut cmd = std::process::Command::new(program);
    cmd.env_clear().envs(LoginEnv::inherited().vars);
    cmd
}

/// [`command`] for async callers.
pub fn async_command(program: impl AsRef<std::ffi::OsStr>) -> tokio::process::Command {
    command(program).into()
}

/// Fully resolved spawn request handed to the `TerminalHost` by core.
#[derive(Clone)]
pub struct PtySpawnSpec {
    pub id: SessionId,
    /// Absolute, resolved.
    pub program: PathBuf,
    pub args: Vec<String>,
    pub cwd: PathBuf,
    /// Complete environment.
    pub env: BTreeMap<String, String>,
    pub cols: u16,
    pub rows: u16,
    pub scrollback_lines: u32,
    pub kind: SessionKind,
    pub events: Arc<dyn TerminalEvents>,
}

impl std::fmt::Debug for PtySpawnSpec {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PtySpawnSpec")
            .field("id", &self.id)
            .field("program", &self.program)
            .field("args", &self.args)
            .field("cwd", &self.cwd)
            .field("env_keys", &self.env.keys().collect::<Vec<_>>())
            .field("cols", &self.cols)
            .field("rows", &self.rows)
            .field("scrollback_lines", &self.scrollback_lines)
            .field("kind", &self.kind)
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frames() {
        let f = exit_frame(-1);
        let (tag, p) = decode_frame(&f).unwrap();
        assert_eq!(tag, FRAME_EXIT);
        assert_eq!(i32::from_le_bytes(p.try_into().unwrap()), -1);
        assert_eq!(encode_frame(FRAME_DATA, b"hi"), vec![1, b'h', b'i']);
    }

    #[test]
    fn swallowed_names_unique() {
        let mut n: Vec<_> = SWALLOWED_QUERIES.iter().map(|q| q.name).collect();
        n.sort_unstable();
        n.dedup();
        assert_eq!(n.len(), SWALLOWED_QUERIES.len());
    }

    #[test]
    fn appimage_runtime_env_is_stripped() {
        let m = "/tmp/user/1000/.mount_Kelta_ab12";
        let env = |pairs: &[(&str, String)]| -> BTreeMap<String, String> {
            pairs.iter().map(|(k, v)| ((*k).to_owned(), v.clone())).collect()
        };
        let mut vars = env(&[
            ("APPDIR", m.to_owned()),
            ("APPIMAGE", "/home/u/Kelta.AppImage".to_owned()),
            ("OWD", "/home/u".to_owned()),
            ("ARGV0", "Kelta".to_owned()),
            ("PYTHONHOME", format!("{m}/usr/")),
            ("PYTHONPATH", format!("{m}/usr/share/pyshared/:")),
            ("LD_LIBRARY_PATH", format!("{m}/usr/lib/:{m}/usr/lib/x86_64-linux-gnu/:/opt/lib")),
            ("PATH", format!("/home/u/bin:{m}/usr/bin/:/usr/bin:{m}/usr/sbin/")),
            ("XDG_DATA_DIRS", format!("{m}/usr/share/:/usr/share")),
            ("GTK_PATH", format!("{m}/usr/lib/gtk-3.0")),
            ("GSETTINGS_SCHEMA_DIR", format!("{m}/usr/share/glib-2.0/schemas")),
            ("GDK_BACKEND", "x11".to_owned()),
            ("APPIMAGE_ORIGINAL_GDK_BACKEND", "wayland".to_owned()),
            ("APPIMAGE_ORIGINAL_GTK_THEME", String::new()),
            ("GTK_THEME", "Adwaita".to_owned()),
            ("QT_PLUGIN_PATH", format!("{m}/usr/lib/qt5/plugins")),
            ("QT_PLUGIN_PATH_ORIG", "/usr/lib/qt".to_owned()),
            ("HOME", "/home/u".to_owned()),
            ("MOUNTS", format!("{m}x/not-this-mount")),
            ("SOME_FUTURE_RUNTIME_DIR", format!("--dir={m}/usr/share/x")),
            ("PERLLIB", format!("{m}/usr/share/perl5/:{m}/usr/lib/perl5/")),
            ("REDIRECT_APPIMAGE", "1".to_owned()),
            ("TARGET_APPIMAGE", "/home/u/Kelta.AppImage".to_owned()),
            ("DESKTOPINTEGRATION", "AppImageLauncher".to_owned()),
            (
                "GIO_LAUNCHED_DESKTOP_FILE",
                "/home/u/.local/share/applications/appimagekit_1-Kelta.desktop".to_owned(),
            ),
            ("GIO_LAUNCHED_DESKTOP_FILE_PID", "42".to_owned()),
            ("LAUNCHER_ARG", "/home/u/Kelta.AppImage --x".to_owned()),
            ("APPIMAGE_EXTRACT_AND_RUN", "1".to_owned()),
        ]);
        strip_appimage_env(&mut vars);
        let expected = env(&[
            ("LD_LIBRARY_PATH", "/opt/lib".to_owned()),
            ("PATH", "/home/u/bin:/usr/bin".to_owned()),
            ("XDG_DATA_DIRS", "/usr/share".to_owned()),
            ("GDK_BACKEND", "wayland".to_owned()),
            ("QT_PLUGIN_PATH", "/usr/lib/qt".to_owned()),
            ("HOME", "/home/u".to_owned()),
            ("MOUNTS", format!("{m}x/not-this-mount")),
            ("APPIMAGE_EXTRACT_AND_RUN", "1".to_owned()),
        ]);
        assert_eq!(vars, expected);

        // Not an AppImage: untouched.
        let plain = env(&[("PATH", "/usr/bin".to_owned()), ("PYTHONHOME", "/opt/py".to_owned())]);
        let mut vars = plain.clone();
        strip_appimage_env(&mut vars);
        assert_eq!(vars, plain);
    }

    #[test]
    fn command_child_gets_no_appimage_env() {
        const NAME: &str = "term::tests::command_child_gets_no_appimage_env";
        // Inner run, re-executed below as a fake AppImage launch: print what its child sees.
        if std::env::var_os("KELTA_TEST_FAKE_LAUNCH").is_some() {
            let out = command("env").output().unwrap();
            std::io::Write::write_all(&mut std::io::stdout(), &out.stdout).unwrap();
            return;
        }
        let m = "/tmp/.mount_KeltaTest";
        let path = std::env::var("PATH").unwrap_or_default();
        let out = command(std::env::current_exe().unwrap())
            .args(["--exact", NAME, "--test-threads=1"])
            .envs([
                ("KELTA_TEST_FAKE_LAUNCH", "1".to_owned()),
                ("APPDIR", m.to_owned()),
                ("APPIMAGE", "/home/u/Kelta.AppImage".to_owned()),
                ("ARGV0", "Kelta".to_owned()),
                ("OWD", "/home/u".to_owned()),
                ("TARGET_APPIMAGE", "/home/u/Kelta.AppImage".to_owned()),
                ("DESKTOPINTEGRATION", "AppImageLauncher".to_owned()),
                ("PATH", format!("{m}/usr/bin:{path}")),
                ("LD_LIBRARY_PATH", format!("{m}/usr/lib")),
                ("UNKNOWN_FUTURE_VAR", format!("{m}/usr/share/thing")),
            ])
            .output()
            .unwrap();
        let env = String::from_utf8_lossy(&out.stdout);
        assert!(
            env.contains("KELTA_TEST_FAKE_LAUNCH=1"),
            "the inner run did not print its child's env: {env}"
        );
        assert!(env.lines().any(|l| l == format!("PATH={path}")), "{env}");
        assert!(!env.contains(m) && !env.contains(".AppImage"), "{env}");
        for k in ["APPDIR=", "APPIMAGE=", "ARGV0=", "OWD=", "DESKTOPINTEGRATION=", "LD_LIBRARY_PATH="] {
            assert!(!env.lines().any(|l| l.starts_with(k)), "{k} reached the child: {env}");
        }
    }
}

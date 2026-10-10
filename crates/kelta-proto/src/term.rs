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

/// Events from a terminal session to core (`TerminalEvents::on_event`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
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
}

impl TerminalLimits {
    pub fn from_settings(t: &crate::settings::TerminalSettings) -> Self {
        Self {
            scrollback: t.scrollback,
            memory_cap_mb: t.memory_cap_mb,
            view_scrollback: t.view_scrollback,
            keyboard_protocol: t.keyboard_protocol,
        }
    }
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
    /// The inherited process environment (stub / fallback).
    pub fn inherited() -> Self {
        Self {
            vars: std::env::vars().collect(),
            source: LoginEnvSource::Inherited,
            shell: std::env::var_os("SHELL").map(PathBuf::from),
        }
    }

    pub fn get(&self, key: &str) -> Option<&str> {
        self.vars.get(key).map(String::as_str)
    }

    pub fn path(&self) -> Option<&str> {
        self.get("PATH")
    }
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
}

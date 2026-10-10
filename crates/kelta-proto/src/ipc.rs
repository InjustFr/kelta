//! IPC return DTOs not owned by a more specific module (ARCHITECTURE §6).
//! Command argument objects are the Tauri command parameters themselves (snake_case keys).

use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::error::KeltaError;
use crate::ids::SessionId;
use crate::tracker::User;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct ToolVersion {
    pub path: PathBuf,
    pub version: String,
    /// Version ≥ the configured minimum (e.g. `claude.min_version`).
    pub ok: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct AppInfo {
    pub version: String,
    /// `macos` | `linux`.
    pub platform: String,
    /// `aarch64` | `x86_64`.
    pub arch: String,
    pub data_dir: PathBuf,
    pub config_dir: PathBuf,
    pub runtime_dir: PathBuf,
    pub claude: Option<ToolVersion>,
    pub safe_graphics: bool,
    /// Effective window decorations (`native` | `none` | `custom`; the desktop resolves `auto`).
    /// `custom` = the UI draws the drag region and resize handles.
    pub decorations: crate::settings::Decorations,
}

/// Update channel, from the running version: `-nightly.` → nightly, else release.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum UpdateChannel {
    Nightly,
    Release,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum UpdateState {
    Idle,
    Checking,
    Downloading,
    /// Installed; the next start runs it.
    Ready {
        version: String,
    },
    UpToDate,
    Failed {
        message: String,
    },
    /// This build never updates itself (dev build, package manager install, app not in a writable place).
    Unsupported {
        reason: String,
    },
}

/// `app_update_status` / `app_update_check` result.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct UpdateStatus {
    /// Running version.
    pub current: String,
    pub channel: UpdateChannel,
    pub state: UpdateState,
    /// End of the last check, RFC 3339.
    pub checked_at: Option<String>,
    /// A restart would stop a working Claude session (same rule as the quit confirmation).
    pub restart_needs_confirm: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum ProcRole {
    Core,
    WebContent,
    Network,
    Gpu,
    Child,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct ProcMem {
    pub pid: u32,
    pub name: String,
    pub role: ProcRole,
    /// Linux PSS / macOS phys_footprint, in KiB.
    pub pss_or_footprint_kb: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct SessionMem {
    pub id: SessionId,
    pub name: String,
    pub history_lines: u32,
    pub model_bytes: u64,
    /// Child process footprint (reported separately), KiB.
    pub child_kb: Option<u64>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct PerfSnapshot {
    pub processes: Vec<ProcMem>,
    pub sessions: Vec<SessionMem>,
    pub live_views: u32,
    pub timers_armed: u32,
    pub http_server: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum CheckStatus {
    Ok,
    Warn,
    Fail,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct Check {
    pub id: String,
    pub label: String,
    pub status: CheckStatus,
    pub detail: String,
    pub fix: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct Diagnostics {
    pub checks: Vec<Check>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct WindowState {
    pub exists: bool,
    pub visible: bool,
    pub focused: bool,
}

/// `events_subscribe` result.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct SubscribeResult {
    pub sub_id: u64,
}

/// `layout_save` result.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct LayoutSaveResult {
    pub rev: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
pub struct AccountTestResult {
    pub ok: bool,
    pub user: Option<User>,
    pub error: Option<KeltaError>,
}

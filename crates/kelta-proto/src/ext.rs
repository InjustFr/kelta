//! Extensibility types (PLUGINS.md): tools, triggers, actions, commands, plugin manifests,
//! permissions and the plugin host API methods. The same schema is used in config files,
//! project files, repo-local config and plugin manifests.

use std::collections::BTreeMap;
use std::path::PathBuf;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::events::ToastLevel;
use crate::ids::{PluginId, ScreenInstanceId, SessionId, ToolId, ToolInstanceId};
use crate::model::{Attention, CloseOnExit, Placement};
use crate::secret::SecretRef;
use crate::settings::{Layer, SessionTemplate};
use crate::tracker::StatusCategory;

/// Chord string like `ctrl+shift+k`, `cmd+opt+left`, `mod+t`.
pub type Chord = String;

// ---------------------------------------------------------------------------------------------
// Tools (§2)
// ---------------------------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, TS, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ToolKind {
    #[default]
    Pty,
    Web,
    /// Launched detached beside Kelta (GUI apps, `open -a Fork .`); never tracked or killed.
    External,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, TS, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum EmbedMode {
    #[default]
    Auto,
    Iframe,
    Proxy,
    External,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, TS, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum WebLifecycle {
    #[default]
    OnClose,
    OnProjectClose,
    Never,
}

/// Readiness detection: `{ stdout_json = "url" }` | `{ stdout_regex = "http://\\S+" }` | `{ port_open = true }`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Ready {
    StdoutJson(String),
    StdoutRegex(String),
    PortOpen(bool),
}

impl Default for Ready {
    fn default() -> Self {
        Self::PortOpen(true)
    }
}

/// `{ signal = "TERM", grace_ms = 3000 }` | `{ command = [...] }`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS, JsonSchema)]
#[serde(untagged)]
pub enum StopSpec {
    Signal {
        signal: String,
        #[serde(default = "default_grace_ms")]
        grace_ms: u64,
    },
    Command {
        command: Vec<String>,
    },
}

fn default_grace_ms() -> u64 {
    3000
}

impl Default for StopSpec {
    fn default() -> Self {
        Self::Signal { signal: "TERM".into(), grace_ms: 3000 }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct WebStart {
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub cwd: Option<String>,
    #[serde(default)]
    pub env: BTreeMap<String, String>,
    #[serde(default)]
    pub ready: Ready,
    #[serde(default = "default_ready_timeout_ms")]
    pub ready_timeout_ms: u64,
    #[serde(default)]
    pub stop: StopSpec,
}

fn default_ready_timeout_ms() -> u64 {
    10_000
}

/// A declarative tool. Every field except `id` has a default so `{ id = "x", enabled = false }`
/// disables an inherited entry (by_id merge).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct ToolDef {
    /// `[a-z0-9-]`; plugin tools are namespaced `<plugin>/<id>` by the host.
    pub id: String,
    pub label: String,
    pub icon: Option<String>,
    pub description: Option<String>,
    pub kind: ToolKind,
    // pty and external
    pub command: Option<String>,
    pub args: Vec<String>,
    pub cwd: Option<String>,
    pub env: BTreeMap<String, String>,
    pub close_on_exit: CloseOnExit,
    pub scrollback: Option<u32>,
    // web
    pub url: Option<String>,
    pub start: Option<WebStart>,
    /// `None` = `web.embed_default`.
    pub embed: Option<EmbedMode>,
    /// `None` = true when `start` is set.
    pub url_is_secret: Option<bool>,
    pub lifecycle: WebLifecycle,
    /// `None` = `web.keep_alive`.
    pub keep_alive: Option<bool>,
    // common
    pub check: Option<Vec<String>>,
    pub install_hint: Option<String>,
    pub placement: Placement,
    pub keybinding: Option<Chord>,
    pub autostart: bool,
    pub enabled: bool,
}

impl Default for ToolDef {
    fn default() -> Self {
        Self {
            id: String::new(),
            label: String::new(),
            icon: None,
            description: None,
            kind: ToolKind::Pty,
            command: None,
            args: Vec::new(),
            cwd: None,
            env: BTreeMap::new(),
            close_on_exit: CloseOnExit::Never,
            scrollback: None,
            url: None,
            start: None,
            embed: None,
            url_is_secret: None,
            lifecycle: WebLifecycle::OnClose,
            keep_alive: None,
            check: None,
            install_hint: None,
            placement: Placement::SplitRight,
            keybinding: None,
            autostart: false,
            enabled: true,
        }
    }
}

/// Where a tool comes from (`tool_list`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ToolSource {
    Layer { layer: Layer },
    Plugin { plugin_id: PluginId },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct ToolInfo {
    pub id: ToolId,
    pub label: String,
    pub icon: Option<String>,
    pub kind: ToolKind,
    /// `None` = not checked yet.
    pub installed: Option<bool>,
    pub source: ToolSource,
    pub keybinding: Option<Chord>,
    pub description: Option<String>,
    pub placement: Placement,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct ToolCheck {
    pub installed: bool,
    pub version: Option<String>,
    pub install_hint: Option<String>,
}

/// `{"kind":"pty","session_id":..}` | `{"kind":"web","instance_id":..,"url":..,"embed":..}` | `{"kind":"external"}`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ToolHandle {
    Pty { session_id: SessionId },
    Web { instance_id: ToolInstanceId, url: String, embed: EmbedMode },
    External,
}

// ---------------------------------------------------------------------------------------------
// Triggers (§3)
// ---------------------------------------------------------------------------------------------

/// Matcher: a string (exact; `glob:…`; `re:…`; `!…` negation), bool, number, or any-of list.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS, JsonSchema)]
#[serde(untagged)]
pub enum Matcher {
    Str(String),
    Bool(bool),
    Num(f64),
    AnyOf(Vec<Matcher>),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct TriggerDef {
    pub id: String,
    pub description: Option<String>,
    /// Event name or glob (`"pr.*"`, `"custom.*"`).
    pub on: String,
    /// Dotted path into `{event, payload, project, session, ticket, pr, app}` → matcher.
    #[serde(rename = "match")]
    pub r#match: BTreeMap<String, Matcher>,
    #[serde(rename = "do")]
    pub r#do: Vec<ActionDef>,
    /// Only honoured on `*.before_*` events (30 s cap).
    pub blocking: bool,
    pub debounce_ms: Option<u64>,
    pub continue_on_error: bool,
    pub allow_send_keys: bool,
    pub enabled: bool,
}

impl Default for TriggerDef {
    fn default() -> Self {
        Self {
            id: String::new(),
            description: None,
            on: String::new(),
            r#match: BTreeMap::new(),
            r#do: Vec::new(),
            blocking: false,
            debounce_ms: None,
            continue_on_error: false,
            allow_send_keys: false,
            enabled: true,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Urgency {
    Low,
    Normal,
    Critical,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum RunStdin {
    Event,
    None,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum RunShow {
    None,
    ToastOnError,
    Pane,
}

/// Trigger/command action; serde tag `action` (PLUGINS §3.1).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS, JsonSchema)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum ActionDef {
    Notify {
        title: String,
        #[serde(default)]
        body: Option<String>,
        #[serde(default)]
        urgency: Option<Urgency>,
    },
    Toast {
        text: String,
        #[serde(default)]
        level: Option<ToastLevel>,
    },
    Run {
        command: String,
        #[serde(default)]
        args: Vec<String>,
        #[serde(default)]
        cwd: Option<String>,
        #[serde(default)]
        env: BTreeMap<String, String>,
        #[serde(default)]
        stdin: Option<RunStdin>,
        #[serde(default)]
        timeout_ms: Option<u64>,
        #[serde(default)]
        show: Option<RunShow>,
    },
    SpawnSession {
        #[serde(default)]
        template: Option<String>,
        #[serde(default)]
        command: Option<String>,
        #[serde(default)]
        args: Vec<String>,
        #[serde(default)]
        cwd: Option<String>,
        #[serde(default)]
        placement: Option<Placement>,
        #[serde(default)]
        focus: bool,
    },
    SendKeys {
        /// `"{event.session_id}"` | `"claude"` | `"editor"`.
        session: String,
        text: String,
        #[serde(default)]
        bracketed: Option<bool>,
    },
    OpenTool {
        tool: String,
        #[serde(default)]
        placement: Option<Placement>,
    },
    OpenScreen {
        #[serde(default)]
        plugin: Option<String>,
        screen: String,
        #[serde(default)]
        params: Option<serde_json::Value>,
        #[serde(default)]
        placement: Option<Placement>,
    },
    StartWork {
        ticket: String,
        #[serde(default)]
        project: Option<String>,
    },
    TransitionTicket {
        #[serde(default)]
        to_category: Option<StatusCategory>,
        #[serde(default)]
        to_name: Option<String>,
    },
    CommentTicket {
        body: String,
    },
    AssignTicket {
        /// `"me"` | `"none"`.
        to: String,
    },
    Http {
        url: String,
        #[serde(default)]
        method: Option<String>,
        #[serde(default)]
        headers: BTreeMap<String, String>,
        #[serde(default)]
        body: Option<String>,
        #[serde(default)]
        secret_headers: BTreeMap<String, SecretRef>,
        #[serde(default)]
        timeout_ms: Option<u64>,
    },
    Focus {
        #[serde(default)]
        project: Option<String>,
        #[serde(default)]
        session: Option<String>,
    },
    SetAttention {
        session: String,
        level: Attention,
    },
    Prompt {
        text: String,
        yes: Vec<ActionDef>,
        #[serde(default)]
        no: Vec<ActionDef>,
    },
    Command {
        id: String,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum CommandWhen {
    Terminal,
    Ticket,
    Review,
    Always,
}

/// Palette command (config `[[commands]]` and plugin `contributes.commands`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct CommandDef {
    pub id: String,
    pub title: String,
    #[serde(rename = "do")]
    pub r#do: Vec<ActionDef>,
    pub when: Option<CommandWhen>,
    pub keybinding: Option<Chord>,
    pub enabled: bool,
}

impl Default for CommandDef {
    fn default() -> Self {
        Self {
            id: String::new(),
            title: String::new(),
            r#do: Vec::new(),
            when: None,
            keybinding: None,
            enabled: true,
        }
    }
}

/// Origin of a trigger (`trigger_list`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TriggerOrigin {
    Global,
    Project { project_id: crate::ids::ProjectId },
    Repo { project_id: crate::ids::ProjectId, repo_id: String, trusted: bool },
    Plugin { plugin_id: PluginId },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct TriggerInfo {
    pub id: String,
    pub origin: TriggerOrigin,
    pub on: String,
    pub enabled: bool,
    pub description: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct TriggerRun {
    pub ts: String,
    pub trigger_id: String,
    pub event: String,
    pub ok: bool,
    pub detail: Option<String>,
    pub depth: u8,
}

/// Result of `PluginHost::run_blocking` for `*.before_*` events.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum BlockingOutcome {
    Proceed {
        /// v0.1 patchable keys: `branch`, `template_id`, `claude.prompt`.
        patch: Option<serde_json::Value>,
    },
    Veto {
        trigger_id: String,
        reason: String,
    },
}

// ---------------------------------------------------------------------------------------------
// Plugin manifest (§4)
// ---------------------------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum PlatformName {
    Macos,
    Linux,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, TS, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ScreenScope {
    #[default]
    Project,
    Global,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ScreenPlacement {
    Tab,
    Pane,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ScreenDef {
    pub id: String,
    pub title: String,
    #[serde(default)]
    pub icon: Option<String>,
    /// Path inside the plugin dir, e.g. `dist/index.html`.
    pub entry: String,
    #[serde(default)]
    pub scope: ScreenScope,
    #[serde(default = "default_screen_placement")]
    pub placement: Vec<ScreenPlacement>,
    #[serde(default)]
    pub keep_alive: bool,
    #[serde(default)]
    pub min_width: Option<u32>,
}

fn default_screen_placement() -> Vec<ScreenPlacement> {
    vec![ScreenPlacement::Tab, ScreenPlacement::Pane]
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct KeybindingDef {
    pub command: String,
    pub key: Chord,
}

/// Button on ticket/review detail panes.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ActionButtonDef {
    pub id: String,
    pub title: String,
    #[serde(rename = "do")]
    pub r#do: Vec<ActionDef>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SettingsContribution {
    /// Relative path of a flat JSON Schema mounted at `plugins.<id>`.
    pub schema: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, TS, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct Contributes {
    pub tools: Vec<ToolDef>,
    pub triggers: Vec<TriggerDef>,
    pub commands: Vec<CommandDef>,
    pub keybindings: Vec<KeybindingDef>,
    pub session_templates: Vec<SessionTemplate>,
    pub screens: Vec<ScreenDef>,
    pub ticket_actions: Vec<ActionButtonDef>,
    pub review_actions: Vec<ActionButtonDef>,
    pub settings: Option<SettingsContribution>,
}

/// `kelta-plugin.toml` (or `.json`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PluginManifest {
    /// `[a-z0-9-]{3,40}`, not starting with `kelta`.
    pub id: PluginId,
    pub name: String,
    /// Semver.
    pub version: String,
    pub description: String,
    pub author: String,
    pub license: String,
    #[serde(default)]
    pub homepage: Option<String>,
    /// Semver requirement against host API `0.1.0`.
    pub kelta_api: String,
    #[serde(default)]
    pub platforms: Vec<PlatformName>,
    /// Permission strings (see [`Permission`]).
    #[serde(default)]
    pub permissions: Vec<String>,
    /// `onStartup` | `onCommand:<id>` | `onScreen:<id>` | `onEvent:<glob>` | `onProjectOpen`.
    #[serde(default)]
    pub activation: Vec<String>,
    #[serde(default)]
    pub contributes: Contributes,
    /// A process (KPP) provider: tracker or code-host accounts served over JSON-RPC on stdio.
    #[serde(default)]
    pub provider: Option<ProviderDef>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ProviderKind {
    Tracker,
    Codehost,
}

/// `[provider]` (PLUGINS §9). The sync trait methods are answered from these templates, without a
/// round trip: `{base_url}`, `{web_url}` (falls back to `base_url`), `{key}`, `{id}` (tickets),
/// `{repo}`, `{number}`, `{branch}` (reviews).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ProviderDef {
    pub kind: ProviderKind,
    /// Program to run; a path with `/` is relative to the plugin dir. Runs in the plugin dir.
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
    /// Per-call timeout (default 30000).
    #[serde(default)]
    pub timeout_ms: Option<u64>,
    /// Tracker capabilities.
    #[serde(default)]
    pub caps: crate::tracker::TrackerCaps,
    /// Tracker `browser_url` (default `{web_url}/{key}`).
    #[serde(default)]
    pub browser_url: Option<String>,
    /// Tracker `branch_key` (default `{key}`).
    #[serde(default)]
    pub branch_key: Option<String>,
    /// Code host `fetch_refspec` (default `pull/{number}/head:{branch}`).
    #[serde(default)]
    pub fetch_refspec: Option<String>,
}

/// Host plugin API version.
pub const KELTA_API_VERSION: &str = "0.1.0";

// ---------------------------------------------------------------------------------------------
// Permissions (§5)
// ---------------------------------------------------------------------------------------------

/// A plugin permission. Serialized as its string form (`"tickets.read"`, `"exec:kubectl"`, `"net:*.acme.com"`).
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, TS)]
#[serde(try_from = "String", into = "String")]
#[ts(type = "string")]
pub enum Permission {
    ProjectsRead,
    TicketsRead,
    TicketsWrite,
    PrsRead,
    PrsWrite,
    SessionsRead,
    SessionsSpawn,
    SessionsWrite,
    TerminalWrite,
    /// `events:<glob>`
    Events(String),
    SettingsRead,
    UiOpen,
    Notify,
    ClipboardWrite,
    /// Own key-value store (`kv.*`).
    Storage,
    /// `exec:<command>` (argv[0] basename)
    Exec(String),
    /// `net:<host>` (exact or `*.domain`)
    Net(String),
    /// Serve tracker / code-host accounts through `[provider]` (KPP).
    Provider,
}

impl Permission {
    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "projects.read" => Self::ProjectsRead,
            "tickets.read" => Self::TicketsRead,
            "tickets.write" => Self::TicketsWrite,
            "prs.read" => Self::PrsRead,
            "prs.write" => Self::PrsWrite,
            "sessions.read" => Self::SessionsRead,
            "sessions.spawn" => Self::SessionsSpawn,
            "sessions.write" => Self::SessionsWrite,
            "terminal.write" => Self::TerminalWrite,
            "settings.read" => Self::SettingsRead,
            "ui.open" => Self::UiOpen,
            "notify" => Self::Notify,
            "clipboard.write" => Self::ClipboardWrite,
            "storage" => Self::Storage,
            "provider" => Self::Provider,
            other => {
                let (kind, arg) = other.split_once(':')?;
                if arg.is_empty() {
                    return None;
                }
                match kind {
                    "events" => Self::Events(arg.to_owned()),
                    "exec" => Self::Exec(arg.to_owned()),
                    "net" => Self::Net(arg.to_owned()),
                    _ => return None,
                }
            }
        })
    }

    /// Plain-language description for the install dialog.
    pub fn describe(&self) -> String {
        match self {
            Self::ProjectsRead => "See your projects".into(),
            Self::TicketsRead => "Read tickets".into(),
            Self::TicketsWrite => "Change tickets (move, comment, assign)".into(),
            Self::PrsRead => "Read pull/merge requests".into(),
            Self::PrsWrite => "Approve and comment on pull/merge requests".into(),
            Self::SessionsRead => "See your terminal sessions".into(),
            Self::SessionsSpawn => "Open terminal sessions and tools".into(),
            Self::SessionsWrite => "Change session attention state".into(),
            Self::TerminalWrite => "Type text into your terminal sessions".into(),
            Self::Events(g) => format!("Receive app events matching `{g}`"),
            Self::SettingsRead => "Read your (non-secret) settings".into(),
            Self::UiOpen => "Open panes and screens, change focus".into(),
            Self::Notify => "Show desktop notifications".into(),
            Self::ClipboardWrite => "Write to the clipboard".into(),
            Self::Storage => "Store its own data in Kelta (up to 1 MiB)".into(),
            Self::Provider => {
                "Run its provider program for the tracker / code-host accounts you point at it, with their settings and secrets".into()
            }
            Self::Exec(c) => format!("Run the program `{c}`"),
            Self::Net(h) => format!("Make network requests to {h}"),
        }
    }
}

impl std::fmt::Display for Permission {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ProjectsRead => f.write_str("projects.read"),
            Self::TicketsRead => f.write_str("tickets.read"),
            Self::TicketsWrite => f.write_str("tickets.write"),
            Self::PrsRead => f.write_str("prs.read"),
            Self::PrsWrite => f.write_str("prs.write"),
            Self::SessionsRead => f.write_str("sessions.read"),
            Self::SessionsSpawn => f.write_str("sessions.spawn"),
            Self::SessionsWrite => f.write_str("sessions.write"),
            Self::TerminalWrite => f.write_str("terminal.write"),
            Self::Events(g) => write!(f, "events:{g}"),
            Self::SettingsRead => f.write_str("settings.read"),
            Self::UiOpen => f.write_str("ui.open"),
            Self::Notify => f.write_str("notify"),
            Self::ClipboardWrite => f.write_str("clipboard.write"),
            Self::Storage => f.write_str("storage"),
            Self::Provider => f.write_str("provider"),
            Self::Exec(c) => write!(f, "exec:{c}"),
            Self::Net(h) => write!(f, "net:{h}"),
        }
    }
}

impl TryFrom<String> for Permission {
    type Error = String;
    fn try_from(s: String) -> Result<Self, Self::Error> {
        Self::parse(&s).ok_or_else(|| format!("unknown permission `{s}`"))
    }
}

impl From<Permission> for String {
    fn from(p: Permission) -> Self {
        p.to_string()
    }
}

// ---------------------------------------------------------------------------------------------
// Host API methods (§7)
// ---------------------------------------------------------------------------------------------

/// Methods callable by plugin screens through `plugin_call`. Serialized as `"tickets.list"` etc.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
pub enum PluginMethod {
    #[serde(rename = "app.info")]
    AppInfo,
    #[serde(rename = "projects.current")]
    ProjectsCurrent,
    #[serde(rename = "projects.list")]
    ProjectsList,
    #[serde(rename = "tickets.list")]
    TicketsList,
    #[serde(rename = "tickets.get")]
    TicketsGet,
    #[serde(rename = "tickets.transitions")]
    TicketsTransitions,
    #[serde(rename = "tickets.columns")]
    TicketsColumns,
    #[serde(rename = "tickets.transition")]
    TicketsTransition,
    #[serde(rename = "tickets.comment")]
    TicketsComment,
    #[serde(rename = "tickets.assign")]
    TicketsAssign,
    #[serde(rename = "reviews.list")]
    ReviewsList,
    #[serde(rename = "reviews.get")]
    ReviewsGet,
    #[serde(rename = "reviews.approve")]
    ReviewsApprove,
    #[serde(rename = "reviews.comment")]
    ReviewsComment,
    #[serde(rename = "reviews.request_changes")]
    ReviewsRequestChanges,
    #[serde(rename = "sessions.list")]
    SessionsList,
    #[serde(rename = "sessions.spawn")]
    SessionsSpawn,
    #[serde(rename = "sessions.send_text")]
    SessionsSendText,
    #[serde(rename = "tools.open")]
    ToolsOpen,
    #[serde(rename = "events.subscribe")]
    EventsSubscribe,
    #[serde(rename = "events.unsubscribe")]
    EventsUnsubscribe,
    #[serde(rename = "settings.get")]
    SettingsGet,
    #[serde(rename = "settings.set")]
    SettingsSet,
    #[serde(rename = "net.fetch")]
    NetFetch,
    #[serde(rename = "ui.toast")]
    UiToast,
    #[serde(rename = "ui.open_screen")]
    UiOpenScreen,
    #[serde(rename = "ui.focus")]
    UiFocus,
    #[serde(rename = "notify.send")]
    NotifySend,
    #[serde(rename = "clipboard.write")]
    ClipboardWrite,
    #[serde(rename = "kv.get")]
    KvGet,
    #[serde(rename = "kv.set")]
    KvSet,
    #[serde(rename = "kv.delete")]
    KvDelete,
    #[serde(rename = "kv.list")]
    KvList,
}

/// Permission requirement of a [`PluginMethod`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MethodPermission {
    /// No permission needed.
    None,
    /// A fixed permission.
    Static(Permission),
    /// Depends on params: `events:<glob>` (subscribe), `net:<host>` (fetch),
    /// `exec:<cmd>` in addition to `sessions.spawn` (spawn with command), `settings.read` (get, optional).
    Dynamic,
}

impl PluginMethod {
    pub const ALL: &'static [PluginMethod] = &[
        Self::AppInfo,
        Self::ProjectsCurrent,
        Self::ProjectsList,
        Self::TicketsList,
        Self::TicketsGet,
        Self::TicketsTransitions,
        Self::TicketsColumns,
        Self::TicketsTransition,
        Self::TicketsComment,
        Self::TicketsAssign,
        Self::ReviewsList,
        Self::ReviewsGet,
        Self::ReviewsApprove,
        Self::ReviewsComment,
        Self::ReviewsRequestChanges,
        Self::SessionsList,
        Self::SessionsSpawn,
        Self::SessionsSendText,
        Self::ToolsOpen,
        Self::EventsSubscribe,
        Self::EventsUnsubscribe,
        Self::SettingsGet,
        Self::SettingsSet,
        Self::NetFetch,
        Self::UiToast,
        Self::UiOpenScreen,
        Self::UiFocus,
        Self::NotifySend,
        Self::ClipboardWrite,
        Self::KvGet,
        Self::KvSet,
        Self::KvDelete,
        Self::KvList,
    ];

    pub fn required_permission(self) -> MethodPermission {
        use MethodPermission::{Dynamic, None as Free, Static};
        match self {
            Self::AppInfo | Self::UiToast | Self::SettingsSet => Free,
            Self::ProjectsCurrent | Self::ProjectsList => Static(Permission::ProjectsRead),
            Self::TicketsList | Self::TicketsGet | Self::TicketsTransitions | Self::TicketsColumns => {
                Static(Permission::TicketsRead)
            }
            Self::TicketsTransition | Self::TicketsComment | Self::TicketsAssign => {
                Static(Permission::TicketsWrite)
            }
            Self::ReviewsList | Self::ReviewsGet => Static(Permission::PrsRead),
            Self::ReviewsApprove | Self::ReviewsComment | Self::ReviewsRequestChanges => {
                Static(Permission::PrsWrite)
            }
            Self::SessionsList => Static(Permission::SessionsRead),
            Self::SessionsSpawn | Self::EventsSubscribe | Self::NetFetch | Self::SettingsGet => Dynamic,
            Self::EventsUnsubscribe => Free,
            Self::SessionsSendText => Static(Permission::TerminalWrite),
            Self::ToolsOpen => Static(Permission::SessionsSpawn),
            Self::UiOpenScreen | Self::UiFocus => Static(Permission::UiOpen),
            Self::NotifySend => Static(Permission::Notify),
            Self::ClipboardWrite => Static(Permission::ClipboardWrite),
            Self::KvGet | Self::KvSet | Self::KvDelete | Self::KvList => Static(Permission::Storage),
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Plugin IPC DTOs
// ---------------------------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
pub struct PluginInfo {
    pub id: PluginId,
    pub name: String,
    pub version: String,
    pub description: String,
    pub enabled: bool,
    pub permissions: Vec<String>,
    pub granted: Vec<String>,
    /// Load/validation problems (incompatible `kelta_api`, platform, invalid manifest, ...).
    pub problems: Vec<String>,
    pub dir: PathBuf,
    /// Loaded from `plugins.dev_paths`.
    pub dev: bool,
    pub contributes: Contributes,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct PermissionInfo {
    pub permission: String,
    pub description: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
pub struct PluginInstallPreview {
    pub manifest: PluginManifest,
    pub permissions: Vec<PermissionInfo>,
    pub sha256: String,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct ScreenOpenResult {
    pub instance_id: ScreenInstanceId,
    /// `kelta-plugin://<id>/<entry>?instance=...`
    pub url: String,
}

/// Who is calling `PluginHost::call`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CallOrigin {
    Screen { instance_id: ScreenInstanceId },
    Trigger { trigger_id: String },
    Command { command_id: String },
}

/// Outbound request performed for a plugin (`net.fetch`, `http` action) after the allowlist check.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct ProxiedRequest {
    pub url: String,
    pub method: String,
    pub headers: BTreeMap<String, String>,
    /// Text body, or base64 when `body_base64`.
    pub body: Option<String>,
    pub body_base64: bool,
    pub timeout_ms: Option<u64>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct ProxiedResponse {
    pub status: u16,
    pub headers: BTreeMap<String, String>,
    /// Text body, or base64 when `body_base64` (5 MB cap).
    pub body: String,
    pub body_base64: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn permissions_round_trip() {
        for s in [
            "projects.read",
            "tickets.write",
            "events:ticket.*",
            "exec:kubectl",
            "net:*.acme.com",
            "clipboard.write",
            "storage",
            "provider",
        ] {
            let p = Permission::parse(s).unwrap();
            assert_eq!(p.to_string(), s);
        }
        assert!(Permission::parse("exec:").is_none());
        assert!(Permission::parse("bogus").is_none());
    }

    #[test]
    fn method_names() {
        assert_eq!(serde_json::to_string(&PluginMethod::TicketsList).unwrap(), "\"tickets.list\"");
        assert_eq!(PluginMethod::ALL.len(), 33);
    }
}

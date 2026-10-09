//! Claude Code launcher (SPEC §3.1 step 4/6, PLUGINS §8): argv per profile, generated
//! `claude-settings.json` (hooks), `mcp.json`, `context.md`, `claude --version` gate.

use std::path::{Path, PathBuf};
use std::time::Duration;

use kelta_proto::dirs::Dirs;
use kelta_proto::error::KeltaError;
use kelta_proto::settings::{ClaudeEffort, ClaudeSettings, PermissionMode};
use serde_json::{Value, json};

use crate::template::shell_quote;

pub const SETTINGS_FILE: &str = "claude-settings.json";
pub const MCP_FILE: &str = "mcp.json";
pub const CONTEXT_FILE: &str = "context.md";
pub const TICKET_FILE: &str = "ticket.md";

/// `'<data>/bin/current/kelta-ctl' hook` (POSIX single-quoted: the macOS data dir has a space).
pub fn hook_command(dirs: &Dirs) -> String {
    format!("{} hook", shell_quote(&dirs.stable_ctl().to_string_lossy()))
}

/// Generated `--settings` document (`kelta_proto::hooks::claude_settings`). `http` = `(port, kelta
/// session id)` when the HTTP transport is active; SessionStart always uses the command hook.
pub fn settings_json(dirs: &Dirs, cfg: &ClaudeSettings, http: Option<(u16, &str)>) -> Value {
    kelta_proto::hooks::claude_settings(&hook_command(dirs), cfg, http)
}

/// `mcp.json` for `--mcp-config`.
pub fn mcp_json(port: u16, sid: &str) -> Value {
    json!({
        "mcpServers": {
            "kelta": {
                "type": "http",
                "url": format!("http://127.0.0.1:{port}/mcp/{sid}"),
                "headers": { "Authorization": "Bearer ${KELTA_MCP_TOKEN}" }
            }
        }
    })
}

/// Facts rendered into `context.md`.
#[derive(Debug, Clone, Default)]
pub struct ContextInfo {
    pub project: String,
    pub worktree: PathBuf,
    pub branch: String,
    pub base: String,
    pub ticket: Option<(String, String, String)>,
    pub ticket_file: Option<PathBuf>,
    pub pr: Option<(String, String)>,
    pub mcp: bool,
    pub append: String,
}

/// `context.md` (passed with `--append-system-prompt-file`).
pub fn context_md(c: &ContextInfo) -> String {
    let mut s = String::from("# Kelta workspace\n\n");
    s.push_str(&format!(
        "You are running inside Kelta, in a dedicated git worktree.\n\n- Project: {}\n- Worktree: {}\n- Branch: {} (base {})\n",
        c.project,
        c.worktree.display(),
        c.branch,
        c.base
    ));
    if let Some((key, title, url)) = &c.ticket {
        s.push_str(&format!("- Ticket: {key} — {title} ({url})\n"));
    }
    if let Some(f) = &c.ticket_file {
        s.push_str(&format!("- Full ticket (description and recent comments): {}\n", f.display()));
    }
    if let Some((url, base)) = &c.pr {
        s.push_str(&format!("- Pull request under review: {url} (target {base})\n"));
    }
    if c.mcp {
        s.push_str(
            "\nKelta MCP tools (server `kelta`): get_ticket, transition_ticket, add_ticket_comment, \
             open_in_editor, create_pr, list_review_requests, notify. Prefer `create_pr` over pushing \
             and opening pull requests by hand.\n",
        );
    }
    if !c.append.trim().is_empty() {
        s.push('\n');
        s.push_str(c.append.trim());
        s.push('\n');
    }
    s
}

/// What to launch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LaunchMode {
    /// `--session-id <uuid> … "<prompt>"`.
    New { uuid: String, prompt: String },
    /// `--resume <uuid>`.
    Resume { uuid: String },
    /// `--continue` (fallback when resume is refused).
    Continue,
}

/// Inputs of the Claude argv.
#[derive(Debug, Clone)]
pub struct LaunchSpec {
    pub mode: LaunchMode,
    pub name: String,
    pub model: String,
    pub effort: ClaudeEffort,
    pub permission_mode: PermissionMode,
    pub settings_file: PathBuf,
    pub mcp_file: Option<PathBuf>,
    pub allowed_tools: Vec<String>,
    pub context_file: PathBuf,
    pub extra_args: Vec<String>,
}

fn enum_str<T: serde::Serialize>(v: &T) -> String {
    serde_json::to_value(v).ok().and_then(|v| v.as_str().map(str::to_owned)).unwrap_or_default()
}

/// argv (without the program).
pub fn argv(spec: &LaunchSpec) -> Vec<String> {
    let mut a: Vec<String> = Vec::new();
    match &spec.mode {
        LaunchMode::New { uuid, .. } => a.extend(["--session-id".into(), uuid.clone()]),
        LaunchMode::Resume { uuid } => a.extend(["--resume".into(), uuid.clone()]),
        LaunchMode::Continue => a.push("--continue".into()),
    }
    if !spec.name.is_empty() {
        a.extend(["-n".into(), spec.name.clone()]);
    }
    if !spec.model.is_empty() {
        a.extend(["--model".into(), spec.model.clone()]);
    }
    a.extend(["--effort".into(), enum_str(&spec.effort)]);
    a.extend(["--permission-mode".into(), enum_str(&spec.permission_mode)]);
    a.extend(["--settings".into(), spec.settings_file.to_string_lossy().into_owned()]);
    if let Some(m) = &spec.mcp_file {
        a.extend(["--mcp-config".into(), m.to_string_lossy().into_owned()]);
    }
    if !spec.allowed_tools.is_empty() {
        // One comma-joined value: a variadic option must not swallow the positional prompt.
        a.extend(["--allowedTools".into(), spec.allowed_tools.join(",")]);
    }
    a.extend(["--append-system-prompt-file".into(), spec.context_file.to_string_lossy().into_owned()]);
    a.extend(spec.extra_args.iter().cloned());
    if let LaunchMode::New { prompt, .. } = &spec.mode
        && !prompt.trim().is_empty()
    {
        if prompt.starts_with('-') {
            a.push("--".into());
        }
        a.push(prompt.clone());
    }
    a
}

/// First semver-looking token of `claude --version` output (`2.1.200 (Claude Code)`).
pub fn parse_version(text: &str) -> Option<semver::Version> {
    text.split(|c: char| c.is_whitespace() || c == '(' || c == ')' || c == ',')
        .map(|t| t.trim_start_matches('v'))
        .find_map(|t| semver::Version::parse(t).ok())
}

/// Run `<binary> --version` (5 s, non-interactive).
pub async fn probe_version(binary: &Path) -> Option<semver::Version> {
    let mut cmd = tokio::process::Command::new(binary);
    cmd.arg("--version")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .kill_on_drop(true);
    // one-shot: bounded version probe, armed once per binary path.
    let out = tokio::time::timeout(Duration::from_secs(5), cmd.output()).await.ok()?.ok()?;
    parse_version(&String::from_utf8_lossy(&out.stdout))
}

/// `true` when `found` satisfies `min` (unparsable minimum → accepted).
pub fn version_ok(found: &semver::Version, min: &str) -> bool {
    semver::Version::parse(min.trim()).map(|m| found >= &m).unwrap_or(true)
}

/// Write the three generated files into `run`.
pub fn write_files(
    run: &Path,
    settings: &Value,
    mcp: Option<&Value>,
    context: &str,
) -> Result<(), KeltaError> {
    let pretty = |v: &Value| serde_json::to_vec_pretty(v).map_err(KeltaError::from);
    crate::files::write_private(&run.join(SETTINGS_FILE), &pretty(settings)?)?;
    match mcp {
        Some(m) => crate::files::write_private(&run.join(MCP_FILE), &pretty(m)?)?,
        None => {
            let _ = std::fs::remove_file(run.join(MCP_FILE));
        }
    }
    crate::files::write_private(&run.join(CONTEXT_FILE), context.as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions() {
        let v = parse_version("2.1.201 (Claude Code)\n").unwrap();
        assert_eq!(v, semver::Version::new(2, 1, 201));
        assert!(version_ok(&v, "2.1.200"));
        assert!(!version_ok(&semver::Version::new(2, 0, 9), "2.1.200"));
        assert!(parse_version("nope").is_none());
    }

    #[test]
    fn dash_prompt_is_protected() {
        let spec = LaunchSpec {
            mode: LaunchMode::New { uuid: "u".into(), prompt: "-x".into() },
            name: String::new(),
            model: String::new(),
            effort: ClaudeEffort::Low,
            permission_mode: PermissionMode::Plan,
            settings_file: "/s".into(),
            mcp_file: None,
            allowed_tools: vec![],
            context_file: "/c".into(),
            extra_args: vec![],
        };
        let a = argv(&spec);
        assert_eq!(&a[a.len() - 2..], &["--".to_owned(), "-x".to_owned()]);
        assert!(a.contains(&"low".to_owned()) && a.contains(&"plan".to_owned()));
    }
}

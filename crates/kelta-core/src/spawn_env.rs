//! SpawnRequest → PtySpawnSpec pieces (ARCHITECTURE §7.2): environment assembly, program
//! resolution, default shell, and argv rewriting for restore policies (§7.5). Pure functions.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use kelta_proto::error::KeltaError;

/// Everything the environment of a new session is built from.
pub struct EnvInputs<'a> {
    pub login: &'a BTreeMap<String, String>,
    /// `terminal.env`.
    pub terminal_env: &'a BTreeMap<String, String>,
    /// Project `[env]` (effective settings `env`).
    pub project_env: &'a BTreeMap<String, String>,
    /// `SpawnRequest.env`.
    pub request_env: &'a BTreeMap<String, String>,
    pub version: &'a str,
    pub session_id: &'a str,
    pub project_id: &'a str,
    pub ctl_sock: &'a Path,
    pub hook_token: &'a str,
    pub ticket: Option<&'a str>,
    /// Claude sessions only.
    pub mcp_token: Option<&'a str>,
    /// `http://127.0.0.1:<port>/mcp/<sid>` when the lazy HTTP server is up (Claude + MCP).
    pub mcp_url: Option<&'a str>,
}

/// Env = LoginEnv ⊕ `terminal.env` ⊕ project env ⊕ request env ⊕ Kelta vars.
pub fn assemble_env(i: &EnvInputs<'_>) -> BTreeMap<String, String> {
    let mut env: BTreeMap<String, String> = i
        .login
        .iter()
        .filter(|(k, _)| !k.starts_with("KELTA_"))
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();
    for layer in [i.terminal_env, i.project_env, i.request_env] {
        for (k, v) in layer {
            env.insert(k.clone(), v.clone());
        }
    }
    let mut set = |k: &str, v: &str| {
        env.insert(k.to_owned(), v.to_owned());
    };
    set("TERM", "xterm-256color");
    set("COLORTERM", "truecolor");
    set("TERM_PROGRAM", "kelta");
    set("TERM_PROGRAM_VERSION", i.version);
    set("KELTA_SESSION_ID", i.session_id);
    set("KELTA_PROJECT_ID", i.project_id);
    set("KELTA_SOCK", &i.ctl_sock.to_string_lossy());
    set("KELTA_HOOK_TOKEN", i.hook_token);
    if let Some(t) = i.ticket {
        set("KELTA_TICKET", t);
    }
    if let Some(t) = i.mcp_token {
        set("KELTA_MCP_TOKEN", t);
    }
    if let Some(u) = i.mcp_url {
        set("KELTA_MCP_URL", u);
    }
    if env.get("LANG").is_none_or(|v| v.is_empty()) {
        env.insert("LANG".into(), "en_US.UTF-8".into());
    }
    env
}

/// Claude Code's config dir as the session will see it: `CLAUDE_CONFIG_DIR`, else `$HOME/.claude`.
pub fn claude_config_dir(env: &BTreeMap<String, String>) -> Option<PathBuf> {
    let home = env.get("HOME").filter(|h| !h.is_empty());
    match env.get("CLAUDE_CONFIG_DIR").filter(|d| !d.is_empty()) {
        Some(d) => Some(PathBuf::from(expand_tilde(d, home.map(String::as_str)))),
        None => home.map(|h| Path::new(h).join(".claude")),
    }
}

/// The user's own `statusLine.command`, which Kelta's `--settings` overrides: Claude's user settings,
/// then the project's `.claude/settings.json` and `.claude/settings.local.json` (last wins).
pub fn user_statusline(env: &BTreeMap<String, String>, cwd: &Path) -> Option<String> {
    let files = [
        claude_config_dir(env)?.join("settings.json"),
        cwd.join(".claude/settings.json"),
        cwd.join(".claude/settings.local.json"),
    ];
    files.iter().rev().find_map(|f| {
        let v: serde_json::Value = serde_json::from_slice(&std::fs::read(f).ok()?).ok()?;
        Some(v.get("statusLine")?.get("command")?.as_str()?.to_owned()).filter(|c| !c.trim().is_empty())
    })
}

/// `terminal.shell`, else the login `$SHELL`, else the platform default.
pub fn default_shell(terminal_shell: &str, login: &BTreeMap<String, String>) -> PathBuf {
    if !terminal_shell.trim().is_empty() {
        return PathBuf::from(expand_tilde(terminal_shell.trim(), login.get("HOME").map(String::as_str)));
    }
    if let Some(s) = login.get("SHELL").filter(|s| !s.is_empty()) {
        return PathBuf::from(s);
    }
    if cfg!(target_os = "macos") { PathBuf::from("/bin/zsh") } else { PathBuf::from("/bin/bash") }
}

/// `~` / `~/x` expansion.
pub fn expand_tilde(p: &str, home: Option<&str>) -> String {
    match (p, home) {
        ("~", Some(h)) => h.to_owned(),
        (s, Some(h)) if s.starts_with("~/") => format!("{}/{}", h.trim_end_matches('/'), &s[2..]),
        (s, _) => s.to_owned(),
    }
}

/// Resolve `program` to an absolute path: paths are taken relative to `cwd`; bare names are
/// looked up with `which` in the login `PATH`. Missing → `NotFound` with a hint in `detail`.
pub fn resolve_program(program: &str, path_var: Option<&str>, cwd: &Path) -> Result<PathBuf, KeltaError> {
    let not_found = || {
        KeltaError::not_found(format!("`{program}` not found in your login PATH"))
            .with_detail(serde_json::json!({ "program": program, "hint": install_hint(program) }))
    };
    if program.contains('/') {
        let p = if Path::new(program).is_absolute() { PathBuf::from(program) } else { cwd.join(program) };
        return if p.is_file() { Ok(p) } else { Err(not_found()) };
    }
    which::which_in(program, path_var, cwd).map_err(|_| not_found())
}

fn install_hint(program: &str) -> Option<&'static str> {
    match program {
        "claude" => Some("Install Claude Code: https://docs.claude.com/claude-code (then open Diagnostics)"),
        "nvim" => Some("Install Neovim (brew install neovim / apt install neovim)"),
        "lazygit" => Some("brew install lazygit / see https://github.com/jesseduffield/lazygit"),
        "lazydocker" => Some("brew install lazydocker / see https://github.com/jesseduffield/lazydocker"),
        _ => None,
    }
}

/// Claude CLI flags that take a value (used to find the positional prompt).
const CLAUDE_VALUE_FLAGS: &[&str] = &[
    "--model",
    "--effort",
    "--permission-mode",
    "--settings",
    "--mcp-config",
    "--allowedTools",
    "--allowed-tools",
    "--disallowedTools",
    "--disallowed-tools",
    "--append-system-prompt",
    "--append-system-prompt-file",
    "--system-prompt",
    "--system-prompt-file",
    "-n",
    "--name",
    "--session-id",
    "--resume",
    "-r",
    "--add-dir",
    "--output-format",
    "--input-format",
    "--fallback-model",
    "--agent",
    "--agents",
    "--setting-sources",
    "--plugin-dir",
    "--max-turns",
];

/// Flags whose value is a file that may be gone after a reboot (`<runtime>` lives in /tmp).
const CLAUDE_FILE_FLAGS: &[&str] =
    &["--settings", "--mcp-config", "--append-system-prompt-file", "--system-prompt-file"];

/// Rewrite the original Claude argv for a restore: drop the session selection
/// (`--session-id/--resume/--continue`) and the positional prompt, drop file flags whose file is
/// gone, then append `--resume <uuid>` (or `--continue` for the fallback).
pub fn claude_restore_args(
    original: &[String],
    uuid: &str,
    use_continue: bool,
    exists: &dyn Fn(&Path) -> bool,
) -> Vec<String> {
    let mut out = Vec::new();
    let mut it = original.iter();
    while let Some(a) = it.next() {
        let flag = a.split_once('=').map_or(a.as_str(), |(f, _)| f);
        if matches!(flag, "--continue" | "-c") {
            continue;
        }
        if CLAUDE_VALUE_FLAGS.contains(&a.as_str()) {
            let Some(v) = it.next() else { break };
            if matches!(a.as_str(), "--session-id" | "--resume" | "-r") {
                continue;
            }
            if CLAUDE_FILE_FLAGS.contains(&a.as_str()) && !exists(Path::new(v)) {
                continue;
            }
            out.push(a.clone());
            out.push(v.clone());
            continue;
        }
        if a.starts_with('-') {
            if matches!(flag, "--session-id" | "--resume") {
                continue;
            }
            out.push(a.clone());
        }
        // positional (the initial prompt): dropped on restore
    }
    if use_continue {
        out.push("--continue".into());
    } else {
        out.push("--resume".into());
        out.push(uuid.into());
    }
    out
}

/// The Claude session uuid passed in the argv (`--session-id` / `--resume`), if any.
pub fn claude_uuid_from_args(args: &[String]) -> Option<String> {
    let mut it = args.iter();
    while let Some(a) = it.next() {
        if let Some((f, v)) = a.split_once('=')
            && matches!(f, "--session-id" | "--resume")
        {
            return Some(v.to_owned());
        }
        if matches!(a.as_str(), "--session-id" | "--resume" | "-r") {
            return it.next().cloned();
        }
    }
    None
}

/// Editor restore: prepend `-S <session file>` when the file exists (nvim/vim), dropping any old one.
pub fn editor_restore_args(
    original: &[String],
    session_file: Option<&Path>,
    exists: &dyn Fn(&Path) -> bool,
) -> Vec<String> {
    let mut out = Vec::new();
    if let Some(f) = session_file.filter(|f| exists(f)) {
        out.push("-S".into());
        out.push(f.to_string_lossy().into_owned());
    }
    let mut it = original.iter();
    while let Some(a) = it.next() {
        if a == "-S" {
            it.next();
            continue;
        }
        out.push(a.clone());
    }
    out
}

/// Value following `flag` in `args` (`--listen <sock>`).
pub fn arg_value(args: &[String], flag: &str) -> Option<String> {
    args.iter().position(|a| a == flag).and_then(|i| args.get(i + 1)).cloned()
}

/// 128-bit random token, hex.
pub fn token() -> String {
    uuid::Uuid::new_v4().simple().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(a: &[&str]) -> Vec<String> {
        a.iter().map(|s| (*s).to_owned()).collect()
    }

    #[test]
    fn claude_resume_strips_prompt_and_session() {
        let orig = v(&[
            "--session-id",
            "U1",
            "-n",
            "SHOP-1 x",
            "--model",
            "opus",
            "--settings",
            "/gone/s.json",
            "--mcp-config",
            "/ok/m.json",
            "--verbose",
            "Work on it",
        ]);
        let exists = |p: &Path| p.starts_with("/ok");
        let out = claude_restore_args(&orig, "U1", false, &exists);
        assert_eq!(
            out,
            v(&[
                "-n",
                "SHOP-1 x",
                "--model",
                "opus",
                "--mcp-config",
                "/ok/m.json",
                "--verbose",
                "--resume",
                "U1"
            ])
        );
        let out = claude_restore_args(&orig, "U1", true, &exists);
        assert_eq!(out.last().map(String::as_str), Some("--continue"));
        assert_eq!(claude_uuid_from_args(&orig).as_deref(), Some("U1"));
    }

    #[test]
    fn editor_restore_prepends_session_file() {
        let out = editor_restore_args(&v(&["--listen", "/s", "."]), Some(Path::new("/f.vim")), &|_| true);
        assert_eq!(out, v(&["-S", "/f.vim", "--listen", "/s", "."]));
        let out = editor_restore_args(&v(&["-S", "/old", "."]), Some(Path::new("/f.vim")), &|_| false);
        assert_eq!(out, v(&["."]));
    }

    #[test]
    fn claude_dir() {
        let env = |kv: &[(&str, &str)]| kv.iter().map(|(k, v)| ((*k).to_owned(), (*v).to_owned())).collect();
        assert_eq!(claude_config_dir(&env(&[("HOME", "/h")])), Some(PathBuf::from("/h/.claude")));
        assert_eq!(
            claude_config_dir(&env(&[("HOME", "/h"), ("CLAUDE_CONFIG_DIR", "~/c")])),
            Some(PathBuf::from("/h/c"))
        );
        assert_eq!(claude_config_dir(&env(&[])), None);
    }

    #[test]
    fn user_statusline_takes_the_last_layer() {
        let d = tempfile::tempdir().unwrap();
        let home = d.path().join("h");
        let cwd = d.path().join("w");
        std::fs::create_dir_all(home.join(".claude")).unwrap();
        std::fs::create_dir_all(cwd.join(".claude")).unwrap();
        let e = BTreeMap::from([("HOME".to_owned(), home.display().to_string())]);
        assert_eq!(user_statusline(&e, &cwd), None);
        let line = |c: &str| format!(r#"{{"statusLine": {{"type": "command", "command": "{c}"}}}}"#);
        std::fs::write(home.join(".claude/settings.json"), line("~/.claude/line.sh")).unwrap();
        assert_eq!(user_statusline(&e, &cwd).as_deref(), Some("~/.claude/line.sh"));
        std::fs::write(cwd.join(".claude/settings.local.json"), line("echo local")).unwrap();
        std::fs::write(cwd.join(".claude/settings.json"), "{not json").unwrap();
        assert_eq!(user_statusline(&e, &cwd).as_deref(), Some("echo local"));
    }

    #[test]
    fn tilde() {
        assert_eq!(expand_tilde("~/x", Some("/h")), "/h/x");
        assert_eq!(expand_tilde("~", Some("/h")), "/h");
        assert_eq!(expand_tilde("/a", Some("/h")), "/a");
    }
}

//! Diagnostics probes (ARCHITECTURE §6 `diagnostics_run`): each probe is a small function that
//! returns a [`Check`]. Parsing and decision logic is pure and unit-tested; the I/O wrappers only
//! feed it.

use std::path::{Path, PathBuf};
use std::time::Duration;

use kelta_proto::ipc::{Check, CheckStatus, ToolVersion};
use kelta_proto::secret::SecretBackendStatus;

/// Maximum socket path length (sun_path is 104 on macOS, 108 on Linux; the project asserts < 100).
pub const MAX_SOCKET_PATH: usize = 100;

fn check(id: &str, label: &str, status: CheckStatus, detail: impl Into<String>, fix: Option<&str>) -> Check {
    Check { id: id.into(), label: label.into(), status, detail: detail.into(), fix: fix.map(str::to_owned) }
}

// ---- pure helpers ------------------------------------------------------------------------------

/// Extracts the first `x.y[.z]` token of a `--version` output (`claude 2.1.210 (Claude Code)`,
/// `git version 2.50.1`, `NVIM v0.11.3`, `gh version 2.80.0 (2026-01-01)`).
pub fn parse_version(output: &str) -> Option<String> {
    for line in output.lines() {
        for word in line.split(|c: char| c.is_whitespace() || c == '(' || c == ')' || c == ',') {
            let w = word.trim_start_matches(['v', 'V']);
            let mut parts = w.split('.');
            let first = parts.next().unwrap_or("");
            let second = parts.next().unwrap_or("");
            if !first.is_empty()
                && first.chars().all(|c| c.is_ascii_digit())
                && !second.is_empty()
                && second.chars().next().is_some_and(|c| c.is_ascii_digit())
            {
                return Some(w.trim_end_matches(['.', ';']).to_owned());
            }
        }
    }
    None
}

fn numeric_parts(v: &str) -> Vec<u64> {
    v.split(['.', '-', '+'])
        .map_while(|p| p.chars().take_while(char::is_ascii_digit).collect::<String>().parse::<u64>().ok())
        .collect()
}

/// `version >= min` comparing numeric components (missing components are zero).
pub fn version_at_least(version: &str, min: &str) -> bool {
    let (a, b) = (numeric_parts(version), numeric_parts(min));
    for i in 0..a.len().max(b.len()) {
        let (x, y) = (a.get(i).copied().unwrap_or(0), b.get(i).copied().unwrap_or(0));
        if x != y {
            return x > y;
        }
    }
    true
}

/// Finds `name` in `dirs` (an executable regular file).
pub fn find_in_dirs(name: &str, dirs: &[PathBuf]) -> Option<PathBuf> {
    use std::os::unix::fs::PermissionsExt;
    dirs.iter().map(|d| d.join(name)).find(|p| {
        std::fs::metadata(p).map(|m| m.is_file() && m.permissions().mode() & 0o111 != 0).unwrap_or(false)
    })
}

/// Directories searched for tools: `$PATH` plus the usual places a Dock/desktop launch misses.
pub fn search_dirs(path_var: Option<&str>, home: Option<&Path>) -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> =
        path_var.unwrap_or("").split(':').filter(|s| !s.is_empty()).map(PathBuf::from).collect();
    for extra in ["/opt/homebrew/bin", "/usr/local/bin", "/usr/bin", "/bin"] {
        dirs.push(PathBuf::from(extra));
    }
    if let Some(h) = home {
        dirs.push(h.join(".local/bin"));
        dirs.push(h.join(".cargo/bin"));
        dirs.push(h.join(".claude/local"));
    }
    let mut seen = std::collections::HashSet::new();
    dirs.retain(|d| seen.insert(d.clone()));
    dirs
}

/// Socket path check: absolute, short enough for `sun_path`.
pub fn socket_status(path: &Path) -> (CheckStatus, String) {
    let len = path.as_os_str().len();
    if len >= MAX_SOCKET_PATH {
        (CheckStatus::Fail, format!("{} is {len} bytes (limit {MAX_SOCKET_PATH})", path.display()))
    } else if path.exists() {
        (CheckStatus::Ok, format!("{} ({len} bytes), listening", path.display()))
    } else {
        (CheckStatus::Warn, format!("{} ({len} bytes), not created yet", path.display()))
    }
}

// ---- probes ------------------------------------------------------------------------------------

/// Graphics environment in effect (Linux); macOS reports the renderer.
pub fn graphics() -> Check {
    #[cfg(target_os = "linux")]
    {
        let wayland = std::env::var_os("WAYLAND_DISPLAY").is_some();
        let session = std::env::var("XDG_SESSION_TYPE").unwrap_or_else(|_| "unknown".into());
        match super::applied_graphics() {
            Some(a) => {
                let vars = if a.vars.is_empty() {
                    "no workaround variables set".to_owned()
                } else {
                    a.vars.iter().map(|(k, v)| format!("{k}={v}")).collect::<Vec<_>>().join(", ")
                };
                let detail = format!(
                    "profile: {}{}{}; session: {session}{}; {vars}",
                    if a.safe { "safe" } else { "normal" },
                    if a.nvidia { "; NVIDIA proprietary driver detected" } else { "" },
                    if a.guard_tripped { "; started in safe mode after a failed launch" } else { "" },
                    if wayland { " (Wayland)" } else { "" },
                );
                let status = if a.guard_tripped { CheckStatus::Warn } else { CheckStatus::Ok };
                let fix = (a.guard_tripped || a.nvidia)
                    .then_some("Settings > Linux graphics, or run `kelta --safe-graphics`");
                check("graphics", "Graphics", status, detail, fix)
            }
            None => check("graphics", "Graphics", CheckStatus::Warn, "pre_init did not run", None),
        }
    }
    #[cfg(not(target_os = "linux"))]
    {
        check("graphics", "Graphics", CheckStatus::Ok, "WKWebView (system WebKit)", None)
    }
}

/// WebKitGTK version (Linux) / WKWebView (macOS).
pub fn webkit() -> Check {
    #[cfg(target_os = "linux")]
    {
        // SAFETY: plain getters with no arguments; webkit2gtk 2.0 only exposes them through ffi.
        let v = unsafe {
            use webkit2gtk::ffi::*;
            format!(
                "{}.{}.{}",
                webkit_get_major_version(),
                webkit_get_minor_version(),
                webkit_get_micro_version()
            )
        };
        let status = if version_at_least(&v, "2.40") { CheckStatus::Ok } else { CheckStatus::Warn };
        check(
            "webkit",
            "WebKitGTK",
            status,
            format!("webkit2gtk-4.1 {v}"),
            (status == CheckStatus::Warn).then_some("Upgrade libwebkit2gtk-4.1 to 2.40 or newer"),
        )
    }
    #[cfg(not(target_os = "linux"))]
    {
        check("webkit", "WebKit", CheckStatus::Ok, "WKWebView (system WebKit)", None)
    }
}

/// Notification daemon: `org.freedesktop.Notifications.GetServerInformation` over zbus (Linux).
pub async fn notification_daemon() -> Check {
    #[cfg(target_os = "linux")]
    {
        let fix = Some("Install and start a notification daemon (mako, dunst, swaync, or your desktop's)");
        let probe = async {
            let conn = zbus::Connection::session().await.map_err(|e| e.to_string())?;
            let reply = conn
                .call_method(
                    Some("org.freedesktop.Notifications"),
                    "/org/freedesktop/Notifications",
                    Some("org.freedesktop.Notifications"),
                    "GetServerInformation",
                    &(),
                )
                .await
                .map_err(|e| e.to_string())?;
            reply.body().deserialize::<(String, String, String, String)>().map_err(|e| e.to_string())
        };
        // one-shot: bound the D-Bus round trip so diagnostics never hang
        match tokio::time::timeout(Duration::from_secs(2), probe).await {
            Ok(Ok((name, vendor, version, _spec))) => check(
                "notifications",
                "Notification daemon",
                CheckStatus::Ok,
                format!("{name} {version} ({vendor})"),
                None,
            ),
            Ok(Err(e)) => check("notifications", "Notification daemon", CheckStatus::Warn, e, fix),
            Err(_) => {
                check("notifications", "Notification daemon", CheckStatus::Warn, "no reply within 2 s", fix)
            }
        }
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = Duration::ZERO;
        check("notifications", "Notifications", CheckStatus::Ok, "macOS Notification Center", None)
    }
}

/// Secret backends as reported by the secrets crate.
pub fn secret_backends(list: &[SecretBackendStatus]) -> Check {
    let primary = if cfg!(target_os = "macos") { "keychain" } else { "secret-service" };
    let label = if cfg!(target_os = "macos") { "Keychain" } else { "Secret Service" };
    match list.iter().find(|b| b.backend == primary) {
        Some(b) if b.available => check(
            "secret-service",
            label,
            CheckStatus::Ok,
            b.detail.clone().unwrap_or_else(|| "available".into()),
            None,
        ),
        Some(b) => check(
            "secret-service",
            label,
            CheckStatus::Warn,
            b.detail.clone().unwrap_or_else(|| "not available".into()),
            Some(if cfg!(target_os = "macos") {
                "Unlock the login keychain, or use file:/env:/command: secrets"
            } else {
                "Start a keyring: gnome-keyring-daemon --start --components=secrets (or KeePassXC), or use file:/env:/command: secrets"
            }),
        ),
        None => check("secret-service", label, CheckStatus::Warn, "backend status unavailable", None),
    }
}

/// The encrypted secrets file, once it exists: warns while it is still locked this run.
pub fn secret_file(list: &[SecretBackendStatus]) -> Option<Check> {
    let b = list.iter().find(|b| b.backend == "encrypted-file")?;
    let detail = b.detail.clone().unwrap_or_default();
    if b.available {
        Some(check("secret-file", "Encrypted secrets file", CheckStatus::Ok, detail, None))
    } else if detail.starts_with("locked") {
        let fix = Some("Enter its passphrase in Settings → Accounts → Secret storage");
        Some(check("secret-file", "Encrypted secrets file", CheckStatus::Warn, detail, fix))
    } else {
        None
    }
}

/// Runs `<path> --version` with a 3 s limit.
async fn run_version(path: &Path) -> Option<String> {
    let mut cmd = tokio::process::Command::new(path);
    cmd.arg("--version").stdin(std::process::Stdio::null()).kill_on_drop(true);
    // one-shot: a hung tool must not hang the diagnostics pane
    let out = tokio::time::timeout(Duration::from_secs(3), cmd.output()).await.ok()?.ok()?;
    let text = String::from_utf8_lossy(&out.stdout).into_owned() + &String::from_utf8_lossy(&out.stderr);
    parse_version(&text)
}

/// One tool probe (`claude`, `nvim`, `git`, `gh`, `glab`).
pub async fn tool(
    name: &str,
    required: bool,
    min: Option<&str>,
    dirs: &[PathBuf],
) -> (Check, Option<ToolVersion>) {
    let label = name.to_owned();
    let missing = if required { CheckStatus::Fail } else { CheckStatus::Warn };
    let Some(path) = find_in_dirs(name, dirs) else {
        let fix = match name {
            "claude" => {
                "Install Claude Code (https://docs.claude.com/claude-code) and make sure it is in your login PATH"
            }
            "nvim" => "Install Neovim (brew install neovim / apt install neovim)",
            "git" => "Install git",
            "gh" => "Optional: install the GitHub CLI for GitHub token reuse",
            _ => "Optional: install the GitLab CLI for GitLab token reuse",
        };
        return (check(name, &label, missing, format!("`{name}` not found in PATH"), Some(fix)), None);
    };
    match run_version(&path).await {
        Some(version) => {
            let ok = min.is_none_or(|m| version_at_least(&version, m));
            let status = if ok { CheckStatus::Ok } else { CheckStatus::Warn };
            let detail = format!("{version} at {}", path.display());
            let fix = (!ok).then(|| format!("Update {name} to {} or newer", min.unwrap_or_default()));
            (check(name, &label, status, detail, fix.as_deref()), Some(ToolVersion { path, version, ok }))
        }
        None => (
            check(
                name,
                &label,
                CheckStatus::Warn,
                format!("{} did not report a version", path.display()),
                None,
            ),
            None,
        ),
    }
}

/// Hooks health: live sessions whose Claude hooks never reported (`hooks_active == false`).
pub fn hooks(total: usize, inactive: usize) -> Check {
    if total == 0 {
        check("hooks", "Claude hooks", CheckStatus::Ok, "no Claude session running", None)
    } else if inactive == 0 {
        check("hooks", "Claude hooks", CheckStatus::Ok, format!("active in {total} session(s)"), None)
    } else {
        check(
            "hooks",
            "Claude hooks",
            CheckStatus::Warn,
            format!(
                "{inactive} of {total} session(s) report no hooks; status falls back to output heuristics"
            ),
            Some("Check that kelta-ctl is reachable and ~/.claude settings are not overriding hooks"),
        )
    }
}

/// Control and runtime socket paths.
pub fn sockets(ctl: &Path, runtime: &Path) -> Check {
    let (status, detail) = socket_status(ctl);
    let fix = (status == CheckStatus::Fail).then_some("Use a shorter XDG_RUNTIME_DIR");
    check("sockets", "Sockets", status, format!("{detail}; runtime dir {}", runtime.display()), fix)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_common_version_outputs() {
        assert_eq!(parse_version("2.1.210 (Claude Code)").as_deref(), Some("2.1.210"));
        assert_eq!(parse_version("git version 2.50.1 (Apple Git-155)").as_deref(), Some("2.50.1"));
        assert_eq!(parse_version("NVIM v0.11.3\nBuild type: Release").as_deref(), Some("0.11.3"));
        assert_eq!(parse_version("gh version 2.80.0 (2026-01-01)\nhttps://x").as_deref(), Some("2.80.0"));
        assert_eq!(parse_version("glab 1.60.0 (abc)").as_deref(), Some("1.60.0"));
        assert_eq!(parse_version("no version here"), None);
        assert_eq!(parse_version(""), None);
    }

    #[test]
    fn compares_versions_numerically() {
        assert!(version_at_least("2.1.210", "2.1.200"));
        assert!(version_at_least("2.1.200", "2.1.200"));
        assert!(!version_at_least("2.1.99", "2.1.200"));
        assert!(version_at_least("2.2", "2.1.200"));
        assert!(!version_at_least("1.9.9", "2.0"));
        assert!(version_at_least("0.11.3-dev", "0.11"));
    }

    #[test]
    fn socket_length_limit() {
        let long = PathBuf::from(format!("/tmp/{}/ctl.sock", "a".repeat(120)));
        assert_eq!(socket_status(&long).0, CheckStatus::Fail);
        assert_eq!(socket_status(Path::new("/tmp/kelta-0/definitely-missing.sock")).0, CheckStatus::Warn);
        let tmp = tempfile::tempdir().unwrap();
        let p = tmp.path().join("c.sock");
        std::fs::write(&p, "").unwrap();
        assert_eq!(socket_status(&p).0, CheckStatus::Ok);
    }

    #[test]
    fn search_dirs_dedupe_and_find_tool() {
        use std::os::unix::fs::PermissionsExt;
        let tmp = tempfile::tempdir().unwrap();
        let exe = tmp.path().join("faketool");
        std::fs::write(&exe, "#!/bin/sh\necho 'faketool 3.4.5'\n").unwrap();
        std::fs::set_permissions(&exe, std::fs::Permissions::from_mode(0o755)).unwrap();
        let path = format!("{0}:{0}", tmp.path().display());
        let dirs = search_dirs(Some(&path), None);
        assert_eq!(dirs.iter().filter(|d| d.as_path() == tmp.path()).count(), 1);
        assert_eq!(find_in_dirs("faketool", &dirs), Some(exe));
        assert_eq!(find_in_dirs("missing-tool", &dirs), None);
    }

    #[tokio::test]
    async fn tool_probe_reads_version_and_min() {
        use std::os::unix::fs::PermissionsExt;
        let tmp = tempfile::tempdir().unwrap();
        let exe = tmp.path().join("faketool");
        std::fs::write(&exe, "#!/bin/sh\necho 'faketool 3.4.5'\n").unwrap();
        std::fs::set_permissions(&exe, std::fs::Permissions::from_mode(0o755)).unwrap();
        // A test thread forking while the script was open for writing makes exec fail with ETXTBSY
        // until that child execs; wait it out so the probe sees the script, not the race.
        for _ in 0..50 {
            match std::process::Command::new(&exe).output() {
                Err(e) if e.raw_os_error() == Some(26) => tokio::time::sleep(Duration::from_millis(20)).await,
                _ => break,
            }
        }
        let dirs = vec![tmp.path().to_path_buf()];
        let (c, v) = tool("faketool", true, Some("3.0"), &dirs).await;
        assert_eq!(c.status, CheckStatus::Ok);
        assert_eq!(v.unwrap().version, "3.4.5");
        let (c, v) = tool("faketool", true, Some("4.0"), &dirs).await;
        assert_eq!(c.status, CheckStatus::Warn);
        assert!(!v.unwrap().ok);
        let (c, _) = tool("absent-tool", true, None, &dirs).await;
        assert_eq!(c.status, CheckStatus::Fail);
        let (c, _) = tool("absent-tool", false, None, &dirs).await;
        assert_eq!(c.status, CheckStatus::Warn);
    }

    #[test]
    fn hooks_and_secret_checks() {
        assert_eq!(hooks(0, 0).status, CheckStatus::Ok);
        assert_eq!(hooks(3, 0).status, CheckStatus::Ok);
        assert_eq!(hooks(3, 1).status, CheckStatus::Warn);
        let primary = if cfg!(target_os = "macos") { "keychain" } else { "secret-service" };
        let up = SecretBackendStatus { backend: primary.into(), available: true, detail: None };
        let down =
            SecretBackendStatus { backend: primary.into(), available: false, detail: Some("x".into()) };
        assert_eq!(secret_backends(&[up]).status, CheckStatus::Ok);
        assert_eq!(secret_backends(&[down]).status, CheckStatus::Warn);
        assert_eq!(secret_backends(&[]).status, CheckStatus::Warn);
        let file = |available, detail: &str| SecretBackendStatus {
            backend: "encrypted-file".into(),
            available,
            detail: Some(detail.into()),
        };
        assert!(secret_file(&[]).is_none());
        assert!(secret_file(&[file(false, "not set up: …")]).is_none());
        assert_eq!(secret_file(&[file(false, "locked: …")]).unwrap().status, CheckStatus::Warn);
        assert_eq!(secret_file(&[file(true, "unlocked")]).unwrap().status, CheckStatus::Ok);
    }
}

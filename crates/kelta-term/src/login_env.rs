//! Login environment (ARCHITECTURE §7.1, D6), resolved once per app start.
//!
//! `$SHELL -l -i -c '<probe>'` prints the environment between NUL-delimited sentinels; rc files
//! may print anything around them. Fallbacks: `$SHELL -l -c`, then (macOS) `path_helper -s` over
//! the inherited environment, then the inherited environment.

use std::collections::BTreeMap;
use std::io::Read;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use kelta_proto::term::{LoginEnv, LoginEnvSource};
use rustix::event::{PollFd, PollFlags, Timespec, poll};

/// Start sentinel (between NUL bytes).
pub const BEGIN: &[u8] = b"\0__KELTA_ENV_BEGIN__\0";
/// End sentinel.
pub const END: &[u8] = b"\0__KELTA_ENV_END__\0";
/// Probe output cap (a runaway rc file must not exhaust memory).
const MAX_OUTPUT: usize = 4 * 1024 * 1024;
/// Variables describing the probe process itself, not the user's environment.
const DROP: &[&str] = &["_", "SHLVL", "PWD", "OLDPWD"];

/// Default shell when `$SHELL` is unset.
pub fn default_shell() -> PathBuf {
    if cfg!(target_os = "macos") { PathBuf::from("/bin/zsh") } else { PathBuf::from("/bin/bash") }
}

/// Probe command for a shell (by basename).
pub fn probe_for(shell: &Path) -> String {
    let name = shell.file_name().and_then(|n| n.to_str()).unwrap_or("sh");
    if name == "nu" || name == "nushell" {
        "^printf '\\000__KELTA_ENV_BEGIN__\\000'; ^env -0; ^printf '\\000__KELTA_ENV_END__\\000'".to_owned()
    } else {
        "printf '\\000__KELTA_ENV_BEGIN__\\000'; command env -0; printf '\\000__KELTA_ENV_END__\\000'"
            .to_owned()
    }
}

fn find(hay: &[u8], needle: &[u8], from: usize) -> Option<usize> {
    if from > hay.len() || needle.is_empty() {
        return None;
    }
    hay[from..].windows(needle.len()).position(|w| w == needle).map(|p| p + from)
}

/// Parse the environment between the sentinels; `None` when they are missing or nothing valid
/// was found. Noise before/after the sentinels is ignored.
pub fn parse_env_block(output: &[u8]) -> Option<BTreeMap<String, String>> {
    let start = find(output, BEGIN, 0)? + BEGIN.len();
    // The env block ends with a NUL that may also be the first byte of the END sentinel.
    let end = find(output, &END[1..], start).map(|e| e.saturating_sub(1).max(start))?;
    let block = &output[start..end.max(start)];
    let mut vars = BTreeMap::new();
    for entry in block.split(|&b| b == 0) {
        if entry.is_empty() {
            continue;
        }
        let Some(eq) = entry.iter().position(|&b| b == b'=') else { continue };
        if eq == 0 {
            continue;
        }
        let key = String::from_utf8_lossy(&entry[..eq]).into_owned();
        if DROP.contains(&key.as_str()) {
            continue;
        }
        let value = String::from_utf8_lossy(&entry[eq + 1..]).into_owned();
        vars.insert(key, value);
    }
    if vars.is_empty() { None } else { Some(vars) }
}

/// Parse `path_helper -s` output (`PATH="..."; export PATH;`).
pub fn parse_path_helper(output: &str) -> BTreeMap<String, String> {
    let mut vars = BTreeMap::new();
    for stmt in output.split(';') {
        let stmt = stmt.trim();
        if let Some((k, v)) = stmt.split_once('=') {
            let k = k.trim();
            if k.is_empty() || k.contains(' ') {
                continue;
            }
            let v = v.trim().trim_matches('"');
            vars.insert(k.to_owned(), v.to_owned());
        }
    }
    vars
}

/// Spawn `program args` without a controlling terminal, stdin `/dev/null`, and collect stdout
/// until EOF or `timeout`. The process group is killed on timeout.
pub fn run_capture(program: &Path, args: &[&str], timeout: Duration) -> Option<Vec<u8>> {
    let mut cmd = Command::new(program);
    cmd.args(args).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::null());
    // SAFETY: `setsid` is async-signal-safe; it detaches the probe from our controlling tty and
    // makes it a process group leader so a timeout can kill the whole group.
    unsafe {
        cmd.pre_exec(|| {
            if libc::setsid() == -1 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
    let mut child = cmd.spawn().ok()?;
    let out = collect(&mut child, timeout);
    let status = match out {
        Some(_) => child.wait().ok(),
        None => {
            kill_group(&child);
            let _ = child.wait();
            None
        }
    };
    match (out, status) {
        (Some(o), Some(_)) => Some(o),
        _ => None,
    }
}

fn kill_group(child: &Child) {
    if let Ok(pid) = i32::try_from(child.id()) {
        // SAFETY: plain syscalls on the probe's own process group (it called setsid).
        unsafe {
            libc::kill(-pid, libc::SIGKILL);
            libc::kill(pid, libc::SIGKILL);
        }
    }
}

/// Read stdout until EOF; None on timeout.
fn collect(child: &mut Child, timeout: Duration) -> Option<Vec<u8>> {
    let deadline = Instant::now() + timeout;
    let mut stdout = child.stdout.take()?;
    let mut out = Vec::new();
    let mut buf = [0u8; 16 * 1024];
    loop {
        let left = deadline.checked_duration_since(Instant::now())?;
        let ts = Timespec::try_from(left).ok()?;
        let mut fds = [PollFd::new(&stdout, PollFlags::IN)];
        match poll(&mut fds, Some(&ts)) {
            Ok(0) => return None,
            Ok(_) => match stdout.read(&mut buf) {
                Ok(0) => return Some(out),
                Ok(n) => {
                    out.extend_from_slice(&buf[..n]);
                    if out.len() > MAX_OUTPUT {
                        return None;
                    }
                }
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
                Err(_) => return Some(out),
            },
            Err(rustix::io::Errno::INTR) => {}
            Err(_) => return None,
        }
    }
}

fn probe(shell: &Path, interactive: bool, timeout: Duration) -> Option<BTreeMap<String, String>> {
    let probe = probe_for(shell);
    let args: Vec<&str> = if interactive { vec!["-l", "-i", "-c", &probe] } else { vec!["-l", "-c", &probe] };
    let out = run_capture(shell, &args, timeout)?;
    let vars = parse_env_block(&out)?;
    vars.contains_key("PATH").then_some(vars)
}

/// Resolve the login environment (blocking, at most ~2 × `timeout`).
pub fn resolve(timeout: Duration) -> LoginEnv {
    let shell = std::env::var_os("SHELL")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .unwrap_or_else(default_shell);
    if let Some(vars) = probe(&shell, true, timeout) {
        return LoginEnv { vars, source: LoginEnvSource::LoginInteractive, shell: Some(shell) };
    }
    if let Some(vars) = probe(&shell, false, timeout) {
        return LoginEnv { vars, source: LoginEnvSource::Login, shell: Some(shell) };
    }
    let mut env = LoginEnv::inherited();
    env.shell = Some(shell);
    if cfg!(target_os = "macos") {
        let helper = Path::new("/usr/libexec/path_helper");
        if let Some(out) = run_capture(helper, &["-s"], timeout) {
            let vars = parse_path_helper(&String::from_utf8_lossy(&out));
            if vars.contains_key("PATH") {
                env.vars.extend(vars);
                env.source = LoginEnvSource::PathHelper;
            }
        }
    }
    env
}

#[cfg(test)]
mod tests {
    use super::*;

    fn block(vars: &[(&str, &str)]) -> Vec<u8> {
        let mut v = Vec::new();
        v.extend_from_slice(BEGIN);
        for (k, val) in vars {
            v.extend_from_slice(format!("{k}={val}").as_bytes());
            v.push(0);
        }
        v.extend_from_slice(END);
        v
    }

    #[test]
    fn parses_between_sentinels_with_noisy_rc_output() {
        let mut out = b"Last login: Mon\nwelcome \x1b[31mred\x1b[0m\nPATH=/evil\n".to_vec();
        out.extend(block(&[
            ("PATH", "/usr/bin:/bin"),
            ("MULTI", "a\nb=c"),
            ("EMPTY", ""),
            ("_", "/usr/bin/env"),
        ]));
        out.extend_from_slice(b"\nbye from .zlogout\0PATH=/after\0");
        let vars = parse_env_block(&out).unwrap();
        assert_eq!(vars.get("PATH").map(String::as_str), Some("/usr/bin:/bin"));
        assert_eq!(vars.get("MULTI").map(String::as_str), Some("a\nb=c"));
        assert_eq!(vars.get("EMPTY").map(String::as_str), Some(""));
        assert!(!vars.contains_key("_"));
        assert_eq!(vars.len(), 3);
    }

    #[test]
    fn missing_or_truncated_sentinels() {
        assert!(parse_env_block(b"no sentinels here").is_none());
        let mut out = BEGIN.to_vec();
        out.extend_from_slice(b"PATH=/bin\0");
        assert!(parse_env_block(&out).is_none(), "no END sentinel");
        assert!(parse_env_block(&block(&[])).is_none(), "empty block");
        // Garbage entries without '=' are skipped.
        let mut out = BEGIN.to_vec();
        out.extend_from_slice(b"garbage\0=nokey\0HOME=/h\0");
        out.extend_from_slice(END);
        let vars = parse_env_block(&out).unwrap();
        assert_eq!(vars.len(), 1);
        assert_eq!(vars["HOME"], "/h");
    }

    #[test]
    fn invalid_utf8_is_lossy() {
        let mut out = BEGIN.to_vec();
        out.extend_from_slice(b"BIN=\xff\xfe\0PATH=/bin\0");
        out.extend_from_slice(END);
        let vars = parse_env_block(&out).unwrap();
        assert_eq!(vars["PATH"], "/bin");
        assert!(vars["BIN"].contains('\u{fffd}'));
    }

    #[test]
    fn path_helper_output() {
        let v = parse_path_helper(
            "PATH=\"/usr/local/bin:/usr/bin\"; export PATH;\nMANPATH=\"/usr/share/man\"; export MANPATH;\n",
        );
        assert_eq!(v["PATH"], "/usr/local/bin:/usr/bin");
        assert_eq!(v["MANPATH"], "/usr/share/man");
        assert_eq!(v.len(), 2);
    }

    #[test]
    fn probe_command_per_shell() {
        assert!(probe_for(Path::new("/usr/bin/nu")).starts_with("^printf"));
        assert!(probe_for(Path::new("/bin/zsh")).contains("command env -0"));
        assert!(probe_for(Path::new("/opt/homebrew/bin/fish")).contains("command env -0"));
    }

    #[test]
    fn sh_probe_round_trip() {
        // A real POSIX shell prints the sentinels around `env -0`, with rc-like noise.
        let probe = format!("echo noise; {}; echo trailing", probe_for(Path::new("/bin/sh")));
        let out = run_capture(Path::new("/bin/sh"), &["-c", &probe], Duration::from_secs(5)).unwrap();
        let vars = parse_env_block(&out).unwrap();
        assert!(vars.contains_key("PATH"), "{vars:?}");
    }

    #[test]
    fn capture_times_out_and_kills_the_group() {
        let t0 = Instant::now();
        let out =
            run_capture(Path::new("/bin/sh"), &["-c", "sleep 30 & sleep 30"], Duration::from_millis(200));
        assert!(out.is_none());
        assert!(t0.elapsed() < Duration::from_secs(5));
    }

    #[test]
    fn resolve_returns_a_path() {
        let env = resolve(Duration::from_secs(5));
        assert!(env.path().is_some_and(|p| !p.is_empty()), "{:?}", env.source);
        assert!(env.shell.is_some());
    }
}

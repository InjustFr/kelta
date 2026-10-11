//! A template's Claude in an external terminal or tmux (#347). Its Kelta pane runs a small wrapper: it
//! opens the host on a script that exports the session's Kelta variables (hooks, MCP) and runs `claude`,
//! then waits on a FIFO for Claude's exit code. The Kelta session so stays live exactly as long as that
//! Claude, and status, Resume and Finish work as in a built-in pane.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use kelta_proto::error::KeltaError;
use kelta_proto::settings::{ClaudeHost, SessionTemplate};
use kelta_work::template::shell_quote;

/// Holds the PID of the host's shell (the parent of Claude): what the focus button walks up from.
pub(crate) const PID_FILE: &str = "host.pid";
const SCRIPT_FILE: &str = "claude.command";
/// Set by the terminal the host opens, not by Kelta's pane.
const TERMINAL_VARS: &[&str] = &["TERM", "COLORTERM", "TERM_PROGRAM", "TERM_PROGRAM_VERSION"];

/// The host of the template a session was started from (a missing template: built-in).
pub(crate) fn host_of(templates: &[SessionTemplate], template_id: Option<&str>) -> ClaudeHost {
    template_id
        .and_then(|id| templates.iter().find(|t| t.id == id))
        .map_or(ClaudeHost::Builtin, |t| t.claude_host)
}

/// The host's shell PID, written by the script of session dir `dir`.
pub(crate) fn pid(dir: &Path) -> Option<u32> {
    std::fs::read_to_string(dir.join(PID_FILE)).ok()?.trim().parse().ok()
}

/// What the wrapper needs.
pub(crate) struct HostLaunch<'a> {
    pub host: ClaudeHost,
    /// `claude.terminal_command`.
    pub terminal_command: &'a [String],
    /// tmux session name for `multiplexer` (sanitized here).
    pub name: &'a str,
    /// Session runtime dir (private): script, PID file, FIFO.
    pub dir: &'a Path,
    pub cwd: &'a Path,
    pub shell: &'a Path,
    pub program: &'a Path,
    pub args: &'a [String],
    pub env: &'a BTreeMap<String, String>,
    pub login: &'a BTreeMap<String, String>,
}

fn sh_line(argv: &[String]) -> String {
    argv.iter().map(|a| shell_quote(a)).collect::<Vec<_>>().join(" ")
}

fn is_name(k: &str) -> bool {
    k.chars().next().is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && k.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// Writes the script the host runs and returns the wrapper (`/bin/sh -c`) for the Kelta pane.
pub(crate) fn prepare(l: &HostLaunch<'_>) -> Result<String, KeltaError> {
    let fifo = l.dir.join(format!("host-{}.fifo", &crate::spawn_env::token()[..8]));
    let (pid, script) = (l.dir.join(PID_FILE), l.dir.join(SCRIPT_FILE));
    let q = |p: &PathBuf| shell_quote(&p.to_string_lossy());
    let (fq, pq, script_q, cwd) = (q(&fifo), q(&pid), q(&script), shell_quote(&l.cwd.to_string_lossy()));

    // The host starts from its own environment: export what Kelta adds (session id, tokens, project env)
    // and PATH, never the terminal's own variables.
    let exports: String = l
        .env
        .iter()
        .filter(|(k, v)| {
            is_name(k)
                && !TERMINAL_VARS.contains(&k.as_str())
                && (k.as_str() == "PATH" || l.login.get(*k) != Some(*v))
        })
        .map(|(k, v)| format!("export {k}={}\n", shell_quote(v)))
        .collect();
    let mut argv = vec![l.program.to_string_lossy().into_owned()];
    argv.extend(l.args.iter().cloned());
    // EXIT trap: the code reaches the pane; a pane already gone removed the FIFO, nothing is written.
    let body = format!(
        "#!/bin/sh\ncd {cwd} || exit 1\necho $$ > {pq}\ntrap 'exit 129' HUP TERM\n\
         trap 's=$?; [ -p {fq} ] && echo $s > {fq}' EXIT\n{exports}{}\n",
        sh_line(&argv)
    );
    write_script(&script, &body)?;

    let launch = match l.host {
        ClaudeHost::Multiplexer => {
            let name: String =
                l.name.chars().map(|c| if c.is_ascii_alphanumeric() || c == '-' { c } else { '-' }).collect();
            let s = format!("kelta-{name}");
            // tmux runs the command through `sh -c`: the path is quoted once more for it.
            let cmd = shell_quote(&script_q);
            // shortcut: tmux only (default server); zellij would need its own launcher.
            format!(
                "if tmux has-session -t {} 2>/dev/null; then tmux new-window -t {} -c {cwd} {cmd}; \
                 else tmux new-session -d -s {} -c {cwd} {cmd}; fi",
                shell_quote(&format!("={s}")),
                shell_quote(&format!("={s}:")),
                shell_quote(&s)
            )
        }
        _ => {
            let shell = l.shell.to_string_lossy();
            let cwd = l.cwd.to_string_lossy();
            let s = script.to_string_lossy();
            let fill =
                |a: &String| a.replace("{command}", &s).replace("{cwd}", &cwd).replace("{shell}", &shell);
            let argv: Vec<String> = if l.terminal_command.is_empty() {
                let d: &[&str] = if cfg!(target_os = "macos") {
                    &["open", "-na", "Terminal", "{command}"]
                } else {
                    &["x-terminal-emulator", "-e", "{command}"]
                };
                d.iter().map(|a| fill(&(*a).to_owned())).collect()
            } else {
                l.terminal_command.iter().map(fill).collect()
            };
            sh_line(&argv)
        }
    };
    let what = if l.host == ClaudeHost::Multiplexer { "tmux" } else { "an external terminal" };
    // The launcher runs in the background: a terminal that stays in the foreground until its window
    // closes would otherwise keep the FIFO without a reader. Its failure ends the wait with 127.
    Ok(format!(
        "f={fq}; p={pq}\nrm -f \"$f\"; mkfifo -m 600 \"$f\" || exit 1\n\
         stop() {{ rm -f \"$f\"; kill $r 2>/dev/null; [ -s \"$p\" ] && pkill -HUP -P \"$(cat \"$p\")\"; exit 129; }}\n\
         trap stop HUP TERM INT\n\
         printf '%s\\n' 'Claude runs in {what}. Closing this pane ends it.'\n\
         ( {launch} || echo 127 > \"$f\" ) &\n\
         ( read c < \"$f\"; exit \"${{c:-1}}\" ) & r=$!\n\
         wait $r; c=$?\nrm -f \"$f\" \"$p\"\nexit $c\n"
    ))
}

fn write_script(path: &Path, body: &str) -> Result<(), KeltaError> {
    use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
    let mut f = std::fs::OpenOptions::new().write(true).create(true).truncate(true).mode(0o700).open(path)?;
    std::io::Write::write_all(&mut f, body.as_bytes())?;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(dir: &Path, terminal_command: &[String], args: &[&str]) -> (std::process::Output, String) {
        let env = BTreeMap::from([
            ("KELTA_SESSION_ID".to_owned(), "s1".to_owned()),
            ("TERM".to_owned(), "xterm-256color".to_owned()),
            ("HOME".to_owned(), "/home/me".to_owned()),
        ]);
        let login = BTreeMap::from([("HOME".to_owned(), "/home/me".to_owned())]);
        let args: Vec<String> = args.iter().map(|a| (*a).to_owned()).collect();
        let wrapper = prepare(&HostLaunch {
            host: ClaudeHost::Terminal,
            terminal_command,
            name: "shop",
            dir,
            cwd: dir,
            shell: Path::new("/bin/sh"),
            program: Path::new("/bin/sh"),
            args: &args,
            env: &env,
            login: &login,
        })
        .unwrap();
        let out = kelta_proto::term::command("/bin/sh").arg("-c").arg(wrapper).output().unwrap();
        (out, std::fs::read_to_string(dir.join(SCRIPT_FILE)).unwrap())
    }

    #[test]
    fn the_pane_ends_with_the_external_claude() {
        let tmp = tempfile::tempdir().unwrap();
        let log = tmp.path().join("ran");
        // A "terminal" that runs the script in the background, like a new window would.
        let term = vec!["/bin/sh".to_owned(), "-c".to_owned(), "\"$0\" &".to_owned(), "{command}".to_owned()];
        let cmd = format!("echo \"$KELTA_SESSION_ID\" > '{}'; exit 3", log.display());
        let (out, script) = run(tmp.path(), &term, &["-c", &cmd]);
        assert_eq!(out.status.code(), Some(3), "{}", String::from_utf8_lossy(&out.stderr));
        // Kelta's variables reach Claude, the pane's TERM does not.
        assert!(script.contains("export KELTA_SESSION_ID='s1'"));
        assert!(!script.contains("TERM=") && !script.contains("HOME="));
        assert_eq!(std::fs::read_to_string(&log).unwrap().trim(), "s1");
        assert!(!tmp.path().join(PID_FILE).exists(), "cleaned up");
        assert!(
            std::fs::read_dir(tmp.path()).unwrap().all(|e| !e
                .unwrap()
                .path()
                .to_string_lossy()
                .ends_with(".fifo"))
        );
    }

    #[test]
    fn a_launcher_that_fails_ends_the_pane() {
        let tmp = tempfile::tempdir().unwrap();
        let (out, _) = run(tmp.path(), &["/nonexistent/terminal".to_owned()], &["-c", "exit 0"]);
        assert_eq!(out.status.code(), Some(127));
    }

    #[test]
    fn closing_the_pane_ends_the_external_claude() {
        let tmp = tempfile::tempdir().unwrap();
        let env = BTreeMap::new();
        let args = ["30".to_owned()];
        let term = vec!["/bin/sh".to_owned(), "-c".to_owned(), "\"$0\" &".to_owned(), "{command}".to_owned()];
        let wrapper = prepare(&HostLaunch {
            host: ClaudeHost::Terminal,
            terminal_command: &term,
            name: "shop",
            dir: tmp.path(),
            cwd: tmp.path(),
            shell: Path::new("/bin/sh"),
            program: Path::new("/bin/sleep"),
            args: &args,
            env: &env,
            login: &env,
        })
        .unwrap();
        // Start the pane, wait for the host's PID, close the pane (SIGHUP like a closed PTY), then
        // the host's `sleep` (Claude) must be gone.
        let pid = tmp.path().join(PID_FILE);
        let check = format!(
            "/bin/sh -c \"$0\" & w=$!; while [ ! -s '{p}' ]; do sleep 0.05; done; h=$(cat '{p}'); \
             while ! pgrep -P $h >/dev/null; do sleep 0.05; done; kill -HUP $w; wait $w; echo pane=$?; \
             n=0; while pgrep -P $h >/dev/null && [ $n -lt 40 ]; do sleep 0.05; n=$((n+1)); done; \
             pgrep -P $h >/dev/null && echo alive || echo gone",
            p = pid.display()
        );
        let out = kelta_proto::term::command("/bin/sh").arg("-c").arg(check).arg(wrapper).output().unwrap();
        assert_eq!(
            String::from_utf8_lossy(&out.stdout).lines().rev().take(2).collect::<Vec<_>>(),
            ["gone", "pane=129"]
        );
    }

    #[test]
    fn multiplexer_opens_a_tmux_session_then_windows_in_it() {
        if which::which("tmux").is_err() {
            return;
        }
        let tmp = tempfile::tempdir().unwrap();
        // A private tmux server: TMUX_TMPDIR moves its default socket.
        let sock = tempfile::tempdir().unwrap();
        let tmux_env =
            BTreeMap::from([("TMUX_TMPDIR".to_owned(), sock.path().to_string_lossy().into_owned())]);
        let env = BTreeMap::new();
        let tmux =
            |args: &[&str]| kelta_proto::term::command("tmux").envs(&tmux_env).args(args).output().unwrap();
        // First no session (new-session), then a running one (new-window in it).
        for (create, code) in [(false, 4), (true, 5)] {
            if create {
                assert!(tmux(&["new-session", "-d", "-s", "kelta-my-repo"]).status.success());
            }
            let args = ["-c".to_owned(), format!("exit {code}")];
            let wrapper = prepare(&HostLaunch {
                host: ClaudeHost::Multiplexer,
                terminal_command: &[],
                name: "my.repo",
                dir: tmp.path(),
                cwd: tmp.path(),
                shell: Path::new("/bin/sh"),
                program: Path::new("/bin/sh"),
                args: &args,
                env: &env,
                login: &env,
            })
            .unwrap();
            let out = kelta_proto::term::command("/bin/sh")
                .envs(&tmux_env)
                .env_remove("TMUX")
                .arg("-c")
                .arg(wrapper)
                .output()
                .unwrap();
            assert_eq!(out.status.code(), Some(code), "{}", String::from_utf8_lossy(&out.stderr));
        }
        tmux(&["kill-server"]);
    }

    #[test]
    fn host_from_the_template() {
        let mut t =
            SessionTemplate { id: "x".into(), claude_host: ClaudeHost::Multiplexer, ..Default::default() };
        assert_eq!(host_of(std::slice::from_ref(&t), Some("x")), ClaudeHost::Multiplexer);
        assert_eq!(host_of(std::slice::from_ref(&t), Some("y")), ClaudeHost::Builtin);
        t.claude_host = ClaudeHost::Builtin;
        assert_eq!(host_of(&[t], None), ClaudeHost::Builtin);
    }
}

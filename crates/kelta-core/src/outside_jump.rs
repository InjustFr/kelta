//! Jump to a Claude session Kelta did not start (#346): select its tmux pane, then focus the terminal
//! window that owns it (Hyprland, Sway; the owning app on macOS), found by walking its parent PIDs.

use std::collections::HashMap;
use std::process::Stdio;
use std::time::Duration;

use kelta_proto::error::KeltaError;
use kelta_proto::term::LoginEnv;
use serde_json::Value;

use crate::Core;

const SHELLS: &[&str] =
    &["sh", "bash", "zsh", "fish", "dash", "ksh", "tcsh", "csh", "nu", "login", "sudo", "su"];
const PANE_FMT: &str = "#{pane_pid}\t#{pane_id}\t#{session_name}:#{window_index}.#{pane_index}";
const CLIENT_FMT: &str = "#{client_pid}\t#{client_tty}\t#{session_name}";

/// pid → (ppid, executable) from `ps -Ao pid=,ppid=,comm=` (the executable may contain spaces).
fn parse_ps(out: &str) -> HashMap<u32, (u32, String)> {
    out.lines()
        .filter_map(|l| {
            let mut it = l.split_whitespace();
            let pid = it.next()?.parse().ok()?;
            let ppid = it.next()?.parse().ok()?;
            let comm = it.collect::<Vec<_>>().join(" ");
            Some((pid, (ppid, comm)))
        })
        .collect()
}

/// `pid` then its parents, up to (not including) init.
fn ancestors(procs: &HashMap<u32, (u32, String)>, pid: u32) -> Vec<u32> {
    let mut chain = vec![pid];
    let mut cur = pid;
    while let Some((ppid, _)) = procs.get(&cur) {
        if *ppid <= 1 || chain.contains(ppid) {
            break;
        }
        chain.push(*ppid);
        cur = *ppid;
    }
    chain
}

/// The macOS app bundle a path lies in: its outermost `.app` (a helper app lives inside its app).
fn app_bundle(comm: &str) -> Option<&str> {
    comm.find(".app/").map(|i| &comm[..i + 4])
}

/// What the session runs in, for the UI's note: the first parent that is not a shell.
fn terminal_of(procs: &HashMap<u32, (u32, String)>, chain: &[u32]) -> String {
    chain
        .iter()
        .skip(1)
        .filter_map(|p| procs.get(p).map(|(_, c)| c.as_str()))
        .map(|c| match app_bundle(c) {
            Some(b) => b.rsplit('/').next().unwrap_or(b).trim_end_matches(".app"),
            None => c.rsplit('/').next().unwrap_or(c).trim_start_matches('-'),
        })
        .find(|n| !SHELLS.contains(n))
        .unwrap_or_default()
        .to_owned()
}

/// The tmux pane whose shell is one of the session's parents: `(pane id, session:window.pane)`.
fn tmux_pane(panes: &str, chain: &[u32]) -> Option<(String, String)> {
    panes.lines().find_map(|l| {
        let mut f = l.split('\t');
        let pid: u32 = f.next()?.parse().ok()?;
        let (id, label) = (f.next()?, f.next()?);
        chain.contains(&pid).then(|| (id.to_owned(), label.to_owned()))
    })
}

/// A client to show the pane in: one on its session, else any (`false`: it must switch to it).
fn tmux_client(clients: &str, session: &str) -> Option<(u32, String, bool)> {
    let all: Vec<(u32, &str, &str)> = clients
        .lines()
        .filter_map(|l| {
            let mut f = l.split('\t');
            Some((f.next()?.parse().ok()?, f.next()?, f.next()?))
        })
        .collect();
    let pick = all.iter().find(|c| c.2 == session).or(all.first())?;
    Some((pick.0, pick.1.to_owned(), pick.2 == session))
}

/// Hyprland (`hyprctl clients -j`): the address of the window of the closest parent.
fn hypr_window(clients: &str, chain: &[u32]) -> Option<String> {
    let list: Vec<Value> = serde_json::from_str(clients).ok()?;
    chain.iter().find_map(|p| {
        list.iter()
            .find(|c| c["pid"].as_u64() == Some(u64::from(*p)))
            .and_then(|c| c["address"].as_str().map(str::to_owned))
    })
}

/// Sway (`swaymsg -t get_tree`): the con id of the window of the closest parent.
fn sway_window(tree: &str, chain: &[u32]) -> Option<i64> {
    fn views<'a>(n: &'a Value, out: &mut Vec<&'a Value>) {
        if n["pid"].is_u64() {
            out.push(n);
        }
        for k in ["nodes", "floating_nodes"] {
            for c in n[k].as_array().into_iter().flatten() {
                views(c, out);
            }
        }
    }
    let tree: Value = serde_json::from_str(tree).ok()?;
    let mut all = Vec::new();
    views(&tree, &mut all);
    chain.iter().find_map(|p| {
        all.iter().find(|v| v["pid"].as_u64() == Some(u64::from(*p))).and_then(|v| v["id"].as_i64())
    })
}

/// `program args` in the login environment: its stdout when it succeeds within 3 s.
async fn out(env: &LoginEnv, program: &str, args: &[&str]) -> Option<String> {
    let bin = crate::spawn_env::resolve_program(program, env.path(), &std::env::temp_dir()).ok()?;
    let child = kelta_proto::term::async_command(bin)
        .args(args)
        .env_clear()
        .envs(&env.vars)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .ok()?;
    // one-shot: a hung compositor or tmux server must not hold the jump.
    let o = tokio::time::timeout(Duration::from_secs(3), child.wait_with_output()).await.ok()?.ok()?;
    o.status.success().then(|| String::from_utf8_lossy(&o.stdout).into_owned())
}

/// Focuses the window owning one of `chain`; false when this desktop offers no way to.
async fn focus_window(env: &LoginEnv, procs: &HashMap<u32, (u32, String)>, chain: &[u32]) -> bool {
    let set = |k: &str| env.get(k).is_some_and(|v| !v.is_empty());
    if set("HYPRLAND_INSTANCE_SIGNATURE") {
        let Some(addr) = out(env, "hyprctl", &["clients", "-j"]).await.and_then(|j| hypr_window(&j, chain))
        else {
            return false;
        };
        // hyprctl answers `ok`, or the error, with status 0.
        let r = out(env, "hyprctl", &["dispatch", "focuswindow", &format!("address:{addr}")]).await;
        return r.is_some_and(|r| r.trim() == "ok");
    }
    if set("SWAYSOCK") {
        let Some(id) = out(env, "swaymsg", &["-t", "get_tree"]).await.and_then(|t| sway_window(&t, chain))
        else {
            return false;
        };
        return out(env, "swaymsg", &[&format!("[con_id={id}]"), "focus"]).await.is_some();
    }
    if cfg!(target_os = "macos") {
        // shortcut: activates the app, not the one window, as the ticket asks; AX would need a permission.
        let app = chain.iter().find_map(|p| procs.get(p).and_then(|(_, c)| app_bundle(c)));
        return match app {
            Some(app) => out(env, "open", &["-a", app]).await.is_some(),
            None => false,
        };
    }
    false
}

impl Core {
    /// Brings forward the tmux pane, then the window, a Claude session Kelta did not start runs in, or a
    /// Kelta session whose Claude runs in an external terminal or tmux (#347).
    /// `None` when it did; else where it runs (terminal or tmux pane, may be empty) for the UI's note.
    pub async fn outside_jump(&self, id: &str) -> Result<Option<String>, KeltaError> {
        let outside = self.outside.lock().get(id).map(|s| s.pid);
        let pid = match outside {
            Some(p) => p,
            None => {
                let sid = kelta_proto::ids::SessionId::new(id);
                let hosted = self.sessions.lock().get(&sid).is_some_and(|e| {
                    e.info.lifecycle == kelta_proto::model::Lifecycle::Live
                        && e.info.claude.as_ref().is_some_and(|c| c.host.is_some())
                });
                hosted
                    .then(|| crate::claude_host::pid(&self.dirs.session_runtime(&sid.sid8())))
                    .ok_or_else(|| KeltaError::not_found("session ended"))?
            }
        };
        let Some(pid) = pid else { return Ok(Some(String::new())) };
        let env = &self.login_env;
        let procs = parse_ps(&out(env, "ps", &["-wwAo", "pid=,ppid=,comm="]).await.unwrap_or_default());
        let mut chain = ancestors(&procs, pid);
        let mut place = terminal_of(&procs, &chain);
        // shortcut: the default tmux server only; other sockets and other multiplexers get the note.
        if let Some(panes) = out(env, "tmux", &["list-panes", "-a", "-F", PANE_FMT]).await
            && let Some((pane, label)) = tmux_pane(&panes, &chain)
        {
            out(env, "tmux", &["select-window", "-t", &pane]).await;
            out(env, "tmux", &["select-pane", "-t", &pane]).await;
            let session = label.rsplit_once(':').map_or(label.as_str(), |(s, _)| s);
            let clients = out(env, "tmux", &["list-clients", "-F", CLIENT_FMT]).await.unwrap_or_default();
            place = format!("tmux {label}");
            // Detached: the pane is selected for the next attach, no window shows it.
            let Some((client, tty, on_it)) = tmux_client(&clients, session) else { return Ok(Some(place)) };
            if !on_it {
                out(env, "tmux", &["switch-client", "-c", &tty, "-t", &pane]).await;
            }
            chain = ancestors(&procs, client);
        }
        Ok((!focus_window(env, &procs, &chain).await).then_some(place))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PS: &str = "    1     0 /sbin/launchd\n  500     1 /System/Applications/Utilities/Terminal.app/Contents/MacOS/Terminal\n  \
                      600   500 login\n  700   600 -zsh\n  800   700 /Users/me/.local/bin/claude\n  \
                      900     1 tmux: server\n  910   900 -fish\n  920   910 claude\n  \
                      930   700 tmux\n 1000     1 /Applications/Visual Studio Code.app/Contents/Frameworks/Code Helper.app/Contents/MacOS/Code Helper\n";

    #[test]
    fn walks_up_to_the_terminal() {
        let procs = parse_ps(PS);
        assert_eq!(
            procs[&1000].1,
            "/Applications/Visual Studio Code.app/Contents/Frameworks/Code Helper.app/Contents/MacOS/Code Helper"
        );
        assert_eq!(ancestors(&procs, 800), [800, 700, 600, 500]);
        assert_eq!(ancestors(&procs, 4242), [4242], "gone: itself only");
        assert_eq!(terminal_of(&procs, &ancestors(&procs, 800)), "Terminal");
        assert_eq!(terminal_of(&procs, &ancestors(&procs, 920)), "tmux: server");
        assert_eq!(terminal_of(&procs, &[4242]), "");
        assert_eq!(app_bundle(&procs[&1000].1), Some("/Applications/Visual Studio Code.app"));
        assert_eq!(app_bundle("claude"), None);
        // A parent loop does not hang.
        let looped = parse_ps("5 6 a\n6 5 b\n");
        assert_eq!(ancestors(&looped, 5), [5, 6]);
    }

    #[test]
    fn finds_the_tmux_pane_and_a_client() {
        let chain = ancestors(&parse_ps(PS), 920);
        let panes = "300\t%1\twork:0.0\n910\t%4\tmain:2.1\n";
        assert_eq!(tmux_pane(panes, &chain), Some(("%4".into(), "main:2.1".into())));
        assert_eq!(tmux_pane(panes, &[800, 700]), None);
        let clients = "930\t/dev/ttys003\twork\n931\t/dev/ttys004\tmain\n";
        assert_eq!(tmux_client(clients, "main"), Some((931, "/dev/ttys004".into(), true)));
        assert_eq!(tmux_client(clients, "other"), Some((930, "/dev/ttys003".into(), false)));
        assert_eq!(tmux_client("", "main"), None);
    }

    #[test]
    fn finds_the_window_of_the_closest_parent() {
        let hypr = r#"[{"address":"0xa","pid":12},{"address":"0xb","pid":500}]"#;
        assert_eq!(hypr_window(hypr, &[800, 700, 500]).as_deref(), Some("0xb"));
        assert_eq!(hypr_window(hypr, &[800]), None);
        let sway = r#"{"id":1,"nodes":[{"id":2,"nodes":[{"id":7,"pid":12,"nodes":[]}],
            "floating_nodes":[{"id":9,"pid":500,"nodes":[]}]}]}"#;
        assert_eq!(sway_window(sway, &[800, 500]), Some(9));
        assert_eq!(sway_window(sway, &[12]), Some(7));
        assert_eq!(sway_window("not json", &[12]), None);
    }
}

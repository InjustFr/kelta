//! Web tool server processes (PLUGINS §2): spawned with `tokio::process` (no PTY) in their own
//! process group, stdout/stderr captured into a 64 KiB ring, readiness read from the stdout stream
//! (no polling: `port_open` tries `connect()` after each stdout line and on a short 100/200/400 ms…
//! backoff armed by the open request), stop per `StopSpec`, then SIGKILL of the whole group.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::Arc;
use std::time::Duration;

use kelta_proto::error::KeltaError;
use kelta_proto::ext::{Ready, StopSpec};
use parking_lot::Mutex;
use regex::Regex;
use tokio::io::AsyncReadExt;
use tokio::sync::{mpsc, watch};

use crate::util::{ByteRing, parse_signal, signal_group};

/// Log ring size per instance.
pub const LOG_CAP: usize = 64 * 1024;
const MAX_LINE: usize = 16 * 1024;

/// A fully expanded server launch.
#[derive(Debug, Clone)]
pub struct Launch {
    pub program: String,
    pub args: Vec<String>,
    pub cwd: PathBuf,
    pub env: BTreeMap<String, String>,
    pub ready: Ready,
    pub ready_timeout: Duration,
    pub port: u16,
}

/// A running server process.
#[derive(Debug, Clone)]
pub struct ServerProc {
    pub pid: u32,
    pub log: Arc<Mutex<ByteRing>>,
    /// `Some(code)` once exited (`-1` = killed by a signal).
    pub exit: watch::Receiver<Option<i32>>,
}

impl ServerProc {
    pub fn exited(&self) -> Option<i32> {
        *self.exit.borrow()
    }

    pub fn log_tail(&self, lines: usize) -> String {
        self.log.lock().tail_lines(lines)
    }

    /// Wait until the process exits or `limit` elapses. True when it exited.
    pub async fn wait_exit(&self, limit: Duration) -> bool {
        let mut rx = self.exit.clone();
        // one-shot: stop grace period armed by tool_close
        tokio::time::timeout(limit, rx.wait_for(Option::is_some)).await.is_ok()
    }
}

/// Reserve a free loopback port (`{port}`).
pub fn free_port() -> Result<u16, KeltaError> {
    let l = std::net::TcpListener::bind(("127.0.0.1", 0))
        .map_err(|e| KeltaError::internal(format!("no free port: {e}")))?;
    l.local_addr().map(|a| a.port()).map_err(|e| KeltaError::internal(e.to_string()))
}

fn ready_from_line(ready: &Ready, re: Option<&Regex>, line: &str) -> Option<String> {
    match ready {
        Ready::StdoutJson(key) => {
            // The JSON object may be preceded by a log prefix on the same line.
            let start = line.find('{')?;
            let end = line.rfind('}')?;
            let v: serde_json::Value = serde_json::from_str(line.get(start..=end)?).ok()?;
            crate::util::json_path(&v, key).and_then(|u| u.as_str()).map(str::to_owned)
        }
        Ready::StdoutRegex(_) => re.and_then(|r| r.find(line)).map(|m| m.as_str().to_owned()),
        Ready::PortOpen(_) => None,
    }
}

async fn pump<R: tokio::io::AsyncRead + Unpin>(
    mut r: R,
    log: Arc<Mutex<ByteRing>>,
    lines: Option<mpsc::UnboundedSender<String>>,
) {
    let mut buf = vec![0u8; 8192];
    let mut pending: Vec<u8> = Vec::new();
    loop {
        let n = match r.read(&mut buf).await {
            Ok(0) | Err(_) => break,
            Ok(n) => n,
        };
        log.lock().push(&buf[..n]);
        let Some(tx) = &lines else { continue };
        for &b in &buf[..n] {
            if b == b'\n' || pending.len() >= MAX_LINE {
                let _ = tx.send(String::from_utf8_lossy(&pending).trim_end_matches('\r').to_owned());
                pending.clear();
                if b != b'\n' {
                    pending.push(b);
                }
            } else {
                pending.push(b);
            }
        }
    }
    if let Some(tx) = &lines
        && !pending.is_empty()
    {
        let _ = tx.send(String::from_utf8_lossy(&pending).into_owned());
    }
}

async fn port_open(port: u16) -> bool {
    tokio::net::TcpStream::connect(("127.0.0.1", port)).await.is_ok()
}

fn failed(err: KeltaError, proc_: &ServerProc) -> KeltaError {
    signal_group(proc_.pid, rustix::process::Signal::KILL);
    err.with_detail(serde_json::json!({ "log": proc_.log_tail(40) }))
}

/// Spawn the server and wait for readiness. Returns the process and the ready URL (`None` for
/// `port_open`: the caller builds `http://127.0.0.1:{port}/`).
pub async fn start(launch: &Launch) -> Result<(ServerProc, Option<String>), KeltaError> {
    let re = match &launch.ready {
        Ready::StdoutRegex(r) => {
            Some(Regex::new(r).map_err(|e| KeltaError::invalid(format!("ready regex: {e}")))?)
        }
        _ => None,
    };
    let mut cmd = tokio::process::Command::new(&launch.program);
    cmd.args(&launch.args)
        .current_dir(&launch.cwd)
        .envs(&launch.env)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .process_group(0)
        .kill_on_drop(false);
    let mut child = cmd.spawn().map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            KeltaError::not_found(format!("`{}` not found", launch.program))
        } else {
            KeltaError::internal(format!("cannot start `{}`: {e}", launch.program))
        }
    })?;
    let pid = child.id().ok_or_else(|| KeltaError::internal("server exited immediately"))?;
    let log = Arc::new(Mutex::new(ByteRing::new(LOG_CAP)));
    let (line_tx, mut line_rx) = mpsc::unbounded_channel::<String>();
    if let Some(out) = child.stdout.take() {
        tokio::spawn(pump(out, log.clone(), Some(line_tx)));
    }
    if let Some(err) = child.stderr.take() {
        tokio::spawn(pump(err, log.clone(), None));
    }
    let (exit_tx, exit_rx) = watch::channel(None);
    tokio::spawn(async move {
        let code = match child.wait().await {
            Ok(s) => s.code().unwrap_or(-1),
            Err(_) => -1,
        };
        let _ = exit_tx.send(Some(code));
    });
    let proc_ = ServerProc { pid, log, exit: exit_rx };

    let port_mode = matches!(launch.ready, Ready::PortOpen(_));
    // one-shot: readiness deadline armed by tool_open
    let deadline = tokio::time::sleep(launch.ready_timeout);
    tokio::pin!(deadline);
    let mut backoff = Duration::from_millis(100);
    // one-shot: port_open backoff step (re-armed only while waiting for readiness)
    let probe = tokio::time::sleep(backoff);
    tokio::pin!(probe);
    let mut exit = proc_.exit.clone();
    let mut stdout_open = true;
    loop {
        tokio::select! {
            () = &mut deadline => {
                return Err(failed(KeltaError::timeout(format!(
                    "`{}` was not ready after {} ms", launch.program, launch.ready_timeout.as_millis()
                )), &proc_));
            }
            changed = exit.changed() => {
                if changed.is_err() || exit.borrow().is_some() {
                    let code = exit.borrow().unwrap_or(-1);
                    return Err(failed(KeltaError::upstream(format!(
                        "`{}` exited with code {code} before it was ready", launch.program
                    )), &proc_));
                }
            }
            line = line_rx.recv(), if stdout_open => {
                match line {
                    Some(line) => {
                        if let Some(url) = ready_from_line(&launch.ready, re.as_ref(), &line) {
                            return Ok((proc_, Some(url)));
                        }
                        if port_mode && port_open(launch.port).await {
                            return Ok((proc_, None));
                        }
                    }
                    None => stdout_open = false,
                }
            }
            () = &mut probe, if port_mode => {
                if port_open(launch.port).await {
                    return Ok((proc_, None));
                }
                backoff = (backoff * 2).min(Duration::from_millis(1600));
                probe.as_mut().reset(tokio::time::Instant::now() + backoff);
            }
        }
    }
}

/// Stop per spec, then SIGKILL the group if it is still alive.
pub async fn stop(proc_: &ServerProc, spec: &StopSpec, stop_argv: Option<(Vec<String>, PathBuf)>) {
    if proc_.exited().is_some() {
        return;
    }
    let grace = match spec {
        StopSpec::Signal { signal, grace_ms } => {
            signal_group(proc_.pid, parse_signal(signal).unwrap_or(rustix::process::Signal::TERM));
            Duration::from_millis(*grace_ms)
        }
        StopSpec::Command { .. } => {
            if let Some((argv, cwd)) = stop_argv
                && let Some((program, args)) = argv.split_first()
            {
                let mut cmd = tokio::process::Command::new(program);
                cmd.args(args)
                    .current_dir(cwd)
                    .stdin(Stdio::null())
                    .stdout(Stdio::null())
                    .stderr(Stdio::null());
                if let Ok(mut child) = cmd.spawn() {
                    // one-shot: stop command deadline
                    let _ = tokio::time::timeout(Duration::from_secs(3), child.wait()).await;
                }
            }
            Duration::from_millis(3000)
        }
    };
    if !proc_.wait_exit(grace).await {
        signal_group(proc_.pid, rustix::process::Signal::KILL);
        proc_.wait_exit(Duration::from_secs(1)).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn readiness_parsers() {
        let json = Ready::StdoutJson("url".into());
        assert_eq!(
            ready_from_line(&json, None, r#"{"url":"http://localhost:3011/?token=x","port":3011}"#)
                .as_deref(),
            Some("http://localhost:3011/?token=x")
        );
        assert_eq!(ready_from_line(&json, None, "starting…"), None);
        assert_eq!(
            ready_from_line(&json, None, r#"INFO ready {"url":"http://x/"}"#).as_deref(),
            Some("http://x/")
        );
        let re = Regex::new(r"http://\S+").unwrap();
        let rx = Ready::StdoutRegex(r"http://\S+".into());
        assert_eq!(
            ready_from_line(&rx, Some(&re), "listening on http://127.0.0.1:9/ now").as_deref(),
            Some("http://127.0.0.1:9/")
        );
    }
}

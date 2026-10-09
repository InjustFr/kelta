//! Shared helpers for kelta-term integration tests.
#![allow(dead_code)]

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Mutex as StdMutex, OnceLock};
use std::time::{Duration, Instant};

use kelta_proto::api::{FrameSink, TerminalEvents};
use kelta_proto::ids::SessionId;
use kelta_proto::model::SessionKind;
use kelta_proto::term::{PtySpawnSpec, TerminalEvent};
use kelta_term::model::TermModel;

pub fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..").canonicalize().unwrap()
}

pub fn recordings_dir() -> PathBuf {
    workspace_root().join("fixtures/recordings")
}

/// A recording: `<name>.<cols>x<rows>.ansi`.
pub struct Recording {
    pub name: String,
    pub cols: u16,
    pub rows: u16,
    pub bytes: Vec<u8>,
}

pub fn recordings() -> Vec<Recording> {
    let mut out = Vec::new();
    for e in std::fs::read_dir(recordings_dir()).unwrap() {
        let p = e.unwrap().path();
        let Some(file) = p.file_name().and_then(|f| f.to_str()) else { continue };
        let Some(stem) = file.strip_suffix(".ansi") else { continue };
        let (name, size) = stem.rsplit_once('.').unwrap();
        let (c, r) = size.split_once('x').unwrap();
        out.push(Recording {
            name: name.to_owned(),
            cols: c.parse().unwrap(),
            rows: r.parse().unwrap(),
            bytes: std::fs::read(&p).unwrap(),
        });
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
}

/// Path to the `tui-sim` fixture binary, building it into a private target dir if needed
/// (a nested `cargo build` on the main target dir would wait on the running `cargo test`).
pub fn tui_sim() -> PathBuf {
    static PATH: OnceLock<PathBuf> = OnceLock::new();
    PATH.get_or_init(|| {
        let target = std::env::var_os("CARGO_TARGET_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| workspace_root().join("target"));
        for profile in ["debug", "release"] {
            let p = target.join(profile).join("tui-sim");
            if p.is_file() {
                return p;
            }
        }
        let dir = target.join("kelta-term-fixtures");
        let status = Command::new(env!("CARGO"))
            .args(["build", "-p", "tui-sim", "--locked", "--target-dir"])
            .arg(&dir)
            .current_dir(workspace_root())
            .status()
            .expect("cargo build -p tui-sim");
        assert!(status.success(), "building tui-sim failed");
        dir.join("debug/tui-sim")
    })
    .clone()
}

/// Minimal environment for test children.
pub fn test_env() -> BTreeMap<String, String> {
    let mut env = BTreeMap::new();
    env.insert("PATH".into(), "/usr/bin:/bin:/usr/sbin:/sbin:/opt/homebrew/bin:/usr/local/bin".into());
    env.insert("TERM".into(), "xterm-256color".into());
    env.insert("LANG".into(), "en_US.UTF-8".into());
    env.insert("HOME".into(), std::env::var("HOME").unwrap_or_else(|_| "/tmp".into()));
    env
}

/// `TerminalEvents` that records events and lets tests wait for them.
#[derive(Default)]
pub struct Events {
    pub list: StdMutex<Vec<TerminalEvent>>,
}

impl TerminalEvents for Events {
    fn on_event(&self, _id: &SessionId, ev: TerminalEvent) {
        self.list.lock().unwrap().push(ev);
    }
}

impl Events {
    pub fn all(&self) -> Vec<TerminalEvent> {
        self.list.lock().unwrap().clone()
    }

    pub fn exited(&self) -> Option<(Option<i32>, Option<i32>)> {
        self.all().into_iter().find_map(|e| match e {
            TerminalEvent::Exited { code, signal } => Some((code, signal)),
            _ => None,
        })
    }

    pub fn wait_exit(&self, timeout: Duration) -> (Option<i32>, Option<i32>) {
        wait_until(timeout, || self.exited()).expect("child did not exit in time")
    }
}

pub fn spec(
    id: &str,
    program: &str,
    args: &[&str],
    cols: u16,
    rows: u16,
    events: Arc<Events>,
) -> PtySpawnSpec {
    PtySpawnSpec {
        id: SessionId::new(id),
        program: PathBuf::from(program),
        args: args.iter().map(|s| (*s).to_owned()).collect(),
        cwd: std::env::temp_dir(),
        env: test_env(),
        cols,
        rows,
        scrollback_lines: 3000,
        kind: SessionKind::Shell,
        events,
    }
}

/// Poll `f` until it returns Some (tests only).
pub fn wait_until<T>(timeout: Duration, mut f: impl FnMut() -> Option<T>) -> Option<T> {
    let deadline = Instant::now() + timeout;
    loop {
        if let Some(v) = f() {
            return Some(v);
        }
        if Instant::now() >= deadline {
            return None;
        }
        #[allow(clippy::disallowed_methods)] // allowlisted: tests poll with short sleeps
        std::thread::sleep(Duration::from_millis(10));
    }
}

/// Aborts the test binary when it is not dropped within `limit`: a call that blocks forever
/// (no deadline of its own) fails the run fast, with every thread's stack on macOS, instead of
/// hanging CI.
pub struct Watchdog(std::sync::mpsc::Sender<()>);

impl Watchdog {
    pub fn arm(limit: Duration) -> Self {
        let (tx, rx) = std::sync::mpsc::channel::<()>();
        let test = std::thread::current().name().unwrap_or("?").to_owned();
        #[allow(clippy::disallowed_methods)] // allowlisted: test watchdog thread
        std::thread::spawn(move || {
            if rx.recv_timeout(limit) == Err(std::sync::mpsc::RecvTimeoutError::Timeout) {
                // Not eprintln!: the test harness captures it, and abort would lose it.
                let msg = format!("watchdog: {test} still running after {limit:?}, aborting\n");
                let _ = std::io::Write::write_all(&mut std::io::stderr(), msg.as_bytes());
                #[cfg(target_os = "macos")]
                let _ = Command::new("sample")
                    .args([&std::process::id().to_string(), "1", "-file", "/dev/stderr"])
                    .status();
                std::process::abort();
            }
        });
        Self(tx)
    }
}

/// A sink pushing frames into a shared vector.
#[derive(Clone, Default)]
pub struct Frames {
    pub frames: Arc<StdMutex<Vec<Vec<u8>>>>,
}

impl FrameSink for Frames {
    fn send(&mut self, frame: Vec<u8>) -> bool {
        self.frames.lock().unwrap().push(frame);
        true
    }
}

impl Frames {
    pub fn take(&self) -> Vec<Vec<u8>> {
        std::mem::take(&mut *self.frames.lock().unwrap())
    }
}

/// Feed `bytes` to a fresh model in one go.
pub fn model_from(bytes: &[u8], cols: u16, rows: u16, history: usize) -> TermModel {
    let mut m = TermModel::new(cols, rows, history);
    m.feed(bytes);
    m
}

/// Human-readable escaping for snapshot goldens (one line per CR LF).
pub fn escape(bytes: &[u8]) -> String {
    let s = String::from_utf8_lossy(bytes);
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '\x1b' => out.push('␛'),
            '\r' => out.push('␍'),
            '\n' => out.push_str("␊\n"),
            '\x07' => out.push('␇'),
            c if c.is_control() => out.push_str(&format!("\\x{:02x}", c as u32)),
            c => out.push(c),
        }
    }
    out
}

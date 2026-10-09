//! # kelta-term (L1)
//!
//! `TerminalHost` implementation (ARCHITECTURE §4, §7.1-§7.4, §9.3, §9.5):
//!
//! - [`backend`]: PTY backends (portable-pty, rustix openpty/fork), direct exec with the complete
//!   environment, new session + process group.
//! - `reader`: one thread per PTY (256 KiB stack): `poll` with the DEC 2026 sync deadline and the
//!   ack watchdog, 64 KiB reads, EAGAIN input queue.
//! - [`model`]: headless `alacritty_terminal::Term` (kitty keyboard off) answering terminal
//!   queries, events for core, OSC 7/9/777 pre-scan ([`prescan`]).
//! - [`snapshot`]: ANSI repaint for (re-)attaching views; [`flow`]: HIGH/LOW watermarks.
//! - [`login_env`]: login environment resolution (`resolve_login_env`).
//!
//! Sessions stay in the host after their process exits (text tail, re-attach shows the exit
//! banner) until a new `spawn` reuses the id.

pub mod backend;
pub mod flow;
pub mod frames;
pub mod inspect;
pub mod login_env;
pub mod model;
pub mod palette;
pub mod prescan;
mod reader;
mod session;
pub mod snapshot;

use std::os::fd::{FromRawFd, OwnedFd};
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

use kelta_proto::api::{FrameSink, TerminalHost};
use kelta_proto::error::KeltaError;
use kelta_proto::ids::SessionId;
use kelta_proto::model::AttachInfo;
use kelta_proto::term::{
    KillSignal, LoginEnv, PtySpawnSpec, SessionTermStats, TerminalLimits, TerminalPalette, TerminalStats,
};

use crate::backend::{PtyBackend, SpawnRequest};
use crate::model::TermModel;
use crate::palette::Palette;
pub use crate::session::{DEFAULT_MEMORY_CAP_MB, SHRINK_FLOOR, VIEW_SCROLLBACK};
use crate::session::{Session, Shared};

/// The PTY-backed terminal host.
pub struct PtyTerminalHost {
    env: LoginEnv,
    shared: Arc<Shared>,
    backend: Arc<dyn PtyBackend>,
}

impl PtyTerminalHost {
    pub fn new(env: LoginEnv, limits: TerminalLimits) -> Self {
        Self::with_backend(env, limits, backend::default_backend())
    }

    /// Host with an explicit PTY backend.
    pub fn with_backend(env: LoginEnv, limits: TerminalLimits, backend: Arc<dyn PtyBackend>) -> Self {
        Self { env, shared: Arc::new(Shared::new(limits)), backend }
    }

    /// Convenience for composition.
    pub fn new_arc(env: LoginEnv, limits: TerminalLimits) -> Arc<Self> {
        Arc::new(Self::new(env, limits))
    }

    /// The login environment this host was created with.
    pub fn login_env(&self) -> &LoginEnv {
        &self.env
    }

    /// Name of the PTY backend in use.
    pub fn backend_name(&self) -> &'static str {
        self.backend.name()
    }

    fn session(&self, id: &SessionId) -> Result<Arc<Session>, KeltaError> {
        self.shared
            .sessions
            .read()
            .get(id)
            .cloned()
            .ok_or_else(|| KeltaError::not_found(format!("terminal session {id}")))
    }

    fn history_for(&self, spec: &PtySpawnSpec) -> usize {
        if spec.scrollback_lines > 0 {
            spec.scrollback_lines as usize
        } else {
            self.shared.limits.lock().scrollback.for_kind(spec.kind.name()) as usize
        }
    }
}

fn set_winsize(master: &OwnedFd, cols: u16, rows: u16) {
    let ws = rustix::termios::Winsize { ws_row: rows, ws_col: cols, ws_xpixel: 0, ws_ypixel: 0 };
    if let Err(e) = rustix::termios::tcsetwinsize(master, ws) {
        tracing::debug!("TIOCSWINSZ failed: {e}");
    }
}

/// Non-blocking, close-on-exec pipe used to wake a reader thread.
fn wake_pipe() -> Result<(OwnedFd, OwnedFd), KeltaError> {
    let mut fds = [0 as libc::c_int; 2];
    // SAFETY: `fds` is a valid 2-element array.
    if unsafe { libc::pipe(fds.as_mut_ptr()) } != 0 {
        return Err(KeltaError::internal(format!("pipe: {}", std::io::Error::last_os_error())));
    }
    // SAFETY: both descriptors were just created and are owned by nobody else.
    let (r, w) = unsafe { (OwnedFd::from_raw_fd(fds[0]), OwnedFd::from_raw_fd(fds[1])) };
    for fd in [&r, &w] {
        let fl = rustix::fs::fcntl_getfl(fd).map_err(|e| KeltaError::internal(format!("fcntl: {e}")))?;
        rustix::fs::fcntl_setfl(fd, fl | rustix::fs::OFlags::NONBLOCK)
            .map_err(|e| KeltaError::internal(format!("fcntl: {e}")))?;
        rustix::io::fcntl_setfd(fd, rustix::io::FdFlags::CLOEXEC)
            .map_err(|e| KeltaError::internal(format!("fcntl: {e}")))?;
    }
    Ok((r, w))
}

fn signal_number(s: KillSignal) -> libc::c_int {
    match s {
        KillSignal::Hup => libc::SIGHUP,
        KillSignal::Term => libc::SIGTERM,
        KillSignal::Kill => libc::SIGKILL,
    }
}

impl TerminalHost for PtyTerminalHost {
    fn spawn(&self, spec: PtySpawnSpec) -> Result<(), KeltaError> {
        let cols = if spec.cols == 0 { 80 } else { spec.cols };
        let rows = if spec.rows == 0 { 24 } else { spec.rows };
        if let Some(old) = self.shared.sessions.read().get(&spec.id)
            && !old.exited.load(Ordering::SeqCst)
        {
            return Err(KeltaError::conflict(format!("terminal session {} is running", spec.id)));
        }
        let req = SpawnRequest {
            program: spec.program.clone(),
            args: spec.args.clone(),
            cwd: spec.cwd.clone(),
            env: spec.env.clone(),
            cols,
            rows,
        };
        let child = self.backend.spawn(&req)?;
        let kill_child = |pid: i32| {
            // SAFETY: plain syscalls on our own, not yet reaped child.
            unsafe {
                libc::kill(-pid, libc::SIGKILL);
                libc::waitpid(pid, std::ptr::null_mut(), 0);
            }
        };
        if let Err(e) = backend::set_nonblocking(&child.master) {
            kill_child(child.pid);
            return Err(e);
        }
        let (wake_r, wake_w) = match wake_pipe() {
            Ok(p) => p,
            Err(e) => {
                kill_child(child.pid);
                return Err(e);
            }
        };
        let master = Arc::new(child.master);
        let model = TermModel::new(cols, rows, self.history_for(&spec));
        let session = Arc::new(Session::new(
            spec.id.clone(),
            spec.kind.clone(),
            spec.events.clone(),
            model,
            child.pid,
            master.clone(),
            wake_w,
        ));
        {
            let mut map = self.shared.sessions.write();
            if let Some(old) = map.get(&spec.id) {
                if !old.exited.load(Ordering::SeqCst) {
                    drop(map);
                    kill_child(child.pid);
                    return Err(KeltaError::conflict(format!("terminal session {} is running", spec.id)));
                }
                let mem = old.state.lock().memory;
                self.shared.add_memory(mem, 0);
            }
            map.insert(spec.id.clone(), session.clone());
        }
        session.state.lock().refresh_memory(&self.shared);
        if let Err(e) = reader::start(session, self.shared.clone(), master, wake_r) {
            self.shared.sessions.write().remove(&spec.id);
            kill_child(child.pid);
            return Err(e);
        }
        Ok(())
    }

    fn attach(&self, id: &SessionId, cols: u16, rows: u16, sink: Box<dyn FrameSink>) -> Result<AttachInfo, KeltaError> {
        let s = self.session(id)?;
        let palette = *self.shared.palette.read();
        let now = Instant::now();
        let (info, outs) = {
            let mut st = s.state.lock();
            // Resize first, then snapshot after reflow (§7.3).
            if cols > 0 && rows > 0 && st.model.resize(cols, rows) {
                if let Some(m) = s.master() {
                    set_winsize(&m, st.model.cols(), st.model.rows());
                }
                st.refresh_memory(&self.shared);
            }
            if st.sink.is_some() {
                // The previous view is replaced.
                st.sink = None;
            }
            let generation = st.flow.attach();
            st.sink = Some(sink);
            st.exit_frame_pending = false;
            let outs = st.send_snapshot(&palette, &self.shared, now);
            let info = AttachInfo { generation, cols: st.model.cols(), rows: st.model.rows() };
            (info, outs)
        };
        s.wake();
        s.deliver(outs);
        Ok(info)
    }

    fn detach(&self, id: &SessionId, generation: u32) {
        let Ok(s) = self.session(id) else { return };
        let mut st = s.state.lock();
        if st.flow.attached() && st.flow.generation() == generation {
            st.drop_view(&self.shared);
            st.model.release_cache();
            st.released_at = Some(st.history);
        }
    }

    fn write(&self, id: &SessionId, bytes: &[u8]) -> Result<(), KeltaError> {
        self.session(id)?.write_input(bytes)
    }

    fn resize(&self, id: &SessionId, cols: u16, rows: u16) -> Result<(), KeltaError> {
        if cols == 0 || rows == 0 {
            return Err(KeltaError::invalid("terminal size must be at least 1x1"));
        }
        let s = self.session(id)?;
        let mut st = s.state.lock();
        // Model first (reflow), then TIOCSWINSZ → SIGWINCH.
        if st.model.resize(cols, rows) {
            st.refresh_memory(&self.shared);
        }
        if let Some(m) = s.master() {
            set_winsize(&m, st.model.cols(), st.model.rows());
        }
        Ok(())
    }

    fn ack(&self, id: &SessionId, generation: u32, bytes: u32) {
        if let Ok(s) = self.session(id) {
            s.ack(&self.shared, generation, bytes);
        }
    }

    fn kill(&self, id: &SessionId, signal: KillSignal) -> Result<(), KeltaError> {
        let s = self.session(id)?;
        let sig = signal_number(signal);
        let foreground = s.master().and_then(|m| rustix::termios::tcgetpgrp(&*m).ok()).map(|p| p.as_raw_nonzero().get());
        let proc = s.proc.lock();
        if proc.reaped {
            return Ok(());
        }
        // SAFETY: signals to our child's process group (and the PTY's foreground job), never to a
        // reaped pid (checked under the process lock).
        unsafe {
            libc::kill(-proc.pid, sig);
            if let Some(fg) = foreground
                && fg != proc.pid
            {
                libc::kill(-fg, sig);
            }
        }
        Ok(())
    }

    fn set_palette(&self, palette: TerminalPalette) {
        *self.shared.palette.write() = Palette::from_proto(&palette);
    }

    fn set_limits(&self, limits: TerminalLimits) {
        *self.shared.limits.lock() = limits;
        let sessions: Vec<Arc<Session>> = self.shared.sessions.read().values().cloned().collect();
        for s in sessions {
            let lines = limits.scrollback.for_kind(s.kind.name()) as usize;
            let mut st = s.state.lock();
            st.shrunk = false;
            st.model.set_history_limit(lines);
            st.refresh_memory(&self.shared);
        }
        self.shared.enforce_budget();
    }

    fn text_tail(&self, id: &SessionId, max_lines: u32) -> Result<String, KeltaError> {
        let s = self.session(id)?;
        let mut st = s.state.lock();
        Ok(st.model.text_tail(max_lines as usize))
    }

    fn stats(&self) -> TerminalStats {
        let sessions: Vec<Arc<Session>> = self.shared.sessions.read().values().cloned().collect();
        let mut out: Vec<SessionTermStats> = sessions
            .iter()
            .map(|s| {
                let st = s.state.lock();
                SessionTermStats {
                    id: s.id.clone(),
                    bytes_in: st.bytes_in,
                    history_lines: u32::try_from(st.history).unwrap_or(u32::MAX),
                    cols: st.model.cols(),
                    rows: st.model.rows(),
                    inflight: st.flow.inflight(),
                    attached: st.flow.attached(),
                    memory_bytes: st.memory,
                }
            })
            .collect();
        out.sort_by(|a, b| a.id.cmp(&b.id));
        TerminalStats {
            total_memory_bytes: out.iter().map(|s| s.memory_bytes).sum(),
            sessions: out,
            reader_threads: self.shared.reader_threads.load(Ordering::SeqCst),
        }
    }
}

/// Resolve the user's login environment (§7.1): `$SHELL -l -i -c` probe with sentinels, then
/// `$SHELL -l -c`, then (macOS) `path_helper`, then the inherited environment.
pub fn resolve_login_env(timeout: Duration) -> LoginEnv {
    login_env::resolve(timeout)
}

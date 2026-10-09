//! One PTY session: model + flow state behind a mutex, PTY fds, input queue, child process.

use std::collections::{HashMap, VecDeque};
use std::os::fd::OwnedFd;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::time::Instant;

use kelta_proto::api::{FrameSink, TerminalEvents};
use kelta_proto::error::KeltaError;
use kelta_proto::ids::SessionId;
use kelta_proto::model::SessionKind;
use kelta_proto::term::{TerminalEvent, TerminalLimits};
use parking_lot::{Mutex, RwLock};

use crate::flow::{AckAction, Flow};
use crate::frames;
use crate::model::{Output, TermModel};
use crate::palette::Palette;

/// Scrollback lines sent with a snapshot (`terminal.view_scrollback` default; see
/// docs/contract-requests/L1.md).
pub const VIEW_SCROLLBACK: usize = 1000;
/// History floor when the global memory cap shrinks sessions (ARCHITECTURE §9.5).
pub const SHRINK_FLOOR: usize = 500;
/// Default `terminal.memory_cap_mb` (used when limits carry 0).
pub const DEFAULT_MEMORY_CAP_MB: u32 = 160;
/// Pending input cap (a child that never reads its input).
const MAX_INPUT_QUEUE: usize = 16 * 1024 * 1024;

/// State shared by the host and every reader thread.
pub(crate) struct Shared {
    pub sessions: RwLock<HashMap<SessionId, Arc<Session>>>,
    pub palette: RwLock<Palette>,
    pub limits: Mutex<TerminalLimits>,
    pub total_memory: AtomicU64,
    pub reader_threads: AtomicU32,
    pub view_tick: AtomicU64,
    shrink_lock: Mutex<()>,
    cap_warned: AtomicBool,
}

impl Shared {
    pub fn new(limits: TerminalLimits) -> Self {
        Self {
            sessions: RwLock::new(HashMap::new()),
            palette: RwLock::new(Palette::default()),
            limits: Mutex::new(limits),
            total_memory: AtomicU64::new(0),
            reader_threads: AtomicU32::new(0),
            view_tick: AtomicU64::new(1),
            shrink_lock: Mutex::new(()),
            cap_warned: AtomicBool::new(false),
        }
    }

    pub fn tick(&self) -> u64 {
        self.view_tick.fetch_add(1, Ordering::Relaxed)
    }

    pub fn cap_bytes(&self) -> u64 {
        let mb = match self.limits.lock().memory_cap_mb {
            0 => DEFAULT_MEMORY_CAP_MB,
            n => n,
        };
        u64::from(mb) * 1024 * 1024
    }

    pub fn add_memory(&self, old: u64, new: u64) {
        if new >= old {
            self.total_memory.fetch_add(new - old, Ordering::Relaxed);
        } else {
            self.total_memory.fetch_sub(old - new, Ordering::Relaxed);
        }
    }

    /// Global scrollback cap (§9.5): shrink the history of the least-recently-viewed sessions to
    /// [`SHRINK_FLOOR`] lines, oldest first, until the estimate fits.
    pub fn enforce_budget(&self) {
        let cap = self.cap_bytes();
        if self.total_memory.load(Ordering::Relaxed) <= cap {
            return;
        }
        let Some(_guard) = self.shrink_lock.try_lock() else { return };
        let sessions: Vec<Arc<Session>> = self.sessions.read().values().cloned().collect();
        let mut order: Vec<(u64, Arc<Session>)> = sessions
            .into_iter()
            .map(|s| {
                let st = s.state.lock();
                let viewed = if st.flow.attached() { u64::MAX } else { st.last_viewed };
                drop(st);
                (viewed, s)
            })
            .collect();
        order.sort_by_key(|(v, _)| *v);
        let mut shrunk = false;
        for (_, s) in order {
            if self.total_memory.load(Ordering::Relaxed) <= cap {
                break;
            }
            let mut st = s.state.lock();
            if st.model.history_limit() > SHRINK_FLOOR {
                st.model.set_history_limit(SHRINK_FLOOR);
                st.refresh_memory(self);
                shrunk = true;
            }
        }
        if shrunk && !self.cap_warned.swap(true, Ordering::Relaxed) {
            tracing::warn!(
                cap_mb = cap / (1024 * 1024),
                "terminal scrollback memory cap reached: least recently viewed sessions trimmed to {SHRINK_FLOOR} lines"
            );
        }
    }
}

/// Mutable per-session state (one lock: model, flow, sink).
pub(crate) struct State {
    pub model: TermModel,
    pub flow: Flow,
    pub sink: Option<Box<dyn FrameSink>>,
    pub bytes_in: u64,
    /// Emit `Activity` on the next output (set at spawn and when the view goes away).
    pub activity_pending: bool,
    /// `(code, signal)` once the child exited.
    pub exit: Option<(Option<i32>, Option<i32>)>,
    pub last_viewed: u64,
    /// History size when the grid cache was last released.
    pub released_at: Option<usize>,
    pub history: usize,
    pub memory: u64,
}

impl State {
    pub fn new(model: TermModel) -> Self {
        Self {
            model,
            flow: Flow::new(),
            sink: None,
            bytes_in: 0,
            activity_pending: true,
            exit: None,
            last_viewed: 0,
            released_at: None,
            history: 0,
            memory: 0,
        }
    }

    /// Update the memory estimate after the history changed.
    pub fn refresh_memory(&mut self, shared: &Shared) {
        let h = self.model.history_size();
        let mem = self.model.memory_bytes();
        shared.add_memory(self.memory, mem);
        self.history = h;
        self.memory = mem;
    }

    /// Send a frame to the view; a closed channel detaches it.
    pub fn send(&mut self, frame: Vec<u8>, shared: &Shared) -> bool {
        let Some(sink) = self.sink.as_mut() else { return false };
        if sink.send(frame) {
            return true;
        }
        self.drop_view(shared);
        false
    }

    pub fn drop_view(&mut self, shared: &Shared) {
        self.sink = None;
        self.flow.drop_view();
        self.activity_pending = true;
        self.last_viewed = shared.tick();
    }

    /// Send a snapshot (attach / catch-up) and, after it, a pending exit frame.
    pub fn send_snapshot(&mut self, palette: &Palette, shared: &Shared, now: Instant) -> Vec<Output> {
        let mut frame = frames::snapshot_buffer(64 * 1024);
        let outs = self.model.snapshot_into(VIEW_SCROLLBACK, palette, &mut frame);
        let len = frame.len() - 1;
        if self.send(frame, shared) {
            self.flow.snapshot_sent(len, now);
            // After an exit the view also needs the exit banner (re-attach, or deferred while paused).
            if let Some((code, _)) = self.exit {
                self.send(frames::exit(code.unwrap_or(-1)), shared);
            }
        }
        outs
    }
}

/// Child process bookkeeping; `kill` never signals a reaped pid.
pub(crate) struct Proc {
    pub pid: i32,
    pub reaped: bool,
}

pub(crate) struct Session {
    pub id: SessionId,
    pub kind: SessionKind,
    pub events: Arc<dyn TerminalEvents>,
    pub state: Mutex<State>,
    pub proc: Mutex<Proc>,
    /// PTY master; taken (and closed once the last writer drops it) when the reader exits.
    master: Mutex<Option<Arc<OwnedFd>>>,
    /// Write end of the reader's wake-up pipe.
    wake: Mutex<Option<OwnedFd>>,
    pub input: Mutex<VecDeque<u8>>,
    pub exited: AtomicBool,
}

impl Session {
    pub fn new(
        id: SessionId,
        kind: SessionKind,
        events: Arc<dyn TerminalEvents>,
        model: TermModel,
        pid: i32,
        master: Arc<OwnedFd>,
        wake: OwnedFd,
    ) -> Self {
        Self {
            id,
            kind,
            events,
            state: Mutex::new(State::new(model)),
            proc: Mutex::new(Proc { pid, reaped: false }),
            master: Mutex::new(Some(master)),
            wake: Mutex::new(Some(wake)),
            input: Mutex::new(VecDeque::new()),
            exited: AtomicBool::new(false),
        }
    }

    pub fn master(&self) -> Option<Arc<OwnedFd>> {
        self.master.lock().clone()
    }

    /// Close our references to the PTY (reader exit).
    pub fn close_io(&self) {
        self.master.lock().take();
        self.wake.lock().take();
        self.input.lock().clear();
    }

    /// Wake the reader thread (input queued, watchdog armed).
    pub fn wake(&self) {
        if let Some(w) = self.wake.lock().as_ref() {
            let _ = rustix::io::write(w, &[1]);
        }
    }

    /// Write input to the PTY: directly when possible, otherwise queued for the reader (EAGAIN).
    pub fn write_input(&self, bytes: &[u8]) -> Result<(), KeltaError> {
        if bytes.is_empty() {
            return Ok(());
        }
        let Some(master) = self.master() else {
            return Err(KeltaError::conflict(format!("session {} has exited", self.id)));
        };
        let mut q = self.input.lock();
        let mut rest = bytes;
        if q.is_empty() {
            while !rest.is_empty() {
                match rustix::io::write(&*master, rest) {
                    Ok(n) => rest = &rest[n..],
                    Err(rustix::io::Errno::INTR) => {}
                    Err(rustix::io::Errno::AGAIN) => break,
                    Err(e) => return Err(KeltaError::internal(format!("pty write: {e}"))),
                }
            }
            if rest.is_empty() {
                return Ok(());
            }
        }
        if q.len() + rest.len() > MAX_INPUT_QUEUE {
            return Err(KeltaError::new(
                kelta_proto::ErrorCode::Conflict,
                format!("session {}: input queue full (the program is not reading)", self.id),
            ));
        }
        q.extend(rest);
        drop(q);
        self.wake();
        Ok(())
    }

    /// Drain queued input (reader thread, on POLLOUT).
    pub fn drain_input(&self, master: &OwnedFd) {
        let mut q = self.input.lock();
        while !q.is_empty() {
            let (a, _) = q.as_slices();
            match rustix::io::write(master, a) {
                Ok(n) => {
                    q.drain(..n);
                }
                Err(rustix::io::Errno::INTR) => {}
                Err(rustix::io::Errno::AGAIN) => break,
                Err(_) => {
                    q.clear();
                    break;
                }
            }
        }
    }

    pub fn has_input(&self) -> bool {
        !self.input.lock().is_empty()
    }

    /// Deliver model outputs: replies to the PTY, events to core (no lock held).
    pub fn deliver(&self, outs: Vec<Output>) {
        for o in outs {
            match o {
                Output::Reply(r) => {
                    let _ = self.write_input(&r);
                }
                Output::Event(ev) => self.events.on_event(&self.id, ev),
            }
        }
    }

    /// Ack from the view.
    pub fn ack(&self, shared: &Shared, generation: u32, bytes: u32) {
        let palette = *shared.palette.read();
        let now = Instant::now();
        let mut st = self.state.lock();
        let was_armed = st.flow.deadline().is_some();
        let outs = match st.flow.on_ack(generation, bytes, now) {
            AckAction::CatchUp => st.send_snapshot(&palette, shared, now),
            AckAction::Accepted | AckAction::Ignored => Vec::new(),
        };
        let armed = st.flow.deadline().is_some();
        drop(st);
        if armed && !was_armed {
            self.wake();
        }
        self.deliver(outs);
    }

    pub fn emit(&self, ev: TerminalEvent) {
        self.events.on_event(&self.id, ev);
    }
}

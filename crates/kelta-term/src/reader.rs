//! Per-session reader thread (ARCHITECTURE D4, §7.3). Allowlisted for `std::thread`.
//!
//! ```text
//! loop:
//!   timeout = min(DEC 2026 sync deadline, ack watchdog deadline) or infinite
//!   poll(master: IN (+OUT while input is queued), wake pipe: IN, timeout)
//!   sync deadline passed  → stop_sync (flush the synchronized update)
//!   ack deadline passed   → TerminalEvent::AckTimeout (one-shot)
//!   POLLOUT               → drain the EAGAIN input queue
//!   POLLIN                → read ≤ 64 KiB, parse, replies → PTY, events → core, Data frame → view
//!   idle                  → release the grid cache of hidden sessions
//! exit (EOF/EIO): flush, close fds, waitpid, Exit frame, TerminalEvent::Exited
//! ```

use std::os::fd::OwnedFd;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

use kelta_proto::error::KeltaError;
use kelta_proto::term::{READ_CHUNK, READER_STACK_SIZE, TerminalEvent};
use rustix::event::{PollFd, PollFlags, Timespec, poll};
use rustix::io::Errno;

use crate::flow::DataAction;
use crate::frames;
use crate::model::Output;
use crate::session::{Session, Shared};

/// Start the reader thread of a session.
pub(crate) fn start(
    session: Arc<Session>,
    shared: Arc<Shared>,
    master: Arc<OwnedFd>,
    wake: OwnedFd,
) -> Result<(), KeltaError> {
    shared.reader_threads.fetch_add(1, Ordering::SeqCst);
    let sh = shared.clone();
    #[allow(clippy::disallowed_methods)] // allowlisted: one reader thread per PTY (ARCHITECTURE D4)
    let r = std::thread::Builder::new()
        .name(format!("kelta-pty-{}", session.id))
        .stack_size(READER_STACK_SIZE)
        .spawn(move || run(&session, &sh, &master, wake));
    match r {
        Ok(_) => Ok(()),
        Err(e) => {
            shared.reader_threads.fetch_sub(1, Ordering::SeqCst);
            Err(KeltaError::internal(format!("reader thread: {e}")))
        }
    }
}

fn earliest(a: Option<Instant>, b: Option<Instant>) -> Option<Instant> {
    match (a, b) {
        (Some(x), Some(y)) => Some(x.min(y)),
        (x, None) => x,
        (None, y) => y,
    }
}

fn run(session: &Arc<Session>, shared: &Arc<Shared>, master: &Arc<OwnedFd>, wake: OwnedFd) {
    let mut buf = vec![0u8; READ_CHUNK];
    let mut idle_check = false;
    loop {
        let (sync_dl, ack_dl) = {
            let st = session.state.lock();
            (st.model.sync_deadline(), st.flow.deadline())
        };
        let deadline = earliest(sync_dl, ack_dl);
        let want_out = session.has_input();
        let timeout = if idle_check {
            Some(Duration::ZERO)
        } else {
            deadline.map(|d| d.saturating_duration_since(Instant::now()))
        };
        let ts = timeout.map(|t| Timespec::try_from(t).unwrap_or(Timespec { tv_sec: 3600, tv_nsec: 0 }));
        let mut flags = PollFlags::IN;
        if want_out {
            flags |= PollFlags::OUT;
        }
        let (m_rev, w_rev) = {
            let mut fds = [PollFd::new(&**master, flags), PollFd::new(&wake, PollFlags::IN)];
            match poll(&mut fds, ts.as_ref()) {
                Ok(_) => (fds[0].revents(), fds[1].revents()),
                Err(Errno::INTR) => continue,
                Err(e) => {
                    tracing::warn!(session = %session.id, "poll failed: {e}");
                    break;
                }
            }
        };
        let now = Instant::now();
        if idle_check && m_rev.is_empty() && w_rev.is_empty() {
            idle_check = false;
            housekeeping(session);
            continue;
        }
        if w_rev.contains(PollFlags::IN) {
            let mut sink = [0u8; 64];
            while matches!(rustix::io::read(&wake, &mut sink), Ok(n) if n > 0) {}
        }
        if m_rev.contains(PollFlags::OUT) {
            session.drain_input(master);
        }
        timers(session, shared, now, sync_dl, ack_dl);
        if m_rev.intersects(PollFlags::IN | PollFlags::HUP | PollFlags::ERR | PollFlags::NVAL) {
            match rustix::io::read(&**master, &mut buf) {
                Ok(0) => break,
                Ok(n) => {
                    process(session, shared, &buf[..n], now);
                    idle_check = true;
                }
                Err(Errno::AGAIN) | Err(Errno::INTR) => {
                    if m_rev.intersects(PollFlags::HUP | PollFlags::NVAL) && !m_rev.contains(PollFlags::IN) {
                        break;
                    }
                }
                // EIO: the slave side is closed (child exited).
                Err(_) => break,
            }
        }
    }
    drop(wake);
    exit(session, shared);
}

/// Expired DEC 2026 deadline → flush; expired ack watchdog → AckTimeout.
fn timers(session: &Session, shared: &Shared, now: Instant, sync_dl: Option<Instant>, ack_dl: Option<Instant>) {
    let sync_due = sync_dl.is_some_and(|d| now >= d);
    let ack_due = ack_dl.is_some_and(|d| now >= d);
    if !sync_due && !ack_due {
        return;
    }
    let palette = *shared.palette.read();
    let mut outs = Vec::new();
    {
        let mut st = session.state.lock();
        if sync_due && st.model.sync_deadline().is_some_and(|d| now >= d) {
            st.model.flush_sync(&palette, &mut outs);
        }
        if ack_due && let Some(generation) = st.flow.check_deadline(now) {
            outs.push(Output::Event(TerminalEvent::AckTimeout { generation }));
        }
    }
    session.deliver(outs);
}

/// Parse one chunk and forward it to the view.
fn process(session: &Session, shared: &Shared, chunk: &[u8], now: Instant) {
    let palette = *shared.palette.read();
    let mut outs = Vec::new();
    {
        let mut st = session.state.lock();
        if st.activity_pending {
            st.activity_pending = false;
            outs.push(Output::Event(TerminalEvent::Activity));
        }
        st.model.advance(chunk, &palette, &mut outs);
        st.bytes_in += chunk.len() as u64;
        if st.flow.on_data(chunk.len(), now) == DataAction::Send {
            st.send(frames::data(chunk), shared);
        }
        let h = st.model.history_size();
        if h != st.history {
            st.refresh_memory(shared);
        }
    }
    session.deliver(outs);
    if shared.total_memory.load(Ordering::Relaxed) > shared.cap_bytes() {
        shared.enforce_budget();
    }
}

/// Idle: hidden sessions give back the rows alacritty pre-allocates (in steps of 1000).
fn housekeeping(session: &Session) {
    let mut st = session.state.lock();
    if st.flow.attached() || st.released_at == Some(st.history) {
        return;
    }
    st.model.release_cache();
    st.released_at = Some(st.history);
}

fn exit(session: &Arc<Session>, shared: &Arc<Shared>) {
    let palette = *shared.palette.read();
    let mut outs = Vec::new();
    session.state.lock().model.flush_sync(&palette, &mut outs);
    session.deliver(outs);
    session.close_io();

    let (code, signal) = reap(session);
    session.exited.store(true, Ordering::SeqCst);
    {
        let mut st = session.state.lock();
        st.exit = Some((code, signal));
        if st.sink.is_some() {
            if st.flow.paused() {
                // Sent after the catch-up snapshot (see `State::send_snapshot`).
                st.exit_frame_pending = true;
            } else {
                st.send(frames::exit(code.unwrap_or(-1)), shared);
            }
        }
        st.model.release_cache();
    }
    shared.reader_threads.fetch_sub(1, Ordering::SeqCst);
    session.emit(TerminalEvent::Exited { code, signal });
}

/// Wait for the child without reaping (so `kill` cannot hit a recycled pid), then reap it under
/// the process lock.
fn reap(session: &Session) -> (Option<i32>, Option<i32>) {
    let pid = session.proc.lock().pid;
    loop {
        // SAFETY: zeroed siginfo_t is a valid out-parameter for waitid.
        let mut info: libc::siginfo_t = unsafe { std::mem::zeroed() };
        // SAFETY: plain syscall on our own child.
        let r = unsafe { libc::waitid(libc::P_PID, pid as libc::id_t, &mut info, libc::WEXITED | libc::WNOWAIT) };
        if r == 0 {
            break;
        }
        if std::io::Error::last_os_error().raw_os_error() == Some(libc::EINTR) {
            continue;
        }
        // ECHILD: already reaped elsewhere (e.g. SIGCHLD ignored).
        session.proc.lock().reaped = true;
        return (None, None);
    }
    let mut proc = session.proc.lock();
    let mut status = 0;
    // SAFETY: plain syscall on our own child.
    let r = unsafe { libc::waitpid(pid, &mut status, libc::WNOHANG) };
    proc.reaped = true;
    if r != pid {
        return (None, None);
    }
    if libc::WIFEXITED(status) {
        (Some(libc::WEXITSTATUS(status)), None)
    } else if libc::WIFSIGNALED(status) {
        (None, Some(libc::WTERMSIG(status)))
    } else {
        (None, None)
    }
}

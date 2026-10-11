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
//! exit (EOF/EIO, or the child's pidfd once the master is drained): flush, close fds, waitpid, Exit
//! frame, TerminalEvent::Exited
//! ```

use std::os::fd::OwnedFd;
use std::panic::AssertUnwindSafe;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

use alacritty_terminal::term::TermMode;
use kelta_proto::error::KeltaError;
use kelta_proto::term::{READ_CHUNK, READER_STACK_SIZE, TerminalEvent};
use rustix::event::{PollFd, PollFlags, Timespec, poll};
use rustix::io::Errno;

use crate::flow::DataAction;
use crate::frames;
use crate::history::BATCH;
use crate::model::Output;
use crate::session::{Session, Shared};

/// History kept by an exited session: the text tail core persists (ARCHITECTURE §7.5, ≤ 200 lines).
const EXITED_HISTORY: usize = 200;

/// How long a child may outlive the PTY hangup (SIGHUP) before its process group is killed.
const HANGUP_GRACE: Duration = Duration::from_secs(2);

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
        .spawn(move || {
            // A parser (alacritty/vte) panic must still reap the child and send Exited
            // (ARCHITECTURE §12.1); parking_lot mutexes don't poison.
            if std::panic::catch_unwind(AssertUnwindSafe(|| run(&session, &sh, &master, wake))).is_err() {
                tracing::error!(session = %session.id, "reader thread panicked; ending the session");
            }
            // Our master fd hangs up the PTY with close_io's: the child gets SIGHUP.
            drop(master);
            exit(&session, &sh);
        });
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
    let child = child_exit_fd(session);
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
        let (m_rev, w_rev, c_rev) = {
            let mut fds = [
                PollFd::new(&**master, flags),
                PollFd::new(&wake, PollFlags::IN),
                PollFd::new(child.as_ref().unwrap_or(&wake), PollFlags::IN),
            ];
            let n = if child.is_some() { 3 } else { 2 };
            match poll(&mut fds[..n], ts.as_ref()) {
                Ok(_) => (
                    fds[0].revents(),
                    fds[1].revents(),
                    if n == 3 { fds[2].revents() } else { PollFlags::empty() },
                ),
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
            housekeeping(session, shared);
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
        if c_rev.contains(PollFlags::IN) && !m_rev.contains(PollFlags::IN) {
            break;
        }
    }
}

/// Readable once the child exits. Linux needs it: a background job still holding the slave keeps
/// the master open (no EIO) after the shell exits. macOS revokes the terminal of an exited session
/// leader, so EOF suffices there.
fn child_exit_fd(session: &Session) -> Option<OwnedFd> {
    #[cfg(target_os = "linux")]
    {
        use rustix::process::{Pid, PidfdFlags, pidfd_open};
        let pid = Pid::from_raw(session.proc.lock().pid)?;
        pidfd_open(pid, PidfdFlags::empty())
            .inspect_err(|e| tracing::debug!(session = %session.id, "pidfd_open: {e}"))
            .ok()
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = session;
        None
    }
}

/// Expired DEC 2026 deadline → flush; expired ack watchdog → AckTimeout.
fn timers(
    session: &Session,
    shared: &Shared,
    now: Instant,
    sync_dl: Option<Instant>,
    ack_dl: Option<Instant>,
) {
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
            st.sync_keyboard(shared);
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
        #[cfg(test)]
        if chunk.windows(tests::PANIC.len()).any(|w| w == tests::PANIC) {
            panic!("test hook: model panic");
        }
        st.model.advance(chunk, &palette, &mut outs);
        st.bytes_in += chunk.len() as u64;
        st.flush_log(&session.id, shared, BATCH);
        if st.flow.on_data(chunk.len(), now) == DataAction::Send {
            st.send(frames::data(chunk), shared);
        }
        st.sync_keyboard(shared);
        // The primary history cannot change while the alternate screen is active, and reading it
        // there swaps the grids.
        if !st.model.term().mode().contains(TermMode::ALT_SCREEN) && st.model.history_size() != st.history {
            st.refresh_memory(shared);
        }
    }
    session.deliver(outs);
    if shared.total_memory.load(Ordering::Relaxed) > shared.cap_bytes() {
        shared.enforce_budget();
    }
}

/// Idle: the history log gets the lines of the burst; hidden sessions give back the rows
/// alacritty pre-allocates (in steps of 1000).
fn housekeeping(session: &Session, shared: &Shared) {
    let mut st = session.state.lock();
    st.flush_log(&session.id, shared, 0);
    if st.flow.attached() || st.released_at == Some(st.history) {
        return;
    }
    st.model.release_cache();
    st.released_at = Some(st.history);
}

/// Sends Exited even when `exit` panics (#314), reaping the child first if `exit` did not.
struct ExitGuard<'a> {
    session: &'a Session,
    shared: &'a Shared,
    status: Option<(Option<i32>, Option<i32>)>,
}

impl Drop for ExitGuard<'_> {
    fn drop(&mut self) {
        let (code, signal) = self.status.unwrap_or_else(|| {
            self.session.close_io();
            reap(self.session)
        });
        self.shared.reader_threads.fetch_sub(1, Ordering::SeqCst);
        self.session.emit(TerminalEvent::Exited { code, signal });
        // Only now may `spawn` reuse the id: a late Exited must not reach core after the new session.
        self.session.exited.store(true, Ordering::SeqCst);
    }
}

fn exit(session: &Session, shared: &Shared) {
    let mut guard = ExitGuard { session, shared, status: None };
    let palette = *shared.palette.read();
    let mut outs = Vec::new();
    let log = {
        let mut st = session.state.lock();
        #[cfg(test)]
        if session.id.as_str() == tests::EXIT_PANIC {
            panic!("test hook: exit panic");
        }
        st.model.flush_sync(&palette, &mut outs);
        // The screen rows never scroll out now: log them, and finish writing before Exited (quit
        // follows the last exit).
        st.model.log_screen();
        st.model.log_buf().map(std::mem::take)
    };
    if let (Some(h), Some(text)) = (shared.history.as_ref(), log.filter(|t| !t.is_empty())) {
        h.append_sync(&session.id, text);
    }
    session.deliver(outs);
    session.close_io();

    let (code, signal) = reap(session);
    guard.status = Some((code, signal));
    {
        let mut st = session.state.lock();
        st.exit = Some((code, signal));
        // An attached view ends with a snapshot then the Exit frame, now or (when bytes are still
        // in flight) after the catch-up of `State::send_snapshot`.
        if st.flow.on_exit() {
            let outs = st.send_snapshot(&palette, shared, Instant::now());
            drop(outs);
        }
        // A dead session only needs what core persists; keep it small until `kill` closes it.
        let lines = st.model.history_limit().min(EXITED_HISTORY);
        st.model.set_history_limit(lines);
        st.model.release_cache();
        st.refresh_memory(shared);
    }
}

/// Wait for the child without reaping (so `kill` cannot hit a recycled pid), then reap it under
/// the process lock. A child still alive `HANGUP_GRACE` after the hangup (SIGHUP ignored) gets its
/// group killed.
fn reap(session: &Session) -> (Option<i32>, Option<i32>) {
    use rustix::process::{Pid, Signal, WaitId, WaitIdOptions, kill_process_group, waitid};
    let pid = session.proc.lock().pid;
    let Some(p) = Pid::from_raw(pid) else { return (None, None) };
    let deadline = Instant::now() + HANGUP_GRACE;
    let mut opts = WaitIdOptions::EXITED | WaitIdOptions::NOWAIT | WaitIdOptions::NOHANG;
    loop {
        match waitid(WaitId::Pid(p), opts) {
            Ok(Some(_)) => break,
            Ok(None) if Instant::now() < deadline => {
                #[allow(clippy::disallowed_methods)] // allowlisted: bounded hangup grace
                std::thread::sleep(Duration::from_millis(10));
            }
            Ok(None) => {
                tracing::warn!(session = %session.id, "child ignored the hangup; killing its group");
                // Not reaped yet (NOWAIT): the group cannot be a recycled one.
                let _ = kill_process_group(p, Signal::KILL);
                opts.remove(WaitIdOptions::NOHANG);
            }
            Err(Errno::INTR) => {}
            // ECHILD: already reaped elsewhere (e.g. SIGCHLD ignored).
            Err(_) => {
                session.proc.lock().reaped = true;
                return (None, None);
            }
        }
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

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::time::{Duration, Instant};

    use kelta_proto::api::TerminalHost;
    use kelta_proto::ids::SessionId;
    use kelta_proto::model::SessionKind;
    use kelta_proto::term::{KillSignal, PtySpawnSpec, TerminalEvent, TerminalLimits};
    use kelta_proto::testing::RecordingTerminalEvents;

    /// Output that makes `process` panic, as a parser bug would.
    pub(super) const PANIC: &[u8] = b"KELTA_TEST_PANIC";
    /// Session id whose `exit` panics, as a broken grid in `flush_sync` would.
    pub(super) const EXIT_PANIC: &str = "exit-panic";

    fn spawn(
        h: &crate::PtyTerminalHost,
        id: &SessionId,
        script: &str,
    ) -> std::sync::Arc<RecordingTerminalEvents> {
        let ev = RecordingTerminalEvents::new();
        h.spawn(PtySpawnSpec {
            id: id.clone(),
            program: "/bin/sh".into(),
            args: vec!["-c".into(), script.into()],
            cwd: "/".into(),
            env: BTreeMap::new(),
            cols: 80,
            rows: 24,
            scrollback_lines: 0,
            kind: SessionKind::Shell,
            events: ev.clone(),
        })
        .unwrap();
        ev
    }

    /// The session's Exited (code, signal), within `within`.
    fn exited(ev: &RecordingTerminalEvents, within: Duration) -> (Option<i32>, Option<i32>) {
        let deadline = Instant::now() + within;
        loop {
            if let Some((code, signal)) = ev.events().iter().find_map(|(_, e)| match e {
                TerminalEvent::Exited { code, signal } => Some((*code, *signal)),
                _ => None,
            }) {
                return (code, signal);
            }
            assert!(Instant::now() < deadline, "no Exited");
            #[allow(clippy::disallowed_methods)] // allowlisted: test polling
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    /// #291: a panicking reader still sends Exited and gives its thread back.
    #[test]
    fn panicking_reader_exits_the_session() {
        let h = crate::PtyTerminalHost::new(TerminalLimits::default());
        let id = SessionId::new("panic");
        // The child outlives the panic: only the PTY hangup ends it.
        let ev = spawn(&h, &id, "echo KELTA_TEST_PANIC; exec sleep 30");
        exited(&ev, Duration::from_secs(10));
        assert_eq!(h.stats().reader_threads, 0);
        // kill closes it, freeing the id.
        h.kill(&id, KillSignal::Kill).unwrap();
        assert!(h.text_tail(&id, 1).is_err());
    }

    /// #314: a child ignoring the hangup is killed after the grace period, not waited on forever.
    #[test]
    fn child_ignoring_sighup_is_killed_after_the_grace() {
        let h = crate::PtyTerminalHost::new(TerminalLimits::default());
        let ev = spawn(&h, &SessionId::new("nohup"), "trap '' HUP; echo KELTA_TEST_PANIC; sleep 1000");
        let (_, signal) = exited(&ev, super::HANGUP_GRACE + Duration::from_secs(3));
        assert_eq!(signal, Some(libc::SIGKILL));
        assert_eq!(h.stats().reader_threads, 0);
    }

    /// #314: a panic in the exit path still reaps the child and sends Exited.
    #[test]
    fn panicking_exit_still_sends_exited() {
        let h = crate::PtyTerminalHost::new(TerminalLimits::default());
        let ev = spawn(&h, &SessionId::new(EXIT_PANIC), "exit 3");
        assert_eq!(exited(&ev, Duration::from_secs(10)), (Some(3), None));
        assert_eq!(h.stats().reader_threads, 0);
    }
}

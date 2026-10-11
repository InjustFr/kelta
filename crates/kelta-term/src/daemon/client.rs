//! App side: a `TerminalHost` forwarding every call to keltad. It holds no terminal model (the
//! memory stays in keltad), only the routing of frames to views and events to core.

use std::collections::{BTreeMap, HashMap};
use std::io::{BufReader, Read, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, Sender, SyncSender};
use std::time::Duration;

use kelta_proto::api::{FrameSink, TerminalEvents, TerminalHost};
use kelta_proto::error::{ErrorCode, KeltaError};
use kelta_proto::ids::SessionId;
use kelta_proto::model::AttachInfo;
use kelta_proto::term::{
    HistoryHit, KillSignal, PtySpawnSpec, TerminalEvent, TerminalLimits, TerminalPalette, TerminalStats,
};
use parking_lot::Mutex;
use serde::de::DeserializeOwned;
use serde_json::Value;

use super::{Call, Out, PROTOCOL_VERSION, Req, SpawnSpec, decode, encode, peer_uid, pidfile, read_msg};

const CALL_TIMEOUT: Duration = Duration::from_secs(if cfg!(test) { 2 } else { 10 });
/// How long a replaced keltad gets to close its socket after SIGTERM.
const STOP_TIMEOUT: Duration = Duration::from_secs(2);
const THREAD_STACK: usize = 256 * 1024;

type Reply = Result<Value, KeltaError>;
type EventJob = (Arc<dyn TerminalEvents>, SessionId, TerminalEvent);

struct View {
    token: u64,
    generation: Option<u32>,
    sink: Box<dyn FrameSink>,
}

struct Conn {
    w: Mutex<UnixStream>,
    seq: AtomicU64,
    pending: Mutex<HashMap<u64, SyncSender<Reply>>>,
    views: Mutex<HashMap<SessionId, View>>,
    events: Mutex<HashMap<SessionId, Arc<dyn TerminalEvents>>>,
    dead: AtomicBool,
    /// Closed on purpose: no synthetic exits.
    closing: AtomicBool,
}

impl Conn {
    fn next_seq(&self) -> u64 {
        self.seq.fetch_add(1, Ordering::SeqCst) + 1
    }

    fn send(&self, seq: u64, req: &Req, raw: &[u8]) -> Result<(), KeltaError> {
        let m = encode(&Call { seq, req: req.clone() }, raw)?;
        self.w.lock().write_all(&m).map_err(|e| KeltaError::internal(format!("keltad: {e}")))
    }

    fn call_seq(&self, seq: u64, req: &Req, raw: &[u8]) -> Reply {
        let (tx, rx) = mpsc::sync_channel(1);
        self.pending.lock().insert(seq, tx);
        if let Err(e) = self.send(seq, req, raw) {
            self.pending.lock().remove(&seq);
            return Err(e);
        }
        match rx.recv_timeout(CALL_TIMEOUT) {
            Ok(r) => r,
            Err(mpsc::RecvTimeoutError::Timeout) => {
                self.pending.lock().remove(&seq);
                Err(KeltaError::timeout("keltad did not answer"))
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => Err(KeltaError::internal("keltad connection lost")),
        }
    }

    fn frame(&self, id: &SessionId, token: u64, frame: &[u8]) {
        let mut views = self.views.lock();
        let Some(v) = views.get_mut(id).filter(|v| v.token == token) else { return };
        if v.sink.send(frame.to_vec()) {
            return;
        }
        // The view went away: detach in keltad too (once its generation is known).
        let generation = v.generation;
        views.remove(id);
        drop(views);
        if let Some(generation) = generation {
            let _ = self.send(0, &Req::Detach { id: id.clone(), generation }, &[]);
        }
    }

    fn read_loop(&self, stream: UnixStream, ev_tx: &Sender<EventJob>) {
        let mut r = BufReader::new(stream);
        while let Ok(Some(body)) = read_msg(&mut r) {
            let Some((out, raw)) = decode::<Out>(&body) else {
                tracing::warn!("keltad sent a malformed message; disconnecting");
                break;
            };
            match out {
                Out::Reply { seq, result } => {
                    if let Some(tx) = self.pending.lock().remove(&seq) {
                        let _ = tx.send(result);
                    }
                }
                Out::Frame { id, token } => self.frame(&id, token, raw),
                Out::Event { id, ev } => {
                    let h = if matches!(ev, TerminalEvent::Exited { .. }) {
                        self.events.lock().remove(&id)
                    } else {
                        self.events.lock().get(&id).cloned()
                    };
                    if let Some(h) = h {
                        let _ = ev_tx.send((h, id, ev));
                    }
                }
            }
        }
        self.dead.store(true, Ordering::SeqCst);
        self.pending.lock().clear();
        self.views.lock().clear();
        let lost: Vec<_> = self.events.lock().drain().collect();
        if !self.closing.load(Ordering::SeqCst) {
            tracing::warn!(sessions = lost.len(), "keltad connection lost");
            for (id, h) in lost {
                let _ = ev_tx.send((h, id, TerminalEvent::Exited { code: None, signal: None }));
            }
        }
    }
}

/// Protocol handshake on a fresh connection, before any reader thread owns the stream.
fn hello(s: &UnixStream) -> Result<(), KeltaError> {
    s.set_read_timeout(Some(CALL_TIMEOUT))?;
    (&*s).write_all(&encode(&Call { seq: 1, req: Req::Hello { v: PROTOCOL_VERSION } }, &[])?)?;
    let body = read_msg(&mut &*s)?.ok_or_else(|| KeltaError::internal("keltad closed the connection"))?;
    s.set_read_timeout(None)?;
    match decode::<Out>(&body) {
        Some((Out::Reply { result, .. }, _)) => result.map(drop),
        _ => Err(KeltaError::internal("keltad sent a malformed hello reply")),
    }
}

/// Pid of the keltad serving `sock`. Peer credentials are no use: keltad listens before it forks,
/// so they name the dead parent. It writes its own pid next to the socket; keltads from before
/// that are found by whoever holds the socket (lsof). Either way the process must be a keltad
/// (a stale pidfile may name a reused pid).
fn keltad_pid(sock: &Path) -> Option<i32> {
    let from_file = std::fs::read_to_string(pidfile(sock)).ok().and_then(|t| t.trim().parse().ok());
    let from_lsof = || {
        let out = kelta_proto::term::command("lsof").args(["-F", "pn", "--"]).arg(sock).output().ok()?;
        lsof_holders(&String::from_utf8_lossy(&out.stdout), sock).into_iter().find(is_keltad)
    };
    from_file.filter(is_keltad).or_else(from_lsof)
}

/// Pids in `lsof -F pn -- <sock>` output whose socket is bound to `sock`. Linux lsof selects unix
/// sockets by the inode bound at bind time, and the filesystem reuses the inode of a deleted
/// socket file: sockets of other keltads whose files are gone match too (#309). The name tells.
fn lsof_holders(out: &str, sock: &Path) -> Vec<i32> {
    let sock = sock.to_string_lossy();
    let mut pid = None;
    let mut pids = Vec::new();
    for l in out.lines() {
        if let Some(p) = l.strip_prefix('p') {
            pid = p.parse().ok();
        } else if let Some(n) = l.strip_prefix('n')
            // Linux appends " type=STREAM".
            && n.strip_prefix(&*sock).is_some_and(|rest| rest.is_empty() || rest.starts_with(" type="))
        {
            pids.extend(pid.filter(|p| !pids.contains(p)));
        }
    }
    pids
}

fn is_keltad(pid: &i32) -> bool {
    *pid != std::process::id() as i32 && argv0(*pid).is_some_and(|a| a.ends_with("keltad"))
}

/// The path `pid` was started as. Not Linux's `comm`: that is the 15-char name of the thread that
/// forked it. Not `/proc/<pid>/exe` either: it reads `… (deleted)` once an update replaced keltad.
#[cfg(target_os = "linux")]
fn argv0(pid: i32) -> Option<String> {
    let cmdline = std::fs::read(format!("/proc/{pid}/cmdline")).ok()?;
    cmdline.split(|&b| b == 0).next().map(|a| String::from_utf8_lossy(a).into_owned())
}

/// macOS `comm` is the exec path.
#[cfg(not(target_os = "linux"))]
fn argv0(pid: i32) -> Option<String> {
    let out = kelta_proto::term::command("ps").args(["-p", &pid.to_string(), "-o", "comm="]).output().ok()?;
    Some(String::from_utf8_lossy(&out.stdout).trim().to_owned())
}

/// SIGTERM the old keltad (`pid`, from [`keltad_pid`]), then wait (bounded) until it closes `s`.
/// No pid, or ESRCH: it exited after refusing our Hello, nothing to stop (#312).
fn stop_peer(pid: Option<i32>, s: &UnixStream) -> Result<(), KeltaError> {
    let Some(pid) = pid.and_then(rustix::process::Pid::from_raw) else { return Ok(()) };
    match rustix::process::kill_process(pid, rustix::process::Signal::TERM) {
        Ok(()) => {}
        Err(rustix::io::Errno::SRCH) => return Ok(()),
        Err(e) => return Err(std::io::Error::from(e).into()),
    }
    // macOS fails setsockopt with EINVAL once the peer has closed (#309): nothing left to wait for.
    if s.set_read_timeout(Some(STOP_TIMEOUT)).is_ok() {
        let mut buf = [0u8; 256];
        // EOF once it is gone; a timeout or error ends the wait too (the redial then tells).
        while matches!((&*s).read(&mut buf), Ok(n) if n > 0) {}
    }
    Ok(())
}

/// `TerminalHost` backed by keltad.
pub struct DaemonTerminalHost {
    sock: PathBuf,
    /// keltad binary + its log file; `None` = never launch (tests).
    launch: Option<(PathBuf, PathBuf, PathBuf)>,
    conn: Mutex<Option<Arc<Conn>>>,
}

impl DaemonTerminalHost {
    /// Connect to a running keltad.
    pub fn connect(sock: &Path) -> Result<Arc<Self>, KeltaError> {
        let h = Self { sock: sock.to_path_buf(), launch: None, conn: Mutex::new(None) };
        h.conn()?;
        Ok(Arc::new(h))
    }

    /// Connect, starting `exe --socket <sock> --history <history>` (stderr appended to `log`) when
    /// none answers.
    pub fn connect_or_launch(
        sock: &Path,
        exe: &Path,
        log: &Path,
        history: &Path,
    ) -> Result<Arc<Self>, KeltaError> {
        let h = Self {
            sock: sock.to_path_buf(),
            launch: Some((exe.to_path_buf(), log.to_path_buf(), history.to_path_buf())),
            conn: Mutex::new(None),
        };
        h.conn()?;
        Ok(Arc::new(h))
    }

    /// Drop the connection (sessions keep running in keltad).
    pub fn close(&self) {
        if let Some(c) = self.conn.lock().take() {
            c.closing.store(true, Ordering::SeqCst);
            let _ = c.w.lock().shutdown(std::net::Shutdown::Both);
        }
    }

    fn dial(&self) -> Result<UnixStream, KeltaError> {
        let first = UnixStream::connect(&self.sock);
        let s = match (first, &self.launch) {
            (Ok(s), _) => s,
            (Err(_), Some((exe, log, history))) => {
                if let Some(dir) = log.parent() {
                    std::fs::create_dir_all(dir)?;
                }
                let log = std::fs::OpenOptions::new().create(true).append(true).open(log)?;
                // keltad forks into the background once its socket is bound, so it is ready here.
                let status = kelta_proto::term::command(exe)
                    .arg("--socket")
                    .arg(&self.sock)
                    .arg("--history")
                    .arg(history)
                    .stdin(Stdio::null())
                    .stdout(Stdio::null())
                    .stderr(log)
                    .status()
                    .map_err(|e| KeltaError::not_found(format!("{}: {e}", exe.display())))?;
                if !status.success() {
                    tracing::warn!(%status, "keltad did not start (maybe another one won the race)");
                }
                UnixStream::connect(&self.sock)?
            }
            (Err(e), None) => return Err(e.into()),
        };
        // The socket sits in our 0700 dir; still check who answers.
        let uid = peer_uid(&s)?;
        if uid != kelta_proto::dirs::my_uid() {
            return Err(KeltaError::permission_denied(format!("keltad socket is served by uid {uid}")));
        }
        Ok(s)
    }

    /// The live connection, (re)connecting when keltad went away.
    fn conn(&self) -> Result<Arc<Conn>, KeltaError> {
        let mut slot = self.conn.lock();
        if let Some(c) = slot.as_ref().filter(|c| !c.dead.load(Ordering::SeqCst)) {
            return Ok(c.clone());
        }
        let mut s = self.dial()?;
        if let Err(e) = hello(&s) {
            // A keltad of another protocol (left by a previous Kelta) refuses us. Stop it rather
            // than run kept sessions in-process beside it: its PTYs get SIGHUP, core restores them.
            if e.code != ErrorCode::Unsupported || self.launch.is_none() {
                return Err(e);
            }
            tracing::warn!(error = %e, "replacing a keltad of another protocol");
            stop_peer(keltad_pid(&self.sock), &s)?;
            let _ = std::fs::remove_file(&self.sock);
            s = self.dial()?;
            hello(&s)?;
        }
        let reader = s.try_clone()?;
        let c = Arc::new(Conn {
            w: Mutex::new(s),
            seq: AtomicU64::new(0),
            pending: Mutex::default(),
            views: Mutex::default(),
            events: Mutex::default(),
            dead: AtomicBool::new(false),
            closing: AtomicBool::new(false),
        });
        // Events run on their own thread: core handlers may call back into this host, which
        // would deadlock on the reader thread that delivers the replies.
        let (ev_tx, ev_rx) = mpsc::channel::<EventJob>();
        std::thread::Builder::new().name("keltad-events".into()).stack_size(THREAD_STACK).spawn(
            move || {
                for (h, id, ev) in ev_rx {
                    h.on_event(&id, ev);
                }
            },
        )?;
        let rc = c.clone();
        std::thread::Builder::new()
            .name("keltad-client".into())
            .stack_size(THREAD_STACK)
            .spawn(move || rc.read_loop(reader, &ev_tx))?;
        *slot = Some(c.clone());
        Ok(c)
    }

    fn call<T: DeserializeOwned>(&self, req: Req, raw: &[u8]) -> Result<T, KeltaError> {
        let c = self.conn()?;
        let seq = c.next_seq();
        Ok(serde_json::from_value(c.call_seq(seq, &req, raw)?)?)
    }

    /// Fire-and-forget on the current connection only (never relaunches keltad).
    fn notify(&self, req: Req) {
        let c = self.conn.lock().clone();
        if let Some(c) = c.filter(|c| !c.dead.load(Ordering::SeqCst)) {
            let _ = c.send(0, &req, &[]);
        }
    }
}

impl Drop for DaemonTerminalHost {
    fn drop(&mut self) {
        self.close();
    }
}

impl TerminalHost for DaemonTerminalHost {
    fn spawn(&self, spec: PtySpawnSpec) -> Result<(), KeltaError> {
        let c = self.conn()?;
        let id = spec.id.clone();
        let prev = c.events.lock().insert(id.clone(), spec.events.clone());
        let req = Req::Spawn {
            spec: SpawnSpec {
                id: spec.id,
                program: spec.program,
                args: spec.args,
                cwd: spec.cwd,
                env: spec.env,
                cols: spec.cols,
                rows: spec.rows,
                scrollback_lines: spec.scrollback_lines,
                kind: spec.kind,
            },
        };
        let seq = c.next_seq();
        let r = c.call_seq(seq, &req, &[]);
        if let Err(e) = &r
            && e.code == ErrorCode::Timeout
            && prev.is_none()
        {
            // keltad may still finish the spawn: queued behind it, the kill leaves no session without a handler.
            // With a handler already there, that id runs: keltad refuses the spawn, the kill would end it.
            let _ = c.send(0, &Req::Kill { id: id.clone(), signal: KillSignal::Kill }, &[]);
        }
        if r.is_err() {
            let mut ev = c.events.lock();
            match prev {
                Some(p) => ev.insert(id, p),
                None => ev.remove(&id),
            };
        }
        r.map(drop)
    }

    fn attach(
        &self,
        id: &SessionId,
        cols: u16,
        rows: u16,
        sink: Box<dyn FrameSink>,
    ) -> Result<AttachInfo, KeltaError> {
        let c = self.conn()?;
        let token = c.next_seq();
        // Registered first: the snapshot frame arrives before the reply.
        c.views.lock().insert(id.clone(), View { token, generation: None, sink });
        let r = c
            .call_seq(token, &Req::Attach { id: id.clone(), cols, rows }, &[])
            .and_then(|v| Ok(serde_json::from_value::<AttachInfo>(v)?));
        let mut views = c.views.lock();
        let mine = views.get(id).is_some_and(|v| v.token == token);
        match &r {
            Ok(info) if mine => {
                if let Some(v) = views.get_mut(id) {
                    v.generation = Some(info.generation);
                }
            }
            // the view closed during the snapshot
            Ok(info) => {
                drop(views);
                let _ = c.send(0, &Req::Detach { id: id.clone(), generation: info.generation }, &[]);
            }
            Err(_) if mine => {
                views.remove(id);
            }
            Err(_) => {}
        }
        r
    }

    fn detach(&self, id: &SessionId, generation: u32) {
        if let Some(c) = self.conn.lock().as_ref() {
            let mut views = c.views.lock();
            if views.get(id).is_some_and(|v| v.generation == Some(generation)) {
                views.remove(id);
            }
        }
        self.notify(Req::Detach { id: id.clone(), generation });
    }

    fn write(&self, id: &SessionId, bytes: &[u8]) -> Result<(), KeltaError> {
        self.call::<Value>(Req::Write { id: id.clone() }, bytes).map(drop)
    }

    fn resize(&self, id: &SessionId, cols: u16, rows: u16) -> Result<(), KeltaError> {
        self.call::<Value>(Req::Resize { id: id.clone(), cols, rows }, &[]).map(drop)
    }

    fn ack(&self, id: &SessionId, generation: u32, bytes: u32) {
        self.notify(Req::Ack { id: id.clone(), generation, bytes });
    }

    fn kill(&self, id: &SessionId, signal: KillSignal) -> Result<(), KeltaError> {
        self.call::<Value>(Req::Kill { id: id.clone(), signal }, &[]).map(drop)
    }

    // shortcut: palette and limits are not replayed when keltad is relaunched after a crash; the next settings / theme change resends them.
    fn set_palette(&self, palette: TerminalPalette) {
        self.notify(Req::SetPalette { palette });
    }

    fn set_limits(&self, limits: TerminalLimits) {
        self.notify(Req::SetLimits { limits });
    }

    fn text_tail(&self, id: &SessionId, max_lines: u32) -> Result<String, KeltaError> {
        self.call(Req::TextTail { id: id.clone(), max_lines }, &[])
    }

    fn history_tail(&self, id: &SessionId, max_lines: u32) -> Result<String, KeltaError> {
        self.call(Req::HistoryTail { id: id.clone(), max_lines }, &[])
    }

    fn history_search(
        &self,
        ids: &[SessionId],
        query: &str,
        limit: u32,
    ) -> Result<Vec<HistoryHit>, KeltaError> {
        self.call(Req::HistorySearch { ids: ids.to_vec(), query: query.to_string(), limit }, &[])
    }

    fn history_delete(&self, id: &SessionId) {
        self.notify(Req::HistoryDelete { id: id.clone() });
    }

    fn stats(&self) -> TerminalStats {
        self.call(Req::Stats, &[]).unwrap_or_default()
    }

    fn persistent(&self) -> bool {
        true
    }

    fn shutdown(&self) {
        self.notify(Req::Shutdown);
    }

    fn adopt(
        &self,
        id: &SessionId,
        events: Arc<dyn TerminalEvents>,
    ) -> Result<Option<BTreeMap<String, String>>, KeltaError> {
        let c = self.conn()?;
        // Registered first: events flow as soon as keltad re-routes them.
        c.events.lock().insert(id.clone(), events);
        let seq = c.next_seq();
        let env = c
            .call_seq(seq, &Req::Adopt { id: id.clone() }, &[])
            .and_then(|v| Ok(serde_json::from_value::<Option<BTreeMap<String, String>>>(v)?));
        if !matches!(env, Ok(Some(_))) {
            c.events.lock().remove(id);
        }
        env
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::disallowed_methods)] // allowlisted: test threads
    use std::os::unix::net::UnixListener;
    use std::process::Command;

    use super::super::server::{bind, serve};
    use super::*;
    use crate::PtyTerminalHost;

    struct NoEvents;
    impl TerminalEvents for NoEvents {
        fn on_event(&self, _: &SessionId, _: TerminalEvent) {}
    }

    fn cat(id: &SessionId, events: Arc<dyn TerminalEvents>) -> PtySpawnSpec {
        PtySpawnSpec {
            id: id.clone(),
            program: "/bin/cat".into(),
            args: Vec::new(),
            cwd: "/".into(),
            env: BTreeMap::new(),
            cols: 80,
            rows: 24,
            scrollback_lines: 0,
            kind: kelta_proto::model::SessionKind::Shell,
            events,
        }
    }

    /// #309: the SIGTERM'd keltad can be gone before stop_peer waits on its socket. macOS then fails
    /// setsockopt with EINVAL: that means stopped, not an InvalidArgument for connect_or_launch.
    #[test]
    fn stop_peer_accepts_a_keltad_already_gone() {
        let tmp = tempfile::tempdir().unwrap();
        let sock = tmp.path().join("keltad.sock");
        let fake = tmp.path().join("keltad");
        std::os::unix::fs::symlink("/bin/sleep", &fake).unwrap();
        let mut old = Command::new(&fake).arg("30").spawn().unwrap();
        std::fs::write(pidfile(&sock), old.id().to_string()).unwrap();
        let (s, peer) = UnixStream::pair().unwrap();
        drop(peer);
        stop_peer(keltad_pid(&sock), &s).unwrap();
        old.wait().unwrap();
    }

    /// #312: the old keltad can exit before its pid is looked up (no pid) or before the kill (ESRCH).
    #[test]
    fn stop_peer_accepts_a_keltad_gone_before_the_kill() {
        let tmp = tempfile::tempdir().unwrap();
        let (s, peer) = UnixStream::pair().unwrap();
        drop(peer);
        assert_eq!(keltad_pid(&tmp.path().join("keltad.sock")), None);
        stop_peer(None, &s).unwrap();
        let mut reaped = Command::new("true").spawn().unwrap();
        let pid = reaped.id() as i32;
        reaped.wait().unwrap();
        stop_peer(Some(pid), &s).unwrap();
    }

    /// #309: lsof also lists sockets of other keltads whose deleted files had the same inode.
    #[test]
    fn lsof_holders_keeps_only_sockets_bound_to_the_path() {
        let linux = "p10\nn/tmp/a/run/keltad.sock type=STREAM\np20\nf3\nn/tmp/b/run/keltad.sock type=STREAM\n\
                     n/tmp/b/run/keltad.sock type=STREAM\np30\nn/tmp/b/run/keltad.sock.old type=STREAM\n";
        assert_eq!(lsof_holders(linux, Path::new("/tmp/b/run/keltad.sock")), [20]);
        let macos = "p7\nf3\nn/tmp/b/keltad.sock\nf5\nn/tmp/b/keltad.sock\np8\nn/tmp/b/keltad.sock x\n";
        assert_eq!(lsof_holders(macos, Path::new("/tmp/b/keltad.sock")), [7]);
    }

    /// #299: a spawn keltad does not answer in time fails with Timeout, rolls its handler back, and
    /// the kill queued behind it leaves no running session behind.
    #[test]
    fn a_timed_out_spawn_is_killed_and_rolled_back() {
        timed_out_spawn(false);
    }

    /// #313: a timed-out spawn of an id that already runs leaves that session running.
    #[test]
    fn a_timed_out_spawn_spares_the_running_session() {
        timed_out_spawn(true);
    }

    fn timed_out_spawn(running: bool) {
        let tmp = tempfile::tempdir().unwrap();
        let real = tmp.path().join("run/keltad.sock");
        let l = bind(&real).unwrap();
        std::thread::spawn(move || {
            serve(l, PtyTerminalHost::new(TerminalLimits::default()), Duration::from_secs(60))
        });
        // A peer between the client and keltad: it answers Hello, then holds the Spawn (and the
        // Kill that follows the timeout) until CALL_TIMEOUT has passed, and only then runs both.
        let fake = tmp.path().join("fake.sock");
        let fl = UnixListener::bind(&fake).unwrap();
        let peer = std::thread::spawn(move || {
            let (mut a, _) = fl.accept().unwrap();
            let hello = read_msg(&mut a).unwrap().unwrap();
            let ok = Out::Reply { seq: 1, result: Ok(Value::from(PROTOCOL_VERSION)) };
            a.write_all(&encode(&ok, &[]).unwrap()).unwrap();
            let spawn = read_msg(&mut a).unwrap().unwrap();
            // No kill after the timeout is the failure under test: fail on the poll below, not here.
            a.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
            let kill = read_msg(&mut a).ok().flatten();
            let mut b = UnixStream::connect(&real).unwrap();
            let sync = encode(&Call { seq: 99, req: Req::Stats }, &[]).unwrap();
            for body in [Some(hello), Some(spawn), kill].into_iter().flatten() {
                b.write_all(&(body.len() as u32).to_le_bytes()).unwrap();
                b.write_all(&body).unwrap();
            }
            b.write_all(&sync).unwrap();
            // keltad answers in order: the Spawn and the Kill are handled once the Stats reply is in.
            let mut r = BufReader::new(b.try_clone().unwrap());
            loop {
                let body = read_msg(&mut r).unwrap().unwrap();
                if matches!(decode::<Out>(&body), Some((Out::Reply { seq: 99, .. }, _))) {
                    return b;
                }
            }
        });
        let d = DaemonTerminalHost::connect(&tmp.path().join("run/keltad.sock")).unwrap();
        let c = DaemonTerminalHost::connect(&fake).unwrap();
        let id = SessionId::new("hung");
        if running {
            // The session this client already runs under that id.
            let prev: Arc<dyn TerminalEvents> = Arc::new(NoEvents);
            d.spawn(cat(&id, Arc::new(NoEvents))).unwrap();
            c.conn().unwrap().events.lock().insert(id.clone(), prev.clone());
            assert_eq!(c.spawn(cat(&id, Arc::new(NoEvents))).unwrap_err().code, ErrorCode::Timeout);
            assert!(Arc::ptr_eq(&c.conn().unwrap().events.lock()[&id], &prev), "handler not rolled back");
            let _b = peer.join().unwrap();
            // A kill would end it within moments (the poll below): it must stay.
            for _ in 0..10 {
                std::thread::sleep(Duration::from_millis(50));
                let e = d.spawn(cat(&id, Arc::new(NoEvents))).unwrap_err();
                assert_eq!(e.code, ErrorCode::Conflict, "the running session was killed: {e:?}");
            }
            d.kill(&id, KillSignal::Kill).unwrap();
            return;
        }
        assert_eq!(c.spawn(cat(&id, Arc::new(NoEvents))).unwrap_err().code, ErrorCode::Timeout);
        assert!(!c.conn().unwrap().events.lock().contains_key(&id), "handler not rolled back");
        let _b = peer.join().unwrap();
        // The kill is only a signal: the exit is noticed later, so poll until a new spawn is accepted.
        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        loop {
            match d.spawn(cat(&id, Arc::new(NoEvents))) {
                Ok(()) => break,
                Err(e) if e.code == ErrorCode::Conflict && std::time::Instant::now() < deadline => {
                    std::thread::sleep(Duration::from_millis(50));
                }
                Err(e) => panic!("the timed-out spawn was left running: {e:?}"),
            }
        }
        d.kill(&id, KillSignal::Kill).unwrap();
    }
}

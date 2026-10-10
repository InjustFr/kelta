//! App side: a `TerminalHost` forwarding every call to keltad. It holds no terminal model (the
//! memory stays in keltad), only the routing of frames to views and events to core.

use std::collections::{BTreeMap, HashMap};
use std::io::{BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, Sender, SyncSender};
use std::time::Duration;

use kelta_proto::api::{FrameSink, TerminalEvents, TerminalHost};
use kelta_proto::error::KeltaError;
use kelta_proto::ids::SessionId;
use kelta_proto::model::AttachInfo;
use kelta_proto::term::{
    KillSignal, PtySpawnSpec, TerminalEvent, TerminalLimits, TerminalPalette, TerminalStats,
};
use parking_lot::Mutex;
use serde::de::DeserializeOwned;
use serde_json::Value;

use super::{Call, Out, PROTOCOL_VERSION, Req, SpawnSpec, decode, encode, peer_uid, read_msg};

const CALL_TIMEOUT: Duration = Duration::from_secs(10);
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

/// `TerminalHost` backed by keltad.
pub struct DaemonTerminalHost {
    sock: PathBuf,
    /// keltad binary + its log file; `None` = never launch (tests).
    launch: Option<(PathBuf, PathBuf)>,
    conn: Mutex<Option<Arc<Conn>>>,
}

impl DaemonTerminalHost {
    /// Connect to a running keltad.
    pub fn connect(sock: &Path) -> Result<Arc<Self>, KeltaError> {
        let h = Self { sock: sock.to_path_buf(), launch: None, conn: Mutex::new(None) };
        h.conn()?;
        Ok(Arc::new(h))
    }

    /// Connect, starting `exe --socket <sock>` (stderr appended to `log`) when none answers.
    pub fn connect_or_launch(sock: &Path, exe: &Path, log: &Path) -> Result<Arc<Self>, KeltaError> {
        let h = Self {
            sock: sock.to_path_buf(),
            launch: Some((exe.to_path_buf(), log.to_path_buf())),
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
            (Err(_), Some((exe, log))) => {
                if let Some(dir) = log.parent() {
                    std::fs::create_dir_all(dir)?;
                }
                let log = std::fs::OpenOptions::new().create(true).append(true).open(log)?;
                // keltad forks into the background once its socket is bound, so it is ready here.
                let status = Command::new(exe)
                    .arg("--socket")
                    .arg(&self.sock)
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
        let s = self.dial()?;
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
        let seq = c.next_seq();
        if let Err(e) = c.call_seq(seq, &Req::Hello { v: PROTOCOL_VERSION }, &[]) {
            // shortcut: an older keltad left running by a previous version keeps its sessions unreachable until they exit, and core respawns the kept ones from Dormant beside them (duplicate Claude --resume); kill the stale keltad's sessions here if that bites.
            c.closing.store(true, Ordering::SeqCst);
            let _ = c.w.lock().shutdown(std::net::Shutdown::Both);
            return Err(e);
        }
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

    fn stats(&self) -> TerminalStats {
        self.call(Req::Stats, &[]).unwrap_or_default()
    }

    fn persistent(&self) -> bool {
        true
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

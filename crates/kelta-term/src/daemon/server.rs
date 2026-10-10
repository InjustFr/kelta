//! keltad side: one [`PtyTerminalHost`], a thread per client connection, events routed to the
//! client that spawned or last adopted each session. Exits after `idle_grace` with no client and no
//! running session (the grace is armed by the disconnect / exit that made it idle).

use std::collections::{BTreeMap, HashMap};
use std::io::{BufReader, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Sender};
use std::time::Duration;

use kelta_proto::api::{FrameSink, TerminalEvents, TerminalHost};
use kelta_proto::error::KeltaError;
use kelta_proto::ids::SessionId;
use kelta_proto::term::{PtySpawnSpec, TerminalEvent};
use parking_lot::{Condvar, Mutex};
use serde_json::Value;

use super::{Call, Out, PROTOCOL_VERSION, Req, decode, encode, peer_uid, read_msg};
use crate::PtyTerminalHost;

const THREAD_STACK: usize = 256 * 1024;

/// Bind `<runtime>/keltad.sock`: 0600 in a private 0700 directory owned by us.
pub fn bind(path: &Path) -> Result<UnixListener, KeltaError> {
    kelta_proto::dirs::bind_private_socket(path)
}

#[derive(Default)]
struct Idle {
    st: Mutex<(usize, usize)>, // (clients, running sessions)
    cv: Condvar,
}

impl Idle {
    fn update(&self, f: impl FnOnce(&mut (usize, usize))) {
        f(&mut self.st.lock());
        self.cv.notify_all();
    }

    fn wait(&self, grace: Duration) {
        let mut st = self.st.lock();
        loop {
            if *st != (0, 0) {
                self.cv.wait(&mut st);
                continue;
            }
            // one-shot: idle grace, armed by the disconnect / exit that left nothing to serve.
            if self.cv.wait_for(&mut st, grace).timed_out() && *st == (0, 0) {
                return;
            }
        }
    }
}

/// Per-session route: where its events go, plus what an adopting client needs.
struct Route {
    out: Mutex<Option<Sender<Vec<u8>>>>,
    env: BTreeMap<String, String>,
    running: AtomicBool,
    idle: Arc<Idle>,
}

impl Route {
    fn exited(&self) {
        if self.running.swap(false, Ordering::SeqCst) {
            self.idle.update(|s| s.1 -= 1);
        }
    }
}

impl TerminalEvents for Route {
    fn on_event(&self, id: &SessionId, ev: TerminalEvent) {
        if matches!(ev, TerminalEvent::Exited { .. }) {
            self.exited();
        }
        let mut out = self.out.lock();
        // shortcut: events while no client owns the session are dropped (title, OSC 7 cwd), resent only by the program.
        let Some(tx) = out.as_ref() else { return };
        let sent = encode(&Out::Event { id: id.clone(), ev }, &[]).is_ok_and(|m| tx.send(m).is_ok());
        if !sent {
            *out = None;
        }
    }
}

/// Frames of one attached view, tagged with the attach call's seq.
struct ConnSink {
    tx: Sender<Vec<u8>>,
    id: SessionId,
    token: u64,
}

impl FrameSink for ConnSink {
    fn send(&mut self, frame: Vec<u8>) -> bool {
        encode(&Out::Frame { id: self.id.clone(), token: self.token }, &frame)
            .is_ok_and(|m| self.tx.send(m).is_ok())
    }
}

struct Daemon {
    host: PtyTerminalHost,
    routes: Mutex<HashMap<SessionId, Arc<Route>>>,
    idle: Arc<Idle>,
    /// Only this uid may connect (ours).
    uid: u32,
}

/// Serve `listener` until idle, then remove the socket file and return.
pub fn serve(listener: UnixListener, host: PtyTerminalHost, idle_grace: Duration) {
    serve_as(listener, host, idle_grace, kelta_proto::dirs::my_uid());
}

fn serve_as(listener: UnixListener, host: PtyTerminalHost, idle_grace: Duration, uid: u32) {
    let path = listener.local_addr().ok().and_then(|a| a.as_pathname().map(Path::to_path_buf));
    let d = Arc::new(Daemon { host, routes: Mutex::default(), idle: Arc::default(), uid });
    let acceptor = d.clone();
    let spawned =
        std::thread::Builder::new().name("keltad-accept".into()).stack_size(THREAD_STACK).spawn(move || {
            for stream in listener.incoming() {
                match stream {
                    Ok(s) => {
                        let d = acceptor.clone();
                        let r = std::thread::Builder::new()
                            .name("keltad-conn".into())
                            .stack_size(THREAD_STACK)
                            .spawn(move || d.serve_conn(s));
                        if let Err(e) = r {
                            tracing::warn!(error = %e, "keltad: cannot start a connection thread");
                        }
                    }
                    Err(e) => tracing::warn!(error = %e, "keltad: accept failed"),
                }
            }
        });
    if let Err(e) = spawned {
        tracing::error!(error = %e, "keltad: cannot start the accept thread");
        return;
    }
    d.idle.wait(idle_grace);
    tracing::info!("keltad: idle, exiting");
    if let Some(p) = path {
        let _ = std::fs::remove_file(p);
    }
}

impl Daemon {
    fn serve_conn(&self, stream: UnixStream) {
        match peer_uid(&stream) {
            Ok(u) if u == self.uid => {}
            Ok(u) => return tracing::warn!(peer_uid = u, "keltad: rejected connection from another uid"),
            Err(e) => return tracing::warn!(error = %e, "keltad: no peer credentials; rejected"),
        }
        let mut w = match stream.try_clone() {
            Ok(w) => w,
            Err(e) => return tracing::warn!(error = %e, "keltad: cannot clone the connection"),
        };
        let (tx, rx) = mpsc::channel::<Vec<u8>>();
        // Writer: an empty message (sent when the reader ends) stops it, so frames / events
        // queued for a gone client fail fast and their sinks auto-detach.
        let writer = std::thread::Builder::new().name("keltad-write".into()).stack_size(THREAD_STACK).spawn(
            move || {
                for m in rx {
                    if m.is_empty() || w.write_all(&m).is_err() {
                        break;
                    }
                }
            },
        );
        if let Err(e) = writer {
            return tracing::warn!(error = %e, "keltad: cannot start a writer thread");
        }
        self.idle.update(|s| s.0 += 1);
        let mut r = BufReader::new(stream);
        while let Ok(Some(body)) = read_msg(&mut r) {
            let Some((call, raw)) = decode::<Call>(&body) else {
                tracing::warn!("keltad: malformed message; closing the connection");
                break;
            };
            let seq = call.seq;
            let result = self.handle(&tx, seq, call.req, raw);
            if seq != 0 {
                match encode(&Out::Reply { seq, result }, &[]) {
                    Ok(m) => {
                        let _ = tx.send(m);
                    }
                    Err(e) => tracing::warn!(error = %e, "keltad: reply not encodable"),
                }
            }
        }
        let _ = tx.send(Vec::new());
        self.idle.update(|s| s.0 -= 1);
    }

    fn handle(&self, tx: &Sender<Vec<u8>>, seq: u64, req: Req, raw: &[u8]) -> Result<Value, KeltaError> {
        let h = &self.host;
        match req {
            Req::Hello { v } if v == PROTOCOL_VERSION => Ok(Value::from(PROTOCOL_VERSION)),
            Req::Hello { v } => Err(KeltaError::unsupported(format!(
                "keltad speaks protocol {PROTOCOL_VERSION}, the app {v}"
            ))),
            Req::Spawn { spec } => {
                let route = Arc::new(Route {
                    out: Mutex::new(Some(tx.clone())),
                    env: spec.env.clone(),
                    running: AtomicBool::new(true),
                    idle: self.idle.clone(),
                });
                self.idle.update(|s| s.1 += 1);
                let id = spec.id.clone();
                let r = h.spawn(PtySpawnSpec {
                    id: spec.id,
                    program: spec.program,
                    args: spec.args,
                    cwd: spec.cwd,
                    env: spec.env,
                    cols: spec.cols,
                    rows: spec.rows,
                    scrollback_lines: spec.scrollback_lines,
                    kind: spec.kind,
                    events: route.clone(),
                });
                match r {
                    Ok(()) => {
                        self.routes.lock().insert(id, route);
                        Ok(Value::Null)
                    }
                    Err(e) => {
                        route.exited();
                        Err(e)
                    }
                }
            }
            Req::Adopt { id } => {
                let routes = self.routes.lock();
                let Some(r) = routes.get(&id).filter(|r| r.running.load(Ordering::SeqCst)) else {
                    return Ok(Value::Null);
                };
                *r.out.lock() = Some(tx.clone());
                Ok(serde_json::to_value(&r.env)?)
            }
            Req::Attach { id, cols, rows } => {
                let sink = ConnSink { tx: tx.clone(), id: id.clone(), token: seq };
                Ok(serde_json::to_value(h.attach(&id, cols, rows, Box::new(sink))?)?)
            }
            Req::Detach { id, generation } => {
                h.detach(&id, generation);
                Ok(Value::Null)
            }
            Req::Write { id } => h.write(&id, raw).map(|()| Value::Null),
            Req::Resize { id, cols, rows } => h.resize(&id, cols, rows).map(|()| Value::Null),
            Req::Ack { id, generation, bytes } => {
                h.ack(&id, generation, bytes);
                Ok(Value::Null)
            }
            Req::Kill { id, signal } => {
                h.kill(&id, signal)?;
                // Killing an exited session closed it in the host.
                let mut routes = self.routes.lock();
                if routes.get(&id).is_some_and(|r| !r.running.load(Ordering::SeqCst)) {
                    routes.remove(&id);
                }
                Ok(Value::Null)
            }
            Req::SetPalette { palette } => {
                h.set_palette(palette);
                Ok(Value::Null)
            }
            Req::SetLimits { limits } => {
                h.set_limits(limits);
                Ok(Value::Null)
            }
            Req::TextTail { id, max_lines } => h.text_tail(&id, max_lines).map(Value::from),
            Req::HistoryTail { id, max_lines } => h.history_tail(&id, max_lines).map(Value::from),
            Req::HistorySearch { ids, query, limit } => {
                Ok(serde_json::to_value(h.history_search(&ids, &query, limit)?)?)
            }
            Req::HistoryDelete { id } => {
                h.history_delete(&id);
                Ok(Value::Null)
            }
            Req::Stats => Ok(serde_json::to_value(h.stats())?),
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::disallowed_methods)] // allowlisted: test threads
    use std::io::Read;
    use std::os::unix::fs::PermissionsExt;

    use kelta_proto::term::TerminalLimits;

    use super::*;

    fn host() -> PtyTerminalHost {
        PtyTerminalHost::new(TerminalLimits::default())
    }

    #[test]
    fn socket_is_private() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("run");
        std::fs::create_dir(&dir).unwrap();
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o755)).unwrap();
        let sock = dir.join("keltad.sock");
        let _l = bind(&sock).unwrap();
        assert_eq!(std::fs::metadata(&dir).unwrap().permissions().mode() & 0o777, 0o700);
        assert_eq!(std::fs::metadata(&sock).unwrap().permissions().mode() & 0o777, 0o600);
        // a live daemon is never replaced
        assert!(bind(&sock).is_err());
        // a symlinked runtime dir is refused
        let link = tmp.path().join("link");
        std::os::unix::fs::symlink(&dir, &link).unwrap();
        assert!(bind(&link.join("other.sock")).is_err());
    }

    #[test]
    fn rejects_another_uid() {
        let tmp = tempfile::tempdir().unwrap();
        let sock = tmp.path().join("run/keltad.sock");
        let l = bind(&sock).unwrap();
        let other = kelta_proto::dirs::my_uid() + 1;
        std::thread::spawn(move || serve_as(l, host(), Duration::from_secs(30), other));
        let mut s = UnixStream::connect(&sock).unwrap();
        // Linux may reset the connection (unread data), macOS closes it: either way no reply.
        let _ = s.write_all(&encode(&Call { seq: 1, req: Req::Hello { v: PROTOCOL_VERSION } }, &[]).unwrap());
        s.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
        let mut buf = Vec::new();
        let _ = s.read_to_end(&mut buf);
        assert!(buf.is_empty(), "rejected peer got a reply");
    }

    #[test]
    fn exits_when_idle() {
        let tmp = tempfile::tempdir().unwrap();
        let sock = tmp.path().join("run/keltad.sock");
        let l = bind(&sock).unwrap();
        let t = std::thread::spawn(move || serve(l, host(), Duration::from_millis(100)));
        let s = UnixStream::connect(&sock).unwrap();
        std::thread::sleep(Duration::from_millis(300));
        assert!(!t.is_finished(), "a connected client keeps keltad alive");
        drop(s);
        t.join().unwrap();
        assert!(!sock.exists());
    }
}

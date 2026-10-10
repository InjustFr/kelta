//! keltad (ARCHITECTURE D3): a small process that owns the PTYs and terminal models (a
//! [`PtyTerminalHost`](crate::PtyTerminalHost)) so sessions survive quitting the app, and the
//! [`DaemonTerminalHost`] the app uses in its place.
//!
//! Transport: `<runtime>/keltad.sock` (0600 in a 0700 dir, peer uid checked on both ends).
//! Message: `u32 LE len` + `u32 LE json_len` + JSON head + raw bytes (PTY input, terminal frames),
//! so frames cross the socket without re-encoding.
//! Client → daemon: [`Call`] (`seq` 0 = no reply). Daemon → client: [`Out`].

mod client;
mod server;

use std::collections::BTreeMap;
use std::io::{self, Read};
use std::os::fd::AsFd;
use std::path::PathBuf;

use kelta_proto::error::KeltaError;
use kelta_proto::ids::SessionId;
use kelta_proto::model::SessionKind;
use kelta_proto::term::{KillSignal, TerminalEvent, TerminalLimits, TerminalPalette};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

pub use client::DaemonTerminalHost;
pub use server::{bind, serve};

/// Bumped on any incompatible change of [`Req`] / [`Out`].
pub const PROTOCOL_VERSION: u32 = 1;
/// Largest accepted message (a full snapshot with 10k history lines fits easily).
const MAX_MSG: usize = 64 << 20;

/// `PtySpawnSpec` without the events sink (the daemon routes events to the owning client).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SpawnSpec {
    pub id: SessionId,
    pub program: PathBuf,
    pub args: Vec<String>,
    pub cwd: PathBuf,
    pub env: BTreeMap<String, String>,
    pub cols: u16,
    pub rows: u16,
    pub scrollback_lines: u32,
    pub kind: SessionKind,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum Req {
    Hello {
        v: u32,
    },
    Spawn {
        spec: SpawnSpec,
    },
    /// Route a running session's events to this client; replies its spawn env or null.
    Adopt {
        id: SessionId,
    },
    /// Frames follow as [`Out::Frame`] tagged with this call's `seq`.
    Attach {
        id: SessionId,
        cols: u16,
        rows: u16,
    },
    Detach {
        id: SessionId,
        generation: u32,
    },
    /// Bytes in the raw part.
    Write {
        id: SessionId,
    },
    Resize {
        id: SessionId,
        cols: u16,
        rows: u16,
    },
    Ack {
        id: SessionId,
        generation: u32,
        bytes: u32,
    },
    Kill {
        id: SessionId,
        signal: KillSignal,
    },
    SetPalette {
        palette: TerminalPalette,
    },
    SetLimits {
        limits: TerminalLimits,
    },
    TextTail {
        id: SessionId,
        max_lines: u32,
    },
    HistoryTail {
        id: SessionId,
        max_lines: u32,
    },
    HistorySearch {
        ids: Vec<SessionId>,
        query: String,
        limit: u32,
    },
    HistoryDelete {
        id: SessionId,
    },
    Stats,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Call {
    pub seq: u64,
    pub req: Req,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "t", rename_all = "snake_case")]
pub enum Out {
    Reply {
        seq: u64,
        result: Result<serde_json::Value, KeltaError>,
    },
    /// Terminal frame (raw part) for the view attached by call `token`.
    Frame {
        id: SessionId,
        token: u64,
    },
    Event {
        id: SessionId,
        ev: TerminalEvent,
    },
}

/// Encode a message: length prefix, JSON head, raw tail.
pub fn encode<T: Serialize>(head: &T, raw: &[u8]) -> Result<Vec<u8>, KeltaError> {
    let json = serde_json::to_vec(head)?;
    let total = 4 + json.len() + raw.len();
    if total > MAX_MSG {
        return Err(KeltaError::invalid(format!("keltad message too large ({total} bytes)")));
    }
    let mut v = Vec::with_capacity(4 + total);
    v.extend_from_slice(&(total as u32).to_le_bytes());
    v.extend_from_slice(&(json.len() as u32).to_le_bytes());
    v.extend_from_slice(&json);
    v.extend_from_slice(raw);
    Ok(v)
}

/// Read one message body (after the length prefix); `None` on a clean EOF.
pub fn read_msg(r: &mut impl Read) -> io::Result<Option<Vec<u8>>> {
    let mut len = [0u8; 4];
    match r.read_exact(&mut len) {
        Ok(()) => {}
        Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => return Ok(None),
        Err(e) => return Err(e),
    }
    let len = u32::from_le_bytes(len) as usize;
    if !(4..=MAX_MSG).contains(&len) {
        return Err(io::Error::new(io::ErrorKind::InvalidData, format!("bad keltad message length {len}")));
    }
    let mut body = vec![0u8; len];
    r.read_exact(&mut body)?;
    Ok(Some(body))
}

/// Split a body into its decoded head and raw tail; `None` when malformed.
pub fn decode<T: DeserializeOwned>(body: &[u8]) -> Option<(T, &[u8])> {
    let (n, rest) = body.split_first_chunk::<4>()?;
    let n = u32::from_le_bytes(*n) as usize;
    if n > rest.len() {
        return None;
    }
    let (json, raw) = rest.split_at(n);
    Some((serde_json::from_slice(json).ok()?, raw))
}

/// Uid of the process on the other end of a unix socket.
pub fn peer_uid(fd: impl AsFd) -> io::Result<u32> {
    #[cfg(target_os = "linux")]
    {
        Ok(rustix::net::sockopt::socket_peercred(fd)?.uid.as_raw())
    }
    #[cfg(not(target_os = "linux"))]
    {
        use std::os::fd::AsRawFd;
        let (mut uid, mut gid) = (0, 0);
        // SAFETY: valid socket fd for the duration of the call, out-params point to locals.
        if unsafe { libc::getpeereid(fd.as_fd().as_raw_fd(), &mut uid, &mut gid) } != 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(uid)
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    fn round_trip<T: Serialize + DeserializeOwned + PartialEq + std::fmt::Debug>(head: T, raw: &[u8]) {
        let msg = encode(&head, raw).unwrap();
        let body = read_msg(&mut msg.as_slice()).unwrap().unwrap();
        let (back, tail) = decode::<T>(&body).unwrap();
        assert_eq!(back, head);
        assert_eq!(tail, raw);
    }

    #[test]
    fn protocol_round_trip() {
        let id = SessionId::new("0190a1b2-c3d4-7e5f-8a9b-0c1d2e3f4a5b");
        let spec = SpawnSpec {
            id: id.clone(),
            program: "/bin/sh".into(),
            args: vec!["-l".into()],
            cwd: "/tmp".into(),
            env: BTreeMap::from([("A".into(), "b".into())]),
            cols: 80,
            rows: 24,
            scrollback_lines: 100,
            kind: SessionKind::Editor { adapter: "nvim".into() },
        };
        for req in [
            Req::Hello { v: PROTOCOL_VERSION },
            Req::Spawn { spec },
            Req::Adopt { id: id.clone() },
            Req::Attach { id: id.clone(), cols: 1, rows: 2 },
            Req::Detach { id: id.clone(), generation: 3 },
            Req::Resize { id: id.clone(), cols: 4, rows: 5 },
            Req::Ack { id: id.clone(), generation: 6, bytes: 7 },
            Req::Kill { id: id.clone(), signal: KillSignal::Hup },
            Req::SetPalette { palette: TerminalPalette::default() },
            Req::SetLimits { limits: TerminalLimits::default() },
            Req::TextTail { id: id.clone(), max_lines: 9 },
            Req::Stats,
        ] {
            round_trip(Call { seq: 42, req }, b"");
        }
        round_trip(Call { seq: 0, req: Req::Write { id: id.clone() } }, b"ls\r\x00\xff");
        round_trip(Out::Frame { id: id.clone(), token: 7 }, &crate::frames::data(b"\x1b[31mred"));
        round_trip(Out::Reply { seq: 1, result: Ok(serde_json::json!({"generation": 1})) }, b"");
        round_trip(Out::Reply { seq: 2, result: Err(KeltaError::not_found("terminal session x")) }, b"");
        for ev in [
            TerminalEvent::Title("t".into()),
            TerminalEvent::Cwd("/x".into()),
            TerminalEvent::Bell,
            TerminalEvent::Exited { code: Some(1), signal: None },
            TerminalEvent::Notify { title: None, body: "b".into() },
        ] {
            round_trip(Out::Event { id: id.clone(), ev }, b"");
        }
    }

    #[test]
    fn rejects_malformed_and_oversized() {
        assert!(read_msg(&mut &[][..]).unwrap().is_none());
        let huge = ((MAX_MSG + 1) as u32).to_le_bytes();
        assert!(read_msg(&mut &huge[..]).is_err());
        assert!(read_msg(&mut &[2, 0, 0, 0, 0, 0][..]).is_err());
        assert!(decode::<Call>(&[9, 0, 0, 0, b'{']).is_none());
        assert!(decode::<Call>(b"\x02\x00\x00\x00{}").is_none());
    }
}

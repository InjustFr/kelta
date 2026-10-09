//! Small shared helpers: hashing, process-group signals, bounded byte ring, JSON paths.

use std::collections::VecDeque;
use std::path::Path;

use kelta_proto::error::KeltaError;
use serde_json::Value;
use sha2::{Digest, Sha256};

/// Lower-case hex SHA-256.
pub fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut out = String::with_capacity(64);
    for b in digest {
        out.push_str(&format!("{b:02x}"));
    }
    out
}

/// `io::Error` → `KeltaError::internal` with context.
pub fn io_err(context: impl std::fmt::Display, e: std::io::Error) -> KeltaError {
    if e.kind() == std::io::ErrorKind::NotFound {
        KeltaError::not_found(format!("{context}: {e}"))
    } else {
        KeltaError::internal(format!("{context}: {e}"))
    }
}

/// Parse a signal name (`TERM`, `SIGTERM`, `INT`, `HUP`, `KILL`, `QUIT`, `USR1`, `USR2`).
pub fn parse_signal(name: &str) -> Option<rustix::process::Signal> {
    use rustix::process::Signal;
    let n = name.trim().to_ascii_uppercase();
    let n = n.strip_prefix("SIG").unwrap_or(&n);
    Some(match n {
        "TERM" => Signal::TERM,
        "INT" => Signal::INT,
        "HUP" => Signal::HUP,
        "KILL" => Signal::KILL,
        "QUIT" => Signal::QUIT,
        "USR1" => Signal::USR1,
        "USR2" => Signal::USR2,
        _ => return None,
    })
}

/// Send `sig` to the process group led by `pid` (children are spawned with `process_group(0)`).
/// Falls back to the single process when the group is gone. Errors are ignored (already dead).
pub fn signal_group(pid: u32, sig: rustix::process::Signal) {
    let Ok(raw) = i32::try_from(pid) else { return };
    let Some(pid) = rustix::process::Pid::from_raw(raw) else { return };
    if rustix::process::kill_process_group(pid, sig).is_err() {
        let _ = rustix::process::kill_process(pid, sig);
    }
}

/// Bounded byte ring (web tool logs, 64 KiB).
#[derive(Debug)]
pub struct ByteRing {
    buf: VecDeque<u8>,
    cap: usize,
}

impl ByteRing {
    pub fn new(cap: usize) -> Self {
        Self { buf: VecDeque::with_capacity(cap.min(4096)), cap }
    }

    pub fn push(&mut self, bytes: &[u8]) {
        if bytes.len() >= self.cap {
            self.buf.clear();
            self.buf.extend(&bytes[bytes.len() - self.cap..]);
            return;
        }
        let overflow = (self.buf.len() + bytes.len()).saturating_sub(self.cap);
        self.buf.drain(..overflow);
        self.buf.extend(bytes);
    }

    pub fn text(&self) -> String {
        let (a, b) = self.buf.as_slices();
        let mut v = Vec::with_capacity(a.len() + b.len());
        v.extend_from_slice(a);
        v.extend_from_slice(b);
        String::from_utf8_lossy(&v).into_owned()
    }

    /// Last `n` lines.
    pub fn tail_lines(&self, n: usize) -> String {
        let text = self.text();
        let lines: Vec<&str> = text.lines().collect();
        let start = lines.len().saturating_sub(n);
        lines[start..].join("\n")
    }

    pub fn len(&self) -> usize {
        self.buf.len()
    }

    pub fn is_empty(&self) -> bool {
        self.buf.is_empty()
    }
}

/// Lookup a dotted path (`a.b.0.c`) in a JSON value. `None` when any segment is missing.
pub fn json_path<'a>(root: &'a Value, path: &str) -> Option<&'a Value> {
    let mut cur = root;
    for seg in path.split('.') {
        if seg.is_empty() {
            return None;
        }
        cur = match cur {
            Value::Object(m) => m.get(seg)?,
            Value::Array(a) => a.get(seg.parse::<usize>().ok()?)?,
            _ => return None,
        };
    }
    Some(cur)
}

/// String form of a JSON scalar (`null` → empty; objects/arrays → compact JSON).
pub fn json_to_string(v: &Value) -> String {
    match v {
        Value::Null => String::new(),
        Value::String(s) => s.clone(),
        Value::Bool(b) => b.to_string(),
        Value::Number(n) => n.to_string(),
        other => other.to_string(),
    }
}

/// `basename(argv0)`.
pub fn basename(cmd: &str) -> &str {
    Path::new(cmd).file_name().and_then(|s| s.to_str()).unwrap_or(cmd)
}

/// True for `127.0.0.1`, `::1`, `[::1]` and `localhost`.
pub fn is_loopback_host(host: &str) -> bool {
    let h = host.trim_start_matches('[').trim_end_matches(']').to_ascii_lowercase();
    h == "localhost"
        || h.ends_with(".localhost")
        || h.parse::<std::net::IpAddr>().map(|ip| ip.is_loopback()).unwrap_or(false)
}

/// Truncate to at most `max` bytes on a char boundary, with an ellipsis.
pub fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        return s.to_owned();
    }
    let mut end = max;
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}…", &s[..end])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ring_keeps_tail() {
        let mut r = ByteRing::new(8);
        r.push(b"hello ");
        r.push(b"world");
        assert_eq!(r.text(), "lo world");
        r.push(b"0123456789");
        assert_eq!(r.text(), "23456789");
        assert_eq!(r.len(), 8);
    }

    #[test]
    fn paths_and_hosts() {
        let v = serde_json::json!({"a": {"b": [1, {"c": "x"}]}});
        assert_eq!(json_path(&v, "a.b.1.c"), Some(&Value::String("x".into())));
        assert!(json_path(&v, "a.z").is_none());
        assert!(is_loopback_host("127.0.0.1"));
        assert!(is_loopback_host("[::1]"));
        assert!(is_loopback_host("localhost"));
        assert!(!is_loopback_host("example.com"));
        assert_eq!(basename("/usr/bin/kubectl"), "kubectl");
        assert_eq!(sha256_hex(b"").len(), 64);
        assert_eq!(truncate("héllo", 2), "h…");
    }
}

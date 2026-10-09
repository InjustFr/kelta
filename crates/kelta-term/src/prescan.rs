//! Byte-level pre-scan of PTY output, run before the alacritty model sees a chunk:
//!
//! - [`OscScanner`]: OSC 7 (cwd), OSC 9 and OSC 777 (notifications), which alacritty ignores.
//!   Streaming: sequences split across reads are reassembled.
//! - [`Tail`]: the incomplete escape sequence / UTF-8 character at the end of the stream, appended
//!   to snapshots so a view that receives a Snapshot followed by raw Data frames ends up in the same
//!   parser state as the model.

use std::path::PathBuf;

use kelta_proto::term::TerminalEvent;

const ESC: u8 = 0x1b;
const BEL: u8 = 0x07;
const CAN: u8 = 0x18;
const SUB: u8 = 0x1a;
/// Longest captured OSC body; longer bodies are skipped.
const MAX_OSC: usize = 8 * 1024;
/// Longest pending tail kept for snapshots.
const MAX_TAIL: usize = 4 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Capture {
    Cwd,
    Notify9,
    Notify777,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum State {
    Ground,
    Esc,
    /// Reading the OSC number.
    OscNum(u32, u8),
    Capture(Capture),
    Skip,
    /// ESC inside an OSC: `\` terminates (ST); anything else terminates and starts a new escape.
    OscEsc(Option<Capture>),
}

/// Streaming OSC 7/9/777 scanner.
#[derive(Debug, Clone)]
pub struct OscScanner {
    state: State,
    buf: Vec<u8>,
    /// Raw OSC 7 payload last seen (re-emitted by snapshots).
    cwd_uri: Option<String>,
}

impl Default for OscScanner {
    fn default() -> Self {
        Self { state: State::Ground, buf: Vec::new(), cwd_uri: None }
    }
}

impl OscScanner {
    pub fn new() -> Self {
        Self::default()
    }

    /// Last OSC 7 URI.
    pub fn cwd_uri(&self) -> Option<&str> {
        self.cwd_uri.as_deref()
    }

    /// Scan a chunk, pushing events for completed OSC 7/9/777 sequences.
    pub fn scan(&mut self, bytes: &[u8], out: &mut Vec<TerminalEvent>) {
        let mut i = 0;
        while i < bytes.len() {
            match self.state {
                State::Ground => match bytes[i..].iter().position(|&b| b == ESC) {
                    Some(p) => {
                        i += p + 1;
                        self.state = State::Esc;
                    }
                    None => return,
                },
                State::Esc => {
                    self.state = if bytes[i] == b']' { State::OscNum(0, 0) } else { State::Ground };
                    if bytes[i] == ESC {
                        self.state = State::Esc;
                    }
                    i += 1;
                }
                State::OscNum(n, digits) => {
                    let b = bytes[i];
                    i += 1;
                    match b {
                        b'0'..=b'9' if digits < 6 => {
                            self.state = State::OscNum(n * 10 + u32::from(b - b'0'), digits + 1);
                        }
                        b';' if digits > 0 => {
                            self.buf.clear();
                            self.state = match n {
                                7 => State::Capture(Capture::Cwd),
                                9 => State::Capture(Capture::Notify9),
                                777 => State::Capture(Capture::Notify777),
                                _ => State::Skip,
                            };
                        }
                        BEL | CAN | SUB => self.state = State::Ground,
                        ESC => self.state = State::OscEsc(None),
                        _ => self.state = State::Skip,
                    }
                }
                State::Capture(kind) => {
                    let rest = &bytes[i..];
                    match rest.iter().position(|&b| matches!(b, BEL | ESC | CAN | SUB)) {
                        Some(p) => {
                            self.push_capture(&rest[..p]);
                            let t = rest[p];
                            i += p + 1;
                            match t {
                                BEL => {
                                    self.finish(kind, out);
                                    self.state = State::Ground;
                                }
                                ESC => self.state = State::OscEsc(Some(kind)),
                                _ => {
                                    self.buf.clear();
                                    self.state = State::Ground;
                                }
                            }
                        }
                        None => {
                            self.push_capture(rest);
                            return;
                        }
                    }
                }
                State::Skip => {
                    let rest = &bytes[i..];
                    match rest.iter().position(|&b| matches!(b, BEL | ESC | CAN | SUB)) {
                        Some(p) => {
                            i += p + 1;
                            self.state = if rest[p] == ESC { State::OscEsc(None) } else { State::Ground };
                        }
                        None => return,
                    }
                }
                State::OscEsc(kind) => {
                    // vte dispatches the OSC on any ESC; `ESC \` is then a no-op.
                    if let Some(kind) = kind {
                        self.finish(kind, out);
                    }
                    if bytes[i] == b'\\' {
                        self.state = State::Ground;
                        i += 1;
                    } else {
                        // Re-process this byte as the start of a new escape.
                        self.state = State::Esc;
                    }
                }
            }
        }
    }

    fn push_capture(&mut self, bytes: &[u8]) {
        if self.buf.len() + bytes.len() > MAX_OSC {
            self.buf.clear();
            self.state = State::Skip;
        } else {
            self.buf.extend_from_slice(bytes);
        }
    }

    fn finish(&mut self, kind: Capture, out: &mut Vec<TerminalEvent>) {
        let body = String::from_utf8_lossy(&self.buf).into_owned();
        self.buf.clear();
        match kind {
            Capture::Cwd => {
                if let Some(path) = parse_cwd(&body) {
                    self.cwd_uri = Some(body);
                    out.push(TerminalEvent::Cwd(path));
                }
            }
            Capture::Notify9 => {
                // `OSC 9 ; 4 ; state ; progress` is ConEmu progress, not a notification.
                if body.starts_with("4;") || body == "4" || body.is_empty() {
                    return;
                }
                out.push(TerminalEvent::Notify { title: None, body });
            }
            Capture::Notify777 => {
                let mut parts = body.splitn(3, ';');
                if parts.next() != Some("notify") {
                    return;
                }
                let title = parts.next().unwrap_or_default().to_owned();
                let text = parts.next().unwrap_or_default().to_owned();
                out.push(TerminalEvent::Notify { title: Some(title), body: text });
            }
        }
    }
}

/// `file://host/path` (or any `scheme://host/path`, or a bare absolute path) → decoded path.
pub fn parse_cwd(uri: &str) -> Option<PathBuf> {
    let path = if let Some(idx) = uri.find("://") {
        let rest = &uri[idx + 3..];
        let slash = rest.find('/')?;
        &rest[slash..]
    } else if uri.starts_with('/') {
        uri
    } else {
        return None;
    };
    let decoded = percent_decode(path);
    if decoded.is_empty() { None } else { Some(PathBuf::from(decoded)) }
}

fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            let hex = |c: u8| (c as char).to_digit(16);
            if let (Some(h), Some(l)) = (hex(bytes[i + 1]), hex(bytes[i + 2])) {
                out.push((h * 16 + l) as u8);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Incomplete trailing sequence of the output stream.
#[derive(Debug, Clone, Default)]
pub struct Tail {
    pending: Vec<u8>,
}

impl Tail {
    pub fn new() -> Self {
        Self::default()
    }

    /// Bytes to append to a snapshot (an unfinished escape sequence or UTF-8 character).
    pub fn pending(&self) -> &[u8] {
        &self.pending
    }

    pub fn clear(&mut self) {
        self.pending.clear();
    }

    /// Account for a chunk of output.
    pub fn update(&mut self, chunk: &[u8]) {
        if chunk.is_empty() {
            return;
        }
        if let Some(p) = chunk.iter().rposition(|&b| b == ESC) {
            let cand = &chunk[p..];
            if incomplete_escape(cand) {
                self.set(cand);
            } else {
                self.utf8_tail(chunk);
            }
            return;
        }
        if self.pending.first() == Some(&ESC) {
            // An unterminated escape continues in this chunk.
            if self.pending.len() + chunk.len() > MAX_TAIL {
                // Very long string sequence (e.g. image data): give up on exact repair.
                self.pending.clear();
                return;
            }
            let mut cand = std::mem::take(&mut self.pending);
            cand.extend_from_slice(chunk);
            if incomplete_escape(&cand) {
                self.pending = cand;
            } else {
                self.utf8_tail(chunk);
            }
            return;
        }
        if chunk.len() >= 3 || self.pending.is_empty() {
            self.utf8_tail(chunk);
        } else {
            // A pending partial UTF-8 character followed by a short chunk.
            let mut cand = std::mem::take(&mut self.pending);
            cand.extend_from_slice(chunk);
            self.utf8_tail(&cand);
        }
    }

    fn set(&mut self, bytes: &[u8]) {
        self.pending.clear();
        if bytes.len() <= MAX_TAIL {
            self.pending.extend_from_slice(bytes);
        }
    }

    fn utf8_tail(&mut self, chunk: &[u8]) {
        let n = incomplete_utf8_suffix(chunk);
        let start = chunk.len() - n;
        self.set(&chunk[start..]);
    }
}

/// Is `s` (starting with ESC) an unterminated escape sequence?
fn incomplete_escape(s: &[u8]) -> bool {
    debug_assert_eq!(s.first(), Some(&ESC));
    let Some(&kind) = s.get(1) else { return true };
    let body = &s[2..];
    match kind {
        b'[' => {
            // CSI: parameters / intermediates until a final byte 0x40..=0x7e.
            for &b in body {
                match b {
                    0x40..=0x7e => return false,
                    CAN | SUB => return false,
                    _ => {}
                }
            }
            true
        }
        b']' | b'P' | b'_' | b'^' | b'X' => {
            // String: terminated by BEL (OSC) or ST; the ESC of ST would be the last ESC itself.
            !body.iter().any(|&b| matches!(b, BEL | CAN | SUB))
        }
        0x20..=0x2f => {
            // nF escape: intermediates until a final 0x30..=0x7e.
            !s[1..].iter().any(|&b| (0x30..=0x7e).contains(&b))
        }
        _ => false,
    }
}

/// Length of an incomplete UTF-8 sequence at the end of `s` (0..=3).
fn incomplete_utf8_suffix(s: &[u8]) -> usize {
    let n = s.len();
    for back in 1..=3.min(n) {
        let b = s[n - back];
        if b & 0b1100_0000 == 0b1000_0000 {
            continue; // continuation byte
        }
        let need = match b {
            0xc0..=0xdf => 2,
            0xe0..=0xef => 3,
            0xf0..=0xf7 => 4,
            _ => return 0,
        };
        return if back < need { back } else { 0 };
    }
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scan_all(chunks: &[&[u8]]) -> Vec<TerminalEvent> {
        let mut s = OscScanner::new();
        let mut out = Vec::new();
        for c in chunks {
            s.scan(c, &mut out);
        }
        out
    }

    #[test]
    fn osc7_cwd_bel_and_st() {
        let ev = scan_all(&[b"ab\x1b]7;file://host/tmp/a%20b\x07cd"]);
        assert_eq!(ev, vec![TerminalEvent::Cwd(PathBuf::from("/tmp/a b"))]);
        let ev = scan_all(&[b"\x1b]7;file:///home/me\x1b\\"]);
        assert_eq!(ev, vec![TerminalEvent::Cwd(PathBuf::from("/home/me"))]);
        let ev = scan_all(&[b"\x1b]7;kitty-shell-cwd://h/srv\x07"]);
        assert_eq!(ev, vec![TerminalEvent::Cwd(PathBuf::from("/srv"))]);
    }

    #[test]
    fn split_across_chunks_at_every_offset() {
        let seq: &[u8] = b"xx\x1b]777;notify;Build;done ok\x1b\\yy\x1b]9;hello there\x07z";
        for cut in 0..seq.len() {
            let ev = scan_all(&[&seq[..cut], &seq[cut..]]);
            assert_eq!(
                ev,
                vec![
                    TerminalEvent::Notify { title: Some("Build".into()), body: "done ok".into() },
                    TerminalEvent::Notify { title: None, body: "hello there".into() },
                ],
                "cut at {cut}"
            );
        }
    }

    #[test]
    fn ignores_other_oscs_progress_and_garbage() {
        let ev = scan_all(&[b"\x1b]0;title\x07\x1b]9;4;1;50\x07\x1b]52;c;aGk=\x07\x1b]777;other;x\x07"]);
        assert!(ev.is_empty(), "{ev:?}");
        let ev = scan_all(&[b"\x1b[31m\x1b]7;relative\x07\x1b]9;\x07"]);
        assert!(ev.is_empty(), "{ev:?}");
    }

    #[test]
    fn esc_inside_osc_terminates_it() {
        // vte dispatches on ESC; the following CSI is a new sequence.
        let ev = scan_all(&[b"\x1b]9;msg\x1b[0m\x1b]9;second\x07"]);
        assert_eq!(
            ev,
            vec![
                TerminalEvent::Notify { title: None, body: "msg".into() },
                TerminalEvent::Notify { title: None, body: "second".into() },
            ]
        );
    }

    #[test]
    fn overlong_bodies_are_skipped() {
        let mut big = b"\x1b]9;".to_vec();
        big.extend(std::iter::repeat_n(b'a', MAX_OSC + 10));
        big.extend_from_slice(b"\x07\x1b]9;ok\x07");
        let ev = scan_all(&[&big]);
        assert_eq!(ev, vec![TerminalEvent::Notify { title: None, body: "ok".into() }]);
    }

    #[test]
    fn remembers_cwd_uri() {
        let mut s = OscScanner::new();
        s.scan(b"\x1b]7;file://h/var\x07", &mut Vec::new());
        assert_eq!(s.cwd_uri(), Some("file://h/var"));
    }

    #[test]
    fn tail_tracks_incomplete_sequences() {
        let mut t = Tail::new();
        t.update(b"abc\x1b[3");
        assert_eq!(t.pending(), b"\x1b[3");
        t.update(b"1mdef");
        assert_eq!(t.pending(), b"");
        t.update(b"x\x1b]2;tit");
        assert_eq!(t.pending(), b"\x1b]2;tit");
        t.update(b"le continues");
        assert_eq!(t.pending(), b"\x1b]2;title continues");
        t.update(b"\x07");
        assert_eq!(t.pending(), b"");
        t.update(b"\x1b");
        assert_eq!(t.pending(), b"\x1b");
        t.update(b"(");
        assert_eq!(t.pending(), b"\x1b(");
        t.update(b"0");
        assert_eq!(t.pending(), b"");
        t.update(b"\x1b]0;x\x1b");
        assert_eq!(t.pending(), b"\x1b");
        t.update(b"\\");
        assert_eq!(t.pending(), b"");
    }

    #[test]
    fn tail_tracks_partial_utf8() {
        let mut t = Tail::new();
        let s = "é€😀".as_bytes();
        t.update(&s[..1]);
        assert_eq!(t.pending(), &s[..1]);
        t.update(&s[1..3]);
        assert_eq!(t.pending(), &s[2..3]);
        t.update(&s[3..5]);
        assert_eq!(t.pending(), b"");
        t.update(&s[5..7]);
        assert_eq!(t.pending(), &s[5..7]);
        t.update(&s[7..]);
        assert_eq!(t.pending(), b"");
        t.update(b"ok\x1b[1m\xe2\x82");
        assert_eq!(t.pending(), b"\xe2\x82");
    }

    #[test]
    fn percent_decoding() {
        assert_eq!(percent_decode("/a%2Fb%zz%4"), "/a/b%zz%4");
        assert_eq!(parse_cwd("file://h"), None);
    }
}

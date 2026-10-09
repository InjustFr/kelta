//! On-disk scrollback history log (ARCHITECTURE §9.6). Allowlisted for `std::thread`.
//!
//! Rows scrolling into a session's primary history are captured as plain text (ANSI stripped,
//! wrapped rows joined) and handed in batches to one writer thread, so the PTY readers never wait
//! on the disk. Layout: `<dir>/<id>.log` (current) and `<id>.1.log` (previous half), dir 0700,
//! files 0600. The current file rotates past half the per-session cap; past the global cap the
//! oldest files (by mtime) are deleted first. A crash can only cut the last line: the first
//! append after a start terminates it, readers treat it as a line.

use std::collections::{HashSet, VecDeque};
use std::fs::{self, File, OpenOptions};
use std::io::{BufRead, BufReader, Read, Seek, SeekFrom, Write};
use std::os::unix::fs::{DirBuilderExt, FileExt, OpenOptionsExt};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, SyncSender, TrySendError};

use kelta_proto::error::KeltaError;
use kelta_proto::ids::SessionId;
use kelta_proto::term::{HistoryHit, TerminalLimits};

/// Default `terminal.history_log_mb` (used when limits carry 0).
pub const DEFAULT_SESSION_MB: u32 = 16;
/// Default `terminal.history_log_total_mb` (used when limits carry 0).
pub const DEFAULT_TOTAL_MB: u32 = 512;
/// A session's captured lines are sent to the writer past this size (and when its reader goes
/// idle or exits: event-driven, no timer).
pub const BATCH: usize = 64 * 1024;
/// Unsent text a session keeps while the writer queue is full before dropping it (with a marker).
const MAX_PENDING: usize = 1024 * 1024;
/// Writer queue depth (≈ 4 MiB of 64 KiB batches).
const QUEUE: usize = 64;
/// Bytes read per requested line by `tail`.
const TAIL_BYTES_PER_LINE: usize = 512;

enum Msg {
    Append(SessionId, String),
    Caps(u64, u64),
    Delete(SessionId),
    Tail(SessionId, usize, mpsc::Sender<String>),
    Search(Vec<SessionId>, String, usize, mpsc::Sender<Vec<HistoryHit>>),
    Sync(mpsc::Sender<()>),
}

/// Handle to the writer thread (it ends when the last handle is dropped).
pub(crate) struct HistoryLog {
    tx: SyncSender<Msg>,
}

/// `(per-session, total)` caps in bytes.
pub(crate) fn caps(limits: &TerminalLimits) -> (u64, u64) {
    let mb = |v: u32, d: u32| u64::from(if v == 0 { d } else { v }) * 1024 * 1024;
    (mb(limits.history_log_mb, DEFAULT_SESSION_MB), mb(limits.history_log_total_mb, DEFAULT_TOTAL_MB))
}

impl HistoryLog {
    pub fn start(dir: PathBuf, limits: &TerminalLimits) -> Result<Self, KeltaError> {
        fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(&dir)
            .map_err(|e| KeltaError::internal(format!("history dir {}: {e}", dir.display())))?;
        let (tx, rx) = mpsc::sync_channel(QUEUE);
        let (per, total) = caps(limits);
        #[allow(clippy::disallowed_methods)] // allowlisted: one history writer per host (ARCHITECTURE §9.6)
        std::thread::Builder::new()
            .name("kelta-history".into())
            .spawn(move || Writer::new(dir, per, total).run(rx))
            .map_err(|e| KeltaError::internal(format!("history thread: {e}")))?;
        Ok(Self { tx })
    }

    /// Queue a batch without waiting; a full queue hands the text back.
    pub fn append(&self, id: &SessionId, text: String) -> Result<(), String> {
        match self.tx.try_send(Msg::Append(id.clone(), text)) {
            Ok(()) | Err(TrySendError::Disconnected(_)) => Ok(()),
            Err(TrySendError::Full(Msg::Append(_, t))) => Err(t),
            Err(TrySendError::Full(_)) => Ok(()),
        }
    }

    /// Append and wait until the writer wrote it (reader exit: the process may quit right after).
    pub fn append_sync(&self, id: &SessionId, text: String) {
        let _ = self.tx.send(Msg::Append(id.clone(), text));
        self.request(Msg::Sync);
    }

    pub fn set_caps(&self, limits: &TerminalLimits) {
        let (per, total) = caps(limits);
        let _ = self.tx.send(Msg::Caps(per, total));
    }

    pub fn delete(&self, id: &SessionId) {
        let _ = self.tx.send(Msg::Delete(id.clone()));
    }

    pub fn tail(&self, id: &SessionId, max_lines: usize) -> String {
        self.request(|tx| Msg::Tail(id.clone(), max_lines, tx)).unwrap_or_default()
    }

    pub fn search(&self, ids: &[SessionId], query: &str, limit: usize) -> Vec<HistoryHit> {
        self.request(|tx| Msg::Search(ids.to_vec(), query.to_lowercase(), limit, tx)).unwrap_or_default()
    }

    fn request<T>(&self, msg: impl FnOnce(mpsc::Sender<T>) -> Msg) -> Option<T> {
        let (tx, rx) = mpsc::channel();
        self.tx.send(msg(tx)).ok()?;
        rx.recv().ok()
    }
}

/// Hand a session's captured lines to the writer when there are at least `min` bytes (0 = any).
/// A full queue keeps them for the next flush, up to [`MAX_PENDING`], then drops them with a
/// marker line so the log shows the gap.
pub(crate) fn flush(log: &HistoryLog, id: &SessionId, buf: &mut String, min: usize) {
    if buf.is_empty() || buf.len() < min {
        return;
    }
    if let Err(text) = log.append(id, std::mem::take(buf)) {
        *buf = if text.len() < MAX_PENDING {
            text
        } else {
            tracing::warn!(session = %id, "history log: writer behind, {} KiB not saved", text.len() / 1024);
            format!("[kelta: {} KiB of output not saved to history: disk too slow]\n", text.len() / 1024)
        };
    }
}

/// File name stem for a session id; ids that are not plain tokens get no log (path safety).
fn stem(id: &SessionId) -> Option<&str> {
    let s = id.as_str();
    (!s.is_empty()
        && s.len() <= 128
        && s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_'))
    .then_some(s)
}

struct Writer {
    dir: PathBuf,
    per: u64,
    total_cap: u64,
    /// Bytes of all logs (rescanned whenever the global cap is enforced).
    total: u64,
    /// Sessions appended to since start (their crash-cut tail is already terminated).
    checked: HashSet<SessionId>,
    warned: bool,
}

impl Writer {
    fn new(dir: PathBuf, per: u64, total_cap: u64) -> Self {
        let total = log_files(&dir).iter().map(|f| f.2).sum();
        let mut w = Self { dir, per, total_cap, total, checked: HashSet::new(), warned: false };
        w.enforce_total();
        w
    }

    fn run(mut self, rx: Receiver<Msg>) {
        for msg in rx {
            match msg {
                Msg::Append(id, text) => {
                    if let Err(e) = self.append(&id, &text)
                        && !std::mem::replace(&mut self.warned, true)
                    {
                        tracing::warn!(session = %id, "history log write failed: {e}");
                    }
                }
                Msg::Caps(per, total) => {
                    (self.per, self.total_cap) = (per, total);
                    self.enforce_total();
                }
                Msg::Delete(id) => {
                    for p in self.paths(&id).into_iter().flatten() {
                        let _ = fs::remove_file(p);
                    }
                    self.checked.remove(&id);
                    self.total = log_files(&self.dir).iter().map(|f| f.2).sum();
                }
                Msg::Tail(id, n, tx) => {
                    let _ = tx.send(self.tail(&id, n));
                }
                Msg::Search(ids, q, limit, tx) => {
                    let _ = tx.send(self.search(&ids, &q, limit));
                }
                Msg::Sync(tx) => {
                    let _ = tx.send(());
                }
            }
        }
    }

    /// `[current, previous]` paths.
    fn paths(&self, id: &SessionId) -> Option<[PathBuf; 2]> {
        let s = stem(id)?;
        Some([self.dir.join(format!("{s}.log")), self.dir.join(format!("{s}.1.log"))])
    }

    fn append(&mut self, id: &SessionId, text: &str) -> std::io::Result<()> {
        let Some([cur, old]) = self.paths(id) else { return Ok(()) };
        let mut f = OpenOptions::new().create(true).append(true).read(true).mode(0o600).open(&cur)?;
        let mut len = f.metadata()?.len();
        if self.checked.insert(id.clone()) && len > 0 {
            let mut last = [0u8];
            f.read_exact_at(&mut last, len - 1)?;
            if last[0] != b'\n' {
                f.write_all(b"\n")?;
                len += 1;
                self.total += 1;
            }
        }
        f.write_all(text.as_bytes())?;
        len += text.len() as u64;
        self.total += text.len() as u64;
        if len > self.per / 2 {
            let replaced = fs::metadata(&old).map_or(0, |m| m.len());
            fs::rename(&cur, &old)?;
            self.total = self.total.saturating_sub(replaced);
        }
        if self.total > self.total_cap {
            self.enforce_total();
        }
        Ok(())
    }

    /// Delete the oldest files until all logs fit the global cap.
    fn enforce_total(&mut self) {
        let mut files = log_files(&self.dir);
        self.total = files.iter().map(|f| f.2).sum();
        files.sort_by_key(|f| f.0);
        for (_, path, len) in files {
            if self.total <= self.total_cap {
                break;
            }
            if fs::remove_file(&path).is_ok() {
                self.total -= len;
            }
        }
    }

    fn tail(&self, id: &SessionId, n: usize) -> String {
        let Some([cur, old]) = self.paths(id) else { return String::new() };
        let budget = n.saturating_mul(TAIL_BYTES_PER_LINE).saturating_add(4096);
        let mut text = read_tail(&cur, budget);
        if text.lines().count() < n {
            text = read_tail(&old, budget) + &text;
        }
        let lines: Vec<&str> = text.lines().collect();
        lines[lines.len().saturating_sub(n)..].join("\n")
    }

    /// Newest `limit` matching lines per session, at most `limit` in all.
    fn search(&self, ids: &[SessionId], q: &str, limit: usize) -> Vec<HistoryHit> {
        let mut out = Vec::new();
        for id in ids {
            let Some([cur, old]) = self.paths(id) else { continue };
            let mut hits: VecDeque<String> = VecDeque::new();
            for path in [old, cur] {
                let Ok(f) = File::open(&path) else { continue };
                let mut r = BufReader::new(f);
                let mut raw = Vec::new();
                while matches!(r.read_until(b'\n', &mut raw), Ok(n) if n > 0) {
                    let line = String::from_utf8_lossy(&raw);
                    let line = line.trim_end_matches('\n');
                    if line.to_lowercase().contains(q) {
                        if hits.len() == limit {
                            hits.pop_front();
                        }
                        hits.push_back(line.to_owned());
                    }
                    raw.clear();
                }
            }
            out.extend(hits.into_iter().map(|line| HistoryHit { session_id: id.clone(), line }));
        }
        out.truncate(limit);
        out
    }
}

/// `(mtime, path, len)` of every `*.log` file in `dir`.
fn log_files(dir: &Path) -> Vec<(std::time::SystemTime, PathBuf, u64)> {
    let Ok(rd) = fs::read_dir(dir) else { return Vec::new() };
    rd.flatten()
        .filter(|e| e.file_name().to_string_lossy().ends_with(".log"))
        .filter_map(|e| {
            let m = e.metadata().ok()?;
            Some((m.modified().ok()?, e.path(), m.len()))
        })
        .collect()
}

/// The last `max` bytes of a file as text, starting at a line boundary.
// shortcut: lines longer than TAIL_BYTES_PER_LINE on average make `tail` return fewer lines; read
// backwards in chunks if restores of very wide output come up short.
fn read_tail(path: &Path, max: usize) -> String {
    let Ok(mut f) = File::open(path) else { return String::new() };
    let len = f.metadata().map_or(0, |m| m.len());
    let start = len.saturating_sub(max as u64);
    let mut buf = Vec::new();
    if f.seek(SeekFrom::Start(start)).and_then(|_| f.read_to_end(&mut buf)).is_err() {
        return String::new();
    }
    let from = if start > 0 { buf.iter().position(|&b| b == b'\n').map_or(buf.len(), |i| i + 1) } else { 0 };
    String::from_utf8_lossy(&buf[from..]).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn writer(dir: &Path, per: u64, total: u64) -> Writer {
        Writer::new(dir.to_owned(), per, total)
    }

    #[test]
    fn rotation_keeps_a_session_under_its_cap() {
        let d = tempfile::tempdir().unwrap();
        let mut w = writer(d.path(), 1000, 1 << 30);
        let id = SessionId::new("s1");
        for i in 0..200 {
            w.append(&id, &format!("line {i:04}\n")).unwrap();
        }
        let [cur, old] = w.paths(&id).unwrap();
        let size = |p: &Path| fs::metadata(p).map_or(0, |m| m.len());
        assert!(size(&old) > 0 && size(&cur) + size(&old) <= 1000 + 10, "{} + {}", size(&cur), size(&old));
        assert_eq!(w.tail(&id, 3), "line 0197\nline 0198\nline 0199");
        // The tail spans the rotation boundary.
        let n = fs::read_to_string(&cur).unwrap().lines().count();
        assert_eq!(w.tail(&id, n + 2).lines().count(), n + 2);
    }

    #[test]
    fn global_cap_deletes_oldest_files() {
        let d = tempfile::tempdir().unwrap();
        let mut w = writer(d.path(), 1 << 20, 3000);
        let line = "x".repeat(99) + "\n";
        for (i, s) in ["a", "b", "c", "d"].into_iter().enumerate() {
            for _ in 0..10 {
                w.append(&SessionId::new(s), &line).unwrap();
            }
            // mtime granularity: make the order of sessions observable.
            let t = std::time::SystemTime::now() - std::time::Duration::from_secs(100 - i as u64);
            File::options().write(true).open(d.path().join(format!("{s}.log"))).unwrap().set_modified(t).ok();
        }
        assert!(w.total <= 3000, "{}", w.total);
        assert!(!d.path().join("a.log").exists());
        assert!(d.path().join("d.log").exists());
    }

    #[test]
    fn truncated_tail_is_terminated_not_merged() {
        let d = tempfile::tempdir().unwrap();
        // A crash cut the last line (and a UTF-8 sequence) short.
        fs::write(d.path().join("s.log"), b"one\ntwo\nthr\xC3").unwrap();
        let mut w = writer(d.path(), 1 << 20, 1 << 30);
        let id = SessionId::new("s");
        assert_eq!(w.tail(&id, 10), "one\ntwo\nthr\u{FFFD}");
        w.append(&id, "four\n").unwrap();
        assert_eq!(w.tail(&id, 10), "one\ntwo\nthr\u{FFFD}\nfour");
        w.append(&id, "five\n").unwrap();
        assert_eq!(w.tail(&id, 2), "four\nfive");
    }

    #[test]
    fn search_is_case_insensitive_newest_first_capped() {
        let d = tempfile::tempdir().unwrap();
        let mut w = writer(d.path(), 1 << 20, 1 << 30);
        let (a, b) = (SessionId::new("a"), SessionId::new("b"));
        w.append(&a, "Error one\nok\nerror two\nERROR three\n").unwrap();
        w.append(&b, "no match\nan error here\n").unwrap();
        let hits = w.search(&[a.clone(), b.clone()], "error", 2);
        let lines: Vec<&str> = hits.iter().map(|h| h.line.as_str()).collect();
        assert_eq!(lines, ["error two", "ERROR three"]);
        let hits = w.search(&[a, b.clone()], "here", 10);
        assert_eq!(hits, [HistoryHit { session_id: b, line: "an error here".into() }]);
    }

    #[test]
    fn unsafe_ids_get_no_file() {
        let d = tempfile::tempdir().unwrap();
        let mut w = writer(d.path(), 1 << 20, 1 << 30);
        w.append(&SessionId::new("../x"), "a\n").unwrap();
        assert!(log_files(d.path()).is_empty());
        assert!(!d.path().parent().unwrap().join("x.log").exists());
    }

    #[test]
    fn delete_removes_both_halves() {
        let d = tempfile::tempdir().unwrap();
        let mut w = writer(d.path(), 100, 1 << 30);
        let id = SessionId::new("s");
        for _ in 0..22 {
            w.append(&id, "0123456789\n").unwrap();
        }
        assert_eq!(log_files(d.path()).len(), 2);
        let (tx, rx) = mpsc::sync_channel(4);
        tx.send(Msg::Delete(id)).unwrap();
        drop(tx);
        w.run(rx);
        assert!(log_files(d.path()).is_empty());
    }
}

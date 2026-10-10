//! `FakeTerminalHost`: in-memory sessions with scripted frames and events.

use std::collections::HashMap;
use std::sync::Arc;

use parking_lot::Mutex;

use crate::api::{FrameSink, TerminalEvents, TerminalHost};
use crate::error::KeltaError;
use crate::ids::SessionId;
use crate::model::AttachInfo;
use crate::term::{
    HistoryHit, KillSignal, PtySpawnSpec, SessionTermStats, TerminalEvent, TerminalLimits, TerminalPalette,
    TerminalStats,
};

/// What the fake recorded for one spawned session.
pub struct FakeSpawned {
    pub spec: PtySpawnSpec,
    pub written: Vec<u8>,
    pub cols: u16,
    pub rows: u16,
    pub killed: Option<KillSignal>,
    pub generation: u32,
    pub acked: u64,
    pub attached: bool,
    sink: Option<Box<dyn FrameSink>>,
}

#[derive(Default)]
pub struct FakeTerminalHost {
    sessions: Mutex<HashMap<SessionId, FakeSpawned>>,
    /// Frames (already tagged) sent on attach.
    scripts: Mutex<HashMap<SessionId, Vec<Vec<u8>>>>,
    /// Text returned by `text_tail`.
    tails: Mutex<HashMap<SessionId, String>>,
    /// On-disk history log contents (`history_*`).
    history: Mutex<HashMap<SessionId, String>>,
    palette: Mutex<Option<TerminalPalette>>,
    limits: Mutex<Option<TerminalLimits>>,
    spawn_error: Mutex<Option<KeltaError>>,
}

impl FakeTerminalHost {
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    /// Frames sent to the sink on every attach (e.g. a Snapshot frame).
    pub fn script(&self, id: &SessionId, frames: Vec<Vec<u8>>) {
        self.scripts.lock().insert(id.clone(), frames);
    }

    pub fn set_text_tail(&self, id: &SessionId, text: impl Into<String>) {
        self.tails.lock().insert(id.clone(), text.into());
    }

    pub fn set_history(&self, id: &SessionId, text: impl Into<String>) {
        self.history.lock().insert(id.clone(), text.into());
    }

    pub fn has_history(&self, id: &SessionId) -> bool {
        self.history.lock().contains_key(id)
    }

    /// Next `spawn` fails with this error.
    pub fn fail_next_spawn(&self, e: KeltaError) {
        *self.spawn_error.lock() = Some(e);
    }

    /// Push a frame to the attached view (returns false if none / closed).
    pub fn push_frame(&self, id: &SessionId, frame: Vec<u8>) -> bool {
        let mut s = self.sessions.lock();
        let Some(sess) = s.get_mut(id) else { return false };
        let Some(sink) = sess.sink.as_mut() else { return false };
        let ok = sink.send(frame);
        if !ok {
            sess.sink = None;
            sess.attached = false;
        }
        ok
    }

    /// Deliver a terminal event through the spec's `TerminalEvents`.
    pub fn emit(&self, id: &SessionId, ev: TerminalEvent) {
        let events = self.sessions.lock().get(id).map(|s| s.spec.events.clone());
        if let Some(events) = events {
            events.on_event(id, ev);
        }
    }

    pub fn spawned_ids(&self) -> Vec<SessionId> {
        let mut v: Vec<_> = self.sessions.lock().keys().cloned().collect();
        v.sort();
        v
    }

    pub fn with_session<R>(&self, id: &SessionId, f: impl FnOnce(&FakeSpawned) -> R) -> Option<R> {
        self.sessions.lock().get(id).map(f)
    }

    pub fn written(&self, id: &SessionId) -> Vec<u8> {
        self.with_session(id, |s| s.written.clone()).unwrap_or_default()
    }

    pub fn palette(&self) -> Option<TerminalPalette> {
        self.palette.lock().clone()
    }

    pub fn limits(&self) -> Option<TerminalLimits> {
        *self.limits.lock()
    }
}

fn missing(id: &SessionId) -> KeltaError {
    KeltaError::not_found(format!("session {id}"))
}

impl TerminalHost for FakeTerminalHost {
    fn spawn(&self, spec: PtySpawnSpec) -> Result<(), KeltaError> {
        if let Some(e) = self.spawn_error.lock().take() {
            return Err(e);
        }
        let id = spec.id.clone();
        let (cols, rows) = (spec.cols, spec.rows);
        self.sessions.lock().insert(
            id,
            FakeSpawned {
                spec,
                written: Vec::new(),
                cols,
                rows,
                killed: None,
                generation: 0,
                acked: 0,
                attached: false,
                sink: None,
            },
        );
        Ok(())
    }

    fn attach(
        &self,
        id: &SessionId,
        cols: u16,
        rows: u16,
        mut sink: Box<dyn FrameSink>,
    ) -> Result<AttachInfo, KeltaError> {
        let frames = self.scripts.lock().get(id).cloned().unwrap_or_default();
        let mut s = self.sessions.lock();
        let sess = s.get_mut(id).ok_or_else(|| missing(id))?;
        sess.generation += 1;
        sess.cols = cols;
        sess.rows = rows;
        let mut open = true;
        for f in frames {
            if !sink.send(f) {
                open = false;
                break;
            }
        }
        sess.attached = open;
        sess.sink = open.then_some(sink);
        Ok(AttachInfo { generation: sess.generation, cols, rows })
    }

    fn detach(&self, id: &SessionId, generation: u32) {
        if let Some(sess) = self.sessions.lock().get_mut(id)
            && (sess.generation == generation || generation == crate::api::ANY_VIEW)
        {
            sess.sink = None;
            sess.attached = false;
        }
    }

    fn write(&self, id: &SessionId, bytes: &[u8]) -> Result<(), KeltaError> {
        let mut s = self.sessions.lock();
        let sess = s.get_mut(id).ok_or_else(|| missing(id))?;
        sess.written.extend_from_slice(bytes);
        Ok(())
    }

    fn resize(&self, id: &SessionId, cols: u16, rows: u16) -> Result<(), KeltaError> {
        let mut s = self.sessions.lock();
        let sess = s.get_mut(id).ok_or_else(|| missing(id))?;
        sess.cols = cols;
        sess.rows = rows;
        Ok(())
    }

    fn ack(&self, id: &SessionId, generation: u32, bytes: u32) {
        if let Some(sess) = self.sessions.lock().get_mut(id)
            && sess.generation == generation
        {
            sess.acked += u64::from(bytes);
        }
    }

    fn kill(&self, id: &SessionId, signal: KillSignal) -> Result<(), KeltaError> {
        let events = {
            let mut s = self.sessions.lock();
            let sess = s.get_mut(id).ok_or_else(|| missing(id))?;
            sess.killed = Some(signal);
            sess.spec.events.clone()
        };
        let sig = match signal {
            KillSignal::Hup => 1,
            KillSignal::Term => 15,
            KillSignal::Kill => 9,
        };
        events.on_event(id, TerminalEvent::Exited { code: None, signal: Some(sig) });
        Ok(())
    }

    fn set_palette(&self, palette: TerminalPalette) {
        *self.palette.lock() = Some(palette);
    }

    fn set_limits(&self, limits: TerminalLimits) {
        *self.limits.lock() = Some(limits);
    }

    fn text_tail(&self, id: &SessionId, max_lines: u32) -> Result<String, KeltaError> {
        if !self.sessions.lock().contains_key(id) {
            return Err(missing(id));
        }
        let text = self.tails.lock().get(id).cloned().unwrap_or_default();
        let lines: Vec<&str> = text.lines().collect();
        let start = lines.len().saturating_sub(max_lines as usize);
        Ok(lines[start..].join("\n"))
    }

    fn history_tail(&self, id: &SessionId, max_lines: u32) -> Result<String, KeltaError> {
        let text = self.history.lock().get(id).cloned().unwrap_or_default();
        let lines: Vec<&str> = text.lines().collect();
        let start = lines.len().saturating_sub(max_lines as usize);
        Ok(lines[start..].join("\n"))
    }

    fn history_search(
        &self,
        ids: &[SessionId],
        query: &str,
        limit: u32,
    ) -> Result<Vec<HistoryHit>, KeltaError> {
        let h = self.history.lock();
        let q = query.to_lowercase();
        let mut hits: Vec<HistoryHit> = ids
            .iter()
            .flat_map(|id| {
                let text = h.get(id).map(String::as_str).unwrap_or_default();
                text.lines()
                    .filter(|l| l.to_lowercase().contains(&q))
                    .map(|l| HistoryHit { session_id: id.clone(), line: l.to_owned() })
                    .collect::<Vec<_>>()
            })
            .collect();
        hits.truncate(limit as usize);
        Ok(hits)
    }

    fn history_delete(&self, id: &SessionId) {
        self.history.lock().remove(id);
    }

    fn stats(&self) -> TerminalStats {
        let s = self.sessions.lock();
        let mut sessions: Vec<SessionTermStats> = s
            .iter()
            .map(|(id, x)| SessionTermStats {
                id: id.clone(),
                bytes_in: 0,
                history_lines: 0,
                cols: x.cols,
                rows: x.rows,
                inflight: 0,
                attached: x.attached,
                memory_bytes: 0,
            })
            .collect();
        sessions.sort_by(|a, b| a.id.cmp(&b.id));
        TerminalStats { sessions, total_memory_bytes: 0, reader_threads: 0 }
    }
}

/// A `FrameSink` recording frames into a shared vector.
#[derive(Clone, Default)]
pub struct RecordingSink {
    pub frames: Arc<Mutex<Vec<Vec<u8>>>>,
    pub closed: Arc<Mutex<bool>>,
}

impl RecordingSink {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn frames(&self) -> Vec<Vec<u8>> {
        self.frames.lock().clone()
    }

    /// Make subsequent sends return false.
    pub fn close(&self) {
        *self.closed.lock() = true;
    }
}

impl FrameSink for RecordingSink {
    fn send(&mut self, frame: Vec<u8>) -> bool {
        if *self.closed.lock() {
            return false;
        }
        self.frames.lock().push(frame);
        true
    }
}

/// A `TerminalEvents` recording every event.
#[derive(Default)]
pub struct RecordingTerminalEvents {
    pub events: Mutex<Vec<(SessionId, TerminalEvent)>>,
}

impl RecordingTerminalEvents {
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    pub fn events(&self) -> Vec<(SessionId, TerminalEvent)> {
        self.events.lock().clone()
    }
}

impl TerminalEvents for RecordingTerminalEvents {
    fn on_event(&self, id: &SessionId, ev: TerminalEvent) {
        self.events.lock().push((id.clone(), ev));
    }
}

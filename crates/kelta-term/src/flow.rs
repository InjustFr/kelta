//! Per-view flow control (ARCHITECTURE §7.3): watermarks, snapshot catch-up and the ack watchdog.
//!
//! Pure state machine; the session drives it and owns the sink. The child is never blocked:
//! above the HIGH watermark Data frames are dropped (the model keeps parsing) and the view is
//! repaired with a Snapshot once acks bring the in-flight count below LOW.

use std::time::{Duration, Instant};

use kelta_proto::term::{ACK_TIMEOUT_MS, HIGH_WATERMARK, LOW_WATERMARK};

/// What to do with a chunk of PTY output for the attached view.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DataAction {
    /// Send a Data frame (in-flight already accounted).
    Send,
    /// No view, or the view is paused: drop the bytes for the view.
    Drop,
}

/// Result of an ack.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AckAction {
    /// Stale generation or no view.
    Ignored,
    /// Accounted.
    Accepted,
    /// The view was paused and is now below LOW: send a Snapshot (then call
    /// [`Flow::snapshot_sent`]).
    CatchUp,
}

/// Flow state of the (single) view attached to a session.
#[derive(Debug, Clone)]
pub struct Flow {
    generation: u32,
    attached: bool,
    inflight: u32,
    paused: bool,
    deadline: Option<Instant>,
    high: u32,
    low: u32,
    timeout: Duration,
}

impl Default for Flow {
    fn default() -> Self {
        Self::with_limits(HIGH_WATERMARK, LOW_WATERMARK, Duration::from_millis(ACK_TIMEOUT_MS))
    }
}

impl Flow {
    pub fn new() -> Self {
        Self::default()
    }

    /// Custom watermarks / watchdog (tests).
    pub fn with_limits(high: u32, low: u32, timeout: Duration) -> Self {
        Self {
            generation: 0,
            attached: false,
            inflight: 0,
            paused: false,
            deadline: None,
            high,
            low,
            timeout,
        }
    }

    /// A new view replaces the previous one; returns the new generation.
    pub fn attach(&mut self) -> u32 {
        self.generation = self.generation.wrapping_add(1).max(1);
        self.attached = true;
        self.inflight = 0;
        self.paused = false;
        self.deadline = None;
        self.generation
    }

    /// Detach `generation` (stale generations are ignored). Returns true when a view was dropped.
    pub fn detach(&mut self, generation: u32) -> bool {
        if !self.attached || generation != self.generation {
            return false;
        }
        self.drop_view();
        true
    }

    /// Drop the current view whatever its generation (sink closed).
    pub fn drop_view(&mut self) {
        self.attached = false;
        self.inflight = 0;
        self.paused = false;
        self.deadline = None;
    }

    pub fn generation(&self) -> u32 {
        self.generation
    }

    pub fn attached(&self) -> bool {
        self.attached
    }

    pub fn inflight(&self) -> u32 {
        self.inflight
    }

    pub fn paused(&self) -> bool {
        self.paused
    }

    /// Watchdog deadline (armed only while bytes are in flight).
    pub fn deadline(&self) -> Option<Instant> {
        self.deadline
    }

    /// `n` bytes of PTY output arrived.
    pub fn on_data(&mut self, n: usize, now: Instant) -> DataAction {
        if !self.attached || self.paused {
            return DataAction::Drop;
        }
        let n = u32::try_from(n).unwrap_or(u32::MAX);
        if self.inflight.saturating_add(n) <= self.high {
            self.inflight += n;
            self.arm(now);
            DataAction::Send
        } else {
            self.paused = true;
            DataAction::Drop
        }
    }

    /// A Snapshot of `len` payload bytes was sent (attach or catch-up).
    pub fn snapshot_sent(&mut self, len: usize, now: Instant) {
        if !self.attached {
            return;
        }
        self.inflight = self.inflight.saturating_add(u32::try_from(len).unwrap_or(u32::MAX));
        self.paused = false;
        self.arm(now);
    }

    /// The view acknowledged `bytes` for `generation`.
    pub fn on_ack(&mut self, generation: u32, bytes: u32, now: Instant) -> AckAction {
        if !self.attached || generation != self.generation {
            return AckAction::Ignored;
        }
        self.inflight = self.inflight.saturating_sub(bytes);
        if self.inflight == 0 {
            self.deadline = None;
        } else if bytes > 0 {
            // Progress: the view is alive; restart the one-shot.
            self.deadline = Some(now + self.timeout);
        }
        if self.paused && self.inflight < self.low { AckAction::CatchUp } else { AckAction::Accepted }
    }

    /// The child exited: the view must end with a snapshot (then the Exit frame). Returns true
    /// when it can be sent now; otherwise the view is paused until acks bring it below LOW.
    pub fn on_exit(&mut self) -> bool {
        if !self.attached {
            return false;
        }
        self.paused = true;
        self.inflight < self.low
    }

    /// Watchdog check: returns the generation to report in `AckTimeout` when the deadline passed.
    /// The watchdog is one-shot: it is disarmed until the next send or ack progress.
    pub fn check_deadline(&mut self, now: Instant) -> Option<u32> {
        match self.deadline {
            Some(d) if now >= d && self.attached && self.inflight > 0 => {
                self.deadline = None;
                Some(self.generation)
            }
            Some(d) if now >= d => {
                self.deadline = None;
                None
            }
            _ => None,
        }
    }

    fn arm(&mut self, now: Instant) {
        if self.inflight > 0 && self.deadline.is_none() {
            // one-shot: ack watchdog, armed by a send while bytes are in flight.
            self.deadline = Some(now + self.timeout);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_view_drops_everything() {
        let mut f = Flow::new();
        assert_eq!(f.on_data(10, Instant::now()), DataAction::Drop);
        assert_eq!(f.on_ack(1, 10, Instant::now()), AckAction::Ignored);
        assert_eq!(f.inflight(), 0);
    }

    #[test]
    fn high_watermark_pauses_and_low_resumes_with_snapshot() {
        let now = Instant::now();
        let mut f = Flow::new();
        let g = f.attach();
        let mut sent = 0u32;
        while f.on_data(64 * 1024, now) == DataAction::Send {
            sent += 64 * 1024;
            assert!(f.inflight() <= HIGH_WATERMARK);
        }
        assert_eq!(sent, HIGH_WATERMARK);
        assert!(f.paused());
        // Paused: further data is dropped without touching inflight.
        assert_eq!(f.on_data(1, now), DataAction::Drop);
        assert_eq!(f.inflight(), HIGH_WATERMARK);
        // Acks above LOW: still paused.
        assert_eq!(f.on_ack(g, 128 * 1024, now), AckAction::Accepted);
        assert!(f.paused());
        // Below LOW: catch up with a snapshot.
        assert_eq!(f.on_ack(g, 100 * 1024, now), AckAction::CatchUp);
        f.snapshot_sent(5000, now);
        assert!(!f.paused());
        assert_eq!(f.inflight(), 28 * 1024 + 5000);
        assert_eq!(f.on_data(10, now), DataAction::Send);
    }

    #[test]
    fn stale_generation_acks_are_ignored() {
        let now = Instant::now();
        let mut f = Flow::new();
        let g1 = f.attach();
        assert_eq!(f.on_data(100, now), DataAction::Send);
        let g2 = f.attach();
        assert_ne!(g1, g2);
        assert_eq!(f.inflight(), 0);
        assert_eq!(f.on_ack(g1, 100, now), AckAction::Ignored);
        assert!(!f.detach(g1));
        assert!(f.attached());
        assert!(f.detach(g2));
        assert!(!f.attached());
    }

    #[test]
    fn watchdog_armed_only_while_in_flight() {
        let t0 = Instant::now();
        let mut f = Flow::with_limits(1000, 100, Duration::from_secs(5));
        let g = f.attach();
        assert_eq!(f.deadline(), None);
        f.on_data(10, t0);
        assert_eq!(f.deadline(), Some(t0 + Duration::from_secs(5)));
        // A later send does not postpone the deadline.
        f.on_data(10, t0 + Duration::from_secs(1));
        assert_eq!(f.deadline(), Some(t0 + Duration::from_secs(5)));
        // Progress restarts it.
        f.on_ack(g, 5, t0 + Duration::from_secs(2));
        assert_eq!(f.deadline(), Some(t0 + Duration::from_secs(7)));
        assert_eq!(f.check_deadline(t0 + Duration::from_secs(6)), None);
        assert_eq!(f.check_deadline(t0 + Duration::from_secs(7)), Some(g));
        // One-shot: disarmed after firing.
        assert_eq!(f.deadline(), None);
        assert_eq!(f.check_deadline(t0 + Duration::from_secs(60)), None);
        // Fully acked: disarmed.
        f.on_data(1, t0);
        f.on_ack(g, 1000, t0);
        assert_eq!(f.inflight(), 0);
        assert_eq!(f.deadline(), None);
    }

    #[test]
    fn exit_pauses_until_caught_up() {
        let now = Instant::now();
        let mut f = Flow::new();
        assert!(!f.on_exit(), "no view");
        let g = f.attach();
        assert!(f.on_exit(), "nothing in flight: snapshot now");
        f.snapshot_sent(10, now);
        f.on_data(200 * 1024, now);
        assert!(!f.on_exit(), "above LOW: deferred");
        assert!(f.paused());
        assert_eq!(f.on_ack(g, 200 * 1024, now), AckAction::CatchUp);
    }

    #[test]
    fn detach_resets_state() {
        let now = Instant::now();
        let mut f = Flow::new();
        let g = f.attach();
        f.on_data(HIGH_WATERMARK as usize + 1, now);
        assert!(f.paused());
        assert!(f.detach(g));
        assert!(!f.paused());
        assert_eq!(f.inflight(), 0);
        assert_eq!(f.deadline(), None);
    }
}

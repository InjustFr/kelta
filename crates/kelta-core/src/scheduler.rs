//! The deadline-heap scheduler (ARCHITECTURE §8.4, D11): the only owner of periodic work.
//!
//! One task. It waits on its command channel and, only while at least one subscription has a
//! deadline, on the earliest deadline. No subscription (or every interval off / every account
//! paused) → no timer armed, no wakeups. Intervals follow window focus (`polling.*`), get ±10 %
//! jitter, respect `polling.min_secs` and the Redmine floor (60 s), and accounts pause on
//! `NeedsAuth` (until settings change / manual refresh) or offline (until focus / manual refresh).

use std::cmp::Reverse;
use std::collections::{BTreeMap, BinaryHeap, HashMap};
use std::sync::{Arc, Weak};
use std::time::Duration;

use async_trait::async_trait;
use futures::FutureExt;
use kelta_proto::error::{ErrorCode, KeltaError};
use kelta_proto::ids::AccountId;
use kelta_proto::ipc::WindowState;
use kelta_proto::settings::{PollingSettings, WhenClosed};
use parking_lot::Mutex;
use tokio::sync::mpsc;
use tokio::time::Instant;

use crate::rt::{Rt, TimerGauge};

/// `(account, query)`; identical queries across projects share one subscription.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SubKey {
    pub account: AccountId,
    /// `tickets:<view hash>` | `reviews:review_requested` | `reviews:authored`.
    pub query: String,
}

/// Interval inputs of one subscription.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IntervalPolicy {
    pub focused_secs: u32,
    /// Unfocused; 0 = off.
    pub background_secs: u32,
    /// `polling.when_closed = background`.
    pub when_closed: bool,
    pub min_secs: u32,
    /// Provider floor (Redmine 60).
    pub floor_secs: u32,
    /// `accounts.<id>.poll_secs`.
    pub override_secs: Option<u32>,
}

impl IntervalPolicy {
    pub fn from_settings(p: &PollingSettings, floor_secs: u32, override_secs: Option<u32>) -> Self {
        Self {
            focused_secs: p.focused_secs,
            background_secs: p.background_secs,
            when_closed: p.when_closed == WhenClosed::Background,
            min_secs: p.min_secs,
            floor_secs,
            override_secs,
        }
    }

    /// Base interval for a window state (`None` = polling off).
    pub fn interval(&self, w: WindowState) -> Option<Duration> {
        let base = if w.exists && w.visible && w.focused {
            self.focused_secs
        } else if (w.exists && w.visible) || self.when_closed {
            self.background_secs
        } else {
            0
        };
        if base == 0 {
            return None;
        }
        let base = match self.override_secs {
            Some(o) if o > 0 => o.max(if w.focused { 0 } else { base }),
            _ => base,
        };
        Some(Duration::from_secs(u64::from(base.max(self.min_secs.max(30)).max(self.floor_secs))))
    }

    pub fn focused_interval(&self) -> Duration {
        let w = WindowState { exists: true, visible: true, focused: true };
        self.interval(w).unwrap_or(Duration::from_secs(u64::from(self.focused_secs.max(30))))
    }
}

/// ±10 % jitter.
pub fn jitter(d: Duration) -> Duration {
    let r = (uuid::Uuid::new_v4().as_u128() & 0xffff) as f64 / 65535.0; // 0..1
    d.mul_f64(0.9 + 0.2 * r)
}

/// What a due subscription runs.
#[async_trait]
pub trait Refresher: Send + Sync {
    async fn refresh(&self, key: &SubKey) -> Result<(), KeltaError>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pause {
    NeedsAuth,
    Offline,
}

enum Cmd {
    Set(Vec<(SubKey, IntervalPolicy)>),
    Window(WindowState),
    Kick(Option<AccountId>),
    ResumeAll,
    Done(SubKey, Result<(), KeltaError>),
}

#[derive(Debug, Clone)]
struct Sub {
    policy: IntervalPolicy,
    due: Option<Instant>,
    last: Option<Instant>,
    inflight: bool,
    /// Kicked while in flight: re-poll as soon as the running poll completes.
    rerun: bool,
}

/// Observable state (tests, perf).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SchedulerSnapshot {
    pub subscriptions: usize,
    pub armed: bool,
    pub paused: Vec<(AccountId, String)>,
    pub refreshes: u64,
}

struct State {
    subs: BTreeMap<SubKey, Sub>,
    paused: HashMap<AccountId, Pause>,
    window: WindowState,
    refreshes: u64,
}

impl State {
    fn sched(&self, policy: &IntervalPolicy, from: Instant) -> Option<Instant> {
        policy.interval(self.window).map(|iv| from + jitter(iv))
    }

    fn set(&mut self, list: Vec<(SubKey, IntervalPolicy)>, now: Instant) {
        let mut next = BTreeMap::new();
        for (k, policy) in list {
            let sub = match self.subs.remove(&k) {
                Some(mut s) => {
                    if s.policy != policy {
                        s.policy = policy;
                        if !s.inflight {
                            s.due = self.sched(&policy, s.last.unwrap_or(now)).map(|d| d.max(now));
                        }
                    }
                    s
                }
                None => {
                    Sub { policy, due: self.sched(&policy, now), last: None, inflight: false, rerun: false }
                }
            };
            next.insert(k, sub);
        }
        self.subs = next;
    }

    fn window(&mut self, w: WindowState, now: Instant) {
        let gained_focus = w.focused && w.visible && !(self.window.focused && self.window.visible);
        self.window = w;
        if gained_focus {
            self.paused.retain(|_, p| *p != Pause::Offline);
        }
        let win = self.window;
        for s in self.subs.values_mut() {
            if s.inflight {
                continue;
            }
            match s.policy.interval(win) {
                None => s.due = None,
                Some(iv) => {
                    let stale = s.last.is_none_or(|l| now.duration_since(l) > s.policy.focused_interval());
                    s.due = Some(if gained_focus && stale {
                        now
                    } else {
                        s.last.map(|l| (l + iv).max(now)).unwrap_or(now + jitter(iv))
                    });
                }
            }
        }
    }

    fn kick(&mut self, account: Option<&AccountId>, now: Instant) {
        match account {
            Some(a) => {
                self.paused.remove(a);
            }
            None => self.paused.clear(),
        }
        for (k, s) in self.subs.iter_mut() {
            if account.is_none_or(|a| &k.account == a) {
                if s.inflight {
                    s.rerun = true;
                } else {
                    s.due = Some(now);
                }
            }
        }
    }

    fn done(&mut self, key: &SubKey, r: Result<(), KeltaError>, now: Instant) {
        self.refreshes += 1;
        let win = self.window;
        let paused = match &r {
            Err(e) if e.code == ErrorCode::NeedsAuth => Some(Pause::NeedsAuth),
            Err(e) if e.code == ErrorCode::Network => Some(Pause::Offline),
            _ => None,
        };
        if let Some(p) = paused {
            self.paused.insert(key.account.clone(), p);
        }
        let Some(s) = self.subs.get_mut(key) else { return };
        s.inflight = false;
        s.last = Some(now);
        s.due = s.policy.interval(win).map(|iv| {
            let iv = match &r {
                Err(e) if e.code == ErrorCode::RateLimited => {
                    iv.max(Duration::from_millis(e.retry_after_ms.unwrap_or(0)))
                }
                _ => iv,
            };
            now + jitter(iv)
        });
        if std::mem::take(&mut s.rerun) {
            s.due = Some(now);
        }
    }

    fn runnable(&self, k: &SubKey, s: &Sub) -> bool {
        !s.inflight && !self.paused.contains_key(&k.account)
    }

    fn next_deadline(&self) -> Option<Instant> {
        let mut heap: BinaryHeap<Reverse<Instant>> = BinaryHeap::new();
        for (k, s) in &self.subs {
            if let Some(d) = s.due
                && self.runnable(k, s)
            {
                heap.push(Reverse(d));
            }
        }
        heap.peek().map(|Reverse(d)| *d)
    }

    fn take_due(&mut self, now: Instant) -> Vec<SubKey> {
        let keys: Vec<SubKey> = self
            .subs
            .iter()
            .filter(|(k, s)| self.runnable(k, s) && s.due.is_some_and(|d| d <= now))
            .map(|(k, _)| k.clone())
            .collect();
        for k in &keys {
            if let Some(s) = self.subs.get_mut(k) {
                s.inflight = true;
                s.due = None;
            }
        }
        keys
    }

    fn snapshot(&self, armed: bool) -> SchedulerSnapshot {
        SchedulerSnapshot {
            subscriptions: self.subs.len(),
            armed,
            paused: self
                .paused
                .iter()
                .map(|(a, p)| {
                    (a.clone(), if *p == Pause::NeedsAuth { "needs_auth".into() } else { "offline".into() })
                })
                .collect(),
            refreshes: self.refreshes,
        }
    }
}

pub struct Scheduler {
    tx: mpsc::UnboundedSender<Cmd>,
    rx: Mutex<Option<mpsc::UnboundedReceiver<Cmd>>>,
    refresher: Weak<dyn Refresher>,
    gauge: TimerGauge,
    snap: Arc<Mutex<SchedulerSnapshot>>,
}

impl Scheduler {
    pub fn new(refresher: Weak<dyn Refresher>, gauge: TimerGauge) -> Self {
        let (tx, rx) = mpsc::unbounded_channel();
        Self {
            tx,
            rx: Mutex::new(Some(rx)),
            refresher,
            gauge,
            snap: Arc::new(Mutex::new(SchedulerSnapshot::default())),
        }
    }

    /// Start the task (once). Commands sent before are buffered.
    pub fn run(&self, rt: &Rt) {
        let Some(rx) = self.rx.lock().take() else { return };
        let fut =
            run_loop(rx, self.tx.clone(), self.refresher.clone(), self.gauge.clone(), self.snap.clone());
        rt.spawn(fut);
    }

    pub fn set_subscriptions(&self, subs: Vec<(SubKey, IntervalPolicy)>) {
        let _ = self.tx.send(Cmd::Set(subs));
    }

    pub fn window_changed(&self, w: WindowState) {
        let _ = self.tx.send(Cmd::Window(w));
    }

    /// Refresh now (manual refresh / after a write); also un-pauses.
    pub fn kick(&self, account: Option<AccountId>) {
        let _ = self.tx.send(Cmd::Kick(account));
    }

    /// Settings changed: un-pause every account.
    pub fn resume_all(&self) {
        let _ = self.tx.send(Cmd::ResumeAll);
    }

    pub fn snapshot(&self) -> SchedulerSnapshot {
        self.snap.lock().clone()
    }
}

async fn run_loop(
    mut rx: mpsc::UnboundedReceiver<Cmd>,
    tx: mpsc::UnboundedSender<Cmd>,
    refresher: Weak<dyn Refresher>,
    gauge: TimerGauge,
    snap: Arc<Mutex<SchedulerSnapshot>>,
) {
    let mut st = State {
        subs: BTreeMap::new(),
        paused: HashMap::new(),
        window: WindowState { exists: true, visible: true, focused: true },
        refreshes: 0,
    };
    loop {
        let next = st.next_deadline();
        *snap.lock() = st.snapshot(next.is_some());
        let cmd = match next {
            Some(deadline) => {
                // The single scheduler deadline (counted in perf_snapshot.timers_armed).
                let _armed = gauge.guard();
                tokio::select! {
                    c = rx.recv() => match c { Some(c) => Some(c), None => break },
                    _ = tokio::time::sleep_until(deadline) => None,
                }
            }
            None => match rx.recv().await {
                Some(c) => Some(c),
                None => break,
            },
        };
        let now = Instant::now();
        match cmd {
            Some(Cmd::Set(list)) => st.set(list, now),
            Some(Cmd::Window(w)) => st.window(w, now),
            Some(Cmd::Kick(a)) => st.kick(a.as_ref(), now),
            Some(Cmd::ResumeAll) => {
                st.paused.clear();
                for s in st.subs.values_mut() {
                    if s.due.is_none() && !s.inflight {
                        s.due = Some(now);
                    }
                }
            }
            Some(Cmd::Done(k, r)) => st.done(&k, r, now),
            None => {}
        }
        for key in st.take_due(now) {
            let Some(r) = refresher.upgrade() else { return };
            let tx = tx.clone();
            tokio::spawn(async move {
                // A panicking provider must still report Done, or the sub stays inflight forever.
                let res = std::panic::AssertUnwindSafe(r.refresh(&key))
                    .catch_unwind()
                    .await
                    .unwrap_or_else(|_| Err(KeltaError::internal("refresh panicked")));
                let _ = tx.send(Cmd::Done(key, res));
            });
        }
    }
    *snap.lock() = SchedulerSnapshot::default();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn intervals() {
        let p = IntervalPolicy::from_settings(&PollingSettings::default(), 0, None);
        let focused = WindowState { exists: true, visible: true, focused: true };
        let unfocused = WindowState { exists: true, visible: true, focused: false };
        let closed = WindowState { exists: false, visible: false, focused: false };
        assert_eq!(p.interval(focused), Some(Duration::from_secs(120)));
        assert_eq!(p.interval(unfocused), Some(Duration::from_secs(600)));
        assert_eq!(p.interval(closed), None);
        let redmine = IntervalPolicy {
            focused_secs: 30,
            ..IntervalPolicy::from_settings(&PollingSettings::default(), 60, None)
        };
        assert_eq!(redmine.interval(focused), Some(Duration::from_secs(60)));
        let j = jitter(Duration::from_secs(100));
        assert!(j >= Duration::from_secs(90) && j <= Duration::from_secs(110));
    }

    #[test]
    fn kick_during_inflight_poll_reruns_once() {
        let focused = WindowState { exists: true, visible: true, focused: true };
        let mut st = State { subs: BTreeMap::new(), paused: HashMap::new(), window: focused, refreshes: 0 };
        let key = SubKey { account: AccountId::new("acc"), query: "tickets:x".into() };
        let policy = IntervalPolicy::from_settings(&PollingSettings::default(), 0, None);
        let t0 = Instant::now();
        st.set(vec![(key.clone(), policy)], t0);
        st.kick(None, t0);
        assert_eq!(st.take_due(t0), vec![key.clone()]);
        st.kick(None, t0); // arrives while the poll is in flight
        assert!(st.take_due(t0).is_empty());
        st.done(&key, Ok(()), t0);
        assert_eq!(st.take_due(t0), vec![key.clone()], "re-polls right after the in-flight poll");
        st.done(&key, Ok(()), t0);
        assert!(st.take_due(t0).is_empty(), "rerun flag is consumed");
    }
}

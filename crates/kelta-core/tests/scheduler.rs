//! Scheduler with paused tokio time: no subscription → no wakeups; intervals follow focus;
//! NeedsAuth pauses; manual refresh resumes; an idle core arms 0 timers.

mod common;

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Weak};
use std::time::Duration;

use async_trait::async_trait;
use common::*;
use kelta_core::rt::Rt;
use kelta_core::scheduler::{IntervalPolicy, Refresher, Scheduler, SubKey};
use kelta_proto::error::KeltaError;
use kelta_proto::ids::AccountId;
use kelta_proto::ipc::WindowState;
use kelta_proto::settings::{AccountKind, PollingSettings, Settings};
use parking_lot::Mutex;

#[derive(Default)]
struct Counter {
    n: AtomicU32,
    fail: Mutex<Option<KeltaError>>,
}

#[async_trait]
impl Refresher for Counter {
    async fn refresh(&self, _key: &SubKey) -> Result<(), KeltaError> {
        self.n.fetch_add(1, Ordering::SeqCst);
        match self.fail.lock().take() {
            Some(e) => Err(e),
            None => Ok(()),
        }
    }
}

async fn wait(secs: u64) {
    tokio::time::sleep(Duration::from_secs(secs)).await;
    settle().await;
}

fn key() -> SubKey {
    SubKey { account: AccountId::new("github-work"), query: "reviews:github-work:review_requested".into() }
}

fn setup() -> (Arc<Counter>, Rt, Scheduler) {
    let c = Arc::new(Counter::default());
    let as_dyn: Arc<dyn Refresher> = c.clone();
    let weak: Weak<dyn Refresher> = Arc::downgrade(&as_dyn);
    std::mem::forget(as_dyn); // keep the refresher alive for the test
    let rt = Rt::new();
    let s = Scheduler::new(weak, rt.timers.clone());
    s.run(&rt);
    (c, rt, s)
}

#[tokio::test(start_paused = true)]
async fn no_subscription_means_no_wakeups() {
    let (c, rt, s) = setup();
    wait(3600).await;
    assert_eq!(c.n.load(Ordering::SeqCst), 0);
    assert_eq!(rt.timers.get(), 0);
    assert!(!s.snapshot().armed);

    let policy = IntervalPolicy::from_settings(&PollingSettings::default(), 0, None); // 120 s focused
    s.set_subscriptions(vec![(key(), policy)]);
    settle().await;
    assert_eq!(rt.timers.get(), 1);
    assert_eq!(s.snapshot().subscriptions, 1);
    wait(140).await;
    assert_eq!(c.n.load(Ordering::SeqCst), 1);
    wait(130).await;
    assert_eq!(c.n.load(Ordering::SeqCst), 2);

    // dropping the subscription disarms everything
    s.set_subscriptions(vec![]);
    settle().await;
    assert_eq!(rt.timers.get(), 0);
    wait(3600).await;
    assert_eq!(c.n.load(Ordering::SeqCst), 2);
}

#[tokio::test(start_paused = true)]
async fn focus_changes_rearm_and_needs_auth_pauses() {
    let (c, rt, s) = setup();
    let policy = IntervalPolicy::from_settings(&PollingSettings::default(), 0, None);
    s.set_subscriptions(vec![(key(), policy)]);
    wait(140).await;
    assert_eq!(c.n.load(Ordering::SeqCst), 1);

    // unfocused: background interval (600 s)
    s.window_changed(WindowState { exists: true, visible: true, focused: false });
    wait(300).await;
    assert_eq!(c.n.load(Ordering::SeqCst), 1);
    // regaining focus with data older than the focused interval refreshes at once
    s.window_changed(WindowState { exists: true, visible: true, focused: true });
    settle().await;
    wait(1).await;
    assert_eq!(c.n.load(Ordering::SeqCst), 2);

    // window closed (when_closed = off): no timer
    s.window_changed(WindowState { exists: false, visible: false, focused: false });
    settle().await;
    assert_eq!(rt.timers.get(), 0);
    wait(7200).await;
    assert_eq!(c.n.load(Ordering::SeqCst), 2);
    s.window_changed(WindowState { exists: true, visible: true, focused: true });
    wait(1).await;
    assert_eq!(c.n.load(Ordering::SeqCst), 3);

    // NeedsAuth pauses the account until a manual refresh
    *c.fail.lock() = Some(KeltaError::needs_auth("401"));
    wait(140).await;
    assert_eq!(c.n.load(Ordering::SeqCst), 4);
    assert_eq!(s.snapshot().paused.len(), 1);
    assert_eq!(rt.timers.get(), 0);
    wait(3600).await;
    assert_eq!(c.n.load(Ordering::SeqCst), 4);
    s.kick(None);
    wait(1).await;
    assert_eq!(c.n.load(Ordering::SeqCst), 5);
    assert!(s.snapshot().paused.is_empty());

    // Redmine floor: 30 s focused setting still polls every 60 s
    let redmine = IntervalPolicy::from_settings(&PollingSettings { focused_secs: 30, ..PollingSettings::default() }, 60, None);
    assert_eq!(redmine.interval(WindowState { exists: true, visible: true, focused: true }), Some(Duration::from_secs(60)));
}

#[tokio::test(start_paused = true)]
async fn idle_core_arms_no_timer() {
    let tmp = tempfile::tempdir().unwrap();
    let h = start(tmp.path(), Settings::defaults(), vec![project("shop", tmp.path())]);
    settle().await;
    wait(3600).await;
    assert_eq!(h.core.perf_snapshot().timers_armed, 0);
    assert_eq!(h.core.scheduler_snapshot().subscriptions, 0);

    // a code-host account + review notifications → background review subscriptions
    h.cfg.update(|s| {
        s.accounts.insert("github-work".into(), account(AccountKind::Github));
    });
    settle().await;
    assert_eq!(h.core.scheduler_snapshot().subscriptions, 2);
    assert_eq!(h.core.perf_snapshot().timers_armed, 1);
    // notifications off and no visible pane → nothing to poll
    h.cfg.update(|s| s.notifications.enabled = false);
    settle().await;
    assert_eq!(h.core.scheduler_snapshot().subscriptions, 0);
    assert_eq!(h.core.perf_snapshot().timers_armed, 0);
}

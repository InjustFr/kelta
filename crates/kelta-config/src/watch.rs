//! Hot reload: directory watches with a 250 ms settle window (SETTINGS §7).
//!
//! `notify`'s own watcher thread only records a deadline; one blocked "settle" thread waits on a
//! condvar (zero wakeups while idle, a sliding deadline while events arrive) and then calls
//! [`ConfigService::reload`], which compares content hashes so the echo of our own writes and
//! editor rename-saves of unchanged content are no-ops. A burst of events yields one reload.
//! (`notify-debouncer-full` polls on a fixed tick forever, which would break the idle-wakeup
//! budget of ARCHITECTURE §13.)

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use notify::{EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use parking_lot::{Condvar, Mutex};

use crate::service::ConfigService;

/// Settle window after the last file event.
pub const SETTLE: Duration = Duration::from_millis(250);

struct Signal {
    deadline: Mutex<Option<Instant>>,
    cv: Condvar,
    stop: AtomicBool,
}

impl Signal {
    fn bump(&self) {
        *self.deadline.lock() = Some(Instant::now() + SETTLE);
        self.cv.notify_one();
    }
}

/// Keeps the watcher and the settle thread alive; dropping it stops both.
pub struct WatchHandle {
    signal: Arc<Signal>,
    _watcher: Arc<Mutex<RecommendedWatcher>>,
}

impl Drop for WatchHandle {
    fn drop(&mut self) {
        self.signal.stop.store(true, Ordering::Release);
        self.signal.cv.notify_all();
    }
}

fn relevant(ev: &notify::Event) -> bool {
    if matches!(ev.kind, EventKind::Access(_)) {
        return false;
    }
    ev.paths.is_empty()
        || ev
            .paths
            .iter()
            .any(|p| !p.file_name().is_some_and(|n| n.to_string_lossy().contains(".kelta-tmp-")))
}

fn wanted_dirs(svc: &ConfigService) -> Vec<PathBuf> {
    let mut dirs = vec![svc.dirs.config.clone(), svc.dirs.projects_dir()];
    for f in svc.repo_config_paths() {
        if let Some(d) = f.parent()
            && d.is_dir()
        {
            dirs.push(d.to_owned());
        }
    }
    dirs
}

fn sync_watches(svc: &ConfigService, watcher: &Mutex<RecommendedWatcher>, watched: &mut HashSet<PathBuf>) {
    let mut w = watcher.lock();
    for d in wanted_dirs(svc) {
        if watched.contains(&d) {
            continue;
        }
        if w.watch(&d, RecursiveMode::NonRecursive).is_ok() {
            watched.insert(d);
        }
    }
}

pub(crate) fn start(svc: Arc<ConfigService>) -> Result<WatchHandle, notify::Error> {
    // the watch dirs must exist before they can be watched
    let _ = std::fs::create_dir_all(svc.dirs.projects_dir());
    let signal =
        Arc::new(Signal { deadline: Mutex::new(None), cv: Condvar::new(), stop: AtomicBool::new(false) });
    let handler_signal = signal.clone();
    let watcher = notify::recommended_watcher(move |res: notify::Result<notify::Event>| match res {
        Ok(ev) => {
            if relevant(&ev) {
                handler_signal.bump();
            }
        }
        Err(_) => handler_signal.bump(),
    })?;
    let watcher = Arc::new(Mutex::new(watcher));
    let mut watched = HashSet::new();
    sync_watches(&svc, &watcher, &mut watched);

    let weak = Arc::downgrade(&svc);
    drop(svc);
    let thread_signal = signal.clone();
    let thread_watcher = watcher.clone();
    // allowlisted: the single blocked settle thread of the hot-reload debounce (SETTINGS §7);
    // it waits on a condvar and has no wakeups while idle.
    std::thread::Builder::new()
        .name("kelta-config-settle".into())
        .stack_size(256 * 1024)
        .spawn(move || {
            loop {
                let mut guard = thread_signal.deadline.lock();
                // wait for the first event
                while guard.is_none() {
                    if thread_signal.stop.load(Ordering::Acquire) {
                        return;
                    }
                    thread_signal.cv.wait(&mut guard);
                }
                // wait until the deadline stops moving
                loop {
                    if thread_signal.stop.load(Ordering::Acquire) {
                        return;
                    }
                    let Some(deadline) = *guard else { break };
                    if Instant::now() >= deadline {
                        break;
                    }
                    thread_signal.cv.wait_until(&mut guard, deadline);
                }
                *guard = None;
                drop(guard);
                let Some(svc) = weak.upgrade() else { return };
                let _ = svc.reload();
                sync_watches(&svc, &thread_watcher, &mut watched);
            }
        })
        .map_err(|e| notify::Error::generic(&e.to_string()))?;
    Ok(WatchHandle { signal, _watcher: watcher })
}

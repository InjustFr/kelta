//! Runtime glue: the tokio handle core spawns on, a queue for work submitted before a runtime is
//! reachable (`Core::start` may run on the Tauri main thread outside the runtime context), and the
//! one-shot timers (`// one-shot:`) whose count feeds `perf_snapshot.timers_armed`.

use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::Duration;

use parking_lot::Mutex;
use tokio::runtime::Handle;
use tokio::task::AbortHandle;

type BoxFut = Pin<Box<dyn Future<Output = ()> + Send + 'static>>;

/// Counts armed timers (scheduler deadline + one-shots).
#[derive(Clone, Default)]
pub struct TimerGauge(Arc<AtomicU32>);

impl TimerGauge {
    pub fn get(&self) -> u32 {
        self.0.load(Ordering::SeqCst)
    }

    /// Increment now; decrement when the guard drops.
    pub fn guard(&self) -> TimerGuard {
        self.0.fetch_add(1, Ordering::SeqCst);
        TimerGuard(self.0.clone())
    }
}

/// Decrements the gauge on drop.
pub struct TimerGuard(Arc<AtomicU32>);

impl Drop for TimerGuard {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::SeqCst);
    }
}

/// Spawner bound to the app runtime once one is reachable.
pub struct Rt {
    handle: OnceLock<Handle>,
    pending: Mutex<Vec<BoxFut>>,
    pub timers: TimerGauge,
}

impl Default for Rt {
    fn default() -> Self {
        Self::new()
    }
}

impl Rt {
    pub fn new() -> Self {
        let rt =
            Self { handle: OnceLock::new(), pending: Mutex::new(Vec::new()), timers: TimerGauge::default() };
        if let Ok(h) = Handle::try_current() {
            let _ = rt.handle.set(h);
        }
        rt
    }

    /// Bind to the current runtime if not bound yet (called from every async entry point) and run
    /// the queued work.
    pub fn capture(&self) {
        if self.handle.get().is_none()
            && let Ok(h) = Handle::try_current()
        {
            let _ = self.handle.set(h);
        }
        self.flush();
    }

    fn flush(&self) {
        let Some(h) = self.handle.get() else { return };
        let queued: Vec<BoxFut> = std::mem::take(&mut *self.pending.lock());
        for f in queued {
            h.spawn(f);
        }
    }

    pub fn handle(&self) -> Option<Handle> {
        if self.handle.get().is_none() {
            self.capture();
        }
        self.handle.get().cloned()
    }

    /// Spawn now, or queue until a runtime is reachable (then `None` is returned).
    pub fn spawn<F>(&self, f: F) -> Option<AbortHandle>
    where
        F: Future<Output = ()> + Send + 'static,
    {
        match self.handle() {
            Some(h) => Some(h.spawn(f).abort_handle()),
            None => {
                self.pending.lock().push(Box::pin(f));
                None
            }
        }
    }

    /// Run blocking work off the async workers (inline when no runtime is reachable).
    pub async fn blocking<R, F>(&self, f: F) -> R
    where
        F: FnOnce() -> R + Send + 'static,
        R: Send + 'static,
    {
        match self.handle() {
            Some(h) => match h.spawn_blocking(f).await {
                Ok(r) => r,
                Err(e) => std::panic::resume_unwind(e.into_panic()),
            },
            None => f(),
        }
    }
}

/// A re-armable one-shot timer. Arming replaces (aborts) the previous deadline.
#[derive(Default)]
pub struct OneShot {
    abort: Mutex<Option<AbortHandle>>,
}

impl OneShot {
    pub fn arm<F>(&self, rt: &Rt, after: Duration, f: F)
    where
        F: Future<Output = ()> + Send + 'static,
    {
        let guard = rt.timers.guard();
        let fut = async move {
            // one-shot: armed by an event (spawn / output), never re-armed by itself.
            tokio::time::sleep(after).await;
            drop(guard);
            f.await;
        };
        let handle = rt.spawn(fut);
        let old = std::mem::replace(&mut *self.abort.lock(), handle);
        if let Some(old) = old {
            old.abort();
        }
    }

    pub fn cancel(&self) {
        if let Some(h) = self.abort.lock().take() {
            h.abort();
        }
    }
}

impl Drop for OneShot {
    fn drop(&mut self) {
        self.cancel();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test(start_paused = true)]
    async fn one_shot_counts_and_cancels() {
        let rt = Rt::new();
        let t = OneShot::default();
        let hit = Arc::new(AtomicU32::new(0));
        let h2 = hit.clone();
        t.arm(&rt, Duration::from_secs(3), async move {
            h2.fetch_add(1, Ordering::SeqCst);
        });
        assert_eq!(rt.timers.get(), 1);
        t.cancel();
        tokio::task::yield_now().await;
        assert_eq!(rt.timers.get(), 0);
        let h3 = hit.clone();
        t.arm(&rt, Duration::from_secs(1), async move {
            h3.fetch_add(1, Ordering::SeqCst);
        });
        tokio::time::sleep(Duration::from_secs(2)).await;
        assert_eq!(hit.load(Ordering::SeqCst), 1);
        assert_eq!(rt.timers.get(), 0);
    }
}

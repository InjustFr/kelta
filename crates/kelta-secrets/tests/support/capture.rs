//! A tiny `tracing` subscriber that records every event field (to grep captured logs).

use std::fmt::Write as _;
use std::sync::Arc;

use parking_lot::Mutex;
use tracing::field::{Field, Visit};
use tracing::span::{Attributes, Id, Record};
use tracing::{Event, Metadata, Subscriber};

#[derive(Clone, Default)]
pub struct Captured(Arc<Mutex<String>>);

impl Captured {
    pub fn contents(&self) -> String {
        self.0.lock().clone()
    }
}

struct Sub(Captured);

struct V<'a>(&'a mut String);

impl Visit for V<'_> {
    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        let _ = write!(self.0, " {}={:?}", field.name(), value);
    }
}

impl Subscriber for Sub {
    fn enabled(&self, _: &Metadata<'_>) -> bool {
        true
    }
    fn new_span(&self, attrs: &Attributes<'_>) -> Id {
        let mut s = String::new();
        attrs.record(&mut V(&mut s));
        self.0.0.lock().push_str(&format!("span{s}\n"));
        Id::from_u64(1)
    }
    fn record(&self, _: &Id, _: &Record<'_>) {}
    fn record_follows_from(&self, _: &Id, _: &Id) {}
    fn event(&self, event: &Event<'_>) {
        let mut s = String::new();
        event.record(&mut V(&mut s));
        self.0.0.lock().push_str(&format!("{} {}{s}\n", event.metadata().level(), event.metadata().target()));
    }
    fn enter(&self, _: &Id) {}
    fn exit(&self, _: &Id) {}
}

/// Install for the current thread (tests use the current-thread runtime).
pub fn install() -> Captured {
    let cap = Captured::default();
    let guard = tracing::subscriber::set_default(Sub(cap.clone()));
    std::mem::forget(guard);
    cap
}

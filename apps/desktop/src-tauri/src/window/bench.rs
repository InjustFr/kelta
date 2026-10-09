//! kelta-bench hooks, active only when `KELTA_BENCH_MARKS` names a file: metrics are merged into
//! that JSON object, and `custom.bench.close_window` closes the window like the user would.

use std::sync::OnceLock;
use std::time::Instant;

static START: OnceLock<Instant> = OnceLock::new();

fn marks_path() -> Option<std::path::PathBuf> {
    std::env::var_os("KELTA_BENCH_MARKS").map(Into::into)
}

pub fn enabled() -> bool {
    marks_path().is_some()
}

/// Call once at the start of `window::setup`.
pub fn started() {
    let _ = START.set(Instant::now());
}

/// Merges `key = value` into the marks file.
pub fn mark(key: &str, value: f64) {
    let Some(path) = marks_path() else { return };
    let mut marks: serde_json::Map<String, serde_json::Value> =
        std::fs::read_to_string(&path).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default();
    marks.insert(key.into(), value.into());
    let _ = std::fs::write(path, serde_json::Value::Object(marks).to_string());
}

/// `app_ready` (the UI's first frame). shortcut: measured from window setup, not process start;
/// upgrade if cold-start budgets need the pre-setup time.
pub fn app_ready() {
    if let Some(t) = START.get() {
        mark("app_ready_ms", t.elapsed().as_secs_f64() * 1000.0);
    }
}

// Fixture for scripts/check-no-timers.test.sh: `BAD` lines must be reported, `OK` lines must not.
use tokio::time;
use tokio::time::{interval, Duration};

#[cfg(test)]
fn helper() {
    let _ = time::interval(Duration::from_secs(1)); // OK: test-only helper
}

async fn grouped() {
    let mut t = interval(Duration::from_secs(1)); // BAD
    let _ = time::interval(Duration::from_secs(1)); // BAD
    let _ = self.interval(); // OK: a method
}

#[cfg(test)]
use std::thread;

fn after_test_items() {
    std::thread::spawn(|| {}); // BAD
}

#[cfg(test)]
mod tests {
    #[test]
    fn t() {
        std::thread::sleep(std::time::Duration::from_millis(1)); // OK: test module
    }
}

//! Shared by the snapshot tests and the criterion benches.

/// Build-log-like workload (≈ 90 columns per line, a few SGR runs).
pub fn build_log(lines: usize) -> Vec<u8> {
    let mut s = String::new();
    for i in 0..lines {
        s.push_str(&format!(
            "\x1b[32m{i:>6}\x1b[0m \x1b[1;32mCompiling\x1b[0m crate-{} v0.{}.{} (/home/dev/src/workspace/crates/crate-{}{})\r\n",
            i % 97,
            i % 13,
            i % 7,
            i % 97,
            "/src".repeat(i % 5)
        ));
        if i % 10 == 0 {
            s.push_str("\x1b[33mwarning\x1b[0m: unused variable: `x` \x1b[2m--> src/lib.rs:12:9\x1b[0m\r\n");
        }
    }
    s.into_bytes()
}

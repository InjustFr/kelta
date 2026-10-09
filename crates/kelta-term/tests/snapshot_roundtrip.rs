//! Snapshot round-trip goldens (BUILD_PLAN §4 L1, ARCHITECTURE §9.3): for every recording, feed
//! Term A, encode a snapshot, feed it into a fresh Term B and require identical cells, attributes,
//! cursor, modes and title. The snapshot bytes are pinned with insta.

mod common;
#[path = "common/workload.rs"]
mod workload;

use common::*;
use workload::build_log;
use kelta_term::inspect::{diff, dump};
use kelta_term::model::TermModel;
use proptest::prelude::*;

const HIST: usize = 10_000;

fn round_trip(name: &str, bytes: &[u8], cols: u16, rows: u16, max_history: usize) -> Vec<u8> {
    let mut a = model_from(bytes, cols, rows, HIST);
    let snap = a.snapshot(max_history);
    let mut b = model_from(&snap, cols, rows, HIST);
    let (da, db) = (dump(&mut a, max_history), dump(&mut b, max_history));
    if let Some(d) = diff(&da, &db) {
        panic!("{name} (history {max_history}): snapshot round trip differs:\n{d}");
    }
    // Encoding is a fixed point: B re-encodes to the same bytes.
    assert_eq!(escape(&b.snapshot(max_history)), escape(&snap), "{name}: snapshot of the copy differs");
    snap
}

#[test]
fn recordings_exist() {
    let names: Vec<String> = recordings().into_iter().map(|r| r.name).collect();
    for want in [
        "coverage-text",
        "coverage-cursor",
        "coverage-alt",
        "coverage-edit",
        "tui-sim-ink",
        "lazygit-like",
        "claude-like",
        "nvim",
    ] {
        assert!(names.iter().any(|n| n == want), "missing recording {want}: {names:?}");
    }
}

#[test]
fn every_recording_round_trips() {
    for r in recordings() {
        let snap = round_trip(&r.name, &r.bytes, r.cols, r.rows, HIST);
        insta::assert_snapshot!(format!("snapshot-{}", r.name), escape(&snap));
    }
}

#[test]
fn every_recording_round_trips_with_limited_history() {
    for r in recordings() {
        round_trip(&r.name, &r.bytes, r.cols, r.rows, 50);
        round_trip(&r.name, &r.bytes, r.cols, r.rows, 0);
    }
}

#[test]
fn every_recording_round_trips_after_resize() {
    for r in recordings() {
        let mut a = model_from(&r.bytes, r.cols, r.rows, HIST);
        a.resize(r.cols - 13, r.rows + 3);
        let snap = a.snapshot(HIST);
        let mut b = model_from(&snap, r.cols - 13, r.rows + 3, HIST);
        if let Some(d) = diff(&dump(&mut a, HIST), &dump(&mut b, HIST)) {
            panic!("{} after resize: {d}", r.name);
        }
    }
}

/// `CSI Ps b` (REP) repeats the last printed character, which a snapshot cannot reproduce.
fn starts_with_rep(rest: &[u8]) -> bool {
    rest.starts_with(b"\x1b[") && {
        let tail = &rest[2..];
        let digits = tail.iter().take_while(|b| b.is_ascii_digit()).count();
        tail.get(digits) == Some(&b'b')
    }
}

/// Mid-stream attach: A has seen `k` bytes (in arbitrary chunks) when the snapshot is taken; the
/// rest of the stream then goes to both A and B (the view), which must end identical.
fn mid_stream(bytes: &[u8], cols: u16, rows: u16, k: usize, chunk: usize) -> Result<(), String> {
    let mut a = TermModel::new(cols, rows, HIST);
    for c in bytes[..k].chunks(chunk.max(1)) {
        a.feed(c);
    }
    let snap = a.snapshot(HIST);
    let mut b = model_from(&snap, cols, rows, HIST);
    for c in bytes[k..].chunks(chunk.max(1)) {
        a.feed(c);
        b.feed(c);
    }
    match diff(&dump(&mut a, HIST), &dump(&mut b, HIST)) {
        None => Ok(()),
        Some(d) => Err(d),
    }
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 48, ..ProptestConfig::default() })]

    #[test]
    fn mid_stream_snapshots_compose_with_raw_output(idx in 0usize..64, split in 0.0f64..1.0, chunk in 1usize..5000) {
        let recs = recordings();
        let r = &recs[idx % recs.len()];
        let k = ((r.bytes.len() as f64) * split) as usize;
        prop_assume!(!starts_with_rep(&r.bytes[k..]));
        if let Err(d) = mid_stream(&r.bytes, r.cols, r.rows, k, chunk) {
            return Err(TestCaseError::fail(format!("{} split at {k} (chunk {chunk}): {d}", r.name)));
        }
    }
}

#[test]
fn mid_stream_at_every_offset_of_a_dense_sequence() {
    // Splits inside CSI, OSC, charset designations and multi-byte characters.
    let s = "a\x1b[1;38;2;1;2;3mé\x1b]2;tït\x07\x1b(0q\x1b(B日\x1b]8;id=x;u\x1b\\l\x1b]8;;\x1b\\\x1b[?1049h\x1b[3;4Hz\x1b[?1049lend";
    let bytes = s.as_bytes();
    for k in 0..=bytes.len() {
        if starts_with_rep(&bytes[k..]) {
            continue;
        }
        if let Err(d) = mid_stream(bytes, 20, 5, k, bytes.len()) {
            panic!("split at {k}: {d}");
        }
    }
}

#[test]
fn snapshot_mid_synchronized_update() {
    let frame = "\x1b[?2026h\x1b[H\x1b[2Jframe one\r\nsecond line\x1b[?2026l";
    let stream = format!("{frame}{}", frame.replace("one", "two"));
    let bytes = stream.as_bytes();
    for k in 0..=bytes.len() {
        if let Err(d) = mid_stream(bytes, 30, 6, k, 7) {
            panic!("split at {k}: {d}");
        }
    }
}

#[test]
fn sync_is_always_emitted_off_and_title_cwd_present() {
    let r = recordings().into_iter().find(|r| r.name == "coverage-text").unwrap();
    let snap = model_from(&r.bytes, r.cols, r.rows, HIST).snapshot(HIST);
    let s = String::from_utf8_lossy(&snap);
    assert!(s.starts_with("\x1bc"));
    assert!(s.contains("\x1b[?2026l"));
    assert!(s.contains("\x1b]2;coverage \u{2713}\x07"));
    assert!(s.contains("\x1b]7;file://host/tmp/cov%20dir\x07"));
    assert!(s.contains("4:3"), "undercurl uses the colon form");
    assert!(s.contains(";58;2;255;0;0"), "underline colour");
    assert!(s.contains("\x1b]8;id=k1;https://example.com/a;b\x1b\\"), "hyperlink with id");
}

#[test]
fn snapshot_budget_200x60_with_1000_lines() {
    let mut m = TermModel::new(200, 60, 3000);
    m.feed(&build_log(1100));
    let snap = m.snapshot(1000);
    assert!(snap.len() <= 150 * 1024, "snapshot is {} bytes", snap.len());
    let mut b = model_from(&snap, 200, 60, 3000);
    assert!(diff(&dump(&mut m, 1000), &dump(&mut b, 1000)).is_none());
}

#[test]
#[ignore = "slow: every split point of every recording (run with --ignored)"]
fn exhaustive_mid_stream() {
    let stride: usize = std::env::var("KELTA_SPLIT_STRIDE").ok().and_then(|s| s.parse().ok()).unwrap_or(1);
    for r in recordings() {
        let mut failures = Vec::new();
        for k in (0..=r.bytes.len()).step_by(stride) {
            if starts_with_rep(&r.bytes[k..]) {
                continue;
            }
            if let Err(d) = mid_stream(&r.bytes, r.cols, r.rows, k, r.bytes.len()) {
                failures.push(format!("{} split at {k}: {d}", r.name));
            }
        }
        assert!(failures.is_empty(), "{} failures, first: {}", failures.len(), failures[0]);
    }
}

#[test]
fn dump_comparison_detects_every_kind_of_difference() {
    let base = b"hello\x1b[3;5H";
    for extra in [
        &b"\x1b]2;t\x07"[..],
        b"\x1b[?1000h",
        b"\x1b[?1015h",
        b"\x1b[5;5H\x1b7\x1b[3;5H",
        b"\x1b(0",
        b"\x0e",
        b"\x1b[2;4r\x1b[3;5H",
        b"\x1b[4 q",
        b"\x1b[31m",
        b"\x1b]8;id=a;u\x1b\\",
        b"\x1b]4;1;rgb:01/02/03\x07",
        b"\x1b]7;file:///x\x07",
        b"\x1b[?1049h",
        b"\x1b[4h",
        b"\x1b=",
        b"\xcc\x81",
    ] {
        let mut a = model_from(base, 20, 5, 100);
        let mut b = model_from(&[&base[..], extra].concat(), 20, 5, 100);
        assert!(diff(&dump(&mut a, 100), &dump(&mut b, 100)).is_some(), "not detected: {}", escape(extra));
    }
}

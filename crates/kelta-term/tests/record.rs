//! Regenerates `fixtures/recordings/*.ansi` (ignored by default):
//!
//! ```text
//! cargo test -p kelta-term --test record -- --ignored --nocapture
//! ```
//!
//! Synthetic streams cover every ARCHITECTURE §9.3 item explicitly; the others are captured from
//! real programs in a PTY (`tui-sim`, `nvim --clean`, `fake-claude`).

#![allow(clippy::unwrap_used, clippy::expect_used)] // test helpers outside #[test] fns

mod common;

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use common::*;
use kelta_proto::api::TerminalHost;
use kelta_proto::ids::SessionId;
use kelta_proto::term::{FRAME_DATA, TerminalLimits};
use kelta_term::PtyTerminalHost;

fn write(name: &str, cols: u16, rows: u16, bytes: &[u8]) {
    let p = recordings_dir().join(format!("{name}.{cols}x{rows}.ansi"));
    std::fs::write(&p, bytes).unwrap();
    println!("wrote {} ({} bytes)", p.display(), bytes.len());
}

/// SGR, colours, underline styles + colour, hyperlinks, wide/combining characters, wraps, tabs,
/// charsets, history.
fn coverage_text() -> Vec<u8> {
    let mut s = String::new();
    s.push_str("\x1b]2;coverage \u{2713}\x07\x1b]7;file://host/tmp/cov%20dir\x07");
    for i in 0..120 {
        s.push_str(&format!("\x1b[3{}mhistory line {i:03}\x1b[0m {}\r\n", i % 8, "·".repeat(i % 40)));
    }
    s.push_str("\x1b[1mbold\x1b[22m \x1b[2mdim\x1b[22m \x1b[3mitalic\x1b[23m \x1b[7minverse\x1b[27m ");
    s.push_str("\x1b[8mhidden\x1b[28m \x1b[9mstrike\x1b[29m \x1b[1;2;3;7;9mall\x1b[0m\r\n");
    s.push_str("\x1b[4msingle\x1b[24m \x1b[4:2mdouble\x1b[4:0m \x1b[4:3;58;2;255;0;0mundercurl\x1b[59;24m ");
    s.push_str("\x1b[4:4;58;5;33mdotted\x1b[0m \x1b[4:5mdashed\x1b[0m \x1b[21mdbl21\x1b[0m\r\n");
    for i in 0..8 {
        s.push_str(&format!("\x1b[3{i}m3{i}\x1b[9{i}m9{i}\x1b[4{i}m4{i}\x1b[10{i}m10{i}\x1b[0m"));
    }
    s.push_str("\r\n");
    for i in (0..256).step_by(17) {
        s.push_str(&format!("\x1b[38;5;{i};48;5;{}m{i:3}", 255 - i));
    }
    s.push_str("\x1b[0m\r\n\x1b[38;2;10;20;30;48;2;200;100;50mtruecolor\x1b[38:2::1:2:3mcolon\x1b[0m\r\n");
    s.push_str("\x1b]8;id=k1;https://example.com/a;b\x1b\\link one\x1b]8;;\x1b\\ plain ");
    s.push_str("\x1b]8;id=k2;file:///tmp/x\x1b\\\x1b[1mbold link\x1b[0m\x1b]8;;\x1b\\\r\n");
    s.push_str("wide 日本語 emoji 😀 combining e\u{301} a\u{308}\u{332} zwj \u{2764}\u{fe0f}\r\n");
    s.push_str("tabs\ta\tbb\tccc\x1b[41m\t\x1b[0mx\r\n");
    s.push_str("\x1b(0lqqqqk\x1b(B ascii \x1b)0\x0exqx\x0f back\r\n");
    // A soft-wrapped logical line and a wide character that does not fit at the end of a row.
    s.push_str(&"wrap-".repeat(40));
    s.push_str("\r\n");
    s.push_str(&"x".repeat(79));
    s.push_str("界after\r\n");
    s.push_str("\x1b[44m\x1b[Kbg erase to eol\x1b[0m\r\n");
    s.push_str("\x1b[42m   \x1b[0m  trailing coloured blanks\x1b[43m  \x1b[0m\r\n");
    s.push_str("end of text");
    s.into_bytes()
}

/// Cursor state: DECSC with attributes and charsets, DECSTBM + origin, modes, shape, pending wrap.
fn coverage_cursor() -> Vec<u8> {
    let mut s = String::new();
    for i in 0..30 {
        s.push_str(&format!("line {i}\r\n"));
    }
    s.push_str("\x1b]4;1;rgb:ff/80/00\x07\x1b]11;rgb:20/20/20\x07\x1b]10;#c0c0c0\x07");
    // Saved cursor with pen, link and G0 = DEC graphics.
    s.push_str(
        "\x1b[5;7H\x1b[1;31;48;5;22m\x1b]8;id=s;https://saved\x1b\\\x1b(0\x1b7\x1b(B\x1b]8;;\x1b\\\x1b[0m",
    );
    // Scroll region + origin mode, modes, keypad, shape.
    s.push_str("\x1b[3;20r\x1b[?6h\x1b[?1h\x1b=\x1b[?2004h\x1b[?1002h\x1b[?1006h\x1b[?1004h\x1b[?1015h");
    s.push_str("\x1b[?25l\x1b[?1007l\x1b[5 q\x1b[20h");
    // Pending wrap on the last column with a pen and G1 shifted in.
    s.push_str("\x1b[4;1H");
    s.push_str(&"P".repeat(80));
    s.push_str("\x1b[3;35m\x1b)0\x0e\x1b]2;cursor test\x07");
    s.into_bytes()
}

/// Alternate screen with main scrollback, mouse modes, saved cursors on both screens.
fn coverage_alt() -> Vec<u8> {
    let mut s = String::new();
    for i in 0..60 {
        s.push_str(&format!("\x1b[3{}mmain {i}\x1b[0m\r\n", i % 7 + 1));
    }
    s.push_str("prompt$ \x1b[1m");
    s.push_str("\x1b[?1049h\x1b[?1000h\x1b[?1005h\x1b[H\x1b[2J");
    s.push_str("\x1b[1;7m Status \x1b[0m  \x1b[32m\u{2713} main\x1b[0m\r\n");
    s.push_str("\x1b(0lqqqqqqk\r\nx file x\r\nmqqqqqqj\x1b(B\r\n");
    for i in 0..10 {
        s.push_str(&format!("  \x1b[33mM\x1b[0m src/file_{i}.rs\r\n"));
    }
    s.push_str("\x1b[10;5H\x1b[4:3m\x1b[58;2;255;0;0mundercurl\x1b[0m\x1b7\x1b[20;70H\x1b[45mend\x1b[0m");
    s.push_str("\x1b[3 q\x1b]2;alt title\x07");
    s.into_bytes()
}

/// Edge cases: scroll region scrolling, insert/delete lines, reverse index, erase with colours.
fn coverage_edit() -> Vec<u8> {
    let mut s = String::new();
    for i in 0..40 {
        s.push_str(&format!("{i:02} abcdefghijklmnopqrstuvwxyz\r\n"));
    }
    s.push_str("\x1b[5;15r\x1b[15;1H");
    for i in 0..8 {
        s.push_str(&format!("\r\nscrolled in region {i}"));
    }
    s.push_str("\x1b[r\x1b[8;1H\x1b[2L\x1b[46minserted\x1b[0m\x1b[12;1H\x1b[3M\x1b[1;1H\x1bM\x1bMtop");
    s.push_str("\x1b[20;10H\x1b[41m\x1b[5X\x1b[0m\x1b[21;10H\x1b[3P\x1b[22;3H\x1b[4@ins\x1b[1J");
    s.push_str("\x1b[24;1H\x1b[?7lno-wrap-");
    s.push_str(&"z".repeat(100));
    s.into_bytes()
}

#[test]
#[ignore = "regenerates fixtures/recordings"]
fn record_synthetic() {
    write("coverage-text", 80, 24, &coverage_text());
    write("coverage-cursor", 80, 24, &coverage_cursor());
    write("coverage-alt", 80, 24, &coverage_alt());
    write("coverage-edit", 80, 24, &coverage_edit());
}

/// Run a program in a PTY, send `inputs` (delay, bytes) and capture the raw output.
fn capture(
    program: &str,
    args: &[&str],
    cols: u16,
    rows: u16,
    inputs: &[(u64, &[u8])],
    max: Duration,
) -> Vec<u8> {
    let host = PtyTerminalHost::new(TerminalLimits::default());
    let events = Arc::new(Events::default());
    let mut sp = spec("rec", program, args, cols, rows, events.clone());
    sp.env.insert("FAKE_CLAUDE_STEP_MS".into(), "50".into());
    sp.env.insert("FAKE_CLAUDE_NO_IDLE".into(), "1".into());
    sp.env.insert("FAKE_CLAUDE_EXIT_AFTER".into(), "1".into());
    host.spawn(sp).unwrap();
    let id = SessionId::new("rec");
    let sink = Frames::default();
    let info = host.attach(&id, cols, rows, Box::new(sink.clone())).unwrap();
    let mut out = Vec::new();
    let start = Instant::now();
    let mut next = 0;
    while start.elapsed() < max {
        for f in sink.take() {
            host.ack(&id, info.generation, kelta_term::frames::ack_len(&f));
            if f[0] == FRAME_DATA {
                out.extend_from_slice(&f[1..]);
            }
        }
        if next < inputs.len() && start.elapsed() >= Duration::from_millis(inputs[next].0) {
            host.write(&id, inputs[next].1).unwrap();
            next += 1;
        }
        if events.exited().is_some() && next >= inputs.len() {
            break;
        }
        #[allow(clippy::disallowed_methods)] // allowlisted: test pacing
        std::thread::sleep(Duration::from_millis(5));
    }
    for f in sink.take() {
        if f[0] == FRAME_DATA {
            out.extend_from_slice(&f[1..]);
        }
    }
    let _ = host.kill(&id, kelta_proto::term::KillSignal::Kill);
    out
}

#[test]
#[ignore = "regenerates fixtures/recordings"]
fn record_programs() {
    let sim = tui_sim();
    let sim = sim.to_str().unwrap();
    // Captured mid-run (cursor hidden, last frame on screen).
    let ink = capture(
        sim,
        &["--ink", "--fps", "30", "--lines", "12", "--duration-ms", "60000"],
        100,
        30,
        &[],
        Duration::from_millis(700),
    );
    write("tui-sim-ink", 100, 30, &ink);
    let alt = capture(
        sim,
        &["--alt", "--duration-ms", "60000"],
        120,
        40,
        &[(300, b"jjk"), (600, b"\x1b[<0;10;5M\x1b[<0;10;5m"), (900, b"\x1b[200~pasted\x1b[201~")],
        // Still on the alternate screen with mouse modes on when the capture stops.
        Duration::from_millis(1300),
    );
    write("lazygit-like", 120, 40, &alt);
    let claude = workspace_root().join("fixtures/fake-claude");
    let claude = capture(claude.to_str().unwrap(), &["fix the bug"], 100, 30, &[], Duration::from_secs(10));
    write("claude-like", 100, 30, &claude);
    if let Some(nvim) = which_nvim() {
        let file = std::env::temp_dir().join("kelta-rec.txt");
        std::fs::write(
            &file,
            (0..200).map(|i| format!("line {i}: the quick brown fox\n")).collect::<String>(),
        )
        .unwrap();
        let nvim_out = capture(
            nvim.to_str().unwrap(),
            &["--clean", "-n", file.to_str().unwrap()],
            100,
            30,
            &[
                (800, b":set number cursorline\r"),
                (1100, b"50Gzz"),
                (1300, b"ohello \xe2\x9c\x93 world\x1b"),
                (1600, b":vsplit\r"),
                (1900, b":set nonumber\r"),
                (2200, b"/fox\r"),
            ],
            Duration::from_secs(3),
        );
        write("nvim", 100, 30, &nvim_out);
    } else {
        println!("nvim not found: skipping the nvim recording");
    }
}

fn which_nvim() -> Option<PathBuf> {
    ["/opt/homebrew/bin/nvim", "/usr/bin/nvim", "/usr/local/bin/nvim"]
        .iter()
        .map(PathBuf::from)
        .find(|p| p.is_file())
}

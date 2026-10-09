//! tui-sim: terminal workload simulator used by kelta-term tests and kelta-bench.
//!
//! ```text
//! tui-sim --ink [--fps 30] [--lines 30] [--duration-ms N]   Claude-like full redraw inside DEC 2026
//! tui-sim --alt [--duration-ms N]                           alt screen + mouse modes, lazygit-like
//! tui-sim --flood <MB>                                      write MB of text as fast as possible
//! tui-sim --queries [--timeout-ms 1000]                     send DA1/DSR/OSC 11 ? and print replies
//! ```
//! Interactive modes exit on `q` (or when `--duration-ms` elapses). Pacing uses `poll(2)` on stdin
//! with a deadline, never sleeping threads.

use std::io::{self, Read, Write};
use std::process::ExitCode;
use std::time::{Duration, Instant};

use rustix::event::{PollFd, PollFlags, poll};
use rustix::termios::{OptionalActions, Termios, isatty, tcgetattr, tcsetattr};

#[derive(Debug, Clone, PartialEq)]
enum Mode {
    Ink,
    Alt,
    Flood(u64),
    Queries,
}

#[derive(Debug, Clone)]
struct Opts {
    mode: Mode,
    fps: u32,
    lines: u16,
    duration: Option<Duration>,
    timeout: Duration,
}

const USAGE: &str = "usage: tui-sim (--ink [--fps N] [--lines N] | --alt | --flood <MB> | --queries [--timeout-ms N]) [--duration-ms N]";

fn parse(args: &[String]) -> Result<Opts, String> {
    let mut mode = None;
    let mut o =
        Opts { mode: Mode::Ink, fps: 30, lines: 30, duration: None, timeout: Duration::from_millis(1000) };
    let mut it = args.iter();
    let num = |v: Option<&String>, flag: &str| -> Result<u64, String> {
        v.ok_or_else(|| format!("{flag} needs a value"))?.parse::<u64>().map_err(|e| format!("{flag}: {e}"))
    };
    while let Some(a) = it.next() {
        match a.as_str() {
            "--ink" => mode = Some(Mode::Ink),
            "--alt" => mode = Some(Mode::Alt),
            "--queries" => mode = Some(Mode::Queries),
            "--flood" => mode = Some(Mode::Flood(num(it.next(), "--flood")?)),
            "--fps" => o.fps = num(it.next(), "--fps")?.clamp(1, 240) as u32,
            "--lines" => o.lines = num(it.next(), "--lines")?.clamp(1, 500) as u16,
            "--duration-ms" => o.duration = Some(Duration::from_millis(num(it.next(), "--duration-ms")?)),
            "--timeout-ms" => o.timeout = Duration::from_millis(num(it.next(), "--timeout-ms")?),
            "-h" | "--help" => return Err(USAGE.into()),
            other => return Err(format!("unknown argument `{other}`\n{USAGE}")),
        }
    }
    o.mode = mode.ok_or_else(|| USAGE.to_owned())?;
    Ok(o)
}

/// Raw mode for the lifetime of the guard (no-op when stdin is not a tty).
struct RawMode {
    saved: Option<Termios>,
}

impl RawMode {
    fn enter() -> Self {
        let stdin = io::stdin();
        if !isatty(&stdin) {
            return Self { saved: None };
        }
        let Ok(saved) = tcgetattr(&stdin) else { return Self { saved: None } };
        let mut raw = saved.clone();
        raw.make_raw();
        if tcsetattr(&stdin, OptionalActions::Now, &raw).is_err() {
            return Self { saved: None };
        }
        Self { saved: Some(saved) }
    }
}

impl Drop for RawMode {
    fn drop(&mut self) {
        if let Some(t) = &self.saved {
            let _ = tcsetattr(io::stdin(), OptionalActions::Now, t);
        }
    }
}

/// Wait up to `timeout` for stdin; returns the bytes read (empty on timeout, None on EOF/error).
fn read_stdin(timeout: Duration) -> Option<Vec<u8>> {
    let stdin = io::stdin();
    let ts = rustix::event::Timespec::try_from(timeout).unwrap_or_default();
    let mut fds = [PollFd::new(&stdin, PollFlags::IN)];
    match poll(&mut fds, Some(&ts)) {
        Ok(0) => Some(Vec::new()),
        Ok(_) => {
            let revents = fds[0].revents();
            if revents.intersects(PollFlags::IN) {
                let mut buf = [0u8; 4096];
                match stdin.lock().read(&mut buf) {
                    Ok(0) | Err(_) => None,
                    Ok(n) => Some(buf[..n].to_vec()),
                }
            } else if revents.intersects(PollFlags::HUP | PollFlags::ERR | PollFlags::NVAL) {
                None
            } else {
                Some(Vec::new())
            }
        }
        Err(rustix::io::Errno::INTR) => Some(Vec::new()),
        Err(_) => None,
    }
}

const SPINNER: &[&str] = &["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];

fn ink_frame(n: u64, lines: u16) -> String {
    let mut s = String::with_capacity(4096);
    s.push_str("\x1b[?2026h\x1b[H\x1b[2J");
    s.push_str("\x1b[38;5;208m╭──────────────────────────────────────────────╮\x1b[0m\r\n");
    s.push_str(&format!(
        "\x1b[38;5;208m│\x1b[0m {} \x1b[1mWorking…\x1b[0m \x1b[2m(frame {n}, esc to interrupt)\x1b[0m\r\n",
        SPINNER[(n as usize) % SPINNER.len()]
    ));
    for i in 0..lines {
        let w = ((n as usize + i as usize) % 60) + 10;
        s.push_str(&format!(
            "\x1b[38;2;{};{};200m{}\x1b[0m {}\r\n",
            (i * 7) % 255,
            (n * 3) % 255,
            "█".repeat(w / 10),
            "·".repeat(w)
        ));
    }
    s.push_str("\x1b[38;5;208m╰──────────────────────────────────────────────╯\x1b[0m\r\n> ");
    s.push_str("\x1b[?2026l");
    s
}

fn run_ink(o: &Opts) -> io::Result<ExitCode> {
    let _raw = RawMode::enter();
    let mut out = io::stdout().lock();
    out.write_all(b"\x1b[?25l")?;
    let start = Instant::now();
    let interval = Duration::from_micros(1_000_000 / u64::from(o.fps));
    let mut next = start;
    let mut n = 0u64;
    let code = loop {
        out.write_all(ink_frame(n, o.lines).as_bytes())?;
        out.flush()?;
        n += 1;
        next += interval;
        if o.duration.is_some_and(|d| start.elapsed() >= d) {
            break ExitCode::SUCCESS;
        }
        let wait = next.saturating_duration_since(Instant::now());
        match read_stdin(wait) {
            None => break ExitCode::SUCCESS,
            Some(b) if b.contains(&b'q') => break ExitCode::SUCCESS,
            Some(_) => {}
        }
    };
    out.write_all(b"\x1b[?25h\r\n")?;
    out.flush()?;
    Ok(code)
}

fn run_alt(o: &Opts) -> io::Result<ExitCode> {
    let _raw = RawMode::enter();
    let mut out = io::stdout().lock();
    // Alt screen, mouse 1000/1002 + SGR 1006, focus 1004, bracketed paste, app cursor keys, title.
    out.write_all(
        b"\x1b[?1049h\x1b[?1000h\x1b[?1002h\x1b[?1006h\x1b[?1004h\x1b[?2004h\x1b[?1h\x1b]2;tui-sim alt\x07",
    )?;
    out.write_all(b"\x1b[H\x1b[2J\x1b[1;7m Status \x1b[0m  \x1b[32m\xe2\x9c\x93 main\x1b[0m\r\n")?;
    out.write_all(b"\x1b(0lqqqqqqqqqqqqqqqqqqqk\r\nx Files           x\r\nmqqqqqqqqqqqqqqqqqqqj\x1b(B\r\n")?;
    for i in 0..10 {
        out.write_all(format!("  \x1b[33mM\x1b[0m src/file_{i}.rs\r\n").as_bytes())?;
    }
    out.write_all(b"\x1b[3;5r\x1b[4:3m\x1b[58;2;255;0;0mundercurl\x1b[0m\x1b[r\x1b[5 q")?;
    out.flush()?;
    let start = Instant::now();
    loop {
        let wait = match o.duration {
            Some(d) => d.saturating_sub(start.elapsed()),
            None => Duration::from_secs(3600),
        };
        if o.duration.is_some() && wait.is_zero() {
            break;
        }
        match read_stdin(wait) {
            None => break,
            Some(b) if b.contains(&b'q') => break,
            Some(b) if !b.is_empty() => {
                out.write_all(format!("\x1b[20;1Hinput: {}\x1b[K", escape(&b)).as_bytes())?;
                out.flush()?;
            }
            Some(_) => {}
        }
    }
    out.write_all(b"\x1b[?1l\x1b[?2004l\x1b[?1004l\x1b[?1006l\x1b[?1002l\x1b[?1000l\x1b[?1049l\x1b[0 q")?;
    out.flush()?;
    Ok(ExitCode::SUCCESS)
}

fn run_flood(mb: u64) -> io::Result<ExitCode> {
    let mut out = io::stdout().lock();
    let target = mb * 1024 * 1024;
    let mut written = 0u64;
    let mut line_no = 0u64;
    let mut buf = String::with_capacity(64 * 1024);
    while written < target {
        buf.clear();
        while buf.len() < 60 * 1024 {
            line_no += 1;
            buf.push_str(&format!(
                "{line_no:>9} \x1b[3{}mThe quick brown fox jumps over the lazy dog\x1b[0m 0123456789abcdef\r\n",
                line_no % 7 + 1
            ));
        }
        let take = ((target - written) as usize).min(buf.len());
        out.write_all(&buf.as_bytes()[..take])?;
        written += take as u64;
    }
    out.write_all(b"\x1b[0m\r\nflood done\r\n")?;
    out.flush()?;
    Ok(ExitCode::SUCCESS)
}

fn escape(b: &[u8]) -> String {
    let mut s = String::new();
    for &c in b {
        match c {
            0x1b => s.push_str("\\e"),
            0x07 => s.push_str("\\a"),
            b'\\' => s.push_str("\\\\"),
            0x20..=0x7e => s.push(c as char),
            _ => s.push_str(&format!("\\x{c:02x}")),
        }
    }
    s
}

/// Split a reply stream into CSI / OSC / other sequences.
fn split_replies(b: &[u8]) -> Vec<Vec<u8>> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        if b[i] == 0x1b && i + 1 < b.len() && b[i + 1] == b'[' {
            let mut j = i + 2;
            while j < b.len() && !(0x40..=0x7e).contains(&b[j]) {
                j += 1;
            }
            let end = (j + 1).min(b.len());
            out.push(b[i..end].to_vec());
            i = end;
        } else if b[i] == 0x1b && i + 1 < b.len() && b[i + 1] == b']' {
            let mut j = i + 2;
            let mut end = b.len();
            while j < b.len() {
                if b[j] == 0x07 {
                    end = j + 1;
                    break;
                }
                if b[j] == 0x1b && j + 1 < b.len() && b[j + 1] == b'\\' {
                    end = j + 2;
                    break;
                }
                j += 1;
            }
            out.push(b[i..end].to_vec());
            i = end;
        } else {
            i += 1;
        }
    }
    out
}

fn classify(seq: &[u8]) -> Option<&'static str> {
    if seq.starts_with(b"\x1b[") {
        match seq.last() {
            Some(b'c') => Some("DA1"),
            Some(b'R') => Some("DSR"),
            _ => None,
        }
    } else if seq.starts_with(b"\x1b]11;") {
        Some("OSC11")
    } else {
        None
    }
}

fn run_queries(o: &Opts) -> io::Result<ExitCode> {
    let _raw = RawMode::enter();
    let mut out = io::stdout().lock();
    out.write_all(b"\x1b[c\x1b[6n\x1b]11;?\x07")?;
    out.flush()?;
    let deadline = Instant::now() + o.timeout;
    let mut got = Vec::new();
    let wanted = ["DA1", "DSR", "OSC11"];
    loop {
        let found: Vec<&str> = split_replies(&got).iter().filter_map(|s| classify(s)).collect();
        if wanted.iter().all(|w| found.contains(w)) {
            break;
        }
        let wait = deadline.saturating_duration_since(Instant::now());
        if wait.is_zero() {
            break;
        }
        match read_stdin(wait) {
            None => break,
            Some(b) => got.extend_from_slice(&b),
        }
    }
    drop(_raw);
    let replies = split_replies(&got);
    let mut seen = Vec::new();
    for r in &replies {
        let name = classify(r).unwrap_or("OTHER");
        seen.push(name);
        out.write_all(format!("{name}: {}\r\n", escape(r)).as_bytes())?;
    }
    let mut ok = true;
    for w in wanted {
        let count = seen.iter().filter(|s| **s == w).count();
        if count == 0 {
            ok = false;
            out.write_all(format!("missing: {w}\r\n").as_bytes())?;
        } else if count > 1 {
            ok = false;
            out.write_all(format!("duplicate: {w} x{count}\r\n").as_bytes())?;
        }
    }
    out.flush()?;
    Ok(if ok { ExitCode::SUCCESS } else { ExitCode::FAILURE })
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let opts = match parse(&args) {
        Ok(o) => o,
        Err(msg) => {
            eprintln!("{msg}");
            return ExitCode::from(2);
        }
    };
    let r = match opts.mode {
        Mode::Ink => run_ink(&opts),
        Mode::Alt => run_alt(&opts),
        Mode::Flood(mb) => run_flood(mb),
        Mode::Queries => run_queries(&opts),
    };
    r.unwrap_or_else(|e| {
        eprintln!("tui-sim: {e}");
        ExitCode::FAILURE
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_modes() {
        let a = |v: &[&str]| parse(&v.iter().map(|s| (*s).to_owned()).collect::<Vec<_>>());
        assert_eq!(a(&["--flood", "5"]).unwrap().mode, Mode::Flood(5));
        assert_eq!(a(&["--ink", "--fps", "60"]).unwrap().fps, 60);
        assert!(a(&["--bogus"]).is_err());
        assert!(a(&[]).is_err());
    }

    #[test]
    fn splits_and_classifies_replies() {
        let r = split_replies(b"\x1b[?62;22c\x1b[12;1R\x1b]11;rgb:1e1e/1e1e/1e1e\x1b\\");
        let names: Vec<_> = r.iter().filter_map(|s| classify(s)).collect();
        assert_eq!(names, vec!["DA1", "DSR", "OSC11"]);
    }

    #[test]
    fn ink_frame_is_synchronized() {
        let f = ink_frame(3, 5);
        assert!(f.starts_with("\x1b[?2026h"));
        assert!(f.ends_with("\x1b[?2026l"));
    }
}

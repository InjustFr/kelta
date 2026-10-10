//! keltad: Kelta's session daemon (`keltad --socket <path> --history <dir>`; without `--history`
//! no on-disk scrollback log). Binds the socket, forks into the background (the launcher's wait
//! returns once the socket accepts), then serves until it has no client and no running session
//! for `IDLE_GRACE`.

use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Duration;

use kelta_proto::term::TerminalLimits;
use kelta_term::PtyTerminalHost;
use kelta_term::daemon;

const IDLE_GRACE: Duration = Duration::from_secs(30);

fn main() -> ExitCode {
    let mut args = std::env::args_os().skip(1);
    let (sock, history) = match (args.next(), args.next(), args.next(), args.next()) {
        (Some(flag), Some(path), None, None) if flag == "--socket" => (PathBuf::from(path), None),
        (Some(f), Some(path), Some(h), Some(dir)) if f == "--socket" && h == "--history" => {
            (PathBuf::from(path), Some(PathBuf::from(dir)))
        }
        _ => {
            eprintln!("usage: keltad --socket <path> [--history <dir>]");
            return ExitCode::from(2);
        }
    };
    let listener = match daemon::bind(&sock) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("keltad: {e}");
            return ExitCode::FAILURE;
        }
    };
    // SAFETY: no other thread exists yet; the child continues with plain Rust code.
    match unsafe { libc::fork() } {
        -1 => {
            eprintln!("keltad: fork: {}", std::io::Error::last_os_error());
            return ExitCode::FAILURE;
        }
        0 => {}
        _ => return ExitCode::SUCCESS,
    }
    // SAFETY: plain syscall; detaches from the app's session and process group.
    unsafe { libc::setsid() };
    let _ = std::env::set_current_dir("/");
    tracing_subscriber::fmt().with_ansi(false).with_writer(std::io::stderr).init();
    tracing::info!(socket = %sock.display(), pid = std::process::id(), "keltad started");
    // Spawn specs carry the complete environment.
    let limits = TerminalLimits::default();
    let host = match history {
        Some(dir) => PtyTerminalHost::with_history_dir(limits, kelta_term::backend::default_backend(), dir),
        None => PtyTerminalHost::new(limits),
    };
    daemon::serve(listener, host, IDLE_GRACE);
    ExitCode::SUCCESS
}

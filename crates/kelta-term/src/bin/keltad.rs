//! keltad: Kelta's session daemon (`keltad --socket <path>`). Binds the socket, forks into the
//! background (the launcher's wait returns once the socket accepts), then serves until it has no
//! client and no running session for `IDLE_GRACE`.

use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Duration;

use kelta_proto::term::{LoginEnv, TerminalLimits};
use kelta_term::PtyTerminalHost;
use kelta_term::daemon;

const IDLE_GRACE: Duration = Duration::from_secs(30);

fn main() -> ExitCode {
    let mut args = std::env::args_os().skip(1);
    let sock = match (args.next(), args.next()) {
        (Some(flag), Some(path)) if flag == "--socket" => PathBuf::from(path),
        _ => {
            eprintln!("usage: keltad --socket <path>");
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
    // Spawn specs carry the complete environment; the host's own login env is unused.
    daemon::serve(listener, PtyTerminalHost::new(LoginEnv::default(), TerminalLimits::default()), IDLE_GRACE);
    ExitCode::SUCCESS
}

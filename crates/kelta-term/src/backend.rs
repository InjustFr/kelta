//! PTY backends (ARCHITECTURE §7.2): open a PTY pair and exec the program directly (no shell
//! wrapper) with the complete environment, as a new session (`setsid`) whose controlling terminal
//! is the PTY. The child is its own process group leader, so `kill(-pid)` reaches the group.
//!
//! - [`PortablePty`]: `portable-pty` (default).
//! - [`RustixPty`]: `openpty` via rustix + `fork`/`execve` (feature `rustix-pty` makes it the
//!   default; always compiled and tested).

use std::collections::BTreeMap;
use std::ffi::CString;
use std::os::fd::{OwnedFd, RawFd};
use std::os::unix::ffi::OsStrExt;
use std::path::PathBuf;

use kelta_proto::error::KeltaError;

/// What a backend needs to start a program.
#[derive(Debug, Clone)]
pub struct SpawnRequest {
    /// Absolute path.
    pub program: PathBuf,
    /// Arguments (without argv\[0\]).
    pub args: Vec<String>,
    pub cwd: PathBuf,
    /// Complete environment (nothing is inherited).
    pub env: BTreeMap<String, String>,
    pub cols: u16,
    pub rows: u16,
}

/// A started child: the PTY master and the child pid (session + process group leader).
#[derive(Debug)]
pub struct PtyChild {
    pub master: OwnedFd,
    pub pid: i32,
}

/// Opens a PTY and starts a program in it.
pub trait PtyBackend: Send + Sync {
    fn name(&self) -> &'static str;
    fn spawn(&self, req: &SpawnRequest) -> Result<PtyChild, KeltaError>;
}

/// The backend selected at build time.
pub fn default_backend() -> std::sync::Arc<dyn PtyBackend> {
    if cfg!(feature = "rustix-pty") {
        std::sync::Arc::new(RustixPty)
    } else {
        std::sync::Arc::new(PortablePty)
    }
}

fn check_program(req: &SpawnRequest) -> Result<(), KeltaError> {
    if !req.program.is_absolute() {
        return Err(KeltaError::invalid(format!("program must be an absolute path: {}", req.program.display())));
    }
    match std::fs::metadata(&req.program) {
        Ok(m) if m.is_file() => Ok(()),
        Ok(_) => Err(KeltaError::invalid(format!("not a file: {}", req.program.display()))),
        Err(e) => Err(KeltaError::not_found(format!("{}: {e}", req.program.display()))),
    }
}

/// Duplicate a raw fd as an owned, close-on-exec descriptor.
fn dup_cloexec(fd: RawFd) -> Result<OwnedFd, KeltaError> {
    // SAFETY: `fd` is a valid open descriptor owned by the caller for the duration of the call.
    let borrowed = unsafe { std::os::fd::BorrowedFd::borrow_raw(fd) };
    rustix::io::fcntl_dupfd_cloexec(borrowed, 3).map_err(|e| KeltaError::internal(format!("dup pty master: {e}")))
}

/// Master side: non-blocking (reads drained until EAGAIN, writes queued on EAGAIN).
pub(crate) fn set_nonblocking(fd: &OwnedFd) -> Result<(), KeltaError> {
    let flags = rustix::fs::fcntl_getfl(fd).map_err(|e| KeltaError::internal(format!("fcntl: {e}")))?;
    rustix::fs::fcntl_setfl(fd, flags | rustix::fs::OFlags::NONBLOCK)
        .map_err(|e| KeltaError::internal(format!("fcntl: {e}")))
}

/// portable-pty backend.
#[derive(Debug, Default, Clone, Copy)]
pub struct PortablePty;

impl PtyBackend for PortablePty {
    fn name(&self) -> &'static str {
        "portable-pty"
    }

    fn spawn(&self, req: &SpawnRequest) -> Result<PtyChild, KeltaError> {
        use portable_pty::{CommandBuilder, PtySize};

        check_program(req)?;
        let pair = portable_pty::native_pty_system()
            .openpty(PtySize { rows: req.rows.max(1), cols: req.cols.max(1), pixel_width: 0, pixel_height: 0 })
            .map_err(|e| KeltaError::internal(format!("openpty: {e}")))?;
        let mut cmd = CommandBuilder::new(req.program.as_os_str());
        cmd.args(&req.args);
        cmd.env_clear();
        for (k, v) in &req.env {
            cmd.env(k, v);
        }
        cmd.cwd(req.cwd.as_os_str());
        let child = pair.slave.spawn_command(cmd).map_err(|e| KeltaError::internal(format!("spawn: {e}")))?;
        // The slave must be closed in this process, or reads never see EOF/EIO after exit.
        drop(pair.slave);
        let pid = child.process_id().and_then(|p| i32::try_from(p).ok()).ok_or_else(|| KeltaError::internal("no pid"))?;
        let raw = pair.master.as_raw_fd().ok_or_else(|| KeltaError::internal("pty master has no fd"))?;
        let master = dup_cloexec(raw)?;
        drop(pair.master);
        // The `std::process::Child` handle does not reap on drop; the reader thread waits on `pid`.
        drop(child);
        Ok(PtyChild { master, pid })
    }
}

/// rustix `openpty` + `fork`/`execve` backend.
#[derive(Debug, Default, Clone, Copy)]
pub struct RustixPty;

fn cstring(s: &[u8]) -> Result<CString, KeltaError> {
    CString::new(s).map_err(|_| KeltaError::invalid("NUL byte in program, argument or environment"))
}

impl PtyBackend for RustixPty {
    fn name(&self) -> &'static str {
        "rustix"
    }

    fn spawn(&self, req: &SpawnRequest) -> Result<PtyChild, KeltaError> {
        use rustix::pty::{OpenptFlags, grantpt, openpt, ptsname, unlockpt};

        check_program(req)?;
        let master = openpt(OpenptFlags::RDWR | OpenptFlags::NOCTTY)
            .map_err(|e| KeltaError::internal(format!("openpt: {e}")))?;
        rustix::io::fcntl_setfd(&master, rustix::io::FdFlags::CLOEXEC)
            .map_err(|e| KeltaError::internal(format!("fcntl: {e}")))?;
        grantpt(&master).map_err(|e| KeltaError::internal(format!("grantpt: {e}")))?;
        unlockpt(&master).map_err(|e| KeltaError::internal(format!("unlockpt: {e}")))?;
        let slave_name = ptsname(&master, Vec::new()).map_err(|e| KeltaError::internal(format!("ptsname: {e}")))?;
        let ws = rustix::termios::Winsize { ws_row: req.rows.max(1), ws_col: req.cols.max(1), ws_xpixel: 0, ws_ypixel: 0 };
        let _ = rustix::termios::tcsetwinsize(&master, ws);

        // Everything the child needs is prepared before fork: after fork only async-signal-safe
        // calls are made.
        let program = cstring(req.program.as_os_str().as_bytes())?;
        let mut argv_owned = vec![program.clone()];
        for a in &req.args {
            argv_owned.push(cstring(a.as_bytes())?);
        }
        let mut envp_owned = Vec::with_capacity(req.env.len());
        for (k, v) in &req.env {
            envp_owned.push(cstring(format!("{k}={v}").as_bytes())?);
        }
        let cwd = cstring(req.cwd.as_os_str().as_bytes())?;
        let mut argv: Vec<*const libc::c_char> = argv_owned.iter().map(|c| c.as_ptr()).collect();
        argv.push(std::ptr::null());
        let mut envp: Vec<*const libc::c_char> = envp_owned.iter().map(|c| c.as_ptr()).collect();
        envp.push(std::ptr::null());
        let slave_path = slave_name.as_ptr();
        // SAFETY: sysconf is always safe to call.
        let max_fd = match unsafe { libc::sysconf(libc::_SC_OPEN_MAX) } {
            n if n > 0 => n.min(65_536) as i32,
            _ => 1024,
        };

        // SAFETY: fork in a multithreaded process; the child only calls async-signal-safe
        // functions (setsid, open, ioctl, dup2, close, chdir, signal, sigprocmask, execve, _exit)
        // on data prepared above.
        let pid = unsafe { libc::fork() };
        if pid < 0 {
            return Err(KeltaError::internal(format!("fork: {}", std::io::Error::last_os_error())));
        }
        if pid == 0 {
            // SAFETY: child side of fork, see above.
            unsafe {
                child_exec(slave_path, &argv, &envp, cwd.as_ptr(), max_fd);
            }
        }
        Ok(PtyChild { master, pid })
    }
}

/// Child side of [`RustixPty::spawn`]. Never returns.
///
/// # Safety
/// Must only be called in the child right after `fork`; all pointers must be valid C strings /
/// NULL-terminated arrays.
unsafe fn child_exec(
    slave_path: *const libc::c_char,
    argv: &[*const libc::c_char],
    envp: &[*const libc::c_char],
    cwd: *const libc::c_char,
    max_fd: i32,
) -> ! {
    // SAFETY: async-signal-safe libc calls in the forked child (see the caller).
    unsafe {
        for sig in [libc::SIGCHLD, libc::SIGHUP, libc::SIGINT, libc::SIGQUIT, libc::SIGTERM, libc::SIGALRM, libc::SIGPIPE]
        {
            libc::signal(sig, libc::SIG_DFL);
        }
        let mut empty: libc::sigset_t = std::mem::zeroed();
        libc::sigemptyset(&mut empty);
        libc::sigprocmask(libc::SIG_SETMASK, &empty, std::ptr::null_mut());
        if libc::setsid() < 0 {
            libc::_exit(126);
        }
        let slave = libc::open(slave_path, libc::O_RDWR);
        if slave < 0 {
            libc::_exit(126);
        }
        // Opening the slave after setsid makes it the controlling terminal on Linux; TIOCSCTTY
        // does it explicitly (required on macOS).
        libc::ioctl(slave, libc::TIOCSCTTY as _, 0);
        libc::dup2(slave, 0);
        libc::dup2(slave, 1);
        libc::dup2(slave, 2);
        for fd in 3..max_fd {
            libc::close(fd);
        }
        if libc::chdir(cwd) != 0 {
            let _ = libc::chdir(c"/".as_ptr());
        }
        libc::execve(argv[0], argv.as_ptr(), envp.as_ptr());
        libc::_exit(127);
    }
}

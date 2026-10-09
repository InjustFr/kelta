//! Control socket (ARCHITECTURE §2, §11.1): `<runtime>/ctl.sock`, mode 0600 in a 0700 directory
//! owned by us, peer uid must equal ours, line-delimited `CtlRequest` → `CtlResponse`.

use std::io;
use std::os::unix::fs::{FileTypeExt, MetadataExt, PermissionsExt};
use std::path::Path;
use std::sync::{Arc, Weak};
use std::time::Duration;

use kelta_proto::api::CoreApi;
use kelta_proto::ctl::{CTL_MAX_LINE, CTL_PROTOCOL_VERSION, CtlCommand, CtlRequest, CtlResponse};
use kelta_proto::error::KeltaError;
use kelta_proto::events::bus;
use tokio::io::{AsyncBufRead, AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{UnixListener, UnixStream};

use crate::auth::{TokenKind, TokenTable};

/// An idle connection is closed after this long without a complete request line.
const IDLE_DEADLINE: Duration = Duration::from_secs(30);

/// State shared by every ctl connection.
pub(crate) struct CtlCtx {
    pub(crate) core: Weak<dyn CoreApi>,
    pub(crate) tokens: Arc<TokenTable>,
}

/// Our real uid (rustix).
pub(crate) fn my_uid() -> u32 {
    rustix::process::getuid().as_raw()
}

/// Create (or fix) the runtime dir: a real directory owned by us with mode 0700.
pub(crate) fn prepare_dir(dir: &Path) -> Result<(), KeltaError> {
    std::fs::create_dir_all(dir)?;
    let meta = std::fs::symlink_metadata(dir)?;
    if !meta.file_type().is_dir() {
        return Err(KeltaError::permission_denied(format!("runtime dir {} is not a directory", dir.display())));
    }
    if meta.uid() != my_uid() {
        return Err(KeltaError::permission_denied(format!(
            "runtime dir {} is owned by uid {}, not us",
            dir.display(),
            meta.uid()
        )));
    }
    if meta.mode() & 0o777 != 0o700 {
        std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))?;
    }
    Ok(())
}

/// Remove a stale socket file; refuse when another live instance answers on it.
fn clear_stale(path: &Path) -> Result<(), KeltaError> {
    let Ok(meta) = std::fs::symlink_metadata(path) else { return Ok(()) };
    if !meta.file_type().is_socket() {
        return Err(KeltaError::conflict(format!("{} exists and is not a socket", path.display())));
    }
    if std::os::unix::net::UnixStream::connect(path).is_ok() {
        return Err(KeltaError::conflict(format!("another instance is listening on {}", path.display())));
    }
    std::fs::remove_file(path)?;
    Ok(())
}

/// Bind the socket (0600) and return the listener.
pub(crate) fn bind(path: &Path) -> Result<UnixListener, KeltaError> {
    if let Some(dir) = path.parent() {
        prepare_dir(dir)?;
    }
    if path.as_os_str().len() >= 100 {
        return Err(KeltaError::invalid(format!("socket path too long: {}", path.display())));
    }
    clear_stale(path)?;
    let listener = UnixListener::bind(path)?;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
    Ok(listener)
}

/// Accept loop (a tokio task, ended by aborting its handle).
pub(crate) async fn accept_loop(listener: UnixListener, ctx: Arc<CtlCtx>) {
    loop {
        match listener.accept().await {
            Ok((stream, _)) => {
                tokio::spawn(serve_conn(stream, ctx.clone()));
            }
            Err(e) => {
                tracing::warn!(error = %e, "ctl accept failed");
                // one-shot: back-off after an accept error (EMFILE) so the loop cannot spin.
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
        }
    }
}

enum LineRead {
    Line,
    Eof,
    TooLong,
}

/// Read one `\n`-terminated line into `buf` (without the newline), refusing more than `max` bytes.
async fn read_line<R: AsyncBufRead + Unpin>(r: &mut R, buf: &mut Vec<u8>, max: usize) -> io::Result<LineRead> {
    loop {
        let chunk = r.fill_buf().await?;
        if chunk.is_empty() {
            return Ok(if buf.is_empty() { LineRead::Eof } else { LineRead::Line });
        }
        let (take, done) = match chunk.iter().position(|&b| b == b'\n') {
            Some(i) => (i, true),
            None => (chunk.len(), false),
        };
        if buf.len() + take > max {
            return Ok(LineRead::TooLong);
        }
        buf.extend_from_slice(&chunk[..take]);
        r.consume(if done { take + 1 } else { take });
        if done {
            return Ok(LineRead::Line);
        }
    }
}

async fn write_response(w: &mut (impl AsyncWriteExt + Unpin), resp: &CtlResponse) -> io::Result<()> {
    let mut line = serde_json::to_vec(resp).map_err(io::Error::other)?;
    line.push(b'\n');
    w.write_all(&line).await?;
    w.flush().await
}

async fn serve_conn(stream: UnixStream, ctx: Arc<CtlCtx>) {
    match stream.peer_cred() {
        Ok(cred) if cred.uid() == my_uid() => {}
        Ok(cred) => {
            tracing::warn!(peer_uid = cred.uid(), "ctl: rejected connection from another uid");
            return;
        }
        Err(e) => {
            tracing::warn!(error = %e, "ctl: cannot read peer credentials; rejected");
            return;
        }
    }
    let (rd, mut wr) = stream.into_split();
    let mut rd = BufReader::new(rd);
    let mut buf = Vec::new();
    loop {
        buf.clear();
        // one-shot: per-request read deadline, armed when waiting for a line.
        let read = tokio::time::timeout(IDLE_DEADLINE, read_line(&mut rd, &mut buf, CTL_MAX_LINE)).await;
        let resp = match read {
            Ok(Ok(LineRead::Line)) => {
                if buf.iter().all(u8::is_ascii_whitespace) {
                    continue;
                }
                handle_line(&ctx, &buf).await
            }
            Ok(Ok(LineRead::TooLong)) => {
                let resp = CtlResponse::err(KeltaError::invalid(format!(
                    "frame too large (max {CTL_MAX_LINE} bytes)"
                )));
                let _ = write_response(&mut wr, &resp).await;
                return;
            }
            Ok(Ok(LineRead::Eof)) | Ok(Err(_)) | Err(_) => return,
        };
        if write_response(&mut wr, &resp).await.is_err() {
            return;
        }
    }
}

/// Parse and dispatch one request line.
pub(crate) async fn handle_line(ctx: &CtlCtx, line: &[u8]) -> CtlResponse {
    let req: CtlRequest = match serde_json::from_slice(line) {
        Ok(r) => r,
        Err(e) => return CtlResponse::err(KeltaError::invalid(format!("bad ctl request: {e}"))),
    };
    if req.v != CTL_PROTOCOL_VERSION {
        return CtlResponse::err(KeltaError::invalid(format!(
            "unsupported ctl protocol version {} (expected {CTL_PROTOCOL_VERSION})",
            req.v
        )));
    }
    match dispatch(ctx, req.command).await {
        Ok(v) => CtlResponse::ok(v),
        Err(e) => CtlResponse::err(e),
    }
}

async fn dispatch(ctx: &CtlCtx, cmd: CtlCommand) -> Result<serde_json::Value, KeltaError> {
    // Version answers even while core is shutting down.
    if cmd == CtlCommand::Version {
        return Ok(serde_json::json!({
            "version": env!("CARGO_PKG_VERSION"),
            "protocol": CTL_PROTOCOL_VERSION,
        }));
    }
    let core = ctx.core.upgrade().ok_or_else(|| KeltaError::unsupported("kelta is shutting down"))?;
    match cmd {
        CtlCommand::Hook { session, token, payload } => {
            if !ctx.tokens.check(&session, TokenKind::Hook, &token) {
                tracing::warn!(session = %session, "ctl: hook with an invalid token rejected");
                return Err(KeltaError::permission_denied("invalid hook token"));
            }
            crate::hooks::ingest(&core, &session, *payload).await?;
            Ok(serde_json::Value::Null)
        }
        CtlCommand::Emit { name, payload } => {
            if !name.starts_with(bus::CUSTOM_PREFIX) || name.len() == bus::CUSTOM_PREFIX.len() {
                return Err(KeltaError::invalid(format!("only custom.* events can be emitted, got {name:?}")));
            }
            core.ctl(CtlCommand::Emit { name, payload }).await
        }
        other => core.ctl(other).await,
    }
}


#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn read_line_caps() {
        let data: &[u8] = b"abc\ndefgh\nlast";
        let mut r = BufReader::with_capacity(2, data);
        let mut buf = Vec::new();
        assert!(matches!(read_line(&mut r, &mut buf, 4).await.unwrap(), LineRead::Line));
        assert_eq!(buf, b"abc");
        buf.clear();
        assert!(matches!(read_line(&mut r, &mut buf, 4).await.unwrap(), LineRead::TooLong));
    }

    #[tokio::test]
    async fn read_line_eof() {
        let data: &[u8] = b"tail";
        let mut r = BufReader::new(data);
        let mut buf = Vec::new();
        assert!(matches!(read_line(&mut r, &mut buf, 10).await.unwrap(), LineRead::Line));
        buf.clear();
        assert!(matches!(read_line(&mut r, &mut buf, 10).await.unwrap(), LineRead::Eof));
    }
}

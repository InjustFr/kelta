//! Bounded loopback serving for the http server and the IDE bridge (#293). `axum::serve` gives hyper
//! no timer, so its header read timeout is off: any local process could hold idle sockets forever
//! and run Kelta out of file descriptors.

use std::future::Future;
use std::sync::Arc;
use std::time::Duration;

use axum::Router;
use hyper::server::conn::http1;
use hyper_util::rt::{TokioIo, TokioTimer};
use hyper_util::service::TowerToHyperService;
use tokio::net::TcpListener;
use tokio::sync::{Semaphore, watch};

/// Connections served at once per listener; the next ones wait in the accept backlog.
pub(crate) const MAX_CONNS: usize = 64;
/// Time a connection gets to send a request head (the http server also bounds the body with it).
pub(crate) const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);

/// Serve `app` (HTTP/1.1, upgrades allowed) until `shutdown` resolves: at most `max` connections
/// at once, each closed when it does not send a full request head within `timeout`, idle
/// keep-alive included. Shutdown closes the listener at once; open connections finish their
/// current request. An upgraded connection (IDE bridge WebSocket, past the token check) frees
/// its slot.
pub(crate) async fn serve(
    listener: TcpListener,
    app: Router,
    max: usize,
    timeout: Duration,
    shutdown: impl Future<Output = ()>,
) {
    let permits = Arc::new(Semaphore::new(max));
    // Dropped on shutdown: every connection then shuts down gracefully.
    let (_stop, stop_rx) = watch::channel(());
    tokio::pin!(shutdown);
    loop {
        let accepted = async {
            let permit = permits.clone().acquire_owned().await;
            (permit, listener.accept().await)
        };
        let (permit, stream) = tokio::select! {
            () = &mut shutdown => break,
            (permit, res) = accepted => match (permit, res) {
                (Ok(p), Ok((s, _))) => (p, s),
                (_, Err(e)) => {
                    // EMFILE and friends: back off instead of spinning.
                    tracing::warn!(error = %e, "loopback accept failed");
                    tokio::time::sleep(Duration::from_millis(50)).await;
                    continue;
                }
                (Err(_), _) => break, // the semaphore is never closed
            },
        };
        let svc = TowerToHyperService::new(app.clone());
        let mut stop = stop_rx.clone();
        tokio::spawn(async move {
            let _permit = permit;
            let mut builder = http1::Builder::new();
            builder.timer(TokioTimer::new()).header_read_timeout(timeout);
            let conn = builder.serve_connection(TokioIo::new(stream), svc).with_upgrades();
            tokio::pin!(conn);
            let res = tokio::select! {
                r = conn.as_mut() => r,
                _ = stop.changed() => {
                    conn.as_mut().graceful_shutdown();
                    conn.await
                }
            };
            if let Err(e) = res {
                tracing::debug!(error = %e, "loopback connection closed");
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use std::time::Instant;

    use axum::routing::post;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpStream;
    use tokio::sync::oneshot;

    use super::*;

    const REQ: &[u8] = b"POST /mcp HTTP/1.1\r\nHost: x\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{}";

    async fn start(max: usize, timeout: Duration) -> (u16, oneshot::Sender<()>) {
        let listener = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let app = Router::new().route("/mcp", post(|| async { "ok" }));
        let (tx, rx) = oneshot::channel::<()>();
        tokio::spawn(serve(listener, app, max, timeout, async move {
            let _ = rx.await;
        }));
        (port, tx)
    }

    async fn connect(port: u16) -> TcpStream {
        TcpStream::connect(("127.0.0.1", port)).await.unwrap()
    }

    /// Read until EOF; panics past `limit`.
    async fn read_all(s: &mut TcpStream, limit: Duration) -> String {
        let mut out = Vec::new();
        let _ = tokio::time::timeout(limit, s.read_to_end(&mut out)).await.expect("connection still open");
        String::from_utf8_lossy(&out).into_owned()
    }

    #[tokio::test]
    async fn the_65th_connection_waits_until_idle_ones_time_out() {
        let timeout = Duration::from_millis(500);
        let (port, _tx) = start(MAX_CONNS, timeout).await;
        let mut idle = Vec::new();
        for _ in 0..MAX_CONNS {
            idle.push(connect(port).await);
        }
        let t0 = Instant::now();
        let mut s = connect(port).await;
        s.write_all(REQ).await.unwrap();
        let reply = read_all(&mut s, Duration::from_secs(10)).await;
        assert!(reply.starts_with("HTTP/1.1 200"), "{reply}");
        assert!(t0.elapsed() >= timeout / 2, "served before an idle slot freed: {:?}", t0.elapsed());
        for mut c in idle {
            assert_eq!(read_all(&mut c, Duration::from_secs(5)).await, "", "idle connection got a reply");
        }
    }

    #[tokio::test]
    async fn half_a_request_is_closed_after_the_timeout_and_shutdown_closes_the_listener() {
        let (port, tx) = start(MAX_CONNS, Duration::from_millis(300)).await;
        let mut half = connect(port).await;
        half.write_all(&REQ[..20]).await.unwrap();
        tx.send(()).unwrap();
        let mut refused = false;
        for _ in 0..100 {
            if TcpStream::connect(("127.0.0.1", port)).await.is_err() {
                refused = true;
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        assert!(refused, "listener still open after shutdown");
        read_all(&mut half, Duration::from_secs(5)).await;
    }
}

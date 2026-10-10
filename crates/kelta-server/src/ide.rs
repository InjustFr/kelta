//! Claude Code IDE bridge (`claude.ide_bridge`, ARCHITECTURE §8.5, §11.1): one loopback WebSocket
//! MCP server per Claude session, advertised to that session by `<claude dir>/ide/<port>.lock` and
//! `CLAUDE_CODE_SSE_PORT`, the protocol of the VS Code / JetBrains / claudecode.nvim integrations.
//!
//! Tools route to the session's editor pane: `openFile` → `editor_open`, `openDiff` →
//! `editor_diff` (nvim RPC); selection, open editors and diagnostics are answered empty.

use std::collections::HashMap;
use std::io::Write;
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Weak};

use axum::Router;
use axum::extract::State;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};
use futures::{SinkExt, StreamExt};
use kelta_proto::api::CoreApi;
use kelta_proto::error::KeltaError;
use kelta_proto::ids::SessionId;
use parking_lot::Mutex;
use serde_json::{Value, json};
use tokio::sync::{mpsc, oneshot};
use tokio::task::AbortHandle;

use crate::auth::ct_eq;
use crate::mcp::{self, PROTOCOL_VERSIONS};

/// `ideName` of Kelta's lock files; startup cleanup only ever removes locks with this name.
pub(crate) const IDE_NAME: &str = "Kelta";
const AUTH_HEADER: &str = "x-claude-code-ide-authorization";

/// A running bridge; dropping it stops the listener and removes the lock file.
pub(crate) struct Bridge {
    lock: PathBuf,
    task: AbortHandle,
}

impl Drop for Bridge {
    fn drop(&mut self) {
        self.task.abort();
        let _ = std::fs::remove_file(&self.lock);
    }
}

struct Cx {
    core: Weak<dyn CoreApi>,
    sid: SessionId,
    token: String,
    folders: Vec<PathBuf>,
    /// `<runtime>/ide`: proposed contents shown by `openDiff`.
    scratch: PathBuf,
}

/// Bind `127.0.0.1:<random>`, write `<claude_dir>/ide/<port>.lock` (0600) and serve.
pub(crate) fn open(
    core: Weak<dyn CoreApi>,
    sid: SessionId,
    claude_dir: &Path,
    folders: Vec<PathBuf>,
    runtime: &Path,
) -> Result<(Bridge, u16), KeltaError> {
    let dir = claude_dir.join("ide");
    std::fs::DirBuilder::new().recursive(true).mode(0o700).create(&dir)?;
    remove_stale(&dir);
    let std_listener = std::net::TcpListener::bind(("127.0.0.1", 0))?;
    std_listener.set_nonblocking(true)?;
    let port = std_listener.local_addr()?.port();
    let listener = tokio::net::TcpListener::from_std(std_listener)?;
    let token = uuid::Uuid::new_v4().simple().to_string();
    let lock = dir.join(format!("{port}.lock"));
    let body = json!({
        "pid": std::process::id(),
        "workspaceFolders": folders,
        "ideName": IDE_NAME,
        "transport": "ws",
        "authToken": token,
    });
    write_private(&lock, &serde_json::to_vec(&body).unwrap_or_default())?;
    let cx = Arc::new(Cx { core, sid, token, folders, scratch: runtime.join("ide") });
    let app = Router::new().fallback(upgrade).with_state(cx);
    let task = tokio::spawn(async move {
        if let Err(e) = axum::serve(listener, app).await {
            tracing::warn!(error = %e, "ide bridge stopped with an error");
        }
    })
    .abort_handle();
    tracing::debug!(port, "ide bridge listening");
    Ok((Bridge { lock, task }, port))
}

/// Write via a fresh 0600 temp file (never follows an existing file or symlink), then rename.
fn write_private(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let tmp = path.with_extension("lock.tmp");
    let _ = std::fs::remove_file(&tmp);
    let mut f = std::fs::OpenOptions::new().write(true).create_new(true).mode(0o600).open(&tmp)?;
    f.write_all(bytes)?;
    std::fs::rename(&tmp, path)
}

/// Remove Kelta lock files whose process is gone (a crash or a kill skipped `Drop`). Lock files of
/// other IDEs, of live Kelta processes and anything unparsable are left alone.
pub(crate) fn remove_stale(dir: &Path) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for e in entries.flatten() {
        let path = e.path();
        if path.extension().is_none_or(|x| x != "lock") || e.metadata().is_ok_and(|m| m.len() > 64 * 1024) {
            continue;
        }
        let Some(v) = std::fs::read(&path).ok().and_then(|b| serde_json::from_slice::<Value>(&b).ok()) else {
            continue;
        };
        let pid =
            v["pid"].as_i64().and_then(|p| i32::try_from(p).ok()).and_then(rustix::process::Pid::from_raw);
        let dead = pid.is_some_and(|p| rustix::process::test_kill_process(p) == Err(rustix::io::Errno::SRCH));
        if v["ideName"] == IDE_NAME && dead {
            tracing::debug!(path = %path.display(), "removing a stale Kelta ide lock");
            let _ = std::fs::remove_file(&path);
        }
    }
}

async fn upgrade(State(cx): State<Arc<Cx>>, headers: HeaderMap, ws: WebSocketUpgrade) -> Response {
    // Browsers always send Origin on WebSocket handshakes; Claude Code does not.
    if headers.contains_key(header::ORIGIN) {
        tracing::warn!("ide bridge: browser handshake rejected");
        return (StatusCode::FORBIDDEN, "forbidden").into_response();
    }
    let presented = headers.get(AUTH_HEADER).map(|v| v.as_bytes()).unwrap_or_default();
    if !ct_eq(presented, cx.token.as_bytes()) {
        tracing::warn!("ide bridge: handshake with an invalid token rejected");
        return (StatusCode::UNAUTHORIZED, "unauthorized").into_response();
    }
    ws.on_upgrade(move |s| serve(cx, s))
}

/// Pending `openDiff` calls of one connection by `tab_name`; dropping a sender resolves it.
type Diffs = Arc<Mutex<HashMap<String, oneshot::Sender<()>>>>;

async fn serve(cx: Arc<Cx>, socket: WebSocket) {
    let (mut sink, mut stream) = socket.split();
    let (tx, mut rx) = mpsc::unbounded_channel::<String>();
    tokio::spawn(async move {
        while let Some(text) = rx.recv().await {
            if sink.send(Message::Text(text.into())).await.is_err() {
                break;
            }
        }
    });
    let diffs = Diffs::default();
    while let Some(Ok(msg)) = stream.next().await {
        let text = match msg {
            Message::Text(t) => t,
            Message::Close(_) => break,
            _ => continue,
        };
        // One task per request: a pending `openDiff` must not block `close_tab`.
        let (cx, tx, diffs) = (cx.clone(), tx.clone(), diffs.clone());
        tokio::spawn(async move {
            if let Some(reply) = handle(&cx, &diffs, text.as_str()).await {
                let _ = tx.send(reply.to_string());
            }
        });
    }
    // Disconnect: pending diffs resolve as rejected.
    diffs.lock().clear();
}

async fn handle(cx: &Cx, diffs: &Diffs, text: &str) -> Option<Value> {
    let Ok(msg) = serde_json::from_str::<Value>(text) else {
        return Some(mcp::error(Value::Null, mcp::PARSE_ERROR, "parse error"));
    };
    // Notifications (`notifications/initialized`, `ide_connected`, …) and responses: no answer.
    let id = msg.get("id").cloned()?;
    let method = msg.get("method").and_then(Value::as_str)?;
    let params = msg.get("params").cloned().unwrap_or(Value::Null);
    Some(match method {
        "initialize" => {
            let requested = params.get("protocolVersion").and_then(Value::as_str);
            let version = requested.filter(|v| PROTOCOL_VERSIONS.contains(v)).unwrap_or(PROTOCOL_VERSIONS[0]);
            mcp::result(
                id,
                json!({
                    "protocolVersion": version,
                    "capabilities": { "tools": { "listChanged": false } },
                    "serverInfo": { "name": "kelta-ide", "title": IDE_NAME, "version": env!("CARGO_PKG_VERSION") },
                }),
            )
        }
        "ping" => mcp::result(id, json!({})),
        "tools/list" => mcp::result(id, json!({ "tools": tool_defs() })),
        "tools/call" => {
            let name = params.get("name").and_then(Value::as_str).unwrap_or_default();
            let args = params.get("arguments").cloned().unwrap_or_else(|| json!({}));
            let Some(core) = cx.core.upgrade() else {
                return Some(mcp::error(id, mcp::INTERNAL_ERROR, "kelta is shutting down"));
            };
            match call(cx, &core, diffs, name, &args).await {
                Some(Ok(texts)) => mcp::result(id, json!({ "content": texts_content(&texts) })),
                Some(Err(e)) => mcp::result(id, json!({ "content": texts_content(&[e]), "isError": true })),
                None => mcp::error(id, mcp::INVALID_PARAMS, &format!("unknown tool: {name}")),
            }
        }
        "resources/list" => mcp::result(id, json!({ "resources": [] })),
        "prompts/list" => mcp::result(id, json!({ "prompts": [] })),
        other => mcp::error(id, mcp::METHOD_NOT_FOUND, &format!("method not found: {other}")),
    })
}

fn texts_content(texts: &[String]) -> Value {
    texts.iter().map(|t| json!({ "type": "text", "text": t })).collect()
}

fn tool_defs() -> Value {
    let obj = |props: Value, required: &[&str]| json!({ "type": "object", "properties": props, "required": required });
    let none = obj(json!({}), &[]);
    let s = json!({ "type": "string" });
    json!([
        { "name": "openFile", "description": "Open a file in the Kelta editor pane of this session.",
          "inputSchema": obj(json!({ "filePath": s, "preview": { "type": "boolean" }, "startText": s, "endText": s,
                                     "selectToEndOfLine": { "type": "boolean" }, "makeFrontmost": { "type": "boolean" } }),
                             &["filePath"]) },
        { "name": "openDiff", "description": "Show proposed changes next to the file in the Kelta editor pane (nvim) until the edit is accepted or rejected in Claude.",
          "inputSchema": obj(json!({ "old_file_path": s, "new_file_path": s, "new_file_contents": s, "tab_name": s }),
                             &["old_file_path", "new_file_path", "new_file_contents", "tab_name"]) },
        { "name": "close_tab", "description": "Close a diff opened by openDiff.", "inputSchema": obj(json!({ "tab_name": s }), &["tab_name"]) },
        { "name": "closeAllDiffTabs", "description": "Close every diff opened by openDiff.", "inputSchema": none },
        { "name": "getWorkspaceFolders", "description": "Folders of this Kelta session.", "inputSchema": none },
        { "name": "getCurrentSelection", "description": "Not tracked by Kelta: always reports no selection.", "inputSchema": none },
        { "name": "getLatestSelection", "description": "Not tracked by Kelta: always reports no selection.", "inputSchema": none },
        { "name": "getOpenEditors", "description": "Not tracked by Kelta: always an empty list.", "inputSchema": none },
        { "name": "getDiagnostics", "description": "Kelta has no language server: always an empty list.",
          "inputSchema": obj(json!({ "uri": s }), &[]) },
    ])
}

type ToolOut = Result<Vec<String>, String>;

/// `None` for an unknown tool.
async fn call(cx: &Cx, core: &Arc<dyn CoreApi>, diffs: &Diffs, name: &str, args: &Value) -> Option<ToolOut> {
    let arg = |k: &str| {
        args.get(k)
            .and_then(Value::as_str)
            .filter(|v| !v.is_empty())
            .ok_or_else(|| format!("missing argument `{k}`"))
    };
    let no_selection = || {
        Ok(vec![json!({ "success": false, "message": "Kelta does not track editor selections" }).to_string()])
    };
    Some(match name {
        "openFile" => match arg("filePath") {
            Ok(p) => open_file(cx, core, p, args.get("startText").and_then(Value::as_str)).await,
            Err(e) => Err(e),
        },
        "openDiff" => match (arg("old_file_path"), arg("tab_name")) {
            (Ok(old), Ok(tab)) => {
                let contents = args.get("new_file_contents").and_then(Value::as_str).unwrap_or_default();
                let new = arg("new_file_path").unwrap_or(old);
                open_diff(cx, core, diffs, old, new, contents, tab).await
            }
            (Err(e), _) | (_, Err(e)) => Err(e),
        },
        "close_tab" => {
            diffs.lock().remove(arg("tab_name").unwrap_or_default());
            Ok(vec!["TAB_CLOSED".into()])
        }
        "closeAllDiffTabs" => {
            let n = std::mem::take(&mut *diffs.lock()).len();
            Ok(vec![format!("CLOSED_{n}_DIFF_TABS")])
        }
        "getWorkspaceFolders" => {
            let folders: Vec<Value> = cx
                .folders
                .iter()
                .map(|f| {
                    let name = f.file_name().map(|n| n.to_string_lossy()).unwrap_or_default();
                    json!({ "name": name, "uri": format!("file://{}", f.display()), "path": f })
                })
                .collect();
            Ok(vec![
                json!({ "success": true, "folders": folders, "rootPath": cx.folders.first() }).to_string(),
            ])
        }
        "getCurrentSelection" | "getLatestSelection" => no_selection(),
        // shortcut: Kelta does not track nvim buffers or LSP diagnostics; answer through nvim RPC if Claude needs them.
        "getOpenEditors" => Ok(vec![json!({ "tabs": [] }).to_string()]),
        "getDiagnostics" => Ok(vec!["[]".into()]),
        _ => return None,
    })
}

fn session_path(core: &Arc<dyn CoreApi>, sid: &SessionId, raw: &str) -> PathBuf {
    let p = Path::new(raw);
    match core.session_get(sid) {
        Some(s) if p.is_relative() => s.cwd.join(p),
        _ => p.to_path_buf(),
    }
}

async fn open_file(cx: &Cx, core: &Arc<dyn CoreApi>, raw: &str, start: Option<&str>) -> ToolOut {
    let path = session_path(core, &cx.sid, raw);
    // `startText` → its first line (1-based), the way the VS Code extension selects it.
    let line = match start.filter(|s| !s.is_empty()) {
        Some(s) => tokio::fs::read_to_string(&path)
            .await
            .ok()
            .and_then(|text| text.find(s).map(|at| text[..at].matches('\n').count() as u32 + 1)),
        None => None,
    };
    core.editor_open(mcp::editor_target(core, &cx.sid), &path, line).await.map_err(|e| e.message)?;
    Ok(vec![format!("Opened file: {}", path.display())])
}

/// Blocks until Claude closes the diff (`close_tab` / `closeAllDiffTabs`) or disconnects; the edit
/// is accepted or rejected in Claude's own prompt, so the answer is always `DIFF_REJECTED`.
// shortcut: saving the proposed buffer in nvim does not answer FILE_SAVED (no nvim → Kelta events);
// add a BufWritePost rpcnotify if accepting from the editor is wanted.
async fn open_diff(
    cx: &Cx,
    core: &Arc<dyn CoreApi>,
    diffs: &Diffs,
    old: &str,
    new: &str,
    contents: &str,
    tab: &str,
) -> ToolOut {
    let old = session_path(core, &cx.sid, old);
    let dir = cx.scratch.join(uuid::Uuid::new_v4().simple().to_string());
    let name = Path::new(new).file_name().map_or_else(|| "proposed".into(), |n| n.to_os_string());
    let proposed = dir.join(name);
    let written = std::fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(&dir)
        .and_then(|()| write_new(&proposed, contents.as_bytes()));
    if let Err(e) = written {
        let _ = std::fs::remove_dir_all(&dir);
        return Err(format!("cannot stage the proposed file: {e}"));
    }
    // Registered first: a `close_tab` racing the editor call still resolves this diff.
    let (tx, rx) = oneshot::channel();
    diffs.lock().insert(tab.to_owned(), tx);
    let target = mcp::editor_target(core, &cx.sid);
    if let Err(e) = core.editor_diff(target.clone(), &old, &proposed, false).await {
        diffs.lock().remove(tab);
        let _ = std::fs::remove_dir_all(&dir);
        return Err(e.message);
    }
    let _ = rx.await;
    let _ = core.editor_diff(target, &old, &proposed, true).await;
    let _ = std::fs::remove_dir_all(&dir);
    Ok(vec!["DIFF_REJECTED".into(), tab.to_owned()])
}

fn write_new(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    std::fs::OpenOptions::new().write(true).create_new(true).mode(0o600).open(path)?.write_all(bytes)
}

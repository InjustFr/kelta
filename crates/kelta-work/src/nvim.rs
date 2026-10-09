//! Minimal Neovim msgpack-RPC client over the `--listen` unix socket.
//!
//! One short-lived connection per operation (no background reader, nothing kept alive). Requests are
//! `[0, msgid, method, params]`, responses `[1, msgid, error, result]`; notifications (`[2, …]`) are
//! skipped.

use std::io::Cursor;
use std::path::Path;
use std::time::Duration;

use kelta_proto::error::KeltaError;
use rmpv::Value;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::UnixStream;

const CONNECT_TIMEOUT: Duration = Duration::from_secs(2);
const CALL_TIMEOUT: Duration = Duration::from_secs(5);

/// `nvim_exec_lua` body: `:edit` the file (fnameescape) and jump to the line.
pub const LUA_EDIT: &str = r#"
local file, line, focus = ...
local cur = vim.api.nvim_buf_get_name(0)
if focus or not vim.bo.modified or cur == file then
  vim.cmd('edit ' .. vim.fn.fnameescape(file))
  if line and line > 0 then
    local last = vim.api.nvim_buf_line_count(0)
    pcall(vim.api.nvim_win_set_cursor, 0, { math.min(line, last), 0 })
    vim.cmd('normal! zz')
  end
else
  vim.cmd('badd ' .. vim.fn.fnameescape(file))
end
return vim.api.nvim_buf_get_name(0)
"#;

/// `nvim_exec_lua` body: current (or last) visual selection → `{path, l1, l2, text}`.
pub const LUA_SELECTION: &str = r#"
local mode = vim.fn.mode()
local s, e, vmode
if mode == 'v' or mode == 'V' or mode == '\22' then
  s = vim.fn.getpos('v'); e = vim.fn.getpos('.'); vmode = mode
else
  s = vim.fn.getpos("'<"); e = vim.fn.getpos("'>"); vmode = vim.fn.visualmode()
end
local cur = vim.fn.line('.')
if s[2] == 0 or e[2] == 0 then
  return { path = vim.api.nvim_buf_get_name(0), l1 = cur, l2 = cur, text = vim.fn.getline(cur) }
end
local l1, l2 = math.min(s[2], e[2]), math.max(s[2], e[2])
local text = ''
local ok, region = pcall(vim.fn.getregion, s, e, { type = (vmode ~= '' and vmode) or 'v' })
if ok and type(region) == 'table' then text = table.concat(region, '\n') end
return { path = vim.api.nvim_buf_get_name(0), l1 = l1, l2 = l2, text = text }
"#;

/// `nvim_exec_lua` body: `:wall | mksession! <file>`.
pub const LUA_MKSESSION: &str = r#"
local file = ...
pcall(vim.cmd, 'silent! wall')
vim.cmd('mksession! ' .. vim.fn.fnameescape(file))
return file
"#;

/// A visual selection read from nvim.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Selection {
    pub path: String,
    pub l1: u32,
    pub l2: u32,
    pub text: String,
}

pub struct NvimClient {
    stream: UnixStream,
    next_id: u32,
    buf: Vec<u8>,
}

fn rpc_err(msg: impl Into<String>) -> KeltaError {
    KeltaError::upstream(format!("nvim rpc: {}", msg.into()))
}

impl NvimClient {
    pub async fn connect(socket: &Path) -> Result<Self, KeltaError> {
        // one-shot: connect deadline for this RPC operation.
        let stream = tokio::time::timeout(CONNECT_TIMEOUT, UnixStream::connect(socket))
            .await
            .map_err(|_| KeltaError::timeout(format!("nvim socket {} did not answer", socket.display())))?
            .map_err(|e| KeltaError::not_found(format!("nvim socket {}: {e}", socket.display())))?;
        Ok(Self { stream, next_id: 1, buf: Vec::new() })
    }

    /// Call `method` with `params`; returns the result or the nvim error as `Upstream`.
    pub async fn call(&mut self, method: &str, params: Vec<Value>) -> Result<Value, KeltaError> {
        let id = self.next_id;
        self.next_id += 1;
        let msg =
            Value::Array(vec![Value::from(0), Value::from(id), Value::from(method), Value::Array(params)]);
        let mut out = Vec::new();
        rmpv::encode::write_value(&mut out, &msg).map_err(|e| rpc_err(e.to_string()))?;
        // one-shot: per-call deadline.
        tokio::time::timeout(CALL_TIMEOUT, async {
            self.stream.write_all(&out).await.map_err(|e| rpc_err(e.to_string()))?;
            loop {
                let v = self.read_message().await?;
                let Value::Array(parts) = v else { continue };
                if parts.first().and_then(Value::as_u64) != Some(1) || parts.len() != 4 {
                    continue;
                }
                if parts[1].as_u64() != Some(u64::from(id)) {
                    continue;
                }
                if !parts[2].is_nil() {
                    return Err(rpc_err(error_text(&parts[2])));
                }
                return Ok(parts[3].clone());
            }
        })
        .await
        .map_err(|_| KeltaError::timeout(format!("nvim rpc {method} timed out")))?
    }

    async fn read_message(&mut self) -> Result<Value, KeltaError> {
        loop {
            if !self.buf.is_empty() {
                let mut cur = Cursor::new(&self.buf[..]);
                match rmpv::decode::read_value(&mut cur) {
                    Ok(v) => {
                        let used = cur.position() as usize;
                        self.buf.drain(..used);
                        return Ok(v);
                    }
                    Err(e) if is_eof(&e) => {}
                    Err(e) => return Err(rpc_err(e.to_string())),
                }
            }
            let mut chunk = [0u8; 8192];
            let n = self.stream.read(&mut chunk).await.map_err(|e| rpc_err(e.to_string()))?;
            if n == 0 {
                return Err(rpc_err("connection closed"));
            }
            self.buf.extend_from_slice(&chunk[..n]);
        }
    }

    pub async fn exec_lua(&mut self, code: &str, args: Vec<Value>) -> Result<Value, KeltaError> {
        self.call("nvim_exec_lua", vec![Value::from(code), Value::Array(args)]).await
    }

    pub async fn command(&mut self, cmd: &str) -> Result<(), KeltaError> {
        self.call("nvim_command", vec![Value::from(cmd)]).await.map(|_| ())
    }

    /// Open `file` at `line`. With `focus = false` a modified current buffer is kept (file is
    /// `:badd`-ed instead).
    pub async fn edit(&mut self, file: &Path, line: Option<u32>, focus: bool) -> Result<(), KeltaError> {
        let line = line.map(Value::from).unwrap_or(Value::Nil);
        self.exec_lua(LUA_EDIT, vec![Value::from(file.to_string_lossy().as_ref()), line, Value::from(focus)])
            .await
            .map(|_| ())
    }

    pub async fn checktime(&mut self) -> Result<(), KeltaError> {
        self.command("silent! checktime").await
    }

    pub async fn selection(&mut self) -> Result<Selection, KeltaError> {
        let v = self.exec_lua(LUA_SELECTION, vec![]).await?;
        let get = |k: &str| -> Option<&Value> {
            v.as_map()?.iter().find(|(kk, _)| kk.as_str() == Some(k)).map(|(_, vv)| vv)
        };
        let num = |k: &str| get(k).and_then(Value::as_u64).unwrap_or(0) as u32;
        let s = |k: &str| get(k).and_then(Value::as_str).unwrap_or_default().to_owned();
        Ok(Selection { path: s("path"), l1: num("l1"), l2: num("l2"), text: s("text") })
    }

    pub async fn mksession(&mut self, file: &Path) -> Result<(), KeltaError> {
        self.exec_lua(LUA_MKSESSION, vec![Value::from(file.to_string_lossy().as_ref())]).await.map(|_| ())
    }
}

fn is_eof(e: &rmpv::decode::Error) -> bool {
    match e {
        rmpv::decode::Error::InvalidMarkerRead(io) | rmpv::decode::Error::InvalidDataRead(io) => {
            io.kind() == std::io::ErrorKind::UnexpectedEof
        }
        _ => false,
    }
}

fn error_text(v: &Value) -> String {
    match v {
        Value::Array(a) => a.iter().filter_map(|x| x.as_str()).collect::<Vec<_>>().join(": "),
        Value::String(s) => s.as_str().unwrap_or_default().to_owned(),
        other => other.to_string(),
    }
}

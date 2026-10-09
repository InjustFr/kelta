//! MCP server for one Claude session at `/mcp/<sid>` (PLUGINS §8).
//!
//! Stateless Streamable-HTTP subset, hand-rolled JSON-RPC 2.0 (the documented fallback of
//! BUILD_PLAN §4 L7): every `POST` carries one message (or a batch) and is answered with
//! `application/json`; no SSE stream, no MCP session, no keep-alive timers. See
//! `docs/contract-requests/L7.md` for why rmcp's server transport is not used.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use kelta_proto::api::{CoreApi, Tracker};
use kelta_proto::codehost::{PrDraft, ReviewKind};
use kelta_proto::events::Notification;
use kelta_proto::ext::Urgency;
use kelta_proto::ids::SessionId;
use kelta_proto::model::{EditorTarget, Scope, WorkItem};
use kelta_proto::tracker::{TicketRef, Transition};
use serde_json::{Value, json};

/// Protocol revisions we speak, newest first.
pub(crate) const PROTOCOL_VERSIONS: &[&str] = &["2025-11-25", "2025-06-18", "2025-03-26", "2024-11-05"];

const PARSE_ERROR: i64 = -32700;
const INVALID_REQUEST: i64 = -32600;
const METHOD_NOT_FOUND: i64 = -32601;
const INVALID_PARAMS: i64 = -32602;

const NO_TICKET: &str = "no ticket linked";

/// Outcome of one HTTP body.
pub(crate) enum Reply {
    /// JSON body (a response object or a batch array).
    Json(Value),
    /// Only notifications/responses were received → 202 Accepted, empty body.
    Accepted,
    /// Malformed body → 400 with a JSON-RPC error.
    BadRequest(Value),
}

/// Handle one POST body for session `sid`.
pub(crate) async fn handle_body(core: &Arc<dyn CoreApi>, sid: &SessionId, body: &[u8]) -> Reply {
    let msg: Value = match serde_json::from_slice(body) {
        Ok(v) => v,
        Err(e) => return Reply::BadRequest(error(Value::Null, PARSE_ERROR, &format!("parse error: {e}"))),
    };
    match msg {
        Value::Array(items) if !items.is_empty() => {
            let mut out = Vec::new();
            for item in items {
                if let Some(r) = handle_message(core, sid, item).await {
                    out.push(r);
                }
            }
            if out.is_empty() { Reply::Accepted } else { Reply::Json(Value::Array(out)) }
        }
        Value::Object(_) => match handle_message(core, sid, msg).await {
            Some(r) => Reply::Json(r),
            None => Reply::Accepted,
        },
        _ => Reply::BadRequest(error(Value::Null, INVALID_REQUEST, "invalid request")),
    }
}

fn error(id: Value, code: i64, message: &str) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } })
}

fn result(id: Value, result: Value) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "result": result })
}

/// `None` for notifications and client responses (nothing to answer).
async fn handle_message(core: &Arc<dyn CoreApi>, sid: &SessionId, msg: Value) -> Option<Value> {
    let id = msg.get("id").cloned();
    let method = msg.get("method").and_then(Value::as_str);
    let (Some(id), Some(method)) = (id, method) else {
        // Notification (`notifications/initialized`, `notifications/cancelled`) or a response.
        let notification = method.is_some();
        let response = msg.get("result").is_some() || msg.get("error").is_some();
        return (!notification && !response).then(|| error(Value::Null, INVALID_REQUEST, "invalid request"));
    };
    if msg.get("jsonrpc").and_then(Value::as_str) != Some("2.0") {
        return Some(error(id, INVALID_REQUEST, "jsonrpc must be \"2.0\""));
    }
    let params = msg.get("params").cloned().unwrap_or(Value::Null);
    Some(match method {
        "initialize" => result(id, initialize(&params)),
        "ping" => result(id, json!({})),
        "tools/list" => result(id, json!({ "tools": tool_defs() })),
        "tools/call" => {
            let Some(name) = params.get("name").and_then(Value::as_str) else {
                return Some(error(id, INVALID_PARAMS, "missing tool name"));
            };
            if !TOOL_NAMES.contains(&name) {
                return Some(error(id, INVALID_PARAMS, &format!("unknown tool: {name}")));
            }
            let args = params.get("arguments").cloned().unwrap_or_else(|| json!({}));
            let out = call_tool(core, sid, name, &args).await;
            let (text, is_error) = match out {
                Ok(t) => (t, false),
                Err(t) => (t, true),
            };
            result(id, json!({ "content": [{ "type": "text", "text": text }], "isError": is_error }))
        }
        "resources/list" => result(id, json!({ "resources": [] })),
        "prompts/list" => result(id, json!({ "prompts": [] })),
        other => error(id, METHOD_NOT_FOUND, &format!("method not found: {other}")),
    })
}

fn initialize(params: &Value) -> Value {
    let requested = params.get("protocolVersion").and_then(Value::as_str);
    let version = requested.filter(|v| PROTOCOL_VERSIONS.contains(v)).unwrap_or(PROTOCOL_VERSIONS[0]);
    json!({
        "protocolVersion": version,
        "capabilities": { "tools": { "listChanged": false } },
        "serverInfo": { "name": "kelta", "title": "Kelta", "version": env!("CARGO_PKG_VERSION") },
        "instructions": "Kelta workbench tools for this Claude session: read and move the linked ticket, \
    comment on it, open files in the tab's editor, create the pull request, list review requests and notify the user."
    })
}

const TOOL_NAMES: &[&str] = &[
    "get_ticket",
    "transition_ticket",
    "add_ticket_comment",
    "open_in_editor",
    "create_pr",
    "list_review_requests",
    "get_review_feedback",
    "notify",
];

pub(crate) fn tool_defs() -> Value {
    let empty = json!({ "type": "object", "properties": {}, "additionalProperties": false });
    json!([
        {
            "name": "get_ticket",
            "description": "Get the tracker ticket linked to this session: key, title, status, URL, description and recent comments.",
            "inputSchema": empty,
        },
        {
            "name": "transition_ticket",
            "description": "Move the linked ticket to another status. `to` is a transition name, a target status name or a category (todo, in_progress, in_review, done).",
            "inputSchema": {
                "type": "object",
                "properties": { "to": { "type": "string", "description": "Target status / transition" } },
                "required": ["to"], "additionalProperties": false
            },
        },
        {
            "name": "add_ticket_comment",
            "description": "Add a Markdown comment to the linked ticket.",
            "inputSchema": {
                "type": "object",
                "properties": { "markdown": { "type": "string" } },
                "required": ["markdown"], "additionalProperties": false
            },
        },
        {
            "name": "open_in_editor",
            "description": "Open a file (absolute or relative to the session directory) in the editor of this tab, optionally at a line.",
            "inputSchema": {
                "type": "object",
                "properties": { "path": { "type": "string" }, "line": { "type": "integer", "minimum": 1 } },
                "required": ["path"], "additionalProperties": false
            },
        },
        {
            "name": "create_pr",
            "description": "Push the work branch and create the pull/merge request for this session's work item.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "title": { "type": "string" }, "body": { "type": "string" }, "draft": { "type": "boolean" }
                },
                "additionalProperties": false
            },
        },
        {
            "name": "list_review_requests",
            "description": "List the pull/merge requests awaiting the user's review.",
            "inputSchema": empty,
        },
        {
            "name": "get_review_feedback",
            "description": "Get the review feedback on this session's pull/merge request: unresolved review threads (author, file:line, comments), review summaries and failed checks with the end of their logs, as Markdown.",
            "inputSchema": empty,
        },
        {
            "name": "notify",
            "description": "Send a desktop notification to the user.",
            "inputSchema": {
                "type": "object",
                "properties": { "message": { "type": "string" } },
                "required": ["message"], "additionalProperties": false
            },
        },
    ])
}

type ToolResult = Result<String, String>;

fn str_arg<'a>(args: &'a Value, key: &str) -> Result<&'a str, String> {
    args.get(key)
        .and_then(Value::as_str)
        .filter(|s| !s.trim().is_empty())
        .ok_or_else(|| format!("missing argument `{key}`"))
}

async fn call_tool(core: &Arc<dyn CoreApi>, sid: &SessionId, name: &str, args: &Value) -> ToolResult {
    match name {
        "get_ticket" => get_ticket(core, sid).await,
        "transition_ticket" => transition_ticket(core, sid, str_arg(args, "to")?).await,
        "add_ticket_comment" => {
            let md = str_arg(args, "markdown")?;
            let (_, t) = linked(core, sid).await?;
            core.ticket_comment(&t, md, Some(sid)).await.map_err(|e| e.message)?;
            Ok(format!("Comment added to {}.", t.key))
        }
        "open_in_editor" => open_in_editor(core, sid, args).await,
        "create_pr" => create_pr(core, sid, args).await,
        "list_review_requests" => list_review_requests(core).await,
        "get_review_feedback" => {
            let w = work(core, sid).await.ok_or_else(|| "no work item linked to this session".to_owned())?;
            Ok(core.work_feedback(&w.id).await.map_err(|e| e.message)?.to_markdown())
        }
        "notify" => {
            let message = str_arg(args, "message")?;
            let session = core.session_get(sid);
            let n = Notification {
                title: session.as_ref().map_or_else(|| "Claude".to_owned(), |s| s.name.clone()),
                body: Some(message.to_owned()),
                urgency: Urgency::Normal,
                project_id: session.map(|s| s.project_id),
                session_id: Some(sid.clone()),
            };
            core.notify(n).await.map_err(|e| e.message)?;
            Ok("Notification sent.".to_owned())
        }
        other => Err(format!("unknown tool: {other}")),
    }
}

async fn work(core: &Arc<dyn CoreApi>, sid: &SessionId) -> Option<WorkItem> {
    core.work_for_session(sid).await
}

async fn linked(core: &Arc<dyn CoreApi>, sid: &SessionId) -> Result<(Arc<dyn Tracker>, TicketRef), String> {
    let t = work(core, sid).await.and_then(|w| w.ticket).ok_or_else(|| NO_TICKET.to_owned())?;
    let tracker = core.tracker_for(&t.account).await.map_err(|e| e.message)?;
    Ok((tracker, t))
}

/// Minimal HTML → text for comment bodies (tags dropped, common entities decoded).
fn html_text(html: &str) -> String {
    let mut out = String::with_capacity(html.len());
    let mut in_tag = false;
    for c in html.chars() {
        match c {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => out.push(c),
            _ => {}
        }
    }
    out.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&amp;", "&")
}

async fn get_ticket(core: &Arc<dyn CoreApi>, sid: &SessionId) -> ToolResult {
    let (tracker, t) = linked(core, sid).await?;
    let d = tracker.get(&t).await.map_err(|e| e.message)?;
    let tk = &d.ticket;
    let mut s = format!("# {}: {}\n\nStatus: {}\nURL: {}\n", tk.r#ref.key, tk.title, tk.status.name, tk.url);
    if let Some(a) = &tk.assignee {
        s.push_str(&format!("Assignee: {}\n", a.name));
    }
    if !tk.labels.is_empty() {
        s.push_str(&format!("Labels: {}\n", tk.labels.join(", ")));
    }
    s.push_str("\n## Description\n\n");
    s.push_str(if d.body_md.trim().is_empty() { "(empty)" } else { d.body_md.trim() });
    s.push('\n');
    if !d.comments.is_empty() {
        s.push_str("\n## Comments\n");
        for c in &d.comments {
            s.push_str(&format!(
                "\n### {} ({})\n\n{}\n",
                c.author.name,
                c.created_at,
                html_text(&c.body_html).trim()
            ));
        }
    }
    Ok(s)
}

fn pick_transition<'a>(ts: &'a [Transition], to: &str) -> Option<&'a Transition> {
    let to = to.trim();
    let norm = |s: &str| s.trim().to_lowercase().replace([' ', '-'], "_");
    ts.iter()
        .find(|t| t.id == to)
        .or_else(|| ts.iter().find(|t| t.name.eq_ignore_ascii_case(to)))
        .or_else(|| ts.iter().find(|t| t.to.name.eq_ignore_ascii_case(to)))
        .or_else(|| {
            let want = norm(to);
            ts.iter().find(|t| {
                serde_json::to_value(t.to.category).ok().and_then(|v| v.as_str().map(str::to_owned))
                    == Some(want.clone())
            })
        })
}

async fn transition_ticket(core: &Arc<dyn CoreApi>, sid: &SessionId, to: &str) -> ToolResult {
    let (tracker, t) = linked(core, sid).await?;
    let ts = tracker.transitions(&t).await.map_err(|e| e.message)?;
    let Some(tr) = pick_transition(&ts, to) else {
        let names: Vec<String> = ts.iter().map(|t| format!("{} → {}", t.name, t.to.name)).collect();
        return Err(format!("no transition matches {to:?}; available: {}", names.join(", ")));
    };
    if tr.needs_fields {
        return Err(format!(
            "transition {:?} requires extra fields; move the ticket from Kelta's UI",
            tr.name
        ));
    }
    let ticket = core.ticket_transition(&t, &tr.id, None, Some(sid)).await.map_err(|e| e.message)?;
    Ok(format!("{} is now {}.", ticket.r#ref.key, ticket.status.name))
}

async fn open_in_editor(core: &Arc<dyn CoreApi>, sid: &SessionId, args: &Value) -> ToolResult {
    let raw = str_arg(args, "path")?;
    let line = match args.get("line") {
        None | Some(Value::Null) => None,
        Some(v) => Some(
            v.as_u64()
                .and_then(|n| u32::try_from(n).ok())
                .filter(|n| *n >= 1)
                .ok_or_else(|| "`line` must be a positive integer".to_owned())?,
        ),
    };
    let session = core.session_get(sid);
    let path = PathBuf::from(raw);
    let path = if path.is_absolute() {
        path
    } else {
        match &session {
            Some(s) => s.cwd.join(&path),
            None => return Err("relative path but the session is unknown; pass an absolute path".to_owned()),
        }
    };
    let target = match session.and_then(|s| s.work_item_id) {
        Some(id) => EditorTarget::WorkItem { id },
        None => EditorTarget::Session { id: sid.clone() },
    };
    core.editor_open(target, Path::new(&path), line).await.map_err(|e| e.message)?;
    Ok(format!("Opened {}{}.", path.display(), line.map(|l| format!(":{l}")).unwrap_or_default()))
}

async fn create_pr(core: &Arc<dyn CoreApi>, sid: &SessionId, args: &Value) -> ToolResult {
    let w = work(core, sid).await.ok_or_else(|| "no work item linked to this session".to_owned())?;
    let draft = PrDraft {
        title: args.get("title").and_then(Value::as_str).map(str::to_owned),
        body: args.get("body").and_then(Value::as_str).map(str::to_owned),
        draft: args.get("draft").and_then(Value::as_bool),
    };
    let w = core.work_create_pr(&w.id, draft).await.map_err(|e| e.message)?;
    Ok(match w.pr_url {
        Some(url) => format!("Pull request: {url}"),
        None => "Pull request created.".to_owned(),
    })
}

async fn list_review_requests(core: &Arc<dyn CoreApi>) -> ToolResult {
    let items = core.review_list(Scope::All, ReviewKind::ReviewRequested).await.map_err(|e| e.message)?;
    if items.is_empty() {
        return Ok("No review requests.".to_owned());
    }
    let mut s = String::new();
    for it in items {
        let r = &it.review;
        s.push_str(&format!(
            "- {}#{} {} — {} ({}{})\n",
            r.r#ref.repo,
            r.r#ref.number,
            r.title,
            r.url,
            r.author.name,
            if r.draft { ", draft" } else { "" }
        ));
    }
    Ok(s)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn negotiates_version() {
        assert_eq!(initialize(&json!({"protocolVersion": "2025-03-26"}))["protocolVersion"], "2025-03-26");
        assert_eq!(
            initialize(&json!({"protocolVersion": "1999-01-01"}))["protocolVersion"],
            PROTOCOL_VERSIONS[0]
        );
    }

    #[test]
    fn html_to_text() {
        assert_eq!(html_text("<p>a &amp; <b>b</b></p>"), "a & b");
    }

    #[test]
    fn tool_defs_match_names() {
        let defs = tool_defs();
        let names: Vec<&str> = defs.as_array().unwrap().iter().map(|d| d["name"].as_str().unwrap()).collect();
        assert_eq!(names, TOOL_NAMES);
    }
}

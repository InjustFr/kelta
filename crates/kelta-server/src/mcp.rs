//! MCP server for one Claude session at `/mcp/<sid>` (PLUGINS §8), and for Claude sessions Kelta did
//! not spawn at `/mcp` (board tools only, ticket #234).
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
use kelta_proto::ids::{AccountId, ProjectId, SessionId};
use kelta_proto::model::{
    EditorTarget, Lifecycle, RunClaim, RunState, Scope, SessionKind, ShipOrigin, WorkItem,
};
use kelta_proto::tracker::{Ticket, TicketDetail, TicketRef, Transition};
use serde_json::{Value, json};

/// Protocol revisions we speak, newest first.
pub(crate) const PROTOCOL_VERSIONS: &[&str] = &["2025-11-25", "2025-06-18", "2025-03-26", "2024-11-05"];

pub(crate) const PARSE_ERROR: i64 = -32700;
const INVALID_REQUEST: i64 = -32600;
pub(crate) const METHOD_NOT_FOUND: i64 = -32601;
pub(crate) const INVALID_PARAMS: i64 = -32602;
pub(crate) const INTERNAL_ERROR: i64 = -32603;

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

/// Handle one POST body for session `sid` (`None` = the global `/mcp`).
pub(crate) async fn handle_body(core: &Arc<dyn CoreApi>, sid: Option<&SessionId>, body: &[u8]) -> Reply {
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

pub(crate) fn error(id: Value, code: i64, message: &str) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } })
}

pub(crate) fn result(id: Value, result: Value) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "result": result })
}

/// `None` for notifications and client responses (nothing to answer).
async fn handle_message(core: &Arc<dyn CoreApi>, sid: Option<&SessionId>, msg: Value) -> Option<Value> {
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
        "tools/list" => {
            let tools: Vec<Value> = tool_defs()
                .as_array()
                .into_iter()
                .flatten()
                .filter(|d| d["name"].as_str().is_some_and(|n| listed(core, sid, n)))
                .cloned()
                .collect();
            result(id, json!({ "tools": tools }))
        }
        "tools/call" => {
            let Some(name) = params.get("name").and_then(Value::as_str) else {
                return Some(error(id, INVALID_PARAMS, "missing tool name"));
            };
            if !TOOL_NAMES.contains(&name) || !listed(core, sid, name) {
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
        "instructions": "Kelta workbench tools: list the ticket board from Kelta's cache, read, move, comment, \
    edit or close any ticket by key; in a session Kelta started, also read and move the linked ticket, open \
    files in the tab's editor, create the pull request, list review requests, add line comments to the \
    pending review of a pull request under review and notify the user. Background runs claim a ticket \
    (claim_ticket), report each phase (report_progress) and release it (release_ticket)."
    })
}

/// Tools acting on the calling session's work item or tab: not on the global `/mcp`.
const SESSION_TOOLS: &[&str] = &[
    "get_ticket",
    "transition_ticket",
    "add_ticket_comment",
    "open_in_editor",
    "create_pr",
    "get_review_feedback",
    "add_review_comment",
    "notify",
];

/// Board-wide writes, behind `claude.mcp` and `claude.mcp_board_writes`.
const BOARD_WRITES: &[&str] =
    &["transition_ticket_by_key", "comment_ticket_by_key", "update_ticket_body", "close_ticket"];

fn listed(core: &Arc<dyn CoreApi>, sid: Option<&SessionId>, name: &str) -> bool {
    if sid.is_none() && SESSION_TOOLS.contains(&name) {
        return false;
    }
    if BOARD_WRITES.contains(&name) {
        let c = &core.settings(None).claude;
        return c.mcp && c.mcp_board_writes;
    }
    true
}

const TOOL_NAMES: &[&str] = &[
    "get_ticket",
    "transition_ticket",
    "add_ticket_comment",
    "open_in_editor",
    "create_pr",
    "list_review_requests",
    "get_review_feedback",
    "add_review_comment",
    "notify",
    "list_tickets",
    "get_ticket_by_key",
    "transition_ticket_by_key",
    "comment_ticket_by_key",
    "update_ticket_body",
    "close_ticket",
    "claim_ticket",
    "report_progress",
    "release_ticket",
];

pub(crate) fn tool_defs() -> Value {
    let empty = json!({ "type": "object", "properties": {}, "additionalProperties": false });
    let key = json!({ "type": "string", "description": "Ticket key, as list_tickets prints it (`SHOP-123`, `acme/shop#12`)" });
    let account = json!({ "type": "string", "description": "Tracker account id; needed only for a ticket list_tickets does not show" });
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
            "name": "add_review_comment",
            "description": "Add a line comment to the pending (draft) review on the pull request this session is reviewing. Nothing is published until the user submits the review in Kelta. `line` is a line of the new version of `path`.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "File path relative to the repository root" },
                    "line": { "type": "integer", "minimum": 1 },
                    "body": { "type": "string" }
                },
                "required": ["path", "line", "body"], "additionalProperties": false
            },
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
        {
            "name": "list_tickets",
            "description": "List the tickets of Kelta's ticket lists (all open projects, or one project / tracker view) as JSON: key, account, title, status, category, assignee, labels, priority, url. Served from Kelta's cache; `refresh` fetches again. Only the first page of each list.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "project": { "type": "string", "description": "Kelta project id (default: every open project)" },
                    "view": { "type": "string", "description": "Tracker view id of the project" },
                    "status": { "type": "string", "description": "Status name or category (todo, in_progress, in_review, done)" },
                    "assignee": { "type": "string", "description": "Assignee name or login; `none` = unassigned" },
                    "label": { "type": "string" },
                    "refresh": { "type": "boolean" }
                },
                "additionalProperties": false
            },
        },
        {
            "name": "get_ticket_by_key",
            "description": "Get any ticket by key: title, status, URL, links, description, recent comments and its revision (for update_ticket_body).",
            "inputSchema": {
                "type": "object",
                "properties": { "key": key, "account": account },
                "required": ["key"], "additionalProperties": false
            },
        },
        {
            "name": "transition_ticket_by_key",
            "description": "Move a ticket to another status. `to` is a transition name, a target status name or a category (todo, in_progress, in_review, done).",
            "inputSchema": {
                "type": "object",
                "properties": { "key": key, "account": account, "to": { "type": "string" } },
                "required": ["key", "to"], "additionalProperties": false
            },
        },
        {
            "name": "comment_ticket_by_key",
            "description": "Add a Markdown comment to a ticket.",
            "inputSchema": {
                "type": "object",
                "properties": { "key": key, "account": account, "markdown": { "type": "string" } },
                "required": ["key", "markdown"], "additionalProperties": false
            },
        },
        {
            "name": "update_ticket_body",
            "description": "Replace a ticket's description (e.g. to tick acceptance checkboxes). `revision` is the one get_ticket_by_key printed; if the description changed since, nothing is written and the call fails: read it again.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "key": key, "account": account,
                    "body": { "type": "string", "description": "The whole new description, Markdown" },
                    "revision": { "type": "string" }
                },
                "required": ["key", "body", "revision"], "additionalProperties": false
            },
        },
        {
            "name": "close_ticket",
            "description": "Close a ticket, with an optional comment first, on trackers where closing is apart from the board status (GitHub issues).",
            "inputSchema": {
                "type": "object",
                "properties": { "key": key, "account": account, "comment": { "type": "string" } },
                "required": ["key"], "additionalProperties": false
            },
        },
        {
            "name": "claim_ticket",
            "description": "Claim a ticket for this run: Kelta tracks it as a work item adopting the run's worktree and branch (never removed by Kelta), shown on the ticket with the run's phase. Fails with Conflict, naming the holder, when the ticket already has live work; pick another ticket then. Does not move the ticket on the board.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "key": key, "account": account,
                    "project": { "type": "string", "description": "Kelta project id (default: the ticket's)" },
                    "worktree": { "type": "string", "description": "Absolute path of a git worktree of one of the project's repos (default: the primary repo)" },
                    "branch": { "type": "string", "description": "Default: the worktree's checked-out branch" },
                    "run": {
                        "type": "object",
                        "properties": { "id": { "type": "string" }, "name": { "type": "string", "description": "Workflow name, shown on the ticket" } },
                        "additionalProperties": false
                    }
                },
                "required": ["key"], "additionalProperties": false
            },
        },
        {
            "name": "report_progress",
            "description": "Report how far the run on a claimed ticket got. `phase` is free text (the workflow's phase title); `state` drives the lamp; blocked and failed runs show in Kelta's Now with their `note`.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "key": key,
                    "phase": { "type": "string" },
                    "state": { "type": "string", "enum": ["running", "blocked", "failed", "done"] },
                    "note": { "type": "string" },
                    "sha": { "type": "string" }
                },
                "required": ["key", "phase", "state"], "additionalProperties": false
            },
        },
        {
            "name": "release_ticket",
            "description": "End the claim on a ticket: its work item is finished; Kelta leaves the worktree and branch alone (remove them yourself when merged).",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "key": key,
                    "outcome": { "type": "string", "enum": ["merged", "abandoned"] },
                    "sha": { "type": "string" }
                },
                "required": ["key", "outcome"], "additionalProperties": false
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

async fn call_tool(core: &Arc<dyn CoreApi>, sid: Option<&SessionId>, name: &str, args: &Value) -> ToolResult {
    match name {
        "list_tickets" => list_tickets(core, args).await,
        "get_ticket_by_key" => {
            let (tracker, t) = by_key(core, args).await?;
            let d = tracker.get(&t).await.map_err(|e| e.message)?;
            Ok(format!(
                "{}\nAccount: {}\nRevision: {}\n",
                ticket_markdown(&d),
                t.account,
                revision(&d.body_md)
            ))
        }
        "transition_ticket_by_key" => {
            let (tracker, t) = by_key(core, args).await?;
            transition(core, &tracker, &t, str_arg(args, "to")?, sid).await
        }
        "comment_ticket_by_key" => {
            let md = str_arg(args, "markdown")?;
            let (_, t) = by_key(core, args).await?;
            core.ticket_comment(&t, md, sid).await.map_err(|e| e.message)?;
            Ok(format!("Comment added to {}.", t.key))
        }
        "update_ticket_body" => update_ticket_body(core, args).await,
        "claim_ticket" => claim_ticket(core, sid, args).await,
        "report_progress" => {
            let state = match str_arg(args, "state")? {
                "running" => RunState::Running,
                "blocked" => RunState::Blocked,
                "failed" => RunState::Failed,
                "done" => RunState::Done,
                other => return Err(format!("InvalidArgument: unknown state {other:?}")),
            };
            let (key, phase) = (str_arg(args, "key")?, str_arg(args, "phase")?);
            core.work_report(key, Some(phase.to_owned()), state, opt_arg(args, "note"), opt_arg(args, "sha"))
                .await
                .map_err(coded)?;
            Ok(format!("{key}: {phase} ({}).", str_arg(args, "state")?))
        }
        "release_ticket" => {
            let key = str_arg(args, "key")?;
            let outcome = str_arg(args, "outcome")?;
            if !matches!(outcome, "merged" | "abandoned") {
                return Err(format!("InvalidArgument: outcome must be merged or abandoned, not {outcome:?}"));
            }
            core.work_release(key, opt_arg(args, "sha")).await.map_err(coded)?;
            Ok(format!("{key} released ({outcome})."))
        }
        "list_review_requests" => list_review_requests(core).await,
        "close_ticket" => {
            let (_, t) = by_key(core, args).await?;
            if let Some(md) = args.get("comment").and_then(Value::as_str).filter(|s| !s.trim().is_empty()) {
                core.ticket_comment(&t, md, sid).await.map_err(|e| e.message)?;
            }
            let ticket = core.ticket_close(&t).await.map_err(|e| e.message)?;
            Ok(format!("{} is now {}.", ticket.r#ref.key, ticket.status.name))
        }
        _ => {
            // `listed` keeps session tools off the global endpoint.
            let sid = sid.ok_or_else(|| format!("unknown tool: {name}"))?;
            call_session_tool(core, sid, name, args).await
        }
    }
}

async fn call_session_tool(core: &Arc<dyn CoreApi>, sid: &SessionId, name: &str, args: &Value) -> ToolResult {
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
        "get_review_feedback" => {
            let w = work(core, sid).await.ok_or_else(|| "no work item linked to this session".to_owned())?;
            Ok(core.work_feedback(&w.id).await.map_err(|e| e.message)?.to_markdown())
        }
        "add_review_comment" => add_review_comment(core, sid, args).await,
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
    // Not `linked`: a refine's sid reads its ticket but has no work item to write through.
    let t = core.ticket_for_session(sid).await.ok_or_else(|| NO_TICKET.to_owned())?;
    let tracker = core.tracker_for(&t.account).await.map_err(|e| e.message)?;
    Ok(ticket_markdown(&tracker.get(&t).await.map_err(|e| e.message)?))
}

fn ticket_markdown(d: &TicketDetail) -> String {
    let tk = &d.ticket;
    let mut s = format!("# {}: {}\n\nStatus: {}\nURL: {}\n", tk.r#ref.key, tk.title, tk.status.name, tk.url);
    if let Some(a) = &tk.assignee {
        s.push_str(&format!("Assignee: {}\n", a.name));
    }
    if !tk.labels.is_empty() {
        s.push_str(&format!("Labels: {}\n", tk.labels.join(", ")));
    }
    if let Some(p) = &d.parent {
        s.push_str(&format!("Parent: {}\n", p.key));
    }
    if !d.children.is_empty() {
        let keys: Vec<&str> = d.children.iter().map(|c| c.ticket.r#ref.key.as_str()).collect();
        s.push_str(&format!("Sub-tasks: {}\n", keys.join(", ")));
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
    s
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
    transition(core, &tracker, &t, to, Some(sid)).await
}

async fn transition(
    core: &Arc<dyn CoreApi>,
    tracker: &Arc<dyn Tracker>,
    t: &TicketRef,
    to: &str,
    sid: Option<&SessionId>,
) -> ToolResult {
    let ts = tracker.transitions(t).await.map_err(|e| e.message)?;
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
    let ticket = core.ticket_transition(t, &tr.id, None, sid).await.map_err(|e| e.message)?;
    Ok(format!("{} is now {}.", ticket.r#ref.key, ticket.status.name))
}

/// The ticket `args.key` names: from Kelta's lists, else `args.account` + the key (providers take keys).
async fn by_key(core: &Arc<dyn CoreApi>, args: &Value) -> Result<(Arc<dyn Tracker>, TicketRef), String> {
    let (t, _) = ref_by_key(core, args).await?;
    let tracker = core.tracker_for(&t.account).await.map_err(|e| e.message)?;
    Ok((tracker, t))
}

/// `by_key` without the tracker, plus the Kelta projects the listed ticket belongs to.
async fn ref_by_key(core: &Arc<dyn CoreApi>, args: &Value) -> Result<(TicketRef, Vec<ProjectId>), String> {
    let key = str_arg(args, "key")?;
    let account = args.get("account").and_then(Value::as_str).filter(|a| !a.is_empty());
    let listed = core.ticket_list(Scope::All, None, None, false).await.ok().and_then(|p| {
        p.items.into_iter().find(|i| {
            i.ticket.r#ref.key == key && account.is_none_or(|a| i.ticket.r#ref.account.as_str() == a)
        })
    });
    match (listed, account) {
        (Some(i), _) => Ok((i.ticket.r#ref, i.project_ids)),
        (None, Some(a)) => Ok((
            TicketRef { account: AccountId::new(a), key: key.to_owned(), id: key.to_owned() },
            Vec::new(),
        )),
        (None, None) => Err(format!("{key} is not in Kelta's ticket lists; pass its `account`")),
    }
}

fn opt_arg(args: &Value, key: &str) -> Option<String> {
    args.get(key).and_then(Value::as_str).filter(|s| !s.trim().is_empty()).map(str::to_owned)
}

/// `Conflict: …`, `InvalidArgument: …`: the run branches on the code.
fn coded(e: kelta_proto::error::KeltaError) -> String {
    format!("{:?}: {}", e.code, e.message)
}

async fn claim_ticket(core: &Arc<dyn CoreApi>, sid: Option<&SessionId>, args: &Value) -> ToolResult {
    let (ticket, projects) = ref_by_key(core, args).await?;
    let project = opt_arg(args, "project")
        .map(ProjectId::new)
        .or_else(|| projects.into_iter().next())
        .or_else(|| sid.and_then(|s| core.session_get(s)).map(|s| s.project_id))
        .ok_or_else(|| format!("no Kelta project for {}; pass `project`", ticket.key))?;
    let run = args.get("run");
    let run_str = |k: &str| run.and_then(|r| r.get(k)).and_then(Value::as_str).map(str::to_owned);
    let claim = RunClaim {
        name: run_str("name"),
        run_id: run_str("id"),
        session_id: sid.cloned(),
        ..RunClaim::default()
    };
    let key = ticket.key.clone();
    let w = core
        .work_claim(
            &project,
            ticket,
            opt_arg(args, "worktree").map(PathBuf::from),
            opt_arg(args, "branch"),
            claim,
        )
        .await
        .map_err(coded)?;
    Ok(format!("Claimed {key}: work item {} on {} ({}).", w.id, w.branch, w.worktree.display()))
}

/// Revision of a description: FNV-1a of its text, stable across restarts.
fn revision(body: &str) -> String {
    let h =
        body.bytes().fold(0xcbf2_9ce4_8422_2325_u64, |h, b| (h ^ u64::from(b)).wrapping_mul(0x100_0000_01b3));
    format!("{h:016x}")
}

async fn update_ticket_body(core: &Arc<dyn CoreApi>, args: &Value) -> ToolResult {
    let body = args.get("body").and_then(Value::as_str).ok_or("missing argument `body`")?;
    let want = str_arg(args, "revision")?;
    let (tracker, t) = by_key(core, args).await?;
    // shortcut: read-then-write, a write landing between the two is lost; trackers offer no compare-and-set.
    let now = revision(&tracker.get(&t).await.map_err(|e| e.message)?.body_md);
    if now != want {
        return Err(format!(
            "conflict: the description of {} changed since revision {want} (now {now}); read it again",
            t.key
        ));
    }
    core.ticket_set_body(&t, body).await.map_err(|e| e.message)?;
    Ok(format!("Description of {} updated; revision {}.", t.key, revision(body)))
}

async fn list_tickets(core: &Arc<dyn CoreApi>, args: &Value) -> ToolResult {
    let opt = |k: &str| args.get(k).and_then(Value::as_str).map(str::trim).filter(|s| !s.is_empty());
    let scope = match opt("project") {
        Some(id) => Scope::Project { id: id.into() },
        None => Scope::All,
    };
    let refresh = args.get("refresh").and_then(Value::as_bool).unwrap_or(false);
    let page = core
        .ticket_list(scope, opt("view").map(str::to_owned), None, refresh)
        .await
        .map_err(|e| e.message)?;
    let norm = |s: &str| s.trim().to_lowercase().replace([' ', '-'], "_");
    let category =
        |t: &Ticket| serde_json::to_value(t.status.category).ok().and_then(|v| v.as_str().map(str::to_owned));
    let rows: Vec<Value> = page
        .items
        .iter()
        .map(|i| &i.ticket)
        .filter(|t| {
            opt("status").is_none_or(|s| t.status.name.eq_ignore_ascii_case(s) || category(t) == Some(norm(s)))
        })
        .filter(|t| {
            opt("assignee").is_none_or(|a| match &t.assignee {
                None => a.eq_ignore_ascii_case("none"),
                Some(u) => u.name.eq_ignore_ascii_case(a) || u.login.as_deref().is_some_and(|l| l.eq_ignore_ascii_case(a)),
            })
        })
        .filter(|t| opt("label").is_none_or(|l| t.labels.iter().any(|x| x.eq_ignore_ascii_case(l))))
        .map(|t| {
            json!({
                "key": t.r#ref.key, "account": t.r#ref.account, "title": t.title, "status": t.status.name,
                "category": category(t), "assignee": t.assignee.as_ref().map(|u| u.login.clone().unwrap_or_else(|| u.name.clone())),
                "labels": t.labels, "priority": t.priority, "url": t.url,
            })
        })
        .collect();
    let mut out = json!({ "tickets": rows });
    if page.stale {
        out["stale"] = json!(true);
    }
    if !page.errors.is_empty() {
        out["errors"] = page
            .errors
            .iter()
            .map(|e| json!({ "account": e.account_id, "error": e.error.message }))
            .collect();
    }
    Ok(out.to_string())
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
    core.editor_open(editor_target(core, sid), Path::new(&path), line).await.map_err(|e| e.message)?;
    Ok(format!("Opened {}{}.", path.display(), line.map(|l| format!(":{l}")).unwrap_or_default()))
}

/// Editor pane of a Claude session: its work item's, else a live editor session of its project.
pub(crate) fn editor_target(core: &Arc<dyn CoreApi>, sid: &SessionId) -> EditorTarget {
    let Some(s) = core.session_get(sid) else { return EditorTarget::Session { id: sid.clone() } };
    if let Some(id) = s.work_item_id {
        return EditorTarget::WorkItem { id };
    }
    let editor = core
        .session_list(Some(&s.project_id))
        .into_iter()
        .find(|e| matches!(e.kind, SessionKind::Editor { .. }) && e.lifecycle == Lifecycle::Live);
    EditorTarget::Session { id: editor.map_or_else(|| sid.clone(), |e| e.id) }
}

async fn create_pr(core: &Arc<dyn CoreApi>, sid: &SessionId, args: &Value) -> ToolResult {
    let w = work(core, sid).await.ok_or_else(|| "no work item linked to this session".to_owned())?;
    let draft = PrDraft {
        title: args.get("title").and_then(Value::as_str).map(str::to_owned),
        body: args.get("body").and_then(Value::as_str).map(str::to_owned),
        draft: args.get("draft").and_then(Value::as_bool),
    };
    let w = core.work_create_pr(&w.id, draft, ShipOrigin::Mcp).await.map_err(|e| e.message)?;
    Ok(match w.pr_url {
        Some(url) => format!("Pull request: {url}"),
        None => "Pull request created.".to_owned(),
    })
}

async fn add_review_comment(core: &Arc<dyn CoreApi>, sid: &SessionId, args: &Value) -> ToolResult {
    let (path, body) = (str_arg(args, "path")?, str_arg(args, "body")?);
    let line = args
        .get("line")
        .and_then(Value::as_u64)
        .and_then(|n| u32::try_from(n).ok())
        .filter(|n| *n >= 1)
        .ok_or_else(|| "`line` must be a positive integer".to_owned())?;
    let w = work(core, sid).await.ok_or("no pull request under review in this session")?;
    let r = w.review.ok_or("no pull request under review in this session")?;
    let host = core.code_host_for(&r.account).await.map_err(|e| e.message)?;
    // Line numbers come from the local checkout; the pending review lands on the remote head.
    let remote = host.get(&r).await.map_err(|e| e.message)?.review.head_sha;
    let local = tokio::process::Command::new("git")
        .arg("-C")
        .arg(&w.worktree)
        .args(["rev-parse", "HEAD"])
        .output()
        .await;
    if !local.is_ok_and(|o| String::from_utf8_lossy(&o.stdout).trim() == remote) {
        return Err("checkout is behind the PR head, update it first".to_owned());
    }
    host.add_pending_comment(&r, path, line, body).await.map_err(|e| e.message)?;
    Ok(format!(
        "Pending comment added on {path}:{line} of {}#{}. The user submits the review.",
        r.repo, r.number
    ))
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

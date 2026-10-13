//! Pure session status / attention decisions (ARCHITECTURE §7.5, §7.6).
//!
//! Hooks deliver a [`StatusChange`] (from `kelta_server::hooks::map`); the hooks-inactive fallback
//! derives changes from terminal output (heuristic). Both are folded into the session state by the
//! functions below, which are table-tested.

use kelta_proto::hooks::{HookPayload, names};
use kelta_proto::model::{Attention, ClaudeTask, ClaudeTaskState, SessionStatus, StatusChange};
use serde_json::Value;

/// Notification candidates; whether they fire is decided by [`crate::notifier`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NotifyKind {
    ClaudeNeedsInput,
    ClaudeDone,
    BellBackground,
    ReviewRequested,
    CiFailedMine,
    PrApproved,
    PrChangesRequested,
    /// OSC 9/777 desktop-notification request from a program.
    Program,
}

/// The parts of a session the decisions depend on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SessState {
    pub status: SessionStatus,
    pub attention: Attention,
    pub seen: bool,
    /// Pane on screen (active project, active tab, window visible).
    pub visible: bool,
}

/// Result of folding one change into a session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Decision {
    pub status: SessionStatus,
    pub attention: Attention,
    pub seen: bool,
    /// `Some(true)` on SessionStart.
    pub hooks_active: Option<bool>,
    pub notify: Option<NotifyKind>,
}

impl Decision {
    fn keep(prev: &SessState) -> Self {
        Self {
            status: prev.status,
            attention: prev.attention,
            seen: prev.seen,
            hooks_active: None,
            notify: None,
        }
    }
}

/// Attention for a state the user may not have looked at: none while visible.
fn unless_visible(prev: &SessState, a: Attention) -> (Attention, bool) {
    if prev.visible { (Attention::None, true) } else { (a, false) }
}

/// Fold a hook [`StatusChange`] (ARCHITECTURE §7.6 table).
pub fn apply_hook(prev: &SessState, change: &StatusChange) -> Decision {
    let mut d = Decision::keep(prev);
    // A tool ran after a permission prompt: the user answered, Claude is working again (#278).
    // shortcut: any tool counts, so a parallel tool or subagent can clear another tool's pending
    // prompt; the next Stop or prompt corrects it, match the tool name if that ever bites.
    let tool_ran = prev.status == SessionStatus::NeedsInput && change.raw_event == names::POST_TOOL_USE;
    let status = if tool_ran && change.status == SessionStatus::Unknown {
        SessionStatus::Working
    } else {
        change.status
    };
    match status {
        // PostToolUse & co: status unchanged.
        SessionStatus::Unknown => {}
        SessionStatus::Starting | SessionStatus::Running => {
            d.status = SessionStatus::Running;
            d.hooks_active = Some(true);
        }
        SessionStatus::Working => {
            d.status = SessionStatus::Working;
            let (a, seen) = unless_visible(prev, Attention::Activity);
            d.attention = a;
            d.seen = seen;
        }
        SessionStatus::NeedsInput => {
            d.status = SessionStatus::NeedsInput;
            d.attention = Attention::NeedsInput;
            d.seen = prev.visible;
            d.notify = Some(NotifyKind::ClaudeNeedsInput);
        }
        SessionStatus::WaitingUser => {
            d.status = SessionStatus::WaitingUser;
            if !prev.seen && !prev.visible {
                d.attention = Attention::NeedsInput;
                d.notify = Some(NotifyKind::ClaudeNeedsInput);
            } else if prev.attention == Attention::NeedsInput && prev.status != SessionStatus::NeedsInput {
                d.attention = Attention::None;
            }
        }
        SessionStatus::Done => {
            d.status = SessionStatus::Done;
            let (a, seen) = unless_visible(prev, Attention::Done);
            d.attention = a;
            d.seen = seen;
            d.notify = Some(NotifyKind::ClaudeDone);
        }
        SessionStatus::Error => {
            d.status = SessionStatus::Error;
            d.attention = Attention::Error;
            d.seen = prev.visible;
        }
        SessionStatus::Exited => {
            d.status = SessionStatus::Exited;
            if prev.attention == Attention::NeedsInput {
                d.attention = Attention::None;
            }
        }
    }
    d
}

/// Inputs of the hooks-inactive heuristic (ARCHITECTURE §7.6, last paragraph).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Heuristic {
    /// Terminal output.
    Output,
    /// BEL / OSC 9 / OSC 777.
    Alert,
    /// 3 s without output after output.
    Quiet,
    /// User input written to the pane (applied with hooks active too).
    Input,
}

pub fn apply_heuristic(prev: &SessState, ev: Heuristic) -> Decision {
    let change = |status| StatusChange {
        status,
        preview: None,
        file_edited: None,
        raw_event: "heuristic".into(),
        session_uuid: None,
    };
    match ev {
        Heuristic::Output => {
            if prev.status == SessionStatus::Working {
                Decision::keep(prev)
            } else {
                apply_hook(prev, &change(SessionStatus::Working))
            }
        }
        Heuristic::Alert => apply_hook(prev, &change(SessionStatus::NeedsInput)),
        Heuristic::Quiet => {
            if prev.status == SessionStatus::Working {
                apply_hook(prev, &change(SessionStatus::Done))
            } else {
                Decision::keep(prev)
            }
        }
        // The keystroke answering a permission prompt / elicitation: the user saw it (#278). Only
        // NeedsInput: an idle_prompt (WaitingUser) waits for UserPromptSubmit.
        Heuristic::Input => {
            let mut d = Decision::keep(prev);
            if prev.status == SessionStatus::NeedsInput {
                d.status = SessionStatus::Working;
                d.attention = Attention::None;
                d.seen = true;
            }
            d
        }
    }
}

/// MCP `ask` (#356): it needs the user like a permission prompt, but the status stays the hooks' (the
/// buttons wait while that status is a real prompt).
pub fn asked(prev: &SessState) -> Decision {
    Decision {
        attention: Attention::NeedsInput,
        seen: prev.visible,
        notify: Some(NotifyKind::ClaudeNeedsInput),
        ..Decision::keep(prev)
    }
}

/// Output on a session not driven by hooks or heuristics (shells, editors, tools, hooked Claude).
pub fn apply_activity(prev: &SessState, kind_is_hooked_claude: bool) -> Decision {
    let mut d = Decision::keep(prev);
    if prev.status == SessionStatus::Starting {
        d.status = SessionStatus::Running;
    }
    if !kind_is_hooked_claude && !prev.visible && prev.attention < Attention::Activity {
        d.attention = Attention::Activity;
        d.seen = false;
    }
    d
}

/// BEL on a plain session (`terminal.bell = attention`).
pub fn apply_bell(prev: &SessState) -> Decision {
    let mut d = Decision::keep(prev);
    if !prev.visible {
        d.attention = d.attention.max(Attention::Activity);
        d.seen = false;
        d.notify = Some(NotifyKind::BellBackground);
    }
    d
}

/// `session_mark_seen`: clears everything but a pending needs-input.
pub fn mark_seen(prev: &SessState) -> Decision {
    let mut d = Decision::keep(prev);
    d.seen = true;
    if !(prev.attention == Attention::NeedsInput && prev.status == SessionStatus::NeedsInput) {
        d.attention = Attention::None;
    }
    d
}

/// Maximum preview length (chars).
pub const PREVIEW_MAX: usize = 200;

pub fn truncate_preview(s: &str) -> String {
    let t = s.trim();
    if t.chars().count() <= PREVIEW_MAX { t.to_owned() } else { t.chars().take(PREVIEW_MAX).collect() }
}

/// Fold a hook into Claude's task list (#406); true when it changed. `TodoWrite` replaces the list,
/// `TaskCreate` appends, `TaskUpdate` changes one task, `SessionStart` `clear` empties it. Unknown
/// tools and shapes leave it as is.
pub fn apply_tasks(tasks: &mut Vec<ClaudeTask>, p: &HookPayload) -> bool {
    let before = tasks.clone();
    let input = p.tool_input.as_ref();
    let field = |v: Option<&Value>, k: &str| v.and_then(|v| v.get(k)).and_then(Value::as_str).map(str::to_owned);
    let event = p.hook_event_name.as_str();
    if event == names::SESSION_START && p.source.as_deref() == Some("clear") {
        tasks.clear();
    }
    if event != names::POST_TOOL_USE {
        return *tasks != before;
    }
    match p.tool_name.as_deref() {
        Some("TodoWrite") => {
            let todos = input.and_then(|i| i.get("todos")).and_then(Value::as_array);
            let parsed = todos.and_then(|todos| {
                todos
                    .iter()
                    .map(|t| {
                        let state = task_state(&field(Some(t), "status")?)?;
                        Some(ClaudeTask { id: None, title: field(Some(t), "content")?, state })
                    })
                    .collect::<Option<Vec<_>>>()
            });
            if let Some(list) = parsed {
                *tasks = list;
            }
        }
        Some("TaskCreate") => {
            if let Some(title) = field(input, "subject") {
                // Claude numbers its tasks from 1; the response carries the id `TaskUpdate` names.
                let id = field(p.extra.get("tool_response").and_then(|r| r.get("task")), "id")
                    .unwrap_or_else(|| (tasks.len() + 1).to_string());
                tasks.push(ClaudeTask { id: Some(id), title, state: ClaudeTaskState::Pending });
            }
        }
        Some("TaskUpdate") => {
            let id = field(input, "taskId");
            if let Some(i) = tasks.iter().position(|t| id.is_some() && t.id == id) {
                match field(input, "status").as_deref() {
                    Some("deleted") => {
                        tasks.remove(i);
                    }
                    status => {
                        if let Some(st) = status.and_then(task_state) {
                            tasks[i].state = st;
                        }
                        if let Some(title) = field(input, "subject") {
                            tasks[i].title = title;
                        }
                    }
                }
            }
        }
        _ => {}
    }
    *tasks != before
}

fn task_state(s: &str) -> Option<ClaudeTaskState> {
    match s {
        "pending" => Some(ClaudeTaskState::Pending),
        "in_progress" => Some(ClaudeTaskState::InProgress),
        "completed" => Some(ClaudeTaskState::Completed),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn hook(v: Value) -> HookPayload {
        serde_json::from_value(v).unwrap()
    }

    fn tool(name: &str, input: Value) -> HookPayload {
        hook(json!({ "hook_event_name": "PostToolUse", "tool_name": name, "tool_input": input }))
    }

    fn task(id: Option<&str>, title: &str, state: ClaudeTaskState) -> ClaudeTask {
        ClaudeTask { id: id.map(Into::into), title: title.into(), state }
    }

    #[test]
    fn todo_write_replaces_the_list() {
        let mut tasks = vec![task(Some("1"), "old", ClaudeTaskState::Pending)];
        let p = tool(
            "TodoWrite",
            json!({ "todos": [
                { "content": "Read", "status": "completed", "activeForm": "Reading" },
                { "content": "Write", "status": "in_progress", "activeForm": "Writing" },
            ] }),
        );
        assert!(apply_tasks(&mut tasks, &p));
        assert_eq!(
            tasks,
            vec![task(None, "Read", ClaudeTaskState::Completed), task(None, "Write", ClaudeTaskState::InProgress)]
        );
        assert!(!apply_tasks(&mut tasks, &p), "same list: no change");
    }

    #[test]
    fn task_create_appends_and_task_update_changes_a_state() {
        let mut tasks = vec![];
        let mut create = tool("TaskCreate", json!({ "subject": "Parse", "description": "d", "activeForm": "Parsing" }));
        create.extra.insert("tool_response".into(), json!({ "task": { "id": "7", "subject": "Parse" } }));
        assert!(apply_tasks(&mut tasks, &create));
        // No response: the next number.
        assert!(apply_tasks(&mut tasks, &tool("TaskCreate", json!({ "subject": "Test" }))));
        assert_eq!(
            tasks,
            vec![task(Some("7"), "Parse", ClaudeTaskState::Pending), task(Some("2"), "Test", ClaudeTaskState::Pending)]
        );

        assert!(apply_tasks(&mut tasks, &tool("TaskUpdate", json!({ "taskId": "7", "status": "in_progress" }))));
        assert_eq!(tasks[0].state, ClaudeTaskState::InProgress);
        assert!(!apply_tasks(&mut tasks, &tool("TaskUpdate", json!({ "taskId": "99", "status": "completed" }))));
        assert!(apply_tasks(&mut tasks, &tool("TaskUpdate", json!({ "taskId": "2", "status": "deleted" }))));
        assert_eq!(tasks.len(), 1);
    }

    #[test]
    fn unknown_tools_and_shapes_are_ignored() {
        let mut tasks = vec![task(Some("1"), "a", ClaudeTaskState::Pending)];
        for p in [
            tool("Bash", json!({ "command": "ls" })),
            tool("TodoWrite", json!({ "todos": "nope" })),
            tool("TodoWrite", json!({ "todos": [{ "content": "x", "status": "weird" }] })),
            tool("TaskCreate", json!({ "title": "no subject" })),
            tool("TaskUpdate", json!({ "taskId": "1", "status": "weird" })),
            tool("TaskUpdate", json!(null)),
            hook(json!({ "hook_event_name": "SessionStart", "source": "resume" })),
            hook(json!({ "hook_event_name": "Stop" })),
        ] {
            assert!(!apply_tasks(&mut tasks, &p), "{p:?}");
        }
    }

    #[test]
    fn clear_empties_the_list() {
        let mut tasks = vec![task(None, "a", ClaudeTaskState::Completed)];
        assert!(apply_tasks(&mut tasks, &hook(json!({ "hook_event_name": "SessionStart", "source": "clear" }))));
        assert!(tasks.is_empty());
    }

    #[test]
    fn preview_truncates_on_chars() {
        let s = "é".repeat(300);
        assert_eq!(truncate_preview(&s).chars().count(), PREVIEW_MAX);
    }
}

//! Live view of the Claude Code `Workflow` runs of a session (#272).
//!
//! Driven by hooks, no watcher: the `PostToolUse` of `Workflow` names a new run (`wf_<id>` in its
//! response), `SubagentStart` / `SubagentStop` (which fire for workflow agents) name the run of an
//! agent, and every hook of the session re-reads its unfinished runs (the final record is written
//! after the last agent stops, before Claude's next turn). The files are Claude internals observed
//! on 2026-10-11 (ARCHITECTURE §7.7); every field is optional and a bad line is skipped.

use std::collections::{BTreeMap, BTreeSet};
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use kelta_proto::error::KeltaError;
use kelta_proto::events::UiEvent;
use kelta_proto::hooks::{HookPayload, names};
use kelta_proto::ids::SessionId;
use kelta_proto::model::{ClaudeRun, ClaudeRunAgent, ClaudeRunPhase, TranscriptLine};
use serde_json::Value;

use crate::Core;

const RUNNING: &str = "running";
const RESULT_CHARS: usize = 4000;
/// Bytes read from the end of an agent transcript, and lines kept.
const TAIL_BYTES: u64 = 256 * 1024;
const TAIL_LINES: usize = 40;

/// The runs Kelta saw in one session (`SessionEntry::claude_runs`).
#[derive(Default)]
pub struct Runs(BTreeMap<String, Tracked>);

struct Tracked {
    /// `~/.claude/projects/<slug>/<claude session uuid>` (the hook's `transcript_path` minus `.jsonl`).
    dir: PathBuf,
    /// Agents a `SubagentStop` reported: done before the journal says so.
    stopped: BTreeSet<String>,
    last: Option<ClaudeRun>,
}

impl Core {
    pub(crate) fn claude_runs_hook(&self, sid: &SessionId, p: &HookPayload) {
        let dir = p.transcript_path.as_deref().map(|t| t.with_extension(""));
        let found = dir.as_deref().and_then(|d| run_of(d, p));
        let todo: Vec<(String, PathBuf, BTreeSet<String>)> = {
            let mut s = self.sessions.lock();
            let Some(e) = s.get_mut(sid) else { return };
            if let (Some(id), Some(d)) = (found, &dir) {
                e.claude_runs.0.entry(id).or_insert_with(|| Tracked {
                    dir: d.clone(),
                    stopped: BTreeSet::new(),
                    last: None,
                });
            }
            let stop =
                (p.hook_event_name == names::SUBAGENT_STOP).then(|| str_field(p, "agent_id")).flatten();
            let mut todo = Vec::new();
            for (id, t) in e
                .claude_runs
                .0
                .iter_mut()
                .filter(|(_, t)| t.last.as_ref().is_none_or(|r| r.status == RUNNING))
            {
                // Agent ids are unique: marking it in every unfinished run is harmless.
                t.stopped.extend(stop.map(str::to_owned));
                todo.push((id.clone(), t.dir.clone(), t.stopped.clone()));
            }
            todo
        };
        for (id, dir, stopped) in todo {
            let run = read_run(&dir, &id, sid, &stopped);
            let changed = {
                let mut s = self.sessions.lock();
                let Some(t) = s.get_mut(sid).and_then(|e| e.claude_runs.0.get_mut(&id)) else { continue };
                let changed = t.last.as_ref() != Some(&run);
                t.last = Some(run.clone());
                changed
            };
            if changed {
                self.emit(UiEvent::WorkflowUpdated { run });
            }
        }
    }

    /// `claude_runs_list`: the session's runs, as last read.
    pub fn claude_runs_list(&self, sid: &SessionId) -> Vec<ClaudeRun> {
        let s = self.sessions.lock();
        s.get(sid)
            .map(|e| e.claude_runs.0.values().filter_map(|t| t.last.clone()).collect())
            .unwrap_or_default()
    }

    /// `claude_run_tail`: the last lines of one agent's transcript.
    pub fn claude_run_tail(
        &self,
        sid: &SessionId,
        run_id: &str,
        agent_id: &str,
    ) -> Result<Vec<TranscriptLine>, KeltaError> {
        if !safe_id(agent_id) {
            return Err(KeltaError::invalid(format!("agent id {agent_id:?}")));
        }
        let dir = {
            let s = self.sessions.lock();
            s.get(sid).and_then(|e| e.claude_runs.0.get(run_id)).map(|t| t.dir.clone())
        }
        .ok_or_else(|| KeltaError::not_found(format!("workflow run {run_id}")))?;
        let path = dir.join("subagents/workflows").join(run_id).join(format!("agent-{agent_id}.jsonl"));
        Ok(transcript_tail(&read_tail(&path)?))
    }
}

/// The run a hook names, if any.
fn run_of(dir: &Path, p: &HookPayload) -> Option<String> {
    match p.hook_event_name.as_str() {
        names::POST_TOOL_USE if p.tool_name.as_deref() == Some("Workflow") => {
            find_run_id(&p.extra.get("tool_response")?.to_string())
        }
        names::SUBAGENT_START | names::SUBAGENT_STOP => {
            let from_path = str_field(p, "agent_transcript_path")
                .and_then(|t| Path::new(t).parent()?.file_name()?.to_str().map(str::to_owned))
                .filter(|n| n.starts_with("wf_") && safe_id(n));
            from_path.or_else(|| {
                let agent = str_field(p, "agent_id").filter(|a| safe_id(a))?;
                std::fs::read_dir(dir.join("subagents/workflows")).ok()?.flatten().find_map(|d| {
                    let run = d.file_name().into_string().ok().filter(|n| n.starts_with("wf_"))?;
                    let p = d.path();
                    (p.join(format!("agent-{agent}.meta.json")).exists()
                        || p.join(format!("agent-{agent}.jsonl")).exists())
                    .then_some(run)
                })
            })
        }
        _ => None,
    }
}

fn str_field<'a>(p: &'a HookPayload, key: &str) -> Option<&'a str> {
    p.extra.get(key).and_then(Value::as_str)
}

/// The first `wf_<id>` in `text`.
fn find_run_id(text: &str) -> Option<String> {
    let start = text.find("wf_")?;
    let id: String =
        text[start..].chars().take_while(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '-').collect();
    (id.len() > 3).then_some(id)
}

/// A path component Claude generated (`wf_0bce8ca-123`, `a46a2c3f2853`): no separators, no dots.
fn safe_id(s: &str) -> bool {
    !s.is_empty() && s.len() <= 64 && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

fn read_json(path: &Path) -> Option<Value> {
    serde_json::from_str(&std::fs::read_to_string(path).ok()?).ok()
}

/// Build the run from its final record (`workflows/<id>.json`, written when it ends), its journal
/// (`started` / `result` per agent) and the agents' `*.meta.json` the journal lacks.
fn read_run(dir: &Path, run_id: &str, sid: &SessionId, stopped: &BTreeSet<String>) -> ClaudeRun {
    let fin = read_json(&dir.join("workflows").join(format!("{run_id}.json"))).filter(Value::is_object);
    let fin = fin.as_ref();
    let get = |k: &str| fin.and_then(|f| f.get(k));
    let num = |k: &str| get(k).and_then(Value::as_u64);
    let run_dir = dir.join("subagents/workflows").join(run_id);

    // (id, label, phase, done) in spawn order.
    let mut agents: Vec<(String, String, String, bool)> = Vec::new();
    let journal = std::fs::read_to_string(run_dir.join("journal.jsonl")).unwrap_or_default();
    for line in journal.lines() {
        let Ok(v) = serde_json::from_str::<Value>(line) else { continue };
        let s = |k: &str| v.get(k).and_then(Value::as_str).unwrap_or_default().to_owned();
        let id = s("agentId");
        if id.is_empty() {
            continue;
        }
        let at = agents.iter().position(|a| a.0 == id);
        match (v.get("type").and_then(Value::as_str), at) {
            (Some("started"), None) => agents.push((id, s("label"), s("phase"), false)),
            (Some("result"), Some(i)) => agents[i].3 = true,
            (Some("result"), None) => agents.push((id, String::new(), String::new(), true)),
            _ => {}
        }
    }
    let mut metas: Vec<PathBuf> =
        std::fs::read_dir(&run_dir).map(|rd| rd.flatten().map(|d| d.path()).collect()).unwrap_or_default();
    metas.sort();
    for path in metas {
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or_default();
        let Some(id) = name.strip_prefix("agent-").and_then(|n| n.strip_suffix(".meta.json")) else {
            continue;
        };
        let meta = read_json(&path);
        let m = |k: &str| {
            meta.as_ref().and_then(|v| v.get(k)).and_then(Value::as_str).unwrap_or_default().to_owned()
        };
        match agents.iter_mut().find(|a| a.0 == id) {
            Some(a) => {
                if a.1.is_empty() {
                    a.1 = m("description");
                }
                if a.2.is_empty() {
                    a.2 = m("workflowPhase");
                }
            }
            None => agents.push((id.to_owned(), m("description"), m("workflowPhase"), false)),
        }
    }

    let mut phases: Vec<ClaudeRunPhase> = get("phases")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|p| p.get("title")?.as_str())
        .map(|t| ClaudeRunPhase { title: t.to_owned(), agents: Vec::new() })
        .collect();
    for (id, label, phase, done) in agents {
        let title = if phase.is_empty() { "Other".to_owned() } else { phase };
        let i = match phases.iter().position(|p| p.title == title) {
            Some(i) => i,
            None => {
                phases.push(ClaudeRunPhase { title, agents: Vec::new() });
                phases.len() - 1
            }
        };
        let done = done || stopped.contains(&id);
        let label = if label.is_empty() { id.clone() } else { label };
        phases[i].agents.push(ClaudeRunAgent { id, label, done });
    }

    let name = get("workflowName").and_then(Value::as_str).map(str::to_owned).or_else(|| {
        let suffix = format!("-{run_id}.js");
        std::fs::read_dir(dir.join("workflows/scripts"))
            .ok()?
            .flatten()
            .find_map(|d| d.file_name().to_str()?.strip_suffix(&suffix).map(str::to_owned))
    });
    let status = match fin {
        None => RUNNING.to_owned(),
        Some(f) => f.get("status").and_then(Value::as_str).unwrap_or("finished").to_owned(),
    };
    let result = get("result").filter(|v| !v.is_null()).map(|v| match v {
        Value::String(s) => truncate(s, RESULT_CHARS),
        v => truncate(&serde_json::to_string_pretty(v).unwrap_or_default(), RESULT_CHARS),
    });
    let logs = get("logs")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|l| l.as_str().map(str::to_owned))
        .collect();
    ClaudeRun {
        run_id: run_id.to_owned(),
        session_id: sid.clone(),
        name,
        status,
        phases,
        duration_ms: num("durationMs"),
        total_tokens: num("totalTokens"),
        total_tool_calls: num("totalToolCalls"),
        result,
        logs,
    }
}

fn truncate(s: &str, max: usize) -> String {
    let t = s.trim();
    match t.char_indices().nth(max) {
        Some((i, _)) => format!("{}…", &t[..i]),
        None => t.to_owned(),
    }
}

/// The last [`TAIL_BYTES`] of a file, from its first whole line.
fn read_tail(path: &Path) -> Result<String, KeltaError> {
    let mut f = std::fs::File::open(path)?;
    let from = f.metadata()?.len().saturating_sub(TAIL_BYTES);
    f.seek(SeekFrom::Start(from))?;
    let mut buf = Vec::new();
    f.read_to_end(&mut buf)?;
    let text = String::from_utf8_lossy(&buf);
    let text = if from > 0 { text.split_once('\n').map_or("", |(_, rest)| rest) } else { &text };
    Ok(text.to_owned())
}

/// Assistant prose, tool calls and tool results of a transcript, the last [`TAIL_LINES`].
fn transcript_tail(text: &str) -> Vec<TranscriptLine> {
    let line = |kind: &str, text: String| TranscriptLine { kind: kind.to_owned(), text };
    let mut out = Vec::new();
    for raw in text.lines() {
        let Ok(v) = serde_json::from_str::<Value>(raw) else { continue };
        let role = v.get("type").and_then(Value::as_str);
        let content = v.get("message").and_then(|m| m.get("content"));
        match (role, content) {
            (Some("assistant"), Some(Value::String(s))) => out.push(line("text", truncate(s, 400))),
            (Some("assistant"), Some(Value::Array(items))) => {
                for it in items {
                    match it.get("type").and_then(Value::as_str) {
                        Some("text") => {
                            let t = it.get("text").and_then(Value::as_str).unwrap_or_default();
                            if !t.trim().is_empty() {
                                out.push(line("text", truncate(t, 400)));
                            }
                        }
                        Some("tool_use") => {
                            let name = it.get("name").and_then(Value::as_str).unwrap_or("tool");
                            let input = it.get("input");
                            let arg = ["description", "command", "file_path", "pattern", "prompt", "query"]
                                .iter()
                                .find_map(|k| input?.get(*k)?.as_str())
                                .unwrap_or_default();
                            out.push(line("tool", truncate(&format!("{name}: {arg}"), 200)));
                        }
                        _ => {}
                    }
                }
            }
            (Some("user"), Some(Value::Array(items))) => {
                for it in
                    items.iter().filter(|i| i.get("type").and_then(Value::as_str) == Some("tool_result"))
                {
                    let body = match it.get("content") {
                        Some(Value::String(s)) => s.clone(),
                        Some(Value::Array(parts)) => parts
                            .iter()
                            .filter_map(|p| p.get("text")?.as_str())
                            .collect::<Vec<_>>()
                            .join("\n"),
                        _ => String::new(),
                    };
                    let error = it.get("is_error").and_then(Value::as_bool).unwrap_or(false);
                    out.push(line(if error { "error" } else { "result" }, truncate(&body, 200)));
                }
            }
            _ => {}
        }
    }
    let skip = out.len().saturating_sub(TAIL_LINES);
    out.split_off(skip)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sess() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/claude-runs/sess")
    }

    fn summary(run: &ClaudeRun) -> Vec<(String, Vec<(String, bool)>)> {
        run.phases
            .iter()
            .map(|p| (p.title.clone(), p.agents.iter().map(|a| (a.label.clone(), a.done)).collect()))
            .collect()
    }

    #[test]
    fn running_run_from_journal_and_metas_skips_bad_lines() {
        let sid = SessionId::new("s1");
        let stopped = BTreeSet::from(["a2".to_owned()]);
        let run = read_run(&sess(), "wf_aaaa1111-222", &sid, &stopped);
        assert_eq!(run.name.as_deref(), Some("kelta-ticket-pass"));
        assert_eq!(run.status, "running");
        assert_eq!(run.result, None);
        let s = |l: &str, d| (l.to_owned(), d);
        assert_eq!(
            summary(&run),
            vec![
                ("Implement".to_owned(), vec![s("implement-#202", true)]),
                // a2: SubagentStop came before its journal result; a3: no label → its id.
                ("Review".to_owned(), vec![s("review-#202-r1", true), s("a3", false)]),
                // a8: broken meta; a9: only a meta, unknown fields ignored.
                ("Other".to_owned(), vec![s("a8", false)]),
                ("Gate".to_owned(), vec![s("gate-#202", false)]),
            ]
        );
    }

    #[test]
    fn finished_run_reads_the_final_record() {
        let run = read_run(&sess(), "wf_bbbb3333-444", &SessionId::new("s1"), &BTreeSet::new());
        assert_eq!(run.status, "killed");
        assert_eq!(
            (run.duration_ms, run.total_tokens, run.total_tool_calls),
            (Some(61000), Some(1234), Some(7))
        );
        assert_eq!(run.logs, ["Picked #7"]);
        assert!(run.result.as_deref().unwrap().contains("\"blocked\""));
        let titles: Vec<_> = run.phases.iter().map(|p| p.title.as_str()).collect();
        assert_eq!(titles, ["Pick", "Implement"]);
        assert_eq!(summary(&run)[1].1, [("implement-#7".to_owned(), false)]);
    }

    #[test]
    fn missing_run_is_an_empty_running_run() {
        let run = read_run(Path::new("/nonexistent"), "wf_x", &SessionId::new("s1"), &BTreeSet::new());
        assert_eq!((run.status.as_str(), run.name, run.phases.len()), ("running", None, 0));
    }

    #[test]
    fn tail_keeps_prose_tools_and_results() {
        let path = sess().join("subagents/workflows/wf_aaaa1111-222/agent-a1.jsonl");
        let tail = transcript_tail(&read_tail(&path).unwrap());
        let got: Vec<_> = tail.iter().map(|l| (l.kind.as_str(), l.text.as_str())).collect();
        assert_eq!(
            got,
            [
                ("text", "Reading the hooks module."),
                ("tool", "Bash: Skeleton of hooks.rs"),
                ("result", "graft skeleton (redacted output)"),
                ("tool", "Read: /w/crates/kelta-core/src/lib.rs"),
                ("error", "File does not exist."),
                ("text", "Implemented and committed."),
            ]
        );
    }

    #[test]
    fn hooks_name_their_run() {
        let mut p = HookPayload {
            hook_event_name: names::POST_TOOL_USE.into(),
            tool_name: Some("Workflow".into()),
            ..Default::default()
        };
        p.extra.insert(
            "tool_response".into(),
            serde_json::json!({"text": "Started run wf_00c72039-31f (task w2)"}),
        );
        assert_eq!(run_of(&sess(), &p).as_deref(), Some("wf_00c72039-31f"));
        p.tool_name = Some("Edit".into());
        assert_eq!(run_of(&sess(), &p), None);

        let mut p = HookPayload { hook_event_name: names::SUBAGENT_START.into(), ..Default::default() };
        p.extra.insert("agent_id".into(), "a9".into());
        assert_eq!(run_of(&sess(), &p).as_deref(), Some("wf_aaaa1111-222"));
        p.extra.insert("agent_id".into(), "../x".into());
        assert_eq!(run_of(&sess(), &p), None);
        p.hook_event_name = names::SUBAGENT_STOP.into();
        p.extra.insert(
            "agent_transcript_path".into(),
            "/c/sess/subagents/workflows/wf_cc-1/agent-z.jsonl".into(),
        );
        assert_eq!(run_of(&sess(), &p).as_deref(), Some("wf_cc-1"));
        // A plain Task subagent is not a run.
        p.extra.insert("agent_transcript_path".into(), "/c/sess/subagents/agent-z.jsonl".into());
        assert_eq!(run_of(&sess(), &p), None);
    }

    #[test]
    fn ids_never_escape_the_run_dir() {
        assert!(safe_id("wf_0bce8ca-123") && safe_id("a46a2c3f2853"));
        assert!(!safe_id("..") && !safe_id("a/b") && !safe_id("") && !safe_id("a.b"));
        assert_eq!(find_run_id("no run"), None);
    }
}

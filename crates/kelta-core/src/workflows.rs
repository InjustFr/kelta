//! Saved Claude Code workflows (#233): `*.js` files in `<primary repo>/.claude/workflows/` or
//! `~/.claude/workflows/`, and a run of one on a ticket in a background Claude session.

use std::collections::BTreeMap;
use std::path::Path;

use kelta_proto::error::KeltaError;
use kelta_proto::ids::ProjectId;
use kelta_proto::model::{
    OpenPaneRequest, PaneContent, Placement, RestorePolicy, RunClaim, SessionInfo, SessionKind, SpawnRequest,
    WorkflowInfo, WorkflowSource,
};
use kelta_proto::tracker::TicketRef;
use kelta_work::template::{Ctx, Mode, render};

use crate::Core;
use crate::templates::write_private;

impl Core {
    /// `workflows_list`: the workflows a run on `ticket` can use (its project as start work picks it).
    pub async fn workflows_list(&self, ticket: &TicketRef) -> Result<Vec<WorkflowInfo>, KeltaError> {
        let project = self.ticket_project(ticket).await?;
        Ok(self.workflows_in(&project))
    }

    /// Repo workflows first: a home one with the same name is shadowed.
    fn workflows_in(&self, project: &ProjectId) -> Vec<WorkflowInfo> {
        let home = self.home_dir();
        let repo = self.project_cwd(project, None);
        let mut out = Vec::new();
        if repo != home {
            scan(&repo.join(".claude/workflows"), WorkflowSource::Repo, &mut out);
        }
        scan(&home.join(".claude/workflows"), WorkflowSource::Home, &mut out);
        out
    }

    /// `workflow_run`: claims the ticket for the run, then starts Claude in the project's primary repo
    /// in a new tab that does not take focus, told to run `workflow` with `args.json`.
    pub async fn workflow_run(&self, ticket: TicketRef, workflow: &str) -> Result<SessionInfo, KeltaError> {
        self.rt.capture();
        let project = self.ticket_project(&ticket).await?;
        let wf = self
            .workflows_in(&project)
            .into_iter()
            .find(|w| w.name == workflow)
            .ok_or_else(|| KeltaError::not_found(format!("workflow {workflow}")))?;
        let sid = self.fresh_session_id();
        let run_id = format!("{}-{}", wf.name, sid.sid8());
        let run = RunClaim {
            name: Some(wf.name.clone()),
            run_id: Some(run_id.clone()),
            session_id: Some(sid.clone()),
            phase: Some("Starting".into()),
            ..RunClaim::default()
        };
        self.work.claim(&project, ticket.clone(), None, None, run).await?;
        let title = format!("{} · {}", wf.name, ticket.key);
        let info = match self.launch(&project, &ticket, &wf.name, &run_id, sid, &title).await {
            Ok(i) => i,
            Err(e) => {
                // Adopted worktrees are never removed: this only ends the claim.
                let _ = self.work.release(&ticket.key, None).await;
                return Err(e);
            }
        };
        self.session_link(&info.id, None, Some(ticket))?;
        let content = PaneContent::Terminal { session_id: info.id.clone() };
        let req = OpenPaneRequest {
            content,
            placement: Placement::NewTab,
            focus: false,
            tab_title: Some(title),
            work_item_id: None,
        };
        self.open_pane(&project, req)?;
        Ok(info)
    }

    /// Writes `ticket.md` and `args.json` to the session's private run dir, then spawns Claude.
    async fn launch(
        &self,
        project: &ProjectId,
        ticket: &TicketRef,
        workflow: &str,
        run_id: &str,
        sid: kelta_proto::ids::SessionId,
        title: &str,
    ) -> Result<SessionInfo, KeltaError> {
        let detail = self.tracker_get(ticket).await?;
        let dir = self.dirs.session_runtime(&sid.sid8());
        let (file, args) = (dir.join("ticket.md"), dir.join("args.json"));
        write_private(&file, &kelta_work::files::ticket_markdown(&detail))?;
        let t = &detail.ticket;
        let json = serde_json::json!({
            "ticket": { "key": ticket.key, "title": t.title, "url": t.url, "file": file },
            "run": run_id,
        });
        write_private(&args, &serde_json::to_string_pretty(&json)?)?;
        let s = self.cfg.effective(Some(project));
        let vars = Ctx::new()
            .with("workflow", workflow)
            .with("ticket.key", ticket.key.clone())
            .with("ticket.title", t.title.clone())
            .with("ticket.url", t.url.clone())
            .with("file", file.to_string_lossy())
            .with("args", args.to_string_lossy());
        let prompt = match s.claude.prompt_templates.get("workflow") {
            Some(tpl) => Some(render(tpl, &vars, Mode::Lenient)?),
            None => None,
        };
        let base = SpawnRequest {
            id: None,
            project_id: project.clone(),
            kind: SessionKind::Claude,
            name: Some(title.to_owned()),
            program: None,
            args: Vec::new(),
            cwd: Some(self.project_cwd(project, None)),
            env: BTreeMap::new(),
            cols: 80,
            rows: 24,
            work_item_id: None,
            restore: RestorePolicy::None,
            close_on_exit: Default::default(),
            template_id: None,
        };
        self.spawn_claude(sid, base, &s, None, prompt.as_deref()).await
    }
}

/// Appends the workflows of `dir` (sorted by file name) whose names are not listed yet.
fn scan(dir: &Path, source: WorkflowSource, out: &mut Vec<WorkflowInfo>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    let mut paths: Vec<_> =
        entries.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "js")).collect();
    paths.sort();
    for path in paths {
        let Some((name, description)) = std::fs::read_to_string(&path).ok().as_deref().and_then(parse_meta)
        else {
            continue;
        };
        if !out.iter().any(|w| w.name == name) {
            out.push(WorkflowInfo { name, description, path, source });
        }
    }
}

/// `name` and `description` of the `export const meta = { … }` literal, read without running the JS:
/// the first `name:` / `description:` string (`'`, `"` or a backtick string) at its top level.
/// `None` without a `name`.
/// shortcut: comments and `${…}` in the literal are read as plain text; a JS parser would lift this.
pub fn parse_meta(src: &str) -> Option<(String, String)> {
    let at = src.find("export const meta")?;
    let body = &src[at..];
    let mut chars = body[body.find('{')? + 1..].chars();
    let (mut name, mut description) = (None, None);
    let mut depth = 0usize;
    // The key just read at depth 0, and the key whose value comes next (after its `:`).
    let mut key = String::new();
    let mut want: Option<String> = None;
    while let Some(c) = chars.next() {
        match c {
            '\'' | '"' | '`' => {
                let s = string(&mut chars, c);
                if depth == 0 {
                    match want.take().as_deref() {
                        Some("name") if name.is_none() => name = Some(s),
                        Some("description") if description.is_none() => description = Some(s),
                        Some(_) => {}
                        None => key = s, // a quoted key
                    }
                }
            }
            '{' | '[' | '(' => {
                depth += 1;
                want = None;
            }
            '}' | ']' | ')' => {
                if depth == 0 {
                    break;
                }
                depth -= 1;
            }
            ':' if depth == 0 => want = Some(std::mem::take(&mut key)),
            c if c.is_whitespace() => {}
            c if c.is_alphanumeric() || c == '_' || c == '$' => {
                want = None;
                key.push(c);
            }
            _ => {
                key.clear();
                want = None;
            }
        }
    }
    Some((name?, description.unwrap_or_default()))
}

/// The rest of a string literal opened by `quote` (escapes keep the escaped character).
fn string(chars: &mut std::str::Chars<'_>, quote: char) -> String {
    let mut s = String::new();
    while let Some(c) = chars.next() {
        match c {
            '\\' => s.extend(chars.next()),
            c if c == quote => break,
            c => s.push(c),
        }
    }
    s
}

#[cfg(test)]
mod tests {
    use super::parse_meta;

    fn meta(body: &str) -> Option<(String, String)> {
        parse_meta(&format!(
            "import x from 'y';\nexport const meta = {body};\nexport default async () => {{}};"
        ))
    }

    #[test]
    fn quotes_single_double_and_backtick() {
        let m = |n: &str, d: &str| Some((n.to_owned(), d.to_owned()));
        assert_eq!(meta("{ name: 'a', description: 'b' }"), m("a", "b"));
        assert_eq!(meta("{ name: \"a\", description: \"it's\" }"), m("a", "it's"));
        assert_eq!(meta("{\n  name: `a`,\n  description: `multi\nline`,\n}"), m("a", "multi\nline"));
        assert_eq!(meta("{ 'name': 'q', \"description\": 'd' }"), m("q", "d"));
        assert_eq!(meta(r#"{ name: 'a\'b', description: "x\"y" }"#), m("a'b", "x\"y"));
    }

    #[test]
    fn missing_name_is_skipped() {
        assert_eq!(meta("{ description: 'no name' }"), None);
        assert_eq!(meta("{ title: 'name' }"), None);
        assert_eq!(parse_meta("export default { name: 'x' }"), None);
        assert_eq!(meta("{ name: 'only' }"), Some(("only".into(), String::new())));
    }

    #[test]
    fn braces_and_commas_in_strings_and_phases_after() {
        let src = "{\n  name: 'kelta-ticket-pass',\n  description: 'Pick {one}, implement: name: x, then merge',\n  \
                   phases: [{ name: 'Pick', description: 'nested' }, { name: 'Merge' }],\n}";
        assert_eq!(
            meta(src),
            Some(("kelta-ticket-pass".into(), "Pick {one}, implement: name: x, then merge".into()))
        );
        // A nested `name` before the top-level one is not it.
        assert_eq!(
            meta("{ phases: [{ name: 'Pick' }], name: 'real', description: 'd' }"),
            Some(("real".into(), "d".into()))
        );
    }
}

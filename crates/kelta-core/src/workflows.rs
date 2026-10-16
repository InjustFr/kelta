//! Workflows run in a background Claude session: saved Claude Code workflows (#233), `*.js` files in
//! `<primary repo>/.claude/workflows/` or `~/.claude/workflows/`, run on a ticket; and `[[workflows]]`
//! prompts or slash commands of the config (#355), run on a ticket, a pull request or with none.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use kelta_proto::codehost::ReviewRef;
use kelta_proto::error::KeltaError;
use kelta_proto::ext::{WorkflowDef, WorkflowOn};
use kelta_proto::ids::{ProjectId, SessionId};
use kelta_proto::model::{
    Lifecycle, OpenPaneRequest, PaneContent, Placement, RestorePolicy, RunClaim, RunState, SessionInfo,
    SessionKind, SpawnRequest, WorkItem, WorkState, WorkflowInfo, WorkflowSource, WorkflowStart,
};
use kelta_proto::settings::Settings;
use kelta_proto::tracker::TicketRef;
use kelta_work::template::{Ctx, Mode, render};

use crate::Core;
use crate::projects::repo_path;
use crate::templates::write_private;

impl Core {
    /// `workflows_list`: the workflows a run on `ticket` can use (its project as start work picks it).
    pub async fn workflows_list(&self, ticket: &TicketRef) -> Result<Vec<WorkflowInfo>, KeltaError> {
        let project = self.ticket_project(ticket).await?;
        Ok(self.workflows_in(&project))
    }

    /// Repo workflows first: a home one with the same name is shadowed, then a config one.
    fn workflows_in(&self, project: &ProjectId) -> Vec<WorkflowInfo> {
        let home = self.home_dir();
        let repo = self.project_cwd(project, None);
        let mut out = Vec::new();
        if repo != home {
            scan(&repo.join(".claude/workflows"), WorkflowSource::Repo, &mut out);
        }
        scan(&home.join(".claude/workflows"), WorkflowSource::Home, &mut out);
        for d in self.workflow_defs(project, WorkflowOn::Ticket) {
            if !out.iter().any(|w| w.name == d.id) {
                let label = if d.label.is_empty() { d.id.clone() } else { d.label };
                let description = d.description.unwrap_or_default();
                out.push(WorkflowInfo {
                    name: d.id,
                    label,
                    description,
                    path: None,
                    source: WorkflowSource::Config,
                });
            }
        }
        out
    }

    /// The enabled `[[workflows]]` offered `on`.
    fn workflow_defs(&self, project: &ProjectId, on: WorkflowOn) -> Vec<WorkflowDef> {
        let s = self.cfg.effective(Some(project));
        s.workflows.iter().filter(|d| d.enabled && d.on == on).cloned().collect()
    }

    /// `workflow_run`: claims the ticket for the run, then starts Claude in the project's primary repo
    /// in a new tab that does not take focus, told to run `workflow` with `args.json` (a `*.js` one) or
    /// given the config prompt. A run already holding the ticket is returned instead (`running`).
    pub async fn workflow_run(&self, ticket: TicketRef, workflow: &str) -> Result<WorkflowStart, KeltaError> {
        self.rt.capture();
        let project = self.ticket_project(&ticket).await?;
        let wf = self
            .workflows_in(&project)
            .into_iter()
            .find(|w| w.name == workflow)
            .ok_or_else(|| KeltaError::not_found(format!("workflow {workflow}")))?;
        if let Some(session) = self.ticket_run(&ticket).await? {
            return Ok(WorkflowStart { session, running: true });
        }
        let def = match wf.source {
            WorkflowSource::Config => {
                self.workflow_defs(&project, WorkflowOn::Ticket).into_iter().find(|d| d.id == wf.name)
            }
            _ => None,
        };
        let sid = self.fresh_session_id();
        let run_id = format!("{}-{}", wf.name, sid.sid8());
        let run = RunClaim {
            name: Some(wf.name.clone()),
            run_id: Some(run_id.clone()),
            session_id: Some(sid.clone()),
            // A prompt reports no phases.
            phase: def.is_none().then(|| "Starting".into()),
            ..RunClaim::default()
        };
        let item = self.work.claim(&project, ticket.clone(), None, None, run).await?;
        let owner = sid.clone();
        let title = format!("{} · {}", wf.label, ticket.key);
        let info = match self.launch(&project, &ticket, &wf.name, def.as_ref(), &run_id, sid, &title).await {
            Ok(i) => i,
            Err(e) => {
                // Adopted worktrees are never removed: this only ends the claim.
                let _ = self.work.release(&ticket.key, None, Some(&owner)).await;
                return Err(e);
            }
        };
        self.session_link(&info.id, None, Some(ticket))?;
        // #491: the claimed item adopts the run's Claude; opening the item moves it into the item's tab.
        self.adopt_session(&item, &info).await;
        self.background_tab(&project, &info, title)?;
        Ok(WorkflowStart { session: info, running: false })
    }

    /// `workflow_run_on`: the `[[workflows]]` entry `workflow` of `project`, `on = "pr"` on `review` or
    /// `on = "global"` without one, in a background Claude tab. A run already on the PR is returned
    /// instead (`running`).
    pub async fn workflow_run_on(
        &self,
        project: ProjectId,
        workflow: &str,
        review: Option<ReviewRef>,
    ) -> Result<WorkflowStart, KeltaError> {
        self.rt.capture();
        let on = if review.is_some() { WorkflowOn::Pr } else { WorkflowOn::Global };
        let def = self
            .workflow_defs(&project, on)
            .into_iter()
            .find(|d| d.id == workflow)
            .ok_or_else(|| KeltaError::not_found(format!("workflow {workflow}")))?;
        let mut title = if def.label.is_empty() { def.id.clone() } else { def.label.clone() };
        let mut vars = Ctx::new().with("workflow", def.id.clone());
        let mut cwd = self.project_cwd(&project, None);
        let mut key = None;
        if let Some(r) = &review {
            let k = format!("{}:{}#{}", r.account, r.repo, r.number);
            let running = self.pr_runs.lock().get(&k).cloned();
            if let Some(session) = running.and_then(|sid| self.live_session(&sid)) {
                return Ok(WorkflowStart { session, running: true });
            }
            let pr = self.review_get(r).await?.review;
            cwd = self.pr_cwd(r, &pr.url).await.unwrap_or(cwd);
            title = format!("{title} · {}#{}", r.repo, r.number);
            vars = vars
                .with("pr.number", r.number.to_string())
                .with("pr.url", pr.url)
                .with("pr.head", pr.source_branch)
                .with("pr.base", pr.target_branch)
                .with("pr.repo", r.repo.clone())
                .with("pr.title", pr.title)
                .with("ticket.key", pr.linked_tickets.into_iter().next().unwrap_or_default());
            key = Some(k);
        }
        if let Some(c) = def.cwd.as_deref() {
            cwd = repo_path(c, &self.home_dir());
        }
        let prompt = render(&def.prompt, &vars, Mode::Lenient)?;
        let s = self.cfg.effective(Some(&project));
        let info = self.spawn_run(&project, self.fresh_session_id(), &title, cwd, Some(&prompt), &s).await?;
        // shortcut: in memory, so a run started before a restart is not found; persist when that matters.
        if let Some(k) = key {
            self.pr_runs.lock().insert(k, info.id.clone());
        }
        self.background_tab(&project, &info, title)?;
        Ok(WorkflowStart { session: info, running: false })
    }

    fn live_session(&self, sid: &SessionId) -> Option<SessionInfo> {
        self.sessions.lock().get(sid).map(|e| e.info.clone()).filter(|s| s.lifecycle == Lifecycle::Live)
    }

    /// #491/#494: `item` adopts the Claude `info` (`WorkService::adopt_run`) and the session links back to
    /// it (`SessionInfo.work_item_id` and its ticket: sidebar and Background rows, `open_in_editor`,
    /// `get_ticket`). `None` when the adoption failed (logged).
    async fn adopt_session(&self, item: &WorkItem, info: &SessionInfo) -> Option<WorkItem> {
        match self.work.adopt_run(&item.id, info).await {
            Ok(w) => {
                if let Err(e) = self.session_link(&info.id, Some(w.id.clone()), w.ticket.clone()) {
                    tracing::warn!(work = %w.id, error = %e.message, "adopted Claude not linked");
                }
                Some(w)
            }
            Err(e) => {
                tracing::warn!(work = %item.id, error = %e.message, "run's Claude not adopted");
                None
            }
        }
    }

    /// #494: the claiming session becomes the claimed item's Claude when it is a live Kelta Claude in no
    /// unfinished work item, whatever started it (a run, a trigger, a plain tab). Read from the store, so
    /// it holds after a restart; one session, at most one live item.
    pub(crate) async fn adopt_claiming_session(&self, item: WorkItem) -> WorkItem {
        let Some(info) = item
            .run
            .as_ref()
            .and_then(|r| r.session_id.as_ref())
            .and_then(|s| self.live_session(s))
            .filter(|s| s.kind == SessionKind::Claude)
        else {
            return item;
        };
        if self.work.for_session(&info.id).await.is_some_and(|w| w.state != WorkState::Finished) {
            return item;
        }
        self.adopt_session(&item, &info).await.unwrap_or(item)
    }

    /// The live session of a run holding `ticket`.
    async fn ticket_run(&self, ticket: &TicketRef) -> Result<Option<SessionInfo>, KeltaError> {
        let items = self.work.list(None).await?;
        Ok(items
            .iter()
            .filter(|w| w.state != WorkState::Finished)
            .filter(|w| w.ticket.as_ref().is_some_and(|t| t.account == ticket.account && t.key == ticket.key))
            .find_map(|w| {
                let r =
                    w.run.as_ref().filter(|r| matches!(r.state, RunState::Running | RunState::Blocked))?;
                self.live_session(r.session_id.as_ref()?)
            }))
    }

    /// The worktree of the PR's work item, else the repo bound to its code host repo.
    async fn pr_cwd(&self, r: &ReviewRef, url: &str) -> Option<PathBuf> {
        let items = self.work.list(None).await.unwrap_or_default();
        let item = items.iter().find(|w| {
            w.state != WorkState::Finished
                && (w.review.as_ref() == Some(r) || w.pr_url.as_deref() == Some(url))
        });
        if let Some(w) = item {
            return Some(w.worktree.clone());
        }
        let home = self.home_dir();
        let projects = self.cfg.projects();
        let repo = projects.iter().flat_map(|p| p.repos.iter()).find(|repo| {
            repo.code_host.as_ref().is_some_and(|c| c.account == r.account && c.repo == r.repo)
        })?;
        Some(repo_path(&repo.path, &home)).filter(|p| p.is_dir())
    }

    /// Writes `ticket.md` and `args.json` to the session's private run dir, then spawns Claude.
    #[allow(clippy::too_many_arguments)]
    async fn launch(
        &self,
        project: &ProjectId,
        ticket: &TicketRef,
        workflow: &str,
        def: Option<&WorkflowDef>,
        run_id: &str,
        sid: SessionId,
        title: &str,
    ) -> Result<SessionInfo, KeltaError> {
        let detail = self.tracker_get(ticket).await?;
        let dir = self.dirs.session_runtime(&sid.sid8());
        let (file, args) = (dir.join("ticket.md"), dir.join("args.json"));
        write_private(
            &file,
            &kelta_work::files::ticket_markdown(&detail, kelta_work::files::BRIEF_COMMENTS),
        )?;
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
        let tpl = match def {
            Some(d) => Some(&d.prompt),
            None => s.claude.prompt_templates.get("workflow"),
        };
        let prompt = match tpl {
            Some(tpl) => Some(render(tpl, &vars, Mode::Lenient)?),
            None => None,
        };
        let cwd = match def.and_then(|d| d.cwd.as_deref()) {
            Some(c) => repo_path(c, &self.home_dir()),
            None => self.project_cwd(project, None),
        };
        self.spawn_run(project, sid, title, cwd, prompt.as_deref(), &s).await
    }

    async fn spawn_run(
        &self,
        project: &ProjectId,
        sid: SessionId,
        title: &str,
        cwd: PathBuf,
        prompt: Option<&str>,
        s: &Settings,
    ) -> Result<SessionInfo, KeltaError> {
        let base = SpawnRequest {
            id: None,
            project_id: project.clone(),
            kind: SessionKind::Claude,
            name: Some(title.to_owned()),
            program: None,
            args: Vec::new(),
            cwd: Some(cwd),
            env: BTreeMap::new(),
            cols: 80,
            rows: 24,
            work_item_id: None,
            restore: RestorePolicy::None,
            close_on_exit: Default::default(),
            template_id: None,
        };
        self.spawn_claude(sid, base, s, None, prompt).await
    }

    /// The run's session in a new tab that does not take focus.
    fn background_tab(
        &self,
        project: &ProjectId,
        info: &SessionInfo,
        title: String,
    ) -> Result<(), KeltaError> {
        let req = OpenPaneRequest {
            content: PaneContent::Terminal { session_id: info.id.clone() },
            placement: Placement::NewTab,
            focus: false,
            tab_title: Some(title),
            work_item_id: None,
        };
        self.open_pane(project, req).map(|_| ())
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
            out.push(WorkflowInfo { label: name.clone(), name, description, path: Some(path), source });
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

//! Tickets T10: refine a ticket with a one-shot `claude -p` (acceptance criteria, open questions,
//! optional sub-tasks). The proposal is returned as Markdown; nothing is written to the tracker.

use std::collections::BTreeMap;
use std::time::Duration;

use kelta_proto::error::KeltaError;
use kelta_proto::ids::{ProjectId, SessionId};
use kelta_proto::tracker::TicketRef;

use crate::{Core, spawn_env};

const PROMPT: &str = "Refine this tracker ticket before work starts. The ticket (description and recent \
comments) is on stdin; the Kelta MCP tool `get_ticket` returns it too. You may read the repository to ground \
your answer, but do not change anything. Reply with Markdown only, exactly these three sections:\n\n\
## Acceptance criteria\n(a `- [ ]` checklist of testable criteria)\n\n\
## Open questions\n(a list, or `None`)\n\n\
## Sub-tasks\n(an optional split into a few sub-tasks as a list, or `None` when the ticket is small)";

/// One-shot: a refine that has not answered by then is killed.
const TIMEOUT: Duration = Duration::from_secs(300);

impl Core {
    /// `tracker_refine`: runs Claude in the project's primary repo with its Claude settings.
    pub async fn tracker_refine(
        &self,
        t: &TicketRef,
        project: Option<&ProjectId>,
    ) -> Result<String, KeltaError> {
        self.rt.capture();
        let detail = self.tracker_get(t).await?;
        let settings = self.cfg.effective(project);
        let cwd = project.map_or_else(|| self.home_dir(), |id| self.project_cwd(id, None));
        let claude = &settings.claude;
        let binary = spawn_env::resolve_program(&claude.binary, self.login_env.path(), &cwd)?;
        let profile = claude.profiles.get("default").cloned().unwrap_or_default();

        let mut args = vec!["-p".to_owned()];
        if !profile.model.is_empty() {
            args.extend(["--model".to_owned(), profile.model.clone()]);
        }
        args.extend(["--effort".to_owned(), kelta_work::claude::enum_str(&profile.effort)]);
        args.extend(["--disallowedTools", "Edit,Write,NotebookEdit"].map(str::to_owned));

        // MCP `get_ticket` for this run: a throwaway sid the MCP route resolves to the ticket.
        let sid = SessionId::new(format!("refine-{}", spawn_env::token()));
        let port = if claude.mcp { self.server.ensure_http().await.ok() } else { None };
        let mut mcp_env = BTreeMap::new();
        if let Some(port) = port {
            let token = spawn_env::token();
            self.server.register_session(&sid, &spawn_env::token(), Some(&token));
            self.refines.lock().insert(sid.clone(), t.clone());
            let mcp = kelta_work::claude::mcp_json(port, sid.as_str());
            args.extend(["--mcp-config".to_owned(), mcp.to_string()]);
            args.extend(["--allowedTools", "mcp__kelta__get_ticket"].map(str::to_owned));
            mcp_env.insert("KELTA_MCP_TOKEN".to_owned(), token);
            mcp_env.insert("KELTA_MCP_URL".to_owned(), format!("http://127.0.0.1:{port}/mcp/{sid}"));
        }
        args.extend(["--".to_owned(), PROMPT.to_owned()]);

        let env = self.login_env.vars.iter().chain(&settings.env).chain(&mcp_env);
        let input = kelta_work::files::ticket_markdown(&detail);
        let out = crate::ctl::claude_oneshot(&binary, args, env, &cwd, Some(input.as_bytes()), TIMEOUT).await;
        if port.is_some() {
            self.refines.lock().remove(&sid);
            self.server.unregister_session(&sid);
            self.server.release_http();
        }
        let out = out?;
        let text = String::from_utf8_lossy(&out.stdout).trim().to_owned();
        if !out.status.success() || text.is_empty() {
            let err = String::from_utf8_lossy(&out.stderr);
            let tail = err.trim().lines().last().unwrap_or("no output");
            return Err(KeltaError::internal(format!("Claude could not refine {}: {tail}", t.key)));
        }
        Ok(text)
    }
}

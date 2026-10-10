//! Tickets T10: refine a ticket with a one-shot `claude -p` (acceptance criteria, open questions,
//! optional sub-tasks). The proposal is returned as Markdown; nothing is written to the tracker.

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
        let detail = self.tracker_of(&t.account)?.get(t).await?;
        let settings = self.cfg.effective(project);
        let cwd = project
            .and_then(|id| self.project_info(id))
            .and_then(|p| p.repos.iter().find(|r| r.primary).or(p.repos.first()).map(|r| r.path.clone()))
            .filter(|p| p.is_dir())
            .unwrap_or_else(|| self.home_dir());
        let claude = &settings.claude;
        let binary = spawn_env::resolve_program(&claude.binary, self.login_env.path(), &cwd)?;
        let profile = claude.profiles.get("default").cloned().unwrap_or_default();

        let mut cmd = tokio::process::Command::new(binary);
        cmd.arg("-p");
        if !profile.model.is_empty() {
            cmd.args(["--model", &profile.model]);
        }
        cmd.args(["--effort", &kelta_work::claude::enum_str(&profile.effort)])
            .args(["--disallowedTools", "Edit,Write,NotebookEdit"])
            .current_dir(&cwd)
            .env_clear()
            .envs(self.login_env.vars.iter())
            .envs(settings.env.iter())
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .kill_on_drop(true);

        // MCP `get_ticket` for this run: a throwaway sid the MCP route resolves to the ticket.
        let sid = SessionId::new(format!("refine-{}", spawn_env::token()));
        let port = if claude.mcp { self.server.ensure_http().await.ok() } else { None };
        if let Some(port) = port {
            let token = spawn_env::token();
            self.server.register_session(&sid, &spawn_env::token(), Some(&token));
            self.refines.lock().insert(sid.clone(), t.clone());
            let mcp = kelta_work::claude::mcp_json(port, sid.as_str());
            cmd.args(["--mcp-config", &mcp.to_string(), "--allowedTools", "mcp__kelta__get_ticket"])
                .env("KELTA_MCP_TOKEN", token)
                .env("KELTA_MCP_URL", format!("http://127.0.0.1:{port}/mcp/{sid}"));
        }
        cmd.args(["--", PROMPT]);

        let out = run(cmd, kelta_work::files::ticket_markdown(&detail)).await;
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

async fn run(mut cmd: tokio::process::Command, input: String) -> Result<std::process::Output, KeltaError> {
    use tokio::io::AsyncWriteExt;
    let io = |e: std::io::Error| KeltaError::internal(format!("claude: {e}"));
    let mut child = cmd.spawn().map_err(io)?;
    if let Some(mut stdin) = child.stdin.take() {
        // A Claude that exits before reading its stdin is reported by its exit status, not here.
        let _ = stdin.write_all(input.as_bytes()).await;
    }
    tokio::time::timeout(TIMEOUT, child.wait_with_output())
        .await
        .map_err(|_| KeltaError::timeout("Claude did not answer within 5 minutes"))?
        .map_err(io)
}

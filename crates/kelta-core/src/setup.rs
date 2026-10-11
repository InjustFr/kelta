//! "Set up with Claude" (#343): a Claude session in Home that configures Kelta through the MCP setup
//! tools (`get_setup`, `add_project`, `set_setting`, `test_account`, `request_token`).

use std::collections::BTreeMap;

use kelta_proto::api::CoreApi;
use kelta_proto::error::KeltaError;
use kelta_proto::ids::{AccountId, ProjectId};
use kelta_proto::ipc::AccountTestResult;
use kelta_proto::model::{
    OpenPaneRequest, PaneContent, Placement, RestorePolicy, SessionInfo, SessionKind, SpawnRequest,
};
use kelta_proto::redact::redact_text;

use crate::Core;

/// The guided script: the order of the steps, what to detect, what to ask, and the secret rule.
pub const PROMPT: &str = "Set up Kelta for me with the kelta MCP tools. Work through these steps in order, \
one at a time, and keep each question short.

1. Call get_setup to see what is configured and what is missing. Then look at this machine yourself: git \
repositories under the usual code folders (~/code, ~/src, ~/Sites, ~/projects, ~/dev, ~/work) and their \
remotes (git remote -v), the CLIs on the PATH (gh, glab, nvim, code), and the NAMES of environment variables \
that look like tokens (JIRA_*, GITHUB_TOKEN, GITLAB_TOKEN, LINEAR_*). Never print or read a token's value.
2. Code hosts: for each host the remotes use (github.com, gitlab.com, a self-hosted GitLab or Gitea, \
Bitbucket), add an account with set_setting at `accounts.<id>` (an object: kind, base_url when self-hosted, \
secret). Prefer `gh-cli` / `glab-cli` when that CLI is logged in.
3. Projects: ask which repositories I work on, then call add_project with each path. It binds the repo to \
the code host account matching its remote (added in step 2) and reports the tracker it guesses.
4. Trackers: ask which tracker holds my tickets (Jira, GitHub issues, GitLab, Linear, Redmine, Gitea), add \
its account, then a ticket source with set_setting at `sources` (the whole list: id, label, projects, \
tracker.account, tracker.views). For a Jira view, set board_id and statuses when I name a board or the \
statuses I care about.
5. Editor and terminal: ask whether I prefer another editor than the default (editor.default is a preset id \
from get_setup) or another terminal font (terminal.font_family, terminal.font_size), and set them.
6. Claude integration: check with get_setup that claude.mcp is on.
7. Finish with test_account on every account, then a short summary of what you configured, what failed and \
what is left to do.

Secrets: you never receive, read or write a token value. An account's `secret` is a reference: \
`env:VAR`, `keyring:<name>`, `file:<name>`, `command:<argv>`, `gh-cli` or `glab-cli`. When I have no \
reference to give, set `keyring:<account id>` and call request_token so I type the token in Kelta; wait \
until I say it is done before testing the account. set_setting runs Kelta's own validation: when a write is \
refused, fix the value from the message instead of retrying it.";

impl Core {
    /// `claude_setup`: registers Kelta's MCP in Claude Code when it is not yet, then starts Claude in
    /// Home with [`PROMPT`] in a new focused tab.
    pub async fn claude_setup(&self) -> Result<SessionInfo, KeltaError> {
        self.rt.capture();
        if self.claude_mcp_status().await != Some(true) {
            self.claude_mcp_install().await?;
        }
        let home = ProjectId::home();
        let s = self.cfg.effective(Some(&home));
        let base = SpawnRequest {
            id: None,
            project_id: home.clone(),
            kind: SessionKind::Claude,
            name: Some("Set up Kelta".to_owned()),
            program: None,
            args: Vec::new(),
            cwd: Some(self.home_dir()),
            env: BTreeMap::new(),
            cols: 80,
            rows: 24,
            work_item_id: None,
            restore: RestorePolicy::None,
            close_on_exit: Default::default(),
            template_id: None,
        };
        let info = self.spawn_claude(self.fresh_session_id(), base, &s, None, Some(PROMPT)).await?;
        let req = OpenPaneRequest {
            content: PaneContent::Terminal { session_id: info.id.clone() },
            placement: Placement::NewTab,
            focus: true,
            tab_title: Some("Set up Kelta".to_owned()),
            work_item_id: None,
        };
        self.open_pane(&home, req)?;
        Ok(info)
    }

    /// Resolve the account's provider (so the cached provider / secret chain is exercised as in normal
    /// use) and call `me()`.
    pub(crate) async fn test_account(&self, account_id: &AccountId) -> AccountTestResult {
        let settings = self.cfg.effective(None);
        let Some(account) = settings.accounts.get(account_id) else {
            return AccountTestResult {
                ok: false,
                user: None,
                error: Some(KeltaError::not_found(format!("unknown account `{account_id}`"))),
            };
        };
        // a token typed seconds ago must win over a cached failure
        if let Some(secret) = account.effective_secret() {
            self.secret_resolver().invalidate(&secret);
        }
        let result = if account.kind.is_code_host() {
            match self.code_host_for(account_id).await {
                Ok(host) => host.me().await,
                Err(e) => Err(e),
            }
        } else {
            match self.tracker_for(account_id).await {
                Ok(tracker) => tracker.me().await,
                Err(e) => Err(e),
            }
        };
        match result {
            Ok(user) => AccountTestResult { ok: true, user: Some(user), error: None },
            Err(mut e) => {
                e.message = redact_text(&e.message);
                AccountTestResult { ok: false, user: None, error: Some(e) }
            }
        }
    }
}

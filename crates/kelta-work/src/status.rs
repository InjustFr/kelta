//! Git status of work items (`work_status`, `work_status_all`) and the review diff (`work_diff`).

use std::collections::{BTreeMap, HashMap};
use std::path::Path;
use std::time::{Duration, Instant};

use kelta_proto::error::KeltaError;
use kelta_proto::ids::{ProjectId, WorkItemId};
use kelta_proto::model::{
    CloseOnExit, GitStatus, RestorePolicy, SessionInfo, SessionKind, SpawnRequest, WorkItem, WorkState,
};

use crate::fixloop::refuse_if_busy;
use crate::saga::{COLS, Env, ROWS};
use crate::template::{Mode, render, shell_quote};
use crate::{WorkService, editor, git, rebase};

/// `work_status_all` fetches a repo at most this often (no polling: it runs on focus and Now open).
const FETCH_FLOOR: Duration = Duration::from_secs(5 * 60);

impl WorkService {
    /// What `ahead`/`behind`/diffstat compare with: `<remote>/<base>`, never the branch's own
    /// upstream (B4); the local base when the remote ref is missing.
    async fn base_ref(env: &Env, item: &WorkItem) -> Result<Option<String>, KeltaError> {
        for r in [format!("{}/{}", env.repo.remote, item.base), item.base.clone()] {
            if git::ref_exists(&item.worktree, &r).await? {
                return Ok(Some(r));
            }
        }
        Ok(None)
    }

    /// Local git state of one item (no fetch). A deleted worktree reads as `missing`.
    pub(crate) async fn git_status(&self, env: &Env, item: &WorkItem) -> Result<GitStatus, KeltaError> {
        if !item.worktree.is_dir() {
            return Ok(GitStatus { missing: true, ..GitStatus::default() });
        }
        let report = self.dirty_report(env, item, &self.load_journal(&item.id)).await?;
        let mut st = GitStatus {
            dirty: !report.files.is_empty(),
            unpushed: report.unpushed > 0,
            ..GitStatus::default()
        };
        if let Some(base) = Self::base_ref(env, item).await? {
            (st.ahead, st.behind) = git::ahead_behind(&item.worktree, &base).await?;
            (st.files, st.insertions, st.deletions) = git::diffstat(&item.worktree, &base).await?;
        }
        let remote_ref = format!("refs/remotes/{}/{}", env.repo.remote, item.branch);
        st.diverged = rebase::diverged(&item.worktree, &remote_ref, item.rebase.as_deref()).await?;
        st.remote_new = rebase::remote_new(&item.worktree, &remote_ref, item.rebase.as_deref()).await?;
        Ok(st)
    }

    /// `work_status_all`: one `git fetch <remote>` per repo (at most every [`FETCH_FLOOR`]), then
    /// the status of every unfinished item. Items whose project or repo is gone are left out.
    pub(crate) async fn status_all_impl(&self) -> Result<BTreeMap<WorkItemId, GitStatus>, KeltaError> {
        let mut repos: HashMap<(ProjectId, String), Vec<WorkItem>> = HashMap::new();
        for w in self.store.list_items(None).await? {
            if w.state != WorkState::Finished {
                repos.entry((w.project_id.clone(), w.repo_id.clone())).or_default().push(w);
            }
        }
        let jobs = repos.into_iter().map(|((project, repo), items)| async move {
            let Ok(env) = self.env(&project, &repo) else { return Vec::new() };
            if self.fetch_due(&env.repo.path) {
                let timeout = Duration::from_secs(u64::from(env.settings.worktree.fetch_timeout_secs.max(1)));
                // The default refspec covers the bases and every PR branch in one fetch.
                if let Err(e) = git::fetch(&env.repo.path, &env.repo.remote, &[], timeout).await {
                    tracing::debug!(repo = %env.repo.path.display(), error = %e.message, "status fetch failed");
                }
                for item in items.iter().filter(|w| w.review.is_some()) {
                    if let Err(e) = self.follow_pr_head(&env, item, timeout).await {
                        tracing::debug!(work_item = %item.id, error = %e.message, "review checkout not updated");
                    }
                }
            }
            let mut out = Vec::new();
            for item in items {
                match self.git_status(&env, &item).await {
                    Ok(s) => out.push((item.id, s)),
                    Err(e) => tracing::debug!(work_item = %item.id, error = %e.message, "work status failed"),
                }
            }
            out
        });
        Ok(futures::future::join_all(jobs).await.into_iter().flatten().collect())
    }

    /// A review checkout follows the PR head (FLOW §4.7): fetch, then fast-forward only on a clean
    /// worktree. Dirty or busy items stay put; Now still shows "Updated since your review".
    async fn follow_pr_head(&self, env: &Env, item: &WorkItem, timeout: Duration) -> Result<(), KeltaError> {
        let Some(r) = &item.review else { return Ok(()) };
        let lock = self.item_lock(&item.id);
        let Ok(_guard) = lock.try_lock() else { return Ok(()) };
        // A reviewing Claude reads files without locking: moving them mid-turn skews its line comments.
        if refuse_if_busy(env, item, &self.load_journal(&item.id), "").is_err() {
            return Ok(());
        }
        if !item.worktree.is_dir() || !git::dirty_files(&item.worktree).await?.is_empty() {
            return Ok(());
        }
        let spec = env.core.code_host_for(&r.account).await?.fetch_refspec(r, &item.branch);
        let head = spec.split(':').next().unwrap_or(&spec);
        // FETCH_HEAD is per worktree, so the fetch runs in the checkout itself.
        git::fetch(&item.worktree, &env.repo.remote, &[head], timeout).await?;
        // shortcut: a force-pushed PR head does not fast-forward and stays put; Start review again to follow it.
        git::run_ok(&item.worktree, &["merge", "--ff-only", "--quiet", "FETCH_HEAD"], timeout)
            .await
            .map(|_| ())
    }

    /// Claims the fetch slot of `repo` when the floor has passed.
    fn fetch_due(&self, repo: &Path) -> bool {
        let mut fetched = self.fetched.lock();
        if fetched.get(repo).is_some_and(|t| t.elapsed() < FETCH_FLOOR) {
            return false;
        }
        fetched.insert(repo.to_path_buf(), Instant::now());
        true
    }

    /// `work_diff`: spawns the review diff of the item in its worktree (merge base to working tree,
    /// so uncommitted work shows). The editor with `editor.review_args` when set, else
    /// `git diff $(git merge-base <base> HEAD)` typed into a shell. The UI places the pane.
    pub(crate) async fn diff_impl(&self, id: &WorkItemId) -> Result<SessionInfo, KeltaError> {
        let item = self.load(id).await?;
        if !item.worktree.is_dir() {
            return Err(KeltaError::not_found(format!("worktree missing at {}", item.worktree.display())));
        }
        let env = self.env(&item.project_id, &item.repo_id)?;
        let ctx = self.item_ctx(&env, &item, &self.load_journal(id));
        let review = &env.settings.editor.review_args;
        let preset = editor::preset(&env.settings.editor, None).filter(|p| !p.external && !review.is_empty());
        let (kind, program, args) = match preset {
            Some(p) => (
                SessionKind::Editor { adapter: p.id.clone() },
                Some(p.command.clone()),
                review.iter().map(|a| render(a, &ctx, Mode::Lenient)).collect::<Result<Vec<_>, _>>()?,
            ),
            None => (SessionKind::Shell, None, Vec::new()),
        };
        let shell = program.is_none();
        let info = env
            .core
            .session_spawn(SpawnRequest {
                id: None,
                project_id: item.project_id.clone(),
                kind,
                name: Some("diff".into()),
                program,
                args,
                cwd: Some(item.worktree.clone()),
                env: BTreeMap::new(),
                cols: COLS,
                rows: ROWS,
                work_item_id: Some(item.id.clone()),
                restore: RestorePolicy::None,
                // Quitting the editor closes the pane and gives the work tab back.
                close_on_exit: if shell { CloseOnExit::Never } else { CloseOnExit::Always },
                template_id: None,
            })
            .await?;
        if shell {
            let base = Self::base_ref(&env, &item).await?.unwrap_or_else(|| item.base.clone());
            let line = format!("git diff $(git merge-base {} HEAD)\r", shell_quote(&base));
            env.core.session_write(&info.id, line.as_bytes()).await?;
        }
        Ok(info)
    }
}

//! Rebase and push of a work item (FLOW §4.4, §4.5): never drop someone else's commits.
//!
//! `RebaseState{pre_head, remote_sha}` records HEAD and the remote branch tip before a rebase.
//! Force push is allowed only for an own rewrite: `remote_sha` is in `pre_head` but not in HEAD,
//! and the lease pins the remote to `remote_sha`. Anything else on the remote is counted by
//! `GitStatus.remote_new` and gets "Rebase onto {remote}/{branch}" instead.

use std::path::Path;
use std::time::Duration;

use kelta_proto::error::{ErrorCode, KeltaError};
use kelta_proto::ids::WorkItemId;
use kelta_proto::model::{RebaseOnto, RebaseOp, RebaseState, WorkItem, WorkKind, WorkState};

use crate::fixloop::refuse_if_busy;
use crate::{WorkService, git};

/// A rebase runs hooks and may replay many commits; bounded anyway.
const REBASE_TIMEOUT: Duration = Duration::from_secs(5 * 60);

fn reason(e: KeltaError, reason: &str) -> KeltaError {
    e.with_detail(serde_json::json!({ "reason": reason }))
}

/// Re-read a recorded rebase from git: stopped → conflicts and progress; finished or aborted →
/// kept (without progress) only while the remote tip is not in HEAD, i.e. a force push is pending.
pub(crate) async fn reread(wt: &Path, st: Box<RebaseState>) -> Result<Option<Box<RebaseState>>, KeltaError> {
    if let Some((conflicts, step, total)) = git::rebase_progress(wt).await? {
        return Ok(Some(Box::new(RebaseState { conflicts, step, total, ..*st })));
    }
    let pending = match &st.remote_sha {
        Some(sha) => !git::is_ancestor(wt, sha, "HEAD").await?,
        None => false,
    };
    Ok(pending.then(|| Box::new(RebaseState { conflicts: Vec::new(), step: 0, total: 0, ..*st })))
}

/// `GitStatus.diverged`: an own rewrite of the pushed branch (FLOW §4.4 step 5), and the last
/// fetched remote tip is still `remote_sha` (otherwise someone pushed: Remote has new commits).
pub(crate) async fn diverged(
    wt: &Path,
    remote_ref: &str,
    st: Option<&RebaseState>,
) -> Result<bool, KeltaError> {
    let Some(st) = st.filter(|s| s.total == 0) else { return Ok(false) };
    let Some(sha) = &st.remote_sha else { return Ok(false) };
    Ok(git::rev(wt, remote_ref).await?.as_ref() == Some(sha)
        && git::is_ancestor(wt, sha, &st.pre_head).await?
        && !git::is_ancestor(wt, sha, "HEAD").await?)
}

/// `GitStatus.remote_new`: commits on `<remote>/<branch>` that neither HEAD nor the pre-rebase
/// HEAD contains (suggestion commits, Update branch, a teammate).
pub(crate) async fn remote_new(
    wt: &Path,
    remote_ref: &str,
    st: Option<&RebaseState>,
) -> Result<u32, KeltaError> {
    if git::rev(wt, remote_ref).await?.is_none() || st.is_some_and(|s| s.total > 0) {
        return Ok(0);
    }
    let pre = st.map(|s| format!("^{}", s.pre_head));
    let mut args = vec![remote_ref, "^HEAD"];
    args.extend(pre.as_deref());
    git::count(wt, &args).await
}

impl WorkService {
    /// Re-read the item's rebase state and persist it when git says something else.
    pub(crate) async fn refresh_rebase(&self, id: &WorkItemId) -> Result<WorkItem, KeltaError> {
        let item = self.load(id).await?;
        let Some(st) = item.rebase.clone().filter(|_| item.worktree.is_dir()) else { return Ok(item) };
        let new = reread(&item.worktree, st).await?;
        self.update(id, |w| {
            w.rebase != new && {
                w.rebase = new;
                true
            }
        })
        .await
    }

    pub(crate) async fn rebase_impl(&self, id: &WorkItemId, op: RebaseOp) -> Result<WorkItem, KeltaError> {
        let lock = self.item_lock(id);
        let _guard = lock.try_lock().map_err(|_| KeltaError::conflict("work item is busy"))?;
        let item = self.load(id).await?;
        if item.kind == WorkKind::Review {
            return Err(KeltaError::conflict("Review checkout: read-only"));
        }
        if item.state == WorkState::Finished {
            return Err(KeltaError::conflict("work item is finished"));
        }
        let env = self.env(&item.project_id, &item.repo_id)?;
        let j = self.load_journal(id);
        let wt = item.worktree.as_path();
        let in_progress = git::rebase_progress(wt).await?.is_some();
        let state = match op {
            RebaseOp::Start { onto, no_fetch } => {
                refuse_if_busy(&env, &item, &j, "Rebase")?;
                if in_progress {
                    return Err(KeltaError::conflict(
                        "A rebase is already in progress. Continue or abort it first.",
                    ));
                }
                let dirty = self.dirty_report(&env, &item, &j).await?.files;
                if !dirty.is_empty() {
                    let n = dirty.len();
                    let msg = format!(
                        "{n} uncommitted file{}. Commit or stash them first.",
                        if n == 1 { "" } else { "s" }
                    );
                    return Err(KeltaError::new(ErrorCode::Dirty, msg)
                        .with_detail(serde_json::json!({ "files": dirty })));
                }
                let remote = env.repo.remote.as_str();
                let remote_ref = format!("refs/remotes/{remote}/{}", item.branch);
                if !no_fetch {
                    let pushed = git::rev(wt, &remote_ref).await?.is_some();
                    let mut refs = vec![item.base.as_str()];
                    if pushed {
                        refs.push(&item.branch);
                    }
                    let timeout =
                        Duration::from_secs(u64::from(env.settings.worktree.fetch_timeout_secs.max(1)));
                    if let Err(e) = git::fetch(&env.repo.path, remote, &refs, timeout).await {
                        return Err(KeltaError::new(ErrorCode::Network, format!("Could not fetch {remote}/{}.", item.base))
                            .with_detail(serde_json::json!({ "reason": "fetch_failed", "error": e.message, "base": item.base })));
                    }
                }
                let head = git::rev(wt, "HEAD")
                    .await?
                    .ok_or_else(|| KeltaError::internal("HEAD does not resolve"))?;
                let remote_sha = git::rev(wt, &remote_ref).await?;
                // A second rebase before the force push keeps the HEAD that still holds the remote tip.
                let pre_head = match &item.rebase {
                    Some(prev) if prev.remote_sha == remote_sha => prev.pre_head.clone(),
                    _ => head,
                };
                let onto_ref = match onto {
                    RebaseOnto::Base => format!("{remote}/{}", item.base),
                    RebaseOnto::RemoteBranch if remote_sha.is_some() => format!("{remote}/{}", item.branch),
                    RebaseOnto::RemoteBranch => {
                        return Err(KeltaError::invalid(format!("{} was never pushed", item.branch)));
                    }
                };
                if git::rev(wt, &format!("refs/remotes/{onto_ref}")).await?.is_none() {
                    return Err(KeltaError::not_found(format!("{onto_ref} not found; fetch it first")));
                }
                // Onto the remote branch, replay only the item's own commits: the upstream is a
                // throwaway merge of base and the remote branch, so base commits (already in HEAD
                // after a rebase onto base) are excluded and already-pushed ones drop by patch-id.
                let upstream = match onto {
                    RebaseOnto::Base => onto_ref.clone(),
                    RebaseOnto::RemoteBranch => {
                        let base_ref = format!("{remote}/{}", item.base);
                        let tree = format!("{onto_ref}^{{tree}}");
                        let args = [
                            "commit-tree",
                            tree.as_str(),
                            "-p",
                            base_ref.as_str(),
                            "-p",
                            onto_ref.as_str(),
                            "-m",
                            "kelta rebase upstream",
                        ];
                        git::run_ok(wt, &args, REBASE_TIMEOUT).await?.stdout.trim().to_owned()
                    }
                };
                let out = git::run(
                    wt,
                    &["-c", "core.editor=true", "rebase", "--onto", &onto_ref, &upstream],
                    REBASE_TIMEOUT,
                )
                .await?;
                if !out.ok() && git::rebase_progress(wt).await?.is_none() {
                    return Err(KeltaError::upstream(format!(
                        "git rebase {onto_ref} failed: {}",
                        out.stderr.trim()
                    )));
                }
                Some(Box::new(RebaseState {
                    onto: onto_ref,
                    pre_head,
                    remote_sha,
                    conflicts: Vec::new(),
                    step: 0,
                    total: 0,
                }))
            }
            RebaseOp::Continue => {
                refuse_if_busy(&env, &item, &j, "Continue the rebase")?;
                if !in_progress {
                    return Err(KeltaError::conflict("No rebase is in progress."));
                }
                let out =
                    git::run(wt, &["-c", "core.editor=true", "rebase", "--continue"], REBASE_TIMEOUT).await?;
                if !out.ok() && git::rebase_progress(wt).await?.is_none() {
                    return Err(KeltaError::upstream(format!(
                        "git rebase --continue failed: {}",
                        out.stderr.trim()
                    )));
                }
                item.rebase.clone()
            }
            RebaseOp::Abort => {
                if in_progress {
                    git::run_ok(wt, &["rebase", "--abort"], REBASE_TIMEOUT).await?;
                }
                item.rebase.clone()
            }
        };
        let new = match state {
            Some(st) => reread(wt, st).await?,
            None => None,
        };
        // Write only the field this operation owns (hooks may have updated the item meanwhile).
        self.update(id, |w| {
            w.rebase = new;
            true
        })
        .await
    }

    /// `work_push`: plain push, or force push over an own rewrite only (lease on `remote_sha`).
    pub(crate) async fn push_impl(&self, id: &WorkItemId, force: bool) -> Result<WorkItem, KeltaError> {
        let lock = self.item_lock(id);
        let _guard = lock.try_lock().map_err(|_| KeltaError::conflict("work item is busy"))?;
        let mut item = self.load(id).await?;
        if item.kind == WorkKind::Review {
            return Err(KeltaError::conflict("Review checkout: read-only"));
        }
        let env = self.env(&item.project_id, &item.repo_id)?;
        let j = self.load_journal(id);
        refuse_if_busy(&env, &item, &j, if force { "Force push" } else { "Push" })?;
        let wt = item.worktree.clone();
        if git::rebase_progress(&wt).await?.is_some() {
            return Err(KeltaError::conflict(
                "A rebase is stopped in this worktree. Continue or abort it first.",
            ));
        }
        let (remote, branch) = (env.repo.remote.clone(), item.branch.clone());
        let remote_ref = format!("refs/remotes/{remote}/{branch}");
        let lease = if force {
            let st = item.rebase.clone();
            let sha = st.as_ref().and_then(|s| s.remote_sha.clone());
            match sha {
                Some(sha) if diverged(&wt, &remote_ref, st.as_deref()).await? => Some(sha),
                _ => {
                    return Err(reason(
                        KeltaError::conflict(
                            "Force push is only offered to rewrite your own rebased commits.",
                        ),
                        "not_diverged",
                    ));
                }
            }
        } else {
            None
        };
        let args: Vec<String> = match &lease {
            // `--force-if-includes` is a no-op next to an explicit lease value; kept as a second
            // guard should the lease ever be given without one.
            Some(sha) => vec![
                "push".into(),
                format!("--force-with-lease={branch}:{sha}"),
                "--force-if-includes".into(),
                remote.clone(),
                branch.clone(),
            ],
            None => vec!["push".into(), "-u".into(), remote.clone(), branch.clone()],
        };
        let code = self.push_pane(&env, &mut item, &j, args).await?;
        if code != 0 {
            // Read-only diagnosis: fetch what the remote has now, then explain the refusal.
            let timeout = Duration::from_secs(u64::from(env.settings.worktree.fetch_timeout_secs.max(1)));
            let fetched = git::fetch(&env.repo.path, &remote, &[&branch], timeout).await.is_ok();
            let tip = git::rev(&wt, &remote_ref).await?;
            return Err(match (&lease, tip) {
                (Some(sha), Some(tip)) if fetched && &tip != sha => reason(
                    KeltaError::conflict(format!(
                        "{remote}/{branch} moved since your last fetch. Someone else pushed."
                    )),
                    "lease_rejected",
                ),
                (None, Some(tip)) if fetched && !git::is_ancestor(&wt, &tip, "HEAD").await? => reason(
                    KeltaError::conflict(format!("{remote} has commits you do not have.")),
                    "non_fast_forward",
                ),
                _ => KeltaError::upstream(format!("git push failed (exit {code})")),
            });
        }
        self.add_title_key(&env, &item, None).await;
        self.update(id, |w| {
            w.rebase = None;
            true
        })
        .await
    }
}

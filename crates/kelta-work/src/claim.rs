//! Background runs claiming tickets (MCP `claim_ticket` / `report_progress` / `release_ticket`, #235).
//! A claimed item adopts the run's worktree and branch: Kelta never creates, removes or deletes them.

use std::path::PathBuf;

use kelta_proto::codehost::Review;
use kelta_proto::error::KeltaError;
use kelta_proto::ids::{ProjectId, SessionId, WorkItemId};
use kelta_proto::model::{
    FinishOpts, Lifecycle, RunClaim, RunState, WorkItem, WorkKind, WorkSource, WorkState,
};
use kelta_proto::tracker::{StatusCategory, TicketRef};

use crate::{WorkService, git, plan};

impl WorkService {
    /// `claim_ticket`: `worktree` defaults to the project's primary repo, `branch` to its checkout.
    pub async fn claim(
        &self,
        project: &ProjectId,
        ticket: TicketRef,
        worktree: Option<PathBuf>,
        branch: Option<String>,
        run: RunClaim,
    ) -> Result<WorkItem, KeltaError> {
        self.ensure_listener();
        let core = self.api()?;
        let p = core.project(project).ok_or_else(|| KeltaError::not_found(format!("project {project}")))?;
        let primary = p.repos.iter().find(|r| r.primary).or(p.repos.first());
        let path = match worktree {
            Some(w) => w,
            None => {
                primary.map(|r| r.path.clone()).ok_or_else(|| KeltaError::invalid("project has no repo"))?
            }
        };
        let mut found = None;
        for r in &p.repos {
            if let Ok(Some(w)) = git::worktree_at(&r.path, &path).await {
                found = Some((r, w));
                break;
            }
        }
        let (repo, wt) = found.ok_or_else(|| {
            KeltaError::invalid(format!(
                "{} is not a git worktree of a repo of project {project}",
                path.display()
            ))
        })?;
        let branch = branch.filter(|b| !b.trim().is_empty()).or(wt.branch).ok_or_else(|| {
            KeltaError::invalid(format!("{} has no branch checked out; pass `branch`", path.display()))
        })?;
        // Later pushed and shown verbatim: refuse anything git would not take as a branch name.
        let branch = git::check_branch_name(&wt.path, &branch).await?;
        let item = WorkItem {
            id: kelta_proto::ids::WorkItemId::generate(),
            project_id: project.clone(),
            kind: WorkKind::Ticket,
            ticket: Some(ticket),
            repo_id: repo.id.clone(),
            worktree: wt.path,
            branch,
            base: repo.base.clone(),
            state: WorkState::Active,
            created_at: kelta_proto::now_rfc3339(),
            run: Some(RunClaim { state: RunState::Running, ..run }),
            ..WorkItem::default()
        };
        // `start_lock`: no interactive start of the same ticket between the check and the insert.
        let claimed = {
            let _s = self.start_lock.lock().await;
            let _w = self.write_lock.lock().await;
            // Liveness is computed, not swept: a holder whose owner is gone is stored orphaned here,
            // so `claim_over` takes it over in the claim right below.
            let held = kelta_proto::api::claim_holder(&self.store.list_items(None).await?, &item);
            if let Some(mut h) = held.filter(|h| h.run.as_ref().is_some_and(|r| self.orphaned(r))) {
                if let Some(r) = h.run.as_mut() {
                    r.state = RunState::Orphaned;
                }
                self.store.put_item(&h).await?;
            }
            self.store.claim_item(&item).await?
        };
        match claimed {
            Ok(w) => {
                self.publish_updated(&w);
                Ok(w)
            }
            Err(holder) => {
                let key = holder.ticket.as_ref().map_or("", |t| t.key.as_str());
                let session =
                    |sid: &SessionId| core.session_get(sid).map_or_else(|| sid.to_string(), |s| s.name);
                let who = match &holder.run {
                    Some(r) => format!(
                        "session {} ({})",
                        r.session_id.as_ref().map_or_else(|| "outside Kelta".to_owned(), session),
                        r.name.as_deref().unwrap_or("run")
                    ),
                    None => format!("work item {} (started in Kelta)", holder.id),
                };
                Err(KeltaError::conflict(format!("{key} is held by {who}")))
            }
        }
    }

    /// MCP `link_pr` (#453): the work item of `ticket` on `pr`'s head branch, without Claude. The
    /// scratch item adopting `pr` gets the ticket; else a new item runs Start work up to its
    /// worktree (the one that has the branch checked out, or a new one) and Resume runs the rest.
    /// The `bool` is `true` when it linked, `false` when the PR's item already had this ticket (a
    /// no-op). `Conflict` names the item holding the ticket or the PR.
    pub async fn link_ticket_pr(
        &self,
        project: &ProjectId,
        ticket: TicketRef,
        pr: &Review,
    ) -> Result<(WorkItem, bool), KeltaError> {
        self.ensure_listener();
        let core = self.api()?;
        let p = core.project(project).ok_or_else(|| KeltaError::not_found(format!("project {project}")))?;
        let guard = self.start_lock.lock().await;
        let all = self.store.list_items(None).await?;
        let source = WorkSource::Ticket { ticket: ticket.clone() };
        // A ticket held by other items gets one more (#456); only the PR's own item decides.
        if let Some(o) = plan::owner_of_pr(&all, &p, pr) {
            match &o.ticket {
                Some(t) if t.account == ticket.account && t.key == ticket.key => {
                    return Ok((self.load(&o.id).await?, false));
                }
                Some(t) => {
                    let who = format!("work item {} on {} ({})", o.id, o.branch, t.key);
                    return Err(KeltaError::conflict(format!("{} is held by {who}", pr.url)));
                }
                None => {
                    let id = o.id.clone();
                    drop(guard);
                    return self.link(&id, ticket, false).await.map(|w| (w, true));
                }
            }
        }
        let repo = p
            .repos
            .iter()
            .find(|r| r.code_host.as_ref().is_some_and(|c| c.repo.eq_ignore_ascii_case(&pr.r#ref.repo)))
            .ok_or_else(|| KeltaError::invalid(format!("{} has no repository {}", p.name, pr.r#ref.repo)))?;
        let mut plan = self.build_plan(project, source, Some(repo.id.clone()), true).await?;
        if plan.adopt_pr.as_deref() != Some(pr.url.as_str()) {
            // A PR that does not name the ticket (yet): its branch replaces the template's.
            plan.repo_id = repo.id.clone();
            plan.branch = pr.source_branch.clone();
            plan.base = pr.target_branch.clone();
            plan.branch_exists = None;
            plan.adopt_pr = Some(pr.url.clone());
        }
        let id = self.create_item(plan).await?;
        let lock = self.item_lock(&id);
        let _item = lock.lock().await;
        drop(guard);
        let item = self.run_saga(&id, Some("worktree")).await?;
        match &item.state {
            WorkState::Failed { step, message } => {
                Err(KeltaError::upstream(format!("work item {id} failed at {step}: {message}")))
            }
            _ => Ok((item, true)),
        }
    }

    /// `report_progress`; `caller` is the MCP session (`None`: the global endpoint).
    pub async fn report(
        &self,
        key: &str,
        phase: Option<String>,
        state: RunState,
        note: Option<String>,
        sha: Option<String>,
        caller: Option<&SessionId>,
    ) -> Result<WorkItem, KeltaError> {
        let id = self.claimed(key, caller).await?.id;
        self.update(&id, |w| {
            let Some(r) = w.run.as_mut() else { return false };
            // An orphaned claim driven by another caller becomes its own, never the dead owner's again.
            if r.session_id.is_some() && r.session_id.as_ref() != caller {
                r.session_id = caller.cloned();
            }
            r.state = state;
            r.phase = phase.or(r.phase.take());
            r.note = note.or(r.note.take());
            r.sha = sha.or(r.sha.take());
            true
        })
        .await
    }

    /// `release_ticket` (merged or abandoned): Finish, which leaves an adopted worktree and branch alone.
    pub async fn release(
        &self,
        key: &str,
        sha: Option<String>,
        caller: Option<&SessionId>,
    ) -> Result<WorkItem, KeltaError> {
        let id = self.claimed(key, caller).await?.id;
        self.end_claim(&id, sha).await
    }

    async fn end_claim(&self, id: &WorkItemId, sha: Option<String>) -> Result<WorkItem, KeltaError> {
        self.update(id, |w| {
            let Some(r) = w.run.as_mut() else { return false };
            r.state = RunState::Done;
            r.sha = sha.or(r.sha.take());
            true
        })
        .await?;
        let opts =
            FinishOpts { remove_worktree: false, delete_branch: false, force: false, transition_to: None };
        self.finish_impl(id, opts).await
    }

    /// Startup and Queue open (FLOW §3.6, #444): a claim no live run drives whose worktree is gone or
    /// whose ticket is done in its tracker is released as `release_ticket` would, so it stops showing.
    pub async fn finish_stale_claims(&self) -> Result<(), KeltaError> {
        let core = self.api()?;
        for w in self.store.list_items(None).await? {
            let Some(r) = w.run.as_ref().filter(|_| w.state != WorkState::Finished) else { continue };
            if r.state == RunState::Running && r.session_id.is_some() && !self.orphaned(r) {
                continue;
            }
            let gone = !w.worktree.exists();
            let done = !gone
                && match &w.ticket {
                    Some(t) => match core.tracker_for(&t.account).await {
                        Ok(tr) => {
                            tr.get(t).await.is_ok_and(|d| d.ticket.status.category == StatusCategory::Done)
                        }
                        Err(_) => false, // account error: the next check retries
                    },
                    None => false,
                };
            if (gone || done)
                && let Err(e) = self.end_claim(&w.id, None).await
            {
                tracing::warn!(work = %w.id, error = %e.message, "stale claim not finished");
            }
        }
        Ok(())
    }

    /// The claim's owner session is not Live (exited, Dormant after a restart, or gone): computed at
    /// each use, never swept, so no missed lifecycle path leaves a ticket locked (#271).
    pub(crate) fn orphaned(&self, r: &RunClaim) -> bool {
        r.state == RunState::Orphaned
            || r.state == RunState::Running
                && r.session_id.as_ref().is_some_and(|sid| {
                    self.api()
                        .is_ok_and(|c| c.session_get(sid).is_none_or(|s| s.lifecycle != Lifecycle::Live))
                })
    }

    /// `w` as shown: a running claim whose owner is gone reads `orphaned`.
    pub(crate) fn shown(&self, w: &mut WorkItem) {
        if let Some(r) = w.run.as_mut()
            && r.state == RunState::Running
            && self.orphaned(r)
        {
            r.state = RunState::Orphaned;
        }
    }

    /// `sid` exited: republish its claims so they show orphaned.
    pub(crate) async fn publish_claims_of(&self, sid: &SessionId) -> Result<(), KeltaError> {
        for w in self.store.list_items(None).await? {
            if w.state != WorkState::Finished
                && w.run.as_ref().is_some_and(|r| r.session_id.as_ref() == Some(sid))
            {
                self.publish_updated(&w);
            }
        }
        Ok(())
    }

    /// The live claim on `key` that `caller` may drive: a claim owned by a live session answers only to it.
    // shortcut: matched by ticket key, not (account, key); MCP progress/release take no account, so two
    // claims of one caller on equal keys of different accounts pick the first. Add `account` if that happens.
    async fn claimed(&self, key: &str, caller: Option<&SessionId>) -> Result<WorkItem, KeltaError> {
        let mut hits: Vec<WorkItem> = self
            .store
            .list_items(None)
            .await?
            .into_iter()
            .filter(|w| {
                w.run.is_some()
                    && w.state != WorkState::Finished
                    && w.ticket.as_ref().is_some_and(|t| t.key == key)
            })
            .collect();
        if hits.is_empty() {
            return Err(KeltaError::not_found(format!("no claim on {key}")));
        }
        // The caller's own claim first, then an unowned one, then an orphaned one (its session is gone).
        let rank = |w: &WorkItem| {
            let r = w.run.as_ref()?;
            match &r.session_id {
                Some(o) if Some(o) == caller => Some(0),
                None => Some(1),
                Some(_) if self.orphaned(r) => Some(2),
                Some(_) => None,
            }
        };
        match hits.iter().enumerate().filter_map(|(i, w)| Some((rank(w)?, i))).min() {
            Some((_, i)) => Ok(hits.swap_remove(i)),
            None => Err(KeltaError::conflict(format!("{key} is claimed by another session"))),
        }
    }
}

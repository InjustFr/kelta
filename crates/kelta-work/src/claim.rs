//! Background runs claiming tickets (MCP `claim_ticket` / `report_progress` / `release_ticket`, #235).
//! A claimed item adopts the run's worktree and branch: Kelta never creates, removes or deletes them.

use std::path::PathBuf;

use kelta_proto::error::KeltaError;
use kelta_proto::ids::{ProjectId, SessionId};
use kelta_proto::model::{FinishOpts, RunClaim, RunState, WorkItem, WorkKind, WorkState};
use kelta_proto::tracker::TicketRef;

use crate::{WorkService, git};

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

    /// `report_progress`.
    pub async fn report(
        &self,
        key: &str,
        phase: Option<String>,
        state: RunState,
        note: Option<String>,
        sha: Option<String>,
    ) -> Result<WorkItem, KeltaError> {
        let id = self.claimed(key).await?.id;
        self.update(&id, |w| {
            let Some(r) = w.run.as_mut() else { return false };
            r.state = state;
            r.phase = phase.or(r.phase.take());
            r.note = note.or(r.note.take());
            r.sha = sha.or(r.sha.take());
            true
        })
        .await
    }

    /// `release_ticket` (merged or abandoned): Finish, which leaves an adopted worktree and branch alone.
    pub async fn release(&self, key: &str, sha: Option<String>) -> Result<WorkItem, KeltaError> {
        let id = self.claimed(key).await?.id;
        self.update(&id, |w| {
            let Some(r) = w.run.as_mut() else { return false };
            r.state = RunState::Done;
            r.sha = sha.or(r.sha.take());
            true
        })
        .await?;
        let opts =
            FinishOpts { remove_worktree: false, delete_branch: false, force: false, transition_to: None };
        self.finish_impl(&id, opts).await
    }

    /// Running claims of `sid` (any exited session for `None`, at startup) turn `orphaned`.
    pub(crate) async fn orphan_claims(&self, sid: Option<&SessionId>) -> Result<(), KeltaError> {
        let core = self.api()?;
        for w in self.store.list_items(None).await? {
            let Some(owner) =
                w.run.as_ref().filter(|r| r.state == RunState::Running).and_then(|r| r.session_id.clone())
            else {
                continue;
            };
            let gone = match sid {
                Some(s) => *s == owner,
                // Boot already adopted daemon sessions as Live: a Dormant or Exited owner has no workflow running.
                None => core
                    .session_get(&owner)
                    .is_none_or(|s| s.lifecycle != kelta_proto::model::Lifecycle::Live),
            };
            if gone && w.state != WorkState::Finished {
                self.update(&w.id, |w| {
                    let Some(r) = w.run.as_mut().filter(|r| r.state == RunState::Running) else {
                        return false;
                    };
                    r.state = RunState::Orphaned;
                    true
                })
                .await?;
            }
        }
        Ok(())
    }

    async fn claimed(&self, key: &str) -> Result<WorkItem, KeltaError> {
        self.store
            .list_items(None)
            .await?
            .into_iter()
            .find(|w| {
                w.run.is_some()
                    && w.state != WorkState::Finished
                    && w.ticket.as_ref().is_some_and(|t| t.key == key)
            })
            .ok_or_else(|| KeltaError::not_found(format!("no claim on {key}")))
    }
}

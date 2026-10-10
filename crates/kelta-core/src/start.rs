//! Which project a start runs in (SPEC §3.1): the default and the choices of the Start sheet.

use kelta_proto::error::KeltaError;
use kelta_proto::events::ToastAction;
use kelta_proto::ids::ProjectId;
use kelta_proto::model::{StartWorkPlan, WorkItem, WorkSource};
use kelta_work::plan::existing_for;
use serde_json::json;

use crate::Core;
use crate::store::q;

impl Core {
    /// `work_plan`: the plan in the default project (`project` is the caller's hint), with the choices.
    pub async fn work_plan(
        &self,
        project: Option<ProjectId>,
        source: WorkSource,
    ) -> Result<StartWorkPlan, KeltaError> {
        if matches!(source, WorkSource::Branch { .. }) {
            let project = project.unwrap_or_else(|| self.active_project());
            return self.work.plan(&project, source).await;
        }
        let open = self.open_projects();
        let configs = self.project_configs();
        let mut choices: Vec<ProjectId> = configs
            .iter()
            .filter(|p| p.id.as_str() != ProjectId::HOME && !p.repos.is_empty() && open.contains(&p.id))
            .map(|p| p.id.clone())
            .collect();
        let account_project = match &source {
            WorkSource::Ticket { ticket } => self.project_for_account(&ticket.account).map(|p| p.id.clone()),
            WorkSource::Review { review } => {
                let binds: Vec<ProjectId> = self
                    .review_bindings()
                    .into_iter()
                    .filter(|(a, r, _)| a == &review.account && r == &review.repo)
                    .map(|(_, _, p)| p)
                    .collect();
                choices.retain(|p| binds.contains(p));
                None
            }
            WorkSource::Branch { .. } => None,
        };
        let items = self.store.call(|c| q::work_list(c, None)).await?;
        let active = self.active_project();
        let (project, guessed) =
            default_project(&source, project.as_ref(), &items, &choices, account_project, &active)?;
        let mut plan = self.work.plan(&project, source).await?;
        plan.project_choices = choices;
        plan.project_guessed = guessed && plan.existing.is_none();
        Ok(plan)
    }
}

/// Rules of SPEC §3.1, first match wins; `true` = only a guess. `choices` are the open projects with a
/// repository (for a PR, those that bind its repository).
fn default_project(
    source: &WorkSource,
    caller: Option<&ProjectId>,
    items: &[WorkItem],
    choices: &[ProjectId],
    account_project: Option<ProjectId>,
    active: &ProjectId,
) -> Result<(ProjectId, bool), KeltaError> {
    // 1. Where its unfinished work item lives, closed project included (Resume opens it).
    if let Some(w) = existing_for(items, source) {
        return Ok((w.project_id.clone(), false));
    }
    let caller = caller.filter(|p| choices.contains(p)).cloned();
    match source {
        WorkSource::Review { review } => {
            caller.or_else(|| choices.first().cloned()).map(|p| (p, false)).ok_or_else(|| {
                refused(
                    format!("No project has {} as a repository", review.repo),
                    "Open project settings",
                    "settings.open",
                    Some(json!({ "section": "projects" })),
                )
            })
        }
        WorkSource::Ticket { ticket } => {
            if choices.is_empty() {
                return Err(refused(
                    "Add a repository to a project first",
                    "Add project",
                    "project.new",
                    None,
                ));
            }
            // 3. My last pick for a ticket of this account.
            let last = || {
                items
                    .iter()
                    .filter(|w| w.ticket.as_ref().is_some_and(|t| t.account == ticket.account))
                    .filter(|w| choices.contains(&w.project_id))
                    .max_by(|a, b| a.created_at.cmp(&b.created_at))
                    .map(|w| w.project_id.clone())
            };
            let usual = caller.or_else(last).or(account_project.filter(|p| choices.contains(p)));
            Ok(match usual {
                Some(p) => (p, false),
                None => (if choices.contains(active) { active.clone() } else { choices[0].clone() }, true),
            })
        }
        WorkSource::Branch { .. } => Ok((caller.unwrap_or_else(|| active.clone()), false)),
    }
}

/// An invalid start whose toast offers the fix (`detail.action`, run by the UI action registry).
fn refused(
    message: impl Into<String>,
    label: &str,
    command: &str,
    args: Option<serde_json::Value>,
) -> KeltaError {
    let action = ToastAction { label: label.into(), command: command.into(), args };
    let mut e = KeltaError::invalid(message);
    e.detail = Some(json!({ "action": action }));
    e
}

#[cfg(test)]
mod tests {
    use kelta_proto::ids::AccountId;
    use kelta_proto::model::WorkState;
    use kelta_proto::samples;

    use super::*;

    fn p(id: &str) -> ProjectId {
        ProjectId::new(id)
    }

    fn ticket() -> WorkSource {
        WorkSource::Ticket { ticket: samples::ticket_ref() }
    }

    fn review() -> WorkSource {
        WorkSource::Review { review: samples::review_ref() }
    }

    /// A work item in `project` on `key` (account of the sample ticket), created at `at`.
    fn item(project: &str, key: &str, at: &str, finished: bool) -> WorkItem {
        let mut w = samples::work_item();
        w.project_id = p(project);
        w.ticket.as_mut().unwrap().key = key.into();
        w.created_at = at.into();
        if finished {
            w.state = WorkState::Finished;
        }
        w
    }

    fn pick(
        source: &WorkSource,
        caller: Option<&str>,
        items: &[WorkItem],
        choices: &[&str],
        account: Option<&str>,
    ) -> Result<(ProjectId, bool), KeltaError> {
        let choices: Vec<ProjectId> = choices.iter().map(|c| p(c)).collect();
        default_project(source, caller.map(p).as_ref(), items, &choices, account.map(p), &p("active"))
    }

    #[test]
    fn ticket_rule_1_unfinished_item_anywhere_wins() {
        let key = samples::ticket_ref().key;
        let items = [item("closed", &key, "2026-01-01", false), item("blog", "X-1", "2026-02-01", false)];
        assert_eq!(
            pick(&ticket(), Some("shop"), &items, &["shop", "blog"], None).unwrap(),
            (p("closed"), false)
        );
        let done = [item("closed", &key, "2026-01-01", true)];
        assert_eq!(pick(&ticket(), Some("shop"), &done, &["shop"], None).unwrap(), (p("shop"), false));
    }

    #[test]
    fn ticket_rule_2_caller_project() {
        let items = [item("blog", "X-1", "2026-02-01", false)];
        assert_eq!(
            pick(&ticket(), Some("shop"), &items, &["shop", "blog"], None).unwrap(),
            (p("shop"), false)
        );
        // a closed caller project is no choice
        assert_eq!(
            pick(&ticket(), Some("gone"), &items, &["shop", "blog"], None).unwrap(),
            (p("blog"), false)
        );
    }

    #[test]
    fn ticket_rule_3_last_project_of_the_account() {
        let mut other = item("api", "Y-1", "2026-03-01", true);
        other.ticket.as_mut().unwrap().account = AccountId::new("linear-team");
        let items = [item("blog", "X-1", "2026-01-01", true), item("shop", "X-2", "2026-02-01", true), other];
        assert_eq!(
            pick(&ticket(), None, &items, &["shop", "blog", "api"], Some("blog")).unwrap(),
            (p("shop"), false)
        );
    }

    #[test]
    fn ticket_rule_4_project_bound_to_the_account() {
        assert_eq!(pick(&ticket(), None, &[], &["shop", "blog"], Some("blog")).unwrap(), (p("blog"), false));
        // closed bound project: falls to the guess
        assert_eq!(pick(&ticket(), None, &[], &["shop", "blog"], Some("gone")).unwrap(), (p("shop"), true));
    }

    #[test]
    fn ticket_rule_5_active_project_is_a_guess() {
        assert_eq!(pick(&ticket(), None, &[], &["shop", "active"], None).unwrap(), (p("active"), true));
        let e = pick(&ticket(), None, &[], &[], None).unwrap_err();
        assert_eq!(e.message, "Add a repository to a project first");
        assert_eq!(e.detail.unwrap()["action"]["command"], "project.new");
    }

    #[test]
    fn review_rules() {
        let mut w = item("closed", "X-1", "2026-01-01", false);
        w.ticket = None;
        w.review = Some(samples::review_ref());
        // 1. its work item's project
        assert_eq!(pick(&review(), Some("shop"), &[w], &["shop"], None).unwrap(), (p("closed"), false));
        // 2. the caller's project when it binds the repo, 3. else the first that does
        assert_eq!(pick(&review(), Some("blog"), &[], &["shop", "blog"], None).unwrap(), (p("blog"), false));
        assert_eq!(pick(&review(), Some("api"), &[], &["shop", "blog"], None).unwrap(), (p("shop"), false));
        let e = pick(&review(), Some("api"), &[], &[], None).unwrap_err();
        assert_eq!(e.message, format!("No project has {} as a repository", samples::review_ref().repo));
        assert_eq!(e.detail.unwrap()["action"]["command"], "settings.open");
    }
}

//! Which project a start runs in (SPEC §3.1): the default and the choices of the Start sheet.

use kelta_proto::error::KeltaError;
use kelta_proto::events::ToastAction;
use kelta_proto::ids::{AccountId, ProjectId};
use kelta_proto::model::{StartWorkPlan, WorkItem, WorkSource};
use kelta_proto::settings::Source;
use kelta_proto::tracker::{Ticket, TicketRef};
use kelta_work::plan::existing_for;
use serde_json::json;

use crate::Core;
use crate::feeds::uses_account;
use crate::store::q;

/// A start's project, the route's repo, and `true` = only a guess.
type Target = (ProjectId, Option<String>, bool);

/// A ticket's source for rules 3-5: its id, the projects it feeds and the route the ticket matches
/// (project, repo).
#[derive(Debug, Default)]
struct SourceHint {
    id: String,
    projects: Vec<ProjectId>,
    route: Option<(ProjectId, Option<String>)>,
}

impl Core {
    /// `work_plan`: the plan in the default project (`project` is an explicit pick), with the choices.
    pub async fn work_plan(
        &self,
        project: Option<ProjectId>,
        source: WorkSource,
    ) -> Result<StartWorkPlan, KeltaError> {
        if matches!(source, WorkSource::Branch { .. }) {
            let project = project.unwrap_or_else(|| self.active_project());
            return self.work.plan(&project, source).await;
        }
        let (target, choices, hint) = self.plan_target(project.as_ref(), &source).await?;
        let (project, repo, guessed) = target;
        let mut plan = self.work.plan_with_repo(&project, source, repo).await?;
        plan.project_choices = choices;
        plan.ticket_source = hint.map(|h| h.id);
        // One choice: nothing to confirm (the sheet shows no Project field).
        plan.project_guessed = guessed && plan.existing.is_none() && plan.project_choices.len() > 1;
        Ok(plan)
    }

    /// The project a ticket's background run goes to (#233): the start-work rules, no plan built.
    pub(crate) async fn ticket_project(&self, ticket: &TicketRef) -> Result<ProjectId, KeltaError> {
        let source = WorkSource::Ticket { ticket: ticket.clone() };
        Ok(self.plan_target(None, &source).await?.0.0)
    }

    /// SPEC §3.1 for a ticket or a PR: the default (project, route repo, guessed), the choices and
    /// the ticket's source.
    async fn plan_target(
        &self,
        project: Option<&ProjectId>,
        source: &WorkSource,
    ) -> Result<(Target, Vec<ProjectId>, Option<SourceHint>), KeltaError> {
        let open = self.open_projects();
        let configs = self.project_configs();
        let mut choices: Vec<ProjectId> = configs
            .iter()
            .filter(|p| p.id.as_str() != ProjectId::HOME && !p.repos.is_empty() && open.contains(&p.id))
            .map(|p| p.id.clone())
            .collect();
        let sources = self.sources();
        let hint = match source {
            WorkSource::Ticket { ticket } => self.source_hint(ticket, &sources).await,
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
        // The source's projects first (stable: the others keep the config order).
        if let Some(h) = &hint {
            choices.sort_by_key(|p| h.projects.iter().position(|x| x == p).unwrap_or(usize::MAX));
        }
        // A removed project (or one without a repository) keeps its work items but cannot host a start.
        let mut items = self.store.call(|c| q::work_list(c, None)).await?;
        items.retain(|w| configs.iter().any(|p| p.id == w.project_id && !p.repos.is_empty()));
        fill_sources(&mut items, &sources);
        let active = self.active_project();
        let target = default_project(source, project, &items, &choices, hint.as_ref(), &active)?;
        Ok((target, choices, hint))
    }

    /// The ticket's source, and the route it matches from the cached ticket, else the tracker's (a
    /// failed fetch matches no route: the plan reports it).
    async fn source_hint(&self, t: &TicketRef, sources: &[Source]) -> Option<SourceHint> {
        let cached = self.cached_ticket(t).await;
        let listed_in = cached.as_ref().map(|(_, ids)| ids.as_slice()).unwrap_or_default();
        let s = source_of(sources, &t.account, listed_in)?;
        let route = if s.routes.is_empty() {
            None
        } else {
            let ticket = match cached {
                Some((x, _)) => Some(x),
                None => match self.tracker_of(&t.account) {
                    Ok(tr) => tr.get(t).await.ok().map(|d| d.ticket),
                    Err(_) => None,
                },
            };
            ticket.and_then(|x| route_for(s, &x))
        };
        Some(SourceHint { id: s.id.clone(), projects: s.projects.clone(), route })
    }
}

/// The source using `account`; when several do, the first listing the ticket (`listed_in`, from the
/// cached views), else the first in config order.
fn source_of<'a>(sources: &'a [Source], account: &AccountId, listed_in: &[String]) -> Option<&'a Source> {
    let mut using = sources.iter().filter(|s| uses_account(s, account));
    let first = using.clone().next()?;
    Some(using.find(|s| listed_in.contains(&s.id)).unwrap_or(first))
}

/// The first route of `s` the ticket matches (component, label or type): its project and repo.
fn route_for(s: &Source, t: &Ticket) -> Option<(ProjectId, Option<String>)> {
    let r = s.routes.iter().find(|r| r.r#match.matches(t.kind.as_deref(), &t.labels))?;
    Some((r.project.clone(), Some(r.repo.clone()).filter(|x| !x.is_empty())))
}

/// Items started before work items stored their source count for the only source on their account.
fn fill_sources(items: &mut [WorkItem], sources: &[Source]) {
    for w in items.iter_mut().filter(|w| w.ticket_source.is_none()) {
        let Some(t) = &w.ticket else { continue };
        let mut using = sources.iter().filter(|s| uses_account(s, &t.account));
        if let (Some(s), None) = (using.next(), using.next()) {
            w.ticket_source = Some(s.id.clone());
        }
    }
}

/// Rules of SPEC §3.1, first match wins: the project, the route's repo when the project is the
/// route's, and `true` = only a guess. `choices` are the open projects with a repository (for a PR,
/// those that bind its repository).
fn default_project(
    source: &WorkSource,
    caller: Option<&ProjectId>,
    items: &[WorkItem],
    choices: &[ProjectId],
    hint: Option<&SourceHint>,
    active: &ProjectId,
) -> Result<Target, KeltaError> {
    // 1. Where its unfinished work item lives, closed project included (Resume opens it).
    if let Some(w) = existing_for(items, source) {
        return Ok((w.project_id.clone(), None, false));
    }
    let caller = caller.filter(|p| choices.contains(p)).cloned();
    match source {
        WorkSource::Review { review } => {
            caller.or_else(|| choices.first().cloned()).map(|p| (p, None, false)).ok_or_else(|| {
                refused(
                    format!("No project has {} as a repository", review.repo),
                    "Open project settings",
                    "settings.open",
                    Some(json!({ "section": "projects" })),
                )
            })
        }
        WorkSource::Ticket { .. } => {
            if choices.is_empty() {
                return Err(refused(
                    "Add a repository to a project first",
                    "Add project",
                    "project.new",
                    None,
                ));
            }
            let route = hint.and_then(|h| h.route.clone()).filter(|(p, _)| choices.contains(p));
            // 4. My last pick for a ticket of this source.
            let last = || {
                let id = hint.map(|h| h.id.as_str())?;
                items
                    .iter()
                    .filter(|w| w.ticket.is_some() && w.ticket_source.as_deref() == Some(id))
                    .filter(|w| choices.contains(&w.project_id))
                    .max_by(|a, b| a.created_at.cmp(&b.created_at))
                    .map(|w| w.project_id.clone())
            };
            // 5. The source's open projects: a guess when there are several.
            let fed = || {
                let fed: Vec<&ProjectId> = hint
                    .map(|h| h.projects.iter().filter(|p| choices.contains(p)).collect())
                    .unwrap_or_default();
                fed.first().map(|p| ((*p).clone(), fed.len() > 1))
            };
            // 2. explicit pick, 3. route, 4, 5, 6. the active project, a guess.
            let (project, guessed) = caller
                .or_else(|| route.as_ref().map(|(p, _)| p.clone()))
                .or_else(last)
                .map(|p| (p, false))
                .or_else(fed)
                .unwrap_or_else(|| {
                    (if choices.contains(active) { active } else { &choices[0] }.clone(), true)
                });
            let repo = route.filter(|(p, _)| p == &project).and_then(|(_, r)| r);
            Ok((project, repo, guessed))
        }
        WorkSource::Branch { .. } => Ok((caller.unwrap_or_else(|| active.clone()), None, false)),
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
    use kelta_proto::model::WorkState;
    use kelta_proto::samples;
    use kelta_proto::settings::{RepoMatch, SourceRoute, TrackerBinding};

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

    /// A finished item on a ticket of source `src` (`None` = started before sources were stored).
    fn picked(project: &str, at: &str, src: Option<&str>) -> WorkItem {
        let mut w = item(project, "X-1", at, true);
        w.ticket_source = src.map(Into::into);
        w
    }

    fn hint(id: &str, projects: &[&str], route: Option<(&str, &str)>) -> SourceHint {
        SourceHint {
            id: id.into(),
            projects: projects.iter().map(|x| p(x)).collect(),
            route: route.map(|(x, r)| (p(x), Some(r.to_owned()).filter(|r| !r.is_empty()))),
        }
    }

    fn pick(
        source: &WorkSource,
        caller: Option<&str>,
        items: &[WorkItem],
        choices: &[&str],
        hint: Option<&SourceHint>,
    ) -> Result<(ProjectId, Option<String>, bool), KeltaError> {
        let choices: Vec<ProjectId> = choices.iter().map(|c| p(c)).collect();
        default_project(source, caller.map(p).as_ref(), items, &choices, hint, &p("active"))
    }

    fn source(id: &str, account: &str, routes: Vec<SourceRoute>) -> Source {
        Source {
            id: id.into(),
            tracker: TrackerBinding { account: AccountId::new(account), ..TrackerBinding::default() },
            routes,
            ..Source::default()
        }
    }

    fn route(component: Option<&str>, label: Option<&str>, kind: Option<&str>, project: &str) -> SourceRoute {
        SourceRoute {
            r#match: RepoMatch {
                component: component.map(Into::into),
                label: label.map(Into::into),
                kind: kind.map(Into::into),
            },
            project: p(project),
            repo: format!("{project}-repo"),
        }
    }

    #[test]
    fn ticket_rule_1_unfinished_item_beats_a_route() {
        let key = samples::ticket_ref().key;
        let items = [item("closed", &key, "2026-01-01", false), item("blog", "X-1", "2026-02-01", false)];
        let h = hint("product", &["shop"], Some(("shop", "web")));
        assert_eq!(
            pick(&ticket(), Some("shop"), &items, &["shop", "blog"], Some(&h)).unwrap(),
            (p("closed"), None, false)
        );
        let done = [item("closed", &key, "2026-01-01", true)];
        assert_eq!(
            pick(&ticket(), None, &done, &["shop"], Some(&h)).unwrap(),
            (p("shop"), Some("web".into()), false)
        );
    }

    #[test]
    fn ticket_rule_2_explicit_pick_beats_a_route() {
        let h = hint("product", &["shop", "blog"], Some(("shop", "web")));
        assert_eq!(
            pick(&ticket(), Some("blog"), &[], &["shop", "blog"], Some(&h)).unwrap(),
            (p("blog"), None, false)
        );
        // the route's repo follows its project
        assert_eq!(
            pick(&ticket(), Some("shop"), &[], &["shop", "blog"], Some(&h)).unwrap(),
            (p("shop"), Some("web".into()), false)
        );
        // a closed pick is no choice
        assert_eq!(
            pick(&ticket(), Some("gone"), &[], &["shop", "blog"], None).unwrap(),
            (p("shop"), None, true)
        );
    }

    #[test]
    fn ticket_rule_3_routes_match_component_label_type_first_wins() {
        let s = source(
            "product",
            "jira-acme",
            vec![
                route(None, None, None, "never"),
                route(Some("frontend"), None, None, "web"),
                route(None, Some("api"), None, "api"),
                route(None, None, Some("bug"), "fixes"),
            ],
        );
        let mut t = samples::ticket();
        let at = |t: &Ticket| route_for(&s, t).map(|(p, r)| (p.as_str().to_owned(), r));
        assert_eq!(at(&t), Some(("api".into(), Some("api-repo".into()))), "label");
        t.labels = vec!["Frontend".into(), "api".into()];
        assert_eq!(at(&t).unwrap().0, "web", "component, and the first match wins");
        t.labels.clear();
        t.kind = Some("Bug".into());
        assert_eq!(at(&t).unwrap().0, "fixes", "type");
        t.kind = None;
        assert_eq!(at(&t), None, "an empty match never matches");
    }

    #[test]
    fn ticket_rule_3_route_to_a_closed_project_falls_through() {
        let h = hint("product", &["shop", "blog"], Some(("gone", "web")));
        let items = [picked("blog", "2026-01-01", Some("product"))];
        assert_eq!(
            pick(&ticket(), None, &items, &["shop", "blog"], Some(&h)).unwrap(),
            (p("blog"), None, false)
        );
        // a route without a repo still picks its project
        let h = hint("product", &["shop", "blog"], Some(("blog", "")));
        assert_eq!(
            pick(&ticket(), None, &[], &["shop", "blog"], Some(&h)).unwrap(),
            (p("blog"), None, false)
        );
    }

    #[test]
    fn ticket_rule_4_last_pick_per_source() {
        // two sources on one account stay apart
        let items = [
            picked("blog", "2026-01-01", Some("product")),
            picked("shop", "2026-02-01", Some("product")),
            picked("api", "2026-03-01", Some("tech-lead")),
        ];
        let h = hint("product", &["blog", "shop", "api"], None);
        assert_eq!(
            pick(&ticket(), None, &items, &["shop", "blog", "api"], Some(&h)).unwrap(),
            (p("shop"), None, false)
        );
        let h = hint("tech-lead", &["shop", "api"], None);
        assert_eq!(
            pick(&ticket(), None, &items, &["shop", "blog", "api"], Some(&h)).unwrap(),
            (p("api"), None, false)
        );
        // an item with no source counts only when its account has one source
        let one = [source("product", "jira-acme", vec![])];
        let two = [source("product", "jira-acme", vec![]), source("tech-lead", "jira-acme", vec![])];
        let mut old = [picked("blog", "2026-01-01", None)];
        fill_sources(&mut old, &two);
        assert_eq!(old[0].ticket_source, None);
        fill_sources(&mut old, &one);
        assert_eq!(old[0].ticket_source.as_deref(), Some("product"));
    }

    #[test]
    fn ticket_rule_5_the_sources_projects() {
        let one = hint("product", &["gone", "blog"], None);
        assert_eq!(
            pick(&ticket(), None, &[], &["shop", "blog"], Some(&one)).unwrap(),
            (p("blog"), None, false)
        );
        let two = hint("product", &["blog", "shop"], None);
        assert_eq!(
            pick(&ticket(), None, &[], &["shop", "blog"], Some(&two)).unwrap(),
            (p("blog"), None, true)
        );
    }

    #[test]
    fn ticket_rule_6_active_project_is_a_guess() {
        assert_eq!(pick(&ticket(), None, &[], &["shop", "active"], None).unwrap(), (p("active"), None, true));
        let none = hint("product", &[], None);
        assert_eq!(pick(&ticket(), None, &[], &["shop"], Some(&none)).unwrap(), (p("shop"), None, true));
        let e = pick(&ticket(), None, &[], &[], None).unwrap_err();
        assert_eq!(e.message, "Add a repository to a project first");
        assert_eq!(e.detail.unwrap()["action"]["command"], "project.new");
    }

    #[test]
    fn source_of_a_ticket() {
        let account = AccountId::new("jira-acme");
        let mut viewed = source("tech-lead", "other", vec![]);
        viewed.tracker.views.push(kelta_proto::settings::TrackerView {
            account: Some(account.clone()),
            ..Default::default()
        });
        let sources = [source("linear", "linear", vec![]), source("product", "jira-acme", vec![]), viewed];
        let id = |listed: &[&str]| {
            let listed: Vec<String> = listed.iter().map(|x| (*x).to_owned()).collect();
            source_of(&sources, &account, &listed).map(|s| s.id.clone())
        };
        assert_eq!(id(&[]).as_deref(), Some("product"), "first in config order");
        assert_eq!(id(&["tech-lead"]).as_deref(), Some("tech-lead"), "the one whose views list it");
        assert_eq!(
            id(&["tech-lead", "product"]).as_deref(),
            Some("product"),
            "several list it: config order"
        );
        assert_eq!(source_of(&sources, &AccountId::new("nobody"), &[]).map(|s| &s.id), None);
    }

    #[test]
    fn review_rules() {
        let mut w = item("closed", "X-1", "2026-01-01", false);
        w.ticket = None;
        w.review = Some(samples::review_ref());
        // 1. its work item's project
        assert_eq!(pick(&review(), Some("shop"), &[w], &["shop"], None).unwrap(), (p("closed"), None, false));
        // 2. the caller's project when it binds the repo, 3. else the first that does
        assert_eq!(
            pick(&review(), Some("blog"), &[], &["shop", "blog"], None).unwrap(),
            (p("blog"), None, false)
        );
        assert_eq!(
            pick(&review(), Some("api"), &[], &["shop", "blog"], None).unwrap(),
            (p("shop"), None, false)
        );
        let e = pick(&review(), Some("api"), &[], &[], None).unwrap_err();
        assert_eq!(e.message, format!("No project has {} as a repository", samples::review_ref().repo));
        assert_eq!(e.detail.unwrap()["action"]["command"], "settings.open");
    }
}

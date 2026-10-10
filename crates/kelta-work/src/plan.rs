//! StartWorkPlan building (SPEC §3.1 step 1, §3.3 "Review locally").

use std::path::{Path, PathBuf};

use kelta_proto::codehost::{CodeHostKind, Review};
use kelta_proto::error::KeltaError;
use kelta_proto::model::{
    BranchChoice, BranchExists, ClaudePlan, PORT_BLOCK, ProjectInfo, RepoInfo, SideEffects, StartWorkPlan,
    WorkItem, WorkSource, WorkState,
};
use kelta_proto::settings::{RepoRule, Settings};
use kelta_proto::tracker::{Ticket, TrackerKind};

use crate::git;
use crate::template::{Ctx, Mode, expand_home, render, tidy_path};

/// Ticket facts kept in the saga journal (prompt, PR title/body, `{closes}`).
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct TicketSnap {
    pub key: String,
    pub branch_key: String,
    pub title: String,
    pub url: String,
    pub kind: Option<String>,
    pub labels: Vec<String>,
    pub tracker: Option<String>,
}

impl TicketSnap {
    pub fn from_ticket(t: &Ticket, branch_key: String, tracker: TrackerKind) -> Self {
        Self {
            key: t.r#ref.key.clone(),
            branch_key,
            title: t.title.clone(),
            url: t.url.clone(),
            kind: t.kind.clone(),
            labels: t.labels.clone(),
            tracker: serde_json::to_value(tracker).ok().and_then(|v| v.as_str().map(str::to_owned)),
        }
    }

    pub fn is_github(&self) -> bool {
        self.tracker.as_deref() == Some("github_issues")
    }
}

pub fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME").filter(|h| !h.is_empty()).map(PathBuf::from)
}

/// Project root used for `{project.root}`: primary repo (or first repo) path.
pub fn project_root(p: &ProjectInfo) -> PathBuf {
    p.repos.iter().find(|r| r.primary).or_else(|| p.repos.first()).map(|r| r.path.clone()).unwrap_or_default()
}

/// Shared placeholder context (SETTINGS §6).
pub fn base_ctx(project: &ProjectInfo, repo: Option<&RepoInfo>, dirs: &kelta_proto::dirs::Dirs) -> Ctx {
    let mut c = Ctx::new();
    c.set("project", project.id.as_str());
    c.set("project.id", project.id.as_str());
    c.set("project.name", project.name.clone());
    c.set("project.root", project_root(project).to_string_lossy().into_owned());
    if let Some(r) = repo {
        c.set("repo", r.id.clone());
        c.set("repo.id", r.id.clone());
        c.set("repo.path", r.path.to_string_lossy().into_owned());
        c.set(
            "repo.name",
            r.path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| r.id.clone()),
        );
        c.set("base", r.base.clone());
    }
    c.set("config_dir", dirs.config.to_string_lossy().into_owned());
    c.set("data_dir", dirs.data.to_string_lossy().into_owned());
    c.set("home", home_dir().map(|h| h.to_string_lossy().into_owned()).unwrap_or_default());
    c.set("user", std::env::var("USER").unwrap_or_default());
    for k in [
        "worktree",
        "branch",
        "key",
        "slug",
        "type",
        "ticket.key",
        "ticket.title",
        "ticket.url",
        "ticket.file",
        "task",
    ] {
        c.set(k, "");
    }
    for k in ["pr.url", "pr.number", "pr.head", "pr.base", "pr.title", "closes"] {
        c.set(k, "");
    }
    c
}

pub fn add_ticket(c: &mut Ctx, t: &TicketSnap) {
    c.set("ticket.key", t.key.clone());
    c.set("ticket.title", t.title.clone());
    c.set("ticket.url", t.url.clone());
}

pub fn add_review(c: &mut Ctx, r: &Review) {
    c.set("pr.url", r.url.clone());
    c.set("pr.number", r.r#ref.number.to_string());
    c.set("pr.head", r.source_branch.clone());
    c.set("pr.base", r.target_branch.clone());
    c.set("pr.title", r.title.clone());
}

/// `repo_rules` (first match wins) → primary → first.
pub fn select_repo(
    project: &ProjectInfo,
    rules: &[RepoRule],
    ticket: Option<&TicketSnap>,
    remembered: Option<&str>,
) -> Option<String> {
    let exists = |id: &str| project.repos.iter().any(|r| r.id == id);
    if let Some(id) = remembered.filter(|id| exists(id)) {
        return Some(id.to_owned());
    }
    if let Some(t) = ticket {
        let has_label = |l: &str| t.labels.iter().any(|x| x.eq_ignore_ascii_case(l));
        for rule in rules {
            let m = &rule.r#match;
            if m.component.is_none() && m.label.is_none() && m.kind.is_none() {
                continue;
            }
            // Tickets carry no component field: components are matched against labels.
            let comp_ok = m.component.as_deref().is_none_or(has_label);
            let label_ok = m.label.as_deref().is_none_or(has_label);
            let kind_ok = m
                .kind
                .as_deref()
                .is_none_or(|k| t.kind.as_deref().is_some_and(|tk| tk.eq_ignore_ascii_case(k)));
            if comp_ok && label_ok && kind_ok && exists(&rule.repo) {
                return Some(rule.repo.clone());
            }
        }
    }
    project.repos.iter().find(|r| r.primary).or_else(|| project.repos.first()).map(|r| r.id.clone())
}

/// `{type}` from `worktree.type_map` (issue type lowercased → value; `default` fallback).
pub fn type_for(settings: &Settings, kind: Option<&str>) -> String {
    let map = &settings.worktree.type_map;
    kind.map(str::to_lowercase)
        .and_then(|k| map.get(&k).cloned())
        .or_else(|| map.get("default").cloned())
        .unwrap_or_else(|| "chore".to_owned())
}

/// Render `worktree.root` into an absolute path.
pub fn worktree_path(settings: &Settings, ctx: &Ctx) -> Result<PathBuf, KeltaError> {
    let rendered = render(&settings.worktree.root, ctx, Mode::Strict)?;
    let p = tidy_path(&expand_home(&rendered, home_dir().as_deref()));
    if !p.is_absolute() {
        return Err(KeltaError::invalid(format!("worktree.root must be absolute: {}", p.display())));
    }
    Ok(p)
}

/// Validate (and if needed sanitize) a branch name with `git check-ref-format --branch`.
pub async fn valid_branch(repo: &Path, raw: &str) -> Result<String, KeltaError> {
    match git::check_branch_name(repo, raw).await {
        Ok(b) => Ok(b),
        Err(_) => git::check_branch_name(repo, &git::sanitize_branch(raw)).await,
    }
}

/// Branch collision: `Some` when the local branch exists (choice defaults to Reuse).
pub async fn branch_exists(repo: &Path, branch: &str) -> Result<Option<BranchExists>, KeltaError> {
    if !git::local_branch_exists(repo, branch).await? {
        return Ok(None);
    }
    let has_worktree = git::worktree_for_branch(repo, branch).await?.is_some();
    Ok(Some(BranchExists { has_worktree, choice: BranchChoice::Reuse }))
}

/// First free `<branch>-N` / `<path>-N` pair (N ≥ 2).
pub async fn suffixed(repo: &Path, branch: &str, path: &Path) -> Result<(String, PathBuf), KeltaError> {
    for n in 2..100 {
        let b = format!("{branch}-{n}");
        let p = PathBuf::from(format!("{}-{n}", path.to_string_lossy()));
        if !git::local_branch_exists(repo, &b).await? && !p.exists() {
            return Ok((b, p));
        }
    }
    Err(KeltaError::conflict(format!("no free suffix for branch {branch}")))
}

/// Local branch for a review (`kelta/pr-<n>` / `kelta/mr-<iid>`).
pub fn review_branch(kind: CodeHostKind, number: u64) -> String {
    match kind {
        CodeHostKind::Github | CodeHostKind::Bitbucket | CodeHostKind::Gitea | CodeHostKind::Plugin => {
            format!("kelta/pr-{number}")
        }
        CodeHostKind::Gitlab => format!("kelta/mr-{number}"),
    }
}

/// Existing (unfinished) work item for the same source.
pub fn existing_for<'a>(items: &'a [WorkItem], source: &WorkSource) -> Option<&'a WorkItem> {
    items.iter().filter(|w| w.state != WorkState::Finished).find(|w| match source {
        WorkSource::Ticket { ticket } => {
            w.ticket.as_ref().is_some_and(|t| t.account == ticket.account && t.key == ticket.key)
        }
        WorkSource::Review { review } => w.review.as_ref() == Some(review),
        WorkSource::Branch { name, .. } => w.ticket.is_none() && w.review.is_none() && &w.branch == name,
    })
}

/// Unfinished own work item behind `pr` (B2): linked by URL, or on its head branch in the repo
/// bound to the PR's repository (a PR Claude opened with `gh`).
pub fn owner_of_pr<'a>(items: &'a [WorkItem], project: &ProjectInfo, pr: &Review) -> Option<&'a WorkItem> {
    let bound = |w: &WorkItem| {
        project
            .repos
            .iter()
            .any(|r| r.id == w.repo_id && r.code_host.as_ref().is_some_and(|c| c.repo == pr.r#ref.repo))
    };
    items
        .iter()
        .filter(|w| w.state != WorkState::Finished && w.review.is_none())
        .find(|w| w.pr_url.as_deref() == Some(pr.url.as_str()) || (w.branch == pr.source_branch && bound(w)))
}

/// Scratch item title: the task's first non-empty line, at most 72 chars.
pub fn task_title(task: &str) -> String {
    let line = task.lines().map(str::trim).find(|l| !l.is_empty()).unwrap_or_default();
    match line.char_indices().nth(72) {
        Some((i, _)) => line[..i].trim_end().to_owned(),
        None => line.to_owned(),
    }
}

/// PR title after a ticket was linked (FLOW §4.3 step 4): `None` when `title` already carries a
/// ticket key (`key` itself or a `reviews.ticket_key_regex` match), else `KEY: title`.
pub fn pr_title_with_key(title: &str, key: &str, key_regex: &str) -> Option<String> {
    let has_key = title.contains(key) || regex::Regex::new(key_regex).is_ok_and(|re| re.is_match(title));
    (!has_key).then(|| format!("{key}: {}", title.trim()))
}

/// Default side effects (status_map overrides `work.on_start`).
pub fn side_effects(settings: &Settings, project: &ProjectInfo, ticket: bool, ctx: &Ctx) -> SideEffects {
    let on = &settings.work.on_start;
    let start = project.tracker.as_ref().and_then(|t| t.status_map.start.clone());
    SideEffects {
        assign_me: ticket && on.assign_me,
        transition_to: if ticket { start.or_else(|| on.transition_to.clone()) } else { None },
        comment: if ticket {
            on.comment
                .as_ref()
                .and_then(|c| render(c, ctx, Mode::Lenient).ok())
                .filter(|c| !c.trim().is_empty())
        } else {
            None
        },
        run_setup: !settings.worktree.setup.is_empty(),
    }
}

/// Claude part of the plan for `profile` with the rendered prompt.
pub fn claude_plan(settings: &Settings, profile: &str, prompt_key: &str, ctx: &Ctx) -> ClaudePlan {
    let p = settings.claude.profiles.get(profile).cloned().unwrap_or_default();
    let template = settings.claude.prompt_templates.get(prompt_key).cloned().unwrap_or_default();
    ClaudePlan {
        profile: profile.to_owned(),
        model: p.model,
        effort: p.effort,
        permission_mode: p.permission_mode,
        prompt: render(&template, ctx, Mode::Lenient).unwrap_or(template),
    }
}

/// Assemble a plan from its parts (shared by the three sources).
#[allow(clippy::too_many_arguments)]
pub fn assemble(
    project: &ProjectInfo,
    source: WorkSource,
    repo: &RepoInfo,
    base: String,
    branch: String,
    branch_exists: Option<BranchExists>,
    worktree_path: PathBuf,
    template_id: String,
    claude: ClaudePlan,
    side_effects: SideEffects,
    existing: Option<&WorkItem>,
) -> StartWorkPlan {
    StartWorkPlan {
        project_id: project.id.clone(),
        source,
        repo_id: repo.id.clone(),
        repo_choices: project.repos.iter().map(|r| r.id.clone()).collect(),
        base,
        branch,
        branch_exists,
        worktree_path,
        template_id,
        claude,
        side_effects,
        existing: existing.map(|w| w.id.clone()),
        adopt_pr: None,
    }
}

/// First block of `PORT_BLOCK` ports in `range` (`first-last`) overlapping no `taken` block whose
/// first port `free` accepts. `Ok(None)` = ports off (empty range).
pub fn alloc_ports(
    range: &str,
    taken: &[u16],
    free: impl Fn(u16) -> bool,
) -> Result<Option<u16>, KeltaError> {
    let range = range.trim();
    if range.is_empty() {
        return Ok(None);
    }
    let bad = || KeltaError::invalid(format!("ports.range `{range}` must be `first-last`, e.g. 20000-29999"));
    let (a, b) = range.split_once('-').ok_or_else(bad)?;
    let first: u16 = a.trim().parse().map_err(|_| bad())?;
    let last: u16 = b.trim().parse().map_err(|_| bad())?;
    (first..=last.saturating_sub(PORT_BLOCK - 1))
        .step_by(PORT_BLOCK.into())
        .find(|&p| taken.iter().all(|t| t.abs_diff(p) >= PORT_BLOCK) && free(p))
        .map(Some)
        .ok_or_else(|| {
            KeltaError::conflict(format!(
                "no free block of {PORT_BLOCK} ports in {range}: finish a work item or widen ports.range"
            ))
        })
}

/// Nothing listens on `port` (any address or loopback).
pub fn port_free(port: u16) -> bool {
    ["0.0.0.0", "127.0.0.1"].iter().all(|h| std::net::TcpListener::bind((*h, port)).is_ok())
}

#[cfg(test)]
mod tests {
    use super::*;
    use kelta_proto::samples;

    #[test]
    fn repo_rules_and_types() {
        let mut p = samples::project_info();
        let mut web = p.repos[0].clone();
        web.id = "web".into();
        web.primary = false;
        p.repos.push(web);
        let rules = samples::tracker_binding().repo_rules;
        let mut t =
            TicketSnap { key: "SHOP-1".into(), labels: vec!["Frontend".into()], ..Default::default() };
        assert_eq!(select_repo(&p, &rules, Some(&t), None).as_deref(), Some("web"));
        t.labels.clear();
        assert_eq!(select_repo(&p, &rules, Some(&t), None).as_deref(), Some("api"));
        assert_eq!(select_repo(&p, &rules, Some(&t), Some("web")).as_deref(), Some("web"));
        let s = Settings::default();
        assert_eq!(type_for(&s, Some("Bug")), "fix");
        assert_eq!(type_for(&s, Some("Epic")), "chore");
        assert_eq!(review_branch(CodeHostKind::Gitlab, 7), "kelta/mr-7");
    }

    #[test]
    fn own_pr_matches_by_url_or_head_branch() {
        let project = samples::project_info();
        let mut pr = samples::review();
        pr.source_branch = "feat/x".into();
        let mut item = samples::work_item();
        item.repo_id = project.repos[0].id.clone();
        item.review = None;
        item.pr_url = None;
        item.branch = "feat/y".into();
        assert!(owner_of_pr(std::slice::from_ref(&item), &project, &pr).is_none());
        item.branch = "feat/x".into();
        let bound = project.repos[0].code_host.as_ref().map(|c| c.repo.clone());
        assert_eq!(bound.as_deref(), Some(pr.r#ref.repo.as_str()), "sample repo is bound to the PR repo");
        assert!(owner_of_pr(std::slice::from_ref(&item), &project, &pr).is_some());
        item.branch = "other".into();
        item.pr_url = Some(pr.url.clone());
        assert!(owner_of_pr(std::slice::from_ref(&item), &project, &pr).is_some());
        item.state = WorkState::Finished;
        assert!(owner_of_pr(std::slice::from_ref(&item), &project, &pr).is_none());
    }

    #[test]
    fn port_blocks() {
        let all = |_| true;
        assert_eq!(alloc_ports("", &[], all).unwrap(), None);
        assert_eq!(alloc_ports("20000-20029", &[], all).unwrap(), Some(20000));
        assert_eq!(alloc_ports("20000-20029", &[20000], all).unwrap(), Some(20010));
        // A block left by another range still overlaps.
        assert_eq!(alloc_ports("20000-20029", &[20005], all).unwrap(), Some(20020));
        // Reuse: a freed block is handed out again.
        assert_eq!(alloc_ports("20000-20029", &[20000, 20020], all).unwrap(), Some(20010));
        // Bind probe: a busy first port skips the block.
        assert_eq!(alloc_ports("20000-20029", &[], |p| p != 20000).unwrap(), Some(20010));
        // Exhaustion, including a range smaller than one block.
        let e = alloc_ports("20000-20029", &[20000, 20010, 20020], all).unwrap_err();
        assert!(e.message.contains("no free block"), "{}", e.message);
        assert!(alloc_ports("20000-20005", &[], all).is_err());
        assert!(alloc_ports("lots", &[], all).is_err());
    }

    #[test]
    fn task_titles() {
        assert_eq!(task_title("\n  Fix the login flake  \nmore"), "Fix the login flake");
        assert_eq!(task_title(""), "");
        let long = "é".repeat(80);
        assert_eq!(task_title(&long).chars().count(), 72);
    }

    #[test]
    fn pr_title_key_rule() {
        let re = &Settings::default().reviews.ticket_key_regex;
        assert_eq!(
            pr_title_with_key("Speed up search", "SHOP-7", re).as_deref(),
            Some("SHOP-7: Speed up search")
        );
        assert_eq!(pr_title_with_key("SHOP-7: Speed up search", "SHOP-7", re), None);
        assert_eq!(pr_title_with_key("OPS-2 speed up search", "SHOP-7", re), None, "any key counts");
        assert_eq!(pr_title_with_key("Fix #12", "SHOP-7", re), None);
        assert_eq!(pr_title_with_key("Speed up", "acme/web#3", "["), Some("acme/web#3: Speed up".into()));
    }
}

//! `project_detect` (SPEC §2): git roots, remotes → code-host hints, tracker hints. Reads the
//! `.git` directory directly (no git process).

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use kelta_proto::ids::{AccountId, ProjectId};
use kelta_proto::model::{CodeHostHint, ProjectDraft, RepoDraft, TrackerHint};
use kelta_proto::settings::{AccountConfig, AccountKind, CodeHostBinding};

/// The git directory of a work tree (`.git` dir, or the `gitdir:` of a `.git` file).
pub fn git_dir(repo: &Path) -> Option<PathBuf> {
    let dot = repo.join(".git");
    if dot.is_dir() {
        return Some(dot);
    }
    let text = std::fs::read_to_string(&dot).ok()?;
    let rel = text.lines().find_map(|l| l.strip_prefix("gitdir:"))?.trim();
    let p = Path::new(rel);
    Some(if p.is_absolute() { p.to_path_buf() } else { repo.join(p) })
}

/// For linked worktrees the shared files (config, refs) live in the common dir.
pub fn common_dir(git_dir: &Path) -> PathBuf {
    std::fs::read_to_string(git_dir.join("commondir"))
        .ok()
        .map(|s| {
            let p = PathBuf::from(s.trim());
            if p.is_absolute() { p } else { git_dir.join(p) }
        })
        .unwrap_or_else(|| git_dir.to_path_buf())
}

/// `[remote "<name>"] url = …` entries of `.git/config`, in file order.
pub fn remotes(common: &Path) -> Vec<(String, String)> {
    let Ok(text) = std::fs::read_to_string(common.join("config")) else { return Vec::new() };
    let mut out = Vec::new();
    let mut current: Option<String> = None;
    for line in text.lines() {
        let l = line.trim();
        if l.starts_with('[') {
            current = l.strip_prefix("[remote \"").and_then(|r| r.strip_suffix("\"]")).map(str::to_owned);
            continue;
        }
        if let Some(name) = &current
            && let Some((k, v)) = l.split_once('=')
            && k.trim() == "url"
        {
            out.push((name.clone(), v.trim().to_owned()));
        }
    }
    out
}

/// Default branch of `remote` (`refs/remotes/<remote>/HEAD`), else `main`/`master` if present.
pub fn default_base(common: &Path, remote: &str) -> String {
    if let Ok(s) = std::fs::read_to_string(common.join("refs/remotes").join(remote).join("HEAD"))
        && let Some(r) = s.trim().strip_prefix(&format!("ref: refs/remotes/{remote}/"))
    {
        return r.to_owned();
    }
    let names = branch_names(common);
    if !names.contains(&"main".to_owned()) && names.contains(&"master".to_owned()) {
        return "master".into();
    }
    "main".into()
}

/// Local branch names (`refs/heads/**` + `packed-refs`).
pub fn branch_names(common: &Path) -> Vec<String> {
    let mut out = BTreeSet::new();
    fn walk(base: &Path, dir: &Path, out: &mut BTreeSet<String>, depth: u32) {
        if depth > 6 {
            return;
        }
        let Ok(rd) = std::fs::read_dir(dir) else { return };
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() {
                walk(base, &p, out, depth + 1);
            } else if let Ok(rel) = p.strip_prefix(base) {
                out.insert(rel.to_string_lossy().replace('\\', "/"));
            }
        }
    }
    let heads = common.join("refs/heads");
    walk(&heads, &heads, &mut out, 0);
    if let Ok(text) = std::fs::read_to_string(common.join("packed-refs")) {
        for l in text.lines() {
            if let Some((_, r)) = l.split_once(' ')
                && let Some(b) = r.strip_prefix("refs/heads/")
            {
                out.insert(b.to_owned());
            }
        }
    }
    out.into_iter().collect()
}

/// `(host, path)` of a remote URL: `git@github.com:acme/shop.git`, `https://gitlab.x/g/s/p.git`,
/// `ssh://git@host:22/a/b.git`.
pub fn parse_remote(url: &str) -> Option<(String, String)> {
    let url = url.trim();
    let (host, path) = if let Some((_, rest)) = url.split_once("://") {
        let rest = rest.rsplit_once('@').map_or(rest, |(_, r)| r);
        let (hostport, path) = rest.split_once('/')?;
        (hostport.split(':').next()?.to_owned(), path.to_owned())
    } else {
        let rest = url.rsplit_once('@').map_or(url, |(_, r)| r);
        let (host, path) = rest.split_once(':')?;
        (host.to_owned(), path.to_owned())
    };
    let path = path.trim_end_matches('/').trim_end_matches(".git").trim_matches('/').to_owned();
    (!host.is_empty() && !path.is_empty()).then_some((host.to_lowercase(), path))
}

fn host_of(url: &str) -> Option<String> {
    let rest = url.split_once("://").map_or(url, |(_, r)| r);
    let host = rest.split(['/', ':']).next()?.to_lowercase();
    Some(host.strip_prefix("api.").map(str::to_owned).unwrap_or(host))
}

/// Configured account of `kind` whose base/web URL host matches `host` (`api.github.com` ≈ `github.com`).
pub fn account_for_host(
    accounts: &BTreeMap<AccountId, AccountConfig>,
    kinds: &[AccountKind],
    host: &str,
) -> Option<AccountId> {
    let want = host.strip_prefix("api.").unwrap_or(host);
    accounts
        .iter()
        .filter(|(_, a)| kinds.contains(&a.kind))
        .find(|(_, a)| {
            [a.effective_base_url(), a.web_url.clone()]
                .into_iter()
                .flatten()
                .any(|u| host_of(&u).as_deref() == Some(want))
        })
        .map(|(id, _)| id.clone())
}

/// Slug `[a-z0-9-]{1,40}` from a folder name.
pub fn slug(name: &str) -> String {
    let mut s = String::new();
    for c in name.chars().flat_map(char::to_lowercase) {
        if c.is_ascii_alphanumeric() {
            s.push(c);
        } else if !s.ends_with('-') {
            s.push('-');
        }
    }
    let s: String = s.trim_matches('-').chars().take(40).collect();
    let s = s.trim_end_matches('-').to_owned();
    if s.is_empty() || s == ProjectId::HOME || s == ProjectId::INBOX {
        format!("project-{s}").trim_end_matches('-').to_owned()
    } else {
        s
    }
}

/// Jira-like keys (`SHOP-123`) in branch names → project key prefixes, most frequent first.
pub fn jira_keys(branches: &[String]) -> Vec<String> {
    let Ok(re) = regex::Regex::new(r"([A-Z][A-Z0-9]+)-\d+") else { return Vec::new() };
    let mut count: BTreeMap<String, usize> = BTreeMap::new();
    for b in branches {
        for c in re.captures_iter(b) {
            if let Some(m) = c.get(1) {
                *count.entry(m.as_str().to_owned()).or_default() += 1;
            }
        }
    }
    let mut v: Vec<_> = count.into_iter().collect();
    v.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    v.into_iter().map(|(k, _)| k).collect()
}

/// Repositories under `path`: the folder itself if it is a work tree, else its direct children.
pub fn find_repos(path: &Path) -> Vec<PathBuf> {
    if git_dir(path).is_some() {
        return vec![path.to_path_buf()];
    }
    let mut out: Vec<PathBuf> = std::fs::read_dir(path)
        .map(|rd| rd.flatten().map(|e| e.path()).filter(|p| p.is_dir() && git_dir(p).is_some()).collect())
        .unwrap_or_default();
    out.sort();
    out
}

/// Build a draft for `path`.
pub fn detect(
    path: &Path,
    accounts: &BTreeMap<AccountId, AccountConfig>,
    existing: &BTreeSet<ProjectId>,
) -> ProjectDraft {
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "project".into());
    let base_slug = slug(&name);
    let mut suggested = base_slug.clone();
    let mut n = 2;
    while existing.contains(&ProjectId::new(&suggested)) {
        let suffix = format!("-{n}");
        let keep = 40usize.saturating_sub(suffix.len());
        suggested = format!("{}{suffix}", base_slug.chars().take(keep).collect::<String>());
        n += 1;
    }

    let mut repos = Vec::new();
    let mut code_host_hints = Vec::new();
    let mut tracker_hints = Vec::new();
    let mut branches_all = Vec::new();
    let mut used_ids = BTreeSet::new();
    for (i, repo) in find_repos(path).into_iter().enumerate() {
        let Some(gd) = git_dir(&repo) else { continue };
        let common = common_dir(&gd);
        let remotes = remotes(&common);
        let (remote, remote_url) = remotes
            .iter()
            .find(|(n, _)| n == "origin")
            .or_else(|| remotes.first())
            .map(|(n, u)| (n.clone(), Some(u.clone())))
            .unwrap_or_else(|| ("origin".into(), None));
        let base = default_base(&common, &remote);
        let mut id = slug(&repo.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default());
        if id.is_empty() || used_ids.contains(&id) {
            id = format!("repo-{}", i + 1);
        }
        used_ids.insert(id.clone());
        let mut code_host = None;
        if let Some((host, rpath)) = remote_url.as_deref().and_then(parse_remote) {
            let kind = if host == "github.com" || host.contains("github") {
                Some(("github", AccountKind::Github))
            } else if host.contains("gitlab") {
                Some(("gitlab", AccountKind::Gitlab))
            } else if host == "bitbucket.org" {
                Some(("bitbucket", AccountKind::Bitbucket))
            } else if host.contains("gitea") || host.contains("forgejo") || host == "codeberg.org" {
                Some(("gitea", AccountKind::Gitea))
            } else {
                None
            };
            let account = account_for_host(
                accounts,
                &[AccountKind::Github, AccountKind::Gitlab, AccountKind::Bitbucket, AccountKind::Gitea],
                &host,
            );
            let kind_name = match (&kind, &account) {
                (Some((k, _)), _) => Some((*k).to_owned()),
                (None, Some(a)) => accounts.get(a).map(|c| match c.kind {
                    AccountKind::Gitlab => "gitlab".into(),
                    AccountKind::Bitbucket => "bitbucket".into(),
                    AccountKind::Gitea => "gitea".into(),
                    _ => "github".into(),
                }),
                _ => None,
            };
            if let Some(k) = kind_name {
                if let Some(a) = &account {
                    code_host = Some(CodeHostBinding { account: a.clone(), repo: rpath.clone() });
                }
                code_host_hints.push(CodeHostHint {
                    repo_id: id.clone(),
                    kind: k.clone(),
                    host: host.clone(),
                    repo: rpath.clone(),
                    account: account.clone(),
                });
                if k == "github" && repo.join(".github").is_dir() {
                    tracker_hints.push(TrackerHint {
                        kind: "github".into(),
                        reason: format!("{rpath} has a .github folder (GitHub Issues)"),
                        key: Some(rpath.clone()),
                        account: account.clone(),
                    });
                } else if k == "gitlab" || k == "gitea" {
                    let label = if k == "gitlab" { "GitLab" } else { "Gitea" };
                    tracker_hints.push(TrackerHint {
                        kind: k.clone(),
                        reason: format!("{rpath} is hosted on {host} ({label} Issues)"),
                        key: Some(rpath.clone()),
                        account: account.clone(),
                    });
                }
            }
        }
        branches_all.extend(branch_names(&common));
        repos.push(RepoDraft {
            id,
            path: repo.clone(),
            primary: i == 0,
            remote,
            base,
            remote_url,
            code_host,
        });
    }
    if let Some(key) = jira_keys(&branches_all).into_iter().next() {
        let jira: Vec<&AccountId> =
            accounts.iter().filter(|(_, a)| a.kind == AccountKind::Jira).map(|(id, _)| id).collect();
        tracker_hints.insert(
            0,
            TrackerHint {
                kind: "jira".into(),
                reason: format!("branch names contain {key}-<n>"),
                key: Some(key),
                account: (jira.len() == 1).then(|| jira[0].clone()),
            },
        );
    }
    ProjectDraft {
        suggested_id: ProjectId::new(suggested),
        name,
        color: None,
        icon: None,
        repos,
        code_host_hints,
        tracker_hints,
        tracker: None,
        default_template: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn remotes_parse() {
        assert_eq!(
            parse_remote("git@github.com:acme/shop.git"),
            Some(("github.com".into(), "acme/shop".into()))
        );
        assert_eq!(
            parse_remote("https://gitlab.acme.dev/grp/sub/proj.git"),
            Some(("gitlab.acme.dev".into(), "grp/sub/proj".into()))
        );
        assert_eq!(parse_remote("ssh://git@host.x:2222/a/b.git"), Some(("host.x".into(), "a/b".into())));
        assert_eq!(parse_remote("nonsense"), None);
    }

    #[test]
    fn slugs_and_keys() {
        assert_eq!(slug("My Shop_API"), "my-shop-api");
        assert_eq!(slug("home"), "project-home");
        assert_eq!(
            jira_keys(&["feat/SHOP-1-x".into(), "fix/SHOP-2".into(), "OPS-3".into()]),
            vec!["SHOP", "OPS"]
        );
    }
}

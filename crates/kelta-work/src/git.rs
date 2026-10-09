//! Git through the CLI (never libgit2): non-interactive environment, bounded runtime, process-group
//! kill on timeout.
//!
//! Every invocation gets `GIT_TERMINAL_PROMPT=0`, `GIT_SSH_COMMAND="ssh -o BatchMode=yes"` (unless the
//! user set one), `SSH_ASKPASS_REQUIRE=never`, `GCM_INTERACTIVE=never`, `LC_ALL=C`, stdin `/dev/null`,
//! its own process group and a timeout (`timeout` → the whole group is killed, `Timeout` error).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use kelta_proto::error::{ErrorCode, KeltaError};
use tokio::io::AsyncReadExt;
use tokio::process::Command;

/// Default timeout for local git operations.
pub const LOCAL_TIMEOUT: Duration = Duration::from_secs(30);

/// Result of one git invocation.
#[derive(Debug, Clone)]
pub struct Output {
    pub code: i32,
    pub stdout: String,
    pub stderr: String,
}

impl Output {
    pub fn ok(&self) -> bool {
        self.code == 0
    }
}

/// Environment applied to every git child (also used by tests to assert non-interactivity).
pub fn git_env() -> BTreeMap<&'static str, String> {
    let mut env = BTreeMap::new();
    env.insert("GIT_TERMINAL_PROMPT", "0".to_owned());
    env.insert("SSH_ASKPASS_REQUIRE", "never".to_owned());
    env.insert("GCM_INTERACTIVE", "never".to_owned());
    env.insert("GIT_OPTIONAL_LOCKS", "0".to_owned());
    env.insert("LC_ALL", "C".to_owned());
    let ssh = std::env::var("GIT_SSH_COMMAND").ok().filter(|s| !s.trim().is_empty());
    env.insert(
        "GIT_SSH_COMMAND",
        match ssh {
            Some(user) if user.contains("BatchMode") => user,
            Some(user) => format!("{user} -o BatchMode=yes"),
            None => "ssh -o BatchMode=yes".to_owned(),
        },
    );
    env
}

/// Run `git <args>` in `cwd` with `timeout`.
pub async fn run(cwd: &Path, args: &[&str], timeout: Duration) -> Result<Output, KeltaError> {
    let mut cmd = Command::new("git");
    cmd.args(args)
        .current_dir(cwd)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .process_group(0);
    for (k, v) in git_env() {
        cmd.env(k, v);
    }
    // Never let a GUI askpass pop up.
    cmd.env_remove("GIT_ASKPASS").env_remove("SSH_ASKPASS");
    let mut child = cmd.spawn().map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            KeltaError::not_found("`git` not found in PATH")
        } else {
            KeltaError::internal(format!("git spawn failed: {e}"))
        }
    })?;
    let pid = child.id();
    let mut out = child.stdout.take();
    let mut err = child.stderr.take();
    let work = async {
        let mut so = Vec::new();
        let mut se = Vec::new();
        let read_out = async {
            if let Some(o) = out.as_mut() {
                let _ = o.read_to_end(&mut so).await;
            }
        };
        let read_err = async {
            if let Some(e) = err.as_mut() {
                let _ = e.read_to_end(&mut se).await;
            }
        };
        tokio::join!(read_out, read_err);
        let status = child.wait().await;
        (status, so, se)
    };
    // one-shot: per-invocation deadline (armed by this git call, dropped when it finishes).
    match tokio::time::timeout(timeout, work).await {
        Ok((status, so, se)) => {
            let status = status.map_err(|e| KeltaError::internal(format!("git wait failed: {e}")))?;
            Ok(Output {
                code: status.code().unwrap_or(-1),
                stdout: String::from_utf8_lossy(&so).into_owned(),
                stderr: String::from_utf8_lossy(&se).into_owned(),
            })
        }
        Err(_) => {
            if let Some(pid) = pid.and_then(|p| rustix::process::Pid::from_raw(p as i32)) {
                let _ = rustix::process::kill_process_group(pid, rustix::process::Signal::KILL);
            }
            Err(KeltaError::timeout(format!("git {} timed out after {}s", args.join(" "), timeout.as_secs())))
        }
    }
}

/// Run and require exit 0 (stderr in the error message).
pub async fn run_ok(cwd: &Path, args: &[&str], timeout: Duration) -> Result<Output, KeltaError> {
    let out = run(cwd, args, timeout).await?;
    if out.ok() {
        Ok(out)
    } else {
        Err(KeltaError::new(
            ErrorCode::Upstream,
            format!("git {} failed: {}", args.join(" "), out.stderr.trim()),
        )
        .with_detail(serde_json::json!({ "code": out.code, "stderr": out.stderr.trim() })))
    }
}

/// `git check-ref-format --branch <name>`; returns the normalized name.
pub async fn check_branch_name(cwd: &Path, name: &str) -> Result<String, KeltaError> {
    if name.is_empty() || name.starts_with('-') {
        return Err(KeltaError::invalid(format!("invalid branch name `{name}`")));
    }
    let out = run(cwd, &["check-ref-format", "--branch", name], LOCAL_TIMEOUT).await?;
    if out.ok() {
        Ok(out.stdout.trim().to_owned())
    } else {
        Err(KeltaError::invalid(format!("invalid branch name `{name}`")))
    }
}

/// Make an arbitrary string a plausible branch name (used before `check_branch_name`).
pub fn sanitize_branch(name: &str) -> String {
    let mut s: String =
        name.chars().map(|c| if c.is_control() || " ~^:?*[\\".contains(c) { '-' } else { c }).collect();
    while s.contains("..") {
        s = s.replace("..", ".");
    }
    while s.contains("//") {
        s = s.replace("//", "/");
    }
    s = s.replace("@{", "-");
    let s = s.trim_matches(|c| c == '/' || c == '.' || c == '-');
    let s = s.strip_suffix(".lock").unwrap_or(s);
    s.split('/').map(|p| p.trim_start_matches('.')).collect::<Vec<_>>().join("/")
}

pub async fn local_branch_exists(repo: &Path, branch: &str) -> Result<bool, KeltaError> {
    let r = format!("refs/heads/{branch}");
    Ok(run(repo, &["show-ref", "--verify", "--quiet", &r], LOCAL_TIMEOUT).await?.ok())
}

pub async fn ref_exists(repo: &Path, r: &str) -> Result<bool, KeltaError> {
    let spec = format!("{r}^{{commit}}");
    Ok(run(repo, &["rev-parse", "--verify", "--quiet", &spec], LOCAL_TIMEOUT).await?.ok())
}

/// One entry of `git worktree list --porcelain`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Worktree {
    pub path: PathBuf,
    pub head: Option<String>,
    /// Short branch name (`refs/heads/` stripped).
    pub branch: Option<String>,
    pub bare: bool,
    pub prunable: bool,
}

pub fn parse_worktrees(porcelain: &str) -> Vec<Worktree> {
    let mut out = Vec::new();
    let mut cur: Option<Worktree> = None;
    for line in porcelain.lines() {
        if let Some(p) = line.strip_prefix("worktree ") {
            if let Some(w) = cur.take() {
                out.push(w);
            }
            cur = Some(Worktree {
                path: PathBuf::from(p),
                head: None,
                branch: None,
                bare: false,
                prunable: false,
            });
        } else if let Some(w) = cur.as_mut() {
            if let Some(h) = line.strip_prefix("HEAD ") {
                w.head = Some(h.to_owned());
            } else if let Some(b) = line.strip_prefix("branch ") {
                w.branch = Some(b.strip_prefix("refs/heads/").unwrap_or(b).to_owned());
            } else if line == "bare" {
                w.bare = true;
            } else if line.starts_with("prunable") {
                w.prunable = true;
            }
        }
    }
    if let Some(w) = cur {
        out.push(w);
    }
    out
}

pub async fn worktrees(repo: &Path) -> Result<Vec<Worktree>, KeltaError> {
    let out = run_ok(repo, &["worktree", "list", "--porcelain"], LOCAL_TIMEOUT).await?;
    Ok(parse_worktrees(&out.stdout))
}

/// Canonical form for path comparisons (symlinked temp dirs on macOS).
pub fn canon(p: &Path) -> PathBuf {
    std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf())
}

pub fn same_path(a: &Path, b: &Path) -> bool {
    canon(a) == canon(b)
}

/// Worktree registered at `path`, if any.
pub async fn worktree_at(repo: &Path, path: &Path) -> Result<Option<Worktree>, KeltaError> {
    Ok(worktrees(repo).await?.into_iter().find(|w| same_path(&w.path, path)))
}

/// Worktree that has `branch` checked out, if any.
pub async fn worktree_for_branch(repo: &Path, branch: &str) -> Result<Option<Worktree>, KeltaError> {
    Ok(worktrees(repo).await?.into_iter().find(|w| w.branch.as_deref() == Some(branch)))
}

/// `git fetch <remote> <refspec>…` with a timeout.
pub async fn fetch(
    repo: &Path,
    remote: &str,
    refspecs: &[&str],
    timeout: Duration,
) -> Result<(), KeltaError> {
    let mut args = vec!["fetch", "--no-tags", "--quiet", remote];
    args.extend_from_slice(refspecs);
    let out = run(repo, &args, timeout).await?;
    if out.ok() {
        Ok(())
    } else {
        Err(KeltaError::network(format!("git fetch {remote} failed: {}", out.stderr.trim())))
    }
}

/// `git worktree add` (new branch from `start`, or existing `branch` when `start` is `None`).
pub async fn worktree_add(
    repo: &Path,
    path: &Path,
    branch: &str,
    start: Option<&str>,
) -> Result<(), KeltaError> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| KeltaError::internal(format!("cannot create {}: {e}", parent.display())))?;
    }
    let p = path.to_string_lossy().into_owned();
    let mut args: Vec<&str> = vec!["worktree", "add"];
    match start {
        Some(s) => {
            args.extend_from_slice(&["--no-track", "-b", branch, &p, s]);
            run_ok(repo, &args, LOCAL_TIMEOUT).await?;
        }
        None => {
            args.extend_from_slice(&[&p, branch]);
            run_ok(repo, &args, LOCAL_TIMEOUT).await?;
        }
    }
    Ok(())
}

pub async fn worktree_remove(repo: &Path, path: &Path, force: bool) -> Result<(), KeltaError> {
    let p = path.to_string_lossy().into_owned();
    let mut args = vec!["worktree", "remove"];
    if force {
        args.push("--force");
    }
    args.push(&p);
    run_ok(repo, &args, LOCAL_TIMEOUT).await.map(|_| ())
}

pub async fn worktree_prune(repo: &Path) -> Result<(), KeltaError> {
    run_ok(repo, &["worktree", "prune"], LOCAL_TIMEOUT).await.map(|_| ())
}

/// `git status --porcelain` paths (tracked changes + untracked, ignored files excluded).
pub async fn dirty_files(worktree: &Path) -> Result<Vec<String>, KeltaError> {
    let out =
        run_ok(worktree, &["status", "--porcelain=v1", "-z", "--untracked-files=all"], LOCAL_TIMEOUT).await?;
    let mut files = Vec::new();
    let mut it = out.stdout.split('\0').filter(|s| !s.is_empty());
    while let Some(entry) = it.next() {
        if entry.len() < 4 {
            continue;
        }
        let (xy, path) = entry.split_at(3);
        files.push(path.to_owned());
        // Renames/copies carry the source path as the next NUL field.
        if xy.starts_with('R') || xy.starts_with('C') {
            it.next();
        }
    }
    Ok(files)
}

/// Is `path` tracked in the index of `worktree`?
pub async fn is_tracked(worktree: &Path, path: &str) -> Result<bool, KeltaError> {
    Ok(run(worktree, &["ls-files", "--error-unmatch", "--", path], LOCAL_TIMEOUT).await?.ok())
}

/// Commits on HEAD that neither a remote-tracking ref nor the local `base` branch contains
/// (`base` keeps a repo without a remote from counting its whole history).
pub async fn unpushed_count(worktree: &Path, base: &str) -> Result<u32, KeltaError> {
    let base = format!("refs/heads/{base}");
    let args = ["rev-list", "--count", "--ignore-missing", "HEAD", "--not", "--remotes", &base];
    let out = run_ok(worktree, &args, LOCAL_TIMEOUT).await?;
    Ok(out.stdout.trim().parse().unwrap_or(0))
}

/// HEAD's changes are already in `target` (squash/rebase merge): merging HEAD into it is a no-op.
/// shortcut: false when `target` later rewrote the same lines (conflict), the user then forces.
pub async fn changes_merged(worktree: &Path, target: &str) -> Result<bool, KeltaError> {
    let out = run(worktree, &["merge-tree", "--write-tree", target, "HEAD"], LOCAL_TIMEOUT).await?;
    if !out.ok() {
        return Ok(false);
    }
    let tree = run_ok(worktree, &["rev-parse", &format!("{target}^{{tree}}")], LOCAL_TIMEOUT).await?;
    Ok(out.stdout.lines().next() == Some(tree.stdout.trim()))
}

/// `(ahead, behind)` of HEAD relative to `upstream`.
pub async fn ahead_behind(worktree: &Path, upstream: &str) -> Result<(u32, u32), KeltaError> {
    let range = format!("HEAD...{upstream}");
    let out = run_ok(worktree, &["rev-list", "--left-right", "--count", &range], LOCAL_TIMEOUT).await?;
    let mut it = out.stdout.split_whitespace().map(|n| n.parse::<u32>().unwrap_or(0));
    Ok((it.next().unwrap_or(0), it.next().unwrap_or(0)))
}

/// `(files, insertions, deletions)` from the merge base of HEAD and `base` to the working tree, so
/// uncommitted work counts; untracked files count in `files` only. Zeros when there is no merge base.
pub async fn diffstat(worktree: &Path, base: &str) -> Result<(u32, u32, u32), KeltaError> {
    let mb = run(worktree, &["merge-base", base, "HEAD"], LOCAL_TIMEOUT).await?;
    if !mb.ok() {
        return Ok((0, 0, 0));
    }
    let out = run_ok(worktree, &["diff", "--numstat", mb.stdout.trim()], LOCAL_TIMEOUT).await?;
    let (mut files, mut ins, mut del) = (0u32, 0u32, 0u32);
    for line in out.stdout.lines() {
        let mut it = line.split('\t').map(|n| n.parse::<u32>().unwrap_or(0)); // binary files: "-"
        files += 1;
        ins += it.next().unwrap_or(0);
        del += it.next().unwrap_or(0);
    }
    let untracked =
        run_ok(worktree, &["ls-files", "-z", "--others", "--exclude-standard"], LOCAL_TIMEOUT).await?;
    files += untracked.stdout.split('\0').filter(|s| !s.is_empty()).count() as u32;
    Ok((files, ins, del))
}

/// Upstream of HEAD (`origin/feat/x`) if configured.
pub async fn upstream(worktree: &Path) -> Result<Option<String>, KeltaError> {
    let out =
        run(worktree, &["rev-parse", "--abbrev-ref", "--symbolic-full-name", "@{u}"], LOCAL_TIMEOUT).await?;
    Ok(out.ok().then(|| out.stdout.trim().to_owned()).filter(|s| !s.is_empty()))
}

/// `git rev-parse --git-common-dir` as an absolute path.
pub async fn common_dir(repo: &Path) -> Result<PathBuf, KeltaError> {
    let out = run_ok(repo, &["rev-parse", "--git-common-dir"], LOCAL_TIMEOUT).await?;
    let p = PathBuf::from(out.stdout.trim());
    Ok(if p.is_absolute() { p } else { repo.join(p) })
}

/// Append `pattern` to `<common-dir>/info/exclude` unless already present.
pub async fn ensure_excluded(repo: &Path, pattern: &str) -> Result<(), KeltaError> {
    let info = common_dir(repo).await?.join("info");
    std::fs::create_dir_all(&info).map_err(|e| KeltaError::internal(format!("{}: {e}", info.display())))?;
    let file = info.join("exclude");
    let current = std::fs::read_to_string(&file).unwrap_or_default();
    if current.lines().any(|l| l.trim() == pattern) {
        return Ok(());
    }
    let mut text = current;
    if !text.is_empty() && !text.ends_with('\n') {
        text.push('\n');
    }
    text.push_str(pattern);
    text.push('\n');
    std::fs::write(&file, text).map_err(|e| KeltaError::internal(format!("{}: {e}", file.display())))
}

/// `git branch -d|-D <branch>`.
pub async fn delete_branch(repo: &Path, branch: &str, force: bool) -> Result<(), KeltaError> {
    run_ok(repo, &["branch", if force { "-D" } else { "-d" }, branch], LOCAL_TIMEOUT).await.map(|_| ())
}

/// Subject of the last commit on HEAD.
pub async fn last_subject(worktree: &Path) -> Result<String, KeltaError> {
    Ok(run_ok(worktree, &["log", "-1", "--format=%s"], LOCAL_TIMEOUT).await?.stdout.trim().to_owned())
}

/// Untracked paths (non-ignored and ignored), fully-untracked directories collapsed (`dir/`).
pub async fn untracked_candidates(repo: &Path) -> Result<Vec<String>, KeltaError> {
    let mut out = Vec::new();
    for extra in [&[][..], &["--ignored"][..]] {
        let mut args = vec!["ls-files", "-z", "--others", "--exclude-standard", "--directory"];
        args.extend_from_slice(extra);
        let o = run_ok(repo, &args, LOCAL_TIMEOUT).await?;
        out.extend(o.stdout.split('\0').filter(|s| !s.is_empty()).map(str::to_owned));
    }
    out.sort();
    out.dedup();
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn porcelain() {
        let text = "worktree /r\nHEAD abc\nbranch refs/heads/main\n\nworktree /w/x y\nHEAD def\nbranch refs/heads/feat/x\nprunable gitdir file points to non-existent location\n\nworktree /w/d\nHEAD 123\ndetached\n";
        let w = parse_worktrees(text);
        assert_eq!(w.len(), 3);
        assert_eq!(w[1].path, PathBuf::from("/w/x y"));
        assert_eq!(w[1].branch.as_deref(), Some("feat/x"));
        assert!(w[1].prunable);
        assert_eq!(w[2].branch, None);
    }

    #[test]
    fn sanitize() {
        assert_eq!(sanitize_branch("feat/a b..c~d"), "feat/a-b.c-d");
        assert_eq!(sanitize_branch("/.x/y.lock"), "x/y");
    }

    #[test]
    fn env_is_non_interactive() {
        let e = git_env();
        assert_eq!(e["GIT_TERMINAL_PROMPT"], "0");
        assert!(e["GIT_SSH_COMMAND"].contains("BatchMode=yes"));
    }
}

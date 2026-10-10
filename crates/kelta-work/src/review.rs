//! Ready for review (#134): every `Stop` snapshots the worktree to `refs/kelta/wi/<id>/last`,
//! Mark reviewed copies it to `refs/kelta/wi/<id>/reviewed`, and the delta between the two is what
//! Louis has not looked at yet. Both refs (and `notes`, review notes) go on Finish and on the startup prune.

use std::collections::HashSet;
use std::path::Path;

use globset::{Glob, GlobSet, GlobSetBuilder};
use kelta_proto::error::KeltaError;
use kelta_proto::ids::WorkItemId;
use kelta_proto::model::{ReviewDelta, WorkItem};

use crate::WorkService;
use crate::git::{self, LOCAL_TIMEOUT};
use crate::saga::Env;

const PREFIX: &str = "refs/kelta/wi/";

/// `refs/kelta/wi/<id>/<name>` (`last` | `reviewed`).
pub(crate) fn wi_ref(id: &WorkItemId, name: &str) -> String {
    format!("{PREFIX}{id}/{name}")
}

/// `git diff --numstat -z` records as `(added, deleted, path)`; a rename keeps its new path and a
/// binary file (`-`) counts 0 lines.
pub(crate) fn numstat(z: &str) -> Vec<(u32, u32, &str)> {
    let mut out = Vec::new();
    let mut it = z.split('\0');
    while let Some(rec) = it.next() {
        let mut f = rec.splitn(3, '\t');
        let (Some(a), Some(d), Some(path)) = (f.next(), f.next(), f.next()) else { continue };
        // Renames: an empty path, then the old and the new path as their own fields.
        let path = if path.is_empty() { it.nth(1).unwrap_or_default() } else { path };
        out.push((a.parse().unwrap_or(0), d.parse().unwrap_or(0), path));
    }
    out
}

/// A test file by its path (`tests/`, `__tests__/`, `*.test.*`, `*_test.*`, `test_*`...).
fn is_test(path: &str) -> bool {
    let p = path.to_ascii_lowercase();
    let name = p.rsplit('/').next().unwrap_or(&p);
    p.split('/').any(|c| matches!(c, "test" | "tests" | "__tests__" | "spec" | "e2e"))
        || [".test.", ".spec.", "_test.", "_spec."].iter().any(|m| name.contains(m))
        || name.starts_with("test_")
}

/// The chip of a delta: real lines/files/tests, and generated lines apart.
pub(crate) fn shape(entries: &[(u32, u32, &str)], generated: impl Fn(&str) -> bool) -> ReviewDelta {
    let mut d = ReviewDelta::default();
    for &(a, del, path) in entries {
        d.insertions += a;
        d.deletions += del;
        if generated(path) {
            d.generated += a + del;
            continue;
        }
        d.files += 1;
        d.lines += a + del;
        d.tests += u32::from(is_test(path));
    }
    d
}

fn glob_set(globs: &[String]) -> GlobSet {
    let mut b = GlobSetBuilder::new();
    for g in globs {
        match Glob::new(g) {
            Ok(g) => {
                b.add(g);
            }
            Err(e) => tracing::warn!(glob = %g, error = %e, "reviews.ignore_globs entry ignored"),
        }
    }
    b.build().unwrap_or_else(|_| GlobSet::empty())
}

/// Paths that are lockfiles or generated: `ignore_globs` (full path or file name), or the
/// `linguist-generated` / `-diff` attributes.
async fn generated_paths(
    wt: &Path,
    paths: &[&str],
    ignore: &[String],
) -> Result<HashSet<String>, KeltaError> {
    let set = glob_set(ignore);
    let mut out: HashSet<String> = paths
        .iter()
        .filter(|p| set.is_match(p) || set.is_match(p.rsplit('/').next().unwrap_or(p)))
        .map(|p| (*p).to_owned())
        .collect();
    if paths.is_empty() {
        return Ok(out);
    }
    // shortcut: paths go on the command line; a delta of tens of thousands of files would need --stdin.
    let mut args = vec!["check-attr", "-z", "linguist-generated", "diff", "--"];
    args.extend_from_slice(paths);
    let attrs = git::run_ok(wt, &args, LOCAL_TIMEOUT).await?;
    let f: Vec<&str> = attrs.stdout.split('\0').collect();
    for [path, attr, value] in f.as_chunks::<3>().0 {
        if matches!((*attr, *value), ("linguist-generated", "set" | "true") | ("diff", "unset")) {
            out.insert((*path).to_owned());
        }
    }
    Ok(out)
}

/// Deletes the `last` / `reviewed` refs of `id` (Finish), so their objects can be collected.
pub(crate) async fn delete_refs(repo: &Path, id: &WorkItemId) {
    for name in ["last", "reviewed", "notes"] {
        let _ = git::run(repo, &["update-ref", "-d", &wi_ref(id, name)], LOCAL_TIMEOUT).await;
    }
}

/// Startup prune: deletes the refs of every work item not in `keep` (finished or gone).
pub(crate) async fn prune_refs(repo: &Path, keep: &HashSet<String>) -> Result<(), KeltaError> {
    let out = git::run_ok(repo, &["for-each-ref", "--format=%(refname)", PREFIX], LOCAL_TIMEOUT).await?;
    for r in out.stdout.lines() {
        let id = r.strip_prefix(PREFIX).and_then(|s| s.split('/').next()).unwrap_or_default();
        if !keep.contains(id) {
            git::run_ok(repo, &["update-ref", "-d", r], LOCAL_TIMEOUT).await?;
        }
    }
    Ok(())
}

impl WorkService {
    /// What Louis last looked at: the `reviewed` ref, else the merge base with the base branch
    /// (nothing reviewed yet, or the branch was rebased since: `reviewed` holds the old base, so
    /// diffing against it would show upstream code), else HEAD.
    pub(crate) async fn reviewed_base(&self, env: &Env, item: &WorkItem) -> Result<String, KeltaError> {
        let wt = &item.worktree;
        let reviewed = git::rev(wt, &wi_ref(&item.id, "reviewed")).await?;
        let Some(base) = Self::base_ref(env, item).await? else {
            return Ok(match reviewed {
                Some(r) => r,
                None => git::rev(wt, "HEAD").await?.unwrap_or_else(|| "HEAD".into()),
            });
        };
        let merge_base = async |rev: &str| -> Result<Option<String>, KeltaError> {
            let mb = git::run(wt, &["merge-base", &base, rev], LOCAL_TIMEOUT).await?;
            Ok(mb.ok().then(|| mb.stdout.trim().to_owned()))
        };
        let head_mb = merge_base("HEAD").await?;
        if let Some(r) = reviewed
            && (head_mb.is_none() || merge_base(&r).await? == head_mb)
        {
            return Ok(r);
        }
        match head_mb {
            Some(mb) => Ok(mb),
            None => Ok(git::rev(wt, "HEAD").await?.unwrap_or_else(|| "HEAD".into())),
        }
    }

    /// `Stop`: snapshots the worktree to `last`, then the shape of `reviewed..last`; `None` when
    /// nothing changed since Louis's last look.
    pub(crate) async fn stop_delta(
        &self,
        env: &Env,
        item: &WorkItem,
    ) -> Result<Option<ReviewDelta>, KeltaError> {
        let wt = &item.worktree;
        let last = git::snapshot(wt, &wi_ref(&item.id, "last")).await?;
        let from = self.reviewed_base(env, item).await?;
        let quiet = git::run(wt, &["diff", "--quiet", &from, &last], LOCAL_TIMEOUT).await?;
        match quiet.code {
            0 => return Ok(None),
            1 => {}
            _ => {
                return Err(KeltaError::upstream(format!(
                    "git diff {from} {last} failed: {}",
                    quiet.stderr.trim()
                )));
            }
        }
        let out = git::run_ok(wt, &["diff", "--numstat", "-M", "-z", &from, &last], LOCAL_TIMEOUT).await?;
        let entries = numstat(&out.stdout);
        let paths: Vec<&str> = entries.iter().map(|e| e.2).collect();
        let generated = generated_paths(wt, &paths, &env.settings.reviews.ignore_globs).await?;
        Ok(Some(shape(&entries, |p| generated.contains(p))))
    }

    /// Mark reviewed: `reviewed` = `last` (a fresh snapshot when Claude never stopped yet).
    pub(crate) async fn stamp_reviewed(&self, item: &WorkItem) -> Result<(), KeltaError> {
        let wt = &item.worktree;
        let last = wi_ref(&item.id, "last");
        let sha = match git::rev(wt, &last).await? {
            Some(s) => s,
            None => git::snapshot(wt, &last).await?,
        };
        git::run_ok(wt, &["update-ref", &wi_ref(&item.id, "reviewed"), &sha], LOCAL_TIMEOUT).await.map(|_| ())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numstat_reads_renames_and_binaries() {
        let z = "3\t1\tsrc/a.rs\x00-\t-\tlogo.png\x000\t0\t\x00old/b.rs\x00new/b.rs\x002\t2\t\x00x.ts\x00x.test.ts\x00";
        assert_eq!(
            numstat(z),
            vec![(3, 1, "src/a.rs"), (0, 0, "logo.png"), (0, 0, "new/b.rs"), (2, 2, "x.test.ts")]
        );
        assert!(numstat("").is_empty());
    }

    #[test]
    fn shape_keeps_lockfiles_and_generated_apart() {
        let z = "200\t10\tsrc/lib.rs\x00-\t-\tassets/logo.png\x001800\t0\tCargo.lock\x000\t0\t\x00a.rs\x00b.rs\x00\
                 2\t0\ttests/it/main.rs\x005\t5\tui/src/gen/Api.ts\x00";
        let generated = ["Cargo.lock", "ui/src/gen/Api.ts"];
        let d = shape(&numstat(z), |p| generated.contains(&p));
        assert_eq!(
            d,
            ReviewDelta { lines: 212, files: 4, tests: 1, generated: 1810, insertions: 2007, deletions: 15 }
        );
    }

    #[test]
    fn tests_by_path() {
        for p in ["tests/it/a.rs", "ui/src/x.test.ts", "pkg/foo_test.go", "test_api.py", "a/__tests__/b.js"] {
            assert!(is_test(p), "{p}");
        }
        for p in ["src/lib.rs", "src/testing.rs", "attest.rs", "contest/a.rs"] {
            assert!(!is_test(p), "{p}");
        }
    }

    #[test]
    fn ignore_globs_match_nested_lockfiles() {
        let set = glob_set(&["package-lock.json".into(), "*.lock".into(), "[".into()]);
        for p in ["package-lock.json", "ui/package-lock.json", "Cargo.lock", "a/b/yarn.lock"] {
            assert!(set.is_match(p) || set.is_match(p.rsplit('/').next().unwrap()), "{p}");
        }
        assert!(!set.is_match("src/lock.rs"));
    }
}

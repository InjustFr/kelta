//! Shared fixture: a temp "remote" + working repo, FakeCore, FakeTracker, FakeCodeHost, MemWorkStore.
#![allow(dead_code, clippy::unwrap_used, clippy::expect_used)]

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Weak};
use std::time::Duration;

use kelta_proto::api::CoreApi;
use kelta_proto::dirs::Dirs;
use kelta_proto::ids::{AccountId, ProjectId, SessionId};
use kelta_proto::model::{SessionInfo, SessionKind};
use kelta_proto::samples;
use kelta_proto::settings::Settings;
use kelta_proto::testing::{FakeCodeHost, FakeCore, FakeTracker, MemWorkStore};
use kelta_work::WorkService;

pub fn git(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .args(["-c", "commit.gpgsign=false", "-c", "user.name=Kelta Test", "-c", "user.email=test@kelta.dev"])
        .args(["-c", "maintenance.auto=false", "-c", "core.fsync=none"])
        .args(args)
        .current_dir(dir)
        .env("GIT_TERMINAL_PROMPT", "0")
        .output()
        .expect("git runs");
    assert!(out.status.success(), "git {args:?} failed: {}", String::from_utf8_lossy(&out.stderr));
    String::from_utf8_lossy(&out.stdout).trim().to_owned()
}

fn append(path: &Path, text: &str) {
    use std::io::Write;
    std::fs::OpenOptions::new().append(true).open(path).unwrap().write_all(text.as_bytes()).unwrap();
}

pub fn has_git() -> bool {
    Command::new("git").arg("--version").output().is_ok_and(|o| o.status.success())
}

pub struct Fx {
    pub tmp: tempfile::TempDir,
    pub core: Arc<FakeCore>,
    pub store: Arc<MemWorkStore>,
    pub dirs: Dirs,
    pub repo: PathBuf,
    pub remote: PathBuf,
    pub wt_root: PathBuf,
    pub tracker: Arc<FakeTracker>,
    pub host: Arc<FakeCodeHost>,
}

pub fn project() -> ProjectId {
    ProjectId::new("shop")
}

impl Fx {
    pub fn new() -> Self {
        let tmp = tempfile::tempdir().expect("tempdir");
        let root = tmp.path().to_path_buf();
        let remote = root.join("remote.git");
        let repo = root.join("repo");
        std::fs::create_dir_all(&remote).unwrap();
        std::fs::create_dir_all(&repo).unwrap();
        git(&remote, &["init", "-q", "--bare", "-b", "main"]);
        git(&repo, &["init", "-q", "-b", "main"]);
        // Config written directly: every git spawn costs ~15 ms on macOS. No auto-maintenance (it forks
        // two processes after each commit/fetch/push, Kelta's own calls included). Kelta's own git
        // calls (rebase) commit too: give the repo an identity (CI has no global one).
        let quiet = "[maintenance]\n\tauto = false\n[gc]\n\tauto = 0\n[core]\n\tfsync = none\n";
        append(&remote.join("config"), quiet);
        append(
            &repo.join(".git/config"),
            &format!(
                "{quiet}[user]\n\tname = Kelta Test\n\temail = test@kelta.dev\n\
                 [remote \"origin\"]\n\turl = {}\n\tfetch = +refs/heads/*:refs/remotes/origin/*\n\
                 [branch \"main\"]\n\tremote = origin\n\tmerge = refs/heads/main\n",
                remote.display()
            ),
        );
        std::fs::write(repo.join("README.md"), "hello\n").unwrap();
        std::fs::write(repo.join(".gitignore"), ".env\n.env.*\n").unwrap();
        std::fs::write(repo.join(".env"), "SECRET=1\n").unwrap();
        git(&repo, &["add", "."]);
        git(&repo, &["commit", "-q", "-m", "init"]);
        // Review head: a commit only reachable through refs/pull/87/head on the remote.
        std::fs::write(repo.join("feature.txt"), "feature\n").unwrap();
        git(&repo, &["add", "feature.txt"]);
        git(&repo, &["commit", "-q", "-m", "feature"]);
        git(&repo, &["push", "-q", "origin", "HEAD~1:refs/heads/main", "HEAD:refs/pull/87/head"]);
        git(&repo, &["reset", "-q", "--hard", "HEAD~1"]);

        // Kelta dirs contain a space (macOS "Application Support").
        let dirs = Dirs::under(&root.join("kelta home"));
        let wt_root = root.join("wt");
        let core = FakeCore::new();
        let mut p = samples::project_info();
        p.repos[0].path = repo.clone();
        core.add_project(p);
        let tracker = Arc::new(FakeTracker::new());
        core.add_tracker(AccountId::new("jira-acme"), tracker.clone());
        let host = Arc::new(FakeCodeHost::new());
        core.add_code_host(AccountId::new("github-work"), host.clone());
        let fx = Self {
            tmp,
            core,
            store: Arc::new(MemWorkStore::new()),
            dirs,
            repo,
            remote,
            wt_root,
            tracker,
            host,
        };
        fx.settings(|_| {});
        fx
    }

    /// Settings = defaults + test paths + `f`.
    pub fn settings(&self, f: impl FnOnce(&mut Settings)) {
        let mut s = Settings::default();
        s.worktree.root = format!("{}/{{project}}/{{repo}}/{{key}}-{{slug}}", self.wt_root.display());
        // Never run the real `claude --version` from tests.
        s.claude.binary = "kelta-test-no-such-claude".into();
        f(&mut s);
        self.core.set_settings(s);
    }

    pub fn service(&self) -> Arc<WorkService> {
        let weak: Weak<dyn CoreApi> = Arc::downgrade(&(self.core.clone() as Arc<dyn CoreApi>));
        WorkService::new(weak, self.store.clone(), self.dirs.clone())
    }

    pub fn spawned(&self) -> Vec<SessionInfo> {
        self.core.sessions()
    }

    pub fn spawned_of(&self, pred: impl Fn(&SessionKind) -> bool) -> Vec<SessionInfo> {
        self.core.sessions().into_iter().filter(|s| pred(&s.kind)).collect()
    }

    pub fn worktree_count(&self) -> usize {
        git(&self.repo, &["worktree", "list", "--porcelain"])
            .lines()
            .filter(|l| l.starts_with("worktree "))
            .count()
    }

    /// Wait (polling, test-only) until a session matching `pred` exists.
    pub async fn wait_session(&self, pred: impl Fn(&SessionInfo) -> bool) -> SessionId {
        for _ in 0..500 {
            if let Some(s) = self.core.sessions().into_iter().find(|s| pred(s)) {
                return s.id;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        panic!("session never spawned");
    }
}

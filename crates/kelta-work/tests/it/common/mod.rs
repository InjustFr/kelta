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
        .args(args)
        .current_dir(dir)
        .env("GIT_TERMINAL_PROMPT", "0")
        .output()
        .expect("git runs");
    assert!(out.status.success(), "git {args:?} failed: {}", String::from_utf8_lossy(&out.stderr));
    String::from_utf8_lossy(&out.stdout).trim().to_owned()
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
        // Kelta's own git calls (rebase) commit too: give the repo an identity (CI has no global one).
        git(&repo, &["config", "user.name", "Kelta Test"]);
        git(&repo, &["config", "user.email", "test@kelta.dev"]);
        std::fs::write(repo.join("README.md"), "hello\n").unwrap();
        std::fs::write(repo.join(".gitignore"), ".env\n.env.*\n").unwrap();
        git(&repo, &["add", "."]);
        git(&repo, &["commit", "-q", "-m", "init"]);
        git(&repo, &["remote", "add", "origin", remote.to_str().unwrap()]);
        git(&repo, &["push", "-q", "-u", "origin", "main"]);
        std::fs::write(repo.join(".env"), "SECRET=1\n").unwrap();
        // Review head: a commit only reachable through refs/pull/87/head on the remote.
        git(&repo, &["checkout", "-q", "-b", "pr-src"]);
        std::fs::write(repo.join("feature.txt"), "feature\n").unwrap();
        git(&repo, &["add", "feature.txt"]);
        git(&repo, &["commit", "-q", "-m", "feature"]);
        git(&repo, &["push", "-q", "origin", "HEAD:refs/pull/87/head"]);
        git(&repo, &["checkout", "-q", "main"]);
        git(&repo, &["branch", "-q", "-D", "pr-src"]);

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

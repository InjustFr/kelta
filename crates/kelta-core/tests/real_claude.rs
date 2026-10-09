//! Gate C1 (BUILD_PLAN §6): Kelta's launcher against the real `claude` CLI, through a real `Core`
//! (PTY host, ctl socket, installed kelta-ctl hook, loopback MCP, work-saga worktree).
//!
//! Opt-in, two cheap model runs (haiku, effort low):
//!   KELTA_REAL_CLAUDE=1 cargo test -p kelta-core --test real_claude -- --nocapture
//!
//! Claude runs in print mode via Kelta's own `claude.extra_args = ["-p"]`: an interactive session
//! in a fresh, untrusted temp repo stops at Claude's workspace-trust dialog (hooks do not run before
//! it), and accepting it would persist trust in `~/.claude.json`. Shift+Enter (interactive only) is
//! checked by hand. Never edits `~/.claude/settings.json` (asserted unchanged).
#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use std::time::{Duration, Instant};

use common::{Factory, MemConfig};
use kelta_core::clipboard::MemClipboard;
use kelta_core::{Core, CoreDeps};
use kelta_proto::api::CoreApi;
use kelta_proto::dirs::{CliArgs, Dirs};
use kelta_proto::events::{BusEvent, bus};
use kelta_proto::ids::{ProjectId, SessionId};
use kelta_proto::model::{Lifecycle, SessionKind, SessionStatus, WorkSource};
use kelta_proto::settings::{ClaudeEffort, PermissionMode, ProjectConfig, RepoConfig, Settings};
use kelta_proto::term::{LoginEnv, LoginEnvSource};
use kelta_proto::testing::{FakeSecrets, FakeUiBridge};
use parking_lot::Mutex;
use serde_json::Value;

const STEP: Duration = Duration::from_secs(180);

fn workspace() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).ancestors().nth(2).unwrap().to_path_buf()
}

fn git(dir: &Path, args: &[&str]) {
    let out = Command::new("git")
        .args(["-c", "commit.gpgsign=false", "-c", "user.name=Kelta Test", "-c", "user.email=test@kelta.dev"])
        .args(args)
        .current_dir(dir)
        .output()
        .unwrap();
    assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
}

/// `kelta-ctl` of this build, else built into a private target dir.
fn kelta_ctl() -> PathBuf {
    let exe = std::env::current_exe().unwrap();
    let candidate = exe.parent().and_then(Path::parent).unwrap().join("kelta-ctl");
    if candidate.is_file() {
        return candidate;
    }
    let target = workspace().join("target").join("kelta-ctl-e2e");
    let ok = Command::new(env!("CARGO"))
        .args(["build", "-q", "-p", "kelta-ctl", "--target-dir"])
        .arg(&target)
        .current_dir(workspace())
        .status()
        .unwrap()
        .success();
    assert!(ok, "building kelta-ctl failed");
    target.join("debug").join("kelta-ctl")
}

/// The test process env as the login env, minus the vars of an enclosing Claude Code session.
fn login_env() -> LoginEnv {
    let vars: BTreeMap<String, String> = std::env::vars()
        .filter(|(k, _)| k != "CLAUDECODE" && !k.starts_with("CLAUDE_CODE_") && !k.starts_with("KELTA_"))
        .collect();
    let shell = vars.get("SHELL").map(PathBuf::from);
    LoginEnv { vars, source: LoginEnvSource::LoginInteractive, shell }
}

struct Run {
    core: Arc<Core>,
    events: Arc<Mutex<Vec<BusEvent>>>,
}

impl Run {
    fn of(&self, sid: &SessionId, name: &str) -> Vec<Value> {
        self.events
            .lock()
            .iter()
            .filter(|e| e.name == name && e.session_id.as_ref() == Some(sid))
            .map(|e| e.payload.clone())
            .collect()
    }

    fn hooks(&self, sid: &SessionId, event: &str) -> Vec<Value> {
        self.of(sid, bus::CLAUDE_HOOK)
            .into_iter()
            .filter(|p| p["event"] == event)
            .map(|p| p["payload"].clone())
            .collect()
    }

    fn statuses(&self, sid: &SessionId) -> Vec<SessionStatus> {
        let mut out: Vec<SessionStatus> = Vec::new();
        for p in self.of(sid, bus::SESSION_STATUS_CHANGED) {
            let s: SessionStatus = serde_json::from_value(p["status"].clone()).unwrap();
            if out.last() != Some(&s) {
                out.push(s);
            }
        }
        out
    }

    /// Wait for SessionEnd and the PTY exit.
    async fn wait_exit(&self, sid: &SessionId) {
        let start = Instant::now();
        loop {
            let info = self.core.session_get(sid).unwrap();
            if info.lifecycle == Lifecycle::Exited && !self.hooks(sid, "SessionEnd").is_empty() {
                return;
            }
            assert!(
                start.elapsed() < STEP,
                "claude did not finish; statuses {:?}\n--- screen ---\n{}",
                self.statuses(sid),
                self.core.session_text_tail(sid, 40).unwrap_or_default()
            );
            tokio::time::sleep(Duration::from_millis(200)).await;
        }
    }
}

fn is_subsequence(needle: &[SessionStatus], hay: &[SessionStatus]) -> bool {
    let mut it = hay.iter();
    needle.iter().all(|n| it.any(|h| h == n))
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn real_claude_gate_c1() {
    if std::env::var("KELTA_REAL_CLAUDE").as_deref() != Ok("1") || which::which("claude").is_err() {
        eprintln!("skipping: set KELTA_REAL_CLAUDE=1 with `claude` on PATH");
        return;
    }
    let user_settings = std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".claude/settings.json"));
    let user_settings_before = user_settings.as_ref().and_then(|p| std::fs::read(p).ok());

    // Kelta dirs: short path (unix socket limit).
    let home = tempfile::Builder::new().prefix("klc").tempdir_in("/tmp").unwrap();
    let base = workspace().join("target").join("real-claude");
    std::fs::create_dir_all(&base).unwrap();
    let work = tempfile::Builder::new().prefix("c1-").tempdir_in(&base).unwrap();
    let (remote, repo, marker) =
        (work.path().join("remote.git"), work.path().join("repo"), work.path().join("marker"));
    std::fs::create_dir_all(&remote).unwrap();
    std::fs::create_dir_all(repo.join(".claude")).unwrap();
    git(&remote, &["init", "-q", "--bare", "-b", "main"]);
    git(&repo, &["init", "-q", "-b", "main"]);
    std::fs::write(repo.join("README.md"), "c1\n").unwrap();
    // A project-level hook: `--settings` must add Kelta's hooks to it, not replace it.
    let project_hook = serde_json::json!({ "hooks": { "SessionStart": [ { "hooks": [
        { "type": "command", "command": format!("echo project >> '{}'", marker.display()) } ] } ] } });
    std::fs::write(repo.join(".claude/settings.json"), project_hook.to_string()).unwrap();
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-q", "-m", "init"]);
    git(&repo, &["remote", "add", "origin", remote.to_str().unwrap()]);
    git(&repo, &["push", "-q", "-u", "origin", "main"]);

    let mut dirs = Dirs::under(home.path());
    // Like macOS "Application Support": the hook command must survive a space.
    dirs.bin = home.path().join("data dir").join("bin");
    kelta_core::ctl::install_ctl_from(&kelta_ctl(), &dirs.bin, kelta_proto::VERSION).unwrap();

    let mut settings = Settings::default();
    settings.worktree.root = format!("{}/wt/{{project}}/{{repo}}/{{key}}-{{slug}}", work.path().display());
    settings.notifications.quiet_hours = String::new();
    settings.claude.extra_args = vec!["-p".into()];
    let pid = ProjectId::new("c1");
    let project = ProjectConfig {
        id: pid.clone(),
        name: "C1".into(),
        repos: vec![RepoConfig {
            id: "main".into(),
            path: repo.to_string_lossy().into_owned(),
            primary: true,
            ..RepoConfig::default()
        }],
        ..ProjectConfig::default()
    };
    let cfg = MemConfig::new(settings, vec![project]);
    let ui = FakeUiBridge::new();
    let mut deps = CoreDeps::new(dirs.clone(), CliArgs::default(), ui.clone());
    deps.config = Some(cfg.clone());
    deps.secrets = Some(FakeSecrets::new());
    let factory = Arc::new(Factory::default());
    deps.trackers = Some(factory.clone());
    deps.code_hosts = Some(factory);
    deps.login_env = Some(login_env());
    deps.clipboard = Some(Arc::new(MemClipboard::default()));
    deps.in_memory_store = true;
    deps.install_ctl = false;
    let core = Core::start_with(deps).unwrap();
    let events = Arc::new(Mutex::new(Vec::new()));
    let mut rx = CoreApi::subscribe(&*core);
    let sink = events.clone();
    tokio::spawn(async move {
        while let Ok(ev) = rx.recv().await {
            sink.lock().push(ev);
        }
    });
    let start = Instant::now();
    while !dirs.ctl_socket().exists() {
        assert!(start.elapsed() < Duration::from_secs(10), "ctl socket never bound");
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    let r = Run { core: core.clone(), events };

    // 1. Work item → Kelta-created worktree → `claude --session-id <uuid> … -p "<prompt>"` in a PTY.
    //    The MCP call is allowed by `--allowedTools mcp__kelta__*`; the write needs permission
    //    (PermissionRequest fires in print mode too, then it is denied).
    let mut plan = core.work().plan(&pid, WorkSource::Branch { name: "c1-real".into() }).await.unwrap();
    plan.template_id = "claude".into();
    plan.claude.model = "haiku".into();
    plan.claude.effort = ClaudeEffort::Low;
    plan.claude.permission_mode = PermissionMode::Default;
    plan.claude.prompt = "Step 1: call the MCP tool mcp__kelta__notify with message \"c1-mcp-ok\". \
        Step 2: use the Write tool to create the file c1.txt containing hi. Then reply with one word."
        .into();
    let item = core.work().start(plan).await.unwrap();
    let uuid = item.claude_uuid.clone().unwrap();
    let sid = core
        .session_list(Some(&pid))
        .into_iter()
        .find(|s| s.kind == SessionKind::Claude)
        .expect("claude session spawned")
        .id;
    r.wait_exit(&sid).await;

    let started = &r.hooks(&sid, "SessionStart")[0];
    assert_eq!(started["session_id"], uuid.as_str(), "--session-id honoured");
    assert_eq!(started["source"], "startup");
    assert_eq!(
        Path::new(started["cwd"].as_str().unwrap()).canonicalize().unwrap(),
        item.worktree.canonicalize().unwrap(),
        "runs in the Kelta worktree"
    );
    assert!(core.session_get(&sid).unwrap().claude.unwrap().hooks_active, "hooks are the status source");
    let statuses = r.statuses(&sid);
    let expected = [
        SessionStatus::Running,
        SessionStatus::Working,
        SessionStatus::NeedsInput,
        SessionStatus::Done,
        SessionStatus::Exited,
    ];
    assert!(is_subsequence(&expected, &statuses), "statuses {statuses:?}");
    eprintln!("c1: statuses {statuses:?}");
    assert!(
        ui.notifications().iter().any(|n| n.body.as_deref() == Some("c1-mcp-ok")),
        "MCP notify reached Kelta: {:?}",
        ui.notifications()
    );

    // 2. Kelta's restore argv for a work-item Claude (`--resume <uuid>`, files regenerated), still in
    //    the worktree; the resumed conversation remembers step 1.
    cfg.update(|s| {
        s.claude.extra_args = vec![
            "-p".into(),
            "What exact message did you send with the notify tool? Reply with only that message.".into(),
        ];
    });
    let mut req = core.work().claude_restore_request(&sid, false).await.unwrap().expect("work item session");
    let at = req.args.iter().position(|a| a == "--resume").expect("--resume in the restore argv");
    assert_eq!(req.args[at + 1], uuid);
    assert_eq!(req.cwd.as_deref(), Some(item.worktree.as_path()));
    req.id = None;
    let resumed = core.session_spawn(req).await.unwrap().id;
    r.wait_exit(&resumed).await;
    let again = &r.hooks(&resumed, "SessionStart")[0];
    assert_eq!(again["source"], "resume", "{again}");
    assert_eq!(again["session_id"], uuid.as_str(), "same conversation");
    let answer = r.hooks(&resumed, "Stop")[0]["last_assistant_message"].as_str().unwrap_or("").to_owned();
    assert!(answer.contains("c1-mcp-ok"), "resumed conversation lost its context: {answer:?}");
    eprintln!("c1: resumed {uuid} in the worktree, answer {answer:?}");

    // 3. Merge, not replace: the project hook ran on both starts; ~/.claude/settings.json untouched.
    let marks = std::fs::read_to_string(&marker).unwrap_or_default();
    assert_eq!(marks.lines().count(), 2, "project SessionStart hook ran alongside Kelta's");
    let after = user_settings.as_ref().and_then(|p| std::fs::read(p).ok());
    assert!(after == user_settings_before, "~/.claude/settings.json changed");

    let _ = core.shutdown().await;
}

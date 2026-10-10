//! End-to-end: fixtures/fake-claude with Kelta-generated hook settings (PLUGINS §8) → real
//! `kelta-ctl hook` → ctl socket → FakeCore receives Running, Working, NeedsInput, Done and Exited.

use crate::common;

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::{Duration, Instant};

use kelta_proto::events::bus;
use kelta_proto::ids::SessionId;
use kelta_proto::model::SessionStatus;
use kelta_proto::settings::ClaudeSettings;
use kelta_proto::testing::FakeCore;
use serde_json::Value;
use tokio::io::AsyncWriteExt;

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).ancestors().nth(2).unwrap().to_path_buf()
}

/// The `kelta-ctl` binary of this build (built by `cargo test --workspace`); otherwise build it
/// into a private target dir (no lock contention with the running cargo).
fn kelta_ctl() -> PathBuf {
    let exe = std::env::current_exe().unwrap();
    let profile_dir = exe.parent().and_then(Path::parent).unwrap();
    let candidate = profile_dir.join("kelta-ctl");
    if candidate.is_file() {
        return candidate;
    }
    let target = repo_root().join("target").join("kelta-ctl-e2e");
    let status = std::process::Command::new(env!("CARGO"))
        .args(["build", "-q", "-p", "kelta-ctl", "--target-dir"])
        .arg(&target)
        .current_dir(repo_root())
        .status()
        .unwrap();
    assert!(status.success(), "building kelta-ctl failed");
    target.join("debug").join("kelta-ctl")
}

/// POSIX single quotes.
fn sh_quote(p: &Path) -> String {
    format!("'{}'", p.display().to_string().replace('\'', r"'\''"))
}

/// The settings document Kelta generates (command transport), shared with kelta-work.
fn claude_settings(ctl: &Path) -> Value {
    let command = format!("{} hook", sh_quote(ctl));
    kelta_proto::hooks::claude_settings(&command, &ClaudeSettings::default(), None)
}

fn statuses(fake: &FakeCore) -> Vec<SessionStatus> {
    let mut out: Vec<SessionStatus> = Vec::new();
    for (_, c) in fake.hooks() {
        if c.status != SessionStatus::Unknown && out.last() != Some(&c.status) {
            out.push(c.status);
        }
    }
    out
}

async fn wait_for(fake: &FakeCore, status: SessionStatus, within: Duration) {
    let start = Instant::now();
    while !fake.hooks().iter().any(|(_, c)| c.status == status) {
        assert!(start.elapsed() < within, "timed out waiting for {status:?}; got {:?}", statuses(fake));
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn fake_claude_drives_statuses_through_kelta_ctl() {
    let bash = Path::new("/bin/bash");
    if !bash.exists() {
        eprintln!("skipping: /bin/bash not found");
        return;
    }
    let e = common::env();
    let sock = e.server.start_ctl().await.unwrap();
    let sid = SessionId::new("0192aaaa-e2e0-7000-8000-000000000001");
    let token = "e2e-hook-token-0123456789abcdef";
    e.server.register_session(&sid, token, None);

    let sdir = e.tmp.path().join("run").join("s").join("0192aaaa");
    std::fs::create_dir_all(&sdir).unwrap();
    let settings = sdir.join("claude-settings.json");
    std::fs::write(&settings, serde_json::to_vec_pretty(&claude_settings(&kelta_ctl())).unwrap()).unwrap();

    let mut child = tokio::process::Command::new(bash)
        .arg(repo_root().join("fixtures").join("fake-claude"))
        .arg("--settings")
        .arg(&settings)
        .args(["--session-id", "6f1d2c3b-4a59-4e8f-9a0b-1c2d3e4f5a6b", "fix the login"])
        .current_dir(e.tmp.path())
        .env("KELTA_SESSION_ID", sid.as_str())
        .env("KELTA_HOOK_TOKEN", token)
        .env("KELTA_SOCK", &sock)
        .env("FAKE_CLAUDE_STEP_MS", "150")
        .env("FAKE_CLAUDE_NO_IDLE", "1")
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .unwrap();

    wait_for(&e.fake, SessionStatus::Done, Duration::from_secs(20)).await;
    let mut stdin = child.stdin.take().unwrap();
    stdin.write_all(b"q\n").await.unwrap();
    drop(stdin);
    let status = tokio::time::timeout(Duration::from_secs(20), child.wait()).await.unwrap().unwrap();
    assert!(status.success());
    wait_for(&e.fake, SessionStatus::Exited, Duration::from_secs(10)).await;

    // Kelta's hooks are `async: true`: one kelta-ctl process per hook, none awaited, so they can reach
    // the socket in any order (a slow SessionStart lands after UserPromptSubmit): only the set is guaranteed.
    let got: HashSet<SessionStatus> = statuses(&e.fake).into_iter().collect();
    let want = HashSet::from([
        SessionStatus::Running,
        SessionStatus::Working,
        SessionStatus::NeedsInput,
        SessionStatus::Done,
        SessionStatus::Exited,
    ]);
    assert_eq!(got, want, "{:?}", statuses(&e.fake));
    assert!(e.fake.hooks().iter().all(|(s, _)| s == &sid));
    let edited: Vec<Value> = e
        .fake
        .published()
        .into_iter()
        .filter(|b| b.name == bus::CLAUDE_FILE_EDITED)
        .map(|b| b.payload)
        .collect();
    assert_eq!(edited.len(), 1);
    assert_eq!(edited[0]["tool"], "Edit");
    assert!(edited[0]["path"].as_str().unwrap().ends_with("src/main.rs"));
    let done = e.fake.hooks().into_iter().find(|(_, c)| c.status == SessionStatus::Done).unwrap().1;
    assert_eq!(done.preview.as_deref(), Some("Done. All tests pass."));
}

#[tokio::test]
async fn wrong_token_from_kelta_ctl_is_ignored() {
    let e = common::env();
    let sock = e.server.start_ctl().await.unwrap();
    e.server.register_session(&SessionId::new("s"), "right", None);
    let mut child = tokio::process::Command::new(kelta_ctl())
        .arg("hook")
        .env("KELTA_SESSION_ID", "s")
        .env("KELTA_HOOK_TOKEN", "wrong")
        .env("KELTA_SOCK", &sock)
        .stdin(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    stdin.write_all(br#"{"hook_event_name":"SessionStart"}"#).await.unwrap();
    drop(stdin);
    assert!(child.wait().await.unwrap().success());
    assert!(e.fake.hooks().is_empty());
}

//! Quit → Dormant persistence → lazy restore on first attach, for each restore policy; eager and
//! none modes; `--resume` refused → `--continue` fallback.

mod common;

use std::sync::Arc;

use common::*;
use kelta_proto::api::CoreApi;
use kelta_proto::ids::{ProjectId, SessionId};
use kelta_proto::model::{Lifecycle, RestorePolicy, SessionKind, SpawnRequest};
use kelta_proto::settings::{RestoreMode, Settings};
use kelta_proto::term::TerminalEvent;
use kelta_proto::testing::{FakeTerminalHost, RecordingSink};

fn req(kind: SessionKind, program: Option<&str>, args: &[&str], restore: RestorePolicy) -> SpawnRequest {
    SpawnRequest {
        project_id: ProjectId::new("shop"),
        kind,
        name: None,
        program: program.map(str::to_owned),
        args: args.iter().map(|s| (*s).to_owned()).collect(),
        cwd: None,
        env: Default::default(),
        cols: 90,
        rows: 30,
        work_item_id: None,
        restore,
        close_on_exit: Default::default(),
        template_id: None,
    }
}

struct Seeded {
    shell: SessionId,
    claude: SessionId,
    editor: SessionId,
    tool: SessionId,
    setup: SessionId,
}

/// First run: spawn one session per policy, then quit.
async fn seed(root: &std::path::Path) -> Seeded {
    let h = start(root, Settings::defaults(), vec![project("shop", root)]);
    let shell = h.core.session_spawn(req(SessionKind::Shell, None, &[], RestorePolicy::None)).await.unwrap();
    let claude = h
        .core
        .session_spawn(req(SessionKind::Claude, Some("claude"), &["--session-id", "U-1", "-n", "x", "do it"], RestorePolicy::None))
        .await
        .unwrap();
    let editor = h
        .core
        .session_spawn(req(SessionKind::Editor { adapter: "nvim".into() }, Some("nvim"), &["--listen", "/tmp/n.sock", "."], RestorePolicy::None))
        .await
        .unwrap();
    let tool = h
        .core
        .session_spawn(req(SessionKind::Tool { tool_id: "lazygit".into() }, Some("lazygit"), &["-p", "."], RestorePolicy::None))
        .await
        .unwrap();
    let setup = h.core.session_spawn(req(SessionKind::Setup, None, &[], RestorePolicy::None)).await.unwrap();
    // OSC 7 moves the shell
    let cwd = root.join("home");
    h.term.emit(&shell.id, TerminalEvent::Cwd(cwd));
    h.term.set_text_tail(&shell.id, "line1\nline2\nline3");
    // nvim session file written by the quit hook (L6)
    let vim = root.join("data").join("sessions").join(format!("{}.vim", editor.id));
    std::fs::create_dir_all(vim.parent().unwrap()).unwrap();
    std::fs::write(&vim, "\" session").unwrap();
    h.core.shutdown().await.unwrap();
    for id in [&shell.id, &claude.id, &editor.id, &tool.id] {
        assert!(h.term.with_session(id, |s| s.killed).unwrap().is_some(), "quit kills {id}");
    }
    Seeded { shell: shell.id, claude: claude.id, editor: editor.id, tool: tool.id, setup: setup.id }
}

fn mode(m: RestoreMode) -> Settings {
    let mut s = Settings::defaults();
    s.app.restore_mode = m;
    s
}

#[tokio::test]
async fn dormant_sessions_spawn_on_first_attach() {
    let tmp = tempfile::tempdir().unwrap();
    let seeded = seed(tmp.path()).await;
    let term = FakeTerminalHost::new();
    let h = start_in(
        tmp.path(),
        MemConfig::new(mode(RestoreMode::Lazy), vec![project("shop", tmp.path())]),
        term.clone(),
        Arc::new(Factory::default()),
    );
    settle().await;
    let list = h.core.session_list(None);
    assert_eq!(list.len(), 4, "setup sessions are not restored");
    assert!(h.core.session_get(&seeded.setup).is_none());
    assert!(list.iter().all(|s| s.lifecycle == Lifecycle::Dormant));
    assert!(term.spawned_ids().is_empty(), "lazy: nothing spawned before attach");
    assert_eq!(h.core.session_text_tail(&seeded.shell, 2).unwrap(), "line2\nline3");

    // shell → login shell in the last OSC 7 cwd
    let info = h.core.session_attach(&seeded.shell, 132, 43, Box::new(RecordingSink::new())).await.unwrap();
    assert_eq!((info.cols, info.rows), (132, 43));
    let (args, cwd) = term.with_session(&seeded.shell, |s| (s.spec.args.clone(), s.spec.cwd.clone())).unwrap();
    assert_eq!(args, vec!["-l"]);
    assert_eq!(cwd, tmp.path().join("home"));
    assert_eq!(h.core.session_get(&seeded.shell).unwrap().lifecycle, Lifecycle::Live);
    assert_eq!(term.spawned_ids().len(), 1);

    // claude → --resume <uuid>, prompt and session selection dropped
    h.core.session_attach(&seeded.claude, 80, 24, Box::new(RecordingSink::new())).await.unwrap();
    let args = term.with_session(&seeded.claude, |s| s.spec.args.clone()).unwrap();
    assert_eq!(args, vec!["-n", "x", "--resume", "U-1"]);

    // nvim → -S <session file>
    h.core.session_attach(&seeded.editor, 80, 24, Box::new(RecordingSink::new())).await.unwrap();
    let args = term.with_session(&seeded.editor, |s| s.spec.args.clone()).unwrap();
    assert_eq!(args[0], "-S");
    assert!(args[1].ends_with(&format!("{}.vim", seeded.editor)));
    assert_eq!(&args[2..], &["--listen", "/tmp/n.sock", "."]);

    // tool → relaunch as is
    h.core.session_attach(&seeded.tool, 80, 24, Box::new(RecordingSink::new())).await.unwrap();
    assert_eq!(term.with_session(&seeded.tool, |s| s.spec.args.clone()).unwrap(), vec!["-p", "."]);

    // a second attach does not respawn
    let gen1 = term.with_session(&seeded.tool, |s| s.generation).unwrap();
    h.core.session_attach(&seeded.tool, 80, 24, Box::new(RecordingSink::new())).await.unwrap();
    assert_eq!(term.with_session(&seeded.tool, |s| s.generation).unwrap(), gen1 + 1);
    assert_eq!(term.spawned_ids().len(), 4);
}

#[tokio::test]
async fn resume_refused_falls_back_to_continue() {
    let tmp = tempfile::tempdir().unwrap();
    let seeded = seed(tmp.path()).await;
    let term = FakeTerminalHost::new();
    let h = start_in(tmp.path(), MemConfig::new(Settings::defaults(), vec![project("shop", tmp.path())]), term.clone(), Arc::new(Factory::default()));
    h.core.session_attach(&seeded.claude, 80, 24, Box::new(RecordingSink::new())).await.unwrap();
    term.emit(&seeded.claude, TerminalEvent::Exited { code: Some(1), signal: None });
    for _ in 0..50 {
        settle().await;
        if term.with_session(&seeded.claude, |s| s.spec.args.last().cloned()).flatten().as_deref() == Some("--continue") {
            break;
        }
    }
    let args = term.with_session(&seeded.claude, |s| s.spec.args.clone()).unwrap();
    assert_eq!(args.last().map(String::as_str), Some("--continue"));
    assert!(!args.contains(&"--resume".to_owned()));
    assert_eq!(h.core.session_get(&seeded.claude).unwrap().lifecycle, Lifecycle::Live);
}

#[tokio::test]
async fn eager_mode_spawns_at_start() {
    let tmp = tempfile::tempdir().unwrap();
    seed(tmp.path()).await;
    let term = FakeTerminalHost::new();
    let h = start_in(tmp.path(), MemConfig::new(mode(RestoreMode::Eager), vec![project("shop", tmp.path())]), term.clone(), Arc::new(Factory::default()));
    for _ in 0..50 {
        settle().await;
        if term.spawned_ids().len() == 4 {
            break;
        }
    }
    assert_eq!(term.spawned_ids().len(), 4);
    assert!(h.core.session_list(None).iter().all(|s| s.lifecycle == Lifecycle::Live));
}

#[tokio::test]
async fn none_mode_drops_dormant_sessions() {
    let tmp = tempfile::tempdir().unwrap();
    seed(tmp.path()).await;
    let term = FakeTerminalHost::new();
    let h = start_in(tmp.path(), MemConfig::new(mode(RestoreMode::None), vec![project("shop", tmp.path())]), term.clone(), Arc::new(Factory::default()));
    settle().await;
    assert!(h.core.session_list(None).is_empty());
    assert!(term.spawned_ids().is_empty());
}

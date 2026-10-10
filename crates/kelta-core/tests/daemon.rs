//! keltad: a quit + restart of the app re-attaches the still-running session instead of
//! respawning it from Dormant; sessions that are not restored are not left behind, and closed
//! sessions are freed in the host (in-process and keltad).
#![allow(clippy::unwrap_used)] // fixture helpers outside #[test] fns
#![allow(clippy::disallowed_methods)] // allowlisted: tests run keltad on a thread

mod common;

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use common::*;
use kelta_core::{Core, CoreDeps};
use kelta_proto::api::{CoreApi, TerminalHost};
use kelta_proto::dirs::{CliArgs, Dirs};
use kelta_proto::ids::ProjectId;
use kelta_proto::model::{
    CloseOnExit, Lifecycle, RestorePolicy, SessionKind, SessionStatus, SpawnRequest, StatusChange,
};
use kelta_proto::settings::RestoreMode;
use kelta_proto::settings::Settings;
use kelta_proto::term::TerminalLimits;
use kelta_proto::testing::{FakeSecrets, FakeUiBridge, RecordingSink};
use kelta_term::PtyTerminalHost;
use kelta_term::daemon::{self, DaemonTerminalHost};

fn app(root: &Path, term: Arc<dyn TerminalHost>) -> Arc<Core> {
    app_with(root, term, Settings::defaults())
}

fn app_with(root: &Path, term: Arc<dyn TerminalHost>, settings: Settings) -> Arc<Core> {
    let mut deps = CoreDeps::new(Dirs::under(root), CliArgs::default(), FakeUiBridge::new());
    deps.config = Some(MemConfig::new(settings, vec![project("shop", root)]));
    deps.terminal = Some(term);
    deps.secrets = Some(FakeSecrets::new());
    deps.trackers = Some(Arc::new(Factory::default()));
    deps.code_hosts = Some(Arc::new(Factory::default()));
    deps.login_env = Some(login_env(root));
    deps.install_ctl = false;
    deps.start_services = false;
    Core::start_with(deps).unwrap()
}

fn req(kind: SessionKind) -> SpawnRequest {
    SpawnRequest {
        id: None,
        project_id: ProjectId::new("shop"),
        kind,
        name: None,
        program: None,
        args: vec![],
        cwd: None,
        env: Default::default(),
        cols: 80,
        rows: 24,
        work_item_id: None,
        restore: RestorePolicy::None,
        close_on_exit: Default::default(),
        template_id: None,
    }
}

async fn eventually(mut f: impl FnMut() -> bool) -> bool {
    for _ in 0..400 {
        if f() {
            return true;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    false
}

#[tokio::test(flavor = "multi_thread")]
async fn sessions_survive_an_app_restart() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    let sock = Dirs::under(root).keltad_socket();
    let l = daemon::bind(&sock).unwrap();
    let host = PtyTerminalHost::new(TerminalLimits::default());
    let keltad = std::thread::spawn(move || daemon::serve(l, host, Duration::from_millis(200)));

    // First run: a shell (restored) and a setup session (never restored).
    let term = DaemonTerminalHost::connect(&sock).unwrap();
    let core = app(root, term.clone());
    let shell = core.session_spawn(req(SessionKind::Shell)).await.unwrap();
    let setup = core.session_spawn(req(SessionKind::Setup)).await.unwrap();
    assert!(!core.quit_needs_confirm(), "nothing is lost on quit");
    core.session_write(&shell.id, b"echo MARK-$((40+2))\n").await.unwrap();
    assert!(eventually(|| core.session_text_tail(&shell.id, 50).unwrap().contains("MARK-42")).await);
    core.shutdown().await.unwrap();
    term.close();
    drop(core);

    // keltad kept only the restorable session running.
    let probe = DaemonTerminalHost::connect(&sock).unwrap();
    assert!(eventually(|| probe.stats().sessions.len() == 1).await, "{:?}", probe.stats());
    assert_eq!(probe.stats().sessions[0].id, shell.id);
    probe.close();

    // Second run: the shell comes back Live, same process (its output is in the snapshot).
    let term = DaemonTerminalHost::connect(&sock).unwrap();
    let core = app(root, term.clone());
    assert!(core.session_get(&setup.id).is_none());
    assert_eq!(core.session_get(&shell.id).unwrap().lifecycle, Lifecycle::Live);
    let view = RecordingSink::new();
    core.session_attach(&shell.id, 80, 24, Box::new(view.clone())).await.unwrap();
    let snap: String = view.frames().iter().map(|f| String::from_utf8_lossy(&f[1..]).into_owned()).collect();
    assert!(snap.contains("MARK-42"), "{snap:?}");
    core.session_write(&shell.id, b"exit\n").await.unwrap();
    assert!(
        eventually(|| core.session_get(&shell.id).is_none_or(|s| s.lifecycle == Lifecycle::Exited)).await
    );

    // Quit with nothing running: keltad leaves after its grace.
    core.shutdown().await.unwrap();
    term.close();
    keltad.join().unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn quit_asks_when_keltad_will_not_keep_a_working_claude() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    let sock = Dirs::under(root).keltad_socket();
    let l = daemon::bind(&sock).unwrap();
    let host = PtyTerminalHost::new(TerminalLimits::default());
    let keltad = std::thread::spawn(move || daemon::serve(l, host, Duration::from_millis(200)));

    let mut settings = Settings::defaults();
    settings.app.restore_mode = RestoreMode::None;
    let term = DaemonTerminalHost::connect(&sock).unwrap();
    let core = app_with(root, term.clone(), settings);
    // A Claude that stays up (the PATH stub exits at once).
    std::fs::write(root.join("bin/claude"), "#!/bin/sh\nexec sleep 60\n").unwrap();
    let claude = core.session_spawn(req(SessionKind::Claude)).await.unwrap();
    let working = StatusChange {
        status: SessionStatus::Working,
        preview: None,
        file_edited: None,
        raw_event: "t".into(),
    };
    core.session_apply_hook(&claude.id, working).await.unwrap();
    assert!(core.quit_needs_confirm(), "restore_mode none: quit kills the working Claude");

    core.shutdown().await.unwrap();
    term.close();
    keltad.join().unwrap();
}

/// Every way a session closes (kill while Live, close_on_exit from the Exited handler, kill
/// while Exited) leaves nothing in the host. The Exited-handler paths call `kill` before the
/// reader marks the session exited (the on_exited race).
async fn closed_sessions_leave_the_host(root: &Path, term: Arc<dyn TerminalHost>) {
    let core = app(root, term.clone());
    let gone = |id| core.session_get(id).is_none();
    let live = core.session_spawn(req(SessionKind::Shell)).await.unwrap();
    core.session_kill(&live.id, false).await.unwrap();
    let mut always = req(SessionKind::Shell);
    always.close_on_exit = CloseOnExit::Always;
    let always = core.session_spawn(always).await.unwrap();
    core.session_write(&always.id, b"exit\n").await.unwrap();
    let mut never = req(SessionKind::Shell);
    never.close_on_exit = CloseOnExit::Never;
    let never = core.session_spawn(never).await.unwrap();
    core.session_write(&never.id, b"exit\n").await.unwrap();
    assert!(
        eventually(|| core.session_get(&never.id).is_some_and(|s| s.lifecycle == Lifecycle::Exited)).await
    );
    let ids = || term.stats().sessions.into_iter().map(|s| s.id).collect::<Vec<_>>();
    // The exited session stays (its tail is still shown) until closed; the others are gone.
    assert!(eventually(|| ids() == [never.id.clone()]).await, "{:?}", ids());
    core.session_kill(&never.id, false).await.unwrap();
    assert!(gone(&live.id) && gone(&always.id) && gone(&never.id));
    assert!(eventually(|| ids().is_empty()).await, "{:?}", ids());
    core.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn closed_sessions_are_freed_in_process() {
    let tmp = tempfile::tempdir().unwrap();
    closed_sessions_leave_the_host(tmp.path(), Arc::new(PtyTerminalHost::new(TerminalLimits::default())))
        .await;
}

#[tokio::test(flavor = "multi_thread")]
async fn closed_sessions_are_freed_in_keltad() {
    let tmp = tempfile::tempdir().unwrap();
    let sock = Dirs::under(tmp.path()).keltad_socket();
    let l = daemon::bind(&sock).unwrap();
    let host = PtyTerminalHost::new(TerminalLimits::default());
    let keltad = std::thread::spawn(move || daemon::serve(l, host, Duration::from_millis(200)));
    let term = DaemonTerminalHost::connect(&sock).unwrap();
    closed_sessions_leave_the_host(tmp.path(), term.clone()).await;
    term.close();
    keltad.join().unwrap();
}

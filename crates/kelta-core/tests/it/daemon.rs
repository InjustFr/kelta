//! keltad: a quit + restart of the app re-attaches the still-running session instead of
//! respawning it from Dormant; sessions that are not restored are not left behind, and closed
//! sessions are freed in the host (in-process and keltad).

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use crate::common::*;
use kelta_core::sessions::QuitPlan;
use kelta_core::{Core, CoreDeps};
use kelta_proto::api::{CoreApi, TerminalHost};
use kelta_proto::dirs::{CliArgs, Dirs};
use kelta_proto::ids::ProjectId;
use kelta_proto::model::{
    CloseOnExit, Lifecycle, RestorePolicy, SessionKind, SessionStatus, SpawnRequest, StatusChange,
};
use kelta_proto::settings::{QuitSessions, RestoreMode, Settings};
use kelta_proto::term::TerminalLimits;
use kelta_proto_testing::{FakeSecrets, FakeUiBridge, RecordingSink};
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
    // A Claude that stays up (the PATH stub exits at once). Written by a child `sh`: a write fd
    // held here leaks into concurrent forks -> ETXTBSY (Linux), as in common::bin_dir.
    let stub = root.join("bin/claude");
    let wrote = std::process::Command::new("sh")
        .args(["-c", "printf '#!/bin/sh\\nexec sleep 60\\n' > \"$1\"", "sh"])
        .arg(&stub)
        .status()
        .unwrap();
    assert!(wrote.success());
    // Without a program the session runs the login shell with Claude's args (dash exits at once).
    let claude = core
        .session_spawn(SpawnRequest { program: Some("claude".into()), ..req(SessionKind::Claude) })
        .await
        .unwrap();
    let working = StatusChange {
        status: SessionStatus::Working,
        preview: None,
        file_edited: None,
        raw_event: "t".into(),
        session_uuid: None,
    };
    core.session_apply_hook(&claude.id, working).await.unwrap();
    assert!(core.quit_needs_confirm(), "restore_mode none: quit kills the working Claude");

    core.shutdown().await.unwrap();
    term.close();
    keltad.join().unwrap();
}

/// A pid still running (a zombie waiting to be reaped is not): its state from `/proc`, else `ps`
/// (macOS, where `kill -0` succeeds on a zombie).
fn running(pid: &str) -> bool {
    let state = std::fs::read_to_string(format!("/proc/{pid}/stat"))
        .map(|s| s.rsplit(')').next().unwrap_or("").trim_start().to_owned())
        .or_else(|_| {
            let out = std::process::Command::new("ps").args(["-o", "stat=", "-p", pid]).output()?;
            Ok::<_, std::io::Error>(String::from_utf8_lossy(&out.stdout).trim().to_owned())
        })
        .unwrap_or_default();
    !state.is_empty() && !state.starts_with('Z')
}

/// #305: `running` treats an exited-but-unreaped child as gone (`kill -0` still succeeds on one).
#[tokio::test]
async fn running_is_false_for_a_zombie() {
    let mut child = std::process::Command::new("sh").args(["-c", "exit 0"]).spawn().unwrap();
    let pid = child.id().to_string();
    let zombie = || {
        let out = std::process::Command::new("ps").args(["-o", "stat=", "-p", &pid]).output().unwrap();
        String::from_utf8_lossy(&out.stdout).trim().starts_with('Z')
    };
    assert!(eventually(zombie).await, "the child never became a zombie");
    assert!(!running(&pid));
    child.wait().unwrap();
}

/// #305: quit with "stop all sessions" ends every session process and keltad (socket removed).
#[tokio::test(flavor = "multi_thread")]
async fn quit_with_stop_ends_the_sessions_and_keltad() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    let sock = Dirs::under(root).keltad_socket();
    let l = daemon::bind(&sock).unwrap();
    let host = PtyTerminalHost::new(TerminalLimits::default());
    // A long grace: only the shutdown ends keltad within the test.
    let keltad = std::thread::spawn(move || daemon::serve(l, host, Duration::from_secs(60)));

    let term = DaemonTerminalHost::connect(&sock).unwrap();
    let core = app(root, term.clone());
    assert_eq!(core.quit_plan(), QuitPlan::Keep, "nothing running: nothing to ask");
    let shell = core.session_spawn(req(SessionKind::Shell)).await.unwrap();
    assert_eq!(core.quit_plan(), QuitPlan::Ask { live: 1, busy: vec![] });
    // The shell's pid, then a child in the foreground, each in a file.
    let (shell_file, child_file) = (root.join("shell.pid"), root.join("child.pid"));
    let cmd = format!(
        "echo $$ > {}; sh -c 'echo $$ > \"$0\"; exec sleep 300' {}\n",
        shell_file.display(),
        child_file.display()
    );
    core.session_write(&shell.id, cmd.as_bytes()).await.unwrap();
    let read = |f: &Path| std::fs::read_to_string(f).unwrap_or_default().trim().to_string();
    assert!(eventually(|| !read(&child_file).is_empty()).await, "the child never started");
    let (shell_pid, child) = (read(&shell_file), read(&child_file));
    assert!(running(&child) && running(&shell_pid));

    core.stop_sessions_on_quit();
    core.shutdown().await.unwrap();
    assert!(eventually(|| keltad.is_finished()).await, "keltad still running");
    assert!(!sock.exists(), "keltad left its socket");
    assert!(eventually(|| !running(&child) && !running(&shell_pid)).await, "a session process survived");
    term.close();
}

/// `app.quit_sessions` keep / stop: no dialog.
#[tokio::test(flavor = "multi_thread")]
async fn quit_sessions_setting_skips_the_dialog() {
    for (setting, plan) in [(QuitSessions::Keep, QuitPlan::Keep), (QuitSessions::Stop, QuitPlan::Stop)] {
        let tmp = tempfile::tempdir().unwrap();
        let sock = Dirs::under(tmp.path()).keltad_socket();
        let l = daemon::bind(&sock).unwrap();
        let host = PtyTerminalHost::new(TerminalLimits::default());
        let keltad = std::thread::spawn(move || daemon::serve(l, host, Duration::from_millis(200)));
        let mut settings = Settings::defaults();
        settings.app.quit_sessions = setting;
        let term = DaemonTerminalHost::connect(&sock).unwrap();
        let core = app_with(tmp.path(), term.clone(), settings);
        let shell = core.session_spawn(req(SessionKind::Shell)).await.unwrap();
        assert_eq!(core.quit_plan(), plan);
        core.session_kill(&shell.id, false).await.unwrap();
        core.shutdown().await.unwrap();
        term.close();
        keltad.join().unwrap();
    }
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

/// #360: a Claude adopted after quit-and-leave reports its live status from its next hooks (sent
/// through the ctl socket with the token it was spawned with), and its live status is not "off".
#[tokio::test(flavor = "multi_thread")]
async fn adopted_claude_follows_its_hooks() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    let sock = Dirs::under(root).keltad_socket();
    let l = daemon::bind(&sock).unwrap();
    let host = PtyTerminalHost::new(TerminalLimits::default());
    let keltad = std::thread::spawn(move || daemon::serve(l, host, Duration::from_millis(200)));

    // A Claude that stays up and leaves the hook token its hooks would send.
    let (stub, token_file) = (root.join("claude-stub"), root.join("token"));
    let script =
        format!("#!/bin/sh\nprintf %s \"$KELTA_HOOK_TOKEN\" > '{}'\nexec sleep 60\n", token_file.display());
    let wrote = std::process::Command::new("sh")
        .args(["-c", "printf '%s' \"$2\" > \"$1\"; chmod +x \"$1\"", "sh"])
        .arg(&stub)
        .arg(&script)
        .status()
        .unwrap();
    assert!(wrote.success());
    let term = DaemonTerminalHost::connect(&sock).unwrap();
    let core = app(root, term.clone());
    let claude = core
        .session_spawn(SpawnRequest {
            program: Some(stub.display().to_string()),
            restore: RestorePolicy::Relaunch,
            ..req(SessionKind::Claude)
        })
        .await
        .unwrap();
    let token = || std::fs::read_to_string(&token_file).unwrap_or_default();
    assert!(eventually(|| !token().is_empty()).await, "the stub never started");
    core.shutdown().await.unwrap();
    term.close();
    drop(core);

    // Second run: adopted Live, hooks drive its status like a Claude started in this run.
    let term = DaemonTerminalHost::connect(&sock).unwrap();
    let core = app(root, term.clone());
    let info = core.session_get(&claude.id).unwrap();
    assert_eq!(info.lifecycle, Lifecycle::Live);
    assert!(info.claude.unwrap().hooks_active, "live status shown as off on an adopted Claude");
    let ctl = core.server().start_ctl().await.unwrap();
    let token = token();
    for (payload, status) in [
        (serde_json::json!({ "hook_event_name": "UserPromptSubmit" }), SessionStatus::Working),
        (
            serde_json::json!({ "hook_event_name": "Notification", "notification_type": "permission_prompt" }),
            SessionStatus::NeedsInput,
        ),
        (serde_json::json!({ "hook_event_name": "Stop" }), SessionStatus::Done),
        (
            serde_json::json!({ "hook_event_name": "Notification", "notification_type": "idle_prompt" }),
            SessionStatus::WaitingUser,
        ),
    ] {
        let r = ctl_send(
            &ctl,
            serde_json::json!({ "v": 1, "cmd": "hook", "session": claude.id, "token": token, "payload": payload }),
        )
        .await;
        assert_eq!(r["ok"], true, "{r}");
        let info = core.session_get(&claude.id).unwrap();
        assert_eq!(info.status, status, "{payload}");
        assert!(info.claude.unwrap().hooks_active);
    }

    core.session_kill(&claude.id, false).await.unwrap();
    core.shutdown().await.unwrap();
    term.close();
    keltad.join().unwrap();
}

/// #499: an adopted Claude keeps the HTTP server on its previous port, so its MCP and http hooks still reach Kelta.
#[tokio::test(flavor = "multi_thread")]
async fn adopted_live_claudes_keep_the_http_server_on_their_port() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    let sock = Dirs::under(root).keltad_socket();
    let l = daemon::bind(&sock).unwrap();
    let host = PtyTerminalHost::new(TerminalLimits::default());
    let keltad = std::thread::spawn(move || daemon::serve(l, host, Duration::from_millis(200)));

    // A Claude that stays up and leaves the URL and tokens its MCP and hooks would use.
    let (stub, env_file) = (root.join("claude-stub"), root.join("env"));
    let script = format!(
        "#!/bin/sh\nprintf '%s %s %s' \"$KELTA_MCP_URL\" \"$KELTA_MCP_TOKEN\" \"$KELTA_HOOK_TOKEN\" > '{}'\nexec sleep 60\n",
        env_file.display()
    );
    let wrote = std::process::Command::new("sh")
        .args(["-c", "printf '%s' \"$2\" > \"$1\"; chmod +x \"$1\"", "sh"])
        .arg(&stub)
        .arg(&script)
        .status()
        .unwrap();
    assert!(wrote.success());
    let term = DaemonTerminalHost::connect(&sock).unwrap();
    let core = app(root, term.clone());
    let claude = core
        .session_spawn(SpawnRequest {
            program: Some(stub.display().to_string()),
            restore: RestorePolicy::Relaunch,
            ..req(SessionKind::Claude)
        })
        .await
        .unwrap();
    let read = || std::fs::read_to_string(&env_file).unwrap_or_default();
    assert!(eventually(|| !read().is_empty()).await, "the stub never started");
    let env = read();
    let mut parts = env.split_whitespace();
    let (url, mcp_token, hook_token) = (parts.next().unwrap(), parts.next().unwrap(), parts.next().unwrap());
    let port: u16 = url.split("127.0.0.1:").nth(1).unwrap().split('/').next().unwrap().parse().unwrap();
    assert_eq!(core.server().http_port(), Some(port));
    core.shutdown().await.unwrap();
    term.close();
    // The process exit of a real restart: free the port whatever still holds the first core.
    while core.server().http_port().is_some() {
        core.server().release_http();
    }
    drop(core);
    assert!(
        eventually(|| std::net::TcpStream::connect(("127.0.0.1", port)).is_err()).await,
        "first run's server still listens"
    );

    // Second run: adopted Live, the server is back on the same port.
    let term = DaemonTerminalHost::connect(&sock).unwrap();
    let core = app(root, term.clone());
    assert_eq!(core.session_get(&claude.id).unwrap().lifecycle, Lifecycle::Live);
    assert!(eventually(|| core.server().http_port() == Some(port)).await, "server not back on {port}");
    let c = kelta_http::client_builder().no_proxy().build().unwrap();
    let ping = c
        .post(url)
        .bearer_auth(mcp_token)
        .json(&serde_json::json!({ "jsonrpc": "2.0", "id": 1, "method": "ping" }))
        .send()
        .await
        .unwrap();
    assert_eq!(ping.status(), 200);
    let hook = c
        .post(format!("http://127.0.0.1:{port}/hook/{}", claude.id))
        .bearer_auth(hook_token)
        .json(&serde_json::json!({ "hook_event_name": "UserPromptSubmit" }))
        .send()
        .await
        .unwrap();
    assert_eq!(hook.status(), 200);
    assert_eq!(core.session_get(&claude.id).unwrap().status, SessionStatus::Working);

    // The reference is the Claude's: its exit stops the server (nothing else holds it).
    core.session_kill(&claude.id, false).await.unwrap();
    assert!(eventually(|| core.server().http_port().is_none()).await, "adopted Claude's reference not released");
    core.shutdown().await.unwrap();
    term.close();
    keltad.join().unwrap();
}

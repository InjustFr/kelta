//! Generated Claude files: PLUGINS §8 settings (exact), mcp.json, argv; paths with spaces.

use std::path::Path;

use kelta_proto::dirs::Dirs;
use kelta_proto::settings::{ClaudeEffort, ClaudeSettings, HookTransport, PermissionMode};
use kelta_work::claude::{self, LaunchMode, LaunchSpec};
use serde_json::json;

fn mac_like_dirs() -> Dirs {
    let mut d = Dirs::under(Path::new("/Users/ada"));
    d.data = "/Users/ada/Library/Application Support/dev.kelta.Kelta".into();
    d.bin = d.data.join("bin");
    d.runtime = "/tmp/kelta-501".into();
    d
}

/// PLUGINS §8, written out literally.
fn expected_command_settings(ctl: &str) -> serde_json::Value {
    let cmd = format!("'{ctl}' hook");
    let h = json!({ "type": "command", "command": cmd, "async": true, "timeout": 5 });
    json!({ "hooks": {
        "SessionStart":      [ { "hooks": [ h ] } ],
        "UserPromptSubmit":  [ { "hooks": [ h ] } ],
        "PermissionRequest": [ { "hooks": [ h ] } ],
        "Notification":      [ { "matcher": "permission_prompt|idle_prompt|elicitation_dialog|agent_needs_input", "hooks": [ h ] } ],
        "PostToolUse":       [ { "matcher": "Edit|Write|MultiEdit", "hooks": [ h ] } ],
        "Stop":              [ { "hooks": [ h ] } ],
        "StopFailure":       [ { "hooks": [ h ] } ],
        "SessionEnd":        [ { "hooks": [ { "type": "command", "command": cmd, "timeout": 5 } ] } ]
    } })
}

#[test]
fn settings_match_plugins_8_exactly() {
    let dirs = mac_like_dirs();
    let got = claude::settings_json(&dirs, &ClaudeSettings::default(), None);
    let ctl = "/Users/ada/Library/Application Support/dev.kelta.Kelta/bin/current/kelta-ctl";
    assert_eq!(got, expected_command_settings(ctl));
    // POSIX single-quoting survives a quote in the path.
    let mut odd = dirs.clone();
    odd.bin = "/Users/o'neil/data dir/bin".into();
    assert_eq!(claude::hook_command(&odd), "'/Users/o'\\''neil/data dir/bin/current/kelta-ctl' hook");
    insta::assert_json_snapshot!("claude_settings_command", got);
}

#[test]
fn http_transport_and_extra_hooks() {
    let dirs = mac_like_dirs();
    let mut cfg = ClaudeSettings { hook_transport: HookTransport::Http, ..ClaudeSettings::default() };
    cfg.extra_hooks
        .insert("Stop".into(), json!([{ "hooks": [{ "type": "command", "command": "say done" }] }]));
    cfg.extra_hooks.insert(
        "PreToolUse".into(),
        json!([{ "matcher": "Bash", "hooks": [{ "type": "command", "command": "audit" }] }]),
    );
    let got = claude::settings_json(&dirs, &cfg, Some((51234, "0192f0c1-0000-7000-8000-000000000001")));
    let hooks = &got["hooks"];
    // SessionStart always uses the command hook.
    assert_eq!(hooks["SessionStart"][0]["hooks"][0]["type"], "command");
    let stop = &hooks["Stop"];
    assert_eq!(
        stop[0]["hooks"][0],
        json!({
            "type": "http",
            "url": "http://127.0.0.1:51234/hook/0192f0c1-0000-7000-8000-000000000001",
            "headers": { "Authorization": "Bearer ${KELTA_HOOK_TOKEN}" },
            "allowedEnvVars": ["KELTA_HOOK_TOKEN"],
            "timeout": 2
        })
    );
    assert_eq!(stop[1]["hooks"][0]["command"], "say done");
    assert_eq!(
        hooks["Notification"][0]["matcher"],
        "permission_prompt|idle_prompt|elicitation_dialog|agent_needs_input"
    );
    assert_eq!(hooks["PreToolUse"][0]["matcher"], "Bash");
    insta::assert_json_snapshot!("claude_settings_http", got);
}

#[test]
fn mcp_json_shape() {
    let got = claude::mcp_json(51234, "0192f0c1-0000-7000-8000-000000000001");
    assert_eq!(
        got,
        json!({"mcpServers":{"kelta":{"type":"http","url":"http://127.0.0.1:51234/mcp/0192f0c1-0000-7000-8000-000000000001","headers":{"Authorization":"Bearer ${KELTA_MCP_TOKEN}"}}}})
    );
    insta::assert_json_snapshot!("mcp_json", got);
}

#[test]
fn argv_with_spaces() {
    let run = Path::new("/tmp/kelta 501/s/6f1d2c3b");
    let spec = LaunchSpec {
        mode: LaunchMode::New {
            uuid: "6f1d2c3b-4a59-4e8f-9a0b-1c2d3e4f5a6b".into(),
            prompt: "Work on SHOP-142: Rate-limit login. The full ticket is in /tmp/kelta 501/s/6f1d2c3b/ticket.md.".into(),
        },
        name: "SHOP-142 Rate-limit login".into(),
        model: "opus".into(),
        effort: ClaudeEffort::High,
        permission_mode: PermissionMode::AcceptEdits,
        settings_file: run.join("claude-settings.json"),
        mcp_file: Some(run.join("mcp.json")),
        allowed_tools: vec!["mcp__kelta__*".into()],
        context_file: run.join("context.md"),
        extra_args: vec!["--verbose".into()],
    };
    let argv = claude::argv(&spec);
    insta::assert_json_snapshot!("claude_argv_new", argv);
    // Each path is one argv element (no shell involved).
    assert!(argv.contains(&"/tmp/kelta 501/s/6f1d2c3b/claude-settings.json".to_owned()));

    let resume = LaunchSpec {
        mode: LaunchMode::Resume { uuid: "6f1d2c3b-4a59-4e8f-9a0b-1c2d3e4f5a6b".into(), prompt: None },
        mcp_file: None,
        ..spec.clone()
    };
    insta::assert_json_snapshot!("claude_argv_resume", claude::argv(&resume));
    let cont = LaunchSpec { mode: LaunchMode::Continue { prompt: None }, ..spec };
    let a = claude::argv(&cont);
    assert_eq!(a[0], "--continue");
    assert!(!a.iter().any(|x| x.starts_with("Work on")));
}

#[test]
fn files_written_private() {
    use std::os::unix::fs::PermissionsExt;
    let tmp = tempfile::tempdir().unwrap();
    let dirs = Dirs::under(&tmp.path().join("with space"));
    let run = kelta_work::files::alloc_run_dir(&dirs, Some("6f1d2c3b")).unwrap();
    let settings = claude::settings_json(&dirs, &ClaudeSettings::default(), None);
    claude::write_files(&run, &settings, Some(&claude::mcp_json(1, "s")), "ctx").unwrap();
    for f in ["claude-settings.json", "mcp.json", "context.md"] {
        let m = std::fs::metadata(run.join(f)).unwrap().permissions().mode() & 0o777;
        assert_eq!(m, 0o600, "{f}");
    }
    let back: serde_json::Value =
        serde_json::from_slice(&std::fs::read(run.join("claude-settings.json")).unwrap()).unwrap();
    assert_eq!(back, settings);
    // Without MCP the stale file is removed.
    claude::write_files(&run, &settings, None, "ctx").unwrap();
    assert!(!run.join("mcp.json").exists());
}

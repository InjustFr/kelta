//! `exec.run` (PLUGINS §7) and the login PATH of plugin-run processes (`run` action, `exec.run`).

use std::os::unix::fs::PermissionsExt;
use std::path::Path;

use crate::common;

use kelta_proto::ErrorCode;
use kelta_proto::events::{BusEvent, bus};
use kelta_proto::ext::{ActionDef, BlockingOutcome, CallOrigin, PluginMethod, TriggerDef};
use kelta_proto::ids::{PluginId, ProjectId, ScreenInstanceId};
use serde_json::{Value, json};

const SCREEN: &str = "\n[[contributes.screens]]\nid = \"main\"\ntitle = \"Main\"\nentry = \"index.html\"\n";
const PERMS: &[&str] = &[
    "exec:echo",
    "exec:./bin/x.sh",
    "exec:bin/x.sh",
    "exec:./bin/out.sh",
    "exec:sh",
    "exec:kelta-login-only",
];

fn executable(path: &Path, script: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, script).unwrap();
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
}

async fn setup(granted: &[&str]) -> (common::Env, ScreenInstanceId) {
    let env = common::Env::new();
    let dir = env.write_plugin(
        "deployer",
        &format!("{}{SCREEN}", common::manifest("deployer", PERMS, "")),
        &[("index.html", "")],
    );
    executable(&dir.join("bin/x.sh"), "#!/bin/sh\necho \"$(pwd) $1\"\n");
    executable(&env.tmp.path().join("x.sh"), "#!/bin/sh\necho escaped\n");
    std::os::unix::fs::symlink(env.tmp.path().join("x.sh"), dir.join("bin/out.sh")).unwrap();
    let id = PluginId::new("deployer");
    env.host.grant(&id, granted.iter().map(|s| s.to_string()).collect()).await.unwrap();
    let i = env.host.screen_open(&id, "main", Some(&ProjectId::new("shop")), Value::Null).await.unwrap();
    (env, i.instance_id)
}

async fn exec(env: &common::Env, i: &ScreenInstanceId, p: Value) -> Result<Value, kelta_proto::KeltaError> {
    env.host.call(i, PluginMethod::ExecRun, p, CallOrigin::Screen { instance_id: i.clone() }).await
}

#[tokio::test]
async fn exec_run_returns_the_result_behind_its_exec_grant() {
    let (env, i) = setup(&[]).await;
    let e = exec(&env, &i, json!({ "command": "echo", "args": ["hi"] })).await.unwrap_err();
    assert_eq!(
        (e.code, e.detail.unwrap()["permission"].clone()),
        (ErrorCode::PermissionDenied, json!("exec:echo"))
    );
    let e = exec(&env, &i, json!({})).await.unwrap_err();
    assert_eq!(e.code, ErrorCode::InvalidArgument);

    let (env, i) = setup(PERMS).await;
    let out = exec(&env, &i, json!({ "command": "echo", "args": ["hi"] })).await.unwrap();
    assert_eq!(out, json!({ "code": 0, "stdout": "hi\n", "stderr": "" }));
    let out = exec(&env, &i, json!({ "command": "sh", "args": ["-c", "cat; echo \"$K\" >&2; exit 3"], "stdin": "in", "env": { "K": "{literal}" } }))
        .await
        .unwrap();
    assert_eq!(out, json!({ "code": 3, "stdout": "in", "stderr": "{literal}\n" }));
}

#[tokio::test]
async fn exec_run_relative_command_stays_inside_the_plugin_dir() {
    let (env, i) = setup(&["exec:./bin/x.sh", "exec:bin/x.sh", "exec:./bin/out.sh"]).await;
    let elsewhere = env.tmp.path().canonicalize().unwrap();
    let out =
        exec(&env, &i, json!({ "command": "./bin/x.sh", "args": ["qa"], "cwd": elsewhere })).await.unwrap();
    assert_eq!(out["stdout"], format!("{} qa\n", elsewhere.display()), "runs the plugin's file in `cwd`");
    let out = exec(&env, &i, json!({ "command": "bin/x.sh" })).await.unwrap();
    let dir = env.plugins_dir().join("deployer").canonicalize().unwrap();
    assert!(
        out["stdout"].as_str().unwrap().starts_with(dir.to_str().unwrap()),
        "cwd defaults to the plugin dir"
    );

    let e = exec(&env, &i, json!({ "command": "./bin/out.sh" })).await.unwrap_err();
    assert_eq!(e.code, ErrorCode::InvalidArgument, "symlink leaving the plugin dir: {e:?}");
    // A path is checked as written: a grant for `./bin/x.sh` is not one for another `x.sh`.
    for other in ["../x.sh", "./bin/../../x.sh", "/bin/x.sh", "./bin/rm"] {
        let e = exec(&env, &i, json!({ "command": other })).await.unwrap_err();
        assert_eq!(e.detail.unwrap()["permission"], format!("exec:{other}"), "{other}");
    }
}

#[tokio::test]
async fn exec_run_timeout_kills_the_group_and_output_is_capped() {
    let (env, i) = setup(&["exec:sh"]).await;
    let pid_file = env.tmp.path().join("pid");
    let script = format!("sleep 30 & echo $! > {}; wait", pid_file.display());
    let e = exec(&env, &i, json!({ "command": "sh", "args": ["-c", script], "timeout_ms": 300 }))
        .await
        .unwrap_err();
    assert_eq!(e.code, ErrorCode::Timeout);
    let pid: i32 = std::fs::read_to_string(&pid_file).unwrap().trim().parse().unwrap();
    let pid = rustix::process::Pid::from_raw(pid).unwrap();
    assert!(common::wait_for(|| rustix::process::test_kill_process(pid).is_err()).await, "grandchild killed");

    let big = "head -c 100000 /dev/zero | tr '\\0' a";
    let out = exec(&env, &i, json!({ "command": "sh", "args": ["-c", big] })).await.unwrap();
    let stdout = out["stdout"].as_str().unwrap();
    assert!(stdout.len() <= 64 * 1024 + 4 && stdout.ends_with('…'), "{}", stdout.len());
}

/// A Dock-launched Kelta has a bare PATH: the `run` action and `exec.run` find binaries through the
/// login shell's PATH.
#[tokio::test]
async fn run_action_and_exec_run_use_the_login_path() {
    let (env, i) = setup(&["exec:kelta-login-only"]).await;
    let bin_dir = env.tmp.path().join("login-bin");
    executable(&bin_dir.join("kelta-login-only"), "#!/bin/sh\necho '{\"veto\":\"found\"}'\n");
    let call = json!({ "command": "kelta-login-only" });
    assert_eq!(
        exec(&env, &i, call.clone()).await.unwrap_err().code,
        ErrorCode::NotFound,
        "not on the dev PATH"
    );

    // The plugin cannot point a granted name at another program through `PATH`.
    let shadow = env.tmp.path().join("shadow-bin");
    executable(&shadow.join("kelta-login-only"), "#!/bin/sh\necho shadow\n");
    let shadowed = json!({ "command": "kelta-login-only", "env": { "PATH": shadow } });
    env.core.respond("login_path", json!(bin_dir));
    let out = exec(&env, &i, shadowed).await.unwrap();
    assert_eq!(out["stdout"], "{\"veto\":\"found\"}\n");
    let out = exec(&env, &i, call).await.unwrap();
    assert_eq!(out["stdout"], "{\"veto\":\"found\"}\n");

    let trigger = TriggerDef {
        id: "guard".into(),
        on: bus::TICKET_BEFORE_START.into(),
        r#do: vec![ActionDef::Run {
            command: "kelta-login-only".into(),
            args: vec![],
            cwd: None,
            env: Default::default(),
            stdin: None,
            timeout_ms: None,
            show: None,
        }],
        blocking: true,
        ..Default::default()
    };
    let env = env.with_settings(|s| s.triggers = vec![trigger]);
    let ev = BusEvent::new(bus::TICKET_BEFORE_START, json!({})).with_project(ProjectId::new("shop"));
    assert_eq!(
        env.host.run_blocking(&ev).await.unwrap(),
        BlockingOutcome::Veto { trigger_id: "guard".into(), reason: "found".into() }
    );
}

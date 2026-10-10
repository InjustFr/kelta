//! examples/plugins/branch-script (docs/plugins-guide/acting-on-work.md): installs from its directory
//! with exactly the permissions it uses, its actions open its screen for the work item, and the screen's
//! `work.get` / `exec.run ./run.sh` calls work through the host.

use crate::common;

use kelta_proto::ext::{CallOrigin, PluginMethod};
use kelta_proto::ids::PluginId;
use kelta_proto::model::{PaneContent, TemplateCtx, WorkItem};
use serde_json::{Value, json};

const PERMS: [&str; 6] =
    ["work.read", "exec:run.sh", "storage", "events:work.updated", "events:pr.merged", "ui.open"];

#[tokio::test]
async fn branch_script_installs_opens_for_the_work_item_and_runs_its_script() {
    let env = common::Env::new();
    let src = common::example("branch-script").display().to_string();
    let p = env.host.inspect(&src).await.unwrap();
    let perms: Vec<&str> = p.permissions.iter().map(|x| x.permission.as_str()).collect();
    assert_eq!(perms, PERMS);
    env.host.install(&src, &p.sha256, PERMS.iter().map(|s| s.to_string()).collect()).await.unwrap();

    // A work item whose worktree exists, so `run.sh` can run in it.
    let worktree = env.tmp.path().join("repo").canonicalize().unwrap();
    let w = WorkItem { worktree: worktree.clone(), ..kelta_proto::samples::work_item() };
    env.core.add_work_item(w.clone());
    let key = w.ticket.clone().unwrap().key;

    // The work menu and the ticket detail run the same `open_screen` with the expanded params.
    for cmd in ["branch-script/run-on-work", "branch-script/run-on-ticket"] {
        let ctx =
            TemplateCtx { work_item_id: Some(w.id.clone()), ticket: w.ticket.clone(), ..Default::default() };
        env.host.command_run(cmd, ctx).await.unwrap();
        let (project, req) = env.core.opened().pop().unwrap();
        assert_eq!(project, w.project_id, "{cmd}");
        let PaneContent::PluginScreen { screen_id, params, .. } = req.content else { panic!("{cmd}") };
        assert_eq!((screen_id.as_str(), params), ("run", json!({ "work": w.id, "ticket": key })), "{cmd}");
    }

    let id = PluginId::new("branch-script");
    let i = env.host.screen_open(&id, "run", Some(&w.project_id), json!({ "work": w.id })).await.unwrap();
    let i = i.instance_id;
    let call =
        |m: PluginMethod, p: Value| env.host.call(&i, m, p, CallOrigin::Screen { instance_id: i.clone() });

    let got = call(PluginMethod::WorkGet, json!({ "id": w.id })).await.unwrap();
    assert_eq!((got["branch"].clone(), got["worktree"].clone()), (json!(w.branch), json!(worktree)));

    // Out of the box the `script` setting is `echo`: run.sh prints the branch and the worktree.
    let args = json!(["echo", w.branch, worktree]);
    let out = call(PluginMethod::ExecRun, json!({ "command": "./run.sh", "args": args, "cwd": worktree }))
        .await
        .unwrap();
    assert_eq!(
        out,
        json!({ "code": 0, "stdout": format!("{} {}\n", w.branch, worktree.display()), "stderr": "" })
    );
}

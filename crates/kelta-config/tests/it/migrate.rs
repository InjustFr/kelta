//! Tools are personal (#243): project-layer `[[tools]]` move to the global file once, losing nothing.

use crate::common::{env, read};
use kelta_proto::api::SettingsSource;
use kelta_proto::ids::ProjectId;

#[test]
fn project_tools_move_to_global_once() {
    let e = env();
    e.global(
        "# my tools\n[[tools]]\nid = \"lazydocker\"\nlabel = \"Docker\"\ncommand = \"lazydocker\"\n\n[[tools]]\nid = \"lazygit\"\nlabel = \"Git\"\ncommand = \"lazygit\"\n",
    );
    e.project(
        "shop",
        r#"[[tools]]
id = "lazydocker"
label = "Docker"
command = "lazydocker"

[[tools]]
id = "lazygit"
enabled = false

[[tools]]
id = "sl-web"
label = "sl web"
command = "sl"
args = ["web"]
cwd = "{worktree|project.root}"

[[tools]]
id = "lazydocker"
label = "Docker"
command = "lazydocker"
args = ["-f", "{repo.path}/docker-compose.yml"]
"#,
    );
    // an identical copy in a second project is deduped against the one migrated from `shop`
    e.project("blog", "[[tools]]\nid = \"sl-web\"\nlabel = \"sl web\"\ncommand = \"sl\"\nargs = [\"web\"]\ncwd = \"{worktree|project.root}\"\n");
    let shop_before = read(&e.dirs.projects_dir().join("shop.toml"));

    let svc = e.load();
    assert_eq!(svc.tools_migrated(), 5);
    assert!(svc.issues().is_empty(), "{:?}", svc.issues());

    let tools = svc.effective(None).tools.clone();
    let ids: Vec<&str> = tools.iter().map(|t| t.id.as_str()).collect();
    assert_eq!(ids, ["lazydocker", "lazygit", "sl-web", "lazydocker-shop"]);
    assert!(tools.iter().all(|t| t.enabled), "the enabled-only override is dropped");
    assert_eq!(tools[2].cwd.as_deref(), Some("{worktree|project.root}"), "placeholders unchanged");
    assert_eq!(tools[3].label, "Docker (shop)");
    assert_eq!(tools[3].args, ["-f", "{repo.path}/docker-compose.yml"]);
    let global = read(&e.dirs.global_config());
    assert!(global.starts_with("# my tools"), "global comments are kept: {global}");

    // the originals are in .trash, the project files have no tools and keep the rest
    let trash: Vec<String> = std::fs::read_dir(e.dirs.projects_dir().join(".trash"))
        .unwrap()
        .flatten()
        .map(|d| read(&d.path()))
        .collect();
    assert_eq!(trash.len(), 2);
    assert!(trash.contains(&shop_before));
    let shop = read(&e.dirs.projects_dir().join("shop.toml"));
    assert!(!shop.contains("[[tools]]") && shop.contains("[[project.repos]]"), "{shop}");
    assert_eq!(svc.effective(Some(&ProjectId::new("shop"))).tools, tools);

    // a second load changes nothing
    let svc = e.load();
    assert_eq!(svc.tools_migrated(), 0);
    assert_eq!(read(&e.dirs.global_config()), global);
    assert_eq!(read(&e.dirs.projects_dir().join("shop.toml")), shop);
    assert_eq!(std::fs::read_dir(e.dirs.projects_dir().join(".trash")).unwrap().count(), 2);
}

#[test]
fn broken_global_does_not_drop_projects_with_tools() {
    let e = env();
    e.global("[[tools\n");
    e.project("shop", "[[tools]]\nid = \"lazygit\"\ncommand = \"lazygit\"\n");
    let before = read(&e.dirs.projects_dir().join("shop.toml"));

    let svc = e.load();
    assert_eq!(svc.tools_migrated(), 0);
    assert_eq!(svc.projects().len(), 1, "the project still loads");
    let issues = svc.issues();
    assert!(
        issues.iter().any(|i| i.issue.message.contains("cannot be set per project (global only)")),
        "{issues:?}"
    );
    assert_eq!(read(&e.dirs.projects_dir().join("shop.toml")), before, "nothing rewritten");
}

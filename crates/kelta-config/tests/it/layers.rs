use crate::common;

use std::collections::BTreeMap;
use std::sync::Arc;

use crate::common::{Env, block_on, env};
use kelta_config::ConfigService;
use kelta_proto::api::SettingsSource;
use kelta_proto::dirs::CliArgs;
use kelta_proto::ids::{PluginId, ProjectId};
use kelta_proto::settings::{Layer, RuntimeOverrides};
use kelta_proto::testing::MemTrustStore;
use serde_json::json;

fn pid(s: &str) -> ProjectId {
    ProjectId::new(s)
}

#[test]
fn defaults_without_files() {
    let e = env();
    let svc = e.load();
    let eff = svc.effective_doc(None).unwrap();
    assert!(eff.sources.values().all(|l| *l == Layer::Default));
    assert_eq!(svc.effective(None).terminal.font_size, 13.0);
    assert!(svc.issues().is_empty());
    assert!(svc.projects().is_empty());
}

fn matrix_env() -> (Env, Arc<ConfigService>) {
    let e = env();
    e.global(
        r#"
[app]
theme = "light"
[terminal]
font_size = 14
[worktree]
branch_template = "g/{key}"
[claude]
append_system_prompt = "global"
"#,
    );
    e.project(
        "shop",
        r#"
[terminal]
font_size = 15
[worktree]
branch_template = "p/{key}"
"#,
    );
    e.repo_file(
        r#"
[worktree]
branch_template = "r/{key}"
include = [".env", ".npmrc"]
[claude]
append_system_prompt = "repo"
"#,
    );
    let cli = CliArgs { sets: vec!["terminal.cursor_style=\"bar\"".into()], ..CliArgs::default() };
    let env_vars = vec![("KELTA__APP__THEME".to_owned(), "\"dark\"".to_owned())];
    let svc = e.load_with(RuntimeOverrides::from_env_and_cli(env_vars, &cli));
    (e, svc)
}

#[test]
fn precedence_and_provenance_matrix() {
    let (_e, svc) = matrix_env();
    let keys = [
        "app.theme",
        "terminal.font_size",
        "terminal.cursor_style",
        "terminal.renderer",
        "worktree.branch_template",
        "worktree.include",
        "claude.append_system_prompt",
    ];
    let mut out: BTreeMap<String, serde_json::Value> = BTreeMap::new();
    for (scope, project) in [("global", None), ("shop", Some(pid("shop")))] {
        let eff = svc.effective_doc(project.as_ref()).unwrap();
        for k in keys {
            let segs = kelta_config::split_path(k).unwrap();
            let mut v = &eff.value;
            for s in &segs {
                v = &v[s];
            }
            out.insert(format!("{scope}:{k}"), json!({"value": v, "source": eff.sources[k]}));
        }
    }
    insta::assert_json_snapshot!("precedence_matrix", out);
}

#[test]
fn runtime_wins_over_everything_and_tables_report_the_highest_layer() {
    let (_e, svc) = matrix_env();
    let eff = svc.effective_doc(Some(&pid("shop"))).unwrap();
    assert_eq!(eff.sources["terminal.cursor_style"], Layer::Runtime);
    assert_eq!(eff.sources["terminal.font_size"], Layer::Project);
    assert_eq!(eff.sources["terminal"], Layer::Runtime);
    assert_eq!(eff.sources["worktree"], Layer::Repo);
    assert_eq!(svc.effective(Some(&pid("shop"))).terminal.font_size, 15.0);
    assert_eq!(svc.effective(None).terminal.font_size, 14.0);
    assert_eq!(svc.effective(None).worktree.branch_template, "g/{key}");
    assert_eq!(svc.effective(Some(&pid("shop"))).worktree.branch_template, "r/{key}");
}

#[test]
fn plugin_defaults_layer_and_validation() {
    let e = env();
    e.global("[plugins.burndown]\nview_id = \"sprint\"\n");
    let svc = e.load();
    svc.set_plugin_schemas(vec![(
        PluginId::new("burndown"),
        json!({"type": "object", "additionalProperties": false, "properties": {
            "view_id": {"type": "string", "default": "mine"},
            "days": {"type": "integer", "default": 14}
        }}),
    )]);
    let eff = svc.effective_doc(None).unwrap();
    assert_eq!(eff.value["plugins"]["burndown"], json!({"view_id": "sprint", "days": 14}));
    assert_eq!(eff.sources["plugins.burndown.view_id"], Layer::Global);
    assert_eq!(eff.sources["plugins.burndown.days"], Layer::Plugin);
    let schema = svc.schema_full();
    assert_eq!(
        schema["properties"]["plugins"]["properties"]["burndown"]["properties"]["days"]["default"],
        14
    );

    // a value that violates the plugin schema is rejected, last good stays
    e.global("[plugins.burndown]\ndays = \"many\"\n");
    svc.reload();
    assert_eq!(svc.effective_doc(None).unwrap().value["plugins"]["burndown"]["view_id"], "sprint");
    assert!(svc.issues().iter().any(|i| i.issue.path.starts_with("plugins.burndown")), "{:?}", svc.issues());
}

#[test]
fn by_id_override_disable_and_append() {
    let e = env();
    e.global(
        r#"
[[tools]]
id = "lazydocker"
label = "Docker"
kind = "pty"
command = "lazydocker"

[[tools]]
id = "lazygit"
label = "Git"
kind = "pty"
command = "lazygit"
"#,
    );
    e.project(
        "shop",
        r#"
[[tools]]
id = "lazydocker"
enabled = false

[[tools]]
id = "lazygit"
label = "Git (shop)"
kind = "pty"
command = "lazygit"
args = ["--use-config-dir", "x"]

[[tools]]
id = "psql"
label = "psql"
kind = "pty"
command = "psql"
"#,
    );
    let svc = e.load();
    let global = svc.effective(None);
    assert_eq!(global.tools.len(), 2);
    assert!(global.tools.iter().all(|t| t.enabled));

    let shop = svc.effective(Some(&pid("shop")));
    let by: BTreeMap<&str, &kelta_proto::ext::ToolDef> =
        shop.tools.iter().map(|t| (t.id.as_str(), t)).collect();
    assert_eq!(shop.tools.len(), 3);
    let docker = by["lazydocker"];
    assert!(!docker.enabled, "stub entry disables the inherited tool");
    assert_eq!(docker.command.as_deref(), Some("lazydocker"), "the rest of the inherited entry is kept");
    assert_eq!(by["lazygit"].label, "Git (shop)", "full entry replaces");
    assert_eq!(by["lazygit"].args.len(), 2);
    assert_eq!(by["psql"].command.as_deref(), Some("psql"));

    let eff = svc.effective_doc(Some(&pid("shop"))).unwrap();
    assert_eq!(eff.sources["tools.lazydocker"], Layer::Project);
    assert_eq!(eff.sources["tools.psql"], Layer::Project);
    assert_eq!(eff.sources["tools"], Layer::Project);
    let eff = svc.effective_doc(None).unwrap();
    assert_eq!(eff.sources["tools.lazydocker"], Layer::Global);
}

#[test]
fn external_tool_round_trips_in_config_order() {
    use kelta_proto::ext::ToolKind;
    let e = env();
    e.global("[[tools]]\nid = \"lazydocker\"\nlabel = \"Docker\"\ncommand = \"lazydocker\"\n");
    e.project(
        "shop",
        "[[tools]]\nid = \"fork\"\nlabel = \"Fork\"\nkind = \"external\"\ncommand = \"open\"\nargs = [\"-a\", \"Fork\", \".\"]\nkeybinding = \"mod+shift+f\"\n",
    );
    let svc = e.load();
    let tools = svc.effective(Some(&pid("shop"))).tools.clone();
    assert_eq!(tools.iter().map(|t| t.id.as_str()).collect::<Vec<_>>(), ["lazydocker", "fork"]);
    assert_eq!((tools[0].kind, tools[1].kind), (ToolKind::Pty, ToolKind::External));
    assert_eq!(tools[1].args, ["-a", "Fork", "."]);
}

#[test]
fn default_editor_presets_merge_by_id() {
    let e = env();
    e.global("[[editor.presets]]\nid = \"nvim\"\nlabel = \"My nvim\"\ncommand = \"nvim\"\nargs = [\"{path}\"]\n\n[[editor.presets]]\nid = \"vim\"\nenabled = false\n");
    let svc = e.load();
    let presets = svc.effective(None).editor.presets.clone();
    assert_eq!(presets.len(), 6);
    assert_eq!(presets[0].label, "My nvim");
    assert!(!presets.iter().find(|p| p.id == "vim").unwrap().enabled);
    assert!(presets.iter().find(|p| p.id == "zed").unwrap().enabled);
}

#[test]
fn maps_merge_by_key() {
    let e = env();
    e.global("[claude.profiles.default]\nmodel = \"sonnet\"\n");
    e.project("shop", "[claude.profiles.fast]\nmodel = \"haiku\"\neffort = \"low\"\n[claude.profiles.review]\nmodel = \"sonnet\"\n");
    let svc = e.load();
    let c = svc.effective(Some(&pid("shop"))).claude.clone();
    assert_eq!(c.profiles["default"].model, "sonnet");
    assert_eq!(c.profiles["default"].effort, kelta_proto::settings::ClaudeEffort::High);
    assert_eq!(c.profiles["fast"].model, "haiku");
    assert_eq!(c.profiles["review"].model, "sonnet");
    assert_eq!(c.profiles["plan"].model, "opus");
    assert_eq!(c.profiles.len(), 4);
    let eff = svc.effective_doc(Some(&pid("shop"))).unwrap();
    assert_eq!(eff.sources["claude.profiles.default.model"], Layer::Global);
    assert_eq!(eff.sources["claude.profiles.fast.model"], Layer::Project);
    assert_eq!(eff.sources["claude.profiles.plan.model"], Layer::Default);
}

#[test]
fn repo_local_disallowed_keys_are_errors_and_ignored() {
    let e = env();
    e.project("shop", "");
    e.repo_file("[terminal]\nfont_size = 30\n[worktree]\nroot = \"/tmp/x\"\ninclude = [\".x\"]\n");
    let svc = e.load();
    let issues = svc.issues();
    assert_eq!(issues.len(), 2, "{issues:?}");
    assert!(issues.iter().all(|i| i.issue.message.contains("not allowed in repo config")));
    assert!(issues.iter().any(|i| i.issue.path == "terminal.font_size" && i.issue.line == Some(2)));
    assert!(issues.iter().any(|i| i.issue.path == "worktree.root" && i.issue.line == Some(4)));
    // the invalid file contributes nothing
    let s = svc.effective(Some(&pid("shop")));
    assert_eq!(s.terminal.font_size, 13.0);
    assert_eq!(s.worktree.include, vec![".env".to_owned(), ".env.*".to_owned()]);
}

fn sha(text: &str) -> String {
    use sha2::Digest;
    sha2::Sha256::digest(text).iter().map(|b| format!("{b:02x}")).collect()
}

#[test]
fn untrusted_trusted_edited_untrusted_cycle() {
    let e = env();
    e.project("shop", "");
    let path = e.repo_file(
        "[worktree]\nsetup = [\"pnpm install\"]\ninclude = [\".npmrc\"]\n\n[env]\nTOKEN_URL = \"x\"\n\n[[tools]]\nid = \"repo-tool\"\nlabel = \"R\"\nkind = \"pty\"\ncommand = \"make\"\n",
    );
    let svc = e.load();
    let p = pid("shop");
    let store = Arc::new(MemTrustStore::new());
    block_on(svc.set_trust_store(store.clone()));

    // untrusted: exec keys inert, non-exec keys active
    let s = svc.effective(Some(&p));
    assert!(s.worktree.setup.is_empty());
    assert!(s.env.is_empty());
    assert!(s.tools.is_empty());
    assert_eq!(s.worktree.include, vec![".npmrc".to_owned()]);
    let doc = svc.layer_get(Layer::Repo, Some(&p), Some("api")).unwrap();
    assert_eq!(doc.trusted, Some(false));
    assert!(doc.text.contains("pnpm install"), "the layer view still shows the file");

    // trust binds to the reviewed content: no hash, or a stale one, is refused
    assert!(block_on(svc.repo_trust(&p, "api", true, None)).is_err());
    let stale = block_on(svc.repo_trust(&p, "api", true, Some(&sha("[env]\n")))).unwrap_err();
    assert_eq!(stale.code, kelta_proto::ErrorCode::Conflict);
    assert!(svc.effective(Some(&p)).worktree.setup.is_empty());

    // trust: active
    let info = block_on(svc.repo_trust(&p, "api", true, Some(&sha(&doc.text)))).unwrap();
    assert!(info.trusted);
    assert_eq!(info.path, path);
    let s = svc.effective(Some(&p));
    assert_eq!(s.worktree.setup, vec!["pnpm install".to_owned()]);
    assert_eq!(s.env["TOKEN_URL"], "x");
    assert_eq!(s.tools.len(), 1);
    assert_eq!(svc.layer_get(Layer::Repo, Some(&p), Some("api")).unwrap().trusted, Some(true));
    assert_eq!(svc.effective_doc(Some(&p)).unwrap().sources["worktree.setup"], Layer::Repo);

    // any edit (here through settings) makes it inert again
    svc.layer_set(Layer::Repo, Some(&p), Some("api"), "worktree.include", json!([".npmrc", ".env"])).unwrap();
    let s = svc.effective(Some(&p));
    assert!(s.worktree.setup.is_empty(), "edited file must be re-trusted");
    assert!(s.tools.is_empty());
    assert_eq!(s.worktree.include.len(), 2, "non-exec edit still applies");
    assert_eq!(svc.layer_get(Layer::Repo, Some(&p), Some("api")).unwrap().trusted, Some(false));

    // an external edit does the same, and re-trusting activates it
    let text = common::read(&path).replace("pnpm install", "pnpm install --frozen-lockfile");
    std::fs::write(&path, text).unwrap();
    svc.reload();
    assert!(svc.effective(Some(&p)).worktree.setup.is_empty());
    let doc = svc.layer_get(Layer::Repo, Some(&p), Some("api")).unwrap();
    block_on(svc.repo_trust(&p, "api", true, Some(&sha(&doc.text)))).unwrap();
    assert_eq!(svc.effective(Some(&p)).worktree.setup, vec!["pnpm install --frozen-lockfile".to_owned()]);

    // the persistent store holds the hash; a fresh service picks it up
    let svc2 = e.load();
    assert!(svc2.effective(Some(&p)).worktree.setup.is_empty());
    block_on(svc2.set_trust_store(store));
    assert_eq!(svc2.effective(Some(&p)).worktree.setup, vec!["pnpm install --frozen-lockfile".to_owned()]);

    // revoke
    let info = block_on(svc2.repo_trust(&p, "api", false, None)).unwrap();
    assert!(!info.trusted);
    assert!(svc2.effective(Some(&p)).worktree.setup.is_empty());
}

#[test]
fn primary_repo_wins_on_conflict() {
    let e = env();
    let other = e.tmp.path().join("repo-b");
    std::fs::create_dir_all(other.join(".kelta")).unwrap();
    std::fs::write(
        other.join(".kelta/config.toml"),
        "[worktree]\nbranch_template = \"b/{key}\"\ninclude = [\".b\"]\n",
    )
    .unwrap();
    e.repo_file("[worktree]\nbranch_template = \"a/{key}\"\n");
    let project = format!(
        "[project]\nid = \"duo\"\nname = \"Duo\"\n\n[[project.repos]]\nid = \"b\"\npath = \"{}\"\n\n[[project.repos]]\nid = \"a\"\npath = \"{}\"\nprimary = true\n",
        other.display(),
        e.repo.display()
    );
    std::fs::write(e.dirs.projects_dir().join("duo.toml"), project).unwrap();
    let svc = e.load();
    let s = svc.effective(Some(&pid("duo")));
    assert_eq!(s.worktree.branch_template, "a/{key}");
    assert_eq!(s.worktree.include, vec![".b".to_owned()]);
}

#[test]
fn env_and_cli_overrides() {
    let e = env();
    e.global("[terminal]\nfont_size = 14\n");
    let cli = CliArgs {
        safe_graphics: true,
        sets: vec![
            "terminal.cursor_style=\"bar\"".into(),
            "terminal.scrollback.shell=500".into(),
            "keys.prefix_timeout_ms=99999".into(), // out of range → ignored
            "no.such.key=1".into(),                // unknown → ignored
        ],
        ..CliArgs::default()
    };
    let env_vars = vec![
        ("KELTA__TERMINAL__FONT_SIZE".to_owned(), "20".to_owned()),
        ("KELTA__LINUX__GRAPHICS__GDK_BACKEND".to_owned(), "\"x11\"".to_owned()),
        ("KELTA__APP__THEME".to_owned(), "dark".to_owned()), // bare word falls back to a string
        ("HOME".to_owned(), "/x".to_owned()),
    ];
    let svc = e.load_with(RuntimeOverrides::from_env_and_cli(env_vars, &cli));
    let s = svc.effective(None);
    assert_eq!(s.terminal.font_size, 20.0);
    assert_eq!(s.terminal.cursor_style, kelta_proto::settings::CursorStyle::Bar);
    assert_eq!(s.terminal.scrollback.shell, 500);
    assert_eq!(s.linux.graphics.gdk_backend, kelta_proto::settings::GdkBackend::X11);
    assert_eq!(s.linux.graphics.profile, kelta_proto::settings::GraphicsProfile::Safe);
    assert_eq!(s.app.theme, kelta_proto::settings::Theme::Dark);
    assert_eq!(s.keys.prefix_timeout_ms, 1000);
    let eff = svc.effective_doc(None).unwrap();
    assert_eq!(eff.sources["terminal.font_size"], Layer::Runtime);
    assert_eq!(eff.sources["linux.graphics.profile"], Layer::Runtime);
    assert_eq!(svc.issues().len(), 2, "{:?}", svc.issues());
    // runtime values are never persisted
    assert!(common::read(&e.dirs.global_config()).contains("font_size = 14"));
    svc.layer_set(Layer::Global, None, None, "terminal.cursor_style", json!("underline")).unwrap();
    assert!(!common::read(&e.dirs.global_config()).contains("20"));
}

#[test]
fn config_dir_override() {
    let e = env();
    let alt = e.tmp.path().join("alt");
    std::fs::create_dir_all(&alt).unwrap();
    std::fs::write(alt.join("config.toml"), "[terminal]\nfont_size = 22\n").unwrap();
    let svc = e.load_with(RuntimeOverrides { config_dir: Some(alt), ..RuntimeOverrides::default() });
    assert_eq!(svc.effective(None).terminal.font_size, 22.0);
}

#[test]
fn invalid_global_at_startup_gives_defaults_and_position() {
    let e = env();
    e.global("[terminal]\nfont_size = 14\nrenderer = = \"x\"\n");
    let svc = e.load();
    assert_eq!(svc.effective(None).terminal.font_size, 13.0);
    let issues = svc.issues();
    assert_eq!(issues.len(), 1);
    assert_eq!(issues[0].issue.line, Some(3));
    assert!(issues[0].display().contains("config.toml:3:"), "{}", issues[0].display());
}

#[test]
fn schema_violations_report_the_key_position() {
    let e = env();
    e.global("[app]\ntheme = \"dark\"\n\n[terminal]\nfont_size = 100\nbogus = true\n");
    let svc = e.load();
    let issues = svc.issues();
    assert!(
        issues.iter().any(|i| i.issue.path == "terminal.font_size" && i.issue.line == Some(5)),
        "{issues:?}"
    );
    assert!(issues.iter().any(|i| i.issue.path == "terminal.bogus" && i.issue.line == Some(6)), "{issues:?}");
    assert_eq!(svc.effective(None).app.theme, kelta_proto::settings::Theme::System, "whole file rejected");
}

#[test]
fn project_file_rules() {
    let e = env();
    e.project("shop", "[linux.graphics]\nprofile = \"safe\"\n");
    std::fs::write(e.dirs.projects_dir().join("other.toml"), "[project]\nid = \"mismatch\"\nname = \"x\"\n")
        .unwrap();
    let svc = e.load();
    assert!(svc.projects().is_empty(), "both files are invalid");
    let msgs: Vec<String> = svc.issues().iter().map(|i| i.issue.message.clone()).collect();
    assert!(msgs.iter().any(|m| m.contains("global only")), "{msgs:?}");
    assert!(msgs.iter().any(|m| m.contains("must equal the file name")), "{msgs:?}");
}

#[test]
fn keybindings_file_merges_into_keys() {
    let e = env();
    e.global("[keys]\nprefix = \"ctrl+shift+a\"\n");
    std::fs::write(e.dirs.config.join("keybindings.toml"), "[bindings]\n\"palette.open\" = [\"mod+j\"]\n")
        .unwrap();
    let svc = e.load();
    let s = svc.effective(None);
    assert_eq!(s.keys.prefix, "ctrl+shift+a");
    assert_eq!(s.keys.bindings["palette.open"], vec!["mod+j".to_owned()]);
    assert_eq!(svc.effective_doc(None).unwrap().sources["keys.bindings.\"palette.open\""], Layer::Global);
    // writes under keys.* go to keybindings.toml
    svc.layer_set(Layer::Global, None, None, "keys.bindings.\"tab.next\"", json!(["mod+n"])).unwrap();
    assert!(common::read(&e.dirs.config.join("keybindings.toml")).contains("\"tab.next\""));
    assert!(!common::read(&e.dirs.global_config()).contains("tab.next"));
    assert_eq!(svc.effective(None).keys.bindings["tab.next"], vec!["mod+n".to_owned()]);
}

#[test]
fn unknown_template_and_preset_ids_are_rejected() {
    let e = env();
    e.global("[worktree]\nroot = \"~/w/{project}/{nope}\"\n");
    let svc = e.load();
    assert!(svc.issues().iter().any(|i| i.issue.message.contains("{nope}")), "{:?}", svc.issues());
    let e = env();
    e.global("[editor]\ndefault = \"ghost\"\n");
    let svc = e.load();
    assert!(
        svc.issues().iter().any(|i| i.issue.message.contains("unknown editor preset")),
        "{:?}",
        svc.issues()
    );
    assert_eq!(svc.effective(None).editor.default, "nvim");
}

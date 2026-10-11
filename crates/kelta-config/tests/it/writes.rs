use crate::common;

use std::sync::{Arc, Mutex};

use crate::common::{env, read};
use kelta_proto::api::SettingsSource;
use kelta_proto::error::ErrorCode;
use kelta_proto::ids::ProjectId;
use kelta_proto::model::{CodeHostHint, ProjectDraft, ProjectPatch, RepoDraft};
use kelta_proto::settings::{Layer, SettingsDiff, Severity};
use serde_json::json;

const GLOBAL: &str = "#:schema https://kelta.dev/schema/0.1/settings.schema.json\n# my config\n[app]\ntheme = \"dark\" # keep me\n\n# terminal block\n[terminal]\nfont_size = 13 # size\nrenderer = \"auto\"\n\n[notifications]\nenabled = true\n";

#[test]
fn set_preserves_comments_and_order() {
    let e = env();
    e.global(GLOBAL);
    let svc = e.load();
    svc.layer_set(Layer::Global, None, None, "terminal.font_size", json!(15)).unwrap();
    svc.layer_set(Layer::Global, None, None, "terminal.cursor_style", json!("bar")).unwrap();
    svc.layer_set(Layer::Global, None, None, "window.restore_geometry", json!(false)).unwrap();
    let text = read(&e.dirs.global_config());
    let expect = "#:schema https://kelta.dev/schema/0.1/settings.schema.json\n# my config\n[app]\ntheme = \"dark\" # keep me\n\n# terminal block\n[terminal]\nfont_size = 15 # size\nrenderer = \"auto\"\ncursor_style = \"bar\"\n\n[notifications]\nenabled = true\n\n[window]\nrestore_geometry = false\n";
    assert_eq!(text, expect);
    assert_eq!(svc.effective(None).terminal.font_size, 15.0);
}

#[test]
fn creating_files_adds_the_schema_header() {
    let e = env();
    let svc = e.load();
    assert!(!e.dirs.global_config().exists());
    svc.layer_set(Layer::Global, None, None, "app.theme", json!("light")).unwrap();
    let text = read(&e.dirs.global_config());
    assert!(text.starts_with("#:schema https://kelta.dev/schema/0.1/settings.schema.json\n"), "{text}");
    assert!(text.contains("[app]\ntheme = \"light\"\n"), "{text}");
}

#[test]
fn reset_removes_only_that_key() {
    let e = env();
    e.global(GLOBAL);
    let svc = e.load();
    let eff = svc.layer_reset(Layer::Global, None, None, "terminal.font_size").unwrap();
    assert_eq!(eff.sources["terminal.font_size"], Layer::Default);
    let text = read(&e.dirs.global_config());
    assert!(!text.contains("font_size"));
    assert!(text.contains("# terminal block\n[terminal]\nrenderer = \"auto\""), "{text}");
    // resetting the last key of a table drops the table, null means reset
    svc.layer_set(Layer::Global, None, None, "notifications.enabled", json!(null)).unwrap();
    assert!(!read(&e.dirs.global_config()).contains("[notifications]"));
    // unknown / absent keys are a no-op
    svc.layer_reset(Layer::Global, None, None, "terminal.nothing").unwrap();
}

#[test]
fn invalid_values_are_refused_and_file_untouched() {
    let e = env();
    e.global(GLOBAL);
    let svc = e.load();
    let err = svc.layer_set(Layer::Global, None, None, "terminal.font_size", json!(100)).unwrap_err();
    assert_eq!(err.code, ErrorCode::InvalidArgument);
    assert!(err.message.contains("terminal.font_size"), "{}", err.message);
    assert!(err.message.contains("config.toml:"), "file:line:col in the message: {}", err.message);
    let err = svc.layer_set(Layer::Global, None, None, "terminal.renderer", json!("vulkan")).unwrap_err();
    assert_eq!(err.code, ErrorCode::InvalidArgument);
    let err = svc.layer_set(Layer::Global, None, None, "terminal.nope", json!(1)).unwrap_err();
    assert!(err.message.contains("unknown key"), "{}", err.message);
    assert_eq!(read(&e.dirs.global_config()), GLOBAL);
    assert_eq!(svc.effective(None).terminal.font_size, 13.0);
}

#[test]
fn raw_write_validates_first() {
    let e = env();
    e.global(GLOBAL);
    let svc = e.load();
    let err = svc.layer_write_raw(Layer::Global, None, None, "[terminal]\nfont_size = = 3\n").unwrap_err();
    assert_eq!(err.code, ErrorCode::InvalidArgument);
    assert!(err.message.contains("config.toml:2:"), "{}", err.message);
    let detail = err.detail.unwrap();
    assert_eq!(detail["issues"][0]["line"], 2);
    assert_eq!(read(&e.dirs.global_config()), GLOBAL);

    let ok = "# rewritten\n[terminal]\nfont_size = 18\n";
    let eff = svc.layer_write_raw(Layer::Global, None, None, ok).unwrap();
    assert_eq!(eff.value["terminal"]["font_size"], 18);
    assert_eq!(read(&e.dirs.global_config()), ok, "raw text is stored verbatim");

    // issues come back with positions through layer_validate too
    let issues = svc.layer_validate(Layer::Global, "[terminal]\nfont_size = 1\n").unwrap();
    assert_eq!(issues.len(), 1);
    assert_eq!((issues[0].line, issues[0].path.as_str()), (Some(2), "terminal.font_size"));
    assert!(svc.layer_validate(Layer::Global, "[terminal]\nfont_size = 12\n").unwrap().is_empty());
    assert!(svc.layer_validate(Layer::Default, "").is_err());
    // a bad project colour and project `tools` do not block the editor: they come back as warnings
    let text = "[project]\nid = \"shop\"\ncolor = \"red\"\n\n[[tools]]\nid = \"t\"\nlabel = \"T\"\ncommand = \"t\"\n";
    let r = svc.layer_validate(Layer::Project, text).unwrap();
    let got: Vec<_> = r.iter().map(|i| (i.path.as_str(), i.line, i.severity)).collect();
    assert_eq!(
        got,
        [("tools", Some(5), Severity::Warning), ("project.color", Some(3), Severity::Warning)],
        "{r:?}"
    );
}

#[test]
fn editor_and_load_agree_on_soft_issues() {
    let e = env();
    // (project `tools` is moved to the global file on load, so only the colour stays to compare)
    let text = "[project]\nid = \"shop\"\ncolor = \"red\"\n\n[terminal]\nfont_size = 15\n";
    std::fs::write(e.dirs.projects_dir().join("shop.toml"), text).unwrap();
    let svc = e.load();
    let loaded: Vec<_> = svc.issues().into_iter().map(|i| i.issue).collect();
    assert_eq!(loaded.len(), 1, "{loaded:?}");
    assert_eq!(loaded[0].severity, Severity::Warning);
    assert_eq!(svc.layer_validate(Layer::Project, text).unwrap(), loaded);
    assert_eq!(svc.effective(Some(&ProjectId::new("shop"))).terminal.font_size, 15.0);
}

#[test]
fn ipc_rejects_a_bad_project_colour() {
    let e = env();
    let svc = e.load();
    let mut d = draft(&e, "shop");
    d.color = Some("red".into());
    assert_eq!(svc.project_create(&d).unwrap_err().code, ErrorCode::InvalidArgument);
    assert!(!e.dirs.projects_dir().join("shop.toml").exists());

    svc.project_create(&draft(&e, "shop")).unwrap();
    let file = e.dirs.projects_dir().join("shop.toml");
    let before = read(&file);
    let patch = ProjectPatch { color: Some("red".into()), ..Default::default() };
    let err = svc.project_update(&ProjectId::new("shop"), &patch).unwrap_err();
    assert_eq!(err.code, ErrorCode::InvalidArgument);
    assert!(err.message.contains("theme colour or #rrggbb"), "{err:?}");
    assert_eq!(read(&file), before);
    let ok = ProjectPatch { color: Some("theme:green".into()), ..Default::default() };
    assert_eq!(
        svc.project_update(&ProjectId::new("shop"), &ok).unwrap().color.as_deref(),
        Some("theme:green")
    );
}

#[test]
fn no_temp_files_and_echo_is_suppressed() {
    let e = env();
    e.global(GLOBAL);
    let svc = e.load();
    let seen: Arc<Mutex<Vec<SettingsDiff>>> = Arc::default();
    let s2 = seen.clone();
    // register callbacks without starting a watcher thread dependency on timing
    svc.watch(Box::new(move |d| s2.lock().unwrap().push(d)));
    svc.layer_set(Layer::Global, None, None, "terminal.font_size", json!(16)).unwrap();
    {
        let v = seen.lock().unwrap();
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].layers, vec![Layer::Global]);
        assert_eq!(v[0].paths, vec!["terminal.font_size".to_owned()]);
        assert!(v[0].requires_restart.is_empty());
    }
    // what the watcher would deliver after our own write: content hash already known
    assert!(svc.reload().is_none(), "echo of our own write must not notify");
    assert_eq!(seen.lock().unwrap().len(), 1);
    let leftovers: Vec<_> = std::fs::read_dir(&e.dirs.config)
        .unwrap()
        .flatten()
        .filter(|f| f.file_name().to_string_lossy().contains("tmp"))
        .collect();
    assert!(leftovers.is_empty());
    // setting the same value again writes nothing and notifies nothing
    svc.layer_set(Layer::Global, None, None, "terminal.font_size", json!(16)).unwrap();
    assert_eq!(seen.lock().unwrap().len(), 1);
}

#[test]
fn restart_keys_are_reported() {
    let e = env();
    let svc = e.load();
    let seen: Arc<Mutex<Vec<SettingsDiff>>> = Arc::default();
    let s2 = seen.clone();
    svc.watch(Box::new(move |d| s2.lock().unwrap().push(d)));
    svc.layer_set(Layer::Global, None, None, "linux.graphics.disable_dmabuf", json!(true)).unwrap();
    svc.layer_set(Layer::Global, None, None, "window.decorations", json!("none")).unwrap();
    let v = seen.lock().unwrap();
    assert_eq!(v[0].requires_restart, vec!["linux.graphics.disable_dmabuf".to_owned()]);
    assert_eq!(v[1].requires_restart, vec!["window.decorations".to_owned()]);
}

#[test]
fn quoted_map_keys_round_trip() {
    let e = env();
    let svc = e.load();
    svc.layer_set(Layer::Global, None, None, "keys.bindings.\"palette.open\"", json!(["mod+j"])).unwrap();
    svc.layer_set(Layer::Global, None, None, "keys.bindings.\"tab.next\"", json!([])).unwrap();
    let text = read(&e.dirs.global_config());
    assert!(text.contains("[keys.bindings]"), "{text}");
    assert!(text.contains("\"palette.open\" = [\"mod+j\"]"), "{text}");
    assert_eq!(svc.effective(None).keys.bindings["tab.next"], Vec::<String>::new());
}

#[test]
fn keyed_list_writes_keep_entry_comments() {
    let e = env();
    e.global("[[tools]]\nid = \"a\"\nlabel = \"A\" # label\nkind = \"pty\"\ncommand = \"a\"\n");
    let svc = e.load();
    svc.layer_set(
        Layer::Global,
        None,
        None,
        "tools",
        json!([{"id": "a", "label": "A", "kind": "pty", "command": "aa"}, {"id": "b", "label": "B", "kind": "pty", "command": "b"}]),
    )
    .unwrap();
    let text = read(&e.dirs.global_config());
    assert!(text.contains("label = \"A\" # label"), "{text}");
    assert!(text.contains("command = \"aa\""), "{text}");
    assert_eq!(svc.effective(None).tools.len(), 2);
}

#[test]
fn project_layer_writes() {
    let e = env();
    e.project("shop", "");
    let svc = e.load();
    let p = ProjectId::new("shop");
    let eff = svc.layer_set(Layer::Project, Some(&p), None, "terminal.font_size", json!(11)).unwrap();
    assert_eq!(eff.sources["terminal.font_size"], Layer::Project);
    assert_eq!(svc.effective(Some(&p)).terminal.font_size, 11.0);
    assert_eq!(svc.effective(None).terminal.font_size, 13.0);
    assert!(read(&e.dirs.projects_dir().join("shop.toml")).contains("[terminal]\nfont_size = 11"));
    // global-only keys are refused at the project layer
    let err = svc.layer_set(Layer::Project, Some(&p), None, "window.decorations", json!("none")).unwrap_err();
    assert!(err.message.contains("global only"), "{}", err.message);
    // missing project
    let err = svc
        .layer_set(Layer::Project, Some(&ProjectId::new("ghost")), None, "app.theme", json!("dark"))
        .unwrap_err();
    assert_eq!(err.code, ErrorCode::NotFound);
    // the Runtime layer is read-only
    assert!(svc.layer_set(Layer::Runtime, None, None, "app.theme", json!("dark")).is_err());
}

fn draft(e: &common::Env, id: &str) -> ProjectDraft {
    ProjectDraft {
        suggested_id: ProjectId::new(id),
        name: "Shop".into(),
        color: Some("#e07a5f".into()),
        icon: Some("S".into()),
        repos: vec![RepoDraft {
            id: "api".into(),
            path: e.repo.clone(),
            primary: false,
            remote: "origin".into(),
            base: "main".into(),
            remote_url: Some("git@github.com:acme/shop-api.git".into()),
            code_host: Some(kelta_proto::settings::CodeHostBinding {
                account: "github-work".into(),
                repo: "acme/shop-api".into(),
            }),
        }],
        code_host_hints: Vec::<CodeHostHint>::new(),
        tracker_hints: vec![],
        tracker: None,
        default_template: Some("claude".into()),
    }
}

#[test]
fn project_crud_with_trash() {
    let e = env();
    let svc = e.load();
    let cfg = svc.project_create(&draft(&e, "shop")).unwrap();
    assert_eq!(cfg.id.as_str(), "shop");
    assert!(cfg.repos[0].primary, "the first repo becomes primary");
    let file = e.dirs.projects_dir().join("shop.toml");
    let text = read(&file);
    assert!(text.starts_with("#:schema https://kelta.dev/schema/0.1/project.schema.json\n"), "{text}");
    assert!(text.contains("[[project.repos]]"), "{text}");
    assert_eq!(svc.projects().len(), 1);

    let dup = svc.project_create(&draft(&e, "shop")).unwrap_err();
    assert_eq!(dup.code, ErrorCode::Conflict);
    assert_eq!(svc.project_create(&draft(&e, "Bad Id")).unwrap_err().code, ErrorCode::InvalidArgument);
    assert_eq!(svc.project_create(&draft(&e, "home")).unwrap_err().code, ErrorCode::InvalidArgument);

    // comments survive an update
    let commented = text.replace("name = \"Shop\"", "name = \"Shop\" # display name");
    std::fs::write(&file, commented).unwrap();
    svc.reload();
    let patch = ProjectPatch {
        name: Some("Shop 2".into()),
        tracker: Some(kelta_proto::settings::TrackerBinding {
            account: "jira-acme".into(),
            views: vec![kelta_proto::settings::TrackerView {
                id: "mine".into(),
                label: "Mine".into(),
                jql: Some("assignee = currentUser()".into()),
                ..Default::default()
            }],
            ..Default::default()
        }),
        ..ProjectPatch::default()
    };
    let cfg = svc.project_update(&ProjectId::new("shop"), &patch).unwrap();
    assert_eq!(cfg.name, "Shop 2");
    // the board goes to the global `[[sources]]`, never to `[project.tracker]`
    assert!(cfg.tracker.is_none());
    let sources = svc.effective(None).sources.clone();
    assert_eq!(sources.len(), 1);
    assert_eq!(sources[0].projects, vec![ProjectId::new("shop")]);
    assert_eq!(sources[0].tracker.views[0].jql.as_deref(), Some("assignee = currentUser()"));
    let text = read(&file);
    assert!(text.contains("name = \"Shop 2\" # display name"), "{text}");
    assert!(!text.contains("tracker"), "{text}");
    svc.project_update(&ProjectId::new("shop"), &ProjectPatch { remove_tracker: true, ..Default::default() })
        .unwrap();
    // the source stays, feeding no project
    let sources = svc.effective(None).sources.clone();
    assert_eq!(sources.len(), 1);
    assert!(sources[0].projects.is_empty());

    // removal moves the file to .trash
    svc.project_remove(&ProjectId::new("shop")).unwrap();
    assert!(!file.exists());
    let trashed: Vec<_> =
        std::fs::read_dir(e.dirs.projects_dir().join(".trash")).unwrap().flatten().collect();
    assert_eq!(trashed.len(), 1);
    assert!(svc.projects().is_empty());
    assert_eq!(svc.project_remove(&ProjectId::new("shop")).unwrap_err().code, ErrorCode::NotFound);
    // and a fresh service does not resurrect it
    assert!(e.load().projects().is_empty());
}

#[test]
fn project_update_rejects_broken_results() {
    let e = env();
    let svc = e.load();
    svc.project_create(&draft(&e, "shop")).unwrap();
    let before = read(&e.dirs.projects_dir().join("shop.toml"));
    let err = svc
        .project_update(
            &ProjectId::new("shop"),
            &ProjectPatch { default_template: Some("ghost".into()), ..Default::default() },
        )
        .unwrap_err();
    assert_eq!(err.code, ErrorCode::InvalidArgument);
    assert_eq!(read(&e.dirs.projects_dir().join("shop.toml")), before);
}

#[test]
fn concurrent_sets_all_land() {
    // same table, many rounds: a read taken before the write gate loses an update
    let edits = [
        ("terminal.font_size", json!(15)),
        ("terminal.cursor_style", json!("bar")),
        ("terminal.renderer", json!("webgl")),
        ("terminal.scrollback.shell", json!(4000)),
    ];
    for _ in 0..50 {
        let e = env();
        e.global(GLOBAL);
        let svc = e.load();
        std::thread::scope(|s| {
            for (k, v) in &edits {
                let svc = svc.clone();
                s.spawn(move || svc.layer_set(Layer::Global, None, None, k, v.clone()).unwrap());
            }
        });
        let text = read(&e.dirs.global_config());
        for needle in ["font_size = 15", "cursor_style = \"bar\"", "renderer = \"webgl\"", "shell = 4000"] {
            assert!(text.contains(needle), "lost `{needle}` in:\n{text}");
        }
    }
}

#[test]
fn new_project_failing_merged_validation_is_rejected() {
    let e = env();
    let svc = e.load();
    let mut d = draft(&e, "shop");
    d.default_template = Some("nope".into());
    assert_eq!(svc.project_create(&d).unwrap_err().code, ErrorCode::InvalidArgument);
    assert!(!e.dirs.projects_dir().join("shop.toml").exists());
}

// ---- ticket sources (#238) ------------------------------------------------------------------

fn board(columns: &str) -> String {
    format!(
        "[project.tracker]\naccount = \"jira-acme\"\nviews = [{{ id = \"mine\", label = \"My open\", jql = \"assignee = currentUser()\" }}]\n{columns}\nstatus_map = {{ start = {{ name = \"In Progress\" }} }}\n"
    )
}

fn trashed(e: &common::Env) -> Vec<String> {
    let mut v: Vec<String> = std::fs::read_dir(e.dirs.projects_dir().join(".trash"))
        .map(|rd| rd.flatten().map(|f| f.file_name().to_string_lossy().into_owned()).collect())
        .unwrap_or_default();
    v.sort();
    v
}

#[test]
fn migration_moves_trackers_to_sources_once() {
    let e = env();
    e.global(GLOBAL);
    let shared = board("columns = [{ id = \"todo\", label = \"To do\", categories = [\"todo\"] }]");
    e.project(
        "web",
        &format!("{shared}repo_rules = [{{ match = {{ component = \"Web\" }}, repo = \"api\" }}]\n"),
    );
    e.project("api", &shared);
    e.project("cli", &board(""));
    let svc = e.load();
    assert_eq!(svc.migrate_project_trackers().unwrap(), 3);

    let s = svc.effective(None).sources.clone();
    let ids: Vec<(&str, &str)> = s.iter().map(|x| (x.id.as_str(), x.label.as_str())).collect();
    assert_eq!(ids, [("my-open-api", "My open · api"), ("my-open-cli", "My open · cli")]);
    assert_eq!(s[0].projects, [ProjectId::new("api"), ProjectId::new("web")]);
    assert_eq!(s[0].routes.len(), 1);
    assert_eq!(s[0].routes[0].project, ProjectId::new("web"));
    let global = read(&e.dirs.global_config());
    assert!(global.contains("theme = \"dark\" # keep me") && global.contains("[[sources]]"), "{global}");
    let trash = trashed(&e);
    assert_eq!(trash.len(), 3, "{trash:?}");
    assert!(trash.iter().all(|f| f.contains("-tracker-")), "{trash:?}");
    for id in ["web", "api", "cli"] {
        let text = read(&e.dirs.projects_dir().join(format!("{id}.toml")));
        assert!(!text.contains("tracker"), "{text}");
        assert!(svc.project(&ProjectId::new(id)).unwrap().tracker.is_none());
    }

    // a re-run (next launch) changes nothing
    let svc = e.load();
    assert_eq!(svc.migrate_project_trackers().unwrap(), 0);
    assert_eq!(read(&e.dirs.global_config()), global);
    assert_eq!(trashed(&e).len(), 3);
}

#[test]
fn migration_leaves_an_account_less_block_in_place() {
    let e = env();
    e.global(GLOBAL);
    e.project("web", &board(""));
    let columns =
        "[project.tracker]\ncolumns = [{ id = \"todo\", label = \"To do\", categories = [\"todo\"] }]\n";
    e.project("old", columns);
    let before = read(&e.dirs.projects_dir().join("old.toml"));
    let svc = e.load();
    assert_eq!(svc.migrate_project_trackers().unwrap(), 1);
    assert_eq!(svc.effective(None).sources.len(), 1);
    assert_eq!(read(&e.dirs.projects_dir().join("old.toml")), before);
    assert!(svc.project(&ProjectId::new("old")).unwrap().tracker.is_some());
    assert_eq!(trashed(&e).len(), 1, "{:?}", trashed(&e));
    // next launch: nothing left to move, no new copies
    let svc = e.load();
    assert_eq!(svc.migrate_project_trackers().unwrap(), 0);
    assert_eq!(trashed(&e).len(), 1);
}

#[test]
fn legacy_blocks_read_as_sources_in_both_the_typed_and_the_ui_value() {
    let e = env();
    e.global(GLOBAL);
    e.project("web", &board(""));
    // no binding account: the view's own account is the source's (#264), as core and the UI see it
    e.project(
        "api",
        "[project.tracker]\nviews = [{ id = \"team\", label = \"Team\", jql = \"x\", account = \"jira-two\" }]\n",
    );
    e.project(
        "old",
        "[project.tracker]\ncolumns = [{ id = \"todo\", label = \"To do\", categories = [\"todo\"] }]\n",
    );
    let svc = e.load();
    let typed: Vec<(String, String)> =
        svc.effective(None).sources.iter().map(|s| (s.id.clone(), s.tracker.account.to_string())).collect();
    assert_eq!(typed, [("team".into(), "jira-two".into()), ("my-open".into(), "jira-acme".into())]);
    let ui = svc.effective_doc(None).unwrap().value["sources"].clone();
    assert_eq!(ui, serde_json::to_value(&svc.effective(None).sources).unwrap(), "the UI reads the same list");
    let web = svc.effective(Some(&ProjectId::new("web")));
    assert_eq!(web.sources, svc.effective(None).sources);
    // the account-less view block moves too; the block with no account anywhere stays, unused
    assert_eq!(svc.migrate_project_trackers().unwrap(), 2);
    assert!(svc.project(&ProjectId::new("old")).unwrap().tracker.is_some());
    assert_eq!(svc.effective(None).sources.len(), 2);
}

#[test]
fn migration_after_a_crash_between_writes_only_strips() {
    let e = env();
    e.project("web", &board(""));
    let svc = e.load();
    svc.migrate_project_trackers().unwrap();
    let global = read(&e.dirs.global_config());
    // the global file was written, the project file was not stripped
    e.project("web", &board(""));
    let svc = e.load();
    assert_eq!(svc.migrate_project_trackers().unwrap(), 1);
    assert_eq!(read(&e.dirs.global_config()), global);
    assert!(!read(&e.dirs.projects_dir().join("web.toml")).contains("tracker"));
    assert_eq!(svc.effective(None).sources[0].projects, [ProjectId::new("web")]);
}

#[cfg(unix)]
#[test]
fn migration_failure_leaves_project_files_untouched() {
    use std::os::unix::fs::PermissionsExt;
    let e = env();
    e.global(GLOBAL);
    e.project("web", &board(""));
    let before = read(&e.dirs.projects_dir().join("web.toml"));
    let svc = e.load();
    let dir = e.dirs.global_config().parent().unwrap().to_path_buf();
    std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o555)).unwrap();
    let writable = std::fs::write(dir.join("probe"), "").is_ok(); // root ignores modes
    let r = svc.migrate_project_trackers();
    std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o755)).unwrap();
    if writable {
        return;
    }
    assert!(r.is_err());
    assert_eq!(read(&e.dirs.global_config()), GLOBAL);
    assert_eq!(read(&e.dirs.projects_dir().join("web.toml")), before);
    // the read path still sees the block
    assert!(svc.project(&ProjectId::new("web")).unwrap().tracker.is_some());
}

#[test]
fn project_create_with_a_tracker_joins_an_equal_source() {
    let e = env();
    let svc = e.load();
    let t = kelta_proto::samples::tracker_binding();
    let mut d = draft(&e, "shop");
    d.tracker = Some(t.clone());
    svc.project_create(&d).unwrap();
    let mut d = draft(&e, "blog");
    d.tracker = Some(t.clone());
    svc.project_create(&d).unwrap();
    for id in ["shop", "blog"] {
        assert!(!read(&e.dirs.projects_dir().join(format!("{id}.toml"))).contains("tracker"));
    }
    let s = svc.effective(None).sources.clone();
    assert_eq!(s.len(), 1, "{s:?}");
    assert_eq!(s[0].projects, [ProjectId::new("shop"), ProjectId::new("blog")]);
    assert_eq!(s[0].binding_for(&ProjectId::new("blog")), t);
    // an update replaces that source's board and this project's routes
    let mut t2 = t.clone();
    t2.repo_rules.clear();
    t2.columns.clear();
    svc.project_update(&ProjectId::new("blog"), &ProjectPatch { tracker: Some(t2), ..Default::default() })
        .unwrap();
    let s = svc.effective(None).sources.clone();
    assert_eq!(s.len(), 1);
    assert!(s[0].tracker.columns.is_empty());
    assert_eq!(s[0].binding_for(&ProjectId::new("shop")).repo_rules, t.repo_rules);
    assert!(s[0].binding_for(&ProjectId::new("blog")).repo_rules.is_empty());
}

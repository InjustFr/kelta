#![allow(clippy::unwrap_used, clippy::expect_used)]
mod common;

use std::sync::{Arc, Mutex};

use common::{env, read};
use kelta_proto::api::SettingsSource;
use kelta_proto::error::ErrorCode;
use kelta_proto::ids::ProjectId;
use kelta_proto::model::{CodeHostHint, ProjectDraft, ProjectPatch, RepoDraft};
use kelta_proto::settings::{Layer, SettingsDiff};
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
    assert_eq!(cfg.tracker.as_ref().unwrap().views[0].jql.as_deref(), Some("assignee = currentUser()"));
    let text = read(&file);
    assert!(text.contains("name = \"Shop 2\" # display name"), "{text}");
    let cfg = svc
        .project_update(&ProjectId::new("shop"), &ProjectPatch { remove_tracker: true, ..Default::default() })
        .unwrap();
    assert!(cfg.tracker.is_none());

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
    let e = env();
    e.global(GLOBAL);
    let svc = e.load();
    let edits = [
        ("terminal.font_size", json!(15)),
        ("terminal.cursor_style", json!("bar")),
        ("window.restore_geometry", json!(false)),
        ("notifications.enabled", json!(false)),
        ("app.theme", json!("light")),
    ];
    std::thread::scope(|s| {
        for (k, v) in &edits {
            let svc = svc.clone();
            s.spawn(move || svc.layer_set(Layer::Global, None, None, k, v.clone()).unwrap());
        }
    });
    let text = read(&e.dirs.global_config());
    for needle in [
        "font_size = 15",
        "cursor_style = \"bar\"",
        "restore_geometry = false",
        "enabled = false",
        "theme = \"light\"",
    ] {
        assert!(text.contains(needle), "lost `{needle}` in:\n{text}");
    }
}

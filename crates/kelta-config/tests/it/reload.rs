use std::sync::mpsc;
use std::time::Duration;

use crate::common::{env, read};
use kelta_proto::api::SettingsSource;
use kelta_proto::ids::ProjectId;
use kelta_proto::settings::{Layer, SettingsDiff};

#[test]
fn valid_external_edit_produces_a_diff() {
    let e = env();
    e.global("[terminal]\nfont_size = 13\n");
    e.project("shop", "");
    let svc = e.load();
    std::fs::write(
        e.dirs.global_config(),
        "[terminal]\nfont_size = 14\n[linux.graphics]\ndisable_dmabuf = true\n",
    )
    .unwrap();
    let diff = svc.reload().expect("a change");
    assert_eq!(diff.layers, vec![Layer::Global]);
    assert_eq!(diff.paths, vec!["linux.graphics.disable_dmabuf".to_owned(), "terminal.font_size".to_owned()]);
    assert_eq!(diff.requires_restart, vec!["linux.graphics.disable_dmabuf".to_owned()]);
    assert_eq!(svc.effective(None).terminal.font_size, 14.0);
    assert_eq!(svc.effective(Some(&ProjectId::new("shop"))).terminal.font_size, 14.0);
    assert!(svc.reload().is_none());
}

#[test]
fn invalid_edit_keeps_last_good_reports_position_and_recovers() {
    let e = env();
    e.global("[terminal]\nfont_size = 14\n");
    let svc = e.load();
    let (tx, rx) = mpsc::channel::<SettingsDiff>();
    svc.watch(Box::new(move |d| {
        let _ = tx.send(d);
    }));
    let (itx, irx) = mpsc::channel();
    svc.watch_issues(Box::new(move |i| {
        let _ = itx.send(i);
    }));

    std::fs::write(e.dirs.global_config(), "[terminal]\nfont_size = 14\nrenderer = = \"dom\"\n").unwrap();
    let diff = svc.reload().expect("issue list changed");
    assert!(diff.paths.is_empty(), "nothing applied");
    assert_eq!(diff.layers, vec![Layer::Global]);
    assert_eq!(svc.effective(None).terminal.font_size, 14.0, "last good config kept");
    let issues = svc.issues();
    assert_eq!(issues.len(), 1);
    assert_eq!((issues[0].issue.line, issues[0].issue.col), (Some(3), Some(12)));
    assert!(issues[0].display().contains("config.toml:3:12"), "{}", issues[0].display());
    assert_eq!(irx.try_recv().unwrap().len(), 1);
    // the on-disk text is exposed for the editor, the value stays last good
    let doc = svc.layer_get(Layer::Global, None, None).unwrap();
    assert!(doc.text.contains("= ="));
    assert_eq!(doc.value["terminal"]["font_size"], 14);
    // the same broken file is not re-reported
    assert!(svc.reload().is_none());

    // a schema error keeps the previous value as well
    std::fs::write(e.dirs.global_config(), "[terminal]\nfont_size = 400\n").unwrap();
    svc.reload();
    assert_eq!(svc.effective(None).terminal.font_size, 14.0);
    assert!(svc.issues()[0].issue.message.contains("maximum"), "{:?}", svc.issues());

    // fixing the file applies it and clears the issues
    std::fs::write(e.dirs.global_config(), "[terminal]\nfont_size = 20\n").unwrap();
    let diff = svc.reload().unwrap();
    assert_eq!(diff.paths, vec!["terminal.font_size".to_owned()]);
    assert!(svc.issues().is_empty());
    assert_eq!(svc.effective(None).terminal.font_size, 20.0);
    let all: Vec<SettingsDiff> = rx.try_iter().collect();
    assert_eq!(all.len(), 3);
}

#[test]
fn semantic_failure_after_external_edit_reverts_the_layer() {
    let e = env();
    e.global("[editor]\ndefault = \"vim\"\n");
    let svc = e.load();
    std::fs::write(e.dirs.global_config(), "[editor]\ndefault = \"ghost\"\n").unwrap();
    svc.reload();
    assert_eq!(svc.effective(None).editor.default, "vim");
    assert!(
        svc.issues().iter().any(|i| i.issue.message.contains("unknown editor preset")),
        "{:?}",
        svc.issues()
    );
}

#[test]
fn project_files_come_and_go_on_reload() {
    let e = env();
    let svc = e.load();
    assert!(svc.projects().is_empty());
    e.project("shop", "[terminal]\nfont_size = 12\n");
    let diff = svc.reload().unwrap();
    assert_eq!(diff.layers, vec![Layer::Project]);
    assert_eq!(svc.projects().len(), 1);
    // a broken project edit keeps the previous project config and values
    let file = e.dirs.projects_dir().join("shop.toml");
    let text = read(&file).replace("font_size = 12", "font_size = = 12");
    std::fs::write(&file, text).unwrap();
    svc.reload();
    assert_eq!(svc.effective(Some(&ProjectId::new("shop"))).terminal.font_size, 12.0);
    assert_eq!(svc.issues().len(), 1);
    std::fs::remove_file(&file).unwrap();
    svc.reload();
    assert!(svc.projects().is_empty());
    assert!(svc.issues().is_empty());
}

#[test]
fn directory_watch_delivers_a_single_debounced_change() {
    let e = env();
    e.global("[terminal]\nfont_size = 13\n");
    let svc = e.load();
    let (tx, rx) = mpsc::channel::<SettingsDiff>();
    svc.watch(Box::new(move |d| {
        let _ = tx.send(d);
    }));
    // a burst of editor-style saves (write temp + rename) inside one settle window
    for size in 14..18 {
        let tmp = e.dirs.config.join("config.toml.swp");
        std::fs::write(&tmp, format!("[terminal]\nfont_size = {size}\n")).unwrap();
        std::fs::rename(&tmp, e.dirs.global_config()).unwrap();
    }
    let diff = rx.recv_timeout(Duration::from_secs(10)).expect("watcher delivered the change");
    assert_eq!(diff.paths, vec!["terminal.font_size".to_owned()]);
    assert_eq!(svc.effective(None).terminal.font_size, 17.0);
    assert!(
        rx.recv_timeout(Duration::from_millis(800)).is_err(),
        "burst must collapse into one notification"
    );

    // an invalid save is reported through the same path and the last good value stays
    std::fs::write(e.dirs.global_config(), "[terminal\nfont_size = 1\n").unwrap();
    let diff = rx.recv_timeout(Duration::from_secs(10)).expect("invalid save reported");
    assert!(diff.paths.is_empty());
    assert_eq!(svc.effective(None).terminal.font_size, 17.0);
    assert_eq!(svc.issues()[0].issue.line, Some(1));

    // new project files are picked up by the directory watch
    e.project("shop", "");
    let diff = rx.recv_timeout(Duration::from_secs(10)).expect("project file seen");
    assert_eq!(diff.layers, vec![Layer::Project]);
    assert_eq!(svc.projects().len(), 1);
}

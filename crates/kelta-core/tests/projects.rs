//! Project lifecycle: Home, open/close/activate/reorder persistence, sessions survive close/switch
//! unless `kill_sessions`, detection from a folder.
#![allow(clippy::unwrap_used)] // fixture helpers outside #[test] fns

mod common;

use common::*;
use kelta_core::Core;
use kelta_proto::api::CoreApi;
use kelta_proto::ids::ProjectId;
use kelta_proto::model::{Lifecycle, SessionKind, SpawnRequest};
use kelta_proto::settings::{AccountKind, Settings};

fn shell(project: &str) -> SpawnRequest {
    SpawnRequest {
        id: None,
        project_id: ProjectId::new(project),
        kind: SessionKind::Shell,
        name: None,
        program: None,
        args: vec![],
        cwd: None,
        env: Default::default(),
        cols: 100,
        rows: 30,
        work_item_id: None,
        restore: Default::default(),
        close_on_exit: Default::default(),
        template_id: None,
    }
}

fn ids(core: &Core) -> Vec<String> {
    core.project_list().into_iter().filter(|p| p.open).map(|p| p.id.to_string()).collect()
}

#[tokio::test]
async fn home_is_open_and_active_by_default() {
    let tmp = tempfile::tempdir().unwrap();
    let h = start(tmp.path(), Settings::defaults(), vec![project("shop", tmp.path())]);
    let list = h.core.project_list();
    let home = list.iter().find(|p| p.id.as_str() == "home").unwrap();
    assert!(home.builtin && home.open && home.active);
    let shop = list.iter().find(|p| p.id.as_str() == "shop").unwrap();
    assert!(!shop.open);
    assert!(shop.repos[0].exists);
    assert_eq!(
        h.core.project_remove(&ProjectId::home(), false).unwrap_err().code,
        kelta_proto::ErrorCode::InvalidArgument
    );
}

#[tokio::test]
async fn close_and_switch_keep_sessions_unless_killed() {
    let tmp = tempfile::tempdir().unwrap();
    let h = start(
        tmp.path(),
        Settings::defaults(),
        vec![project("shop", tmp.path()), project("blog", tmp.path())],
    );
    let shop = ProjectId::new("shop");
    let blog = ProjectId::new("blog");
    h.core.project_activate(&shop).unwrap();
    h.core.project_open(&blog).unwrap();
    let s = h.core.session_spawn(shell("shop")).await.unwrap();
    assert_eq!(s.lifecycle, Lifecycle::Live);
    assert!(s.cwd.ends_with("repos/shop"));

    // switch: nothing killed
    h.core.project_activate(&blog).unwrap();
    assert!(h.term.with_session(&s.id, |x| x.killed).unwrap().is_none());
    // close without kill: session keeps running
    let info = h.core.project_close(&shop, false).unwrap();
    assert!(!info.open && !info.active);
    assert!(h.term.with_session(&s.id, |x| x.killed).unwrap().is_none());
    assert_eq!(h.core.session_get(&s.id).unwrap().lifecycle, Lifecycle::Live);
    assert_eq!(ids(&h.core), vec!["home", "blog"]);

    // reopen + close with kill: session killed and removed
    h.core.project_open(&shop).unwrap();
    h.core.project_close(&shop, true).unwrap();
    assert!(h.term.with_session(&s.id, |x| x.killed).unwrap().is_some());
    assert!(h.core.session_get(&s.id).is_none());
    assert!(h.ui.event_names().contains(&"session.removed"));
}

#[tokio::test]
async fn open_set_order_and_active_persist() {
    let tmp = tempfile::tempdir().unwrap();
    let projects = || vec![project("a", tmp.path()), project("b", tmp.path()), project("c", tmp.path())];
    {
        let h = start(tmp.path(), Settings::defaults(), projects());
        for p in ["a", "b", "c"] {
            h.core.project_open(&ProjectId::new(p)).unwrap();
        }
        // non-adjacent duplicates are dropped
        h.core.project_reorder(&[ProjectId::new("c"), ProjectId::new("a"), ProjectId::new("c")]).unwrap();
        h.core.project_activate(&ProjectId::new("b")).unwrap();
        assert_eq!(ids(&h.core), vec!["c", "a", "home", "b"]);
        h.core.store().flush().await.unwrap();
    }
    let h = start(tmp.path(), Settings::defaults(), projects());
    assert_eq!(ids(&h.core), vec!["c", "a", "home", "b"]);
    assert_eq!(h.core.active_project().as_str(), "b");
}

#[tokio::test]
async fn create_opens_and_activates() {
    let tmp = tempfile::tempdir().unwrap();
    let h = start(tmp.path(), Settings::defaults(), vec![]);
    let repo = tmp.path().join("work").join("My Shop");
    std::fs::create_dir_all(repo.join(".git/refs/heads/feat")).unwrap();
    std::fs::write(
        repo.join(".git/config"),
        "[core]\n\tbare = false\n[remote \"origin\"]\n\turl = git@github.com:acme/shop.git\n",
    )
    .unwrap();
    std::fs::write(repo.join(".git/refs/heads/feat/SHOP-12-login"), "x").unwrap();
    std::fs::create_dir_all(repo.join(".github")).unwrap();
    let mut settings = Settings::defaults();
    settings.accounts.insert("github-work".into(), account(AccountKind::Github));
    settings.accounts.get_mut(&kelta_proto::ids::AccountId::new("github-work")).unwrap().base_url = None;
    settings.accounts.insert("jira-acme".into(), account(AccountKind::Jira));
    *h.cfg.global.write() = settings;

    let draft = h.core.project_detect(&repo).unwrap();
    assert_eq!(draft.suggested_id.as_str(), "my-shop");
    assert_eq!(draft.repos.len(), 1);
    assert_eq!(draft.repos[0].remote_url.as_deref(), Some("git@github.com:acme/shop.git"));
    let hint = &draft.code_host_hints[0];
    assert_eq!((hint.kind.as_str(), hint.repo.as_str()), ("github", "acme/shop"));
    assert_eq!(hint.account.as_ref().map(|a| a.as_str()), Some("github-work"));
    assert_eq!(draft.tracker_hints[0].kind, "jira");
    assert_eq!(draft.tracker_hints[0].key.as_deref(), Some("SHOP"));
    assert_eq!(draft.tracker_hints[0].account.as_ref().map(|a| a.as_str()), Some("jira-acme"));
    assert!(draft.tracker_hints.iter().any(|t| t.kind == "github"));

    let info = h.core.project_create(&draft).unwrap();
    assert!(info.open && info.active);
    assert_eq!(h.core.project_create(&draft).unwrap_err().code, kelta_proto::ErrorCode::Conflict);
    // detecting again suggests a fresh id
    assert_eq!(h.core.project_detect(&repo).unwrap().suggested_id.as_str(), "my-shop-2");
    // ctl open of a path inside the repo focuses the existing project
    let v = h.core.ctl(kelta_proto::ctl::CtlCommand::Open { path: repo.join("src") }).await.unwrap();
    assert_eq!(v["id"], "my-shop");
}

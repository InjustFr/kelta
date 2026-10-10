//! Shared test setup: a `PluginHost` over `FakeCore`, a temp data dir and an in-memory grant store.
#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::sync::{Arc, Weak};

use kelta_plugins::PluginHost;
use kelta_proto::api::{CoreApi, GrantStore};
use kelta_proto::dirs::Dirs;
use kelta_proto::model::ProjectInfo;
use kelta_proto::settings::Settings;
use kelta_proto::testing::{FakeCore, MemGrantStore, fixtures};

pub struct Env {
    pub core: Arc<FakeCore>,
    pub host: Arc<PluginHost>,
    pub grants: Arc<MemGrantStore>,
    pub tmp: tempfile::TempDir,
}

impl Env {
    pub fn new() -> Self {
        let core = FakeCore::new();
        let tmp = tempfile::tempdir().unwrap();
        let mut project: ProjectInfo = fixtures::load("project_info").unwrap();
        let repo = tmp.path().join("repo");
        std::fs::create_dir_all(&repo).unwrap();
        for r in &mut project.repos {
            r.path = repo.clone();
        }
        core.add_project(project);
        let weak: Weak<dyn CoreApi> = Arc::downgrade(&(core.clone() as Arc<dyn CoreApi>));
        let grants = Arc::new(MemGrantStore::new());
        let host = PluginHost::new(weak, Dirs::under(tmp.path()), grants.clone() as Arc<dyn GrantStore>);
        Self { core, host, grants, tmp }
    }

    pub fn with_settings(self, f: impl FnOnce(&mut Settings)) -> Self {
        let mut s = Settings::defaults();
        f(&mut s);
        self.core.set_settings(s);
        self.host.refresh();
        self
    }

    pub fn plugins_dir(&self) -> PathBuf {
        self.host.dirs().plugins_dir()
    }

    /// Write a plugin directly into the data dir.
    pub fn write_plugin(&self, id: &str, manifest: &str, files: &[(&str, &str)]) -> PathBuf {
        let dir = self.plugins_dir().join(id);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("kelta-plugin.toml"), manifest).unwrap();
        for (rel, content) in files {
            let p = dir.join(rel);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, content).unwrap();
        }
        self.host.refresh();
        dir
    }
}

pub fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..").canonicalize().unwrap()
}

pub fn example(name: &str) -> PathBuf {
    repo_root().join("examples/plugins").join(name)
}

pub fn manifest(id: &str, permissions: &[&str], extra: &str) -> String {
    let perms: Vec<String> = permissions.iter().map(|p| format!("{p:?}")).collect();
    format!(
        "id = \"{id}\"\nname = \"{id}\"\nversion = \"0.1.0\"\nkelta_api = \"^0.1\"\ndescription = \"test\"\n\
         author = \"t\"\nlicense = \"MIT\"\npermissions = [{}]\n{extra}",
        perms.join(", ")
    )
}

/// Poll a condition (tests only) for up to ~5 s.
pub async fn wait_for(mut cond: impl FnMut() -> bool) -> bool {
    for _ in 0..500 {
        if cond() {
            return true;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    cond()
}

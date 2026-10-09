#![allow(dead_code, clippy::unwrap_used, clippy::expect_used)]

use std::future::Future;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::task::{Context, Poll, Waker};

use kelta_config::ConfigService;
use kelta_proto::dirs::Dirs;
use kelta_proto::settings::RuntimeOverrides;
use tempfile::TempDir;

pub struct Env {
    pub tmp: TempDir,
    pub dirs: Dirs,
    pub repo: PathBuf,
}

pub fn env() -> Env {
    let tmp = tempfile::tempdir().unwrap();
    let dirs = Dirs::under(tmp.path());
    std::fs::create_dir_all(dirs.projects_dir()).unwrap();
    let repo = tmp.path().join("repo-a");
    std::fs::create_dir_all(&repo).unwrap();
    Env { tmp, dirs, repo }
}

impl Env {
    pub fn global(&self, text: &str) {
        std::fs::write(self.dirs.global_config(), text).unwrap();
    }

    pub fn project(&self, id: &str, extra: &str) {
        let text = format!(
            "[project]\nid = \"{id}\"\nname = \"{id}\"\n\n[[project.repos]]\nid = \"api\"\npath = \"{}\"\nprimary = true\n\n{extra}",
            self.repo.display()
        );
        std::fs::write(self.dirs.projects_dir().join(format!("{id}.toml")), text).unwrap();
    }

    pub fn repo_file(&self, text: &str) -> PathBuf {
        let dir = self.repo.join(".kelta");
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("config.toml");
        std::fs::write(&p, text).unwrap();
        p
    }

    pub fn load(&self) -> Arc<ConfigService> {
        ConfigService::load(&self.dirs, RuntimeOverrides::default()).unwrap()
    }

    pub fn load_with(&self, o: RuntimeOverrides) -> Arc<ConfigService> {
        ConfigService::load(&self.dirs, o).unwrap()
    }
}

pub fn read(p: &Path) -> String {
    std::fs::read_to_string(p).unwrap()
}

/// Minimal executor for futures that are ready without a reactor (in-memory stores).
pub fn block_on<F: Future>(f: F) -> F::Output {
    let mut f = Box::pin(f);
    let mut cx = Context::from_waker(Waker::noop());
    loop {
        if let Poll::Ready(v) = f.as_mut().poll(&mut cx) {
            return v;
        }
        std::thread::yield_now();
    }
}

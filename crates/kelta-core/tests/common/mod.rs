//! Shared test harness: a `Core` wired with the `kelta_proto::testing` fakes.
#![allow(dead_code, clippy::unwrap_used, clippy::type_complexity)] // test helpers, not #[test] fns

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use async_trait::async_trait;
use kelta_core::clipboard::MemClipboard;
use kelta_core::{ConfigBackend, Core, CoreDeps};
use kelta_http::{HttpCtx, ProviderFactory};
use kelta_proto::api::{CodeHost, SecretResolver, SettingsSource, Tracker};
use kelta_proto::codehost::{CodeHostKind, PrCreate, Review, ReviewDetail, ReviewQuery, ReviewRef};
use kelta_proto::dirs::{CliArgs, Dirs};
use kelta_proto::error::KeltaError;
use kelta_proto::ids::{AccountId, ProjectId};
use kelta_proto::model::{ProjectDraft, ProjectPatch};
use kelta_proto::settings::{AccountConfig, AccountKind, ProjectConfig, RepoConfig, Settings, SettingsDiff};
use kelta_proto::term::{LoginEnv, LoginEnvSource};
use kelta_proto::testing::{FakeSecrets, FakeTerminalHost, FakeUiBridge};
use kelta_proto::tracker::User;
use parking_lot::{Mutex, RwLock};

/// In-memory settings + project CRUD.
#[derive(Default)]
pub struct MemConfig {
    pub global: RwLock<Settings>,
    pub projects: RwLock<Vec<Arc<ProjectConfig>>>,
    pub env: RwLock<HashMap<ProjectId, BTreeMap<String, String>>>,
    watchers: Mutex<Vec<Box<dyn Fn(SettingsDiff) + Send + Sync>>>,
}

impl MemConfig {
    pub fn new(settings: Settings, projects: Vec<ProjectConfig>) -> Arc<Self> {
        Arc::new(Self {
            global: RwLock::new(settings),
            projects: RwLock::new(projects.into_iter().map(Arc::new).collect()),
            env: RwLock::new(HashMap::new()),
            watchers: Mutex::new(Vec::new()),
        })
    }

    pub fn update(&self, f: impl FnOnce(&mut Settings)) {
        f(&mut self.global.write());
        for w in self.watchers.lock().iter() {
            w(SettingsDiff { layers: vec![], paths: vec!["*".into()], requires_restart: vec![] });
        }
    }
}

impl SettingsSource for MemConfig {
    fn effective(&self, project: Option<&ProjectId>) -> Arc<Settings> {
        let mut s = self.global.read().clone();
        if let Some(p) = project
            && let Some(env) = self.env.read().get(p)
        {
            s.env = env.clone();
        }
        Arc::new(s)
    }

    fn project(&self, id: &ProjectId) -> Option<Arc<ProjectConfig>> {
        self.projects.read().iter().find(|p| &p.id == id).cloned()
    }

    fn projects(&self) -> Vec<Arc<ProjectConfig>> {
        self.projects.read().clone()
    }
}

impl ConfigBackend for MemConfig {
    fn project_create(&self, draft: &ProjectDraft) -> Result<Arc<ProjectConfig>, KeltaError> {
        let p = Arc::new(ProjectConfig {
            id: draft.suggested_id.clone(),
            name: draft.name.clone(),
            color: draft.color.clone(),
            icon: draft.icon.clone(),
            default_template: draft.default_template.clone(),
            repos: draft
                .repos
                .iter()
                .map(|r| RepoConfig {
                    id: r.id.clone(),
                    path: r.path.to_string_lossy().into_owned(),
                    primary: r.primary,
                    remote: r.remote.clone(),
                    base: r.base.clone(),
                    code_host: r.code_host.clone(),
                })
                .collect(),
            tracker: draft.tracker.clone(),
        });
        self.projects.write().push(p.clone());
        Ok(p)
    }

    fn project_update(&self, id: &ProjectId, patch: &ProjectPatch) -> Result<Arc<ProjectConfig>, KeltaError> {
        let mut ps = self.projects.write();
        let p = ps.iter_mut().find(|p| &p.id == id).ok_or_else(|| KeltaError::not_found("project"))?;
        let mut c = (**p).clone();
        if let Some(n) = &patch.name {
            c.name = n.clone();
        }
        if patch.remove_tracker {
            c.tracker = None;
        } else if let Some(t) = &patch.tracker {
            c.tracker = Some(t.clone());
        }
        *p = Arc::new(c);
        Ok(p.clone())
    }

    fn project_remove(&self, id: &ProjectId) -> Result<(), KeltaError> {
        self.projects.write().retain(|p| &p.id != id);
        Ok(())
    }

    fn watch(&self, on_change: Box<dyn Fn(SettingsDiff) + Send + Sync>) {
        self.watchers.lock().push(on_change);
    }
}

/// Providers by account id.
#[derive(Default)]
pub struct Factory {
    pub trackers: Mutex<HashMap<AccountId, Arc<dyn Tracker>>>,
    pub hosts: Mutex<HashMap<AccountId, Arc<dyn CodeHost>>>,
}

impl ProviderFactory for Factory {
    fn tracker(
        &self,
        account: &AccountConfig,
        http: HttpCtx,
        _secrets: Arc<dyn SecretResolver>,
    ) -> Result<Arc<dyn Tracker>, KeltaError> {
        let _ = account;
        self.trackers
            .lock()
            .get(http.account_id())
            .cloned()
            .ok_or_else(|| KeltaError::not_found(format!("no fake tracker for {}", http.account_id())))
    }

    fn code_host(
        &self,
        _account: &AccountConfig,
        http: HttpCtx,
        _secrets: Arc<dyn SecretResolver>,
    ) -> Result<Arc<dyn CodeHost>, KeltaError> {
        self.hosts
            .lock()
            .get(http.account_id())
            .cloned()
            .ok_or_else(|| KeltaError::not_found(format!("no fake code host for {}", http.account_id())))
    }
}

/// A code host whose review list the test mutates.
pub struct ListHost {
    pub reviews: Mutex<Vec<Review>>,
    pub calls: Mutex<u32>,
    /// `changed_since_last` answer (the notifications-ETag gate).
    pub changed: Mutex<bool>,
}

impl ListHost {
    pub fn new(reviews: Vec<Review>) -> Arc<Self> {
        Arc::new(Self { reviews: Mutex::new(reviews), calls: Mutex::new(0), changed: Mutex::new(true) })
    }
}

#[async_trait]
impl CodeHost for ListHost {
    fn kind(&self) -> CodeHostKind {
        CodeHostKind::Github
    }
    async fn me(&self) -> Result<User, KeltaError> {
        Ok(kelta_proto::samples::user())
    }
    async fn changed_since_last(&self) -> Result<bool, KeltaError> {
        Ok(*self.changed.lock())
    }
    async fn list_reviews(&self, q: &ReviewQuery) -> Result<Vec<Review>, KeltaError> {
        *self.calls.lock() += 1;
        Ok(self.reviews.lock().iter().filter(|r| r.kind == q.kind).cloned().collect())
    }
    async fn get(&self, r: &ReviewRef) -> Result<ReviewDetail, KeltaError> {
        Err(KeltaError::not_found(format!("{}", r.number)))
    }
    async fn approve(&self, _r: &ReviewRef, _head_sha: &str) -> Result<(), KeltaError> {
        Ok(())
    }
    async fn comment(&self, _r: &ReviewRef, _body: &str) -> Result<(), KeltaError> {
        Ok(())
    }
    async fn request_changes(&self, _r: &ReviewRef, _body: &str) -> Result<(), KeltaError> {
        Ok(())
    }
    async fn create(&self, _d: &PrCreate) -> Result<Review, KeltaError> {
        Err(KeltaError::unsupported("create"))
    }
    async fn find_for_branch(&self, _repo: &str, _branch: &str) -> Result<Option<Review>, KeltaError> {
        Ok(None)
    }
    fn fetch_refspec(&self, r: &ReviewRef, local: &str) -> String {
        format!("pull/{}/head:{local}", r.number)
    }
    fn repo_from_remote(&self, _url: &str) -> Option<String> {
        None
    }
}

pub struct H {
    pub core: Arc<Core>,
    pub term: Arc<FakeTerminalHost>,
    pub ui: Arc<FakeUiBridge>,
    pub cfg: Arc<MemConfig>,
    pub factory: Arc<Factory>,
    pub clip: Arc<MemClipboard>,
    pub root: PathBuf,
}

/// Executable stubs on the test PATH (`claude`, `nvim`, `lazygit`).
pub fn bin_dir(root: &Path) -> PathBuf {
    let bin = root.join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    // Written by a child `sh`: a write fd held here leaks into concurrent forks -> ETXTBSY (Linux).
    let ok = std::process::Command::new("sh")
        .args([
            "-c",
            "for f; do printf '#!/bin/sh\\nexit 0\\n' > \"$f\" && chmod 755 \"$f\" || exit 1; done",
            "sh",
        ])
        .args(["claude", "nvim", "lazygit"].map(|n| bin.join(n)))
        .status()
        .unwrap();
    assert!(ok.success());
    bin
}

pub fn login_env(root: &Path) -> LoginEnv {
    let bin = bin_dir(root);
    let mut vars = BTreeMap::new();
    vars.insert("PATH".to_owned(), format!("{}:/usr/bin:/bin", bin.display()));
    vars.insert("HOME".to_owned(), root.join("home").to_string_lossy().into_owned());
    vars.insert("SHELL".to_owned(), "/bin/sh".to_owned());
    vars.insert("KELTA_TICKET".to_owned(), "LEAKED".to_owned());
    std::fs::create_dir_all(root.join("home")).unwrap();
    LoginEnv { vars, source: LoginEnvSource::LoginInteractive, shell: Some(PathBuf::from("/bin/sh")) }
}

pub fn project(id: &str, root: &Path) -> ProjectConfig {
    let path = root.join("repos").join(id);
    std::fs::create_dir_all(path.join(".git")).unwrap();
    ProjectConfig {
        id: ProjectId::new(id),
        name: id.to_uppercase(),
        repos: vec![RepoConfig {
            id: "main".into(),
            path: path.to_string_lossy().into_owned(),
            primary: true,
            ..RepoConfig::default()
        }],
        ..ProjectConfig::default()
    }
}

pub fn account(kind: AccountKind) -> AccountConfig {
    AccountConfig {
        kind,
        base_url: Some("https://example.invalid".into()),
        flavor: Default::default(),
        auth: None,
        email: None,
        user: None,
        secret: None,
        text_format: Default::default(),
        poll_secs: None,
        web_url: None,
        plugin: None,
    }
}

/// Build a core in `root` (file database `root/data/kelta.db`).
pub fn start_in(root: &Path, cfg: Arc<MemConfig>, term: Arc<FakeTerminalHost>, factory: Arc<Factory>) -> H {
    let ui = FakeUiBridge::new();
    let clip = Arc::new(MemClipboard::default());
    let mut deps = CoreDeps::new(Dirs::under(root), CliArgs::default(), ui.clone());
    deps.config = Some(cfg.clone());
    deps.terminal = Some(term.clone());
    deps.secrets = Some(FakeSecrets::new());
    deps.trackers = Some(factory.clone());
    deps.code_hosts = Some(factory.clone());
    deps.login_env = Some(login_env(root));
    deps.clipboard = Some(clip.clone());
    deps.install_ctl = false;
    deps.start_services = false;
    let core = Core::start_with(deps).unwrap();
    H { core, term, ui, cfg, factory, clip, root: root.to_path_buf() }
}

pub fn start(root: &Path, settings: Settings, projects: Vec<ProjectConfig>) -> H {
    start_in(root, MemConfig::new(settings, projects), FakeTerminalHost::new(), Arc::new(Factory::default()))
}

/// Let spawned tasks run.
pub async fn settle() {
    for _ in 0..20 {
        tokio::task::yield_now().await;
    }
}

/// One request over the ctl socket (the path `kelta-ctl` takes in production).
pub async fn ctl_send(sock: &Path, req: serde_json::Value) -> serde_json::Value {
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt};
    let mut s = tokio::net::UnixStream::connect(sock).await.unwrap();
    s.write_all(format!("{req}\n").as_bytes()).await.unwrap();
    let mut line = String::new();
    tokio::io::BufReader::new(s).read_line(&mut line).await.unwrap();
    serde_json::from_str(&line).unwrap()
}

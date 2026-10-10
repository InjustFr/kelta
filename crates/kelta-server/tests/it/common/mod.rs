#![allow(dead_code, clippy::unwrap_used, clippy::expect_used)]

use std::sync::Arc;

use kelta_proto::api::CoreApi;
use kelta_proto::dirs::Dirs;
use kelta_proto::testing::FakeCore;
use kelta_server::Server;

pub struct Env {
    pub fake: Arc<FakeCore>,
    pub core: Arc<dyn CoreApi>,
    pub server: Arc<Server>,
    pub tmp: tempfile::TempDir,
}

pub fn env() -> Env {
    let fake = FakeCore::new();
    let core: Arc<dyn CoreApi> = fake.clone();
    // Short root: macOS limits unix socket paths to ~104 bytes.
    let tmp = tempfile::Builder::new().prefix("kl7").tempdir_in("/tmp").unwrap();
    let server = Server::new(Arc::downgrade(&core), Dirs::under(tmp.path()));
    Env { fake, core, server, tmp }
}

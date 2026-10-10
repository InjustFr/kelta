#![allow(clippy::unwrap_used, clippy::expect_used, clippy::disallowed_methods)] // test helpers outside #[test] fns

mod common;
mod daemon;
mod feeds;
mod layout;
mod oauth;
mod projects;
mod real_claude;
mod restore;
mod scheduler;
mod spawn;
mod status;
mod store;

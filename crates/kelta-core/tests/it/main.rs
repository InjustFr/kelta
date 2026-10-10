#![allow(clippy::unwrap_used, clippy::expect_used, clippy::disallowed_methods)] // test helpers outside #[test] fns

mod claude_mcp;
mod common;
mod daemon;
mod feeds;
mod layout;
mod oauth;
mod park;
mod projects;
mod real_claude;
mod restore;
mod scheduler;
mod sources;
mod spawn;
mod status;
mod store;
mod workflows;

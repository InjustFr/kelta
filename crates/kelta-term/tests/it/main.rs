#![allow(clippy::unwrap_used, clippy::expect_used, clippy::disallowed_methods)] // test helpers outside #[test] fns

mod common;
mod daemon;
mod pty;
mod queries;
mod record;
mod snapshot_roundtrip;

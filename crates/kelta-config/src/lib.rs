//! # kelta-config (L4)
//!
//! Layered settings (SETTINGS.md): Default, Plugin defaults, Global, Project, trusted Repo and
//! Runtime layers merged with a per-path provenance map; JSON Schema + semantic validation;
//! comment-preserving atomic `toml_edit` writes; project file CRUD; hot reload that keeps the
//! last good configuration; the early `[linux.graphics]` reader.

mod edit;
mod merge;
mod path;
mod schema_info;
mod service;
mod toml_io;
mod validate;
mod watch;
mod writes;

pub use path::{join_path, split_path};
pub use service::{ConfigIssue, ConfigService, OnChange, OnIssues, expand_home};
pub use watch::SETTLE;

/// Reads needed before any GTK/WebKit init (single-threaded `platform::pre_init`).
pub mod early;

/// Annotation lookups over the settings schema (`x-kelta-*`), for UI and tests.
pub mod schema {
    pub use crate::schema_info::{NodeInfo, SchemaIndex, index};
}

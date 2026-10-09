//! # kelta-proto
//!
//! The frozen contract shared by every Kelta crate (ARCHITECTURE §3-§6, §10; SETTINGS.md; PLUGINS.md):
//! ids, DTOs, IPC types, `UiEvent`/`BusEvent`, settings structs (+ `Default`, `JsonSchema`),
//! plugin/tool/trigger types, hook payloads, terminal frame constants, the ActionId catalog,
//! service traits and `KeltaError`.
//!
//! Feature `testing`: fakes (`FakeCore`, `FakeTerminalHost`, `FakeTracker`, `FakeCodeHost`,
//! `FakeSecrets`, `FakeSettings`, `FakeUiBridge`, in-memory stores) and `fixtures::load`.

pub mod actions;
pub mod api;
pub mod codehost;
pub mod ctl;
pub mod dirs;
pub mod error;
pub mod events;
pub mod ext;
pub mod hooks;
pub mod ids;
pub mod ipc;
pub mod model;
pub mod redact;
pub mod samples;
pub mod schema;
pub mod secret;
pub mod settings;
pub mod store;
pub mod term;
pub mod tracker;
pub mod typescript;

#[cfg(feature = "testing")]
pub mod testing;

pub use error::{ErrorCode, KeltaError, Result};

/// Crate version of the Kelta contract.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Current time as RFC 3339 (UTC).
pub fn now_rfc3339() -> String {
    time::OffsetDateTime::now_utc()
        .format(&time::format_description::well_known::Rfc3339)
        .unwrap_or_else(|_| String::from("1970-01-01T00:00:00Z"))
}

/// Everything most crates need.
pub mod prelude {
    pub use crate::api::*;
    pub use crate::codehost::*;
    pub use crate::ctl::*;
    pub use crate::dirs::*;
    pub use crate::error::{ErrorCode, KeltaError, Result};
    pub use crate::events::*;
    pub use crate::ext::*;
    pub use crate::hooks::HookPayload;
    pub use crate::ids::*;
    pub use crate::ipc::*;
    pub use crate::model::*;
    pub use crate::secret::*;
    pub use crate::settings::*;
    pub use crate::term::*;
    pub use crate::tracker::*;
}

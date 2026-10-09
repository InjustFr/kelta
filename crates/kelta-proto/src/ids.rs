//! Identifier newtypes (ARCHITECTURE §5). All serialize as plain strings.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

macro_rules! string_id {
    ($(#[$meta:meta])* $name:ident) => {
        $(#[$meta])*
        #[derive(
            Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Default, Serialize, Deserialize, TS,
            JsonSchema,
        )]
        #[serde(transparent)]
        pub struct $name(pub String);

        impl $name {
            pub fn new(s: impl Into<String>) -> Self {
                Self(s.into())
            }
            pub fn as_str(&self) -> &str {
                &self.0
            }
            pub fn into_inner(self) -> String {
                self.0
            }
        }

        impl std::fmt::Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str(&self.0)
            }
        }

        impl From<&str> for $name {
            fn from(s: &str) -> Self {
                Self(s.to_owned())
            }
        }

        impl From<String> for $name {
            fn from(s: String) -> Self {
                Self(s)
            }
        }

        impl AsRef<str> for $name {
            fn as_ref(&self) -> &str {
                &self.0
            }
        }

        impl std::borrow::Borrow<str> for $name {
            fn borrow(&self) -> &str {
                &self.0
            }
        }
    };
}

string_id!(
    /// Project slug `[a-z0-9-]{1,40}`; `"home"` reserved (built-in Home project), `"inbox"` reserved pseudo id.
    ProjectId
);
string_id!(
    /// Session id: uuid v7.
    SessionId
);
string_id!(
    /// Work item id: uuid v7.
    WorkItemId
);
string_id!(
    /// Account key from settings, e.g. `"jira-acme"`.
    AccountId
);
string_id!(TabId);
string_id!(PaneId);
string_id!(ToolInstanceId);
string_id!(ScreenInstanceId);
string_id!(
    /// Plugin id `[a-z0-9-]{3,40}`, not starting with `kelta`.
    PluginId
);
string_id!(
    /// Tool id: `"lazydocker"` or `"<plugin>/<tool>"`.
    ToolId
);

impl ProjectId {
    pub const HOME: &'static str = "home";
    pub const INBOX: &'static str = "inbox";

    pub fn home() -> Self {
        Self::new(Self::HOME)
    }

    /// `[a-z0-9-]{1,40}`.
    pub fn is_valid_slug(s: &str) -> bool {
        !s.is_empty()
            && s.len() <= 40
            && s.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
    }
}

impl SessionId {
    /// A fresh uuid v7.
    pub fn generate() -> Self {
        Self(uuid::Uuid::now_v7().to_string())
    }

    /// First 8 hex chars of the uuid (`<sid8>`, ARCHITECTURE §2.1).
    pub fn sid8(&self) -> String {
        self.0.chars().filter(|c| *c != '-').take(8).collect()
    }
}

impl WorkItemId {
    pub fn generate() -> Self {
        Self(uuid::Uuid::now_v7().to_string())
    }
}

macro_rules! uuid_id {
    ($name:ident) => {
        impl $name {
            pub fn generate() -> Self {
                Self(uuid::Uuid::new_v4().simple().to_string())
            }
        }
    };
}
uuid_id!(TabId);
uuid_id!(PaneId);
uuid_id!(ToolInstanceId);
uuid_id!(ScreenInstanceId);

impl PluginId {
    /// `[a-z0-9-]{3,40}`, not starting with `kelta`.
    pub fn is_valid(s: &str) -> bool {
        (3..=40).contains(&s.len())
            && !s.starts_with("kelta")
            && s.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
    }
}

impl ToolId {
    /// `Some(plugin)` for namespaced plugin tools `"<plugin>/<tool>"`.
    pub fn plugin(&self) -> Option<&str> {
        self.0.split_once('/').map(|(p, _)| p)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sid8_and_slugs() {
        let s = SessionId::new("0192f0c1-aaaa-7bbb-8ccc-dddddddddddd");
        assert_eq!(s.sid8(), "0192f0c1");
        assert!(ProjectId::is_valid_slug("shop-2"));
        assert!(!ProjectId::is_valid_slug("Shop"));
        assert!(PluginId::is_valid("tools-pack"));
        assert!(!PluginId::is_valid("kelta-x"));
        assert_eq!(ToolId::new("tools-pack/k9s").plugin(), Some("tools-pack"));
        assert_eq!(serde_json::to_string(&ProjectId::home()).unwrap(), "\"home\"");
    }
}

//! Permission engine (PLUGINS §5, ARCHITECTURE §11.3). Effective grants = permissions declared by
//! the current manifest ∩ permissions granted in the `GrantStore`, so a manifest update adding
//! permissions leaves the new capabilities disabled until re-granted.

use std::collections::BTreeSet;

use axum::http::Uri;
use kelta_proto::api::PluginGrant;
use kelta_proto::error::KeltaError;
use kelta_proto::ext::{MethodPermission, Permission, PluginMethod};
use serde_json::Value;

use crate::matcher;
use crate::util::{basename, is_loopback_host};

/// The effective permission set of one plugin.
#[derive(Debug, Clone, Default)]
pub struct Granted {
    set: BTreeSet<Permission>,
}

impl Granted {
    /// `declared ∩ granted`.
    pub fn new(declared: &[String], grants: &[PluginGrant]) -> Self {
        let granted: BTreeSet<&str> = grants.iter().map(|g| g.permission.as_str()).collect();
        let set = declared
            .iter()
            .filter(|p| granted.contains(p.as_str()))
            .filter_map(|p| Permission::parse(p))
            .collect();
        Self { set }
    }

    /// Everything (config-defined actions are not permission-gated).
    pub fn from_permissions(perms: impl IntoIterator<Item = Permission>) -> Self {
        Self { set: perms.into_iter().collect() }
    }

    pub fn strings(&self) -> Vec<String> {
        self.set.iter().map(ToString::to_string).collect()
    }

    pub fn has(&self, p: &Permission) -> bool {
        self.set.contains(p)
    }

    pub fn require(&self, p: Permission) -> Result<(), KeltaError> {
        if self.has(&p) { Ok(()) } else { Err(KeltaError::permission_denied(p.to_string())) }
    }

    /// `exec:<basename(argv0)>`.
    pub fn require_exec(&self, command: &str) -> Result<(), KeltaError> {
        self.require(Permission::Exec(basename(command).to_owned()))
    }

    /// `events:<glob>` covering `requested` (a name or a glob).
    pub fn covers_events(&self, requested: &str) -> bool {
        self.set.iter().any(|p| match p {
            Permission::Events(g) => g == requested || matcher::glob(g).is_ok_and(|m| m.is_match(requested)),
            _ => false,
        })
    }

    pub fn require_events(&self, requested: &str) -> Result<(), KeltaError> {
        if self.covers_events(requested) {
            Ok(())
        } else {
            Err(KeltaError::permission_denied(format!("events:{requested}")))
        }
    }

    /// `net:<host>` for an outbound URL: https only; http only for loopback hosts granted exactly.
    pub fn require_net(&self, url: &str) -> Result<(), KeltaError> {
        let uri: Uri = url.parse().map_err(|_| KeltaError::invalid(format!("invalid URL `{url}`")))?;
        let host = uri
            .host()
            .ok_or_else(|| KeltaError::invalid(format!("URL without host `{url}`")))?
            .trim_start_matches('[')
            .trim_end_matches(']')
            .to_ascii_lowercase();
        let wanted = Permission::Net(host.clone());
        match uri.scheme_str() {
            Some("https") => {
                let ok = self.set.iter().any(|p| match p {
                    Permission::Net(h) => {
                        let h = h.to_ascii_lowercase();
                        match h.strip_prefix("*.") {
                            Some(domain) => host.ends_with(&format!(".{domain}")),
                            None => h == host,
                        }
                    }
                    _ => false,
                });
                if ok { Ok(()) } else { Err(KeltaError::permission_denied(wanted.to_string())) }
            }
            Some("http") => {
                if !is_loopback_host(&host) {
                    return Err(KeltaError::permission_denied(wanted.to_string()).with_detail(
                        serde_json::json!({ "permission": wanted.to_string(), "reason": "https only" }),
                    ));
                }
                self.require(wanted)
            }
            _ => Err(KeltaError::invalid(format!("unsupported URL scheme in `{url}`"))),
        }
    }
}

fn param_str<'a>(params: &'a Value, key: &str) -> Option<&'a str> {
    params.get(key).and_then(Value::as_str).filter(|s| !s.is_empty())
}

/// The permission gate for `plugin_call`: every method without its permission is `PermissionDenied`.
pub fn check_method(method: PluginMethod, params: &Value, granted: &Granted) -> Result<(), KeltaError> {
    match method.required_permission() {
        MethodPermission::None => Ok(()),
        MethodPermission::Static(p) => granted.require(p),
        MethodPermission::Dynamic => match method {
            PluginMethod::SessionsSpawn => {
                granted.require(Permission::SessionsSpawn)?;
                if let Some(cmd) = param_str(params, "command") {
                    granted.require_exec(cmd)?;
                }
                Ok(())
            }
            PluginMethod::EventsSubscribe => {
                let names: Vec<&str> = params
                    .get("names")
                    .and_then(Value::as_array)
                    .map(|a| a.iter().filter_map(Value::as_str).collect())
                    .unwrap_or_default();
                if names.is_empty() {
                    return Err(KeltaError::invalid(
                        "events.subscribe: `names` must list event names or globs",
                    ));
                }
                names.iter().try_for_each(|n| granted.require_events(n))
            }
            PluginMethod::NetFetch => {
                let url = param_str(params, "url")
                    .ok_or_else(|| KeltaError::invalid("net.fetch: missing `url`"))?;
                granted.require_net(url)
            }
            // Own namespace always; `settings.read` only widens the result.
            PluginMethod::SettingsGet => Ok(()),
            other => Err(KeltaError::internal(format!("no dynamic permission rule for {other:?}"))),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn granted(perms: &[&str]) -> Granted {
        Granted::from_permissions(perms.iter().map(|p| Permission::parse(p).unwrap()))
    }

    #[test]
    fn declared_intersection_grants() {
        let declared = vec!["tickets.read".to_owned(), "notify".to_owned()];
        let grants = vec![
            PluginGrant {
                permission: "tickets.read".into(),
                granted_at: String::new(),
                manifest_sha256: String::new(),
            },
            PluginGrant {
                permission: "ui.open".into(),
                granted_at: String::new(),
                manifest_sha256: String::new(),
            },
        ];
        let g = Granted::new(&declared, &grants);
        assert_eq!(g.strings(), vec!["tickets.read"]);
    }

    #[test]
    fn net_rules() {
        let g = granted(&["net:api.example.com", "net:*.acme.com", "net:127.0.0.1"]);
        assert!(g.require_net("https://api.example.com/x").is_ok());
        assert!(g.require_net("https://other.example.com/x").is_err());
        assert!(g.require_net("https://ci.acme.com/").is_ok());
        assert!(g.require_net("https://acme.com/").is_err());
        assert!(g.require_net("http://api.example.com/").is_err());
        assert!(g.require_net("http://127.0.0.1:8080/").is_ok());
        assert!(g.require_net("http://localhost:8080/").is_err());
        assert!(g.require_net("ftp://api.example.com/").is_err());
        let e = g.require_net("https://evil.test/").unwrap_err();
        assert_eq!(e.code, kelta_proto::ErrorCode::PermissionDenied);
        assert_eq!(e.detail.unwrap()["permission"], "net:evil.test");
    }

    #[test]
    fn events_and_exec() {
        let g = granted(&["events:ticket.*", "exec:kubectl"]);
        assert!(g.covers_events("ticket.transitioned"));
        assert!(g.covers_events("ticket.*"));
        assert!(!g.covers_events("pr.merged"));
        assert!(!g.covers_events("*"));
        assert!(g.require_exec("/usr/local/bin/kubectl").is_ok());
        assert!(g.require_exec("rm").is_err());
    }
}

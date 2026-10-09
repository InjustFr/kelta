//! Index over the generated JSON Schemas: `x-kelta-*` annotations per dotted path, cached
//! validators and the `by_id` merge paths.

use std::collections::HashSet;
use std::sync::OnceLock;

use jsonschema::Validator;
use serde_json::Value;

use crate::path::join_path;

/// Annotations that apply to one settings path (inherited from the nearest ancestor where
/// the schema says so).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NodeInfo {
    /// `x-kelta-scope` (nearest annotation; default `["global","project"]`).
    pub scope: Vec<String>,
    /// `x-kelta-exec` on the node or any ancestor.
    pub exec: bool,
    /// `x-kelta-restart` on the node or any ancestor.
    pub restart: bool,
    /// `x-kelta-secret` on the node itself.
    pub secret: bool,
    /// `x-kelta-merge = "by_id"` on the node itself.
    pub by_id: bool,
    /// The path exists in the schema.
    pub known: bool,
}

impl NodeInfo {
    pub fn allows(&self, scope: &str) -> bool {
        self.scope.iter().any(|s| s == scope)
    }
}

pub struct SchemaIndex {
    settings: Value,
    project: Value,
    settings_validator: Option<Validator>,
    project_validator: Option<Validator>,
    by_id: HashSet<String>,
}

static INDEX: OnceLock<SchemaIndex> = OnceLock::new();

/// Process-wide schema index (the schemas are compiled in).
pub fn index() -> &'static SchemaIndex {
    INDEX.get_or_init(SchemaIndex::build)
}

fn child<'a>(node: &'a Value, seg: &str) -> Option<&'a Value> {
    if let Some(p) = node.get("properties").and_then(|p| p.get(seg)) {
        return Some(p);
    }
    if let Some(a) = node.get("additionalProperties")
        && a.is_object()
    {
        return Some(a);
    }
    for key in ["anyOf", "oneOf"] {
        if let Some(arr) = node.get(key).and_then(Value::as_array) {
            for alt in arr {
                if let Some(c) = child(alt, seg) {
                    return Some(c);
                }
            }
        }
    }
    None
}

fn collect_by_id(node: &Value, path: &mut Vec<String>, out: &mut HashSet<String>) {
    if node.get("x-kelta-merge").and_then(Value::as_str) == Some("by_id") {
        out.insert(join_path(path));
    }
    if let Some(props) = node.get("properties").and_then(Value::as_object) {
        for (k, v) in props {
            path.push(k.clone());
            collect_by_id(v, path, out);
            path.pop();
        }
    }
}

impl SchemaIndex {
    fn build() -> Self {
        let settings = kelta_proto::schema::settings_schema();
        let project = kelta_proto::schema::project_schema();
        let settings_validator = jsonschema::validator_for(&settings).ok();
        let project_validator = jsonschema::validator_for(&project).ok();
        let mut by_id = HashSet::new();
        collect_by_id(&settings, &mut Vec::new(), &mut by_id);
        Self { settings, project, settings_validator, project_validator, by_id }
    }

    pub fn settings_schema(&self) -> &Value {
        &self.settings
    }

    pub fn project_schema(&self) -> &Value {
        &self.project
    }

    pub fn settings_validator(&self) -> Option<&Validator> {
        self.settings_validator.as_ref()
    }

    pub fn project_validator(&self) -> Option<&Validator> {
        self.project_validator.as_ref()
    }

    /// Dotted paths of keyed lists that merge by `id`.
    pub fn by_id_paths(&self) -> &HashSet<String> {
        &self.by_id
    }

    /// Top-level setting keys (sections).
    pub fn top_level_keys(&self) -> Vec<String> {
        self.settings
            .get("properties")
            .and_then(Value::as_object)
            .map(|m| m.keys().cloned().collect())
            .unwrap_or_default()
    }

    /// Schema node at a path (maps resolve to their value schema).
    pub fn node(&self, path: &[String]) -> Option<&Value> {
        let mut cur = &self.settings;
        for seg in path {
            cur = child(cur, seg)?;
        }
        Some(cur)
    }

    pub fn info(&self, path: &[String]) -> NodeInfo {
        let mut scope: Vec<String> = vec!["global".into(), "project".into()];
        let mut exec = false;
        let mut restart = false;
        let mut secret = false;
        let mut by_id = false;
        let mut known = true;
        let mut cur = &self.settings;
        for seg in path {
            match child(cur, seg) {
                Some(n) => {
                    cur = n;
                    if let Some(s) = n.get("x-kelta-scope").and_then(Value::as_array) {
                        scope = s.iter().filter_map(|x| x.as_str().map(str::to_owned)).collect();
                    }
                    exec |= n.get("x-kelta-exec").and_then(Value::as_bool).unwrap_or(false);
                    restart |= n.get("x-kelta-restart").and_then(Value::as_bool).unwrap_or(false);
                    secret = n.get("x-kelta-secret").and_then(Value::as_bool).unwrap_or(false);
                    by_id = n.get("x-kelta-merge").and_then(Value::as_str) == Some("by_id");
                }
                None => {
                    known = false;
                    secret = false;
                    by_id = false;
                    break;
                }
            }
        }
        NodeInfo { scope, exec, restart, secret, by_id, known }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(s: &str) -> Vec<String> {
        crate::path::split_path(s).unwrap()
    }

    #[test]
    fn annotations_resolve() {
        let i = index();
        assert!(i.settings_validator().is_some());
        assert!(i.project_validator().is_some());
        assert!(!i.info(&p("linux.graphics.profile")).allows("project"));
        assert!(i.info(&p("linux.graphics.profile")).restart);
        assert!(i.info(&p("window.decorations")).restart);
        assert!(i.info(&p("terminal.font_size")).allows("project"));
        assert!(i.info(&p("worktree.setup")).exec);
        assert!(i.info(&p("worktree.setup")).allows("repo"));
        assert!(i.info(&p("accounts.jira.secret")).secret);
        assert!(!i.info(&p("accounts.jira.secret")).allows("project"));
        assert!(i.info(&p("tools")).by_id);
        assert!(!i.info(&p("nope.zip")).known);
        assert!(i.by_id_paths().contains("editor.presets"));
        assert!(i.by_id_paths().contains("session_templates"));
    }
}

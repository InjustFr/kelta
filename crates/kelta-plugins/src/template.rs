//! Placeholder templates (PLUGINS §1, SETTINGS §6): `{path}`, filters `|slug`, `|shell`, `|json`,
//! fallbacks `{a|b}` (first non-empty). Text in braces that does not look like a placeholder (e.g.
//! `{"veto": "…"}`, `{{filename}}`) is copied verbatim. Values are substituted per
//! argv element: no shell parsing ever happens here.

use kelta_proto::error::KeltaError;
use serde_json::{Map, Value};

use crate::util::{json_path, json_to_string};

/// Roots with fixed sub-keys.
const STRUCTURED: &[(&str, &[&str])] = &[
    ("project", &["id", "name", "root"]),
    ("repo", &["id", "path", "name"]),
    ("ticket", &["key", "title", "url", "file", "provider"]),
    ("pr", &["url", "number", "head", "base", "title", "repo"]),
    ("session", &["id", "name", "cwd", "kind", "status", "visible"]),
    ("plugin", &["dir", "id"]),
    ("app", &["focused"]),
    ("work", &["port"]),
];

/// Scalar roots.
const SCALARS: &[&str] = &[
    "worktree",
    "branch",
    "base",
    "key",
    "slug",
    "type",
    "sid8",
    "run",
    "port",
    "config_dir",
    "data_dir",
    "home",
    "user",
];

/// Roots that accept any sub-path.
const OPEN: &[&str] = &["settings", "event", "payload"];

const FILTERS: &[&str] = &["slug", "shell", "json"];

/// True when `path` names a known placeholder.
pub fn is_known_path(path: &str) -> bool {
    let (root, rest) = match path.split_once('.') {
        Some((r, rest)) => (r, Some(rest)),
        None => (path, None),
    };
    if SCALARS.contains(&root) {
        return rest.is_none();
    }
    if OPEN.contains(&root) {
        return rest.is_none_or(|r| !r.is_empty());
    }
    if let Some((_, keys)) = STRUCTURED.iter().find(|(r, _)| *r == root) {
        return rest.is_some_and(|r| keys.contains(&r));
    }
    false
}

fn is_placeholder_body(body: &str) -> bool {
    !body.is_empty()
        && body.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'.' | b'|' | b'-'))
        && body.bytes().next().is_some_and(|b| b.is_ascii_alphabetic())
}

#[derive(Debug, PartialEq)]
enum Piece<'a> {
    Lit(String),
    Ph(&'a str),
}

fn parse(s: &str) -> Vec<Piece<'_>> {
    let mut out = Vec::new();
    let mut lit = String::new();
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        let c = bytes[i];
        // `{{…}}` (Go/handlebars templates of other tools, e.g. lazygit) is copied verbatim.
        if c == b'{'
            && bytes.get(i + 1) == Some(&b'{')
            && let Some(end) = s[i..].find("}}")
        {
            lit.push_str(&s[i..i + end + 2]);
            i += end + 2;
            continue;
        }
        if c == b'{'
            && let Some(end) = s[i + 1..].find('}')
        {
            let body = &s[i + 1..i + 1 + end];
            if is_placeholder_body(body) {
                if !lit.is_empty() {
                    out.push(Piece::Lit(std::mem::take(&mut lit)));
                }
                out.push(Piece::Ph(body));
                i += end + 2;
                continue;
            }
        }
        // Copy one UTF-8 char.
        let ch_len = s[i..].chars().next().map(char::len_utf8).unwrap_or(1);
        lit.push_str(&s[i..i + ch_len]);
        i += ch_len;
    }
    if !lit.is_empty() {
        out.push(Piece::Lit(lit));
    }
    out
}

/// Placeholder bodies used in `s`.
pub fn placeholders(s: &str) -> Vec<&str> {
    parse(s)
        .into_iter()
        .filter_map(|p| match p {
            Piece::Ph(b) => Some(b),
            Piece::Lit(_) => None,
        })
        .collect()
}

/// Load-time validation: every placeholder path must be known. Returns the offending names.
pub fn unknown_placeholders(s: &str) -> Vec<String> {
    let mut bad = Vec::new();
    for body in placeholders(s) {
        for part in body.split('|') {
            if FILTERS.contains(&part) {
                continue;
            }
            if !is_known_path(part) {
                bad.push(part.to_owned());
            }
        }
    }
    bad
}

/// Slug: lowercase ASCII alphanumerics, everything else collapses to `-`, trimmed, ≤ 40 chars.
pub fn slugify(s: &str) -> String {
    let mut out = String::new();
    let mut dash = false;
    for c in s.chars() {
        let c = c.to_ascii_lowercase();
        if c.is_ascii_alphanumeric() {
            out.push(c);
            dash = false;
        } else if !dash && !out.is_empty() {
            out.push('-');
            dash = true;
        }
    }
    while out.ends_with('-') {
        out.pop();
    }
    if out.len() > 40 {
        out.truncate(40);
        while out.ends_with('-') {
            out.pop();
        }
    }
    out
}

/// POSIX single-quoting.
pub fn shell_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', r"'\''"))
}

/// Variables available to a template: a JSON object keyed by root.
#[derive(Debug, Clone, Default)]
pub struct Vars {
    root: Map<String, Value>,
}

impl Vars {
    pub fn new() -> Self {
        Self::default()
    }

    /// Set a scalar or object root (`"worktree"`, `"project"` → `{id, name, root}`).
    pub fn set(&mut self, key: &str, value: Value) -> &mut Self {
        self.root.insert(key.to_owned(), value);
        self
    }

    /// Set `root.sub` (creating `root` as an object).
    pub fn set_sub(&mut self, root: &str, sub: &str, value: Value) -> &mut Self {
        let entry = self.root.entry(root.to_owned()).or_insert_with(|| Value::Object(Map::new()));
        if !entry.is_object() {
            *entry = Value::Object(Map::new());
        }
        if let Value::Object(m) = entry {
            m.insert(sub.to_owned(), value);
        }
        self
    }

    pub fn get(&self, path: &str) -> Option<&Value> {
        let (root, rest) = match path.split_once('.') {
            Some((r, rest)) => (r, Some(rest)),
            None => (path, None),
        };
        let v = self.root.get(root)?;
        match rest {
            None => Some(v),
            Some(r) => json_path(v, r),
        }
    }

    pub fn as_value(&self) -> Value {
        Value::Object(self.root.clone())
    }

    fn resolve(&self, body: &str) -> Result<String, KeltaError> {
        let mut value: Option<Value> = None;
        let as_str = |v: &Option<Value>| v.as_ref().map(json_to_string).unwrap_or_default();
        for part in body.split('|') {
            match part {
                "slug" => value = Some(Value::String(slugify(&as_str(&value)))),
                "shell" => value = Some(Value::String(shell_quote(&as_str(&value)))),
                "json" => {
                    let v = value.take().unwrap_or(Value::String(String::new()));
                    value = Some(Value::String(v.to_string()));
                }
                path => {
                    if value.as_ref().is_some_and(|v| !json_to_string(v).is_empty()) {
                        continue;
                    }
                    if !is_known_path(path) {
                        return Err(KeltaError::invalid(format!("unknown placeholder `{{{path}}}`")));
                    }
                    value = Some(self.get(path).cloned().unwrap_or(Value::Null));
                }
            }
        }
        Ok(as_str(&value))
    }

    /// Expand every placeholder in `s`.
    pub fn expand(&self, s: &str) -> Result<String, KeltaError> {
        let mut out = String::with_capacity(s.len());
        for piece in parse(s) {
            match piece {
                Piece::Lit(l) => out.push_str(&l),
                Piece::Ph(body) => out.push_str(&self.resolve(body)?),
            }
        }
        Ok(out)
    }

    pub fn expand_all(&self, items: &[String]) -> Result<Vec<String>, KeltaError> {
        items.iter().map(|s| self.expand(s)).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn vars() -> Vars {
        let mut v = Vars::new();
        v.set("project", json!({"id": "shop", "name": "Shop", "root": "/src/shop"}));
        v.set("repo", json!({"id": "main", "path": "/src/shop", "name": "shop"}));
        v.set("worktree", json!(""));
        v.set("port", json!(4312));
        v.set("payload", json!({"path": "src/main.rs", "n": 3}));
        v.set_sub("ticket", "title", json!("Fix the Ünïcode bug — now!"));
        v
    }

    #[test]
    fn expands_paths_fallbacks_and_filters() {
        let v = vars();
        assert_eq!(v.expand("{worktree|repo.path}").unwrap(), "/src/shop");
        assert_eq!(v.expand("--port={port}").unwrap(), "--port=4312");
        assert_eq!(v.expand("{ticket.title|slug}").unwrap(), "fix-the-n-code-bug-now");
        assert_eq!(v.expand("{project.name|shell}").unwrap(), "'Shop'");
        assert_eq!(v.expand("{payload.path|json}").unwrap(), "\"src/main.rs\"");
        assert_eq!(v.expand("{payload.n}").unwrap(), "3");
        assert_eq!(v.expand("{{filename}}").unwrap(), "{{filename}}");
        assert_eq!(v.expand(r#"{"a":{"b":1}}"#).unwrap(), r#"{"a":{"b":1}}"#);
        assert_eq!(v.expand("{branch}").unwrap(), "");
        assert_eq!(v.expand("{payload|json}").unwrap(), r#"{"n":3,"path":"src/main.rs"}"#);
        assert_eq!(v.expand("{payload.n|json}").unwrap(), "3");
    }

    #[test]
    fn json_braces_are_literal() {
        let v = vars();
        let s = r#"git diff --quiet || echo '{"veto":"main checkout is dirty"}'"#;
        assert_eq!(v.expand(s).unwrap(), s);
        assert!(unknown_placeholders(s).is_empty());
    }

    #[test]
    fn unknown_placeholders_are_errors() {
        assert_eq!(unknown_placeholders("{nope} {repo.path} {project.bogus}"), vec!["nope", "project.bogus"]);
        assert!(vars().expand("{nope}").is_err());
        assert!(unknown_placeholders("{settings.view_id} {event.name} {repo.path|shell}").is_empty());
    }

    #[test]
    fn shell_quoting() {
        assert_eq!(shell_quote("it's"), r"'it'\''s'");
    }
}

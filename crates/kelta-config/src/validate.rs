//! Per-layer validation: JSON Schema, layer scope, repo-local allowed keys, duplicate ids,
//! template placeholders, project file rules and cross references of the merged result.
//!
//! Everything works on `serde_json::Value` and produces raw [`RawIssue`]s (path segments +
//! message); [`position`] turns them into line/col [`ValidationIssue`]s against the TOML text.

use std::collections::HashSet;

use kelta_proto::settings::{Layer, REPO_ALLOWED_KEYS, ValidationIssue};
use serde_json::Value;

use crate::path::{get, join_path, leaves, pointer_segments};
use crate::schema_info::index;
use crate::toml_io::{Parsed, issue_at};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawIssue {
    pub path: Vec<String>,
    pub message: String,
}

impl RawIssue {
    pub fn new(path: &[String], message: impl Into<String>) -> Self {
        Self { path: path.to_vec(), message: message.into() }
    }
}

/// Convert raw issues to positioned issues (`doc` gives the line/col).
pub fn position(text: &str, parsed: Option<&Parsed>, issues: Vec<RawIssue>) -> Vec<ValidationIssue> {
    issues.into_iter().map(|i| issue_at(text, parsed.map(|p| &p.doc), &i.path, i.message)).collect()
}

fn unexpected_name(message: &str) -> Option<String> {
    if !(message.contains("was unexpected") || message.contains("were unexpected")) {
        return None;
    }
    let start = message.find('\'')? + 1;
    let rest = &message[start..];
    let end = rest.find('\'')?;
    Some(rest[..end].to_owned())
}

fn schema_errors(
    validator: Option<&jsonschema::Validator>,
    value: &Value,
    prefix: &[String],
) -> Vec<RawIssue> {
    let Some(v) = validator else { return Vec::new() };
    let mut out = Vec::new();
    for err in v.iter_errors(value) {
        let mut path: Vec<String> = prefix.to_vec();
        path.extend(pointer_segments(&err.instance_path().to_string()));
        let message = err.to_string();
        if let Some(name) = unexpected_name(&message) {
            path.push(name.clone());
            out.push(RawIssue { path, message: format!("unknown key `{name}`") });
        } else {
            out.push(RawIssue { path, message });
        }
    }
    out
}

/// JSON Schema validation of one layer document.
pub fn schema_issues(layer: Layer, value: &Value) -> Vec<RawIssue> {
    let idx = index();
    let mut out = match layer {
        Layer::Project => {
            let mut v = schema_errors(idx.project_validator(), value, &[]);
            if let Some(obj) = value.as_object() {
                let known = idx.top_level_keys();
                for k in obj.keys() {
                    if k != "project" && !known.iter().any(|x| x == k) {
                        v.push(RawIssue::new(std::slice::from_ref(k), format!("unknown key `{k}`")));
                    }
                }
            }
            v
        }
        _ => schema_errors(idx.settings_validator(), value, &[]),
    };
    out.dedup();
    out
}

/// Project files may only hold keys whose schema scope includes `project`.
pub fn scope_issues(layer: Layer, value: &Value) -> Vec<RawIssue> {
    if layer != Layer::Project {
        return Vec::new();
    }
    let idx = index();
    let mut out = Vec::new();
    for p in leaves(value) {
        if p.first().map(String::as_str) == Some("project") {
            continue;
        }
        let info = idx.info(&p);
        if info.known && !info.allows("project") {
            out.push(RawIssue::new(
                &p,
                format!("`{}` cannot be set per project (global only)", join_path(&p)),
            ));
        }
    }
    out
}

/// `x-kelta-secret` fields hold a `SecretRef`, never a raw token (the message must not echo the value).
pub fn secret_issues(value: &Value) -> Vec<RawIssue> {
    let idx = index();
    leaves(value)
        .into_iter()
        .filter(|p| idx.info(p).secret)
        .filter(|p| {
            let bad = |s: &Value| {
                matches!(s, Value::String(s) if !s.is_empty()
                    && kelta_proto::secret::SecretRef::new(s.as_str()).parse().is_none())
            };
            match get(value, p) {
                Some(Value::Array(a)) => a.iter().any(bad),
                Some(v) => bad(v),
                None => false,
            }
        })
        .map(|p| {
            RawIssue::new(
                &p,
                "expected keyring:<name>, gh-cli, glab-cli, command:<argv> or env:<VAR> (secret values never go in config files)",
            )
        })
        .collect()
}

/// Repo-local files accept only [`REPO_ALLOWED_KEYS`] (SETTINGS §4).
pub fn repo_key_issues(value: &Value) -> Vec<RawIssue> {
    let mut out = Vec::new();
    for p in leaves(value) {
        // an empty table such as `[worktree]` carries no key
        if matches!(get(value, &p), Some(Value::Object(m)) if m.is_empty()) {
            continue;
        }
        let dotted = join_path(&p);
        let allowed = REPO_ALLOWED_KEYS.iter().any(|a| dotted == *a || dotted.starts_with(&format!("{a}.")));
        if !allowed {
            out.push(RawIssue::new(&p, format!("`{dotted}` is not allowed in repo config")));
        }
    }
    out
}

/// Duplicate or missing ids in keyed lists, per layer.
pub fn id_issues(value: &Value) -> Vec<RawIssue> {
    let mut out = Vec::new();
    let mut paths: Vec<&String> = index().by_id_paths().iter().collect();
    paths.sort();
    for dotted in paths {
        let segs = crate::path::split_path(dotted).unwrap_or_default();
        let Some(Value::Array(items)) = get(value, &segs) else { continue };
        let mut seen: HashSet<&str> = HashSet::new();
        for (i, item) in items.iter().enumerate() {
            let mut p = segs.clone();
            p.push(i.to_string());
            match item.get("id").and_then(Value::as_str) {
                Some(id) if !id.is_empty() => {
                    if !seen.insert(id) {
                        p.push("id".into());
                        out.push(RawIssue::new(&p, format!("duplicate id `{id}` in `{dotted}`")));
                    }
                }
                _ => {
                    p.push("id".into());
                    out.push(RawIssue::new(&p, format!("entry of `{dotted}` needs a non-empty `id`")));
                }
            }
        }
    }
    out
}

// ---------------------------------------------------------------------------------------------
// Templates
// ---------------------------------------------------------------------------------------------

const PLAIN_ROOTS: &[&str] = &[
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
    "project",
    "repo",
    "closes",
    "ticket",
    "pr",
    "session",
];
const FILTERS: &[&str] = &["slug", "shell", "json"];

fn sub_allowed(root: &str) -> Option<&'static [&'static str]> {
    match root {
        "project" => Some(&["id", "name", "root"]),
        "repo" => Some(&["id", "path", "name"]),
        "ticket" => Some(&["key", "title", "url", "file"]),
        "pr" => Some(&["url", "number", "head", "base", "title"]),
        "session" => Some(&["id", "name", "cwd"]),
        _ => None,
    }
}

fn placeholder_known(name: &str, extra: &[&str]) -> bool {
    if extra.contains(&name) {
        return true;
    }
    let mut parts = name.splitn(2, '.');
    let root = parts.next().unwrap_or_default();
    let sub = parts.next();
    // open namespaces: plugin settings, trigger events, plugin dir
    if matches!(root, "settings" | "event" | "payload") && sub.is_some() {
        return true;
    }
    if root == "plugin" {
        return sub == Some("dir");
    }
    if !PLAIN_ROOTS.contains(&root) {
        return false;
    }
    match (sub, sub_allowed(root)) {
        (None, _) => !matches!(root, "ticket" | "pr" | "session"),
        (Some(s), Some(allowed)) => allowed.contains(&s),
        (Some(_), None) => false,
    }
}

/// First unknown placeholder of a template string, as an error message.
pub fn template_error(s: &str, extra: &[&str]) -> Option<String> {
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'{'
            && let Some(rel) = s[i + 1..].find('}')
        {
            let inner = &s[i + 1..i + 1 + rel];
            let looks_like_placeholder = !inner.is_empty()
                && inner.chars().next().is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
                && inner.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '|' | '-'));
            if looks_like_placeholder {
                let mut parts = inner.split('|');
                let first = parts.next().unwrap_or_default();
                if !placeholder_known(first, extra) {
                    return Some(format!("unknown placeholder `{{{first}}}`"));
                }
                for alt in parts {
                    if FILTERS.contains(&alt) {
                        continue;
                    }
                    if !placeholder_known(alt, extra) {
                        return Some(format!("unknown placeholder `{{{alt}}}` in `{{{inner}}}`"));
                    }
                }
            }
            i += rel + 2;
            continue;
        }
        i += 1;
    }
    None
}

const EDITOR_EXTRA: &[&str] = &["sock", "path", "file", "line", "cwd", "sid8"];

fn check_str(out: &mut Vec<RawIssue>, path: &[String], v: Option<&Value>, extra: &[&str]) {
    if let Some(Value::String(s)) = v
        && let Some(msg) = template_error(s, extra)
    {
        out.push(RawIssue::new(path, msg));
    }
}

fn check_str_list(out: &mut Vec<RawIssue>, base: &[String], key: &str, v: &Value, extra: &[&str]) {
    if let Some(Value::Array(a)) = v.get(key) {
        for (i, item) in a.iter().enumerate() {
            let mut p = base.to_vec();
            p.push(key.to_owned());
            p.push(i.to_string());
            check_str(out, &p, Some(item), extra);
        }
    }
}

fn check_layout(out: &mut Vec<RawIssue>, path: &[String], node: &Value) {
    if let Some(cmd) = node.get("command") {
        let mut p = path.to_vec();
        p.push("command".into());
        check_str(out, &p, Some(cmd), &[]);
    }
    if let Some(Value::Array(children)) = node.get("children") {
        for (i, c) in children.iter().enumerate() {
            let mut p = path.to_vec();
            p.push("children".into());
            p.push(i.to_string());
            check_layout(out, &p, c);
        }
    }
}

/// Unknown `{placeholders}` in the template-typed settings of one document.
pub fn template_issues(value: &Value) -> Vec<RawIssue> {
    let mut out = Vec::new();
    let s = |p: &[&str]| -> Vec<String> { p.iter().map(|x| (*x).to_owned()).collect() };
    for p in [
        &["worktree", "root"][..],
        &["worktree", "branch_template"],
        &["work", "on_start", "comment"],
        &["work", "on_pr", "comment"],
        &["work", "pr", "title_template"],
        &["work", "pr", "body_template"],
    ] {
        let path = s(p);
        check_str(&mut out, &path, get(value, &path), &[]);
    }
    if let Some(Value::Object(m)) = get(value, &s(&["claude", "prompt_templates"])) {
        for (k, v) in m {
            check_str(&mut out, &s(&["claude", "prompt_templates", k]), Some(v), &[]);
        }
    }
    if let Some(Value::Array(presets)) = get(value, &s(&["editor", "presets"])) {
        for (i, p) in presets.iter().enumerate() {
            let base = vec!["editor".to_owned(), "presets".to_owned(), i.to_string()];
            check_str_list(&mut out, &base, "args", p, EDITOR_EXTRA);
            check_str_list(&mut out, &base, "open_cmd", p, EDITOR_EXTRA);
            let mut k = base.clone();
            k.push("open_keys".into());
            check_str(&mut out, &k, p.get("open_keys"), EDITOR_EXTRA);
        }
    }
    if let Some(Value::Array(ts)) = value.get("session_templates") {
        for (i, t) in ts.iter().enumerate() {
            if let Some(layout) = t.get("layout") {
                check_layout(
                    &mut out,
                    &["session_templates".to_owned(), i.to_string(), "layout".to_owned()],
                    layout,
                );
            }
        }
    }
    if let Some(Value::Array(tools)) = value.get("tools") {
        for (i, t) in tools.iter().enumerate() {
            let base = vec!["tools".to_owned(), i.to_string()];
            for key in ["command", "cwd"] {
                let mut p = base.clone();
                p.push(key.into());
                check_str(&mut out, &p, t.get(key), &[]);
            }
            check_str_list(&mut out, &base, "args", t, &[]);
            if let Some(Value::Object(env)) = t.get("env") {
                for (k, v) in env {
                    let mut p = base.clone();
                    p.push("env".into());
                    p.push(k.clone());
                    check_str(&mut out, &p, Some(v), &[]);
                }
            }
        }
    }
    out
}

// ---------------------------------------------------------------------------------------------
// Project file
// ---------------------------------------------------------------------------------------------

/// `[a-z0-9-]{1,40}`.
pub fn valid_slug(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 40
        && id.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

fn dup_ids(out: &mut Vec<RawIssue>, base: &[String], list: Option<&Value>, what: &str) {
    let Some(Value::Array(items)) = list else { return };
    let mut seen = HashSet::new();
    for (i, item) in items.iter().enumerate() {
        if let Some(id) = item.get("id").and_then(Value::as_str)
            && !seen.insert(id)
        {
            let mut p = base.to_vec();
            p.push(i.to_string());
            p.push("id".into());
            out.push(RawIssue::new(&p, format!("duplicate {what} id `{id}`")));
        }
    }
}

/// Rules of the `[project]` table; `stem` is the file stem when known.
pub fn project_issues(value: &Value, stem: Option<&str>) -> Vec<RawIssue> {
    let mut out = Vec::new();
    let Some(project) = value.get("project") else {
        out.push(RawIssue::new(&[], "missing [project] table"));
        return out;
    };
    let pid = vec!["project".to_owned(), "id".to_owned()];
    match project.get("id").and_then(Value::as_str) {
        Some(id) if !valid_slug(id) => out.push(RawIssue::new(&pid, "project id must match [a-z0-9-]{1,40}")),
        Some("home" | "inbox") => out.push(RawIssue::new(&pid, "project id is reserved")),
        Some(id) => {
            if let Some(stem) = stem
                && stem != id
            {
                out.push(RawIssue::new(&pid, format!("project id `{id}` must equal the file name `{stem}`")));
            }
        }
        None => out.push(RawIssue::new(&pid, "project id is required")),
    }
    let proj = vec!["project".to_owned()];
    let mut repos_path = proj.clone();
    repos_path.push("repos".into());
    dup_ids(&mut out, &repos_path, project.get("repos"), "repo");
    if let Some(Value::Array(repos)) = project.get("repos") {
        for (i, r) in repos.iter().enumerate() {
            let mut p = repos_path.clone();
            p.push(i.to_string());
            if r.get("id").and_then(Value::as_str).is_none_or(str::is_empty) {
                let mut q = p.clone();
                q.push("id".into());
                out.push(RawIssue::new(&q, "repo needs an id"));
            }
            if r.get("path").and_then(Value::as_str).is_none_or(str::is_empty) {
                p.push("path".into());
                out.push(RawIssue::new(&p, "repo needs a path"));
            }
        }
    }
    if let Some(t) = project.get("tracker") {
        let mut base = proj.clone();
        base.push("tracker".into());
        let mut v = base.clone();
        v.push("views".into());
        dup_ids(&mut out, &v, t.get("views"), "view");
        let mut c = base;
        c.push("columns".into());
        dup_ids(&mut out, &c, t.get("columns"), "column");
    }
    out
}

// ---------------------------------------------------------------------------------------------
// Cross references of the merged document
// ---------------------------------------------------------------------------------------------

fn ids_of(v: Option<&Value>) -> HashSet<String> {
    v.and_then(Value::as_array)
        .map(|a| a.iter().filter_map(|e| e.get("id").and_then(Value::as_str).map(str::to_owned)).collect())
        .unwrap_or_default()
}

fn collect_profiles(node: &Value, path: &mut Vec<String>, out: &mut Vec<(Vec<String>, String)>) {
    if let Some(p) = node.get("profile").and_then(Value::as_str) {
        let mut q = path.clone();
        q.push("profile".into());
        out.push((q, p.to_owned()));
    }
    if let Some(Value::Array(children)) = node.get("children") {
        for (i, c) in children.iter().enumerate() {
            path.push("children".into());
            path.push(i.to_string());
            collect_profiles(c, path, out);
            path.pop();
            path.pop();
        }
    }
}

/// Unknown preset / template / profile ids in the merged document.
pub fn cross_issues(merged: &Value, project_default_template: Option<&str>) -> Vec<RawIssue> {
    let mut out = Vec::new();
    let presets = ids_of(get(merged, &["editor".to_owned(), "presets".to_owned()]));
    if let Some(d) = get(merged, &["editor".to_owned(), "default".to_owned()]).and_then(Value::as_str)
        && !presets.contains(d)
    {
        out.push(RawIssue::new(&["editor".into(), "default".into()], format!("unknown editor preset `{d}`")));
    }
    let templates = ids_of(merged.get("session_templates"));
    for key in ["default_template", "review_template"] {
        let p = vec!["work".to_owned(), key.to_owned()];
        if let Some(t) = get(merged, &p).and_then(Value::as_str)
            && !templates.contains(t)
        {
            out.push(RawIssue::new(&p, format!("unknown session template `{t}`")));
        }
    }
    if let Some(t) = project_default_template
        && !templates.contains(t)
    {
        out.push(RawIssue::new(
            &["project".into(), "default_template".into()],
            format!("unknown session template `{t}`"),
        ));
    }
    let profiles: HashSet<String> = get(merged, &["claude".to_owned(), "profiles".to_owned()])
        .and_then(Value::as_object)
        .map(|m| m.keys().cloned().collect())
        .unwrap_or_default();
    if let Some(Value::Array(ts)) = merged.get("session_templates") {
        for (i, t) in ts.iter().enumerate() {
            let Some(layout) = t.get("layout") else { continue };
            let mut found = Vec::new();
            collect_profiles(
                layout,
                &mut vec!["session_templates".to_owned(), i.to_string(), "layout".to_owned()],
                &mut found,
            );
            for (p, name) in found {
                if !profiles.contains(&name) {
                    out.push(RawIssue::new(&p, format!("unknown Claude profile `{name}`")));
                }
            }
        }
    }
    out
}

/// `plugins.<id>` tables validated against the plugin's flat settings schema.
pub fn plugin_issues(merged: &Value, fragments: &[(kelta_proto::ids::PluginId, Value)]) -> Vec<RawIssue> {
    let mut out = Vec::new();
    for (id, schema) in fragments {
        let Some(table) = get(merged, &["plugins".to_owned(), id.as_str().to_owned()]) else { continue };
        let Ok(v) = jsonschema::validator_for(schema) else { continue };
        for err in v.iter_errors(table) {
            let mut path = vec!["plugins".to_owned(), id.as_str().to_owned()];
            path.extend(pointer_segments(&err.instance_path().to_string()));
            let message = err.to_string();
            if let Some(name) = unexpected_name(&message) {
                path.push(name.clone());
                out.push(RawIssue { path, message: format!("unknown plugin setting `{name}`") });
            } else {
                out.push(RawIssue { path, message });
            }
        }
    }
    out
}

/// All checks that depend on one layer document only.
pub fn layer_issues(layer: Layer, value: &Value, stem: Option<&str>) -> Vec<RawIssue> {
    let mut out = schema_issues(layer, value);
    // structural problems make the follow-up checks noisy; report them first and alone
    if !out.is_empty() {
        return out;
    }
    out.extend(scope_issues(layer, value));
    out.extend(secret_issues(value));
    if layer == Layer::Repo {
        out.extend(repo_key_issues(value));
    }
    out.extend(id_issues(value));
    out.extend(template_issues(value));
    if layer == Layer::Project {
        out.extend(project_issues(value, stem));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn raw_token_in_secret_field_is_rejected() {
        let bad = json!({"accounts": {"jira": {"secret": "ghp_abc"}}});
        let ok = json!({"accounts": {"jira": {"secret": "env:JIRA_TOKEN"}}});
        let issues = secret_issues(&bad);
        assert_eq!(issues.len(), 1);
        assert!(!issues[0].message.contains("ghp_abc"));
        assert!(secret_issues(&ok).is_empty());
    }

    #[test]
    fn schema_reports_range_and_unknown_keys() {
        let v = json!({"terminal": {"font_size": 100}, "bogus": 1, "app": {"nope": true}});
        let issues = schema_issues(Layer::Global, &v);
        let paths: Vec<String> = issues.iter().map(|i| join_path(&i.path)).collect();
        assert!(paths.contains(&"terminal.font_size".to_owned()), "{paths:?}");
        assert!(paths.contains(&"bogus".to_owned()), "{paths:?}");
        assert!(paths.contains(&"app.nope".to_owned()), "{paths:?}");
    }

    #[test]
    fn project_scope_rejects_global_only_keys() {
        let v = json!({"project": {"id": "x", "name": "X"}, "linux": {"graphics": {"profile": "safe"}}, "terminal": {"font_size": 12}});
        let issues = layer_issues(Layer::Project, &v, Some("x"));
        assert_eq!(issues.len(), 1, "{issues:?}");
        assert!(issues[0].message.contains("global only"));
    }

    #[test]
    fn repo_keys_allow_list() {
        let v = json!({"worktree": {"include": [".env"], "root": "/tmp"}, "env": {"A": "1"}, "terminal": {"font_size": 12}});
        let issues = repo_key_issues(&v);
        let paths: Vec<String> = issues.iter().map(|i| join_path(&i.path)).collect();
        assert_eq!(paths, vec!["terminal.font_size", "worktree.root"]);
        assert!(issues[0].message.contains("not allowed in repo config"));
    }

    #[test]
    fn duplicate_ids_and_templates() {
        let v = json!({"tools": [{"id": "a"}, {"id": "a"}], "worktree": {"root": "~/w/{project}/{nope}"}});
        assert_eq!(id_issues(&v).len(), 1);
        let t = template_issues(&v);
        assert_eq!(t.len(), 1);
        assert!(t[0].message.contains("{nope}"));
        assert!(template_error("{worktree|project.root} {repo.path|shell} {x y} {\"a\": 1}", &[]).is_none());
        assert!(template_error("{ticket.nope}", &[]).is_some());
        assert!(template_error("{ticket}", &[]).is_some());
        assert!(template_error("{sock}", EDITOR_EXTRA).is_none());
    }

    #[test]
    fn project_rules() {
        let ok = json!({"project": {"id": "shop", "name": "S", "repos": [{"id": "a", "path": "~/a"}]}});
        assert!(project_issues(&ok, Some("shop")).is_empty());
        assert_eq!(project_issues(&ok, Some("other")).len(), 1);
        let bad = json!({"project": {"id": "Shop!", "repos": [{"id": "a", "path": "x"}, {"id": "a", "path": "y"}]}});
        assert_eq!(project_issues(&bad, None).len(), 2);
    }

    #[test]
    fn cross_refs() {
        let mut d = serde_json::to_value(kelta_proto::settings::Settings::defaults()).unwrap();
        assert!(cross_issues(&d, Some("claude")).is_empty());
        d["editor"]["default"] = json!("ghost");
        d["work"]["review_template"] = json!("ghost");
        assert_eq!(cross_issues(&d, Some("ghost")).len(), 3);
    }
}

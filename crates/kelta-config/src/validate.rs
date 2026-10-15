//! Per-layer validation: JSON Schema, layer scope, repo-local allowed keys, duplicate ids,
//! template placeholders, project file rules and cross references of the merged result.
//!
//! Everything works on `serde_json::Value` and produces raw [`RawIssue`]s (path segments +
//! message); [`position`] turns them into line/col [`ValidationIssue`]s against the TOML text.

use std::collections::HashSet;

use kelta_proto::settings::{Layer, REPO_ALLOWED_KEYS, Severity, ValidationIssue};
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
        .filter(|p| match get(value, p) {
            Some(Value::Array(a)) => a.iter().any(raw_token),
            Some(v) => raw_token(v),
            None => false,
        })
        .map(|p| RawIssue::new(&p, SECRET_REF_EXPECTED))
        .collect()
}

const SECRET_REF_EXPECTED: &str = "expected keyring:<name>, file:<name>, gh-cli, glab-cli, command:<argv> or env:<VAR> (secret values never go in config files)";

/// A non-empty string that is not a `SecretRef`.
fn raw_token(v: &Value) -> bool {
    matches!(v, Value::String(s) if !s.is_empty() && kelta_proto::secret::SecretRef::new(s.as_str()).parse().is_none())
}

/// Repo-local files accept only [`REPO_ALLOWED_KEYS`] (SETTINGS §4).
pub fn repo_key_issues(value: &Value) -> Vec<RawIssue> {
    let mut out = Vec::new();
    for p in leaves(value) {
        // an empty table such as `[worktree]` carries no key
        if matches!(get(value, &p), Some(Value::Object(m)) if m.is_empty()) {
            continue;
        }
        if p.first().map(String::as_str) == Some("tools") {
            out.push(RawIssue::new(&p[..1], REPO_TOOLS_PERSONAL));
            continue;
        }
        let dotted = join_path(&p);
        let allowed = REPO_ALLOWED_KEYS.iter().any(|a| dotted == *a || dotted.starts_with(&format!("{a}.")));
        if !allowed {
            out.push(RawIssue::new(&p, format!("`{dotted}` is not allowed in repo config")));
        }
    }
    out.dedup();
    out
}

/// Tools belong to the user, not to a committed repo file: reported, ignored, rest of the file applies.
pub const REPO_TOOLS_PERSONAL: &str = "`tools` is personal: add it in Settings → Tools";

pub const PROJECT_TOOLS_GLOBAL: &str = "`tools` cannot be set per project (global only)";

pub const PROJECT_COLOR: &str = "`color` must be a theme colour or #rrggbb";

pub const STATUS_COLOR: &str = "a status colour must be a theme colour or #rrggbb";

pub const SOURCE_FIELD: &str = "a source field needs the tracker's field id";

pub const STATUS_MAP_DEPRECATED: &str =
    "`status_map` is deprecated: set `pipeline` steps instead (it is still read)";

/// Keys a hand-written file may get wrong without losing the rest of it: on load and in the settings
/// editor the key is dropped and its issue becomes this warning (`*` matches any one segment). IPC
/// writes stay strict ([`layer_issues`]).
const SOFT_KEYS: &[(Layer, &[&str], &str)] = &[
    (Layer::Repo, &["tools"], REPO_TOOLS_PERSONAL),
    (Layer::Project, &["tools"], PROJECT_TOOLS_GLOBAL),
    (Layer::Project, &["project", "color"], PROJECT_COLOR),
    (Layer::Global, &["sources", "*", "status_colors", "*"], STATUS_COLOR),
    (Layer::Global, &["sources", "*", "fields", "*"], SOURCE_FIELD),
];

/// Load-time reading of a global, project or repo document: an issue under a [`SOFT_KEYS`] path drops
/// that key from `value` and comes back as a warning (the rest of the file still applies).
/// Returns `(errors, warnings)`.
pub fn lenient_issues(layer: Layer, value: &mut Value, stem: Option<&str>) -> (Vec<RawIssue>, Vec<RawIssue>) {
    // `#rgb` reads as `#rrggbb`
    if let Some(c) = value.pointer_mut("/project/color")
        && let Some(h) = c.as_str().and_then(|s| s.strip_prefix('#'))
        && h.len() == 3
        && h.chars().all(|ch| ch.is_ascii_hexdigit())
    {
        *c = Value::String(format!("#{}", h.chars().flat_map(|ch| [ch, ch]).collect::<String>()));
    }
    // `notifications.claude_needs_input` reads as `claude_blocked` (#433); the new key wins
    if let Some(n) = value.get_mut("notifications").and_then(Value::as_object_mut)
        && let Some(v) = n.remove("claude_needs_input")
    {
        n.entry("claude_blocked").or_insert(v);
    }
    let mut warnings = Vec::new();
    loop {
        let raw = layer_issues(layer, value, stem);
        let mut again = false;
        for (k, (_, pattern, message)) in SOFT_KEYS.iter().enumerate().filter(|(_, (l, ..))| *l == layer) {
            // last first: dropping an array item leaves the earlier indices valid; one entry can raise
            // several issues, and a second drop would remove the item that shifted into its place
            let mut dropped: HashSet<&[String]> = HashSet::new();
            for r in raw.iter().rev() {
                let hit = r.path.len() >= pattern.len()
                    && pattern.iter().zip(&r.path).all(|(p, s)| *p == "*" || p == s);
                let path = &r.path[..pattern.len().min(r.path.len())];
                // schema problems hide the follow-up checks, so validate again after a drop
                if hit && !dropped.contains(path) && crate::path::remove(value, path) {
                    dropped.insert(path);
                    warnings.push((k, RawIssue::new(path, *message)));
                    again = true;
                }
            }
        }
        if !again {
            // in SOFT_KEYS order, whichever drop revealed them; deprecations last
            warnings.sort_by_key(|(k, _)| *k);
            let mut out: Vec<RawIssue> = warnings.into_iter().map(|(_, w)| w).collect();
            out.extend(deprecated_issues(value));
            return (raw, out);
        }
    }
}

/// Deprecated keys that are still read: warnings, the key is kept.
fn deprecated_issues(value: &Value) -> Vec<RawIssue> {
    let path = |segs: &[&str]| -> Vec<String> { segs.iter().map(|s| (*s).to_owned()).collect() };
    // an empty `status_map = {}` (left by UI and migration writes) maps nothing
    let set = |m: Option<&Value>| m.and_then(Value::as_object).is_some_and(|m| !m.is_empty());
    let mut out = Vec::new();
    if set(value.pointer("/project/tracker/status_map")) {
        out.push(RawIssue::new(&path(&["project", "tracker", "status_map"]), STATUS_MAP_DEPRECATED));
    }
    for (i, s) in value.get("sources").and_then(Value::as_array).into_iter().flatten().enumerate() {
        if set(s.pointer("/tracker/status_map")) {
            let at = path(&["sources", &i.to_string(), "tracker", "status_map"]);
            out.push(RawIssue::new(&at, STATUS_MAP_DEPRECATED));
        }
    }
    for (on, key) in [
        ("on_start", "transition_to"),
        ("on_start", "comment"),
        ("on_pr", "transition_to"),
        ("on_pr", "comment"),
        ("on_merge", "transition_to"),
    ] {
        if value.pointer(&format!("/work/{on}/{key}")).is_some() {
            let msg = format!(
                "`work.{on}.{key}` is deprecated: set `work.pipeline` steps instead (it is still read)"
            );
            out.push(RawIssue::new(&path(&["work", on, key]), msg));
        }
    }
    out
}

/// [`position`] for non-blocking issues.
pub fn position_warnings(text: &str, parsed: Option<&Parsed>, issues: Vec<RawIssue>) -> Vec<ValidationIssue> {
    position(text, parsed, issues)
        .into_iter()
        .map(|i| ValidationIssue { severity: Severity::Warning, ..i })
        .collect()
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
    "work",
];
const FILTERS: &[&str] = &["slug", "shell", "json"];

fn sub_allowed(root: &str) -> Option<&'static [&'static str]> {
    match root {
        "project" => Some(&["id", "name", "root"]),
        "repo" => Some(&["id", "path", "name"]),
        "ticket" => Some(&["key", "title", "url", "file"]),
        "pr" => Some(&["url", "number", "head", "base", "title"]),
        "session" => Some(&["id", "name", "cwd"]),
        "work" => Some(&["port"]),
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
        (None, _) => !matches!(root, "ticket" | "pr" | "session" | "work"),
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
/// Prompt-template variables beyond the common ones, as the shipped defaults use them. `{options}`:
/// where the start options go (#357), filled by `Core::work_plan`.
const PROMPT_EXTRA: &[&str] = &["options", "task", "file", "args", "workflow", "onto", "step", "total"];

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
            check_str(&mut out, &s(&["claude", "prompt_templates", k]), Some(v), PROMPT_EXTRA);
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

/// `pipeline` steps: a non-empty, unique `id`; a status trigger names a status.
fn pipeline_issues(out: &mut Vec<RawIssue>, base: &[String], list: Option<&Value>) {
    let Some(Value::Array(steps)) = list else { return };
    let mut seen = HashSet::new();
    for (i, s) in steps.iter().enumerate() {
        let at = |keys: &[&str]| -> Vec<String> {
            base.iter().cloned().chain([i.to_string()]).chain(keys.iter().map(|k| (*k).to_owned())).collect()
        };
        match s.get("id").and_then(Value::as_str) {
            Some(id) if !id.is_empty() => {
                if !seen.insert(id) {
                    out.push(RawIssue::new(&at(&["id"]), format!("duplicate step id `{id}`")));
                }
            }
            _ => out.push(RawIssue::new(&at(&["id"]), "a pipeline step needs a non-empty `id`")),
        }
        for (j, t) in s.get("enter").and_then(Value::as_array).into_iter().flatten().enumerate() {
            if t.pointer("/status/name").and_then(Value::as_str).is_some_and(|n| n.trim().is_empty()) {
                let p = at(&["enter", &j.to_string(), "status", "name"]);
                out.push(RawIssue::new(&p, "a status trigger needs a status name"));
            }
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
        Some("home" | "inbox" | "global") => out.push(RawIssue::new(
            &pid,
            "project id is reserved (`home`, `inbox`, `global`): rename the project and its file, e.g. `global-project`",
        )),
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
        let mut p = base.clone();
        p.push("pipeline".into());
        pipeline_issues(&mut out, &p, t.get("pipeline"));
        let mut c = base;
        c.push("columns".into());
        dup_ids(&mut out, &c, t.get("columns"), "column");
    }
    out
}

/// Rules of the global `[[sources]]` (ids: `id_issues`). Unknown projects and accounts are not
/// errors: queries ignore them, fetches report the account.
pub fn source_issues(value: &Value) -> Vec<RawIssue> {
    let mut out = Vec::new();
    let Some(Value::Array(sources)) = value.get("sources") else { return out };
    for (i, s) in sources.iter().enumerate() {
        let at = |keys: &[&str]| -> Vec<String> {
            ["sources", &i.to_string()].iter().chain(keys).map(|k| (*k).to_owned()).collect()
        };
        let t = s.get("tracker");
        let field = |k: &str| t.and_then(|t| t.get(k));
        if field("account").and_then(Value::as_str).is_none_or(str::is_empty) {
            out.push(RawIssue::new(&at(&["tracker", "account"]), "source needs a tracker account"));
        }
        dup_ids(&mut out, &at(&["tracker", "views"]), field("views"), "view");
        dup_ids(&mut out, &at(&["tracker", "columns"]), field("columns"), "column");
        pipeline_issues(&mut out, &at(&["tracker", "pipeline"]), field("pipeline"));
        if field("repo_rules").and_then(Value::as_array).is_some_and(|a| !a.is_empty()) {
            out.push(RawIssue::new(
                &at(&["tracker", "repo_rules"]),
                "use `sources.routes` instead of `repo_rules`",
            ));
        }
        let projects = s.get("projects").and_then(Value::as_array);
        for (j, r) in s.get("routes").and_then(Value::as_array).into_iter().flatten().enumerate() {
            let p = r.get("project").and_then(Value::as_str).unwrap_or_default();
            if !projects.is_some_and(|ps| ps.iter().any(|x| x.as_str() == Some(p))) {
                let msg = format!("route project `{p}` is not in this source's `projects`");
                out.push(RawIssue::new(&at(&["routes", &j.to_string(), "project"]), msg));
            }
        }
    }
    out
}

// ---------------------------------------------------------------------------------------------
// Cross references of the merged document
// ---------------------------------------------------------------------------------------------

pub(crate) fn ids_of(v: Option<&Value>) -> HashSet<String> {
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

/// Unknown preset / template / profile ids in the merged document. A template in `pending` (defined
/// only by a repo file that is not trusted yet) is not unknown: it appears once the file is trusted.
pub fn cross_issues(
    merged: &Value,
    project_default_template: Option<&str>,
    pending: &HashSet<String>,
) -> Vec<RawIssue> {
    let mut out = Vec::new();
    let presets = ids_of(get(merged, &["editor".to_owned(), "presets".to_owned()]));
    if let Some(d) = get(merged, &["editor".to_owned(), "default".to_owned()]).and_then(Value::as_str)
        && !presets.contains(d)
    {
        out.push(RawIssue::new(&["editor".into(), "default".into()], format!("unknown editor preset `{d}`")));
    }
    let mut templates = ids_of(merged.get("session_templates"));
    templates.extend(pending.iter().cloned());
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
        // `x-kelta-secret` keys of the fragment, as `secret_issues` does for the static schema.
        for (k, prop) in schema.get("properties").and_then(Value::as_object).into_iter().flatten() {
            if prop.get("x-kelta-secret").and_then(Value::as_bool) == Some(true)
                && table.get(k).is_some_and(raw_token)
            {
                let path = ["plugins".to_owned(), id.as_str().to_owned(), k.clone()];
                out.push(RawIssue::new(&path, SECRET_REF_EXPECTED));
            }
        }
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
    pipeline_issues(&mut out, &["work".to_owned(), "pipeline".to_owned()], value.pointer("/work/pipeline"));
    out.extend(template_issues(value));
    if layer == Layer::Project {
        out.extend(project_issues(value, stem));
    }
    if layer == Layer::Global {
        out.extend(source_issues(value));
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
    fn raw_token_in_plugin_secret_setting_is_rejected() {
        let frag = json!({"type": "object", "properties": {
            "qa_token": {"type": "string", "x-kelta-secret": true}, "url": {"type": "string"}}});
        let frags = [(kelta_proto::ids::PluginId::new("qa"), frag)];
        let bad = json!({"plugins": {"qa": {"qa_token": "s3cr3t-tok", "url": "plain"}}});
        let issues = plugin_issues(&bad, &frags);
        assert_eq!(issues.len(), 1, "{issues:?}");
        assert_eq!(join_path(&issues[0].path), "plugins.qa.qa_token");
        assert!(!issues[0].message.contains("s3cr3t-tok"));
        let ok = json!({"plugins": {"qa": {"qa_token": "env:QA_TOKEN", "url": "plain"}}});
        assert!(plugin_issues(&ok, &frags).is_empty());
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
    fn pipeline_rules() {
        let paths = |v: &Value, l: Layer, stem: Option<&str>| -> Vec<String> {
            layer_issues(l, v, stem).iter().map(|i| join_path(&i.path)).collect()
        };
        let src =
            |steps: Value| json!({"sources": [{"id": "a", "tracker": {"account": "j", "pipeline": steps}}]});
        let g = |v: &Value| paths(v, Layer::Global, None);
        let step = |id: &str| json!({"id": id, "label": id});
        let ok = src(json!([
            {"id": "a", "label": "A", "enter": ["pr_ready", {"status": {"name": "PO Review"}}]},
            {"id": "b", "label": "B", "transition_to": {"name": "Done"}},
        ]));
        assert!(g(&ok).is_empty());
        let dup = src(json!([step("a"), step("b"), step("a")]));
        assert_eq!(g(&dup), ["sources.0.tracker.pipeline.2.id"]);
        let issues = layer_issues(Layer::Global, &dup, None);
        assert!(issues[0].message.contains("duplicate step id `a`"));
        assert_eq!(g(&json!({"work": {"pipeline": [{"id": ""}]}})), ["work.pipeline.0.id"]);
        let blank = src(json!([{"id": "a", "enter": ["merged", {"status": {"name": " "}}]}]));
        assert_eq!(g(&blank), ["sources.0.tracker.pipeline.0.enter.1.status.name"]);
        let proj = json!({"project": {"id": "x", "name": "X",
            "tracker": {"account": "j", "pipeline": [{"id": "a"}, {"id": "a"}]}}});
        assert_eq!(paths(&proj, Layer::Project, Some("x")), ["project.tracker.pipeline.1.id"]);
    }

    #[test]
    fn sources_rules() {
        let src = |extra: Value| {
            let mut s = json!({"id": "product", "projects": ["web"], "tracker": {"account": "jira",
                "views": [{"id": "mine"}], "columns": [{"id": "todo"}]}});
            crate::service::deep_merge(&mut s, &extra);
            json!({"sources": [s]})
        };
        let paths = |v: &Value| -> Vec<String> {
            layer_issues(Layer::Global, v, None).iter().map(|i| join_path(&i.path)).collect()
        };
        // unknown projects and accounts are fine
        assert!(
            paths(&src(json!({"projects": ["gone"], "routes": [{"project": "gone", "repo": "x"}]})))
                .is_empty()
        );
        assert!(paths(&json!({"sources": [{"id": "a", "tracker": {"account": "j"}}]})).is_empty());
        assert_eq!(paths(&src(json!({"tracker": {"account": ""}}))), ["sources.0.tracker.account"]);
        assert_eq!(paths(&src(json!({"id": ""}))), ["sources.0.id"]);
        let two = json!({"sources": [{"id": "a", "tracker": {"account": "j"}}, {"id": "a", "tracker": {"account": "j"}}]});
        assert_eq!(paths(&two), ["sources.1.id"]);
        let dup = src(
            json!({"tracker": {"views": [{"id": "x"}, {"id": "x"}], "columns": [{"id": "c"}, {"id": "c"}]}}),
        );
        assert_eq!(paths(&dup), ["sources.0.tracker.views.1.id", "sources.0.tracker.columns.1.id"]);
        let rules = src(json!({"tracker": {"repo_rules": [{"repo": "web"}]}}));
        assert_eq!(paths(&rules), ["sources.0.tracker.repo_rules"]);
        let route = src(json!({"routes": [{"project": "api", "repo": "x"}]}));
        assert_eq!(paths(&route), ["sources.0.routes.0.project"]);
        // global only
        let proj = json!({"project": {"id": "x", "name": "X"}, "sources": [{"id": "a", "tracker": {"account": "j"}}]});
        let issues = layer_issues(Layer::Project, &proj, Some("x"));
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
        let t = json!({"tools": [{"id": "a", "command": "x"}, {"id": "b"}]});
        assert_eq!(repo_key_issues(&t), vec![RawIssue::new(&["tools".to_owned()], REPO_TOOLS_PERSONAL)]);
    }

    #[test]
    fn tools_are_global_only() {
        let v = json!({"project": {"id": "x", "name": "X"}, "tools": [{"id": "a", "command": "x"}]});
        let issues = layer_issues(Layer::Project, &v, Some("x"));
        let t = vec!["tools".to_owned()];
        assert_eq!(issues, vec![RawIssue::new(&t, "`tools` cannot be set per project (global only)")]);
    }

    #[test]
    fn soft_keys_are_dropped_as_warnings() {
        let paths = |v: &[RawIssue]| -> Vec<String> { v.iter().map(|i| join_path(&i.path)).collect() };
        // `tools` that fails the schema, next to a bad colour hidden behind it until the first drop
        let doc = json!({"project": {"id": "x", "color": "red"}, "tools": [{"id": 3}]});
        let mut v = doc.clone();
        let (errors, warnings) = lenient_issues(Layer::Project, &mut v, Some("x"));
        assert!(errors.is_empty(), "{errors:?}");
        assert_eq!(paths(&warnings), ["tools", "project.color"]);
        assert_eq!(v, json!({"project": {"id": "x"}}));
        let mut v = json!({"tools": [{"id": 3}], "env": {"A": "1"}});
        let (errors, warnings) = lenient_issues(Layer::Repo, &mut v, None);
        assert!(errors.is_empty(), "{errors:?}");
        assert_eq!(warnings, [RawIssue::new(&["tools".to_owned()], REPO_TOOLS_PERSONAL)]);
        // global `tools` is no soft key: its errors stay errors
        let mut v = json!({"tools": [{"id": 3}]});
        let (errors, warnings) = lenient_issues(Layer::Global, &mut v, None);
        assert!(!errors.is_empty() && warnings.is_empty());
    }

    #[test]
    fn a_bad_status_colour_is_dropped_as_a_warning() {
        let source = |c: Value| json!({"id": "a", "tracker": {"account": "j"}, "status_colors": c});
        let mut v = json!({"sources": [source(json!({"In QA": "theme:orange", "Ready": "#ff8800"}))]});
        let (errors, warnings) = lenient_issues(Layer::Global, &mut v, None);
        assert!(errors.is_empty(), "{errors:?}");
        let path: Vec<String> = ["sources", "0", "status_colors", "In QA"].map(str::to_owned).into();
        assert_eq!(warnings, [RawIssue::new(&path, STATUS_COLOR)]);
        assert_eq!(v, json!({"sources": [source(json!({"Ready": "#ff8800"}))]}));
        // IPC writes stay strict
        let bad = json!({"sources": [source(json!({"Ready": "orange"}))]});
        assert_eq!(layer_issues(Layer::Global, &bad, None).len(), 1);
    }

    #[test]
    fn a_source_field_without_id_is_dropped_as_a_warning() {
        let field = |id: &str| json!({"id": id, "label": "Verified"});
        let source =
            json!({"id": "a", "tracker": {"account": "j"}, "fields": [field(""), field("cf_1"), field("")]});
        let mut v = json!({"sources": [source]});
        let (errors, warnings) = lenient_issues(Layer::Global, &mut v, None);
        assert!(errors.is_empty(), "{errors:?}");
        let path = |i: &str| -> Vec<String> { ["sources", "0", "fields", i].map(str::to_owned).into() };
        assert_eq!(
            warnings,
            [RawIssue::new(&path("2"), SOURCE_FIELD), RawIssue::new(&path("0"), SOURCE_FIELD)]
        );
        assert_eq!(v["sources"][0]["fields"], json!([field("cf_1")]));
    }

    #[test]
    fn a_source_field_with_two_issues_or_no_id_drops_only_itself() {
        // empty id plus a misspelt key: two issues on one entry; a missing id: one
        let fields = json!([{"id": "", "lable": "x"}, {"id": "cf_1"}, {"label": "Verified"}]);
        let mut v = json!({"sources": [{"id": "a", "tracker": {"account": "j"}, "fields": fields}]});
        let (errors, warnings) = lenient_issues(Layer::Global, &mut v, None);
        assert!(errors.is_empty(), "{errors:?}");
        assert_eq!(warnings.len(), 2, "{warnings:?}");
        assert_eq!(v["sources"][0]["fields"], json!([{"id": "cf_1"}]));
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
        for id in ["home", "inbox", "global"] {
            let reserved = json!({"project": {"id": id, "name": "R", "repos": []}});
            assert_eq!(project_issues(&reserved, Some(id)).len(), 1, "{id}");
        }
    }

    #[test]
    fn project_color_is_a_theme_slot_or_a_hex() {
        let issues = |c: &str| {
            let v = json!({"project": {"id": "x", "name": "X", "color": c}});
            layer_issues(Layer::Project, &v, Some("x"))
        };
        for ok in ["theme:green", "theme:bright-cyan", "#ff8800", "#5E8C4A", "#f80"] {
            assert!(issues(ok).is_empty(), "{ok}: {:?}", issues(ok));
        }
        for bad in ["green", "theme:black", "theme:bright-white", "theme:", "#f8", "#ff880", "rgb(1,2,3)"] {
            let i = issues(bad);
            assert_eq!(i.len(), 1, "{bad}: {i:?}");
            assert_eq!(join_path(&i[0].path), "project.color");
        }
    }

    #[test]
    fn cross_refs() {
        let mut d = serde_json::to_value(kelta_proto::settings::Settings::defaults()).unwrap();
        let none = HashSet::new();
        assert!(cross_issues(&d, Some("claude"), &none).is_empty());
        d["editor"]["default"] = json!("ghost");
        d["work"]["review_template"] = json!("ghost");
        assert_eq!(cross_issues(&d, Some("ghost"), &none).len(), 3);
        // Defined by an untrusted repo file: not an error until it is trusted.
        assert_eq!(cross_issues(&d, Some("ghost"), &HashSet::from(["ghost".to_owned()])).len(), 1);
    }
}

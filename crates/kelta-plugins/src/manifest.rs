//! `kelta-plugin.toml` / `.json` parsing and validation (PLUGINS §4): JSON Schema (generated from
//! the proto types), id rules, semver `version`, `kelta_api` requirement, platforms, permissions,
//! activation events, contributed ids, safe relative paths and template placeholders.

use std::collections::HashSet;
use std::path::{Component, Path, PathBuf};
use std::sync::LazyLock;

use kelta_proto::error::KeltaError;
use kelta_proto::ext::{
    ActionDef, KELTA_API_VERSION, Permission, PlatformName, PluginManifest, ToolDef, ToolKind, TriggerDef,
};
use kelta_proto::ids::PluginId;
use serde_json::Value;

use crate::matcher;
use crate::template::unknown_placeholders;
use crate::util::sha256_hex;

pub const MANIFEST_TOML: &str = "kelta-plugin.toml";
pub const MANIFEST_JSON: &str = "kelta-plugin.json";

/// A parsed, schema-valid manifest.
#[derive(Debug, Clone)]
pub struct ParsedManifest {
    pub manifest: PluginManifest,
    /// SHA-256 of the manifest file bytes (grants are keyed by it).
    pub sha256: String,
    pub path: PathBuf,
    /// Non-fatal: the plugin is listed but not activated (incompatible API, platform, missing files).
    pub problems: Vec<String>,
}

static VALIDATOR: LazyLock<Result<jsonschema::Validator, String>> = LazyLock::new(|| {
    jsonschema::validator_for(&kelta_proto::schema::plugin_manifest_schema()).map_err(|e| e.to_string())
});

/// Manifest file inside `dir` (TOML preferred).
pub fn manifest_path(dir: &Path) -> Option<PathBuf> {
    [MANIFEST_TOML, MANIFEST_JSON].iter().map(|n| dir.join(n)).find(|p| p.is_file())
}

/// Invalid-manifest error carrying every message in `detail.errors`.
pub fn invalid(errors: Vec<String>) -> KeltaError {
    let first = errors.first().cloned().unwrap_or_else(|| "invalid manifest".into());
    let msg = if errors.len() > 1 {
        format!("invalid plugin manifest: {first} (+{} more)", errors.len() - 1)
    } else {
        format!("invalid plugin manifest: {first}")
    };
    KeltaError::invalid(msg).with_detail(serde_json::json!({ "errors": errors }))
}

/// Parse manifest bytes (`json = true` for `.json`). Errors are human-readable lines.
pub fn parse(bytes: &[u8], json: bool) -> Result<PluginManifest, Vec<String>> {
    let text = std::str::from_utf8(bytes).map_err(|_| vec!["manifest is not UTF-8".to_owned()])?;
    let value: Value = if json {
        serde_json::from_str(text).map_err(|e| vec![format!("JSON: {e}")])?
    } else {
        toml::from_str(text).map_err(|e| vec![format!("TOML: {}", e.to_string().trim())])?
    };
    let validator = VALIDATOR.as_ref().map_err(|e| vec![format!("manifest schema: {e}")])?;
    let mut errors: Vec<String> = validator
        .iter_errors(&value)
        .map(|e| {
            let path = e.instance_path().to_string();
            if path.is_empty() { e.to_string() } else { format!("{path}: {e}") }
        })
        .collect();
    if !errors.is_empty() {
        errors.sort();
        errors.dedup();
        return Err(errors);
    }
    let manifest: PluginManifest = serde_json::from_value(value).map_err(|e| vec![e.to_string()])?;
    let errors = validate(&manifest);
    if errors.is_empty() { Ok(manifest) } else { Err(errors) }
}

/// Parse + validate the manifest in `dir`, compute its hash and the non-fatal problems.
pub fn load_dir(dir: &Path) -> Result<ParsedManifest, KeltaError> {
    let path = manifest_path(dir)
        .ok_or_else(|| KeltaError::not_found(format!("no {MANIFEST_TOML} in {}", dir.display())))?;
    let bytes = std::fs::read(&path).map_err(|e| crate::util::io_err(path.display(), e))?;
    let json = path.extension().and_then(|e| e.to_str()) == Some("json");
    let manifest = parse(&bytes, json).map_err(invalid)?;
    let mut problems = compatibility_problems(&manifest);
    for screen in &manifest.contributes.screens {
        if !dir.join(&screen.entry).is_file() {
            problems.push(format!("screen `{}`: entry `{}` not found", screen.id, screen.entry));
        }
    }
    if let Some(s) = &manifest.contributes.settings
        && !dir.join(&s.schema).is_file()
    {
        problems.push(format!("settings schema `{}` not found", s.schema));
    }
    Ok(ParsedManifest { manifest, sha256: sha256_hex(&bytes), path, problems })
}

/// `kelta_api` and `platforms` checks (the plugin is shown with the reason, not loaded).
pub fn compatibility_problems(m: &PluginManifest) -> Vec<String> {
    let mut out = Vec::new();
    if let Ok(req) = semver::VersionReq::parse(&m.kelta_api)
        && let Ok(host) = semver::Version::parse(KELTA_API_VERSION)
        && !req.matches(&host)
    {
        out.push(format!("requires Kelta API `{}`; this Kelta provides {KELTA_API_VERSION}", m.kelta_api));
    }
    let current = if cfg!(target_os = "macos") { PlatformName::Macos } else { PlatformName::Linux };
    if !m.platforms.is_empty() && !m.platforms.contains(&current) {
        out.push(format!("not available on this platform ({current:?})").to_lowercase());
    }
    out
}

fn is_local_id(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 64
        && s.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

fn is_command_id(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 96
        && s.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'/'))
}

/// Relative path that stays inside the plugin dir (no `..`, not absolute, no backslashes).
pub fn is_safe_relative(p: &str) -> bool {
    if p.is_empty() || p.contains('\\') || p.contains('\0') {
        return false;
    }
    Path::new(p).components().all(|c| matches!(c, Component::Normal(_) | Component::CurDir))
}

fn check_strings(v: &Value, ctx: &str, errors: &mut Vec<String>) {
    match v {
        Value::String(s) => {
            for bad in unknown_placeholders(s) {
                errors.push(format!("{ctx}: unknown placeholder `{{{bad}}}`"));
            }
        }
        Value::Array(a) => a.iter().for_each(|x| check_strings(x, ctx, errors)),
        // `secret_headers` formats use `{secret}`, filled host-side, not the template vars.
        Value::Object(m) => {
            m.iter().filter(|(k, _)| *k != "secret_headers").for_each(|(_, x)| check_strings(x, ctx, errors))
        }
        _ => {}
    }
}

/// Validate one tool definition (shared with config tools).
pub fn validate_tool(t: &ToolDef, ctx: &str, errors: &mut Vec<String>) {
    if !is_local_id(&t.id) {
        errors.push(format!("{ctx}: invalid tool id `{}` (expected [a-z0-9-]+)", t.id));
    }
    match t.kind {
        ToolKind::Pty if t.command.as_deref().is_none_or(str::is_empty) => {
            errors.push(format!("{ctx}: pty tool `{}` needs `command`", t.id));
        }
        ToolKind::Web if t.url.is_none() && t.start.is_none() => {
            errors.push(format!("{ctx}: web tool `{}` needs `url` or `start`", t.id));
        }
        _ => {}
    }
    if let Some(check) = &t.check
        && check.is_empty()
    {
        errors.push(format!("{ctx}: tool `{}`: `check` must not be empty", t.id));
    }
    if let Some(start) = &t.start
        && let kelta_proto::ext::Ready::StdoutRegex(re) = &start.ready
        && let Err(e) = regex::Regex::new(re)
    {
        errors.push(format!("{ctx}: tool `{}`: invalid ready regex: {e}", t.id));
    }
    if let Ok(v) = serde_json::to_value(t) {
        check_strings(&v, &format!("{ctx}: tool `{}`", t.id), errors);
    }
}

fn validate_actions(actions: &[ActionDef], ctx: &str, errors: &mut Vec<String>) {
    if let Ok(v) = serde_json::to_value(actions) {
        check_strings(&v, ctx, errors);
    }
}

/// Validate one trigger definition (shared with config triggers).
pub fn validate_trigger(t: &TriggerDef, ctx: &str, errors: &mut Vec<String>) {
    if !is_command_id(&t.id) {
        errors.push(format!("{ctx}: invalid trigger id `{}`", t.id));
    }
    if t.on.trim().is_empty() {
        errors.push(format!("{ctx}: trigger `{}` needs `on`", t.id));
    } else if let Err(e) = matcher::glob(&t.on) {
        errors.push(format!("{ctx}: trigger `{}`: {}", t.id, e.message));
    }
    for (path, m) in &t.r#match {
        if let Err(e) = matcher::compile(m) {
            errors.push(format!("{ctx}: trigger `{}` match `{path}`: {}", t.id, e.message));
        }
    }
    validate_actions(&t.r#do, &format!("{ctx}: trigger `{}`", t.id), errors);
}

/// Semantic validation (after the schema). Returns every error.
pub fn validate(m: &PluginManifest) -> Vec<String> {
    let mut errors = Vec::new();
    if !PluginId::is_valid(m.id.as_str()) {
        errors.push(format!("id `{}`: expected [a-z0-9-]{{3,40}} not starting with `kelta`", m.id));
    }
    for (field, value) in
        [("name", &m.name), ("description", &m.description), ("author", &m.author), ("license", &m.license)]
    {
        if value.trim().is_empty() {
            errors.push(format!("`{field}` must not be empty"));
        }
    }
    if let Err(e) = semver::Version::parse(&m.version) {
        errors.push(format!("version `{}`: {e}", m.version));
    }
    if let Err(e) = semver::VersionReq::parse(&m.kelta_api) {
        errors.push(format!("kelta_api `{}`: {e}", m.kelta_api));
    }
    for p in &m.permissions {
        match Permission::parse(p) {
            None => errors.push(format!("unknown permission `{p}`")),
            Some(Permission::Net(host)) => {
                let h = host.strip_prefix("*.").unwrap_or(&host);
                if h.is_empty() || h.contains(['/', ':', '*', ' ', '@']) {
                    errors.push(format!("permission `{p}`: expected a host or `*.domain`"));
                }
            }
            Some(Permission::Events(g)) => {
                if matcher::glob(&g).is_err() {
                    errors.push(format!("permission `{p}`: invalid glob"));
                }
            }
            Some(Permission::Exec(c)) => {
                if c.contains('/') {
                    errors.push(format!("permission `{p}`: expected a program name, not a path"));
                }
            }
            Some(_) => {}
        }
    }
    for a in &m.activation {
        let ok = match a.split_once(':') {
            None => matches!(a.as_str(), "onStartup" | "onProjectOpen"),
            Some(("onCommand" | "onScreen", id)) => !id.is_empty(),
            Some(("onEvent", g)) => !g.is_empty() && matcher::glob(g).is_ok(),
            Some(_) => false,
        };
        if !ok {
            errors.push(format!("unknown activation event `{a}`"));
        }
    }
    let c = &m.contributes;
    let mut seen = HashSet::new();
    for t in &c.tools {
        validate_tool(t, "contributes.tools", &mut errors);
        if !seen.insert(t.id.clone()) {
            errors.push(format!("contributes.tools: duplicate id `{}`", t.id));
        }
    }
    let mut seen = HashSet::new();
    for t in &c.triggers {
        validate_trigger(t, "contributes.triggers", &mut errors);
        if !seen.insert(t.id.clone()) {
            errors.push(format!("contributes.triggers: duplicate id `{}`", t.id));
        }
    }
    let mut command_ids = HashSet::new();
    for cmd in &c.commands {
        if !is_command_id(&cmd.id) {
            errors.push(format!("contributes.commands: invalid id `{}`", cmd.id));
        }
        if cmd.title.trim().is_empty() {
            errors.push(format!("contributes.commands `{}`: `title` must not be empty", cmd.id));
        }
        if !command_ids.insert(cmd.id.clone()) {
            errors.push(format!("contributes.commands: duplicate id `{}`", cmd.id));
        }
        validate_actions(&cmd.r#do, &format!("contributes.commands `{}`", cmd.id), &mut errors);
    }
    for kb in &c.keybindings {
        if !command_ids.contains(&kb.command) {
            errors.push(format!("contributes.keybindings: unknown command `{}`", kb.command));
        }
        if kb.key.trim().is_empty() {
            errors.push(format!("contributes.keybindings `{}`: empty key", kb.command));
        }
    }
    let mut seen = HashSet::new();
    for s in &c.screens {
        if !is_local_id(&s.id) {
            errors.push(format!("contributes.screens: invalid id `{}`", s.id));
        }
        if !seen.insert(s.id.clone()) {
            errors.push(format!("contributes.screens: duplicate id `{}`", s.id));
        }
        if !is_safe_relative(&s.entry) {
            errors.push(format!(
                "contributes.screens `{}`: entry `{}` must be a relative path inside the plugin",
                s.id, s.entry
            ));
        }
        if s.placement.is_empty() {
            errors.push(format!("contributes.screens `{}`: `placement` must not be empty", s.id));
        }
    }
    for (kind, list) in [("ticket_actions", &c.ticket_actions), ("review_actions", &c.review_actions)] {
        let mut seen = HashSet::new();
        for b in list {
            if !is_command_id(&b.id) || !seen.insert(b.id.clone()) {
                errors.push(format!("contributes.{kind}: invalid or duplicate id `{}`", b.id));
            }
            validate_actions(&b.r#do, &format!("contributes.{kind} `{}`", b.id), &mut errors);
        }
    }
    let mut seen = HashSet::new();
    for t in &c.session_templates {
        if !is_command_id(&t.id) || !seen.insert(t.id.clone()) {
            errors.push(format!("contributes.session_templates: invalid or duplicate id `{}`", t.id));
        }
    }
    if let Some(p) = &m.provider {
        if !m.permissions.iter().any(|x| x == "provider") {
            errors.push("[provider] needs the `provider` permission".to_owned());
        }
        if p.command.trim().is_empty() || (p.command.contains('/') && !is_safe_relative(&p.command)) {
            errors.push(format!(
                "[provider]: command `{}` must be a program name or a relative path inside the plugin",
                p.command
            ));
        }
    }
    if let Some(s) = &c.settings
        && !is_safe_relative(&s.schema)
    {
        errors.push(format!(
            "contributes.settings: schema `{}` must be a relative path inside the plugin",
            s.schema
        ));
    }
    errors
}

#[cfg(test)]
mod tests {
    use super::*;

    const MINIMAL: &str = r#"
id = "hello"
name = "Hello"
version = "0.1.0"
kelta_api = "^0.1"
description = "d"
author = "a"
license = "MIT"
"#;

    #[test]
    fn minimal_manifest_parses() {
        let m = parse(MINIMAL.as_bytes(), false).unwrap();
        assert_eq!(m.id.as_str(), "hello");
        assert!(compatibility_problems(&m).is_empty());
    }

    #[test]
    fn rejects_unknown_fields_and_bad_ids() {
        let e = parse(format!("{MINIMAL}\nbogus = 1").as_bytes(), false).unwrap_err();
        assert!(e.iter().any(|x| x.contains("bogus")), "{e:?}");
        let e = parse(MINIMAL.replace("\"hello\"", "\"kelta-x\"").as_bytes(), false).unwrap_err();
        assert!(e.iter().any(|x| x.contains("not starting with")), "{e:?}");
    }

    #[test]
    fn api_mismatch_is_a_problem_not_an_error() {
        let m = parse(MINIMAL.replace("^0.1", "^2.0").as_bytes(), false).unwrap();
        assert_eq!(compatibility_problems(&m).len(), 1);
    }

    #[test]
    fn safe_paths() {
        assert!(is_safe_relative("dist/index.html"));
        assert!(is_safe_relative("./a.js"));
        assert!(!is_safe_relative("../x"));
        assert!(!is_safe_relative("/etc/passwd"));
        assert!(!is_safe_relative("a\\b"));
        assert!(!is_safe_relative(""));
    }
}

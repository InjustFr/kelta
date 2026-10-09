//! Trigger matchers (PLUGINS §3): exact / `glob:` / `re:` / `!` negation, bools, numbers, any-of.
//! No expression language. A missing path never matches; an array value matches when any of its
//! elements matches.

use globset::{Glob, GlobMatcher};
use kelta_proto::error::KeltaError;
use kelta_proto::ext::Matcher;
use regex::Regex;
use serde_json::Value;

use crate::util::json_path;

/// A compiled matcher.
#[derive(Debug, Clone)]
pub enum Compiled {
    Exact(String),
    Glob(GlobMatcher),
    Re(Regex),
    Not(Box<Compiled>),
    Bool(bool),
    Num(f64),
    AnyOf(Vec<Compiled>),
}

/// Compile a glob where `*` matches any character (event names and values contain no paths).
pub fn glob(pattern: &str) -> Result<GlobMatcher, KeltaError> {
    Glob::new(pattern)
        .map(|g| g.compile_matcher())
        .map_err(|e| KeltaError::invalid(format!("invalid glob `{pattern}`: {e}")))
}

pub fn compile(m: &Matcher) -> Result<Compiled, KeltaError> {
    Ok(match m {
        Matcher::Str(s) => compile_str(s)?,
        Matcher::Bool(b) => Compiled::Bool(*b),
        Matcher::Num(n) => Compiled::Num(*n),
        Matcher::AnyOf(items) => Compiled::AnyOf(items.iter().map(compile).collect::<Result<_, _>>()?),
    })
}

fn compile_str(s: &str) -> Result<Compiled, KeltaError> {
    if let Some(rest) = s.strip_prefix('!') {
        return Ok(Compiled::Not(Box::new(compile_str(rest)?)));
    }
    if let Some(g) = s.strip_prefix("glob:") {
        return Ok(Compiled::Glob(glob(g)?));
    }
    if let Some(r) = s.strip_prefix("re:") {
        return Regex::new(r)
            .map(Compiled::Re)
            .map_err(|e| KeltaError::invalid(format!("invalid regex `{r}`: {e}")));
    }
    Ok(Compiled::Exact(s.to_owned()))
}

fn scalar_str(v: &Value) -> Option<String> {
    match v {
        Value::String(s) => Some(s.clone()),
        Value::Bool(b) => Some(b.to_string()),
        Value::Number(n) => Some(n.to_string()),
        _ => None,
    }
}

impl Compiled {
    /// Match one present value.
    pub fn matches(&self, v: &Value) -> bool {
        if let Value::Array(items) = v {
            // A negation over a list means "no element matches".
            if let Compiled::Not(inner) = self {
                return !items.iter().any(|i| inner.matches(i));
            }
            return items.iter().any(|i| self.matches(i));
        }
        match self {
            Compiled::Exact(s) => scalar_str(v).is_some_and(|x| &x == s),
            Compiled::Glob(g) => scalar_str(v).is_some_and(|x| g.is_match(x)),
            Compiled::Re(r) => scalar_str(v).is_some_and(|x| r.is_match(&x)),
            Compiled::Not(inner) => !inner.matches(v),
            Compiled::Bool(b) => v.as_bool() == Some(*b),
            Compiled::Num(n) => v.as_f64().is_some_and(|x| (x - n).abs() < f64::EPSILON),
            Compiled::AnyOf(items) => items.iter().any(|m| m.matches(v)),
        }
    }
}

/// All `(path, matcher)` pairs must match the context. Missing paths never match.
pub fn matches_all(ctx: &Value, matchers: &[(String, Compiled)]) -> bool {
    matchers
        .iter()
        .all(|(path, m)| json_path(ctx, path).filter(|v| !v.is_null()).is_some_and(|v| m.matches(v)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn m(v: Value) -> Compiled {
        compile(&serde_json::from_value::<Matcher>(v).unwrap()).unwrap()
    }

    #[test]
    fn string_forms() {
        assert!(m(json!("needs_input")).matches(&json!("needs_input")));
        assert!(!m(json!("needs_input")).matches(&json!("working")));
        assert!(m(json!("glob:*.rs")).matches(&json!("src/main.rs")));
        assert!(!m(json!("glob:*.rs")).matches(&json!("README.md")));
        assert!(m(json!("re:^SHOP-\\d+$")).matches(&json!("SHOP-12")));
        assert!(m(json!("!done")).matches(&json!("todo")));
        assert!(!m(json!("!done")).matches(&json!("done")));
        assert!(m(json!("!glob:*.md")).matches(&json!("a.rs")));
    }

    #[test]
    fn scalars_lists_and_any_of() {
        assert!(m(json!(false)).matches(&json!(false)));
        assert!(!m(json!(false)).matches(&json!("false")));
        assert!(m(json!(3)).matches(&json!(3)));
        assert!(m(json!(["needs_input", "waiting_user"])).matches(&json!("waiting_user")));
        assert!(!m(json!(["needs_input", "waiting_user"])).matches(&json!("done")));
        // array values: any element
        assert!(m(json!("re:.+")).matches(&json!(["SHOP-1"])));
        assert!(!m(json!("re:.+")).matches(&json!([])));
        assert!(m(json!("!SHOP-2")).matches(&json!(["SHOP-1"])));
        assert!(!m(json!("!SHOP-1")).matches(&json!(["SHOP-1", "SHOP-3"])));
    }

    #[test]
    fn missing_paths_never_match() {
        let ctx = json!({"payload": {"status": "done"}, "session": {"visible": false}});
        let ms = vec![
            ("payload.status".to_owned(), m(json!("done"))),
            ("session.visible".to_owned(), m(json!(false))),
        ];
        assert!(matches_all(&ctx, &ms));
        let neg = vec![("payload.nope".to_owned(), m(json!("!x")))];
        assert!(!matches_all(&ctx, &neg));
    }

    #[test]
    fn invalid_patterns_error() {
        assert!(compile(&Matcher::Str("re:(".into())).is_err());
        assert!(compile(&Matcher::Str("glob:[".into())).is_err());
    }
}

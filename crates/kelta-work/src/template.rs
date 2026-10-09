//! Placeholder templates (SETTINGS §6), slugs and POSIX quoting helpers.
//!
//! Syntax: `{name}`, `{name|filter}` (filters `slug`, `shell`, `json`), `{a|b}` = first non-empty of
//! `a` or `b`; `{{` / `}}` are literal braces. Values are substituted per string (callers render
//! each argv element separately, so no shell parsing happens).

use std::collections::BTreeMap;

use kelta_proto::error::KeltaError;

/// Placeholder values.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Ctx {
    values: BTreeMap<String, String>,
}

impl Ctx {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn set(&mut self, key: &str, value: impl Into<String>) -> &mut Self {
        self.values.insert(key.to_owned(), value.into());
        self
    }

    pub fn with(mut self, key: &str, value: impl Into<String>) -> Self {
        self.set(key, value);
        self
    }

    pub fn get(&self, key: &str) -> Option<&str> {
        self.values.get(key).map(String::as_str)
    }
}

/// How unknown placeholders are treated.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// Unknown placeholder → `InvalidArgument`.
    Strict,
    /// Unknown placeholder → kept verbatim (`{run}` survives plan-time rendering).
    Lenient,
}

const FILTERS: &[&str] = &["slug", "shell", "json"];

/// Render `template` with `ctx`.
pub fn render(template: &str, ctx: &Ctx, mode: Mode) -> Result<String, KeltaError> {
    let mut out = String::with_capacity(template.len());
    let mut rest = template;
    while let Some(i) = rest.find(['{', '}']) {
        out.push_str(&rest[..i]);
        let tail = &rest[i..];
        if let Some(t) = tail.strip_prefix("{{") {
            out.push('{');
            rest = t;
            continue;
        }
        if let Some(t) = tail.strip_prefix("}}").or_else(|| tail.strip_prefix('}')) {
            out.push('}');
            rest = t;
            continue;
        }
        let Some(end) = tail.find('}') else {
            out.push_str(tail);
            rest = "";
            break;
        };
        let expr = &tail[1..end];
        match eval(expr, ctx) {
            Some(v) => out.push_str(&v),
            None => match mode {
                Mode::Strict => {
                    return Err(KeltaError::invalid(format!(
                        "unknown placeholder {{{expr}}} in \"{template}\""
                    )));
                }
                Mode::Lenient => out.push_str(&tail[..=end]),
            },
        }
        rest = &tail[end + 1..];
    }
    out.push_str(rest);
    Ok(out)
}

/// Evaluate one `{…}` expression; `None` = unknown placeholder.
fn eval(expr: &str, ctx: &Ctx) -> Option<String> {
    let mut parts = expr.split('|').map(str::trim);
    let first = parts.next()?;
    if first.is_empty() || !first.chars().all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '_') {
        return None;
    }
    let mut known = ctx.get(first).is_some();
    let mut value = ctx.get(first).unwrap_or_default().to_owned();
    for p in parts {
        if FILTERS.contains(&p) {
            value = match p {
                "slug" => slugify(&value, 40),
                "shell" => shell_quote(&value),
                _ => serde_json::Value::String(value).to_string(),
            };
        } else if let Some(alt) = ctx.get(p) {
            known = true;
            if value.is_empty() {
                value = alt.to_owned();
            }
        }
    }
    known.then_some(value)
}

/// POSIX single-quote a string (`'` → `'\''`).
pub fn shell_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

/// Split a command line into words (shell-words subset: single/double quotes, backslash escapes).
pub fn shell_words(line: &str) -> Result<Vec<String>, KeltaError> {
    let mut words = Vec::new();
    let mut cur = String::new();
    let mut in_word = false;
    let mut chars = line.chars();
    while let Some(c) = chars.next() {
        match c {
            '\'' => {
                in_word = true;
                loop {
                    match chars.next() {
                        Some('\'') => break,
                        Some(x) => cur.push(x),
                        None => return Err(KeltaError::invalid(format!("unterminated quote in `{line}`"))),
                    }
                }
            }
            '"' => {
                in_word = true;
                loop {
                    match chars.next() {
                        Some('"') => break,
                        Some('\\') => match chars.next() {
                            Some(x @ ('"' | '\\' | '$' | '`')) => cur.push(x),
                            Some('\n') => {}
                            Some(x) => {
                                cur.push('\\');
                                cur.push(x);
                            }
                            None => {
                                return Err(KeltaError::invalid(format!("unterminated quote in `{line}`")));
                            }
                        },
                        Some(x) => cur.push(x),
                        None => return Err(KeltaError::invalid(format!("unterminated quote in `{line}`"))),
                    }
                }
            }
            '\\' => {
                in_word = true;
                match chars.next() {
                    Some('\n') | None => {}
                    Some(x) => cur.push(x),
                }
            }
            c if c.is_whitespace() => {
                if in_word {
                    words.push(std::mem::take(&mut cur));
                    in_word = false;
                }
            }
            c => {
                in_word = true;
                cur.push(c);
            }
        }
    }
    if in_word {
        words.push(cur);
    }
    Ok(words)
}

/// ASCII transliteration of common Latin letters with diacritics.
fn translit(c: char) -> Option<&'static str> {
    Some(match c {
        'à' | 'á' | 'â' | 'ã' | 'ä' | 'å' | 'ā' | 'ă' | 'ą' => "a",
        'æ' => "ae",
        'ç' | 'ć' | 'ĉ' | 'ċ' | 'č' => "c",
        'ď' | 'đ' | 'ð' => "d",
        'è' | 'é' | 'ê' | 'ë' | 'ē' | 'ĕ' | 'ė' | 'ę' | 'ě' => "e",
        'ĝ' | 'ğ' | 'ġ' | 'ģ' => "g",
        'ĥ' | 'ħ' => "h",
        'ì' | 'í' | 'î' | 'ï' | 'ĩ' | 'ī' | 'ĭ' | 'į' | 'ı' => "i",
        'ĵ' => "j",
        'ķ' => "k",
        'ĺ' | 'ļ' | 'ľ' | 'ŀ' | 'ł' => "l",
        'ñ' | 'ń' | 'ņ' | 'ň' => "n",
        'ò' | 'ó' | 'ô' | 'õ' | 'ö' | 'ø' | 'ō' | 'ŏ' | 'ő' => "o",
        'œ' => "oe",
        'ŕ' | 'ŗ' | 'ř' => "r",
        'ś' | 'ŝ' | 'ş' | 'š' | 'ș' => "s",
        'ß' => "ss",
        'ţ' | 'ť' | 'ŧ' | 'ț' => "t",
        'þ' => "th",
        'ù' | 'ú' | 'û' | 'ü' | 'ũ' | 'ū' | 'ŭ' | 'ů' | 'ű' | 'ų' => "u",
        'ŵ' => "w",
        'ý' | 'ÿ' | 'ŷ' => "y",
        'ź' | 'ż' | 'ž' => "z",
        _ => return None,
    })
}

/// `[a-z0-9-]` slug, cut at a word boundary to at most `max` chars.
pub fn slugify(text: &str, max: usize) -> String {
    let mut out = String::with_capacity(text.len());
    let mut dash = false;
    for c in text.chars().flat_map(char::to_lowercase) {
        let piece: Option<std::borrow::Cow<'static, str>> =
            if c.is_ascii_alphanumeric() { Some(c.to_string().into()) } else { translit(c).map(Into::into) };
        match piece {
            Some(p) => {
                if dash && !out.is_empty() {
                    out.push('-');
                }
                dash = false;
                out.push_str(&p);
            }
            None => dash = true,
        }
    }
    if max == 0 || out.len() <= max {
        return out;
    }
    // ASCII only from here: byte slicing is safe.
    let cut = &out[..max];
    if out.as_bytes().get(max) == Some(&b'-') {
        return cut.trim_end_matches('-').to_owned();
    }
    match cut.rfind('-') {
        Some(i) if i > 0 => cut[..i].to_owned(),
        _ => cut.to_owned(),
    }
}

/// Expand a leading `~` / `~/` with `home`.
pub fn expand_home(path: &str, home: Option<&std::path::Path>) -> std::path::PathBuf {
    match (path, home) {
        ("~", Some(h)) => h.to_path_buf(),
        (p, Some(h)) if p.starts_with("~/") => h.join(&p[2..]),
        (p, _) => std::path::PathBuf::from(p),
    }
}

/// Trim `-`/`_`/`.`-only leftovers from each path component (empty placeholders like `{key}-{slug}`).
pub fn tidy_path(p: &std::path::Path) -> std::path::PathBuf {
    let mut out = std::path::PathBuf::new();
    for c in p.components() {
        match c {
            std::path::Component::Normal(s) => {
                let s = s.to_string_lossy();
                let t = s.trim_matches(|ch| ch == '-' || ch == '_');
                if !t.is_empty() {
                    out.push(t);
                }
            }
            other => out.push(other.as_os_str()),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn render_basic_filters_fallback() {
        let ctx = Ctx::new()
            .with("ticket.key", "SHOP-1")
            .with("ticket.title", "Hello World")
            .with("worktree", "")
            .with("project.root", "/p");
        assert_eq!(
            render("{ticket.key}: {ticket.title}", &ctx, Mode::Strict).unwrap(),
            "SHOP-1: Hello World"
        );
        assert_eq!(render("{ticket.title|slug}", &ctx, Mode::Strict).unwrap(), "hello-world");
        assert_eq!(render("{worktree|project.root}", &ctx, Mode::Strict).unwrap(), "/p");
        assert_eq!(render("{ticket.title|shell}", &ctx, Mode::Strict).unwrap(), "'Hello World'");
        assert_eq!(render("{ticket.title|json}", &ctx, Mode::Strict).unwrap(), "\"Hello World\"");
        assert_eq!(render("a {{b}} c", &ctx, Mode::Strict).unwrap(), "a {b} c");
        assert!(render("{nope}", &ctx, Mode::Strict).is_err());
        assert_eq!(render("x {run}/t.md", &ctx, Mode::Lenient).unwrap(), "x {run}/t.md");
    }

    #[test]
    fn slug_unicode_and_cut() {
        assert_eq!(slugify("Rate-limit login", 40), "rate-limit-login");
        assert_eq!(slugify("Café crème — Über naïve façade", 40), "cafe-creme-uber-naive-facade");
        assert_eq!(slugify("Straße  &  Œuvre!!", 40), "strasse-oeuvre");
        assert_eq!(slugify("日本語 title", 40), "title");
        let long = "Implement the very long ticket title that goes beyond forty characters";
        let s = slugify(long, 40);
        assert!(s.len() <= 40, "{s}");
        assert_eq!(s, "implement-the-very-long-ticket-title");
        assert_eq!(slugify("abcdefghij", 4), "abcd");
    }

    #[test]
    fn words() {
        assert_eq!(
            shell_words(r#"pnpm install --frozen-lockfile"#).unwrap(),
            vec!["pnpm", "install", "--frozen-lockfile"]
        );
        assert_eq!(
            shell_words(r#"sh -c 'echo "a b"' x\ y"#).unwrap(),
            vec!["sh", "-c", "echo \"a b\"", "x y"]
        );
        assert!(shell_words("'oops").is_err());
        assert_eq!(shell_quote("it's"), "'it'\\''s'");
    }

    #[test]
    fn tidy() {
        assert_eq!(
            tidy_path(std::path::Path::new("/w/shop/api/-my-branch")),
            std::path::PathBuf::from("/w/shop/api/my-branch")
        );
    }
}

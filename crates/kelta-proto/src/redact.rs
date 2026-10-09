//! Log redaction helpers (ARCHITECTURE §11.1): tokens, secrets and tokenized URLs are never logged.
//!
//! Use `tracing::info!(url = %redact_url(&u), token = %Redacted(&t))`.

use std::fmt;

/// Wrapper whose `Display`/`Debug` never reveal the inner value.
pub struct Redacted<T>(pub T);

impl<T> fmt::Display for Redacted<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("[redacted]")
    }
}

impl<T> fmt::Debug for Redacted<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("[redacted]")
    }
}

/// Strip userinfo, query string and fragment from a URL (`https://u:p@h/x?token=1` → `https://h/x?[redacted]`).
pub fn redact_url(url: &str) -> String {
    let (scheme, rest) = match url.split_once("://") {
        Some((s, r)) => (Some(s), r),
        None => (None, url),
    };
    let (authority, path) = match rest.find('/') {
        Some(i) => (&rest[..i], &rest[i..]),
        None => (rest, ""),
    };
    let authority = authority.rsplit_once('@').map_or(authority, |(_, h)| h);
    let (path, had_query) = match path.find(['?', '#']) {
        Some(i) => (&path[..i], true),
        None => (path, false),
    };
    let mut out = String::with_capacity(url.len());
    if let Some(s) = scheme {
        out.push_str(s);
        out.push_str("://");
    }
    out.push_str(authority);
    out.push_str(path);
    if had_query {
        out.push_str("?[redacted]");
    }
    out
}

/// Known token prefixes (GitHub, GitLab, Slack, Atlassian, generic JWT).
const TOKEN_PREFIXES: &[&str] =
    &["ghp_", "gho_", "ghs_", "ghu_", "github_pat_", "glpat-", "xoxb-", "xoxp-", "ATATT", "eyJ"];

/// Mask secrets inside free text: `Bearer <tok>`, `token=<tok>`, `Authorization: <x>`,
/// `PRIVATE-TOKEN: <x>`, and words starting with a known token prefix.
pub fn redact_text(text: &str) -> String {
    let mut out: Vec<String> = Vec::new();
    let mut mask_next = false;
    for word in text.split(' ') {
        let lower = word.to_ascii_lowercase();
        if mask_next && !word.is_empty() && lower != "bearer" && lower != "basic" {
            out.push("[redacted]".into());
            mask_next = false;
            continue;
        }
        if lower == "bearer"
            || lower == "basic"
            || lower == "token"
            || lower == "authorization:"
            || lower == "private-token:"
            || lower == "x-redmine-api-key:"
        {
            mask_next = true;
            out.push(word.to_owned());
            continue;
        }
        if let Some((k, _)) = word.split_once('=') {
            let kl = k.to_ascii_lowercase();
            if ["token", "access_token", "api_key", "apikey", "password", "secret", "key"]
                .iter()
                .any(|s| kl.ends_with(s))
            {
                out.push(format!("{k}=[redacted]"));
                continue;
            }
        }
        if TOKEN_PREFIXES.iter().any(|p| word.starts_with(p)) && word.len() > 12 {
            out.push("[redacted]".into());
            continue;
        }
        out.push(word.to_owned());
    }
    out.join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn urls() {
        assert_eq!(redact_url("http://127.0.0.1:3011/?token=abc"), "http://127.0.0.1:3011/?[redacted]");
        assert_eq!(redact_url("https://u:p@h.io/a/b"), "https://h.io/a/b");
        assert_eq!(redact_url("https://h.io"), "https://h.io");
    }

    #[test]
    fn text() {
        let t = redact_text("Authorization: Bearer abcdef token=xyz ghp_0123456789abcdef ok");
        assert!(!t.contains("abcdef"));
        assert!(!t.contains("xyz"));
        assert!(t.ends_with("ok"));
        assert_eq!(format!("{}", Redacted("s3cret")), "[redacted]");
    }
}

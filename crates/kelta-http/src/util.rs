//! Small helpers shared by the provider crates: percent-encoding, `Link` header paging, host
//! extraction, HTML escaping.

use std::collections::BTreeMap;

/// Percent-encode a URL path segment or query value (RFC 3986 unreserved characters stay as is).
/// `grp/sub/proj` → `grp%2Fsub%2Fproj` (GitLab project paths).
pub fn percent_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 8);
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => out.push(b as char),
            _ => {
                out.push('%');
                out.push(char::from(b"0123456789ABCDEF"[(b >> 4) as usize]));
                out.push(char::from(b"0123456789ABCDEF"[(b & 0x0f) as usize]));
            }
        }
    }
    out
}

/// The URL a `Link: <url>; rel="<rel>"` header points to for `rel`.
pub fn link_rel(headers: &BTreeMap<String, String>, rel: &str) -> Option<String> {
    let link = headers.get("link")?;
    for part in link.split(',') {
        let mut it = part.split(';');
        let url = it.next()?.trim();
        let url = url.strip_prefix('<')?.strip_suffix('>')?;
        if it.any(|p| {
            let p = p.trim();
            p.strip_prefix("rel=").map(|v| v.trim_matches('"') == rel).unwrap_or(false)
        }) {
            return Some(url.to_owned());
        }
    }
    None
}

/// Value of a query parameter of `url` (`page` of a `Link` URL).
pub fn query_param(url: &str, key: &str) -> Option<String> {
    let u = reqwest::Url::parse(url).ok()?;
    u.query_pairs().find(|(k, _)| k == key).map(|(_, v)| v.into_owned())
}

/// Lower-cased host of a URL (no port); also accepts scheme-less `host/path`.
pub fn url_host(url: &str) -> Option<String> {
    let parsed = reqwest::Url::parse(url).or_else(|_| reqwest::Url::parse(&format!("https://{url}"))).ok()?;
    parsed.host_str().map(str::to_ascii_lowercase)
}

/// `https://host/a/b/` → `https://host/a/b`.
pub fn trim_url(url: &str) -> String {
    url.trim().trim_end_matches('/').to_owned()
}

/// Escape `&`, `<`, `>`, `"` and `'`.
pub fn escape_html(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 8);
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            _ => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encodes_project_paths() {
        assert_eq!(percent_encode("grp/sub/proj"), "grp%2Fsub%2Fproj");
        assert_eq!(percent_encode("a b&c"), "a%20b%26c");
        assert_eq!(percent_encode("é"), "%C3%A9");
        assert_eq!(percent_encode("ok-1.2_~"), "ok-1.2_~");
    }

    #[test]
    fn link_headers() {
        let mut h = BTreeMap::new();
        h.insert(
            "link".to_owned(),
            "<https://api.github.com/x?page=2>; rel=\"next\", <https://api.github.com/x?page=9>; rel=\"last\""
                .to_owned(),
        );
        assert_eq!(link_rel(&h, "next").as_deref(), Some("https://api.github.com/x?page=2"));
        assert_eq!(query_param(&link_rel(&h, "last").unwrap(), "page").as_deref(), Some("9"));
        assert_eq!(link_rel(&h, "prev"), None);
    }

    #[test]
    fn hosts() {
        assert_eq!(url_host("https://GitLab.Acme.example:8443/x").as_deref(), Some("gitlab.acme.example"));
        assert_eq!(url_host("github.com").as_deref(), Some("github.com"));
    }
}

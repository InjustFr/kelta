//! Markdown → sanitized HTML (ARCHITECTURE D12, §11.2): `pulldown-cmark` + `ammonia`.
//! No scripts, styles or iframes; links get `rel="noopener noreferrer"` (the UI opens them via
//! `open_external`).

use pulldown_cmark::{Options, Parser, html};

/// Render Markdown (CommonMark + tables, strikethrough, task lists) to sanitized HTML.
pub fn to_html(md: &str) -> String {
    let mut opts = Options::empty();
    opts.insert(Options::ENABLE_TABLES);
    opts.insert(Options::ENABLE_STRIKETHROUGH);
    opts.insert(Options::ENABLE_TASKLISTS);
    let parser = Parser::new_ext(md, opts);
    let mut out = String::with_capacity(md.len() * 3 / 2);
    html::push_html(&mut out, parser);
    sanitize(&out)
}

/// Plain text (Textile, Jira wiki, anything not rendered) shown preformatted: escaped inside `<pre>`.
pub fn plain_to_html(text: &str) -> String {
    sanitize(&format!("<pre>{}</pre>", crate::util::escape_html(text)))
}

/// Sanitize provider HTML (Jira rendered fields, GitLab/GitHub HTML bodies).
pub fn sanitize(html: &str) -> String {
    ammonia::Builder::default()
        .link_rel(Some("noopener noreferrer"))
        .url_schemes(["http", "https", "mailto"].into_iter().collect())
        .clean(html)
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_and_sanitizes() {
        let h = to_html("# Hi\n\n**bold** [x](https://e.com) <script>alert(1)</script>");
        assert!(h.contains("<h1>Hi</h1>"));
        assert!(h.contains("<strong>bold</strong>"));
        assert!(h.contains("rel=\"noopener noreferrer\""));
        assert!(!h.contains("<script"));
        let s = sanitize("<iframe src=x></iframe><a href=\"javascript:alert(1)\">a</a><style>x</style>");
        assert!(!s.contains("iframe"));
        assert!(!s.contains("javascript"));
        assert!(!s.contains("<style"));
    }

    #[test]
    fn plain_text_is_preformatted_and_escaped() {
        let h = plain_to_html("h1. Title <script>x</script> & more");
        assert!(h.starts_with("<pre>"));
        assert!(h.contains("&lt;script&gt;"));
        assert!(!h.contains("<script"));
    }
}

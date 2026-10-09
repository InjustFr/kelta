//! `kelta-plugin://<plugin-id>/<path>` scheme handler (ARCHITECTURE §11.3): serves only
//! canonicalized files inside an enabled plugin's directory (no `..`, no encoded traversal, no
//! symlink escape), with a MIME type and the per-plugin CSP on **every** response.

use std::path::{Component, Path, PathBuf};

use axum::http::{HeaderValue, Method, Request, Response, StatusCode, header};

use crate::PluginHost;

/// Content-Security-Policy for plugin screens (ARCHITECTURE §11.3).
pub fn csp(plugin_id: &str) -> String {
    format!(
        "default-src 'self' kelta-plugin://{plugin_id}; script-src kelta-plugin://{plugin_id}; \
         style-src kelta-plugin://{plugin_id} 'unsafe-inline'; img-src kelta-plugin://{plugin_id} data:; \
         connect-src 'none'; frame-ancestors 'self'"
    )
}

/// Percent-decode; `None` on malformed escapes or a decoded NUL.
fn percent_decode(s: &str) -> Option<String> {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            let hex = s.get(i + 1..i + 3)?;
            let b = u8::from_str_radix(hex, 16).ok()?;
            out.push(b);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    if out.contains(&0) {
        return None;
    }
    String::from_utf8(out).ok()
}

/// `(plugin id, relative path)` from the request URI. Handles `kelta-plugin://id/path` and the
/// `http://kelta-plugin.localhost/id/path` form some webviews use for custom schemes.
pub fn split_uri(uri: &axum::http::Uri) -> Option<(String, String)> {
    let host = uri.host().unwrap_or("");
    let path = uri.path();
    if host.is_empty() || host == "kelta-plugin.localhost" || host == "localhost" {
        let p = path.trim_start_matches('/');
        let (id, rest) = p.split_once('/').unwrap_or((p, ""));
        return Some((id.to_owned(), rest.to_owned()));
    }
    Some((host.to_owned(), path.trim_start_matches('/').to_owned()))
}

/// Resolve a request path inside `root` (canonical). `None` = refused.
pub fn resolve_path(root: &Path, raw: &str) -> Option<PathBuf> {
    let decoded = percent_decode(raw)?;
    if decoded.contains('\\') {
        return None;
    }
    let rel = Path::new(&decoded);
    if rel.is_absolute() {
        return None;
    }
    if !rel.components().all(|c| matches!(c, Component::Normal(_) | Component::CurDir)) {
        return None;
    }
    let mut target = root.join(rel);
    if decoded.is_empty() || decoded.ends_with('/') {
        target = target.join("index.html");
    }
    let canonical = target.canonicalize().ok()?;
    (canonical.starts_with(root) && canonical.is_file()).then_some(canonical)
}

fn mime_for(path: &Path) -> String {
    let m = mime_guess::from_path(path).first_or_octet_stream();
    let s = m.essence_str().to_owned();
    match s.as_str() {
        "application/javascript" | "text/javascript" => "text/javascript; charset=utf-8".into(),
        "text/html" | "text/css" | "text/plain" | "application/json" | "image/svg+xml" => {
            format!("{s}; charset=utf-8")
        }
        _ => s,
    }
}

fn respond(plugin_id: &str, status: StatusCode, body: Vec<u8>, content_type: &str) -> Response<Vec<u8>> {
    let mut resp = Response::new(body);
    *resp.status_mut() = status;
    let h = resp.headers_mut();
    let safe_id: String = plugin_id.chars().filter(|c| c.is_ascii_alphanumeric() || *c == '-').collect();
    if let Ok(v) = HeaderValue::from_str(&csp(&safe_id)) {
        h.insert(header::CONTENT_SECURITY_POLICY, v);
    }
    if let Ok(v) = HeaderValue::from_str(content_type) {
        h.insert(header::CONTENT_TYPE, v);
    }
    h.insert(header::X_CONTENT_TYPE_OPTIONS, HeaderValue::from_static("nosniff"));
    h.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-cache"));
    // Screens run in an opaque-origin sandbox: module scripts from their own scheme need CORS.
    h.insert(header::ACCESS_CONTROL_ALLOW_ORIGIN, HeaderValue::from_static("*"));
    h.insert(header::REFERRER_POLICY, HeaderValue::from_static("no-referrer"));
    resp
}

fn text(plugin_id: &str, status: StatusCode, msg: &str) -> Response<Vec<u8>> {
    respond(plugin_id, status, msg.as_bytes().to_vec(), "text/plain; charset=utf-8")
}

/// Serve a request.
pub fn handle(host: &PluginHost, req: Request<Vec<u8>>) -> Response<Vec<u8>> {
    let Some((id, rel)) = split_uri(req.uri()) else {
        return text("", StatusCode::BAD_REQUEST, "bad request");
    };
    if req.method() != Method::GET && req.method() != Method::HEAD {
        return text(&id, StatusCode::METHOD_NOT_ALLOWED, "method not allowed");
    }
    if !kelta_proto::ids::PluginId::is_valid(&id) {
        return text(&id, StatusCode::NOT_FOUND, "not found");
    }
    let reg = host.registry();
    let Some(entry) = reg.get(&id).filter(|e| e.loadable() && host.is_enabled(&e.id)) else {
        return text(&id, StatusCode::NOT_FOUND, "not found");
    };
    let Some(path) = resolve_path(&entry.dir, &rel) else {
        return text(&id, StatusCode::NOT_FOUND, "not found");
    };
    match std::fs::read(&path) {
        Ok(bytes) => {
            let ct = mime_for(&path);
            let body = if req.method() == Method::HEAD { Vec::new() } else { bytes };
            respond(&id, StatusCode::OK, body, &ct)
        }
        Err(_) => text(&id, StatusCode::NOT_FOUND, "not found"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decode_rejects_nul_and_bad_escapes() {
        assert_eq!(percent_decode("a%20b").as_deref(), Some("a b"));
        assert!(percent_decode("a%2").is_none());
        assert!(percent_decode("a%00b").is_none());
    }

    #[test]
    fn uri_forms() {
        let u: axum::http::Uri = "kelta-plugin://hello-screen/dist/index.html?instance=x".parse().unwrap();
        assert_eq!(split_uri(&u), Some(("hello-screen".into(), "dist/index.html".into())));
        let u: axum::http::Uri = "http://kelta-plugin.localhost/hello-screen/a.js".parse().unwrap();
        assert_eq!(split_uri(&u), Some(("hello-screen".into(), "a.js".into())));
    }

    #[test]
    fn csp_shape() {
        let c = csp("x-y");
        assert!(c.contains("connect-src 'none'"));
        assert!(c.contains("script-src kelta-plugin://x-y"));
    }
}

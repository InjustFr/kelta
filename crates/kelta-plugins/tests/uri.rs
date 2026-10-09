//! `kelta-plugin://` handler: never serves outside the plugin dir (`../`, encoded, symlinks), always
//! sends the per-plugin CSP, serves MIME types, refuses disabled/unknown plugins.

mod common;

use axum::http::{Method, Request, StatusCode, header};
use kelta_plugins::uri;
use kelta_proto::ids::PluginId;

fn get(env: &common::Env, url: &str) -> axum::http::Response<Vec<u8>> {
    let req = Request::builder().method(Method::GET).uri(url).body(Vec::new()).unwrap();
    uri::handle(&env.host, req)
}

fn setup() -> common::Env {
    let env = common::Env::new();
    let screen = "\n[[contributes.screens]]\nid = \"main\"\ntitle = \"Main\"\nentry = \"dist/index.html\"\n";
    let dir = env.write_plugin(
        "web-ui",
        &format!("{}{screen}", common::manifest("web-ui", &[], "")),
        &[("dist/index.html", "<p>hi</p>"), ("dist/app.js", "export {}"), ("dist/a b.css", "p{}")],
    );
    std::fs::write(env.tmp.path().join("secret.txt"), "TOP SECRET").unwrap();
    std::fs::write(env.plugins_dir().join("other-secret.txt"), "OTHER").unwrap();
    std::os::unix::fs::symlink(env.tmp.path().join("secret.txt"), dir.join("dist/link.txt")).unwrap();
    std::os::unix::fs::symlink(dir.join("dist/app.js"), dir.join("dist/inner-link.js")).unwrap();
    env.host.refresh();
    env
}

fn assert_csp(resp: &axum::http::Response<Vec<u8>>) {
    let csp = resp.headers().get(header::CONTENT_SECURITY_POLICY).expect("CSP header").to_str().unwrap();
    assert_eq!(csp, uri::csp("web-ui"));
    assert!(csp.contains("connect-src 'none'"));
}

#[test]
fn serves_files_with_mime_and_csp() {
    let env = setup();
    let r = get(&env, "kelta-plugin://web-ui/dist/index.html?instance=abc");
    assert_eq!(r.status(), StatusCode::OK);
    assert_eq!(r.body(), b"<p>hi</p>");
    assert_eq!(r.headers()[header::CONTENT_TYPE], "text/html; charset=utf-8");
    assert_csp(&r);
    let r = get(&env, "kelta-plugin://web-ui/dist/app.js");
    assert_eq!(r.headers()[header::CONTENT_TYPE], "text/javascript; charset=utf-8");
    assert_eq!(r.headers()[header::X_CONTENT_TYPE_OPTIONS], "nosniff");
    let r = get(&env, "kelta-plugin://web-ui/dist/a%20b.css");
    assert_eq!(r.status(), StatusCode::OK);
    assert!(r.headers()[header::CONTENT_TYPE].to_str().unwrap().starts_with("text/css"));
    let r = get(&env, "kelta-plugin://web-ui/dist/inner-link.js");
    assert_eq!(r.status(), StatusCode::OK, "symlinks inside the plugin are fine");
    let r = get(&env, "http://kelta-plugin.localhost/web-ui/dist/index.html");
    assert_eq!(r.status(), StatusCode::OK);
}

#[test]
fn traversal_is_refused_and_still_carries_csp() {
    let env = setup();
    for url in [
        "kelta-plugin://web-ui/../secret.txt",
        "kelta-plugin://web-ui/dist/../../secret.txt",
        "kelta-plugin://web-ui/%2e%2e/secret.txt",
        "kelta-plugin://web-ui/%2E%2E%2Fother-secret.txt",
        "kelta-plugin://web-ui/dist%2f..%2f..%2fother-secret.txt",
        "kelta-plugin://web-ui/..%5cother-secret.txt",
        "kelta-plugin://web-ui/dist/link.txt",
        "kelta-plugin://web-ui/%2Fetc%2Fpasswd",
        "kelta-plugin://web-ui/dist/index.html%00.txt",
        "kelta-plugin://web-ui/kelta-plugin.toml/../../other-secret.txt",
    ] {
        let r = get(&env, url);
        assert_eq!(r.status(), StatusCode::NOT_FOUND, "{url}");
        assert!(!String::from_utf8_lossy(r.body()).contains("SECRET"), "{url}");
        assert!(!String::from_utf8_lossy(r.body()).contains("OTHER"), "{url}");
        assert_csp(&r);
    }
}

#[test]
fn unknown_or_disabled_plugins_are_not_served() {
    let env = setup();
    assert_eq!(get(&env, "kelta-plugin://nope-x/dist/index.html").status(), StatusCode::NOT_FOUND);
    let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
    rt.block_on(env.host.enable(&PluginId::new("web-ui"), false)).unwrap();
    let r = get(&env, "kelta-plugin://web-ui/dist/index.html");
    assert_eq!(r.status(), StatusCode::NOT_FOUND);
    assert_csp(&r);
    let post = Request::builder()
        .method(Method::POST)
        .uri("kelta-plugin://web-ui/dist/index.html")
        .body(Vec::new())
        .unwrap();
    assert_eq!(uri::handle(&env.host, post).status(), StatusCode::METHOD_NOT_ALLOWED);
}

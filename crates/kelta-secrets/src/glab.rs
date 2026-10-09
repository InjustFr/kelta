//! Minimal reader for glab's `config.yml` (only `hosts.<host>.token` is needed).

use std::path::PathBuf;

fn unquote(v: &str) -> &str {
    let v = v.trim();
    for q in ['"', '\''] {
        if let Some(inner) = v.strip_prefix(q).and_then(|x| x.strip_suffix(q)) {
            return inner;
        }
    }
    v
}

/// Token of `host` in a glab config, if stored in plain text.
pub fn token_from_yaml(text: &str, host: &str) -> Option<String> {
    let mut in_hosts = false;
    let mut host_indent: Option<usize> = None;
    let mut current: Option<String> = None;
    for raw in text.lines() {
        let line = raw.trim_end();
        let t = line.trim_start();
        if t.is_empty() || t.starts_with('#') {
            continue;
        }
        let indent = line.len() - t.len();
        if indent == 0 {
            in_hosts = t == "hosts:";
            host_indent = None;
            current = None;
            continue;
        }
        if !in_hosts {
            continue;
        }
        let hi = *host_indent.get_or_insert(indent);
        if indent <= hi {
            current = t.strip_suffix(':').map(|h| unquote(h).to_owned());
            continue;
        }
        if current.as_deref() == Some(host)
            && let Some(rest) = t.strip_prefix("token:")
        {
            let v = unquote(rest.trim());
            if v.is_empty() || v.starts_with("!!null") || v == "null" || v == "~" {
                return None;
            }
            // `!!str value`, `!!binary value`
            let v = if v.starts_with("!!") { v.split_once(' ').map_or("", |(_, x)| x) } else { v };
            let v = unquote(v);
            return (!v.is_empty()).then(|| v.to_owned());
        }
    }
    None
}

/// Candidate config files in glab's search order.
pub fn config_paths(env: &dyn Fn(&str) -> Option<String>) -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Some(d) = env("GLAB_CONFIG_DIR").filter(|d| !d.is_empty()) {
        out.push(PathBuf::from(d).join("config.yml"));
    }
    if let Some(d) = env("XDG_CONFIG_HOME").filter(|d| !d.is_empty()) {
        out.push(PathBuf::from(d).join("glab-cli").join("config.yml"));
    }
    if let Some(h) = env("HOME").filter(|d| !d.is_empty()) {
        out.push(PathBuf::from(h).join(".config").join("glab-cli").join("config.yml"));
    }
    out
}

/// Pull the token out of `glab auth status --show-token` output (`… Token: glpat-…` / `Token found: …`).
pub fn token_from_status(output: &str) -> Option<String> {
    for line in output.lines() {
        let l = line.trim();
        if let Some(i) = l.find("Token") {
            let after = &l[i..];
            if let Some((_, v)) = after.split_once(':') {
                let v = v.trim();
                if !v.is_empty() && !v.chars().all(|c| c == '*') && !v.contains(' ') {
                    return Some(v.to_owned());
                }
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_and_tagged_tokens() {
        let yml = "git_protocol: ssh\nhosts:\n    gitlab.com:\n        token: glpat-aaa\n        api_host: gitlab.com\n    gitlab.acme.example:\n        token: \"glpat-bbb\"\n        user: me\n    gitlab.nulltoken.io:\n        token: !!null \n        is_oauth2: \"true\"\n";
        assert_eq!(token_from_yaml(yml, "gitlab.com").as_deref(), Some("glpat-aaa"));
        assert_eq!(token_from_yaml(yml, "gitlab.acme.example").as_deref(), Some("glpat-bbb"));
        assert_eq!(token_from_yaml(yml, "gitlab.nulltoken.io"), None);
        assert_eq!(token_from_yaml(yml, "other"), None);
    }

    #[test]
    fn status_output() {
        assert_eq!(
            token_from_status("gitlab.com\n  ✓ Logged in to gitlab.com as me\n  ✓ Token found: glpat-zzz\n")
                .as_deref(),
            Some("glpat-zzz")
        );
        assert_eq!(token_from_status("  ✓ Token: glpat-yyy").as_deref(), Some("glpat-yyy"));
        assert_eq!(token_from_status("  ✓ Token: **************"), None);
        assert_eq!(token_from_status("nothing here"), None);
    }
}

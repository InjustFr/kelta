//! Helpers shared by the code hosts: remote URL parsing, linked ticket keys.

use std::sync::OnceLock;

use kelta_proto::error::KeltaError;
use kelta_proto::settings::ReviewsSettings;
use kelta_proto::tracker::User;
use parking_lot::RwLock;
use regex::Regex;
use serde_json::Value;

static TICKET_RE: RwLock<Option<Regex>> = RwLock::new(None);

fn default_ticket_regex() -> Option<&'static Regex> {
    static DEFAULT: OnceLock<Option<Regex>> = OnceLock::new();
    DEFAULT.get_or_init(|| Regex::new(&ReviewsSettings::default().ticket_key_regex).ok()).as_ref()
}

/// Process-wide `reviews.ticket_key_regex` (the `ProviderFactory` signature carries no settings;
/// core calls this at startup and on settings reload). Invalid patterns are rejected and the
/// previous one stays in effect.
pub fn set_ticket_key_regex(pattern: &str) -> Result<(), KeltaError> {
    let re =
        Regex::new(pattern).map_err(|e| KeltaError::invalid(format!("reviews.ticket_key_regex: {e}")))?;
    *TICKET_RE.write() = Some(re);
    Ok(())
}

/// Ticket keys found in `parts` (branch, title, ...), in order of appearance, without duplicates.
pub fn linked_tickets(parts: &[&str]) -> Vec<String> {
    let guard = TICKET_RE.read();
    let re: &Regex = match guard.as_ref() {
        Some(r) => r,
        None => match default_ticket_regex() {
            Some(r) => r,
            None => return Vec::new(),
        },
    };
    let mut out: Vec<String> = Vec::new();
    for p in parts {
        for m in re.find_iter(p) {
            let k = m.as_str().to_owned();
            if !out.contains(&k) {
                out.push(k);
            }
        }
    }
    out
}

/// `(host, path)` of a git remote: https/http/git/ssh URLs and scp-like `git@host:path`.
/// The host is lower-cased and has no port or userinfo; the path has no leading `/`, no
/// trailing `/` and no `.git` suffix.
pub fn parse_remote(url: &str) -> Option<(String, String)> {
    let url = url.trim();
    let (authority, path) = if let Some((_, rest)) = url.split_once("://") {
        let (auth, path) = rest.split_once('/')?;
        (auth.to_owned(), path.to_owned())
    } else {
        // scp-like: [user@]host:path (a `/` before the first `:` means a local path)
        let (auth, path) = url.split_once(':')?;
        if auth.contains('/') || auth.is_empty() {
            return None;
        }
        (auth.to_owned(), path.to_owned())
    };
    let host = authority.rsplit_once('@').map_or(authority.as_str(), |(_, h)| h);
    let host = host.split(':').next().unwrap_or(host).to_ascii_lowercase();
    if host.is_empty() {
        return None;
    }
    let path = path.split(['?', '#']).next().unwrap_or("").trim_matches('/');
    let path = path.strip_suffix(".git").unwrap_or(path).trim_end_matches('/');
    (!path.is_empty()).then(|| (host, path.to_owned()))
}

/// `remote` host equals the account host (`ssh.<host>` is GitHub's ssh-over-443 alias).
pub fn host_matches(remote_host: &str, account_host: &str) -> bool {
    remote_host == account_host || remote_host.strip_prefix("ssh.") == Some(account_host)
}

/// `owner/name` as an encoded URL path (`owner/name`): exactly two plain segments, never `.`/`..`.
/// Repo strings come from the UI and are put into URLs.
pub fn repo_path(repo: &str) -> Result<String, KeltaError> {
    match repo.split_once('/') {
        Some((a, b))
            if ![a, b].iter().any(|p| p.is_empty() || *p == "." || *p == ".." || p.contains('/')) =>
        {
            Ok(format!("{}/{}", kelta_http::util::percent_encode(a), kelta_http::util::percent_encode(b)))
        }
        _ => Err(KeltaError::invalid(format!("bad repo: {repo}"))),
    }
}

pub fn s<'a>(v: &'a Value, key: &str) -> Option<&'a str> {
    v.get(key).and_then(Value::as_str)
}

pub fn user(login: &str, name: Option<&str>, avatar: Option<&str>) -> User {
    User {
        id: login.to_owned(),
        name: name.filter(|n| !n.is_empty()).unwrap_or(login).to_owned(),
        login: Some(login.to_owned()),
        avatar_url: avatar.map(str::to_owned),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn remote_forms() {
        let cases = [
            ("https://github.com/acme/shop.git", ("github.com", "acme/shop")),
            ("https://user:tok@GitHub.com/acme/shop", ("github.com", "acme/shop")),
            ("git@github.com:acme/shop.git", ("github.com", "acme/shop")),
            ("ssh://git@ghe.acme.example:2222/acme/shop.git", ("ghe.acme.example", "acme/shop")),
            ("git@gitlab.acme.example:grp/sub/proj.git", ("gitlab.acme.example", "grp/sub/proj")),
            ("https://gitlab.acme.example/grp/sub/proj/", ("gitlab.acme.example", "grp/sub/proj")),
            ("git://host.test/a/b.git", ("host.test", "a/b")),
        ];
        for (url, (h, p)) in cases {
            assert_eq!(parse_remote(url), Some((h.to_owned(), p.to_owned())), "{url}");
        }
        assert_eq!(parse_remote("/local/path/repo"), None);
        assert_eq!(parse_remote("not a url"), None);
        assert_eq!(parse_remote("https://github.com/"), None);
    }

    #[test]
    fn ssh_alias_host() {
        assert!(host_matches("ssh.github.com", "github.com"));
        assert!(host_matches("github.com", "github.com"));
        assert!(!host_matches("gitlab.com", "github.com"));
    }

    #[test]
    fn linked_ticket_keys() {
        assert_eq!(
            linked_tickets(&["feature/SHOP-142-rate-limit", "SHOP-142: Add limiter (fixes #12)"]),
            vec!["SHOP-142", "#12"]
        );
        assert!(linked_tickets(&["main", "bump deps"]).is_empty());
    }
}

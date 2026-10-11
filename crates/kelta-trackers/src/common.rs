//! Helpers shared by the tracker providers.

use kelta_proto::error::KeltaError;
use kelta_proto::settings::{AccountConfig, TrackerBinding, TrackerView};
use kelta_proto::tracker::{Column, Comment, Status, StatusCategory, Ticket, TicketItem, User};
use serde_json::Value;

/// Comments shown in `TicketDetail` (oldest first).
pub const COMMENT_LIMIT: usize = 20;

pub fn s<'a>(v: &'a Value, key: &str) -> Option<&'a str> {
    v.get(key).and_then(Value::as_str)
}

/// JSON number or string as a string (`id: 10001` and `id: "10001"` both work).
pub fn idstr(v: &Value) -> Option<String> {
    match v {
        Value::String(x) => Some(x.clone()),
        Value::Number(n) => Some(n.to_string()),
        _ => None,
    }
}

/// Today in the local timezone as `YYYY-MM-DD`: a milestone due today stays current until local midnight.
pub fn today() -> String {
    let mut t: libc::time_t = 0;
    // SAFETY: `time` writes the current time into `t`; `localtime_r` fills `tm` (a plain C struct,
    // zero-initialised) and does not retain the pointers.
    let tm = unsafe {
        libc::time(&mut t);
        let mut tm: libc::tm = std::mem::zeroed();
        if libc::localtime_r(&t, &mut tm).is_null() {
            return time::OffsetDateTime::now_utc().date().to_string();
        }
        tm
    };
    format!("{:04}-{:02}-{:02}", tm.tm_year + 1900, tm.tm_mon + 1, tm.tm_mday)
}

/// `base_url` of the account, trimmed; required for Jira and Redmine.
pub fn base_url(account: &AccountConfig) -> Result<String, KeltaError> {
    account
        .effective_base_url()
        .map(|u| kelta_http::util::trim_url(&u))
        .filter(|u| !u.is_empty())
        .ok_or_else(|| KeltaError::invalid("account needs a base_url"))
}

/// Keep the last `n` items (providers return comments oldest first).
pub fn last_n<T>(mut v: Vec<T>, n: usize) -> Vec<T> {
    if v.len() > n {
        v.drain(..v.len() - n);
    }
    v
}

/// Status category from a status/column name (used where the provider gives no category).
/// Matches word starts, never inside a word: "Incomplete" / "Unresolved" are not Done, and a
/// "not" / "non" / "n't" (but not "won't") drops the rest of the name ("Not yet done" is Todo, not Done). Todo wins over InProgress:
/// "Ready for development" is not started.
pub fn category_from_name(name: &str) -> StatusCategory {
    // Words split on non-alphanumerics and camelCase ("InProgress"); apostrophes kept until negation is read.
    let mut split = String::new();
    let mut prev_lower = false;
    for c in name.chars().map(|c| if c == '’' { '\'' } else { c }) {
        let word = c.is_alphanumeric() || c == '\'';
        if !word || (c.is_uppercase() && prev_lower) {
            split.push(' ');
        }
        if word {
            split.extend(c.to_lowercase());
        }
        if c != '\'' {
            prev_lower = c.is_lowercase();
        }
    }
    let (mut n, mut negated) = (String::new(), false);
    for w in split.split_whitespace() {
        // "n't" contractions negate like "not", except "won't" ("Won't fix" is Done).
        if w == "not" || w == "non" || (w.ends_with("n't") && w != "won't") {
            negated = true;
        }
        if negated {
            continue;
        }
        n.push(' ');
        n.push_str(&w.replace('\'', ""));
    }
    let has = |w: &[&str]| w.iter().any(|x| n.contains(&format!(" {x}")));
    if has(&["done", "closed", "resolved", "complete", "finished", "rejected", "cancel", "wont"]) {
        StatusCategory::Done
    } else if has(&["review", "qa", "testing", "to test", "feedback", "validation"]) {
        StatusCategory::InReview
    } else if negated
        || has(&[
            "new", "open", "reopen", "todo", "to do", "backlog", "ready", "planned", "triage", "selected",
        ])
    {
        StatusCategory::Todo
    } else if has(&["progress", "doing", "develop", "wip", "started", "active", "implement", "ongoing"]) {
        StatusCategory::InProgress
    } else {
        StatusCategory::Unknown
    }
}

pub fn user_from(
    id: impl Into<String>,
    name: impl Into<String>,
    login: Option<String>,
    avatar: Option<String>,
) -> User {
    User { id: id.into(), name: name.into(), login, avatar_url: avatar, email: None }
}

/// Configured `columns` of a binding as `Column`s (explicit configuration wins over discovery).
pub fn columns_from_binding(b: &TrackerBinding) -> Option<Vec<Column>> {
    if b.columns.is_empty() {
        return None;
    }
    Some(
        b.columns
            .iter()
            .enumerate()
            .map(|(i, c)| Column {
                id: c.id.clone(),
                name: if c.label.is_empty() { c.id.clone() } else { c.label.clone() },
                category: c.categories.first().copied().unwrap_or_else(|| {
                    c.names.first().map(|n| category_from_name(n)).unwrap_or(StatusCategory::Unknown)
                }),
                order: i as u32,
                match_names: c.names.clone(),
            })
            .collect(),
    )
}

/// One column per category (To do, In progress, In review, Done) from a flat status list; empty
/// categories are skipped. Status names go to `match_names`.
pub fn columns_from_statuses(statuses: &[Status]) -> Vec<Column> {
    let groups = [
        (StatusCategory::Todo, "todo", "To do"),
        (StatusCategory::InProgress, "in_progress", "In progress"),
        (StatusCategory::InReview, "in_review", "In review"),
        (StatusCategory::Done, "done", "Done"),
    ];
    let mut out = Vec::new();
    for (cat, id, label) in groups {
        let names: Vec<String> =
            statuses.iter().filter(|s| s.category == cat).map(|s| s.name.clone()).collect();
        if !names.is_empty() {
            out.push(Column {
                id: id.to_owned(),
                name: label.to_owned(),
                category: cat,
                order: out.len() as u32,
                match_names: names,
            });
        }
    }
    let unknown: Vec<String> =
        statuses.iter().filter(|s| s.category == StatusCategory::Unknown).map(|s| s.name.clone()).collect();
    if !unknown.is_empty() {
        out.push(Column {
            id: "other".into(),
            name: "Other".into(),
            category: StatusCategory::Unknown,
            order: out.len() as u32,
            match_names: unknown,
        });
    }
    out
}

/// Users whose name or login contains `query`, ignoring case (an empty query keeps them all).
pub fn users_matching(users: impl IntoIterator<Item = User>, query: &str) -> Vec<User> {
    let q = query.trim().to_lowercase();
    users
        .into_iter()
        .filter(|u| {
            [Some(&u.name), u.login.as_ref()].into_iter().flatten().any(|x| x.to_lowercase().contains(&q))
        })
        .collect()
}

pub fn comment(author: User, created_at: String, body_html: String) -> Comment {
    Comment { author, created_at, body_html }
}

/// A sub-task in `TicketDetail.children`: the provider gives the ticket, kelta-core the rest.
pub fn child(ticket: Ticket) -> TicketItem {
    TicketItem { ticket, ..TicketItem::default() }
}

/// Split `"acme/shop#12"` into (`"acme/shop"`, 12).
pub fn split_repo_number(key: &str) -> Result<(String, u64), KeltaError> {
    let (repo, n) =
        key.rsplit_once('#').ok_or_else(|| KeltaError::invalid(format!("bad ticket key: {key}")))?;
    let n = n.parse::<u64>().map_err(|_| KeltaError::invalid(format!("bad ticket key: {key}")))?;
    // Keys reach URL paths unescaped: only plain `/`-separated name segments.
    let ok = |p: &str| {
        !p.is_empty()
            && p != ".."
            && p != "."
            && p.chars().all(|c| c.is_ascii_alphanumeric() || "._-".contains(c))
    };
    if !repo.split('/').all(ok) {
        return Err(KeltaError::invalid(format!("bad ticket key: {key}")));
    }
    Ok((repo.to_owned(), n))
}

/// Where `Tracker::create` files a ticket: `value`, the view's `field`. It reaches URL paths, so it is
/// checked like a ticket key's repo part.
pub fn create_in<'a>(view: &TrackerView, field: &str, value: Option<&'a str>) -> Result<&'a str, KeltaError> {
    value.filter(|v| split_repo_number(&format!("{v}#1")).is_ok()).ok_or_else(|| {
        KeltaError::invalid(format!(
            "tracker source `{}` names no valid `{field}` to create tickets in",
            view.label
        ))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn categories_from_names() {
        assert_eq!(category_from_name("In Review"), StatusCategory::InReview);
        assert_eq!(category_from_name("In Progress"), StatusCategory::InProgress);
        assert_eq!(category_from_name("Closed"), StatusCategory::Done);
        assert_eq!(category_from_name("Backlog"), StatusCategory::Todo);
        assert_eq!(category_from_name("Zorp"), StatusCategory::Unknown);
        assert_eq!(category_from_name("InProgress"), StatusCategory::InProgress);
        assert_eq!(category_from_name("won't fix"), StatusCategory::Done);
        assert_eq!(category_from_name("workflow::to-do"), StatusCategory::Todo);
        // negated or embedded words are not matched (#289)
        assert_eq!(category_from_name("Not started"), StatusCategory::Todo);
        assert_eq!(category_from_name("workflow::not-started"), StatusCategory::Todo);
        assert_eq!(category_from_name("Not done"), StatusCategory::Todo);
        assert_eq!(category_from_name("workflow::not-done"), StatusCategory::Todo);
        assert_eq!(category_from_name("Not yet done"), StatusCategory::Todo);
        assert_eq!(category_from_name("workflow::not-yet-done"), StatusCategory::Todo);
        assert_eq!(category_from_name("Not in progress"), StatusCategory::Todo);
        assert_eq!(category_from_name("Not in review"), StatusCategory::Todo);
        assert_eq!(category_from_name("Incomplete"), StatusCategory::Unknown);
        assert_eq!(category_from_name("Unresolved"), StatusCategory::Unknown);
        assert_eq!(category_from_name("Inactive"), StatusCategory::Unknown);
        assert_eq!(category_from_name("Ready for development"), StatusCategory::Todo);
        // "n't" contractions negate too, except "won't" (#316)
        assert_eq!(category_from_name("Isn't done"), StatusCategory::Todo);
        assert_eq!(category_from_name("Hasn’t started"), StatusCategory::Todo);
        assert_eq!(category_from_name("Don't merge"), StatusCategory::Todo);
        assert_eq!(category_from_name("ISN'T DONE"), StatusCategory::Todo);
        assert_eq!(category_from_name("Won't fix"), StatusCategory::Done);
        assert_eq!(category_from_name("Won’t do"), StatusCategory::Done);
        assert_eq!(category_from_name("Deployment done"), StatusCategory::Done);
    }

    #[test]
    fn keys() {
        assert_eq!(split_repo_number("grp/sub/proj#12").unwrap(), ("grp/sub/proj".to_owned(), 12));
        assert!(split_repo_number("nope").is_err());
        assert!(split_repo_number("#3").is_err());
        assert!(split_repo_number("a/b/../../x#1").is_err());
        assert!(split_repo_number("a/b?x#1").is_err());
        let v = TrackerView { label: "Mine".into(), ..TrackerView::default() };
        assert_eq!(create_in(&v, "repo", Some("acme/shop")).unwrap(), "acme/shop");
        assert!(create_in(&v, "repo", None).unwrap_err().message.contains("`Mine` names no valid `repo`"));
        assert!(create_in(&v, "repo", Some("a/../b")).is_err());
    }

    #[test]
    fn users_match_name_or_login() {
        let u = |name: &str, login: &str| user_from(login, name, Some(login.to_owned()), None);
        let all = vec![u("Dave Lee", "dlee"), u("Ann Smith", "asmith")];
        let ids = |q| users_matching(all.clone(), q).into_iter().map(|u| u.id).collect::<Vec<_>>();
        assert_eq!(ids("LEE"), ["dlee"]);
        assert_eq!(ids("asm"), ["asmith"]);
        assert_eq!(ids(" ").len(), 2);
    }

    #[test]
    fn last_n_keeps_the_newest() {
        assert_eq!(last_n(vec![1, 2, 3, 4], 2), vec![3, 4]);
        assert_eq!(last_n(vec![1], 5), vec![1]);
    }
}

//! Helpers shared by the tracker providers.

use kelta_proto::error::KeltaError;
use kelta_proto::settings::{AccountConfig, TrackerBinding};
use kelta_proto::tracker::{Column, Comment, Status, StatusCategory, User};
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
pub fn category_from_name(name: &str) -> StatusCategory {
    let n = name.to_ascii_lowercase();
    let has = |w: &[&str]| w.iter().any(|x| n.contains(x));
    if has(&["done", "closed", "resolved", "complete", "finished", "rejected", "cancel", "won't", "wont"]) {
        StatusCategory::Done
    } else if has(&["review", "qa", "testing", "to test", "feedback", "validation"]) {
        StatusCategory::InReview
    } else if has(&["progress", "doing", "develop", "wip", "started", "active", "implement", "ongoing"]) {
        StatusCategory::InProgress
    } else if has(&[
        "new", "open", "todo", "to do", "to-do", "backlog", "ready", "planned", "triage", "selected",
    ]) {
        StatusCategory::Todo
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
    User { id: id.into(), name: name.into(), login, avatar_url: avatar }
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

pub fn comment(author: User, created_at: String, body_html: String) -> Comment {
    Comment { author, created_at, body_html }
}

/// Split `"acme/shop#12"` into (`"acme/shop"`, 12).
pub fn split_repo_number(key: &str) -> Result<(String, u64), KeltaError> {
    let (repo, n) =
        key.rsplit_once('#').ok_or_else(|| KeltaError::invalid(format!("bad ticket key: {key}")))?;
    let n = n.parse::<u64>().map_err(|_| KeltaError::invalid(format!("bad ticket key: {key}")))?;
    if repo.is_empty() {
        return Err(KeltaError::invalid(format!("bad ticket key: {key}")));
    }
    Ok((repo.to_owned(), n))
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
    }

    #[test]
    fn keys() {
        assert_eq!(split_repo_number("grp/sub/proj#12").unwrap(), ("grp/sub/proj".to_owned(), 12));
        assert!(split_repo_number("nope").is_err());
        assert!(split_repo_number("#3").is_err());
    }

    #[test]
    fn last_n_keeps_the_newest() {
        assert_eq!(last_n(vec![1, 2, 3, 4], 2), vec![3, 4]);
        assert_eq!(last_n(vec![1], 5), vec![1]);
    }
}

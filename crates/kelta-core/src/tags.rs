//! Local tags (#349): my reminders on tickets and pull requests, one JSON value in `ui_state`.
//! Never sent to a tracker or code host.

use std::collections::HashSet;

use kelta_proto::error::KeltaError;
use kelta_proto::model::LocalTags;

use crate::Core;
use crate::store::q;

const KEY: &str = "tags";

impl Core {
    pub async fn tags_get(&self) -> Result<LocalTags, KeltaError> {
        let v = self.store.call(|c| q::ui_state_get(c, KEY)).await?;
        Ok(v.and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default())
    }

    /// Replaces the whole set; returns it as stored (see [`clean`]).
    pub async fn tags_set(&self, tags: LocalTags) -> Result<LocalTags, KeltaError> {
        let tags = clean(tags)?;
        let v = serde_json::to_string(&tags).map_err(|e| KeltaError::internal(format!("tags: {e}")))?;
        self.store.call(move |c| q::ui_state_set(c, KEY, &v)).await?;
        Ok(tags)
    }
}

/// Names are trimmed, non-empty and unique, colours like a project's. An item keeps only defined
/// tags, once each, in the definitions' order; an item left with none is dropped.
fn clean(mut t: LocalTags) -> Result<LocalTags, KeltaError> {
    let mut names = HashSet::new();
    for tag in &mut t.tags {
        tag.name = tag.name.trim().to_owned();
        if tag.name.is_empty() {
            return Err(KeltaError::invalid("A tag needs a name"));
        }
        if !names.insert(tag.name.clone()) {
            return Err(KeltaError::invalid(format!("Two tags are named {}", tag.name)));
        }
        if tag.color.as_deref().is_some_and(|c| !color_ok(c)) {
            return Err(KeltaError::invalid("A tag colour must be a theme colour or #rrggbb"));
        }
    }
    let order = |n: &String| t.tags.iter().position(|x| &x.name == n);
    for map in [&mut t.tickets, &mut t.prs] {
        map.retain(|_, v| {
            v.retain(|n| names.contains(n));
            v.sort_by_key(order);
            v.dedup();
            !v.is_empty()
        });
    }
    Ok(t)
}

/// The project colour pattern (`settings::COLOR_PATTERN`).
fn color_ok(c: &str) -> bool {
    let hex = |h: &str| matches!(h.len(), 3 | 6) && h.chars().all(|c| c.is_ascii_hexdigit());
    let slot = |s: &str| {
        ["red", "green", "yellow", "blue", "magenta", "cyan"]
            .contains(&s.strip_prefix("bright-").unwrap_or(s))
    };
    c.strip_prefix('#').is_some_and(hex) || c.strip_prefix("theme:").is_some_and(slot)
}

#[cfg(test)]
mod tests {
    use kelta_proto::model::LocalTag;

    use super::*;

    fn tag(name: &str, color: Option<&str>) -> LocalTag {
        LocalTag { name: name.into(), color: color.map(Into::into) }
    }

    #[test]
    fn items_keep_defined_tags_once_in_order() {
        let t = clean(LocalTags {
            tags: vec![tag(" retest ", Some("theme:bright-red")), tag("ping", Some("#a1b2c3"))],
            tickets: [
                ("j:A-1".into(), vec!["ping".into(), "gone".into(), "retest".into(), "ping".into()]),
                ("j:A-2".into(), vec!["gone".into()]),
            ]
            .into(),
            prs: [("gh:acme/shop#3".into(), vec!["ping".into()])].into(),
        })
        .unwrap();
        assert_eq!(t.tags[0].name, "retest");
        assert_eq!(t.tickets.len(), 1, "an item left without tags is dropped");
        assert_eq!(t.tickets["j:A-1"], ["retest", "ping"]);
        assert_eq!(t.prs["gh:acme/shop#3"], ["ping"]);
    }

    #[test]
    fn bad_definitions_are_refused() {
        let with = |tags| clean(LocalTags { tags, ..LocalTags::default() });
        assert!(with(vec![tag(" ", None)]).is_err());
        assert!(with(vec![tag("a", None), tag("a ", None)]).is_err());
        assert!(with(vec![tag("a", Some("theme:purple"))]).is_err());
        assert!(with(vec![tag("a", Some("#12345"))]).is_err());
        assert!(with(vec![tag("a", Some("#abc")), tag("b", Some("theme:cyan")), tag("c", None)]).is_ok());
    }
}

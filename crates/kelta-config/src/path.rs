//! Dotted settings paths (`keys.bindings."palette.open"`) and JSON navigation helpers.
//!
//! A segment containing anything but `[A-Za-z0-9_-]` is double-quoted when joined, and quoted
//! segments are understood when splitting, so map keys that contain dots (ActionIds) round-trip.

use serde_json::{Map, Value};

fn bare(seg: &str) -> bool {
    !seg.is_empty() && seg.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

/// Join segments into a dotted path.
pub fn join_path(segs: &[String]) -> String {
    let mut out = String::new();
    for (i, s) in segs.iter().enumerate() {
        if i > 0 {
            out.push('.');
        }
        if bare(s) {
            out.push_str(s);
        } else {
            out.push('"');
            for c in s.chars() {
                if c == '"' || c == '\\' {
                    out.push('\\');
                }
                out.push(c);
            }
            out.push('"');
        }
    }
    out
}

/// Split a dotted path; quoted segments may contain dots. Empty input yields no segments.
pub fn split_path(path: &str) -> Result<Vec<String>, String> {
    let mut segs = Vec::new();
    let mut cur = String::new();
    let mut chars = path.chars().peekable();
    let mut quoted = false;
    let mut had_any = false;
    while let Some(c) = chars.next() {
        had_any = true;
        if quoted {
            match c {
                '\\' => match chars.next() {
                    Some(n) => cur.push(n),
                    None => return Err("dangling escape in path".into()),
                },
                '"' => quoted = false,
                _ => cur.push(c),
            }
        } else {
            match c {
                '"' => quoted = true,
                '.' => {
                    if cur.is_empty() {
                        return Err(format!("empty segment in path `{path}`"));
                    }
                    segs.push(std::mem::take(&mut cur));
                }
                _ => cur.push(c),
            }
        }
    }
    if quoted {
        return Err(format!("unterminated quote in path `{path}`"));
    }
    if had_any {
        if cur.is_empty() {
            return Err(format!("empty segment in path `{path}`"));
        }
        segs.push(cur);
    }
    Ok(segs)
}

/// Segments of a JSON pointer (`/a/b~1c`).
pub fn pointer_segments(pointer: &str) -> Vec<String> {
    pointer.split('/').skip(1).map(|s| s.replace("~1", "/").replace("~0", "~")).collect()
}

/// Navigate an object tree (arrays are addressed by numeric segments).
pub fn get<'a>(v: &'a Value, segs: &[String]) -> Option<&'a Value> {
    let mut cur = v;
    for s in segs {
        cur = match cur {
            Value::Object(m) => m.get(s)?,
            Value::Array(a) => a.get(s.parse::<usize>().ok()?)?,
            _ => return None,
        };
    }
    Some(cur)
}

/// Set a value, creating intermediate objects. Fails if an intermediate is not an object.
pub fn set(root: &mut Value, segs: &[String], value: Value) -> Result<(), String> {
    if segs.is_empty() {
        *root = value;
        return Ok(());
    }
    if root.is_null() {
        *root = Value::Object(Map::new());
    }
    let mut cur = root;
    for s in &segs[..segs.len() - 1] {
        let Value::Object(m) = cur else {
            return Err(format!("`{s}` is not inside a table"));
        };
        cur = m.entry(s.clone()).or_insert_with(|| Value::Object(Map::new()));
        if cur.is_null() {
            *cur = Value::Object(Map::new());
        }
    }
    match cur {
        Value::Object(m) => {
            m.insert(segs[segs.len() - 1].clone(), value);
            Ok(())
        }
        _ => Err("parent is not a table".into()),
    }
}

/// Remove a value; parents that become empty objects are pruned. Returns whether it existed.
pub fn remove(root: &mut Value, segs: &[String]) -> bool {
    fn rec(v: &mut Value, segs: &[String]) -> bool {
        let Value::Object(m) = v else { return false };
        if segs.len() == 1 {
            return m.remove(&segs[0]).is_some();
        }
        let Some(child) = m.get_mut(&segs[0]) else { return false };
        let removed = rec(child, &segs[1..]);
        if removed && matches!(child, Value::Object(c) if c.is_empty()) {
            m.remove(&segs[0]);
        }
        removed
    }
    if segs.is_empty() {
        return false;
    }
    rec(root, segs)
}

/// Dotted paths of every leaf (scalars, arrays, empty objects).
pub fn leaves(v: &Value) -> Vec<Vec<String>> {
    fn rec(v: &Value, cur: &mut Vec<String>, out: &mut Vec<Vec<String>>) {
        match v {
            Value::Object(m) if !m.is_empty() => {
                for (k, c) in m {
                    cur.push(k.clone());
                    rec(c, cur, out);
                    cur.pop();
                }
            }
            _ => out.push(cur.clone()),
        }
    }
    let mut out = Vec::new();
    rec(v, &mut Vec::new(), &mut out);
    out.retain(|p| !p.is_empty());
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn round_trip_quoted_segments() {
        let segs = vec!["keys".to_owned(), "bindings".to_owned(), "palette.open".to_owned()];
        let joined = join_path(&segs);
        assert_eq!(joined, "keys.bindings.\"palette.open\"");
        assert_eq!(split_path(&joined).unwrap(), segs);
        assert_eq!(split_path("").unwrap(), Vec::<String>::new());
        assert!(split_path("a..b").is_err());
        assert!(split_path("a.\"b").is_err());
    }

    #[test]
    fn set_get_remove_prunes() {
        let mut v = json!({});
        let p = split_path("a.b.c").unwrap();
        set(&mut v, &p, json!(1)).unwrap();
        assert_eq!(get(&v, &p), Some(&json!(1)));
        assert!(remove(&mut v, &p));
        assert_eq!(v, json!({}));
        assert!(!remove(&mut v, &p));
    }

    #[test]
    fn pointer() {
        assert_eq!(pointer_segments("/a/b~1c/0"), vec!["a", "b/c", "0"]);
        assert!(pointer_segments("").is_empty());
    }
}

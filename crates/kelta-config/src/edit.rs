//! Comment- and order-preserving edits of TOML documents (`toml_edit`).
//!
//! Only the touched key changes: replacing a value keeps its decor (comments), updating a table
//! syncs key by key, setting an equal value is a no-op.

use serde_json::{Map, Value};
use toml_edit::{Array, ArrayOfTables, DocumentMut, InlineTable, Item, Table, TableLike};

use crate::toml_io::item_to_json;

fn num_eq(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Number(x), Value::Number(y)) => x.as_f64() == y.as_f64(),
        (Value::Array(x), Value::Array(y)) => {
            x.len() == y.len() && x.iter().zip(y).all(|(p, q)| num_eq(p, q))
        }
        (Value::Object(x), Value::Object(y)) => {
            x.len() == y.len() && x.iter().all(|(k, v)| y.get(k).is_some_and(|w| num_eq(v, w)))
        }
        _ => a == b,
    }
}

fn scalar_value(v: &Value) -> Result<toml_edit::Value, String> {
    Ok(match v {
        Value::Bool(b) => toml_edit::Value::from(*b),
        Value::String(s) => toml_edit::Value::from(s.as_str()),
        Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                toml_edit::Value::from(i)
            } else if let Some(f) = n.as_f64() {
                if n.is_u64() {
                    return Err(format!("integer {n} is too large for TOML"));
                }
                toml_edit::Value::from(f)
            } else {
                return Err(format!("unsupported number {n}"));
            }
        }
        Value::Null => return Err("null has no TOML representation".into()),
        Value::Array(_) | Value::Object(_) => return Err("not a scalar".into()),
    })
}

/// Any JSON value as an inline TOML value (nulls inside objects are dropped).
fn inline_value(v: &Value) -> Result<toml_edit::Value, String> {
    match v {
        Value::Array(a) => {
            let mut arr = Array::new();
            for e in a {
                arr.push(inline_value(e)?);
            }
            Ok(toml_edit::Value::Array(arr))
        }
        Value::Object(m) => Ok(toml_edit::Value::InlineTable(inline_table(m)?)),
        other => scalar_value(other),
    }
}

fn inline_table(m: &Map<String, Value>) -> Result<InlineTable, String> {
    let mut t = InlineTable::new();
    for (k, v) in m {
        if v.is_null() {
            continue;
        }
        t.insert(k, inline_value(v)?);
    }
    Ok(t)
}

fn is_table_list(a: &[Value]) -> bool {
    !a.is_empty() && a.iter().all(Value::is_object)
}

/// A table of an array of tables: nested objects stay inline.
fn aot_table(m: &Map<String, Value>) -> Result<Table, String> {
    let mut t = Table::new();
    for (k, v) in m {
        if v.is_null() {
            continue;
        }
        t.insert(k, Item::Value(inline_value(v)?));
    }
    Ok(t)
}

fn aot(a: &[Value]) -> Result<ArrayOfTables, String> {
    let mut out = ArrayOfTables::new();
    for e in a {
        if let Value::Object(m) = e {
            out.push(aot_table(m)?);
        }
    }
    Ok(out)
}

/// A standard table: nested objects become sub-tables, object lists become `[[arrays]]`.
fn std_table(m: &Map<String, Value>) -> Result<Table, String> {
    let mut t = Table::new();
    let mut has_values = false;
    for (k, v) in m {
        match v {
            Value::Null => {}
            Value::Object(o) => {
                t.insert(k, Item::Table(std_table(o)?));
            }
            Value::Array(a) if is_table_list(a) => {
                t.insert(k, Item::ArrayOfTables(aot(a)?));
            }
            other => {
                t.insert(k, Item::Value(inline_value(other)?));
                has_values = true;
            }
        }
    }
    if !has_values && !m.is_empty() {
        t.set_implicit(true);
    }
    Ok(t)
}

fn to_item(v: &Value, inline: bool) -> Result<Item, String> {
    if inline {
        return Ok(Item::Value(inline_value(v)?));
    }
    match v {
        Value::Object(m) => Ok(Item::Table(std_table(m)?)),
        Value::Array(a) if is_table_list(a) => Ok(Item::ArrayOfTables(aot(a)?)),
        other => Ok(Item::Value(inline_value(other)?)),
    }
}

fn sync_table(t: &mut dyn TableLike, m: &Map<String, Value>, inline: bool) -> Result<(), String> {
    let existing: Vec<String> = t.iter().map(|(k, _)| k.to_owned()).collect();
    for k in existing {
        if m.get(&k).is_none_or(Value::is_null) {
            t.remove(&k);
        }
    }
    for (k, v) in m {
        if v.is_null() {
            continue;
        }
        if let Some(e) = t.get_mut(k) {
            apply_item(e, v, inline)?;
            continue;
        }
        let item = to_item(v, inline)?;
        t.insert(k, item);
    }
    Ok(())
}

fn apply_item(existing: &mut Item, new: &Value, inline: bool) -> Result<(), String> {
    if num_eq(&item_to_json(existing), new) {
        return Ok(());
    }
    match (&mut *existing, new) {
        (Item::Table(t), Value::Object(m)) => return sync_table(t, m, false),
        (Item::Value(toml_edit::Value::InlineTable(t)), Value::Object(m)) => return sync_table(t, m, true),
        (Item::ArrayOfTables(a), Value::Array(arr)) if is_table_list(arr) => {
            for (i, v) in arr.iter().enumerate() {
                let Value::Object(m) = v else { continue };
                match a.get_mut(i) {
                    Some(t) => sync_table(t, m, false)?,
                    None => a.push(aot_table(m)?),
                }
            }
            while a.len() > arr.len() {
                a.remove(a.len() - 1);
            }
            return Ok(());
        }
        _ => {}
    }
    let mut new_item = to_item(new, inline)?;
    match (&*existing, &mut new_item) {
        (Item::Value(old), Item::Value(nv)) => *nv.decor_mut() = old.decor().clone(),
        (Item::Table(old), Item::Table(nt)) => *nt.decor_mut() = old.decor().clone(),
        _ => {}
    }
    *existing = new_item;
    Ok(())
}

/// Set `segs` to `value`, creating tables as needed. `null` is rejected (use [`remove_path`]).
pub fn set_path(doc: &mut DocumentMut, segs: &[String], value: &Value) -> Result<(), String> {
    if segs.is_empty() {
        return Err("empty path".into());
    }
    if value.is_null() {
        return Err("null cannot be stored in TOML; reset the key instead".into());
    }
    set_in(doc.as_table_mut(), segs, value, false)
}

fn set_in(t: &mut dyn TableLike, segs: &[String], value: &Value, inline: bool) -> Result<(), String> {
    let key = &segs[0];
    if segs.len() == 1 {
        if let Some(e) = t.get_mut(key) {
            return apply_item(e, value, inline);
        }
        let item = to_item(value, inline)?;
        t.insert(key, item);
        return Ok(());
    }
    if t.get(key).is_none() {
        let item = if inline {
            Item::Value(toml_edit::Value::InlineTable(InlineTable::new()))
        } else {
            let mut tb = Table::new();
            tb.set_implicit(true);
            Item::Table(tb)
        };
        t.insert(key, item);
    }
    let Some(child) = t.get_mut(key) else { return Err("internal: missing child".into()) };
    let child_inline = matches!(child, Item::Value(_));
    match child.as_table_like_mut() {
        Some(ct) => set_in(ct, &segs[1..], value, child_inline || inline),
        None => Err(format!("`{key}` is not a table")),
    }
}

/// Remove a key; tables left empty are removed too. Returns whether anything was removed.
pub fn remove_path(doc: &mut DocumentMut, segs: &[String]) -> bool {
    if segs.is_empty() {
        return false;
    }
    remove_in(doc.as_table_mut(), segs)
}

fn remove_in(t: &mut dyn TableLike, segs: &[String]) -> bool {
    if segs.len() == 1 {
        return t.remove(&segs[0]).is_some();
    }
    let (removed, empty) = {
        let Some(child) = t.get_mut(&segs[0]) else { return false };
        let Some(ct) = child.as_table_like_mut() else { return false };
        let r = remove_in(ct, &segs[1..]);
        (r, ct.is_empty())
    };
    if removed && empty {
        t.remove(&segs[0]);
    }
    removed
}

/// Drop empty tables / arrays / nulls recursively (used before writing project drafts).
pub fn prune_empty(v: &Value) -> Value {
    match v {
        Value::Object(m) => {
            let mut out = Map::new();
            for (k, x) in m {
                let p = prune_empty(x);
                let empty = match &p {
                    Value::Null => true,
                    Value::Object(o) => o.is_empty(),
                    Value::Array(a) => a.is_empty(),
                    _ => false,
                };
                if !empty {
                    out.insert(k.clone(), p);
                }
            }
            Value::Object(out)
        }
        Value::Array(a) => Value::Array(a.iter().map(prune_empty).collect()),
        other => other.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::path::split_path;
    use serde_json::json;

    fn doc(s: &str) -> DocumentMut {
        s.parse().unwrap()
    }

    fn p(s: &str) -> Vec<String> {
        split_path(s).unwrap()
    }

    #[test]
    fn replaces_value_keeping_comments_and_order() {
        let src = "# top\n[terminal]\n# size\nfont_size = 13 # trailing\nrenderer = \"auto\"\n\n[app]\ntheme = \"dark\"\n";
        let mut d = doc(src);
        set_path(&mut d, &p("terminal.font_size"), &json!(15)).unwrap();
        assert_eq!(
            d.to_string(),
            "# top\n[terminal]\n# size\nfont_size = 15 # trailing\nrenderer = \"auto\"\n\n[app]\ntheme = \"dark\"\n"
        );
    }

    #[test]
    fn equal_value_is_a_noop() {
        let src = "[terminal]\nfont_size   =   13.0   # keep\n";
        let mut d = doc(src);
        set_path(&mut d, &p("terminal.font_size"), &json!(13)).unwrap();
        assert_eq!(d.to_string(), src);
    }

    #[test]
    fn creates_nested_tables_and_quoted_keys() {
        let mut d = doc("[app]\ntheme = \"dark\"\n");
        set_path(&mut d, &p("keys.bindings.\"palette.open\""), &json!(["mod+k"])).unwrap();
        let text = d.to_string();
        assert!(text.contains("[keys.bindings]"), "{text}");
        assert!(text.contains("\"palette.open\" = [\"mod+k\"]"), "{text}");
        assert!(!text.contains("[keys]\n"), "implicit parent must not print: {text}");
    }

    #[test]
    fn tables_sync_per_key() {
        let src = "[claude.profiles.default]\n# model\nmodel = \"opus\" # c\neffort = \"high\"\n";
        let mut d = doc(src);
        set_path(
            &mut d,
            &p("claude.profiles"),
            &json!({"default": {"model": "sonnet", "effort": "high"}, "plan": {"model": "opus"}}),
        )
        .unwrap();
        let text = d.to_string();
        assert!(text.contains("model = \"sonnet\" # c"), "{text}");
        assert!(text.contains("# model"), "{text}");
        assert!(text.contains("[claude.profiles.plan]"), "{text}");
    }

    #[test]
    fn object_lists_become_array_of_tables_and_sync_by_index() {
        let mut d = doc("# c\n");
        set_path(&mut d, &p("tools"), &json!([{"id": "a", "command": "x", "start": {"command": "y"}}]))
            .unwrap();
        let text = d.to_string();
        assert!(text.contains("[[tools]]"), "{text}");
        assert!(text.contains("start = { command = \"y\" }"), "{text}");
        let with_comment = text.replace("command = \"x\"", "command = \"x\" # keep");
        let mut d2 = doc(&with_comment);
        set_path(
            &mut d2,
            &p("tools"),
            &json!([{"id": "a", "command": "x", "start": {"command": "y"}}, {"id": "b"}]),
        )
        .unwrap();
        let t2 = d2.to_string();
        assert!(t2.contains("# keep"), "{t2}");
        assert!(t2.contains("id = \"b\""), "{t2}");
    }

    #[test]
    fn remove_prunes_empty_tables() {
        let mut d = doc("[a]\nb = 1\n[c]\nd = 1\ne = 2\n");
        assert!(remove_path(&mut d, &p("a.b")));
        assert!(remove_path(&mut d, &p("c.d")));
        assert!(!remove_path(&mut d, &p("c.zzz")));
        assert_eq!(d.to_string(), "[c]\ne = 2\n");
    }

    #[test]
    fn rejects_null_and_scalar_parents() {
        let mut d = doc("[a]\nb = 1\n");
        assert!(set_path(&mut d, &p("a.c"), &Value::Null).is_err());
        assert!(set_path(&mut d, &p("a.b.c"), &json!(1)).is_err());
    }
}

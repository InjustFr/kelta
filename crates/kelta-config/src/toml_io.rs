//! TOML text ⇄ JSON values, error positions and atomic file IO.

use std::io::Write;
use std::ops::Range;
use std::path::{Path, PathBuf};

use kelta_proto::error::KeltaError;
use kelta_proto::settings::ValidationIssue;
use serde_json::{Map, Number, Value};
use sha2::{Digest, Sha256};
use toml_edit::{Document, Item, Table};

use crate::path::join_path;

/// A parsed TOML document with spans kept for error positions.
pub struct Parsed {
    pub doc: Document<String>,
    pub value: Value,
}

/// 1-based line and column of a byte offset.
pub fn line_col(text: &str, offset: usize) -> (u32, u32) {
    let offset = offset.min(text.len());
    let mut line = 1u32;
    let mut col = 1u32;
    for (i, c) in text.char_indices() {
        if i >= offset {
            break;
        }
        if c == '\n' {
            line += 1;
            col = 1;
        } else {
            col += 1;
        }
    }
    (line, col)
}

fn value_to_json(v: &toml_edit::Value) -> Value {
    match v {
        toml_edit::Value::String(s) => Value::String(s.value().clone()),
        toml_edit::Value::Integer(i) => Value::Number((*i.value()).into()),
        toml_edit::Value::Float(f) => Number::from_f64(*f.value()).map_or(Value::Null, Value::Number),
        toml_edit::Value::Boolean(b) => Value::Bool(*b.value()),
        toml_edit::Value::Datetime(d) => Value::String(d.value().to_string()),
        toml_edit::Value::Array(a) => Value::Array(a.iter().map(value_to_json).collect()),
        toml_edit::Value::InlineTable(t) => {
            let mut m = Map::new();
            for (k, v) in t.iter() {
                m.insert(k.to_owned(), value_to_json(v));
            }
            Value::Object(m)
        }
    }
}

fn table_to_json(t: &Table) -> Value {
    let mut m = Map::new();
    for (k, v) in t.iter() {
        if !v.is_none() {
            m.insert(k.to_owned(), item_to_json(v));
        }
    }
    Value::Object(m)
}

/// Convert any item to JSON (datetimes become strings).
pub fn item_to_json(item: &Item) -> Value {
    match item {
        Item::None => Value::Null,
        Item::Value(v) => value_to_json(v),
        Item::Table(t) => table_to_json(t),
        Item::ArrayOfTables(a) => Value::Array(a.iter().map(table_to_json).collect()),
    }
}

/// Parse text; on syntax error return an issue with `line`/`col`.
pub fn parse(text: &str) -> Result<Parsed, ValidationIssue> {
    match Document::parse(text.to_owned()) {
        Ok(doc) => {
            let value = item_to_json(doc.as_item());
            Ok(Parsed { doc, value })
        }
        Err(e) => {
            let (line, col) = match e.span() {
                Some(r) => {
                    let (l, c) = line_col(text, r.start);
                    (Some(l), Some(c))
                }
                None => (None, None),
            };
            Err(ValidationIssue { path: String::new(), message: e.message().to_owned(), line, col })
        }
    }
}

enum Node<'a> {
    Item(&'a Item),
    Table(&'a Table),
    Val(&'a toml_edit::Value),
}

impl<'a> Node<'a> {
    fn of_item(item: &'a Item) -> Self {
        match item {
            Item::Table(t) => Node::Table(t),
            Item::Value(v) => Node::Val(v),
            other => Node::Item(other),
        }
    }

    fn span(&self) -> Option<Range<usize>> {
        match self {
            Node::Item(i) => i.span(),
            Node::Table(t) => t.span(),
            Node::Val(v) => v.span(),
        }
    }

    /// One path step; the second element is the span to report for the step.
    fn step(&self, seg: &str) -> Option<(Node<'a>, Option<Range<usize>>)> {
        match self {
            Node::Table(t) => {
                t.get_key_value(seg).map(|(k, v)| (Node::of_item(v), k.span().or_else(|| v.span())))
            }
            Node::Val(toml_edit::Value::InlineTable(t)) => {
                t.get_key_value(seg).map(|(k, v)| (Node::of_item(v), k.span().or_else(|| v.span())))
            }
            Node::Val(toml_edit::Value::Array(a)) => {
                let v = a.get(seg.parse::<usize>().ok()?)?;
                Some((Node::Val(v), v.span()))
            }
            Node::Item(Item::ArrayOfTables(a)) => {
                let t = a.get(seg.parse::<usize>().ok()?)?;
                Some((Node::Table(t), t.span()))
            }
            _ => None,
        }
    }
}

/// Span of the key (or element) addressed by `path`; falls back to the nearest ancestor.
pub fn span_of(doc: &Document<String>, path: &[String]) -> Option<Range<usize>> {
    let mut node = Node::of_item(doc.as_item());
    let mut best: Option<Range<usize>> = None;
    for seg in path {
        match node.step(seg) {
            Some((next, span)) => {
                best = span.or(best);
                node = next;
            }
            None => return best.or_else(|| node.span()),
        }
    }
    best.or_else(|| node.span())
}

/// Build a positioned issue for `path` in `doc`.
pub fn issue_at(
    text: &str,
    doc: Option<&Document<String>>,
    path: &[String],
    message: String,
) -> ValidationIssue {
    let (line, col) = doc
        .and_then(|d| span_of(d, path))
        .map(|r| line_col(text, r.start))
        .map_or((None, None), |(l, c)| (Some(l), Some(c)));
    ValidationIssue { path: join_path(path), message, line, col }
}

/// SHA-256 hex digest.
pub fn sha256_hex(bytes: &[u8]) -> String {
    let mut h = Sha256::new();
    h.update(bytes);
    let d = h.finalize();
    let mut s = String::with_capacity(64);
    for b in d {
        s.push_str(&format!("{b:02x}"));
    }
    s
}

/// Read a text file; `Ok(None)` when it does not exist.
pub fn read_optional(path: &Path) -> Result<Option<String>, KeltaError> {
    match std::fs::read_to_string(path) {
        Ok(s) => Ok(Some(s)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(KeltaError::from(e)),
    }
}

/// Atomic write: temp file in the same directory, fsync, rename. Existing permissions are kept.
pub fn write_atomic(path: &Path, text: &str) -> Result<(), KeltaError> {
    let dir = path.parent().ok_or_else(|| KeltaError::invalid("path has no parent directory"))?;
    std::fs::create_dir_all(dir)?;
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let tmp: PathBuf = dir.join(format!(".{name}.kelta-tmp-{}", std::process::id()));
    // A hostile repo may pre-plant a symlink at the temp name: drop it, then create exclusively.
    let _ = std::fs::remove_file(&tmp);
    {
        let mut f = std::fs::OpenOptions::new().write(true).create_new(true).open(&tmp)?;
        f.write_all(text.as_bytes())?;
        f.sync_all()?;
    }
    if let Ok(meta) = std::fs::metadata(path) {
        let _ = std::fs::set_permissions(&tmp, meta.permissions());
    }
    if let Err(e) = std::fs::rename(&tmp, path) {
        let _ = std::fs::remove_file(&tmp);
        return Err(KeltaError::from(e));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parses_and_positions_errors() {
        let p = parse("[a]\nx = 1\n[[l]]\nid = \"q\"\n").unwrap();
        assert_eq!(p.value, json!({"a": {"x": 1}, "l": [{"id": "q"}]}));
        let e = parse("[a]\nx = = 1\n").err().unwrap();
        assert_eq!(e.line, Some(2));
        assert!(e.col.is_some());
    }

    #[test]
    fn spans_follow_paths() {
        let text = "[a]\nx = 1\ny = 2\n\n[[l]]\nid = \"q\"\n[[l]]\nid = \"r\"\n";
        let p = parse(text).unwrap();
        let at = |path: &[&str]| {
            let segs: Vec<String> = path.iter().map(|s| (*s).to_owned()).collect();
            issue_at(text, Some(&p.doc), &segs, String::new())
        };
        assert_eq!(at(&["a", "y"]).line, Some(3));
        assert_eq!(at(&["a", "y"]).col, Some(1));
        assert_eq!(at(&["l", "1"]).line, Some(7));
        assert_eq!(at(&["l", "1", "id"]).line, Some(8));
        assert_eq!(at(&["a", "missing"]).line, Some(1));
    }

    #[test]
    fn atomic_write_replaces() {
        let d = tempfile::tempdir().unwrap();
        let f = d.path().join("sub/c.toml");
        write_atomic(&f, "a = 1\n").unwrap();
        write_atomic(&f, "a = 2\n").unwrap();
        assert_eq!(std::fs::read_to_string(&f).unwrap(), "a = 2\n");
        let leftovers: Vec<_> = std::fs::read_dir(f.parent().unwrap()).unwrap().collect();
        assert_eq!(leftovers.len(), 1);
    }
}

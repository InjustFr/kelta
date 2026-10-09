//! Layer merge (SETTINGS §1) with a per-path provenance map.
//!
//! - tables deep-merge, scalars and plain arrays are replaced by the higher layer;
//! - `by_id` lists merge by `id`: an entry with the same id replaces the inherited one
//!   entirely, except that a stub holding only `id` / `enabled` toggles the inherited entry;
//! - provenance records the winning layer for every leaf, every `by_id` entry
//!   (`tools.lazydocker`) and, for tables, the highest layer found underneath.

use std::collections::{BTreeMap, HashSet};

use kelta_proto::settings::Layer;
use serde_json::Value;

use crate::path::join_path;

type Prov = BTreeMap<Vec<String>, Layer>;

pub struct Merger<'a> {
    by_id: &'a HashSet<String>,
    prov: Prov,
}

fn is_stub(entry: &Value) -> bool {
    entry.as_object().is_some_and(|m| m.keys().all(|k| k == "id" || k == "enabled"))
}

fn entry_id(entry: &Value) -> Option<&str> {
    entry.get("id").and_then(Value::as_str)
}

impl<'a> Merger<'a> {
    /// Start from a base layer (normally the compiled defaults).
    pub fn new(by_id: &'a HashSet<String>, base: &Value, layer: Layer) -> Self {
        let mut m = Self { by_id, prov: Prov::new() };
        m.record(&mut Vec::new(), base, layer);
        m
    }

    /// Record `value` (just inserted at `path`) as coming from `layer`.
    fn record(&mut self, path: &mut Vec<String>, value: &Value, layer: Layer) {
        self.clear_under(path);
        match value {
            Value::Object(m) if !m.is_empty() => {
                for (k, v) in m {
                    path.push(k.clone());
                    self.record(path, v, layer);
                    path.pop();
                }
            }
            Value::Array(a) if self.by_id.contains(&join_path(path)) => {
                self.prov.insert(path.clone(), layer);
                for e in a {
                    if let Some(id) = entry_id(e) {
                        let mut p = path.clone();
                        p.push(id.to_owned());
                        self.prov.insert(p, layer);
                    }
                }
            }
            _ => {
                if !path.is_empty() {
                    self.prov.insert(path.clone(), layer);
                }
            }
        }
    }

    fn clear_under(&mut self, path: &[String]) {
        if path.is_empty() {
            self.prov.clear();
            return;
        }
        let doomed: Vec<Vec<String>> = self
            .prov
            .range(path.to_vec()..)
            .take_while(|(k, _)| k.starts_with(path))
            .map(|(k, _)| k.clone())
            .collect();
        for k in doomed {
            self.prov.remove(&k);
        }
    }

    /// Merge a higher layer over `base`.
    pub fn apply(&mut self, base: &mut Value, over: &Value, layer: Layer) {
        self.merge(base, over, &mut Vec::new(), layer);
    }

    fn merge(&mut self, base: &mut Value, over: &Value, path: &mut Vec<String>, layer: Layer) {
        if let (Value::Object(b), Value::Object(o)) = (&mut *base, over) {
            for (k, ov) in o {
                path.push(k.clone());
                match b.get_mut(k) {
                    Some(bv) => self.merge(bv, ov, path, layer),
                    None => {
                        b.insert(k.clone(), ov.clone());
                        self.record(path, ov, layer);
                    }
                }
                path.pop();
            }
            return;
        }
        if let (Value::Array(b), Value::Array(o)) = (&mut *base, over)
            && self.by_id.contains(&join_path(path))
        {
            for entry in o {
                let Some(id) = entry_id(entry).map(str::to_owned) else {
                    b.push(entry.clone());
                    continue;
                };
                let pos = b.iter().position(|x| entry_id(x) == Some(id.as_str()));
                match pos {
                    Some(i) if is_stub(entry) => {
                        if let (Some(target), Some(en)) = (b[i].as_object_mut(), entry.get("enabled")) {
                            target.insert("enabled".into(), en.clone());
                        }
                    }
                    Some(i) => b[i] = entry.clone(),
                    None => b.push(entry.clone()),
                }
                let mut p = path.clone();
                p.push(id);
                self.prov.insert(p, layer);
            }
            self.prov.insert(path.clone(), layer);
            return;
        }
        *base = over.clone();
        self.record(path, over, layer);
    }

    /// Final provenance map (dotted path → layer), ancestors carry the highest layer below them.
    pub fn finish(self) -> BTreeMap<String, Layer> {
        let mut all: Prov = self.prov.clone();
        for (k, layer) in &self.prov {
            for n in 1..k.len() {
                let e = all.entry(k[..n].to_vec()).or_insert(*layer);
                if *layer > *e {
                    *e = *layer;
                }
            }
        }
        all.into_iter().map(|(k, v)| (join_path(&k), v)).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn by_id() -> HashSet<String> {
        ["tools".to_owned()].into_iter().collect()
    }

    #[test]
    fn deep_merge_and_provenance() {
        let ids = by_id();
        let mut base = json!({"a": {"x": 1, "y": 2}, "list": [1, 2], "tools": []});
        let mut m = Merger::new(&ids, &base, Layer::Default);
        m.apply(&mut base, &json!({"a": {"y": 3}, "list": [9]}), Layer::Global);
        m.apply(&mut base, &json!({"a": {"z": 4}}), Layer::Project);
        assert_eq!(base, json!({"a": {"x": 1, "y": 3, "z": 4}, "list": [9], "tools": []}));
        let s = m.finish();
        assert_eq!(s["a.x"], Layer::Default);
        assert_eq!(s["a.y"], Layer::Global);
        assert_eq!(s["a.z"], Layer::Project);
        assert_eq!(s["a"], Layer::Project);
        assert_eq!(s["list"], Layer::Global);
    }

    #[test]
    fn by_id_replace_disable_and_append() {
        let ids = by_id();
        let mut base = json!({"tools": [{"id": "a", "command": "x", "enabled": true}]});
        let mut m = Merger::new(&ids, &base, Layer::Default);
        m.apply(
            &mut base,
            &json!({"tools": [{"id": "a", "enabled": false}, {"id": "b", "command": "y"}]}),
            Layer::Global,
        );
        assert_eq!(
            base,
            json!({"tools": [{"id": "a", "command": "x", "enabled": false}, {"id": "b", "command": "y"}]})
        );
        m.apply(&mut base, &json!({"tools": [{"id": "a", "command": "z"}]}), Layer::Project);
        assert_eq!(base["tools"][0], json!({"id": "a", "command": "z"}));
        let s = m.finish();
        assert_eq!(s["tools.a"], Layer::Project);
        assert_eq!(s["tools.b"], Layer::Global);
        assert_eq!(s["tools"], Layer::Project);
    }

    #[test]
    fn scalar_replacing_table_drops_stale_provenance() {
        let ids = by_id();
        let mut base = json!({"m": {"k": 1}});
        let mut m = Merger::new(&ids, &base, Layer::Default);
        m.apply(&mut base, &json!({"m": 5}), Layer::Global);
        let s = m.finish();
        assert!(!s.contains_key("m.k"));
        assert_eq!(s["m"], Layer::Global);
    }
}

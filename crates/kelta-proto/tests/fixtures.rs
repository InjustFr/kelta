//! Every JSON fixture under `fixtures/` must round-trip through its Rust type, match the sample
//! registry (`kelta_proto::samples`), and no stray fixture may exist.

use std::collections::BTreeSet;
use std::path::PathBuf;

fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("fixtures")
}

#[test]
fn fixtures_round_trip() {
    let registry = kelta_proto::samples::all();
    let mut expected = BTreeSet::new();
    for f in &registry {
        expected.insert(format!("{}.json", f.name));
        let path = fixtures_dir().join(format!("{}.json", f.name));
        let text = std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("{}: {e} (run `cargo run -p xtask -- codegen`)", path.display()));
        let value: serde_json::Value = serde_json::from_str(&text).unwrap();
        let back = (f.roundtrip)(&value).unwrap_or_else(|e| panic!("{} ({}): {e}", f.name, f.type_name));
        assert_eq!(back, value, "{} ({}) does not round-trip", f.name, f.type_name);
        assert_eq!(value, f.value, "{} is stale (run `cargo run -p xtask -- codegen`)", f.name);
    }
    let present: BTreeSet<String> = std::fs::read_dir(fixtures_dir())
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n.ends_with(".json"))
        .collect();
    let stray: Vec<_> = present.difference(&expected).collect();
    assert!(stray.is_empty(), "fixtures without a registered type: {stray:?}");
}

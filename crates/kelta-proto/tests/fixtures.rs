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

/// Ticket #135: Claude Code's statusline JSON, with and without `rate_limits` (API-key accounts).
#[test]
fn statusline_fixtures_parse() {
    let read = |name: &str| -> kelta_proto::hooks::HookPayload {
        serde_json::from_str(&std::fs::read_to_string(fixtures_dir().join(name)).unwrap()).unwrap()
    };
    let u = read("hook_statusline.json").usage().unwrap();
    assert_eq!(u.context_pct, Some(72.0));
    assert_eq!(u.cost_usd, 1.8412);
    assert_eq!((u.lines_added, u.lines_removed), (210, 40));
    let five = u.five_hour.unwrap();
    assert_eq!((five.used_percentage, five.resets_at), (64.2, 1_791_637_800));
    assert_eq!(u.seven_day.unwrap().used_percentage, 31.0);
    assert_eq!(u.unsaved_usd, 0.0);

    let api = read("hook_statusline_api_key.json").usage().unwrap();
    assert_eq!((api.five_hour, api.seven_day), (None, None));
    assert_eq!(api.cost_usd, 1.8412);

    // Not a statusline: no usage.
    assert_eq!(read("hook_stop.json").usage(), None);
}

//! Manifest corpus: every file under fixtures/manifests/valid parses, every file under invalid fails
//! with the message announced on its first line (`# expect: <substring>`).

use crate::common;

use std::path::Path;

use kelta_plugins::manifest;

fn files(dir: &str) -> Vec<std::path::PathBuf> {
    let d = Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/manifests").join(dir);
    let mut v: Vec<_> = std::fs::read_dir(d).unwrap().map(|e| e.unwrap().path()).collect();
    v.sort();
    assert!(!v.is_empty());
    v
}

#[test]
fn valid_corpus_parses() {
    for f in files("valid") {
        let bytes = std::fs::read(&f).unwrap();
        let json = f.extension().is_some_and(|e| e == "json");
        let m = manifest::parse(&bytes, json).unwrap_or_else(|e| panic!("{}: {e:?}", f.display()));
        assert!(manifest::compatibility_problems(&m).is_empty(), "{}", f.display());
    }
}

#[test]
fn invalid_corpus_fails_with_reason() {
    for f in files("invalid") {
        let text = std::fs::read_to_string(&f).unwrap();
        let expect = text
            .lines()
            .next()
            .and_then(|l| l.strip_prefix("# expect: "))
            .unwrap_or_else(|| panic!("{}: first line must be `# expect: …`", f.display()));
        let errors =
            manifest::parse(text.as_bytes(), false).expect_err(&format!("{} should be invalid", f.display()));
        assert!(
            errors.iter().any(|e| e.contains(expect)),
            "{}: expected an error containing {expect:?}, got {errors:?}",
            f.display()
        );
    }
}

#[test]
fn example_plugins_are_valid() {
    for name in ["hello-screen", "tools-pack", "json-tracker"] {
        let parsed = manifest::load_dir(&common::example(name)).unwrap();
        assert_eq!(parsed.manifest.id.as_str(), name);
        assert!(parsed.problems.is_empty(), "{name}: {:?}", parsed.problems);
        assert_eq!(parsed.sha256.len(), 64);
    }
}

#[test]
fn incompatible_api_and_platform_are_problems() {
    let env = common::Env::new();
    let other = if cfg!(target_os = "macos") { "linux" } else { "macos" };
    env.write_plugin("future", &common::manifest("future", &[], "").replace("^0.1", "^1.0"), &[]);
    env.write_plugin(
        "elsewhere",
        &common::manifest("elsewhere", &[], &format!("platforms = [\"{other}\"]")),
        &[],
    );
    let reg = env.host.refresh();
    for id in ["future", "elsewhere"] {
        let e = reg.get(id).unwrap();
        assert!(e.parsed.is_some());
        assert!(!e.loadable(), "{id} should not load");
        assert_eq!(e.problems.len(), 1, "{id}: {:?}", e.problems);
    }
}

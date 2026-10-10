//! Install from a directory, a tarball and a git tag; SHA-256 check; uninstall; enable/disable;
//! dev paths; the hello-screen preview used by the UI e2e.

#![allow(clippy::unwrap_used, clippy::expect_used)] // helpers outside #[test] fns unwrap too

mod common;

use std::process::Command;

use kelta_proto::ErrorCode;
use kelta_proto::api::GrantStore;
use kelta_proto::ids::PluginId;

fn hello() -> String {
    common::example("hello-screen").display().to_string()
}

#[tokio::test]
async fn inspect_shows_permissions_in_plain_language_and_install_checks_the_hash() {
    let env = common::Env::new();
    let p = env.host.inspect(&hello()).await.unwrap();
    assert_eq!(p.manifest.id.as_str(), "hello-screen");
    assert_eq!(p.sha256.len(), 64);
    let perms: Vec<(&str, &str)> =
        p.permissions.iter().map(|x| (x.permission.as_str(), x.description.as_str())).collect();
    assert!(perms.contains(&("notify", "Show desktop notifications")));
    assert!(perms.contains(&("events:session.*", "Receive app events matching `session.*`")));

    let e = env.host.install(&hello(), &"0".repeat(64), vec![]).await.unwrap_err();
    assert_eq!(e.code, ErrorCode::Conflict);

    let info =
        env.host.install(&hello(), &p.sha256, vec!["projects.read".into(), "bogus".into()]).await.unwrap();
    assert_eq!(info.granted, vec!["projects.read"], "undeclared grants are dropped");
    assert!(info.enabled);
    assert!(!info.dev);
    assert!(env.plugins_dir().join("hello-screen/dist/kelta-sdk.js").is_file());
    let list = env.host.plugins().await.unwrap();
    assert_eq!(list.len(), 1);

    // Re-inspecting the installed plugin warns that it replaces it.
    let again = env.host.inspect(&hello()).await.unwrap();
    assert!(again.warnings.iter().any(|w| w.starts_with("Replaces the installed version")));

    env.grants.kv_set(&PluginId::new("hello-screen"), "k", "1".into(), 10).await.unwrap();
    env.host.uninstall(&PluginId::new("hello-screen")).await.unwrap();
    assert!(env.host.plugins().await.unwrap().is_empty());
    assert!(env.grants.kv_keys(&PluginId::new("hello-screen")).await.unwrap().is_empty(), "kv cleared");
    assert!(env.grants.grants(&PluginId::new("hello-screen")).await.unwrap().is_empty());
    assert!(!env.plugins_dir().join("hello-screen").exists());
}

#[tokio::test]
async fn hello_screen_preview_matches_the_ui_e2e_fixture() {
    let env = common::Env::new();
    let p = env.host.inspect(&hello()).await.unwrap();
    let path = common::repo_root().join("ui/tests/e2e/plugins/hello-screen.preview.json");
    let json = format!("{}\n", serde_json::to_string_pretty(&p).unwrap());
    if std::env::var_os("KELTA_UPDATE_FIXTURES").is_some() {
        std::fs::write(&path, &json).unwrap();
    }
    // Compared as JSON: the committed file is formatted by prettier (ui lint).
    let committed: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    assert_eq!(
        committed,
        serde_json::to_value(&p).unwrap(),
        "regenerate with KELTA_UPDATE_FIXTURES=1 cargo test -p kelta-plugins --test install (then prettier --write)"
    );
}

#[tokio::test]
async fn installs_from_a_tarball_and_rejects_unsafe_archives() {
    let env = common::Env::new();
    let work = tempfile::tempdir().unwrap();
    let tar = work.path().join("tools-pack.tar.gz");
    let status = Command::new("tar")
        .arg("-czf")
        .arg(&tar)
        .arg("-C")
        .arg(common::repo_root().join("examples/plugins"))
        .arg("tools-pack")
        .status()
        .unwrap();
    assert!(status.success());
    let p = env.host.inspect(tar.to_str().unwrap()).await.unwrap();
    assert_eq!(p.manifest.id.as_str(), "tools-pack");
    assert!(p.warnings.iter().any(|w| w.contains("can run `k9s`")));
    env.host.install(tar.to_str().unwrap(), &p.sha256, vec![]).await.unwrap();
    assert!(env.plugins_dir().join("tools-pack/kelta-plugin.toml").is_file());
    assert!(!env.plugins_dir().join(".staging").read_dir().unwrap().any(|_| true), "staging cleaned");

    // An uncompressed `.tar` is accepted too (tar detects the compression itself).
    let plain = work.path().join("tools-pack.tar");
    let status = Command::new("tar")
        .arg("-cf")
        .arg(&plain)
        .arg("-C")
        .arg(common::repo_root().join("examples/plugins"))
        .arg("tools-pack")
        .status()
        .unwrap();
    assert!(status.success());
    assert_eq!(env.host.inspect(plain.to_str().unwrap()).await.unwrap().sha256, p.sha256);

    // An archive with `..` entries is refused before extraction.
    let evil_src = work.path().join("evil");
    std::fs::create_dir_all(evil_src.join("p")).unwrap();
    std::fs::write(evil_src.join("x.txt"), "x").unwrap();
    let evil = work.path().join("evil.tar.gz");
    let ok = Command::new("tar")
        .current_dir(evil_src.join("p"))
        .arg("-czf")
        .arg(&evil)
        .arg("../x.txt")
        .status()
        .unwrap();
    if ok.success() {
        let e = env.host.inspect(evil.to_str().unwrap()).await.unwrap_err();
        assert_eq!(e.code, ErrorCode::InvalidArgument, "{e:?}");
    }
}

#[tokio::test]
async fn installs_a_git_tag_with_a_shallow_clone() {
    if Command::new("git").arg("--version").output().is_err() {
        eprintln!("skipping: git not installed");
        return;
    }
    let env = common::Env::new();
    let repo = tempfile::tempdir().unwrap();
    let git = |args: &[&str]| {
        let out = Command::new("git")
            .current_dir(repo.path())
            .args([
                "-c",
                "user.email=t@example.com",
                "-c",
                "user.name=t",
                "-c",
                "commit.gpgsign=false",
                "-c",
                "tag.gpgsign=false",
            ])
            .args(args)
            .output()
            .unwrap();
        assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
    };
    git(&["init", "-q"]);
    std::fs::write(repo.path().join("kelta-plugin.toml"), common::manifest("git-plugin", &[], "")).unwrap();
    git(&["add", "."]);
    git(&["commit", "-q", "-m", "v1"]);
    git(&["tag", "v1.0.0"]);
    std::fs::write(
        repo.path().join("kelta-plugin.toml"),
        common::manifest("git-plugin", &[], "").replace("0.1.0\"", "0.2.0\""),
    )
    .unwrap();
    git(&["commit", "-qam", "v2"]);
    let source = format!("file://{}#v1.0.0", repo.path().display());
    let p = env.host.inspect(&source).await.unwrap();
    assert_eq!(p.manifest.version, "0.1.0", "the tag, not the branch head");
    let info = env.host.install(&source, &p.sha256, vec![]).await.unwrap();
    assert_eq!(info.version, "0.1.0");
    assert!(!env.plugins_dir().join("git-plugin/.git").exists());
}

#[tokio::test]
async fn enable_disable_and_dev_paths() {
    let dev = tempfile::tempdir().unwrap();
    std::fs::write(dev.path().join("kelta-plugin.toml"), common::manifest("dev-one", &[], "")).unwrap();
    let dev_path = dev.path().to_path_buf();
    let env = common::Env::new().with_settings(move |s| s.plugins.dev_paths = vec![dev_path]);
    let list = env.host.plugins().await.unwrap();
    assert_eq!(list.len(), 1);
    assert!(list[0].dev);
    let id = PluginId::new("dev-one");
    env.host.enable(&id, false).await.unwrap();
    assert!(!env.host.plugins().await.unwrap()[0].enabled);
    env.host.enable(&id, true).await.unwrap();
    assert!(env.host.plugins().await.unwrap()[0].enabled);
    let e = env.host.uninstall(&id).await.unwrap_err();
    assert_eq!(e.code, ErrorCode::InvalidArgument);
    assert_eq!(env.host.enable(&PluginId::new("nope-x"), true).await.unwrap_err().code, ErrorCode::NotFound);
}

#[tokio::test]
async fn invalid_plugins_are_listed_with_their_problems() {
    let env = common::Env::new();
    env.write_plugin("broken", "id = \"broken\"\nname = 1\n", &[]);
    let list = env.host.plugins().await.unwrap();
    assert_eq!(list.len(), 1);
    assert!(!list[0].problems.is_empty());
    assert!(env.host.inspect(env.plugins_dir().join("broken").to_str().unwrap()).await.is_err());
}

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::collections::BTreeMap;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use kelta_proto::api::{SecretResolver, SettingsSource};
use kelta_proto::error::ErrorCode;
use kelta_proto::ids::AccountId;
use kelta_proto::secret::{SecretCtx, SecretRef};
use kelta_proto::settings::Settings;
use kelta_proto::testing::FakeSettings;
use kelta_secrets::{Secrets, SecretsOptions};

#[path = "support/capture.rs"]
mod capture;

/// Built the way production builds it (`Authed`, KPP providers).
fn secret_ctx(account: Option<AccountId>, base_url: Option<&str>) -> SecretCtx {
    SecretCtx { account, host: base_url.and_then(kelta_http::util::url_host) }
}

fn script(dir: &Path, name: &str, body: &str) -> PathBuf {
    let p = dir.join(name);
    // Written by a child `sh`, never by this process: a write fd opened here is inherited by
    // whatever another test thread forks at that instant, and exec then fails with ETXTBSY (Linux).
    let tmp = dir.join(format!(".{name}.src"));
    std::fs::write(&tmp, format!("#!/bin/sh\n{body}\n")).unwrap();
    let ok = std::process::Command::new("sh")
        .args(["-c", "cat \"$1\" > \"$2\" && chmod 755 \"$2\"", "sh"])
        .args([&tmp, &p])
        .status()
        .unwrap();
    assert!(ok.success());
    p
}

fn fixture(name: &str) -> String {
    std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/glab").join(name))
        .unwrap()
}

struct Rig {
    dir: tempfile::TempDir,
    settings: Arc<FakeSettings>,
    secrets: Arc<Secrets>,
    log: capture::Captured,
}

fn rig_with(extra_env: &[(&str, &str)], timeout: Option<Duration>) -> Rig {
    // every test thread gets a subscriber so callsite interest is never cached as "never"
    let log = capture::install();
    let dir = tempfile::tempdir().unwrap();
    let bin = dir.path().join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    let mut env: BTreeMap<String, String> = BTreeMap::new();
    env.insert("PATH".into(), format!("{}:/usr/bin:/bin", bin.display()));
    env.insert("HOME".into(), dir.path().join("home").display().to_string());
    for (k, v) in extra_env {
        env.insert((*k).into(), (*v).into());
    }
    let settings = FakeSettings::defaults();
    let store: Arc<keyring_core::CredentialStore> = keyring_core::mock::Store::new().unwrap();
    let secrets = Secrets::with_options(
        settings.clone(),
        SecretsOptions { store: Some(store), env: Some(env), timeout, file: Some(secrets_file(dir.path())) },
    );
    Rig { dir, settings, secrets, log }
}

fn rig() -> Rig {
    rig_with(&[], None)
}

fn secrets_file(dir: &Path) -> PathBuf {
    dir.join("data").join(kelta_secrets::SECRETS_FILE)
}

impl Rig {
    fn bin(&self) -> PathBuf {
        self.dir.path().join("bin")
    }

    fn file(&self) -> PathBuf {
        secrets_file(self.dir.path())
    }

    /// A second process run on the same data dir: same file, nothing unlocked.
    fn next_run(&self) -> Arc<Secrets> {
        Secrets::with_options(
            self.settings.clone(),
            SecretsOptions { file: Some(self.file()), ..SecretsOptions::default() },
        )
    }
}

async fn token(r: &Rig, reference: &str, ctx: &SecretCtx) -> String {
    r.secrets.resolve(&SecretRef::new(reference), ctx).await.unwrap().expose().to_owned()
}

#[tokio::test]
async fn env_refs() {
    let r = rig_with(&[("JIRA_TOKEN", "env-token-123")], None);
    assert_eq!(token(&r, "env:JIRA_TOKEN", &SecretCtx::default()).await, "env-token-123");
    let e = r.secrets.resolve(&SecretRef::new("env:NOPE"), &SecretCtx::default()).await.unwrap_err();
    assert_eq!(e.code, ErrorCode::NeedsAuth);
    assert!(e.message.contains("NOPE"));
    let e = r.secrets.resolve(&SecretRef::new("bogus"), &SecretCtx::default()).await.unwrap_err();
    assert_eq!(e.code, ErrorCode::InvalidArgument);
}

#[tokio::test]
async fn command_refs_run_without_a_shell() {
    let r = rig();
    script(&r.bin(), "pass", "[ \"$1\" = show ] && echo \"pw-for-$2\"");
    script(&r.bin(), "chatty", "echo '  '; echo 'line-one-token'; echo 'second'");
    assert_eq!(token(&r, "command:pass show jira/acme", &SecretCtx::default()).await, "pw-for-jira/acme");
    assert_eq!(token(&r, "command:pass show 'two words'", &SecretCtx::default()).await, "pw-for-two words");
    assert_eq!(token(&r, "command:chatty", &SecretCtx::default()).await, "line-one-token");
    // no shell: `$HOME` and `;` are plain argument text
    script(&r.bin(), "echoarg", "echo \"$1\"");
    assert_eq!(token(&r, "command:echoarg $HOME;ls", &SecretCtx::default()).await, "$HOME;ls");
    // absolute paths work too
    let abs = script(r.dir.path(), "abs-tool", "echo abs-token");
    assert_eq!(token(&r, &format!("command:{}", abs.display()), &SecretCtx::default()).await, "abs-token");
}

#[tokio::test]
async fn command_failures_do_not_leak_output() {
    let r = rig();
    script(&r.bin(), "failing", "echo 'leaky-secret-9999'; echo 'leaky-secret-9999' >&2; exit 3");
    script(&r.bin(), "empty", "exit 0");
    let ctx = SecretCtx::default();
    let e = r.secrets.resolve(&SecretRef::new("command:failing"), &ctx).await.unwrap_err();
    assert_eq!(e.code, ErrorCode::NeedsAuth);
    assert!(e.message.contains("exit 3"), "{}", e.message);
    assert!(!format!("{e:?}{e}").contains("leaky-secret-9999"));
    let e = r.secrets.resolve(&SecretRef::new("command:empty"), &ctx).await.unwrap_err();
    assert_eq!(e.code, ErrorCode::NeedsAuth);
    let e = r.secrets.resolve(&SecretRef::new("command:does-not-exist-anywhere"), &ctx).await.unwrap_err();
    assert_eq!(e.code, ErrorCode::Unsupported);
    let e = r.secrets.resolve(&SecretRef::new("command:echo 'unterminated"), &ctx).await.unwrap_err();
    assert_eq!(e.code, ErrorCode::InvalidArgument);
}

#[tokio::test]
async fn command_timeout() {
    let r = rig_with(&[], Some(Duration::from_millis(300)));
    script(&r.bin(), "slow", "sleep 5; echo late");
    let t = Instant::now();
    let e = r.secrets.resolve(&SecretRef::new("command:slow"), &SecretCtx::default()).await.unwrap_err();
    assert_eq!(e.code, ErrorCode::Timeout);
    assert!(t.elapsed() < Duration::from_secs(3));
}

#[tokio::test]
async fn gh_cli_uses_the_account_host() {
    let r = rig();
    script(
        &r.bin(),
        "gh",
        "[ \"$1 $2 $3\" = 'auth token --hostname' ] && echo \"gh-token-for-$4\" || exit 1",
    );
    let ctx = secret_ctx(None, Some("https://api.github.com"));
    assert_eq!(ctx.host.as_deref(), Some("api.github.com"));
    assert_eq!(token(&r, "gh-cli", &ctx).await, "gh-token-for-github.com");
    let ghe = secret_ctx(None, Some("https://ghe.corp.example/api/v3"));
    assert_eq!(token(&r, "gh-cli", &ghe).await, "gh-token-for-ghe.corp.example");
    assert_eq!(token(&r, "gh-cli", &SecretCtx::default()).await, "gh-token-for-github.com");
    // logged out
    script(&r.bin(), "gh", "echo 'not logged in' >&2; exit 1");
    r.secrets.invalidate(&SecretRef::new("gh-cli"));
    let e = r.secrets.resolve(&SecretRef::new("gh-cli"), &ghe).await.unwrap_err();
    assert_eq!(e.code, ErrorCode::NeedsAuth);
    assert!(e.message.contains("ghe.corp.example"));
}

#[tokio::test]
async fn gh_missing_is_reported() {
    let r = rig();
    let e = r.secrets.resolve(&SecretRef::new("gh-cli"), &SecretCtx::default()).await.unwrap_err();
    assert!(matches!(e.code, ErrorCode::Unsupported | ErrorCode::NeedsAuth), "{e:?}");
}

#[tokio::test]
async fn glab_reads_config_yml_fixtures() {
    let r = rig();
    let cfg = r.dir.path().join("glab");
    std::fs::create_dir_all(&cfg).unwrap();
    std::fs::write(cfg.join("config.yml"), fixture("config-plain.yml")).unwrap();
    let r = {
        let d = cfg.display().to_string();
        let rr = rig_with(&[("GLAB_CONFIG_DIR", &d)], None);
        std::fs::write(rr.dir.path().join("unused"), "").unwrap();
        rr
    };
    assert_eq!(
        token(&r, "glab-cli", &secret_ctx(None, Some("https://gitlab.com"))).await,
        "glpat-plain-com-token"
    );
    assert_eq!(
        token(&r, "glab-cli", &secret_ctx(None, Some("https://gitlab.acme.example/api/v4"))).await,
        "glpat-acme-token"
    );
}

#[tokio::test]
async fn glab_falls_back_to_auth_status_for_oauth_configs() {
    let dir = tempfile::tempdir().unwrap();
    let cfg = dir.path().join("cfg");
    std::fs::create_dir_all(&cfg).unwrap();
    std::fs::write(cfg.join("config.yml"), fixture("config-oauth-null.yml")).unwrap();
    let d = cfg.display().to_string();
    let r = rig_with(&[("GLAB_CONFIG_DIR", &d)], None);
    script(
        &r.bin(),
        "glab",
        "[ \"$1 $2 $3 $4\" = 'auth status --show-token --hostname' ] || exit 2\necho \"gitlab.com\" >&2\necho \"  ✓ Logged in to gitlab.com as louis\" >&2\necho \"  ✓ Token found: glpat-from-status\" >&2",
    );
    assert_eq!(
        token(&r, "glab-cli", &secret_ctx(None, Some("https://gitlab.com"))).await,
        "glpat-from-status"
    );
    // not logged in at all
    script(&r.bin(), "glab", "echo 'x No token provided' >&2; exit 1");
    r.secrets.invalidate(&SecretRef::new("glab-cli"));
    let e = r
        .secrets
        .resolve(&SecretRef::new("glab-cli"), &secret_ctx(None, Some("https://gitlab.com")))
        .await
        .unwrap_err();
    assert_eq!(e.code, ErrorCode::NeedsAuth);
}

#[tokio::test]
async fn keyring_set_resolve_delete_with_in_memory_store() {
    let r = rig();
    let jira = SecretRef::new("keyring:jira-acme");
    let ctx = SecretCtx::default();
    let e = r.secrets.resolve(&jira, &ctx).await.unwrap_err();
    assert_eq!(e.code, ErrorCode::NeedsAuth);
    r.secrets.set(&jira, "jira-keyring-token").await.unwrap();
    assert_eq!(r.secrets.resolve(&jira, &ctx).await.unwrap().expose(), "jira-keyring-token");
    // overwrite invalidates the cache
    r.secrets.set(&jira, "rotated").await.unwrap();
    assert_eq!(r.secrets.resolve(&jira, &ctx).await.unwrap().expose(), "rotated");
    r.secrets.delete(&jira).await.unwrap();
    assert_eq!(r.secrets.resolve(&jira, &ctx).await.unwrap_err().code, ErrorCode::NeedsAuth);
    r.secrets.delete(&jira).await.unwrap(); // deleting a missing entry is fine
    // only keyring refs are writable
    for bad in ["env:X", "gh-cli", "command:pass show x"] {
        assert_eq!(
            r.secrets.set(&SecretRef::new(bad), "v").await.unwrap_err().code,
            ErrorCode::InvalidArgument
        );
        assert_eq!(
            r.secrets.delete(&SecretRef::new(bad)).await.unwrap_err().code,
            ErrorCode::InvalidArgument
        );
    }
    assert_eq!(r.secrets.set(&jira, "").await.unwrap_err().code, ErrorCode::InvalidArgument);
}

#[tokio::test]
async fn cache_is_used_and_invalidated() {
    let r = rig();
    let counter = r.dir.path().join("count");
    script(&r.bin(), "counted", &format!("echo run >> {}\necho counted-token", counter.display()));
    let reference = SecretRef::new("command:counted");
    let ctx = SecretCtx::default();
    let runs = || std::fs::read_to_string(&counter).unwrap().lines().count();
    r.secrets.resolve(&reference, &ctx).await.unwrap();
    r.secrets.resolve(&reference, &ctx).await.unwrap();
    assert_eq!(runs(), 1, "second resolve is served from memory");
    // 401 → explicit invalidation
    r.secrets.invalidate(&reference);
    r.secrets.resolve(&reference, &ctx).await.unwrap();
    assert_eq!(runs(), 2);
    // a settings change drops the cache
    let mut s = Settings::defaults();
    s.terminal.font_size = 20.0;
    r.settings.set_global(s);
    r.secrets.resolve(&reference, &ctx).await.unwrap();
    assert_eq!(runs(), 3);
    // different hosts are different entries
    script(&r.bin(), "gh", &format!("echo run >> {}\necho \"t-$4\"", counter.display()));
    let a = secret_ctx(None, Some("https://github.com"));
    let b = secret_ctx(None, Some("https://ghe.example"));
    assert_eq!(token(&r, "gh-cli", &a).await, "t-github.com");
    assert_eq!(token(&r, "gh-cli", &b).await, "t-ghe.example");
    assert_eq!(token(&r, "gh-cli", &a).await, "t-github.com");
    assert_eq!(runs(), 5);
    // the settings Arc is stable between changes
    let _ = r.settings.effective(None);
}

#[tokio::test]
async fn backend_status_lists_every_source() {
    let r = rig();
    script(&r.bin(), "gh", "echo gh");
    let st = r.secrets.backends_status().await;
    let names: Vec<&str> = st.iter().map(|s| s.backend.as_str()).collect();
    assert!(
        names.contains(&"gh-cli")
            && names.contains(&"glab-cli")
            && names.contains(&"command")
            && names.contains(&"env"),
        "{names:?}"
    );
    assert!(st.iter().find(|s| s.backend == "gh-cli").unwrap().available);
    let glab = st.iter().find(|s| s.backend == "glab-cli").unwrap();
    assert!(glab.detail.as_deref().unwrap_or("").contains("glab") || glab.available);
    assert!(st.iter().any(|s| s.backend == "keychain" || s.backend == "secret-service"), "{names:?}");
    let file = st.iter().find(|s| s.backend == "encrypted-file").unwrap();
    assert!(!file.available && file.detail.as_deref().unwrap().contains("not set up"), "{file:?}");
    r.secrets.unlock("pw".into(), true).await.unwrap();
    let st = r.secrets.backends_status().await;
    assert!(st.iter().find(|s| s.backend == "encrypted-file").unwrap().available);
    let st = r.next_run().backends_status().await;
    let file = st.iter().find(|s| s.backend == "encrypted-file").unwrap();
    assert!(!file.available && file.detail.as_deref().unwrap().contains("locked"), "{file:?}");
}

#[tokio::test]
async fn file_round_trip_across_runs_and_wrong_passphrase() {
    let r = rig();
    let jira = SecretRef::new("file:jira-acme");
    let ctx = SecretCtx::default();
    // locked (and absent): resolve and set need the passphrase first
    assert_eq!(r.secrets.resolve(&jira, &ctx).await.unwrap_err().code, ErrorCode::NeedsAuth);
    assert_eq!(r.secrets.set(&jira, "t").await.unwrap_err().code, ErrorCode::NeedsAuth);
    assert_eq!(r.secrets.unlock("pw".into(), false).await.unwrap_err().code, ErrorCode::NotFound);
    assert_eq!(r.secrets.unlock(String::new(), true).await.unwrap_err().code, ErrorCode::InvalidArgument);
    r.secrets.unlock("correct horse".into(), true).await.unwrap();
    assert_eq!(r.secrets.resolve(&jira, &ctx).await.unwrap_err().code, ErrorCode::NeedsAuth);
    r.secrets.set(&jira, "file-token-1").await.unwrap();
    r.secrets.set(&SecretRef::new("file:other"), "file-token-2").await.unwrap();
    assert_eq!(token(&r, "file:jira-acme", &ctx).await, "file-token-1");
    r.secrets.set(&jira, "rotated").await.unwrap();
    assert_eq!(token(&r, "file:jira-acme", &ctx).await, "rotated");
    // 0600, and no plaintext on disk
    let meta = std::fs::metadata(r.file()).unwrap();
    assert_eq!(meta.permissions().mode() & 0o777, 0o600);
    let raw = std::fs::read(r.file()).unwrap();
    assert!(!raw.windows(7).any(|w| w == b"rotated") && !raw.windows(8).any(|w| w == b"jira-acme"));
    // next run: locked until the right passphrase is given; create never overwrites
    let next = r.next_run();
    assert_eq!(next.resolve(&jira, &ctx).await.unwrap_err().code, ErrorCode::NeedsAuth);
    let e = next.unlock("wrong horse".into(), true).await.unwrap_err();
    assert_eq!(e.code, ErrorCode::NeedsAuth);
    assert!(e.message.contains("wrong passphrase"), "{}", e.message);
    assert_eq!(std::fs::read(r.file()).unwrap(), raw);
    next.unlock("correct horse".into(), false).await.unwrap();
    assert_eq!(next.resolve(&jira, &ctx).await.unwrap().expose(), "rotated");
    next.delete(&jira).await.unwrap();
    next.delete(&jira).await.unwrap();
    assert_eq!(next.resolve(&jira, &ctx).await.unwrap_err().code, ErrorCode::NeedsAuth);
    assert_eq!(next.resolve(&SecretRef::new("file:other"), &ctx).await.unwrap().expose(), "file-token-2");
    // without a configured file, file: refs are unsupported
    let e = Secrets::new(r.settings.clone()).resolve(&jira, &ctx).await.unwrap_err();
    assert_eq!(e.code, ErrorCode::Unsupported);
}

#[tokio::test]
async fn tampered_file_is_rejected() {
    let r = rig();
    r.secrets.unlock("pw".into(), true).await.unwrap();
    r.secrets.set(&SecretRef::new("file:a"), "tamper-token").await.unwrap();
    let good = std::fs::read(r.file()).unwrap();
    // magic, salt, nonce, ciphertext, tag: any flipped byte fails authentication
    for i in [0, 9, 30, 50, good.len() - 1] {
        let mut bad = good.clone();
        bad[i] ^= 1;
        std::fs::write(r.file(), &bad).unwrap();
        assert_eq!(
            r.next_run().unlock("pw".into(), false).await.unwrap_err().code,
            ErrorCode::NeedsAuth,
            "byte {i}"
        );
        r.secrets.invalidate(&SecretRef::new("file:a"));
        let e = r.secrets.resolve(&SecretRef::new("file:a"), &SecretCtx::default()).await.unwrap_err();
        assert_eq!(e.code, ErrorCode::NeedsAuth, "byte {i}");
        assert!(!format!("{e:?}").contains("tamper-token"));
    }
    std::fs::write(r.file(), &good[..20]).unwrap();
    assert_eq!(r.next_run().unlock("pw".into(), false).await.unwrap_err().code, ErrorCode::NeedsAuth);
    std::fs::write(r.file(), &good).unwrap();
    assert_eq!(token(&r, "file:a", &SecretCtx::default()).await, "tamper-token");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_file_writes_keep_every_token() {
    let r = rig();
    r.secrets.unlock("pw".into(), true).await.unwrap();
    let writers: Vec<_> = (0..24)
        .map(|i| {
            let s = r.secrets.clone();
            tokio::spawn(async move { s.set(&SecretRef::new(format!("file:t{i}")), &format!("v{i}")).await })
        })
        .collect();
    for w in writers {
        w.await.unwrap().unwrap();
    }
    let next = r.next_run();
    next.unlock("pw".into(), false).await.unwrap();
    for i in 0..24 {
        let v = next.resolve(&SecretRef::new(format!("file:t{i}")), &SecretCtx::default()).await.unwrap();
        assert_eq!(v.expose(), format!("v{i}"));
    }
    // temp files were renamed over the target, none left behind
    let left: Vec<_> = std::fs::read_dir(r.file().parent().unwrap()).unwrap().collect();
    assert_eq!(left.len(), 1, "{left:?}");
}

#[tokio::test]
async fn no_secret_value_reaches_logs_errors_or_serialized_output() {
    let r = rig_with(&[("SECRET_ENV", "env-secret-AAAA1111")], Some(Duration::from_millis(400)));
    script(&r.bin(), "pass", "echo cmd-secret-BBBB2222");
    script(&r.bin(), "gh", "echo gh-secret-CCCC3333");
    script(&r.bin(), "glab", "echo '  Token found: glpat-DDDD4444' >&2");
    script(&r.bin(), "leaker", "echo leak-secret-EEEE5555 >&2; echo leak-secret-EEEE5555; exit 9");
    script(&r.bin(), "slowleak", "echo slow-secret-FFFF6666; sleep 3");
    let ctx = secret_ctx(Some("acct".into()), Some("https://gitlab.com"));
    let mut seen = String::new();
    let jira = SecretRef::new("keyring:jira");
    r.secrets.set(&jira, "keyring-secret-GGGG7777").await.unwrap();
    r.secrets.unlock("passphrase-IIII9999".into(), true).await.unwrap();
    r.secrets.set(&SecretRef::new("file:jira"), "file-secret-HHHH8888").await.unwrap();
    let e = r.next_run().unlock("passphrase-JJJJ0000".into(), false).await.unwrap_err();
    seen.push_str(&format!("{e:?} {e} {}", serde_json::to_string(&e).unwrap()));
    for reference in [
        "env:SECRET_ENV",
        "command:pass",
        "gh-cli",
        "glab-cli",
        "keyring:jira",
        "file:jira",
        "file:missing",
        "command:leaker",
        "command:slowleak",
    ] {
        match r.secrets.resolve(&SecretRef::new(reference), &ctx).await {
            Ok(s) => {
                // Debug is redacted
                seen.push_str(&format!("{s:?}"));
            }
            Err(e) => {
                seen.push_str(&format!("{e:?} {e} {}", serde_json::to_string(&e).unwrap()));
            }
        }
    }
    seen.push_str(
        &serde_json::to_string(
            &r.secrets.backends_status().await.iter().map(|s| format!("{s:?}")).collect::<Vec<_>>(),
        )
        .unwrap(),
    );
    let logs = r.log.contents();
    assert!(!logs.is_empty(), "the resolver does log (kind only)");
    let needles = [
        "env-secret-AAAA1111",
        "cmd-secret-BBBB2222",
        "gh-secret-CCCC3333",
        "glpat-DDDD4444",
        "keyring-secret-GGGG7777",
        "file-secret-HHHH8888",
        "passphrase-IIII9999",
        "passphrase-JJJJ0000",
        "leak-secret-EEEE5555",
        "slow-secret-FFFF6666",
    ];
    for n in needles {
        assert!(!logs.contains(n), "secret {n} leaked into logs:\n{logs}");
        assert!(!seen.contains(n), "secret {n} leaked into errors/debug output:\n{seen}");
    }
}

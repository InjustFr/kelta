//! Kelta's MCP in Claude Code: startup offer, install / remove, silent re-registration in
//! `~/.claude.json` of a fake home, with a fake `claude` that logs any call (none is expected: the
//! token must not reach an argv, #294).

use std::path::Path;
use std::time::Duration;

use crate::common::*;
use kelta_proto::events::UiEvent;
use kelta_proto::settings::Settings;

fn config(root: &Path) -> std::path::PathBuf {
    root.join("home/.claude.json")
}

/// A `claude` that logs each call to `claude.log`.
fn fake_claude(root: &Path) {
    let script = format!("#!/bin/sh\necho \"$*\" >> '{}'\n", root.join("claude.log").display());
    // Written by a child `sh` (see `bin_dir`): no write fd of ours leaks into a concurrent fork.
    let ok = std::process::Command::new("sh")
        .args(["-c", "printf '%s' \"$1\" > \"$2\" && chmod 755 \"$2\"", "sh", &script])
        .arg(root.join("bin/claude"))
        .status()
        .unwrap();
    assert!(ok.success());
}

/// The registered `(url, Authorization)`.
fn registered(root: &Path) -> (String, String) {
    let v: serde_json::Value = serde_json::from_slice(&std::fs::read(config(root)).unwrap()).unwrap();
    let e = &v["mcpServers"]["kelta"];
    assert_eq!(e["type"], "http");
    (e["url"].as_str().unwrap().to_owned(), e["headers"]["Authorization"].as_str().unwrap().to_owned())
}

fn log(root: &Path) -> Vec<String> {
    std::fs::read_to_string(root.join("claude.log")).unwrap_or_default().lines().map(str::to_owned).collect()
}

fn offers(h: &H) -> usize {
    h.ui.take_events()
        .iter()
        .filter(|e| matches!(e, UiEvent::Toast { toast } if toast.text.starts_with("Claude Code detected")))
        .count()
}

#[tokio::test]
async fn offer_install_and_silent_reregistration() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    let h = start(root, Settings::defaults(), vec![]);
    fake_claude(root);

    // Found, not registered: the offer, with Install and Never.
    h.core.claude_mcp_startup().await;
    let toast = h.ui.events().into_iter().find_map(|e| match e {
        UiEvent::Toast { toast } => Some(toast),
        _ => None,
    });
    let toast = toast.unwrap();
    assert_eq!(toast.action.unwrap().command, "claude.mcp_install");
    assert_eq!(toast.secondary.unwrap().command, "claude.mcp_never");
    assert_eq!(h.core.claude_mcp_status().await, Some(false));
    h.ui.take_events();

    // Never is the `claude.mcp_offer` setting.
    h.cfg.update(|s| s.claude.mcp_offer = false);
    h.core.claude_mcp_startup().await;
    assert_eq!(offers(&h), 0);

    // Install: user scope, global endpoint, install token, written without running `claude`.
    h.core.claude_mcp_install().await.unwrap();
    let port = h.core.server().http_port().unwrap();
    let (url, auth) = registered(root);
    assert_eq!(url, format!("http://127.0.0.1:{port}/mcp"));
    let token = auth.strip_prefix("Bearer ").unwrap_or_else(|| panic!("{auth}"));
    assert!(token.len() >= 32);
    assert_eq!(h.core.claude_mcp_status().await, Some(true));
    let ping = |tok: &str| {
        kelta_http::client_builder()
            .no_proxy()
            .build()
            .unwrap()
            .post(format!("http://127.0.0.1:{port}/mcp"))
            .bearer_auth(tok)
            .json(&serde_json::json!({"jsonrpc":"2.0","id":1,"method":"ping"}))
            .send()
    };
    assert_eq!(ping(token).await.unwrap().status(), 200);
    assert_eq!(ping("other").await.unwrap().status(), 401);

    // Up to date: left as it is.
    let before = std::fs::read(config(root)).unwrap();
    h.core.claude_mcp_startup().await;
    assert_eq!(std::fs::read(config(root)).unwrap(), before);

    // Stale port: updated without asking, keeping the token Claude Code holds.
    std::fs::write(
        config(root),
        r#"{"numStartups":3,"mcpServers":{"kelta":{"type":"http","url":"http://127.0.0.1:1/mcp","headers":{"Authorization":"Bearer old"}}}}"#,
    )
    .unwrap();
    h.cfg.update(|s| s.claude.mcp_offer = true);
    h.core.claude_mcp_startup().await;
    assert_eq!(offers(&h), 0);
    assert_eq!(registered(root), (url.clone(), "Bearer old".to_owned()));
    assert_eq!(ping("old").await.unwrap().status(), 200);

    // The token lives in the registration only: no Keychain access at all (#275).
    assert!(h.secrets.resolved().is_empty(), "{:?}", h.secrets.resolved());
    assert!(h.secrets.invalidated().is_empty());

    // Remove: unregistered (other keys kept), endpoint closed.
    h.core.claude_mcp_remove().await.unwrap();
    let v: serde_json::Value = serde_json::from_slice(&std::fs::read(config(root)).unwrap()).unwrap();
    assert_eq!(v, serde_json::json!({"mcpServers": {}, "numStartups": 3}));
    assert_eq!(h.core.claude_mcp_status().await, Some(false));
    assert_eq!(h.core.server().http_port(), None);
    // Nothing written to, read from or deleted in the Keychain.
    assert!(h.secrets.resolved().is_empty(), "{:?}", h.secrets.resolved());
    let r = kelta_proto::secret::SecretRef::new("keyring:kelta-mcp");
    let ctx = kelta_proto::secret::SecretCtx::default();
    assert!(kelta_proto::api::SecretResolver::resolve(&*h.secrets, &r, &ctx).await.is_err());
    // The token never reached an argv: `claude` did not run.
    assert!(log(root).is_empty(), "{:?}", log(root));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn concurrent_installs_then_remove_close_the_endpoint() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    let h = start(root, Settings::defaults(), vec![]);
    fake_claude(root);
    // Toast Install and the Settings button at once: one server reference, one token. The unguarded
    // check-then-take window has no await, so only two threads released together hit it; rounds make
    // a regression show on every run (#321).
    let rt = tokio::runtime::Handle::current();
    for round in 0..20 {
        let gate = std::sync::Barrier::new(2);
        std::thread::scope(|s| {
            for _ in 0..2 {
                s.spawn(|| {
                    gate.wait();
                    rt.block_on(h.core.claude_mcp_install()).unwrap();
                });
            }
        });
        assert!(h.core.server().http_port().is_some());
        h.core.claude_mcp_remove().await.unwrap();
        assert_eq!(h.core.server().http_port(), None, "round {round}: a server reference leaked");
    }
    assert_eq!(h.core.claude_mcp_status().await, Some(false));
}

#[tokio::test]
async fn no_offer_without_claude() {
    let tmp = tempfile::tempdir().unwrap();
    let mut s = Settings::defaults();
    s.claude.binary = "no-such-claude".into();
    let h = start(tmp.path(), s, vec![]);
    h.core.claude_mcp_startup().await;
    assert_eq!(offers(&h), 0);
    assert_eq!(h.core.claude_mcp_status().await, None);
    assert!(h.core.claude_mcp_install().await.is_err());
}

/// Whether `kelta` is in the config.
fn has_entry(root: &Path) -> bool {
    let v: serde_json::Value = serde_json::from_slice(&std::fs::read(config(root)).unwrap()).unwrap();
    v["mcpServers"].get("kelta").is_some()
}

/// A write by a concurrent Claude Code save is repaired by the check armed 5 s after the install;
/// Remove cancels it.
#[tokio::test(start_paused = true)]
async fn recheck_restores_a_dropped_entry_and_remove_cancels_it() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    let h = start(root, Settings::defaults(), vec![]);
    fake_claude(root);
    h.core.claude_mcp_install().await.unwrap();
    let before = registered(root);

    std::fs::write(config(root), r#"{"mcpServers":{}}"#).unwrap();
    settle().await; // the timer task starts its sleep before the clock moves
    tokio::time::advance(Duration::from_secs(6)).await;
    // The check reads and writes through the blocking pool: let it finish.
    for _ in 0..200 {
        if has_entry(root) {
            break;
        }
        settle().await;
        std::thread::sleep(Duration::from_millis(5));
    }
    assert_eq!(registered(root), before);

    // Re-armed by nothing: a second drop is not repaired once Remove ran.
    h.core.claude_mcp_install().await.unwrap();
    h.core.claude_mcp_remove().await.unwrap();
    std::fs::write(config(root), r#"{"mcpServers":{}}"#).unwrap();
    settle().await; // the timer task starts its sleep before the clock moves
    tokio::time::advance(Duration::from_secs(6)).await;
    for _ in 0..20 {
        settle().await;
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(!has_entry(root));
}

#[tokio::test]
async fn setup_registers_the_mcp_then_starts_claude_in_home() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    let h = start(root, Settings::defaults(), vec![]);
    fake_claude(root);
    let info = h.core.claude_setup().await.unwrap();
    assert_eq!(h.core.claude_mcp_status().await, Some(true));
    assert!(matches!(info.kind, kelta_proto::model::SessionKind::Claude));
    let home = kelta_proto::ids::ProjectId::home();
    assert_eq!(info.project_id, home);
    let prompt = h.term.with_session(&info.id, |s| s.spec.args.last().cloned()).unwrap().unwrap();
    assert!(prompt.contains("get_setup") && prompt.contains("request_token"), "{prompt}");
    let layout = h.core.layout_get(&home).unwrap();
    let tab = layout.tabs.iter().find(|t| t.title == "Set up Kelta").unwrap();
    assert_eq!(layout.active_tab.as_ref(), Some(&tab.id), "the setup tab takes the focus");
}

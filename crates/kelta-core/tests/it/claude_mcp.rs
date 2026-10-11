//! Kelta's MCP in Claude Code: startup offer, install / remove, silent re-registration, with a fake
//! `claude` that keeps its user-scope registration in `~/.claude.json` of a fake home.

use std::path::Path;

use crate::common::*;
use kelta_proto::events::UiEvent;
use kelta_proto::settings::Settings;

fn config(root: &Path) -> std::path::PathBuf {
    root.join("home/.claude.json")
}

/// `claude mcp add|remove kelta`, logging each call to `claude.log`.
fn fake_claude(root: &Path) {
    let reg = config(root);
    let log = root.join("claude.log");
    let script = format!(
        r#"#!/bin/sh
echo "$*" >> '{log}'
case "$2" in
  remove) [ -f '{reg}' ] && rm '{reg}' && exit 0; exit 1 ;;
  add) mkdir -p "$(dirname '{reg}')"; h="${{10}}"
    printf '{{"mcpServers":{{"kelta":{{"type":"http","url":"%s","headers":{{"Authorization":"%s"}}}}}}}}' "$8" "${{h#Authorization: }}" > '{reg}' ;;
esac
"#,
        log = log.display(),
        reg = reg.display()
    );
    // Written by a child `sh` (see `bin_dir`): no write fd of ours leaks into a concurrent fork.
    let ok = std::process::Command::new("sh")
        .args(["-c", "printf '%s' \"$1\" > \"$2\" && chmod 755 \"$2\"", "sh", &script])
        .arg(root.join("bin/claude"))
        .status()
        .unwrap();
    assert!(ok.success());
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

    // Install: user scope, global endpoint, install token.
    h.core.claude_mcp_install().await.unwrap();
    let add = log(root).into_iter().find(|l| l.starts_with("mcp add")).unwrap();
    let port = h.core.server().http_port().unwrap();
    let prefix = format!(
        "mcp add --scope user --transport http kelta http://127.0.0.1:{port}/mcp --header Authorization: Bearer "
    );
    let token = add.strip_prefix(&prefix).unwrap_or_else(|| panic!("{add}"));
    assert_eq!(h.core.claude_mcp_status().await, Some(true));
    let ping = |tok: &str| {
        reqwest::Client::builder()
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

    // Up to date: read from the config file, `claude` never runs.
    let calls = log(root).len();
    h.core.claude_mcp_startup().await;
    assert!(log(root)[calls..].is_empty(), "{:?}", &log(root)[calls..]);

    // Stale port: updated without asking, keeping the token Claude Code holds.
    std::fs::write(
        config(root),
        r#"{"mcpServers":{"kelta":{"type":"http","url":"http://127.0.0.1:1/mcp","headers":{"Authorization":"Bearer old"}}}}"#,
    )
    .unwrap();
    let calls = log(root).len();
    h.cfg.update(|s| s.claude.mcp_offer = true);
    h.core.claude_mcp_startup().await;
    assert_eq!(offers(&h), 0);
    let tail = &log(root)[calls..];
    assert_eq!(tail, ["mcp remove --scope user kelta".to_owned(), format!("{prefix}old")]);
    assert_eq!(ping("old").await.unwrap().status(), 200);

    // The token lives in the registration only: no Keychain access at all (#275).
    assert!(h.secrets.resolved().is_empty(), "{:?}", h.secrets.resolved());
    assert!(h.secrets.invalidated().is_empty());

    // Remove: unregistered, endpoint closed.
    h.core.claude_mcp_remove().await.unwrap();
    assert_eq!(log(root).last().unwrap(), "mcp remove --scope user kelta");
    assert_eq!(h.core.claude_mcp_status().await, Some(false));
    assert_eq!(h.core.server().http_port(), None);
    // Nothing written to, read from or deleted in the Keychain.
    assert!(h.secrets.resolved().is_empty(), "{:?}", h.secrets.resolved());
    let r = kelta_proto::secret::SecretRef::new("keyring:kelta-mcp");
    let ctx = kelta_proto::secret::SecretCtx::default();
    assert!(kelta_proto::api::SecretResolver::resolve(&*h.secrets, &r, &ctx).await.is_err());
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

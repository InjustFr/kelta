//! `cargo run -p xtask -- kpp-check <plugin dir> [--account <json>] [--secret <value>]`
//!
//! Runs the provider conformance contract (`kelta_proto::testing::conformance`, the one every
//! built-in tracker and code host passes) against a process (KPP) plugin. `--account` adds account
//! fields (`base_url`, `user`, …); `--secret` is handed to the plugin as the account secret.
//! The contract writes (transition, comment, assign, create): point the plugin at test data.

use std::path::Path;
use std::sync::Arc;

use anyhow::{Context, Result, anyhow, bail};
use kelta_plugins::kpp::{KppCodeHost, KppProcess, KppTracker, Source};
use kelta_proto::codehost::CodeHostKind;
use kelta_proto::ext::ProviderKind;
use kelta_proto::ids::AccountId;
use kelta_proto::settings::{AccountConfig, TrackerBinding, TrackerView};
use kelta_proto::testing::FakeSecrets;
use kelta_proto::testing::conformance::{CodeHostCase, TrackerCase, code_host_contract, tracker_contract};
use kelta_proto::tracker::TrackerKind;
use serde_json::{Value, json};

const ACCOUNT: &str = "kpp-check";

pub fn run(args: &[String]) -> Result<()> {
    let mut dir = None;
    let mut account = json!({});
    let mut secret = None;
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--account" => {
                account = serde_json::from_str(it.next().context("--account needs a JSON object")?)
                    .context("--account")?;
            }
            "--secret" => secret = Some(it.next().context("--secret needs a value")?.clone()),
            other if dir.is_none() => dir = Some(other.to_owned()),
            other => bail!("unexpected argument `{other}`"),
        }
    }
    let dir = dir.context("usage: kpp-check <plugin dir> [--account <json>] [--secret <value>]")?;
    check(Path::new(&dir), account, secret)?;
    println!("kpp-check: {dir} passes the provider contract");
    Ok(())
}

pub fn check(dir: &Path, extra: Value, secret: Option<String>) -> Result<()> {
    let dir = dir.canonicalize().with_context(|| format!("{}", dir.display()))?;
    let parsed = kelta_plugins::manifest::load_dir(&dir)?;
    let def = parsed.manifest.provider.clone().context("the manifest has no [provider]")?;
    let kind = match def.kind {
        ProviderKind::Tracker => "plugin_tracker",
        ProviderKind::Codehost => "plugin_codehost",
    };
    let mut v = json!({ "kind": kind, "plugin": parsed.manifest.id });
    if secret.is_some() {
        v["secret"] = json!("kpp-check");
    }
    for (k, val) in extra.as_object().context("--account must be a JSON object")? {
        v[k] = val.clone();
    }
    let account: AccountConfig = serde_json::from_value(v).context("account")?;
    let secrets = FakeSecrets::with(&[("kpp-check", secret.as_deref().unwrap_or(""))]);
    let proc_ = Arc::new(KppProcess::new(&def, &dir, &parsed.sha256, None));
    let source = Source::Direct(proc_.clone());
    let id = AccountId::new(ACCOUNT);

    let rt = tokio::runtime::Builder::new_current_thread().enable_all().build()?;
    let result = rt.block_on(async {
        match def.kind {
            ProviderKind::Tracker => {
                let t = KppTracker::new(source, def.clone(), id, account, secrets);
                let case = TrackerCase {
                    kind: TrackerKind::Plugin,
                    account_id: ACCOUNT.into(),
                    view: TrackerView { id: "mine".into(), label: "Mine".into(), ..TrackerView::default() },
                    binding: TrackerBinding::default(),
                    ticket: None,
                    branch_key: None,
                };
                tracker_contract(&t, &case).await
            }
            ProviderKind::Codehost => {
                let h = KppCodeHost::new(source, def.clone(), id, account, secrets);
                let case = CodeHostCase {
                    kind: CodeHostKind::Plugin,
                    account_id: ACCOUNT.into(),
                    repo: None,
                    refspec: None,
                };
                code_host_contract(&h, &case).await
            }
        }
    });
    let log = proc_.log_tail(40);
    proc_.kill();
    result
        .map_err(|e| if log.is_empty() { anyhow!(e) } else { anyhow!("{e}\n--- provider stderr ---\n{log}") })
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_example_plugin_passes() {
        if which_node().is_none() {
            eprintln!("skipped: node not found");
            return;
        }
        let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("../examples/plugins/json-tracker");
        let tmp = tempfile::tempdir().unwrap();
        for f in ["kelta-plugin.toml", "tracker.cjs", "tickets.json"] {
            std::fs::copy(src.join(f), tmp.path().join(f)).unwrap();
        }
        super::check(tmp.path(), serde_json::json!({}), None).unwrap();
        // the writes landed in the copy
        let db = std::fs::read_to_string(tmp.path().join("tickets.json")).unwrap();
        assert!(db.contains("hello\\nworld"));
    }

    fn which_node() -> Option<()> {
        std::process::Command::new("node")
            .arg("--version")
            .output()
            .ok()
            .filter(|o| o.status.success())
            .map(|_| ())
    }

    use std::path::Path;
}

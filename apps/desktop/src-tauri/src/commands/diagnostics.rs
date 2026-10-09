//! `diagnostics` commands — diagnostics probes (owner: L10, ARCHITECTURE §6). The probes live in
//! `platform::probes`; this only gathers their inputs.

use std::sync::Arc;

use kelta_core::Core;
use kelta_proto::api::{CoreApi, SettingsSource};
use kelta_proto::ctl::CTL_SOCKET_NAME;
use kelta_proto::prelude::*;
use tauri::State;

use super::Res;
use crate::platform::probes;

#[tauri::command(rename_all = "snake_case")]
pub async fn diagnostics_run(core: State<'_, Arc<Core>>) -> Res<Diagnostics> {
    let settings = core.config().effective(None);
    let home = std::env::var_os("HOME").map(std::path::PathBuf::from);
    let dirs = probes::search_dirs(std::env::var("PATH").ok().as_deref(), home.as_deref());

    let claude: Vec<_> = core.session_list(None).into_iter().filter_map(|s| s.claude).collect();
    let inactive = claude.iter().filter(|c| !c.hooks_active).count();

    let secrets = core.secret_resolver();
    let (backends, notifications) = tokio::join!(secrets.backends_status(), probes::notification_daemon());
    let mut checks =
        vec![probes::graphics(), probes::webkit(), notifications, probes::secret_backends(&backends)];

    let claude_bin = settings.claude.binary.clone();
    for (name, required, min) in [
        (claude_bin.as_str(), true, Some(settings.claude.min_version.as_str())),
        ("nvim", true, None),
        ("git", true, None),
        ("gh", false, None),
        ("glab", false, None),
    ] {
        checks.push(probes::tool(name, required, min, &dirs).await.0);
    }
    checks.push(probes::hooks(claude.len(), inactive));
    checks.push(probes::sockets(&core.dirs().runtime.join(CTL_SOCKET_NAME), &core.dirs().runtime));
    Ok(Diagnostics { checks })
}

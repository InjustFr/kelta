//! `settings` commands — settings layers, validation, repo trust, account test (owner: L4, ARCHITECTURE §6).
//!
//! Reads go straight to the in-memory `ConfigService`; writes (small, fsynced files) run on the
//! blocking pool. Nothing here ever handles a secret value: accounts only carry `SecretRef`s.

use std::path::PathBuf;
use std::sync::Arc;

use kelta_core::Core;
use kelta_proto::prelude::*;
use kelta_proto::redact::redact_text;
use serde_json::Value;
use tauri::State;

use super::Res;

/// Run a blocking config operation off the async workers.
async fn blocking<T, F>(f: F) -> Res<T>
where
    T: Send + 'static,
    F: FnOnce() -> Res<T> + Send + 'static,
{
    tokio::task::spawn_blocking(f)
        .await
        .map_err(|e| KeltaError::internal(format!("settings task failed: {e}")))?
}

#[tauri::command(rename_all = "snake_case")]
pub async fn settings_schema(core: State<'_, Arc<Core>>) -> Res<Value> {
    Ok(core.config().schema_full())
}

#[tauri::command(rename_all = "snake_case")]
pub async fn settings_effective(
    core: State<'_, Arc<Core>>,
    project_id: Option<ProjectId>,
) -> Res<EffectiveSettings> {
    core.config().effective_doc(project_id.as_ref())
}

#[tauri::command(rename_all = "snake_case")]
pub async fn settings_layer_get(
    core: State<'_, Arc<Core>>,
    layer: Layer,
    project_id: Option<ProjectId>,
    repo_id: Option<String>,
) -> Res<LayerDoc> {
    core.config().layer_get(layer, project_id.as_ref(), repo_id.as_deref())
}

#[tauri::command(rename_all = "snake_case")]
pub async fn settings_set(
    core: State<'_, Arc<Core>>,
    layer: Layer,
    project_id: Option<ProjectId>,
    repo_id: Option<String>,
    path: String,
    value: Value,
) -> Res<EffectiveSettings> {
    let cfg = core.config().clone();
    blocking(move || cfg.layer_set(layer, project_id.as_ref(), repo_id.as_deref(), &path, value)).await
}

#[tauri::command(rename_all = "snake_case")]
pub async fn settings_reset(
    core: State<'_, Arc<Core>>,
    layer: Layer,
    project_id: Option<ProjectId>,
    repo_id: Option<String>,
    path: String,
) -> Res<EffectiveSettings> {
    let cfg = core.config().clone();
    blocking(move || cfg.layer_reset(layer, project_id.as_ref(), repo_id.as_deref(), &path)).await
}

#[tauri::command(rename_all = "snake_case")]
pub async fn settings_validate(
    core: State<'_, Arc<Core>>,
    layer: Layer,
    text: String,
) -> Res<Vec<ValidationIssue>> {
    core.config().layer_validate(layer, &text)
}

#[tauri::command(rename_all = "snake_case")]
pub async fn settings_write_raw(
    core: State<'_, Arc<Core>>,
    layer: Layer,
    project_id: Option<ProjectId>,
    repo_id: Option<String>,
    text: String,
) -> Res<EffectiveSettings> {
    let cfg = core.config().clone();
    blocking(move || cfg.layer_write_raw(layer, project_id.as_ref(), repo_id.as_deref(), &text)).await
}

/// Open the layer file in `$VISUAL` / `$EDITOR` (default `nvim`) in a new tab.
#[tauri::command(rename_all = "snake_case")]
pub async fn settings_open_file(
    core: State<'_, Arc<Core>>,
    layer: Layer,
    project_id: Option<ProjectId>,
    repo_id: Option<String>,
) -> Res<SessionInfo> {
    let doc = core.config().layer_get(layer, project_id.as_ref(), repo_id.as_deref())?;
    if doc.path.as_os_str().is_empty() {
        return Err(KeltaError::invalid("this layer has no file"));
    }
    let path: PathBuf = doc.path;
    let dir = path.parent().map(PathBuf::from).unwrap_or_default();
    std::fs::create_dir_all(&dir)?;
    let editor = std::env::var("VISUAL")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .or_else(|| std::env::var("EDITOR").ok().filter(|s| !s.trim().is_empty()))
        .unwrap_or_else(|| "nvim".to_owned());
    let mut words = editor.split_whitespace().map(str::to_owned);
    let program = words.next().unwrap_or_else(|| "nvim".to_owned());
    let mut args: Vec<String> = words.collect();
    args.push(path.display().to_string());
    let project = project_id.unwrap_or_else(|| ProjectId::new("home"));
    let adapter = std::path::Path::new(&program)
        .file_name()
        .map_or_else(|| "nvim".to_owned(), |n| n.to_string_lossy().into_owned());
    let title = path.file_name().map_or_else(|| "config".to_owned(), |n| n.to_string_lossy().into_owned());
    let info = core
        .session_spawn(SpawnRequest {
            id: None,
            project_id: project.clone(),
            kind: SessionKind::Editor { adapter },
            name: Some(title.clone()),
            program: Some(program),
            args,
            cwd: Some(dir),
            env: Default::default(),
            cols: 100,
            rows: 30,
            work_item_id: None,
            restore: RestorePolicy::None,
            close_on_exit: CloseOnExit::OnSuccess,
            template_id: None,
        })
        .await?;
    core.layout_open(
        &project,
        OpenPaneRequest {
            content: PaneContent::Terminal { session_id: info.id.clone() },
            placement: Placement::NewTab,
            focus: true,
            tab_title: Some(title),
            work_item_id: None,
        },
    )
    .await?;
    Ok(info)
}

#[tauri::command(rename_all = "snake_case")]
pub async fn repo_trust(
    core: State<'_, Arc<Core>>,
    project_id: ProjectId,
    repo_id: String,
    trust: bool,
    sha256: Option<String>,
) -> Res<TrustInfo> {
    core.config().repo_trust(&project_id, &repo_id, trust, sha256.as_deref()).await
}

/// Resolve the account's provider through the core (so the cached provider / secret chain is
/// exercised exactly as in normal use) and call `me()`. Provider failures are data, not errors.
#[tauri::command(rename_all = "snake_case")]
pub async fn account_test(core: State<'_, Arc<Core>>, account_id: AccountId) -> Res<AccountTestResult> {
    let settings = core.config().effective(None);
    let Some(account) = settings.accounts.get(&account_id) else {
        return Ok(AccountTestResult {
            ok: false,
            user: None,
            error: Some(KeltaError::not_found(format!("unknown account `{account_id}`"))),
        });
    };
    // a token typed seconds ago must win over a cached failure
    if let Some(secret) = account.effective_secret() {
        core.secret_resolver().invalidate(&secret);
    }
    let api: &Core = &core;
    let result = if account.kind.is_code_host() {
        match api.code_host_for(&account_id).await {
            Ok(host) => host.me().await,
            Err(e) => Err(e),
        }
    } else {
        match api.tracker_for(&account_id).await {
            Ok(tracker) => tracker.me().await,
            Err(e) => Err(e),
        }
    };
    Ok(match result {
        Ok(user) => AccountTestResult { ok: true, user: Some(user), error: None },
        Err(mut e) => {
            e.message = redact_text(&e.message);
            AccountTestResult { ok: false, user: None, error: Some(e) }
        }
    })
}

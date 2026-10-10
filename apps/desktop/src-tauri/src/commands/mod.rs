//! Tauri command registry (ARCHITECTURE §6). Domain files are owned by lanes; this file is
//! scaffold-owned. Every command is `async`, takes snake_case arguments and returns
//! `Result<T, KeltaError>` (the IPC error shape).

use kelta_proto::error::KeltaError;
use kelta_proto::ids::SessionId;
use serde_json::Value;
use tauri::ipc::InvokeBody;

pub mod app;
pub mod clipboard;
pub mod diagnostics;
pub mod editor;
pub mod layout;
pub mod names;
pub mod plugin;
pub mod project;
pub mod review;
pub mod secrets;
pub mod session;
pub mod settings;
pub mod tool;
pub mod tracker;
pub mod trigger;
pub mod work;

/// Command result.
pub type Res<T> = Result<T, KeltaError>;

/// Header carrying the session id when `session_write` is invoked with a raw `Uint8Array` body.
pub const SESSION_ID_HEADER: &str = "x-kelta-session-id";

/// Decode `session_write` input. Accepted shapes:
/// - raw body (`invoke('session_write', bytes, { headers: { 'x-kelta-session-id': id } })`)
/// - JSON `{ id, data }` where `data` is a number array, an index-keyed object (a serialized
///   `Uint8Array`) or a UTF-8 string.
pub fn parse_session_write(request: &tauri::ipc::Request<'_>) -> Res<(SessionId, Vec<u8>)> {
    let header = request.headers().get(SESSION_ID_HEADER).and_then(|v| v.to_str().ok());
    parse_write_body(request.body(), header)
}

/// Pure part of [`parse_session_write`].
pub fn parse_write_body(body: &InvokeBody, header_id: Option<&str>) -> Res<(SessionId, Vec<u8>)> {
    match body {
        InvokeBody::Raw(bytes) => {
            let id = header_id
                .filter(|s| !s.is_empty())
                .ok_or_else(|| KeltaError::invalid(format!("missing {SESSION_ID_HEADER} header")))?;
            Ok((SessionId::new(id), bytes.clone()))
        }
        InvokeBody::Json(v) => {
            let id = v
                .get("id")
                .and_then(Value::as_str)
                .or(header_id)
                .ok_or_else(|| KeltaError::invalid("session_write: missing `id`"))?;
            let data = match v.get("data") {
                Some(Value::Array(a)) => bytes_from_iter(a.iter())?,
                Some(Value::Object(m)) => {
                    let mut out = Vec::with_capacity(m.len());
                    for i in 0..m.len() {
                        let b = m
                            .get(&i.to_string())
                            .ok_or_else(|| KeltaError::invalid("session_write: sparse byte object"))?;
                        out.push(byte(b)?);
                    }
                    out
                }
                Some(Value::String(s)) => s.as_bytes().to_vec(),
                _ => return Err(KeltaError::invalid("session_write: missing `data`")),
            };
            Ok((SessionId::new(id), data))
        }
    }
}

fn byte(v: &Value) -> Res<u8> {
    v.as_u64()
        .and_then(|n| u8::try_from(n).ok())
        .ok_or_else(|| KeltaError::invalid("session_write: data must be bytes"))
}

fn bytes_from_iter<'a>(it: impl Iterator<Item = &'a Value>) -> Res<Vec<u8>> {
    it.map(byte).collect()
}

/// The invoke handler with every command of ARCHITECTURE §6.
pub fn handler() -> impl Fn(tauri::ipc::Invoke<tauri::Wry>) -> bool + Send + Sync + 'static {
    tauri::generate_handler![
        // app
        app::app_info,
        app::app_ready,
        app::events_subscribe,
        app::open_external,
        app::perf_snapshot,
        diagnostics::diagnostics_run,
        clipboard::clipboard_read,
        clipboard::clipboard_write,
        app::notify_test,
        // settings
        settings::settings_schema,
        settings::settings_effective,
        settings::settings_layer_get,
        settings::settings_set,
        settings::settings_reset,
        settings::settings_validate,
        settings::settings_write_raw,
        settings::settings_open_file,
        settings::repo_trust,
        secrets::secret_set,
        secrets::secret_delete,
        secrets::secret_backends_status,
        secrets::secret_unlock,
        settings::account_test,
        // projects
        project::project_list,
        project::project_detect,
        project::project_create,
        project::project_update,
        project::project_remove,
        project::project_open,
        project::project_close,
        project::project_activate,
        project::project_reorder,
        // layout
        layout::layout_get,
        layout::layout_save,
        // sessions
        session::session_spawn,
        session::session_spawn_template,
        session::session_attach,
        session::session_detach,
        session::session_write,
        session::session_resize,
        session::session_ack,
        session::session_kill,
        session::session_restart,
        session::session_rename,
        session::session_list,
        session::session_mark_seen,
        session::session_link,
        session::session_text_tail,
        session::session_history_search,
        session::terminal_set_palette,
        // tickets
        tracker::tracker_list,
        tracker::tracker_get,
        tracker::tracker_columns,
        tracker::tracker_transitions,
        tracker::tracker_transition,
        tracker::tracker_move,
        tracker::tracker_comment,
        tracker::tracker_assign,
        tracker::tracker_search,
        // reviews
        review::review_list,
        review::review_get,
        review::review_approve,
        review::review_comment,
        review::review_request_changes,
        // work
        work::work_plan,
        work::work_start,
        work::work_list,
        work::work_resume,
        work::work_retry_step,
        work::work_create_pr,
        work::work_finish,
        work::work_status,
        work::work_status_all,
        work::work_diff,
        work::work_mark_reviewed,
        editor::editor_open,
        editor::editor_send_selection,
        // tools / plugins / triggers
        tool::tool_list,
        tool::tool_check,
        tool::tool_open,
        tool::tool_close,
        plugin::plugin_list,
        plugin::plugin_inspect,
        plugin::plugin_install,
        plugin::plugin_uninstall,
        plugin::plugin_enable,
        plugin::plugin_grant,
        plugin::plugin_screen_open,
        plugin::plugin_screen_close,
        plugin::plugin_call,
        plugin::command_run,
        trigger::trigger_list,
        trigger::trigger_test,
        trigger::trigger_log,
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn handler_lists_every_command_once() {
        let src = include_str!("mod.rs");
        let start = src.find("generate_handler![").unwrap();
        let end = start + src[start..].find(']').unwrap();
        let listed: Vec<&str> = src[start..end]
            .lines()
            .map(str::trim)
            .filter(|l| l.contains("::"))
            .map(|l| l.trim_end_matches(',').rsplit("::").next().unwrap())
            .collect();
        assert_eq!(listed, names::COMMANDS, "generate_handler! and names::COMMANDS differ");
    }

    #[test]
    fn capability_allows_every_command() {
        let cap: Value = serde_json::from_str(include_str!("../../capabilities/main.json")).unwrap();
        let perms: Vec<&str> =
            cap["permissions"].as_array().unwrap().iter().filter_map(Value::as_str).collect();
        for c in names::COMMANDS {
            let p = format!("allow-{}", c.replace('_', "-"));
            assert!(perms.contains(&p.as_str()), "capabilities/main.json lacks {p}");
        }
        assert_eq!(cap["windows"], serde_json::json!(["main"]));
    }

    #[test]
    fn session_write_shapes() {
        let (id, d) = parse_write_body(&InvokeBody::Raw(vec![1, 2]), Some("s1")).unwrap();
        assert_eq!((id.as_str(), d), ("s1", vec![1, 2]));
        assert!(parse_write_body(&InvokeBody::Raw(vec![1]), None).is_err());
        let j = serde_json::json!({"id": "s2", "data": [104, 105]});
        assert_eq!(parse_write_body(&InvokeBody::Json(j), None).unwrap().1, b"hi".to_vec());
        let j = serde_json::json!({"id": "s3", "data": {"0": 1, "1": 255}});
        assert_eq!(parse_write_body(&InvokeBody::Json(j), None).unwrap().1, vec![1, 255]);
        let j = serde_json::json!({"id": "s4", "data": "ls\r"});
        assert_eq!(parse_write_body(&InvokeBody::Json(j), None).unwrap().1, b"ls\r".to_vec());
        let j = serde_json::json!({"id": "s5", "data": [256]});
        assert!(parse_write_body(&InvokeBody::Json(j), None).is_err());
    }
}

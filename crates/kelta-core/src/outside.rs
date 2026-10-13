//! Claude sessions Kelta did not start (#345): the user-scope hook relay (`kelta-ctl hook --outside`)
//! in Claude Code's user settings, and the sessions it reports, keyed by Claude's session id.

use std::io::{ErrorKind, Write};
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::time::Duration;

use kelta_proto::api::CoreApi;
use kelta_proto::error::KeltaError;
use kelta_proto::events::{Toast, ToastAction, UiEvent};
use kelta_proto::hooks::{HookPayload, names};
use kelta_proto::model::{Attention, OutsideSession, SessionStatus};
use kelta_proto::tracker::TicketRef;
use serde_json::value::RawValue;
use serde_json::{Value, json};

use crate::Core;
use crate::claude_mcp::{ConfigLock, Json, Ordered};

pub const INSTALL_ACTION: &str = "claude.hooks_install";
pub const NEVER_ACTION: &str = "claude.hooks_never";

/// Events the relay is registered for.
const EVENTS: &[&str] = &[
    names::SESSION_START,
    names::SESSION_END,
    names::USER_PROMPT_SUBMIT,
    names::PERMISSION_REQUEST,
    names::POST_TOOL_USE,
    names::NOTIFICATION,
    names::STOP,
    names::STOP_FAILURE,
];

const TITLE_CHARS: usize = 60;

/// The relay's command, whatever kelta-ctl path wrote it.
fn is_relay_command(cmd: &str) -> bool {
    cmd.contains("kelta-ctl") && cmd.ends_with(" hook --outside")
}

/// A hook group (`{matcher?, hooks: [...]}`) running the relay.
fn is_relay_group(group: &Value) -> bool {
    group.get("hooks").and_then(Value::as_array).is_some_and(|hs| {
        hs.iter().any(|h| h.get("command").and_then(Value::as_str).is_some_and(is_relay_command))
    })
}

/// Whether Claude Code's user settings run the relay.
fn relay_in(settings: &[u8]) -> bool {
    let Ok(v) = serde_json::from_slice::<Value>(settings) else { return false };
    v.get("hooks")
        .and_then(Value::as_object)
        .is_some_and(|h| h.values().filter_map(Value::as_array).flatten().any(is_relay_group))
}

fn relay_group(event: &str, command: &str) -> Value {
    let mut h = json!({ "type": "command", "command": command, "timeout": 5 });
    if event != names::SESSION_END {
        h["async"] = json!(true);
    }
    match event {
        names::NOTIFICATION => json!({ "matcher": names::NOTIFICATION_MATCHER, "hooks": [h] }),
        names::POST_TOOL_USE => json!({ "matcher": names::POST_TOOL_USE_MATCHER, "hooks": [h] }),
        _ => json!({ "hooks": [h] }),
    }
}

/// Set (`Some(command)`) or drop (`None`) the relay's groups in Claude Code's user settings, every other
/// key, group and event kept in its order and as written; the file keeps its permissions.
fn write_relay(path: &Path, command: Option<&str>) -> Result<(), KeltaError> {
    // A symlinked settings file (dotfiles) is updated where it points.
    let path = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_owned());
    let invalid = |what: &str| KeltaError::invalid(format!("{}: {what}", path.display()));
    let mut v: Ordered = match std::fs::read(&path) {
        // Unparsable or not an object: refuse rather than clobber Claude Code's settings.
        Ok(b) => serde_json::from_slice(&b).map_err(|e| invalid(&e.to_string()))?,
        Err(e) if e.kind() == ErrorKind::NotFound && command.is_none() => return Ok(()),
        Err(e) if e.kind() == ErrorKind::NotFound => Ordered::default(),
        Err(e) => return Err(e.into()),
    };
    let mut hooks: Ordered = match v.get("hooks") {
        Some(raw) => serde_json::from_str(raw.get()).map_err(|_| invalid("hooks is not an object"))?,
        None => Ordered::default(),
    };
    // Each event's groups as written, the relay's left out; an event only the relay used goes.
    let (mut removed, mut emptied) = (false, Vec::new());
    for (event, groups) in &mut hooks.0 {
        let Json::Raw(raw) = groups else { continue };
        let Ok(list) = serde_json::from_str::<Vec<Box<RawValue>>>(raw.get()) else { continue };
        let n = list.len();
        let kept: Vec<Json> = list
            .into_iter()
            .filter(|g| !serde_json::from_str(g.get()).is_ok_and(|g: Value| is_relay_group(&g)))
            .map(Json::Raw)
            .collect();
        removed |= kept.len() != n;
        if kept.is_empty() && n > 0 {
            emptied.push(event.clone());
        }
        *groups = Json::Array(kept);
    }
    hooks.0.retain(|(e, _)| !emptied.contains(e));
    match command {
        Some(cmd) => {
            for event in EVENTS {
                let group = Json::Value(relay_group(event, cmd));
                match hooks.0.iter_mut().find(|(e, _)| e == event) {
                    Some((_, Json::Array(list))) => list.push(group),
                    Some(_) => return Err(invalid(&format!("hooks.{event} is not a list"))),
                    None => hooks.0.push(((*event).to_owned(), Json::Array(vec![group]))),
                }
            }
        }
        None if !removed => return Ok(()),
        None => {}
    }
    if hooks.0.is_empty() && command.is_none() {
        v.remove("hooks");
    } else {
        v.set("hooks", Json::Object(hooks));
    }
    let perms = std::fs::metadata(&path).ok().map(|m| m.permissions());
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let mut tmp = path.clone().into_os_string();
    tmp.push(format!(".kelta-{}", std::process::id()));
    let tmp = PathBuf::from(tmp);
    let text = serde_json::to_string_pretty(&v)?;
    create_tmp(&tmp, perms.as_ref().map(PermissionsExt::mode))?.write_all(text.as_bytes())?;
    // The umask may have narrowed the mode: restore the original exactly.
    if let Some(p) = perms {
        std::fs::set_permissions(&tmp, p)?;
    }
    std::fs::rename(&tmp, &path).inspect_err(|_| {
        let _ = std::fs::remove_file(&tmp);
    })?;
    Ok(())
}

/// A new, empty temp file with `mode` (a 0600 settings file can hold env tokens), so the content is
/// never readable wider than the file it replaces; a stale one from an earlier run is replaced.
fn create_tmp(tmp: &Path, mode: Option<u32>) -> std::io::Result<std::fs::File> {
    let _ = std::fs::remove_file(tmp);
    std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(mode.map_or(0o666, |m| m & 0o7777))
        .open(tmp)
}

/// `key` in `hay` as a whole token, ignoring case: `SHOP-14` is not in `shop-142`.
fn carries_key(hay: &str, key: &str) -> bool {
    if key.is_empty() {
        return false;
    }
    let (hay, key) = (hay.to_ascii_lowercase(), key.to_ascii_lowercase());
    hay.match_indices(&key).any(|(i, _)| {
        let before = hay[..i].chars().next_back();
        let after = hay[i + key.len()..].chars().next();
        !before.is_some_and(|c| c.is_ascii_alphanumeric())
            && !after.is_some_and(|c| c.is_ascii_alphanumeric())
    })
}

fn default_title(cwd: Option<&Path>) -> String {
    cwd.and_then(Path::file_name).map_or_else(|| "Claude".to_owned(), |n| n.to_string_lossy().into_owned())
}

fn prompt_title(prompt: &str) -> Option<String> {
    let line = prompt.trim().lines().next()?.trim();
    (!line.is_empty()).then(|| line.chars().take(TITLE_CHARS).collect())
}

fn alive(pid: u32) -> bool {
    let Ok(pid) = libc::pid_t::try_from(pid) else { return false };
    // SAFETY: signal 0 sends nothing, it only checks that the process exists.
    let r = unsafe { libc::kill(pid, 0) };
    r == 0 || std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
}

async fn branch_of(cwd: &Path) -> Option<String> {
    let out = kelta_work::git::run(cwd, &["rev-parse", "--abbrev-ref", "HEAD"], Duration::from_secs(2)).await;
    let out = out.ok().filter(|o| o.ok())?;
    let b = out.stdout.trim();
    (!b.is_empty() && b != "HEAD").then(|| b.to_owned())
}

impl Core {
    /// Claude Code's user settings: `$CLAUDE_CONFIG_DIR/settings.json`, else `~/.claude/settings.json`.
    fn claude_settings_file(&self) -> Option<PathBuf> {
        crate::spawn_env::claude_config_dir(&self.login_env.vars).map(|d| d.join("settings.json"))
    }

    /// `'<kelta-ctl>' hook --outside`.
    fn relay_command(&self) -> String {
        format!("{} --outside", kelta_work::claude::hook_command(&self.dirs))
    }

    /// Settings → Claude: `None` = Claude Code not found, else whether the relay is registered.
    pub async fn claude_hooks_status(&self) -> Option<bool> {
        self.claude_bin()?;
        let file = self.claude_settings_file()?;
        Some(tokio::fs::read(file).await.is_ok_and(|b| relay_in(&b)))
    }

    /// Startup: offer the relay when Claude Code is found and it is not registered.
    pub async fn claude_hooks_startup(&self) {
        if !self.cfg.effective(None).claude.hooks_offer || self.claude_hooks_status().await != Some(false) {
            return;
        }
        let action = |label: &str, command: &str| ToastAction {
            label: label.into(),
            command: command.into(),
            args: None,
        };
        let mut t = Toast::info("Show the Claude sessions you start outside Kelta in the Queue?");
        t.action = Some(action("Install", INSTALL_ACTION));
        t.secondary = Some(action("Never", NEVER_ACTION));
        self.toast(t);
    }

    /// Install (toast or Settings → Claude): registers the relay in Claude Code's user settings.
    pub async fn claude_hooks_install(&self) -> Result<(), KeltaError> {
        self.claude_bin().ok_or_else(|| KeltaError::not_found("Claude Code not found in your login PATH"))?;
        self.write_relay(Some(&self.relay_command())).await
    }

    /// Remove: drops the relay's groups only.
    pub async fn claude_hooks_remove(&self) -> Result<(), KeltaError> {
        self.write_relay(None).await
    }

    async fn write_relay(&self, command: Option<&str>) -> Result<(), KeltaError> {
        let file = self.claude_settings_file().ok_or_else(|| KeltaError::not_found("HOME is not set"))?;
        // shortcut: Claude Code's `<file>.lock` convention is assumed to cover settings.json as it does
        // `.claude.json`; harmless if it does not.
        let _lock = ConfigLock::acquire(&file).await?;
        write_relay(&file, command)
    }

    /// The sessions the relay reported, without those whose process is gone.
    pub fn outside_list(&self) -> Vec<OutsideSession> {
        self.outside_prune();
        self.outside.lock().values().cloned().collect()
    }

    /// Drops the sessions whose process is gone. Checked on each relayed hook and each list (Now's
    /// refresh): no watcher, no polling.
    fn outside_prune(&self) {
        let gone: Vec<String> = {
            let mut m = self.outside.lock();
            let ids: Vec<String> = m
                .iter()
                .filter(|(_, s)| s.pid.is_some_and(|p| !alive(p)))
                .map(|(id, _)| id.clone())
                .collect();
            for id in &ids {
                m.remove(id);
            }
            ids
        };
        for id in gone {
            self.emit(UiEvent::OutsideRemoved { id });
        }
    }

    /// One relayed hook of a session Kelta did not start.
    pub async fn outside_hook(&self, payload: HookPayload, pid: Option<u32>) {
        self.outside_prune();
        let Some(id) = payload.session_id.clone().filter(|s| !s.is_empty()) else { return };
        let event = payload.hook_event_name.as_str();
        if event == names::SESSION_END {
            if self.outside.lock().remove(&id).is_some() {
                self.emit(UiEvent::OutsideRemoved { id });
            }
            return;
        }
        let Some(change) = kelta_server::hooks::map(&payload) else { return };
        // The lookups first: hooks of one session arrive concurrently, the fold below holds the lock.
        let known = self.outside.lock().get(&id).map(|s| (s.branch.clone(), s.ticket.is_some()));
        let branch = match (&known, &payload.cwd) {
            (Some((b, _)), _) => b.clone(),
            (None, Some(d)) => branch_of(d).await,
            (None, None) => None,
        };
        // Retried until a list holding it has loaded; PostToolUse is too frequent to look each time.
        let ticket = match known {
            Some((_, true)) => None,
            Some(_) if event == names::POST_TOOL_USE => None,
            _ => self.outside_ticket(branch.as_deref(), payload.cwd.as_deref()).await,
        };
        let mut m = self.outside.lock();
        let prev = m.get(&id).cloned();
        let mut s = prev.clone().unwrap_or_else(|| OutsideSession {
            id: id.clone(),
            title: default_title(payload.cwd.as_deref()),
            branch,
            cwd: payload.cwd.clone(),
            status: SessionStatus::Starting,
            // 1 = orphaned to init: not Claude.
            pid: pid.filter(|p| *p > 1),
            ticket: None,
            preview: None,
            updated_at: String::new(),
        });
        let state = crate::status::SessState {
            status: s.status,
            attention: Attention::None,
            seen: true,
            visible: false,
        };
        s.status = crate::status::apply_hook(&state, &change).status;
        if change.preview.is_some() {
            s.preview = change.preview;
        }
        if event == names::USER_PROMPT_SUBMIT
            && s.title == default_title(s.cwd.as_deref())
            && let Some(t) = payload.prompt.as_deref().and_then(prompt_title)
        {
            s.title = t;
        }
        s.ticket = s.ticket.or(ticket);
        if prev.as_ref() == Some(&s) {
            return;
        }
        s.updated_at = kelta_proto::now_rfc3339();
        m.insert(id, s.clone());
        drop(m);
        self.emit(UiEvent::OutsideUpdated { session: s });
    }

    /// A listed ticket whose `branch_key` the session's branch or cwd carries.
    async fn outside_ticket(&self, branch: Option<&str>, cwd: Option<&Path>) -> Option<TicketRef> {
        let hay: Vec<String> = [branch.map(str::to_owned), cwd.map(|c| c.to_string_lossy().into_owned())]
            .into_iter()
            .flatten()
            .collect();
        if hay.is_empty() {
            return None;
        }
        let accounts = self.cfg.effective(None).accounts.clone();
        self.listed_tickets().await.into_iter().find_map(|t| {
            let key = self.providers.tracker(&t.r#ref.account, &accounts).ok()?.branch_key(&t.r#ref);
            hay.iter().any(|h| carries_key(h, &key)).then_some(t.r#ref)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_as_a_whole_token() {
        assert!(carries_key("feat/SHOP-142-login", "shop-142"));
        assert!(carries_key("/src/.worktrees/gh-12", "gh-12"));
        assert!(!carries_key("feat/SHOP-1420", "SHOP-142"));
        assert!(!carries_key("feat/xSHOP-142", "SHOP-142"));
        assert!(!carries_key("main", ""));
    }

    #[test]
    fn titles() {
        assert_eq!(default_title(Some(Path::new("/src/shop"))), "shop");
        assert_eq!(default_title(None), "Claude");
        assert_eq!(prompt_title("  fix the total\nmore").as_deref(), Some("fix the total"));
        assert_eq!(prompt_title(" \n"), None);
        assert_eq!(prompt_title(&"é".repeat(100)).map(|t| t.chars().count()), Some(TITLE_CHARS));
    }

    #[test]
    fn this_process_is_alive() {
        assert!(alive(std::process::id()));
        assert!(!alive(u32::MAX));
    }

    #[test]
    fn temp_file_never_wider_than_the_original() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("settings.json");
        let t = tmp.path().join("settings.json.kelta-1");
        let mode = |p: &Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
        // A stale temp left world-readable is not reused as it is.
        std::fs::write(&t, "stale").unwrap();
        std::fs::set_permissions(&t, std::fs::Permissions::from_mode(0o644)).unwrap();
        drop(create_tmp(&t, Some(0o100_600)).unwrap());
        assert_eq!(mode(&t), 0o600, "before any content is written");
        assert_eq!(std::fs::read(&t).unwrap(), b"");

        std::fs::write(&path, "{}").unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
        write_relay(&path, Some("kelta-ctl hook --outside")).unwrap();
        assert_eq!(mode(&path), 0o600);
        write_relay(&path, None).unwrap();
        assert_eq!(mode(&path), 0o600);
    }

    #[test]
    fn relay_written_and_removed_leaving_the_rest() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("settings.json");
        let cmd = "'/k/bin/current/kelta-ctl' hook --outside";
        // As Claude Code writes it (`JSON.stringify(c, null, 2)`), with a user hook on Stop.
        let before = r#"{
  "model": "opus",
  "hooks": {
    "Stop": [
      {
        "hooks": [
          {
            "type": "command",
            "command": "say done"
          }
        ]
      }
    ]
  },
  "env": {
    "A": "1"
  }
}"#;
        std::fs::write(&path, before).unwrap();
        assert!(!relay_in(before.as_bytes()));

        write_relay(&path, Some(cmd)).unwrap();
        let after = std::fs::read(&path).unwrap();
        assert!(relay_in(&after));
        let v: Value = serde_json::from_slice(&after).unwrap();
        let events: Vec<&str> = v["hooks"].as_object().unwrap().keys().map(String::as_str).collect();
        assert_eq!(events.len(), EVENTS.len());
        assert_eq!(v["hooks"]["Stop"][0]["hooks"][0]["command"], "say done");
        assert_eq!(v["hooks"]["Stop"][1]["hooks"][0]["command"], cmd);
        assert_eq!(v["hooks"]["Notification"][0]["matcher"], names::NOTIFICATION_MATCHER);
        assert!(v["hooks"]["SessionEnd"][0]["hooks"][0].get("async").is_none());

        // Installing again replaces the groups rather than doubling them.
        write_relay(&path, Some(cmd)).unwrap();
        let v: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(v["hooks"]["Stop"].as_array().unwrap().len(), 2);

        write_relay(&path, None).unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), before);
        assert_eq!(std::fs::read_dir(tmp.path()).unwrap().count(), 1, "no temp file left");

        // No hooks before: none after. Missing file: removing writes nothing.
        std::fs::write(&path, "{\n  \"model\": \"opus\"\n}").unwrap();
        write_relay(&path, Some(cmd)).unwrap();
        write_relay(&path, None).unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "{\n  \"model\": \"opus\"\n}");
        let fresh = tmp.path().join("cfg/settings.json");
        write_relay(&fresh, None).unwrap();
        assert!(!fresh.exists());
        write_relay(&fresh, Some(cmd)).unwrap();
        assert!(relay_in(&std::fs::read(&fresh).unwrap()));

        // Unparsable: left as it is.
        std::fs::write(&path, "not json").unwrap();
        assert!(write_relay(&path, Some(cmd)).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), b"not json");
    }
}

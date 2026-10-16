//! `CtlCommand` dispatch (ARCHITECTURE §2, SPEC §8 `kelta-ctl`), the stable `kelta-ctl` copy
//! (`<data>/bin/<ver>/kelta-ctl` + `current` symlink), `open_external`, plugin `http_fetch`,
//! Claude version probe.

use std::path::{Path, PathBuf};
use std::time::Duration;

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as B64;
use kelta_proto::ctl::CtlCommand;
use kelta_proto::dirs::Dirs;
use kelta_proto::error::KeltaError;
use kelta_proto::events::{BusEvent, UiEvent, bus};
use kelta_proto::ext::{ProxiedRequest, ProxiedResponse};
use kelta_proto::ipc::ToolVersion;
use kelta_proto::model::{EditorTarget, Lifecycle, Placement, SessionKind, TemplateCtx, WorkSource};
use kelta_proto::settings::ClaudeSettings;
use kelta_proto::term::LoginEnv;
use kelta_proto::tracker::TicketRef;
use serde_json::{Value, json};

use crate::Core;
use crate::store::q;

/// Ticket key from a key or a browser URL (`…/browse/SHOP-1`, `…/issues/12`).
pub fn ticket_key(input: &str) -> String {
    let s = input.trim();
    if !s.contains("://") {
        return s.to_owned();
    }
    let path = s.split_once("://").map_or(s, |(_, r)| r);
    let path = path.split(['?', '#']).next().unwrap_or(path);
    path.trim_end_matches('/').rsplit('/').next().unwrap_or(s).to_owned()
}

/// Copy `kelta-ctl` (sibling of the running binary, or `$APPDIR/usr/bin`) to
/// `<data>/bin/<version>/kelta-ctl` and point `<data>/bin/current` at that version.
pub fn install_stable_ctl(dirs: &Dirs) -> Result<PathBuf, KeltaError> {
    install_stable_bin(&bundled("kelta-ctl")?, &dirs.bin, kelta_proto::VERSION)
}

/// A binary shipped next to the kelta executable (Tauri externalBin) or in the AppImage.
pub fn bundled(name: &str) -> Result<PathBuf, KeltaError> {
    let exe = std::env::current_exe()?;
    let mut candidates: Vec<PathBuf> = exe.parent().map(|d| vec![d.join(name)]).unwrap_or_default();
    if let Some(appdir) = std::env::var_os("APPDIR") {
        candidates.push(PathBuf::from(appdir).join("usr/bin").join(name));
    }
    candidates
        .into_iter()
        .find(|p| p.is_file())
        .ok_or_else(|| KeltaError::not_found(format!("{name} not found next to the kelta binary")))
}

/// Copy `src` to `<bin>/<version>/<file name>` (kept when identical) and point `<bin>/current`
/// at that version; returns `<bin>/current/<file name>`. Survives app updates and AppImage unmounts.
pub fn install_stable_bin(src: &Path, bin: &Path, version: &str) -> Result<PathBuf, KeltaError> {
    use std::os::unix::fs::PermissionsExt;
    let name =
        src.file_name().ok_or_else(|| KeltaError::invalid(format!("{} has no file name", src.display())))?;
    let dir = bin.join(version);
    std::fs::create_dir_all(&dir)?;
    let dest = dir.join(name);
    let data = std::fs::read(src)?;
    let same = std::fs::read(&dest).map(|d| d == data).unwrap_or(false);
    if !same {
        let tmp = dir.join(format!(".{}.{}", name.to_string_lossy(), std::process::id()));
        std::fs::write(&tmp, &data)?;
        std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o755))?;
        std::fs::rename(&tmp, &dest)?;
    }
    let current = bin.join("current");
    let points_here = std::fs::read_link(&current).map(|t| t == Path::new(version)).unwrap_or(false);
    if !points_here {
        let tmp = bin.join(format!(".current.{}", std::process::id()));
        let _ = std::fs::remove_file(&tmp);
        std::os::unix::fs::symlink(version, &tmp)?;
        std::fs::rename(&tmp, &current)?;
    }
    Ok(current.join(name))
}

/// `open_external`: http/https/mailto only, opened by the OS handler.
pub async fn open_external(url: &str) -> Result<(), KeltaError> {
    let lower = url.trim().to_ascii_lowercase();
    if !(lower.starts_with("https://") || lower.starts_with("http://") || lower.starts_with("mailto:")) {
        return Err(KeltaError::invalid("only http, https and mailto links can be opened"));
    }
    let opener = if cfg!(target_os = "macos") { "open" } else { "xdg-open" };
    let mut child = kelta_proto::term::async_command(opener)
        .arg(url.trim())
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map_err(|e| KeltaError::unsupported(format!("cannot run {opener}: {e}")))?;
    tokio::spawn(async move {
        let _ = child.wait().await;
    });
    Ok(())
}

/// Plugin fetches: own client, redirects never followed (a 3xx to another host would bypass the
/// `net:<host>` grant the caller checked on `req.url`), returned to the plugin as-is.
fn plugin_client() -> Result<&'static reqwest::Client, KeltaError> {
    static C: std::sync::OnceLock<Option<reqwest::Client>> = std::sync::OnceLock::new();
    C.get_or_init(|| {
        kelta_http::client_builder()
            .user_agent(kelta_http::default_user_agent())
            .timeout(Duration::from_secs(20))
            .pool_idle_timeout(Duration::from_secs(30))
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .ok()
    })
    .as_ref()
    .ok_or_else(|| KeltaError::internal("plugin http client unavailable"))
}

/// Plugin `net.fetch` (the caller checked the `net:` grant). https only (http for loopback),
/// no redirects, 5 MB response cap enforced while streaming.
pub async fn http_fetch(req: ProxiedRequest) -> Result<ProxiedResponse, KeltaError> {
    const CAP: usize = 5 * 1024 * 1024;
    let url = reqwest::Url::parse(&req.url).map_err(|e| KeltaError::invalid(format!("invalid URL: {e}")))?;
    let loopback = matches!(url.host_str(), Some("127.0.0.1" | "localhost" | "[::1]"));
    if !(url.scheme() == "https" || (url.scheme() == "http" && loopback)) {
        return Err(KeltaError::invalid("only https URLs can be fetched (http for localhost)"));
    }
    let c = plugin_client()?;
    let mut rb = match req.method.to_ascii_uppercase().as_str() {
        "GET" | "" => c.get(url),
        "POST" => c.post(url),
        "PUT" => c.put(url),
        "PATCH" => c.patch(url),
        "DELETE" => c.delete(url),
        "HEAD" => c.head(url),
        other => return Err(KeltaError::invalid(format!("unsupported method {other}"))),
    };
    for (k, v) in &req.headers {
        rb = rb.header(k.as_str(), v.as_str());
    }
    if let Some(body) = req.body {
        rb = if req.body_base64 {
            rb.body(B64.decode(&body).map_err(|e| KeltaError::invalid(format!("invalid base64 body: {e}")))?)
        } else {
            rb.body(body)
        };
    }
    if let Some(ms) = req.timeout_ms {
        rb = rb.timeout(Duration::from_millis(ms.clamp(100, 60_000)));
    }
    let mut resp = rb.send().await.map_err(|e| {
        KeltaError::network(format!("fetch failed: {}", kelta_proto::redact::redact_url(&e.to_string())))
    })?;
    let too_big = || KeltaError::invalid("response larger than 5 MB");
    if resp.content_length().is_some_and(|n| n > CAP as u64) {
        return Err(too_big());
    }
    let status = resp.status().as_u16();
    let headers = resp
        .headers()
        .iter()
        .filter_map(|(k, v)| v.to_str().ok().map(|v| (k.as_str().to_ascii_lowercase(), v.to_owned())))
        .collect();
    let mut bytes = Vec::new();
    while let Some(chunk) = resp.chunk().await.map_err(|e| KeltaError::network(format!("fetch body: {e}")))? {
        if bytes.len() + chunk.len() > CAP {
            return Err(too_big());
        }
        bytes.extend_from_slice(&chunk);
    }
    let (body, body_base64) = match String::from_utf8(bytes) {
        Ok(s) => (s, false),
        Err(e) => (B64.encode(e.as_bytes()), true),
    };
    Ok(ProxiedResponse { status, headers, body, body_base64 })
}

/// One-shot `claude <args>` in `cwd` with exactly `env` (the login environment, plus the caller's
/// extras): `stdin` is written to it (null when `None`), and it is killed after `timeout`.
pub(crate) async fn claude_oneshot<'a, A: AsRef<std::ffi::OsStr>>(
    bin: &Path,
    args: impl IntoIterator<Item = A>,
    env: impl IntoIterator<Item = (&'a String, &'a String)>,
    cwd: &Path,
    stdin: Option<&[u8]>,
    timeout: Duration,
) -> Result<std::process::Output, KeltaError> {
    use std::process::Stdio;
    use tokio::io::AsyncWriteExt;
    let io = |e: std::io::Error| KeltaError::internal(format!("claude: {e}"));
    let mut child = kelta_proto::term::async_command(bin)
        .args(args)
        .current_dir(cwd)
        .env_clear()
        .envs(env)
        .stdin(if stdin.is_some() { Stdio::piped() } else { Stdio::null() })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .map_err(io)?;
    if let (Some(input), Some(mut pipe)) = (stdin, child.stdin.take()) {
        // A Claude that exits before reading its stdin is reported by its exit status, not here.
        let _ = pipe.write_all(input).await;
    }
    tokio::time::timeout(timeout, child.wait_with_output())
        .await
        .map_err(|_| KeltaError::timeout(format!("Claude did not answer within {} s", timeout.as_secs())))?
        .map_err(io)
}

/// `claude --version` in the login PATH (5 s budget), compared with `claude.min_version`.
pub async fn probe_claude(login: LoginEnv, claude: ClaudeSettings) -> Option<ToolVersion> {
    let cwd = std::env::temp_dir();
    let path = crate::spawn_env::resolve_program(&claude.binary, login.path(), &cwd).ok()?;
    // one-shot: bounded probe, armed once at startup.
    let out =
        claude_oneshot(&path, ["--version"], &login.vars, &cwd, None, Duration::from_secs(5)).await.ok()?;
    let text = String::from_utf8_lossy(&out.stdout);
    let version =
        text.split_whitespace().find(|w| w.chars().next().is_some_and(|c| c.is_ascii_digit()))?.to_owned();
    let ok = match (semver::Version::parse(&version), semver::Version::parse(&claude.min_version)) {
        (Ok(v), Ok(min)) => v >= min,
        _ => true,
    };
    Some(ToolVersion { path, version, ok })
}

impl Core {
    /// Dispatch of ctl socket commands.
    pub(crate) async fn dispatch_ctl(&self, cmd: CtlCommand) -> Result<Value, KeltaError> {
        match cmd {
            // kelta-server authenticates and ingests hooks itself (`hooks::ingest`).
            CtlCommand::Hook { .. } => Err(KeltaError::invalid("hooks go through the ctl server")),
            CtlCommand::OutsideHook { payload, pid } => {
                self.outside_hook(*payload, pid).await;
                Ok(Value::Null)
            }
            CtlCommand::Toggle
            | CtlCommand::Palette
            | CtlCommand::Next
            | CtlCommand::Back
            | CtlCommand::PluginInstall { .. } => {
                self.emit(UiEvent::CtlCommand { cmd });
                Ok(Value::Null)
            }
            CtlCommand::Open { path } => {
                let home = self.home_dir();
                let path = crate::projects::repo_path(&path.to_string_lossy(), &home);
                let id = match self.projects_for_path(&path).into_iter().next() {
                    Some(id) => id,
                    None => {
                        let draft = self.project_detect(&path)?;
                        if draft.repos.is_empty() {
                            return Err(KeltaError::invalid(format!(
                                "{} contains no git repository",
                                path.display()
                            )));
                        }
                        self.project_create(&draft)?.id
                    }
                };
                let info = self.project_activate(&id)?;
                self.emit(UiEvent::CtlCommand { cmd: CtlCommand::FocusProject { id } });
                Ok(serde_json::to_value(info)?)
            }
            CtlCommand::FocusProject { id } => {
                let info = self.project_activate(&id)?;
                self.emit(UiEvent::CtlCommand { cmd: CtlCommand::FocusProject { id } });
                Ok(serde_json::to_value(info)?)
            }
            CtlCommand::Start { ticket, project, .. } => {
                // A bare key: the first tracker account (the given project's, else every project's, the
                // active one's first) that has it, by GET, so unassigned or unlisted tickets resolve too;
                // none answers → the first account.
                let configs = match &project {
                    Some(p) => vec![
                        self.cfg.project(p).ok_or_else(|| KeltaError::not_found(format!("project {p}")))?,
                    ],
                    None => {
                        let active = self.active_project();
                        let mut all = self.project_configs();
                        all.sort_by_key(|p| p.id != active);
                        all
                    }
                };
                let mut accounts = Vec::new();
                for b in configs.iter().filter_map(|p| self.project_binding(&p.id)) {
                    for a in
                        std::iter::once(&b.account).chain(b.views.iter().filter_map(|v| v.account.as_ref()))
                    {
                        if !accounts.contains(a) {
                            accounts.push(a.clone());
                        }
                    }
                }
                let Some(first) = accounts.first().cloned() else {
                    return Err(KeltaError::invalid(match &project {
                        Some(p) => format!("project {p} has no tracker"),
                        None => "no project with a tracker; pass --project".into(),
                    }));
                };
                let key = ticket_key(&ticket);
                let mut tref = None;
                if accounts.len() > 1 {
                    for account in accounts {
                        let probe = TicketRef { account, key: key.clone(), id: key.clone() };
                        if let Ok(d) = self.tracker_get(&probe).await {
                            tref = Some(d.ticket.r#ref);
                            break;
                        }
                    }
                }
                let tref = tref.unwrap_or(TicketRef { account: first, key: key.clone(), id: key });
                let asked = project.clone();
                let plan =
                    self.work_plan(project, WorkSource::Ticket { ticket: tref.clone() }, None, false).await?;
                // An explicit --project that cannot host the start is an error, not a silent switch.
                if let Some(p) = asked.filter(|p| plan.existing.is_none() && *p != plan.project_id) {
                    return Err(KeltaError::invalid(format!("project {p} is closed or has no repository")));
                }
                let project = plan.project_id.clone();
                // A guessed project is confirmed in the sheet, as with `plan_preview`.
                if plan.project_guessed || self.cfg.effective(Some(&project)).work.plan_preview {
                    self.emit(UiEvent::CtlCommand {
                        cmd: CtlCommand::Start {
                            ticket,
                            // A guess stays unset so the sheet's re-plan keeps its guess hint.
                            project: if plan.project_guessed { None } else { Some(project.clone()) },
                            resolved: Some(tref),
                        },
                    });
                    self.emit(UiEvent::CtlCommand { cmd: CtlCommand::FocusProject { id: project } });
                    Ok(serde_json::to_value(plan)?)
                } else {
                    let w = self.work.start(plan).await?;
                    self.emit(UiEvent::CtlCommand { cmd: CtlCommand::FocusProject { id: project } });
                    Ok(serde_json::to_value(w)?)
                }
            }
            CtlCommand::StartTask { task, project } => {
                // Scripted: no sheet, same plan + saga as New work item (FLOW §4.3).
                let project = project.unwrap_or_else(|| self.active_project());
                let source = WorkSource::Branch { name: String::new(), task: Some(task), repo: None };
                let plan = self.work.plan(&project, source).await?;
                let w = self.work.start(plan).await?;
                self.project_activate(&project)?;
                self.emit(UiEvent::CtlCommand { cmd: CtlCommand::FocusProject { id: project } });
                Ok(serde_json::to_value(w)?)
            }
            CtlCommand::New { template, cwd, project } => {
                let project = project.unwrap_or_else(|| self.active_project());
                let ctx = TemplateCtx { cwd, ..TemplateCtx::default() };
                let sessions =
                    self.session_spawn_template(&project, &template, ctx, Placement::NewTab).await?;
                self.project_activate(&project)?;
                self.emit(UiEvent::CtlCommand { cmd: CtlCommand::FocusProject { id: project } });
                Ok(serde_json::to_value(sessions)?)
            }
            CtlCommand::Emit { name, payload, project } => {
                if !name.starts_with(bus::CUSTOM_PREFIX) || name.len() <= bus::CUSTOM_PREFIX.len() {
                    return Err(KeltaError::invalid("only custom.* events can be emitted"));
                }
                if let Some(p) = &project
                    && self.cfg.project(p).is_none()
                {
                    return Err(KeltaError::not_found(format!("project {p}")));
                }
                // kelta-bench drives the desktop shell through `custom.bench.*` (honoured by the
                // bridge only when KELTA_BENCH_MARKS is set).
                if name.starts_with("custom.bench.") {
                    self.emit(UiEvent::CtlCommand {
                        cmd: CtlCommand::Emit {
                            name: name.clone(),
                            payload: payload.clone(),
                            project: project.clone(),
                        },
                    });
                }
                let ev = BusEvent::new(name, payload);
                self.publish_ev(match project {
                    Some(p) => ev.with_project(p),
                    None => ev,
                });
                Ok(Value::Null)
            }
            CtlCommand::Trust { repo, sha256 } => {
                let repo = crate::projects::repo_path(&repo.to_string_lossy(), &self.home_dir());
                // Trust is keyed by the project TOML's path: match canonically (`..`, symlinks).
                let canon = |p: &std::path::Path| std::fs::canonicalize(p).ok();
                let want = canon(&repo.join(".kelta").join("config.toml"));
                let file = self
                    .config
                    .repo_config_paths()
                    .into_iter()
                    .find(|p| want.is_some() && canon(p) == want)
                    .ok_or_else(|| KeltaError::not_found("not a repo of any project"))?;
                Ok(serde_json::to_value(self.config.trust_file(&file, Some(&sha256)).await?)?)
            }
            CtlCommand::EditorOpen { file, line } => {
                let home = self.home_dir();
                let file = crate::projects::repo_path(&file.to_string_lossy(), &home);
                let target = self.editor_target_for(&file).await?;
                self.work.editor_open(target, &file, line).await?;
                Ok(Value::Null)
            }
            CtlCommand::NoteAdd { session, path, line_start, line_end, body } => Ok(serde_json::to_value(
                self.work.note_add(&session, &path, (line_start, line_end), &body).await?,
            )?),
            CtlCommand::NoteList { session } => {
                Ok(serde_json::to_value(self.work.notes_of_session(&session).await?)?)
            }
            CtlCommand::NoteSend { session } => {
                Ok(serde_json::to_value(self.work.notes_send_of_session(&session).await?)?)
            }
            CtlCommand::NoteResolve { session, note } => {
                Ok(serde_json::to_value(self.work.note_resolve_of_session(&session, note).await?)?)
            }
            CtlCommand::Version => Ok(json!({ "version": kelta_proto::VERSION })),
        }
    }

    /// A work item whose worktree holds `file`, else the newest live editor of the active project.
    async fn editor_target_for(&self, file: &Path) -> Result<EditorTarget, KeltaError> {
        let items = self.store.call(|c| q::work_list(c, None)).await.unwrap_or_default();
        if let Some(w) = items
            .iter()
            .filter(|w| file.starts_with(&w.worktree))
            .max_by_key(|w| w.worktree.as_os_str().len())
        {
            return Ok(EditorTarget::WorkItem { id: w.id.clone() });
        }
        let active = self.active_project();
        self.list_sessions(Some(&active))
            .into_iter()
            .rev()
            .find(|s| matches!(s.kind, SessionKind::Editor { .. }) && s.lifecycle == Lifecycle::Live)
            .map(|s| EditorTarget::Session { id: s.id })
            .ok_or_else(|| KeltaError::not_found("no editor session to open the file in"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn helpers() {
        assert_eq!(ticket_key("SHOP-1"), "SHOP-1");
        assert_eq!(ticket_key("https://acme.atlassian.net/browse/SHOP-142?x=1"), "SHOP-142");
    }

    #[test]
    fn installs_ctl_copy_and_symlink() {
        let tmp = tempfile::tempdir().unwrap();
        let src = tmp.path().join("kelta-ctl");
        std::fs::write(&src, b"#!/bin/sh\n").unwrap();
        let bin = tmp.path().join("bin");
        let p = install_stable_bin(&src, &bin, "0.1.0").unwrap();
        assert_eq!(std::fs::read(&p).unwrap(), b"#!/bin/sh\n".to_vec());
        assert_eq!(std::fs::read_link(bin.join("current")).unwrap(), PathBuf::from("0.1.0"));
        // idempotent, then upgrade
        install_stable_bin(&src, &bin, "0.1.0").unwrap();
        install_stable_bin(&src, &bin, "0.2.0").unwrap();
        assert_eq!(std::fs::read_link(bin.join("current")).unwrap(), PathBuf::from("0.2.0"));
    }

    /// One-shot loopback server answering every connection with `resp`.
    async fn serve(resp: &'static str) -> String {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = l.local_addr().unwrap();
        tokio::spawn(async move {
            while let Ok((mut s, _)) = l.accept().await {
                let mut buf = [0u8; 2048];
                let _ = s.read(&mut buf).await;
                let _ = s.write_all(resp.as_bytes()).await;
            }
        });
        format!("http://{addr}/r")
    }

    fn get(url: String) -> ProxiedRequest {
        ProxiedRequest { url, method: "GET".into(), ..Default::default() }
    }

    #[tokio::test]
    async fn fetch_does_not_follow_redirects_or_buffer_huge_bodies() {
        let url =
            serve("HTTP/1.1 302 Found\r\nLocation: http://127.0.0.1:9/x\r\nContent-Length: 0\r\n\r\n").await;
        let r = http_fetch(get(url)).await.unwrap();
        assert_eq!(r.status, 302);
        assert_eq!(
            r.headers.iter().find(|h| h.0 == "location").map(|h| h.1.as_str()),
            Some("http://127.0.0.1:9/x")
        );

        let url = serve("HTTP/1.1 200 OK\r\nContent-Length: 999999999\r\n\r\nabc").await;
        assert!(http_fetch(get(url)).await.unwrap_err().message.contains("5 MB"));

        assert!(http_fetch(get("http://example.com/".into())).await.is_err(), "plain http off loopback");
    }

    #[tokio::test]
    async fn fetch_rejects_malformed_base64_body() {
        let req = ProxiedRequest {
            body: Some("A".into()),
            body_base64: true,
            ..get("http://127.0.0.1:9/x".into())
        };
        assert_eq!(http_fetch(req).await.unwrap_err().code, kelta_proto::error::ErrorCode::InvalidArgument);
    }
}

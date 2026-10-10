//! Tools registry (config layers + plugins, by id, plugin tools namespaced `<plugin>/<id>`),
//! `tool_check`, PTY tools (sessions via `CoreApi::session_spawn`) and web tools (server process,
//! embed decision, proxy registration, lifecycle).

use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use kelta_proto::error::KeltaError;
use kelta_proto::events::{BusEvent, UiEvent, bus};
use kelta_proto::ext::{
    EmbedMode, Permission, ProxiedRequest, StopSpec, ToolCheck, ToolDef, ToolHandle, ToolInfo, ToolKind,
    ToolSource, WebLifecycle,
};
use kelta_proto::ids::{PluginId, ProjectId, ScreenInstanceId, ToolId, ToolInstanceId};
use kelta_proto::model::{
    OpenPaneRequest, PaneContent, Placement, RestorePolicy, SessionKind, SpawnRequest, TemplateCtx,
};
use kelta_proto::settings::Layer;
use parking_lot::Mutex;
use serde_json::json;

use crate::PluginHost;
use crate::context::{CtxSpec, project_root};
use crate::registry::Entry;
use crate::template::Vars;
use crate::util::{is_loopback_host, signal_group, truncate};
use crate::web::{self, Launch, ServerProc};

/// UI event name carrying a web tool handle to the pane (`plugin.event` with the tool instance id).
pub const TOOL_HANDLE_EVENT: &str = "kelta.tool_handle";
/// UI event name for a web tool that exited (`{code, log}`).
pub const TOOL_EXITED_EVENT: &str = "kelta.tool_exited";

/// Detached launch: own process group (no terminal signals from Kelta), no stdio.
// shortcut: no setsid (needs unsafe pre_exec); a GUI-launched Kelta has no controlling tty anyway.
pub(crate) fn external_command(
    program: &str,
    args: &[String],
    cwd: &std::path::Path,
    env: &BTreeMap<String, String>,
) -> tokio::process::Command {
    let mut cmd = tokio::process::Command::new(program);
    cmd.args(args)
        .current_dir(cwd)
        .envs(env)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .process_group(0)
        .kill_on_drop(false);
    cmd
}

#[derive(Clone)]
pub(crate) struct Resolved {
    pub id: ToolId,
    pub def: ToolDef,
    pub source: ToolSource,
    pub plugin: Option<Arc<Entry>>,
}

pub(crate) struct WebInstance {
    pub tool_id: ToolId,
    pub project_id: ProjectId,
    pub lifecycle: WebLifecycle,
    pub plugin: Option<PluginId>,
    pub proc_: Option<ServerProc>,
    pub stop: StopSpec,
    pub stop_argv: Option<(Vec<String>, PathBuf)>,
    pub closing: AtomicBool,
}

#[derive(Default)]
pub(crate) struct State {
    checks: Mutex<HashMap<String, ToolCheck>>,
    web: Mutex<HashMap<String, Arc<WebInstance>>>,
}

impl State {
    /// SIGKILL every web tool group (host drop / app exit).
    pub fn kill_all(&self) {
        for (id, inst) in self.web.lock().drain() {
            crate::proxy::unregister(&id);
            if let Some(p) = &inst.proc_
                && p.exited().is_none()
            {
                signal_group(p.pid, rustix::process::Signal::KILL);
            }
        }
    }

    pub fn running(&self) -> usize {
        self.web.lock().len()
    }
}

fn plugin_tool_id(plugin: &PluginId, id: &str) -> String {
    format!("{plugin}/{id}")
}

impl PluginHost {
    /// Config tools (merged by id by kelta-config) + enabled plugin tools.
    pub(crate) fn resolve_tools(&self, project: Option<&ProjectId>) -> Vec<Resolved> {
        let effective = self.settings(project);
        let global = self.settings(None);
        let mut out: Vec<Resolved> = Vec::new();
        for t in effective.tools.iter().filter(|t| t.enabled && !t.id.is_empty()) {
            let layer = if project.is_some() && !global.tools.iter().any(|g| g == t) {
                Layer::Project
            } else {
                Layer::Global
            };
            out.push(Resolved {
                id: ToolId::new(t.id.clone()),
                def: t.clone(),
                source: ToolSource::Layer { layer },
                plugin: None,
            });
        }
        for e in self.active() {
            let Some(m) = e.manifest() else { continue };
            for t in m.contributes.tools.iter().filter(|t| t.enabled) {
                let mut def = t.clone();
                def.id = plugin_tool_id(&e.id, &t.id);
                out.push(Resolved {
                    id: ToolId::new(def.id.clone()),
                    def,
                    source: ToolSource::Plugin { plugin_id: e.id.clone() },
                    plugin: Some(e.clone()),
                });
            }
        }
        out
    }

    pub(crate) fn tool_infos(&self, project: &ProjectId) -> Vec<ToolInfo> {
        let checks = self.tools.checks.lock().clone();
        self.resolve_tools(Some(project))
            .into_iter()
            .map(|r| ToolInfo {
                label: if r.def.label.is_empty() { r.def.id.clone() } else { r.def.label.clone() },
                icon: r.def.icon.clone(),
                kind: r.def.kind,
                installed: checks.get(r.id.as_str()).map(|c| c.installed),
                keybinding: r.def.keybinding.clone(),
                description: r.def.description.clone(),
                source: r.source,
                id: r.id,
            })
            .collect()
    }

    fn find_tool(&self, project: Option<&ProjectId>, tool: &ToolId) -> Result<Resolved, KeltaError> {
        self.resolve_tools(project)
            .into_iter()
            .find(|r| &r.id == tool)
            .ok_or_else(|| KeltaError::not_found(format!("tool `{tool}` not found")))
    }

    /// `tool_check` has no project: global + plugin tools first, then each project's config tools.
    fn find_tool_any(&self, tool: &ToolId) -> Result<Resolved, KeltaError> {
        self.find_tool(None, tool).or_else(|e| {
            let projects = self.wiring().settings.map(|s| s.projects()).unwrap_or_default();
            projects.iter().find_map(|p| self.find_tool(Some(&p.id), tool).ok()).ok_or(e)
        })
    }

    fn program_of(def: &ToolDef) -> Option<&str> {
        match def.kind {
            ToolKind::Pty | ToolKind::External => def.command.as_deref(),
            ToolKind::Web => def.start.as_ref().map(|s| s.command.as_str()),
        }
    }

    /// Plugin tools need `sessions.spawn` plus `exec:<program>` for what they run.
    async fn check_tool_permission(&self, r: &Resolved, program: Option<&str>) -> Result<(), KeltaError> {
        let Some(entry) = &r.plugin else { return Ok(()) };
        let granted = self.granted(entry).await?;
        granted.require(Permission::SessionsSpawn)?;
        if let Some(p) = program {
            granted.require_exec(p)?;
        }
        Ok(())
    }

    pub(crate) async fn check_tool(&self, tool: &ToolId) -> Result<ToolCheck, KeltaError> {
        let r = self.find_tool_any(tool)?;
        let vars = self.build_vars(CtxSpec { plugin: r.plugin.as_deref(), ..Default::default() }).await;
        let hint = r.def.install_hint.clone();
        // Same PATH the tool will launch with (see `launch_env`).
        let path = self.core().and_then(|c| c.login_path()).or_else(|| std::env::var("PATH").ok());
        let result = match &r.def.check {
            Some(argv) => {
                let argv: Vec<String> =
                    argv.iter().map(|a| vars.expand(a).unwrap_or_else(|_| a.clone())).collect();
                self.check_tool_permission(&r, argv.first().map(String::as_str)).await?;
                run_check(&argv, path.as_deref(), hint.clone()).await
            }
            None => match Self::program_of(&r.def) {
                Some(p) => {
                    let p = vars.expand(p).unwrap_or_else(|_| p.to_owned());
                    let cwd = std::env::current_dir().unwrap_or_default();
                    let installed = which::which_in(&p, path.as_deref(), cwd).is_ok();
                    ToolCheck { installed, version: None, install_hint: if installed { None } else { hint } }
                }
                None => ToolCheck { installed: true, version: None, install_hint: None },
            },
        };
        self.tools.checks.lock().insert(tool.to_string(), result.clone());
        Ok(result)
    }

    pub(crate) async fn open_tool(
        &self,
        project: &ProjectId,
        tool: &ToolId,
        ctx: TemplateCtx,
        placement: Placement,
    ) -> Result<ToolHandle, KeltaError> {
        let r = self.find_tool(Some(project), tool)?;
        if let Some(e) = &r.plugin {
            self.mark_activated(&e.id);
        }
        let vars = self
            .build_vars(CtxSpec {
                project: Some(project),
                tctx: Some(&ctx),
                plugin: r.plugin.as_deref(),
                ..Default::default()
            })
            .await;
        match r.def.kind {
            ToolKind::Pty => self.open_pty(project, &r, &ctx, &vars, placement).await,
            ToolKind::Web => self.open_web(project, &r, &ctx, vars, placement).await,
            ToolKind::External => self.open_external(project, &r, &ctx, &vars).await,
        }
    }

    fn default_cwd(&self, project: &ProjectId, ctx: &TemplateCtx) -> PathBuf {
        ctx.cwd
            .clone()
            .or_else(|| self.core().and_then(|c| c.project(project)).as_ref().and_then(project_root))
            .unwrap_or_else(|| PathBuf::from(std::env::var("HOME").unwrap_or_else(|_| "/".into())))
    }

    fn expand_cwd(
        &self,
        tmpl: Option<&str>,
        vars: &Vars,
        project: &ProjectId,
        ctx: &TemplateCtx,
    ) -> Result<PathBuf, KeltaError> {
        match tmpl.map(|t| vars.expand(t)).transpose()?.filter(|s| !s.is_empty()) {
            Some(s) => Ok(PathBuf::from(s)),
            None => Ok(self.default_cwd(project, ctx)),
        }
    }

    fn expand_env(
        vars: &Vars,
        env: &BTreeMap<String, String>,
    ) -> Result<BTreeMap<String, String>, KeltaError> {
        env.iter().map(|(k, v)| Ok((k.clone(), vars.expand(v)?))).collect()
    }

    /// Env of a non-PTY child: project env, the tool's own env on top, and the login shell's PATH
    /// when neither sets one (a Dock-launched Kelta has a bare PATH, so `sl`/`node` are not found).
    fn launch_env(
        &self,
        project: &ProjectId,
        vars: &Vars,
        tool_env: &BTreeMap<String, String>,
    ) -> Result<BTreeMap<String, String>, KeltaError> {
        let mut env = Self::expand_env(vars, &self.settings(Some(project)).env)?;
        env.extend(Self::expand_env(vars, tool_env)?);
        if let Some(p) = self.core().and_then(|c| c.login_path()) {
            env.entry("PATH".to_owned()).or_insert(p);
        }
        Ok(env)
    }

    async fn open_pty(
        &self,
        project: &ProjectId,
        r: &Resolved,
        ctx: &TemplateCtx,
        vars: &Vars,
        placement: Placement,
    ) -> Result<ToolHandle, KeltaError> {
        let def = &r.def;
        let command = vars.expand(def.command.as_deref().unwrap_or_default())?;
        if command.is_empty() {
            return Err(KeltaError::invalid(format!("tool `{}` has no command", r.id)));
        }
        self.check_tool_permission(r, Some(&command)).await?;
        let core = self.core_or_err()?;
        let req = SpawnRequest {
            id: None,
            project_id: project.clone(),
            kind: SessionKind::Tool { tool_id: r.id.clone() },
            name: Some(if def.label.is_empty() { r.id.to_string() } else { def.label.clone() }),
            program: Some(command),
            args: vars.expand_all(&def.args)?,
            cwd: Some(self.expand_cwd(def.cwd.as_deref(), vars, project, ctx)?),
            env: Self::expand_env(vars, &def.env)?,
            cols: 80,
            rows: 24,
            work_item_id: ctx.work_item_id.clone(),
            restore: RestorePolicy::Relaunch,
            close_on_exit: def.close_on_exit,
            template_id: None,
        };
        let session = core.session_spawn(req).await?;
        core.layout_open(
            project,
            OpenPaneRequest {
                content: PaneContent::Terminal { session_id: session.id.clone() },
                placement,
                focus: true,
                tab_title: Some(session.name.clone()),
                work_item_id: ctx.work_item_id.clone(),
            },
        )
        .await?;
        self.publish(
            BusEvent::new(
                bus::TOOL_OPENED,
                json!({ "tool_id": r.id, "instance_id": session.id, "kind": "pty" }),
            )
            .with_project(project.clone())
            .with_session(session.id.clone()),
        );
        Ok(ToolHandle::Pty { session_id: session.id })
    }

    /// Launches the tool detached: Kelta keeps no handle, so closing or crashing Kelta leaves it
    /// running. Project env first, the tool's own env on top.
    async fn open_external(
        &self,
        project: &ProjectId,
        r: &Resolved,
        ctx: &TemplateCtx,
        vars: &Vars,
    ) -> Result<ToolHandle, KeltaError> {
        let def = &r.def;
        let command = vars.expand(def.command.as_deref().unwrap_or_default())?;
        if command.is_empty() {
            return Err(KeltaError::invalid(format!("tool `{}` has no command", r.id)));
        }
        self.check_tool_permission(r, Some(&command)).await?;
        let env = self.launch_env(project, vars, &def.env)?;
        let cwd = self.expand_cwd(def.cwd.as_deref(), vars, project, ctx)?;
        let mut child = external_command(&command, &vars.expand_all(&def.args)?, &cwd, &env)
            .spawn()
            .map_err(|e| match e.kind() {
                std::io::ErrorKind::NotFound => KeltaError::not_found(format!("`{command}` not found")),
                _ => KeltaError::internal(format!("cannot start `{command}`: {e}")),
            })?;
        // Reap (no zombie from a quick-exiting launcher like `open`) and log failed launches.
        let id = r.id.clone();
        tokio::spawn(async move {
            if let Ok(s) = child.wait().await
                && !s.success()
            {
                tracing::warn!(tool = %id, status = ?s, "external tool exited with error");
            }
        });
        self.publish(
            BusEvent::new(bus::TOOL_OPENED, json!({ "tool_id": r.id, "kind": "external" }))
                .with_project(project.clone()),
        );
        Ok(ToolHandle::External)
    }

    async fn decide_embed(&self, pref: EmbedMode, url: &str) -> EmbedMode {
        let host =
            url.parse::<axum::http::Uri>().ok().and_then(|u| u.host().map(str::to_owned)).unwrap_or_default();
        let loopback = is_loopback_host(&host) && url.starts_with("http://");
        match pref {
            EmbedMode::External => EmbedMode::External,
            EmbedMode::Iframe => EmbedMode::Iframe,
            EmbedMode::Proxy => {
                if loopback {
                    EmbedMode::Proxy
                } else {
                    EmbedMode::External
                }
            }
            EmbedMode::Auto => {
                let blocked = if url.starts_with("http://") {
                    crate::proxy::probe_http(url).await
                } else if let Some(core) = self.core() {
                    core.http_fetch(ProxiedRequest {
                        url: url.to_owned(),
                        method: "HEAD".into(),
                        timeout_ms: Some(2000),
                        ..Default::default()
                    })
                    .await
                    .map(|resp| {
                        resp.headers.iter().any(|(k, v)| {
                            let k = k.to_ascii_lowercase();
                            k == "x-frame-options"
                                || (k == "content-security-policy"
                                    && v.to_ascii_lowercase().contains("frame-ancestors"))
                        })
                    })
                } else {
                    Ok(false)
                };
                match blocked {
                    Ok(true) if loopback => EmbedMode::Proxy,
                    Ok(true) => EmbedMode::External,
                    Ok(false) | Err(_) => EmbedMode::Iframe,
                }
            }
        }
    }

    async fn open_web(
        &self,
        project: &ProjectId,
        r: &Resolved,
        ctx: &TemplateCtx,
        mut vars: Vars,
        placement: Placement,
    ) -> Result<ToolHandle, KeltaError> {
        let def = &r.def;
        let settings = self.settings(Some(project));
        let program = def.start.as_ref().map(|s| vars.expand(&s.command)).transpose()?;
        self.check_tool_permission(r, program.as_deref()).await?;
        let core = self.core_or_err()?;
        let instance_id = ToolInstanceId::generate();
        let secret = def.url_is_secret.unwrap_or(def.start.is_some());

        let (proc_, url, stop_argv) = match (&def.start, program) {
            (Some(start), Some(program)) => {
                let port = web::free_port()?;
                vars.set("port", json!(port));
                let cwd = self.expand_cwd(start.cwd.as_deref(), &vars, project, ctx)?;
                // Expanded and checked before the spawn: nothing below may fail with a live server.
                let url_tpl = def.url.as_deref().map(|u| vars.expand(u)).transpose()?;
                let stop_argv = match &start.stop {
                    StopSpec::Command { command } => Some((vars.expand_all(command)?, cwd.clone())),
                    StopSpec::Signal { .. } => None,
                };
                if let Some((argv, _)) = &stop_argv {
                    self.check_tool_permission(r, argv.first().map(String::as_str)).await?;
                }
                let launch = Launch {
                    program,
                    args: vars.expand_all(&start.args)?,
                    cwd: cwd.clone(),
                    env: self.launch_env(project, &vars, &start.env)?,
                    ready: start.ready.clone(),
                    ready_timeout: Duration::from_millis(start.ready_timeout_ms.max(100)),
                    port,
                };
                let (proc_, ready_url) = web::start(&launch).await?;
                let url = ready_url.or(url_tpl).unwrap_or_else(|| format!("http://127.0.0.1:{port}/"));
                (Some(proc_), url, stop_argv)
            }
            _ => {
                let url = vars.expand(def.url.as_deref().unwrap_or_default())?;
                if url.is_empty() {
                    return Err(KeltaError::invalid(format!("web tool `{}` has no url", r.id)));
                }
                (None, url, None)
            }
        };
        let pid = proc_.as_ref().map(|p| p.pid);
        let res: Result<ToolHandle, KeltaError> = async {
            let log_url = if secret { kelta_proto::redact::redact_url(&url) } else { url.clone() };
            tracing::info!(tool = %r.id, url = %log_url, "web tool ready");

            let pref = def.embed.unwrap_or(settings.web.embed_default);
            let mut embed = self.decide_embed(pref, &url).await;
            let mut public_url = url.clone();
            if embed == EmbedMode::Proxy {
                match crate::proxy::register(instance_id.as_str(), &url) {
                    Ok(_) => {
                        let port = crate::proxy::ensure_listener(instance_id.as_str()).await?;
                        public_url = format!(
                            "http://127.0.0.1:{port}{}",
                            crate::proxy::proxy_path(instance_id.as_str(), &url)
                        );
                    }
                    Err(e) => {
                        tracing::warn!(error = %e.message, "web proxy refused; opening externally");
                        embed = EmbedMode::External;
                    }
                }
            }
            let inst = Arc::new(WebInstance {
                tool_id: r.id.clone(),
                project_id: project.clone(),
                lifecycle: def.lifecycle,
                plugin: r.plugin.as_ref().map(|e| e.id.clone()),
                proc_: proc_.clone(),
                stop: def.start.as_ref().map(|s| s.stop.clone()).unwrap_or_default(),
                stop_argv,
                closing: AtomicBool::new(false),
            });
            self.tools.web.lock().insert(instance_id.to_string(), inst.clone());
            if let Some(p) = proc_ {
                self.watch_exit(instance_id.clone(), inst, p);
            }

            let handle = ToolHandle::Web { instance_id: instance_id.clone(), url: public_url, embed };
            let label = if def.label.is_empty() { r.id.to_string() } else { def.label.clone() };
            self.emit_ui(UiEvent::PluginEvent {
                instance_id: ScreenInstanceId::new(instance_id.to_string()),
                name: TOOL_HANDLE_EVENT.into(),
                payload: json!({
                    "handle": handle,
                    "tool_id": r.id,
                    "label": label,
                    "project_id": project,
                    "lifecycle": def.lifecycle,
                }),
            });
            if embed != EmbedMode::External {
                core.layout_open(
                    project,
                    OpenPaneRequest {
                        content: PaneContent::Web { tool_instance_id: instance_id.clone() },
                        placement,
                        focus: true,
                        tab_title: Some(label),
                        work_item_id: ctx.work_item_id.clone(),
                    },
                )
                .await?;
            }
            self.publish(
                BusEvent::new(
                    bus::TOOL_OPENED,
                    json!({ "tool_id": r.id, "instance_id": instance_id, "kind": "web", "embed": embed }),
                )
                .with_project(project.clone()),
            );
            Ok(handle)
        }
        .await;
        if res.is_err() {
            // Nothing else knows this server yet: stop it so it does not outlive the failed open.
            crate::proxy::unregister(instance_id.as_str());
            if let Some(inst) = self.tools.web.lock().remove(instance_id.as_str()) {
                inst.closing.store(true, Ordering::SeqCst);
            }
            if let Some(pid) = pid {
                signal_group(pid, rustix::process::Signal::KILL);
            }
        }
        res
    }

    fn watch_exit(&self, id: ToolInstanceId, inst: Arc<WebInstance>, p: ServerProc) {
        let me = self.me.clone();
        tokio::spawn(async move {
            let mut rx = p.exit.clone();
            let _ = rx.wait_for(Option::is_some).await;
            let code = p.exited().unwrap_or(-1);
            let Some(host) = me.upgrade() else { return };
            crate::proxy::unregister(id.as_str());
            host.tools.web.lock().remove(id.as_str());
            if !inst.closing.load(Ordering::SeqCst) {
                host.emit_ui(UiEvent::PluginEvent {
                    instance_id: ScreenInstanceId::new(id.to_string()),
                    name: TOOL_EXITED_EVENT.into(),
                    payload: json!({ "code": code, "log": truncate(&p.log_tail(40), 8192), "tool_id": inst.tool_id }),
                });
            }
            if let Some(core) = host.core() {
                core.publish(
                    BusEvent::new(
                        bus::TOOL_EXITED,
                        json!({ "tool_id": inst.tool_id, "instance_id": id, "code": code }),
                    )
                    .with_project(inst.project_id.clone()),
                );
            }
        });
    }

    pub(crate) async fn close_tool(&self, instance: &ToolInstanceId) -> Result<(), KeltaError> {
        let Some(inst) = self.tools.web.lock().remove(instance.as_str()) else { return Ok(()) };
        inst.closing.store(true, Ordering::SeqCst);
        crate::proxy::unregister(instance.as_str());
        if let Some(p) = &inst.proc_ {
            web::stop(p, &inst.stop, inst.stop_argv.clone()).await;
        }
        Ok(())
    }

    /// `project.closed`: stop web tools bound to the project (`on_close` and `on_project_close`).
    pub(crate) async fn close_project_tools(&self, project: &ProjectId) {
        let ids: Vec<String> = self
            .tools
            .web
            .lock()
            .iter()
            .filter(|(_, i)| &i.project_id == project && i.lifecycle != WebLifecycle::Never)
            .map(|(k, _)| k.clone())
            .collect();
        for id in ids {
            let _ = self.close_tool(&ToolInstanceId::new(id)).await;
        }
    }

    pub(crate) async fn close_plugin_tools(&self, plugin: &PluginId) {
        let ids: Vec<String> = self
            .tools
            .web
            .lock()
            .iter()
            .filter(|(_, i)| i.plugin.as_ref() == Some(plugin))
            .map(|(k, _)| k.clone())
            .collect();
        for id in ids {
            let _ = self.close_tool(&ToolInstanceId::new(id)).await;
        }
    }

    /// Process ids of running web tool servers (perf reporting of children, tests).
    pub fn web_tool_pids(&self) -> Vec<u32> {
        self.tools.web.lock().values().filter_map(|i| i.proc_.as_ref().map(|p| p.pid)).collect()
    }

    /// Number of running web tool instances (tests, perf).
    pub fn web_tools_running(&self) -> usize {
        self.tools.running()
    }
}

/// Run a `check` argv (5 s cap): exit 0 = installed; first output line = version.
async fn run_check(argv: &[String], path: Option<&str>, hint: Option<String>) -> ToolCheck {
    let Some((program, args)) = argv.split_first() else {
        return ToolCheck { installed: false, version: None, install_hint: hint };
    };
    let mut cmd = tokio::process::Command::new(program);
    cmd.args(args).stdin(std::process::Stdio::null()).kill_on_drop(true);
    if let Some(p) = path {
        cmd.env("PATH", p);
    }
    // one-shot: tool_check deadline
    let out = tokio::time::timeout(Duration::from_secs(5), cmd.output()).await;
    match out {
        Ok(Ok(o)) if o.status.success() => {
            let text =
                String::from_utf8_lossy(if o.stdout.is_empty() { &o.stderr } else { &o.stdout }).into_owned();
            let version = text.lines().map(str::trim).find(|l| !l.is_empty()).map(|l| truncate(l, 120));
            ToolCheck { installed: true, version, install_hint: None }
        }
        _ => ToolCheck { installed: false, version: None, install_hint: hint },
    }
}

#[cfg(test)]
mod external_tests {
    use super::*;

    #[test]
    fn external_command_argv_cwd_env() {
        let env = BTreeMap::from([("A".to_owned(), "1".to_owned())]);
        let c = external_command(
            "open",
            &["-a".into(), "Fork".into(), ".".into()],
            std::path::Path::new("/tmp"),
            &env,
        );
        let c = c.as_std();
        assert_eq!(c.get_program(), "open");
        assert_eq!(c.get_args().collect::<Vec<_>>(), ["-a", "Fork", "."]);
        assert_eq!(c.get_current_dir(), Some(std::path::Path::new("/tmp")));
        assert_eq!(
            c.get_envs().collect::<Vec<_>>(),
            [(std::ffi::OsStr::new("A"), Some(std::ffi::OsStr::new("1")))]
        );
    }

    #[tokio::test]
    async fn external_command_uses_env_path_for_lookup() {
        let dir = std::env::temp_dir().join(format!("kelta-ext-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        // Written by a child `sh`: a write fd held here leaks into concurrent forks -> ETXTBSY (Linux).
        let ok = std::process::Command::new("sh")
            .args(["-c", "printf '#!/bin/sh\\nexit 0\\n' > \"$1\" && chmod 755 \"$1\"", "sh"])
            .arg(dir.join("kelta-fake-tool"))
            .status()
            .unwrap();
        assert!(ok.success());
        let env = BTreeMap::from([("PATH".to_owned(), dir.to_string_lossy().into_owned())]);
        let st = external_command("kelta-fake-tool", &[], &dir, &env).status().await.unwrap();
        assert!(st.success());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn kind_serde() {
        assert_eq!(serde_json::to_string(&ToolKind::External).unwrap(), "\"external\"");
        let d: ToolDef = serde_json::from_str(r#"{"id":"fork","kind":"external","command":"open"}"#).unwrap();
        assert_eq!(d.kind, ToolKind::External);
    }
}

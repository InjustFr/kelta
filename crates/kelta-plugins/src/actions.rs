//! Action executor (PLUGINS §3.1) shared by triggers and commands, contributed/configured palette
//! commands (`commands`, `command_run`), and the in-app `prompt` confirmations. Actions of plugin
//! triggers/commands are permission-checked against the plugin's effective grants; config-defined
//! ones are not (repo-local exec keys are already inert until trusted, SETTINGS §4).

use std::future::Future;
use std::pin::Pin;
use std::process::Stdio;
use std::sync::Arc;
use std::time::Duration;

use kelta_proto::error::{ErrorCode, KeltaError};
use kelta_proto::events::{BusEvent, Notification, Toast, ToastAction, ToastLevel, TriggerChain, bus};
use kelta_proto::ext::{
    ActionDef, CommandDef, CommandWhen, Permission, ProxiedRequest, RunShow, RunStdin, Urgency,
};
use kelta_proto::ids::{AccountId, PluginId, ProjectId, SessionId, ToolId};
use kelta_proto::model::{Attention, SessionKind, SessionStatus, StatusChange, TemplateCtx};
use kelta_proto::tracker::{Assignee, StatusCategory, TicketRef};
use serde_json::{Map, Value, json};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWriteExt};

use crate::PluginHost;
use crate::context::CtxSpec;
use crate::perms::Granted;
use crate::registry::Entry;
use crate::screens::{SpawnParams, keys_bytes};
use crate::template::Vars;
use crate::util::{signal_group, truncate};

tokio::task_local! {
    /// Chain of the action being executed: events published while it runs carry it.
    pub(crate) static CHAIN: TriggerChain;
}

/// Command id prefix answering a `prompt` action (`kelta.prompt:<token>`).
pub const PROMPT_COMMAND_PREFIX: &str = "kelta.prompt:";
/// UI action id carried by prompt toasts (registered by the UI: answers through `command_run`).
pub const PROMPT_UI_ACTION: &str = "trigger.prompt";
const MAX_NESTED: u8 = 4;
const RUN_OUTPUT_CAP: usize = 64 * 1024;
/// Patchable keys of blocking pre-events (v0.1).
pub const PATCHABLE: &[&str] = &["branch", "template_id", "claude.prompt"];

/// Veto/patch collected while a blocking trigger runs.
#[derive(Debug, Default)]
pub struct BlockState {
    pub veto: Option<String>,
    pub patch: Map<String, Value>,
}

/// Execution context of one trigger run or command run.
#[derive(Clone)]
pub(crate) struct ActionCx {
    pub key: String,
    pub project: Option<ProjectId>,
    pub session: Option<SessionId>,
    pub event: Option<BusEvent>,
    pub vars: Vars,
    /// Chain carried by events published by these actions.
    pub chain: TriggerChain,
    /// `None` = config-defined (not permission-gated).
    pub granted: Option<Granted>,
    pub plugin: Option<Arc<Entry>>,
    pub allow_send_keys: bool,
    pub nested: u8,
}

impl ActionCx {
    fn require(&self, p: Permission) -> Result<(), KeltaError> {
        self.granted.as_ref().map_or(Ok(()), |g| g.require(p))
    }

    fn require_exec(&self, cmd: &str) -> Result<(), KeltaError> {
        self.granted.as_ref().map_or(Ok(()), |g| g.require_exec(cmd))
    }

    fn project(&self) -> Result<ProjectId, KeltaError> {
        self.project
            .clone()
            .ok_or_else(|| KeltaError::invalid("this action needs a project (the event has none)"))
    }

    fn x(&self, s: &str) -> Result<String, KeltaError> {
        self.vars.expand(s)
    }
}

/// A `prompt` waiting for its "Yes".
#[derive(Clone)]
pub(crate) struct PendingPrompt {
    pub yes: Vec<ActionDef>,
    pub cx: ActionCx,
}

/// A palette command with its plugin (if contributed).
#[derive(Clone)]
pub(crate) struct CommandEntry {
    pub def: CommandDef,
    pub plugin: Option<Arc<Entry>>,
}

type BoxFut<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

fn parse_json_line(stdout: &str) -> Option<Value> {
    let trimmed = stdout.trim();
    if let Ok(v @ Value::Object(_)) = serde_json::from_str::<Value>(trimmed) {
        return Some(v);
    }
    trimmed.lines().rev().map(str::trim).filter(|l| !l.is_empty()).find_map(|l| {
        match serde_json::from_str::<Value>(l) {
            Ok(v @ Value::Object(_)) => Some(v),
            _ => None,
        }
    })
}

/// Keep only the v0.1 patchable keys (`branch`, `template_id`, `claude.prompt`), flattened.
pub fn filter_patch(patch: &Value) -> Map<String, Value> {
    let mut out = Map::new();
    for key in PATCHABLE {
        let v = patch.get(*key).or_else(|| crate::util::json_path(patch, key));
        if let Some(v) = v.filter(|v| !v.is_null()) {
            out.insert((*key).to_owned(), v.clone());
        }
    }
    out
}

fn tail(s: &str) -> String {
    s.lines().rev().map(str::trim).find(|l| !l.is_empty()).map(|l| truncate(l, 300)).unwrap_or_default()
}

impl PluginHost {
    /// Publish a bus event; events published while an action runs carry the trigger chain.
    pub(crate) fn publish(&self, mut ev: BusEvent) {
        if let Ok(chain) = CHAIN.try_with(Clone::clone) {
            ev.chain = chain;
        }
        if let Some(core) = self.core() {
            core.publish(ev);
        }
    }

    /// Run actions in order. Returns `(ok, details)`.
    pub(crate) async fn run_actions(
        &self,
        actions: &[ActionDef],
        cx: &mut ActionCx,
        continue_on_error: bool,
        mut block: Option<&mut BlockState>,
    ) -> (bool, Vec<String>) {
        let mut ok = true;
        let mut details = Vec::new();
        for (i, a) in actions.iter().enumerate() {
            let chain = cx.chain.clone();
            let res = CHAIN.scope(chain, self.run_action(a, cx, block.as_deref_mut())).await;
            match res {
                Ok(d) => {
                    if !d.is_empty() {
                        details.push(d);
                    }
                }
                Err(e) => {
                    ok = false;
                    details.push(format!("action {} ({}): {}", i + 1, action_name(a), e.message));
                    if !continue_on_error {
                        break;
                    }
                }
            }
            if block.as_ref().is_some_and(|b| b.veto.is_some()) {
                break;
            }
        }
        (ok, details)
    }

    fn run_action<'a>(
        &'a self,
        a: &'a ActionDef,
        cx: &'a mut ActionCx,
        block: Option<&'a mut BlockState>,
    ) -> BoxFut<'a, Result<String, KeltaError>> {
        Box::pin(async move { self.run_action_inner(a, cx, block).await })
    }

    async fn run_action_inner(
        &self,
        a: &ActionDef,
        cx: &mut ActionCx,
        block: Option<&mut BlockState>,
    ) -> Result<String, KeltaError> {
        let core = self.core_or_err()?;
        match a {
            ActionDef::Notify { title, body, urgency } => {
                cx.require(Permission::Notify)?;
                core.notify(Notification {
                    title: cx.x(title)?,
                    body: body.as_deref().map(|b| cx.x(b)).transpose()?,
                    urgency: urgency.unwrap_or(Urgency::Normal),
                    project_id: cx.project.clone(),
                    session_id: cx.session.clone(),
                })
                .await?;
                Ok(String::new())
            }
            ActionDef::Toast { text, level } => {
                core.toast(Toast {
                    level: level.unwrap_or_default(),
                    text: truncate(&cx.x(text)?, 500),
                    action: None,
                });
                Ok(String::new())
            }
            ActionDef::Run { command, args, cwd, env, stdin, timeout_ms, show } => {
                let command = cx.x(command)?;
                cx.require_exec(&command)?;
                let args = cx.vars.expand_all(args)?;
                let cwd = cwd.as_deref().map(|c| cx.x(c)).transpose()?.filter(|c| !c.is_empty());
                let env: std::collections::BTreeMap<String, String> =
                    env.iter().map(|(k, v)| Ok((k.clone(), cx.x(v)?))).collect::<Result<_, KeltaError>>()?;
                if *show == Some(RunShow::Pane) && block.is_none() {
                    let project = cx.project()?;
                    self.spawn_session(
                        &project,
                        SpawnParams { command: Some(command), args, cwd, ..Default::default() },
                        None,
                    )
                    .await?;
                    return Ok(String::new());
                }
                let stdin_json = match stdin.unwrap_or(RunStdin::Event) {
                    RunStdin::Event => cx.event.as_ref().map(|e| serde_json::to_vec(e).unwrap_or_default()),
                    RunStdin::None => None,
                };
                let out = run_process(
                    &command,
                    &args,
                    cwd.as_deref(),
                    &env,
                    stdin_json,
                    Duration::from_millis(timeout_ms.unwrap_or(60_000)),
                )
                .await?;
                if let Some(b) = block {
                    if out.code != 0 {
                        let reason =
                            [tail(&out.stdout), tail(&out.stderr)].into_iter().find(|s| !s.is_empty());
                        b.veto = Some(
                            reason.unwrap_or_else(|| format!("`{command}` exited with code {}", out.code)),
                        );
                        return Ok(String::new());
                    }
                    if let Some(v) = parse_json_line(&out.stdout) {
                        if let Some(r) = v.get("veto") {
                            b.veto = Some(crate::util::json_to_string(r));
                            return Ok(String::new());
                        }
                        if let Some(p) = v.get("patch") {
                            b.patch.extend(filter_patch(p));
                        }
                    }
                    return Ok(truncate(out.stdout.trim(), 500));
                }
                if out.code != 0 {
                    let msg = format!(
                        "`{command}` exited with code {}: {}",
                        out.code,
                        tail(&out.stderr).max(tail(&out.stdout))
                    );
                    if *show == Some(RunShow::ToastOnError) {
                        core.toast(Toast::error(truncate(&msg, 300)));
                    }
                    return Err(KeltaError::upstream(msg));
                }
                Ok(truncate(out.stdout.trim(), 500))
            }
            ActionDef::SpawnSession { template, command, args, cwd, placement, focus: _ } => {
                cx.require(Permission::SessionsSpawn)?;
                let command = command.as_deref().map(|c| cx.x(c)).transpose()?;
                if let Some(c) = &command {
                    cx.require_exec(c)?;
                }
                let project = cx.project()?;
                let params = SpawnParams {
                    template: template.as_deref().map(|t| cx.x(t)).transpose()?,
                    command,
                    args: cx.vars.expand_all(args)?,
                    cwd: cwd.as_deref().map(|c| cx.x(c)).transpose()?.filter(|c| !c.is_empty()),
                    placement: *placement,
                    project: None,
                };
                self.spawn_session(&project, params, None).await?;
                Ok(String::new())
            }
            ActionDef::SendKeys { session, text, bracketed } => {
                if !cx.allow_send_keys {
                    return Err(KeltaError::permission_denied("allow_send_keys"));
                }
                cx.require(Permission::TerminalWrite)?;
                let target = self.resolve_session(cx, &cx.x(session)?)?;
                if !self.engine.send_keys_ok(target.as_str()) {
                    return Err(KeltaError::rate_limited(
                        format!(
                            "send_keys to {target}: at most one every {} s",
                            crate::triggers::SEND_KEYS_INTERVAL.as_secs()
                        ),
                        Some(crate::triggers::SEND_KEYS_INTERVAL.as_millis() as u64),
                    ));
                }
                core.session_write(&target, &keys_bytes(&cx.x(text)?, bracketed.unwrap_or(true))).await?;
                Ok(String::new())
            }
            ActionDef::OpenTool { tool, placement } => {
                cx.require(Permission::SessionsSpawn)?;
                let project = cx.project()?;
                let mut id = cx.x(tool)?;
                if let Some(p) = &cx.plugin
                    && !id.contains('/')
                {
                    id = format!("{}/{id}", p.id);
                }
                let tctx = TemplateCtx { session_id: cx.session.clone(), ..Default::default() };
                self.open_tool(&project, &ToolId::new(id), tctx, placement.unwrap_or_default()).await?;
                Ok(String::new())
            }
            ActionDef::OpenScreen { plugin, screen, params, placement } => {
                cx.require(Permission::UiOpen)?;
                let project = cx.project.clone().unwrap_or_else(ProjectId::home);
                let plugin_id = match (plugin, &cx.plugin) {
                    (Some(p), _) => PluginId::new(cx.x(p)?),
                    (None, Some(e)) => e.id.clone(),
                    (None, None) => return Err(KeltaError::invalid("open_screen needs `plugin`")),
                };
                if let Some(own) = &cx.plugin
                    && own.id != plugin_id
                {
                    return Err(KeltaError::permission_denied(format!("ui.open (screens of `{plugin_id}`)")));
                }
                self.open_screen_pane(
                    &plugin_id,
                    &cx.x(screen)?,
                    &project,
                    params.clone().unwrap_or(Value::Null),
                    placement.unwrap_or_default(),
                )
                .await?;
                Ok(String::new())
            }
            ActionDef::StartWork { ticket, project } => {
                cx.require(Permission::TicketsRead)?;
                cx.require(Permission::SessionsSpawn)?;
                let project = match project {
                    Some(p) => Some(ProjectId::new(cx.x(p)?)),
                    None => cx.project.clone(),
                };
                core.ctl(kelta_proto::ctl::CtlCommand::Start { ticket: cx.x(ticket)?, project }).await?;
                Ok(String::new())
            }
            ActionDef::TransitionTicket { to_category, to_name } => {
                cx.require(Permission::TicketsWrite)?;
                let t = self.ticket_ref(cx)?;
                let tracker = core.tracker_for(&t.account).await?;
                let transitions = tracker.transitions(&t).await?;
                let wanted_name = to_name.as_deref().map(|n| cx.x(n)).transpose()?;
                let pick = transitions.iter().find(|tr| match (&wanted_name, to_category) {
                    (Some(n), _) => tr.name.eq_ignore_ascii_case(n) || tr.to.name.eq_ignore_ascii_case(n),
                    (None, Some(c)) => tr.to.category == *c,
                    (None, None) => false,
                });
                let Some(tr) = pick else {
                    let target = wanted_name.unwrap_or_else(|| {
                        format!("{:?}", to_category.unwrap_or(StatusCategory::Unknown)).to_lowercase()
                    });
                    return Err(KeltaError::not_found(format!("no transition of {} to `{target}`", t.key)));
                };
                let ticket = tracker.transition(&t, &tr.id, None).await?;
                self.publish(with_ids(
                    BusEvent::new(bus::TICKET_TRANSITIONED, json!({ "ticket": ticket, "to": tr.to })),
                    cx,
                ));
                Ok(format!("{} → {}", t.key, tr.to.name))
            }
            ActionDef::CommentTicket { body } => {
                cx.require(Permission::TicketsWrite)?;
                let t = self.ticket_ref(cx)?;
                let body = cx.x(body)?;
                core.tracker_for(&t.account).await?.comment(&t, &body).await?;
                self.publish(with_ids(
                    BusEvent::new(bus::TICKET_COMMENTED, json!({ "ticket": t, "markdown": body })),
                    cx,
                ));
                Ok(String::new())
            }
            ActionDef::AssignTicket { to } => {
                cx.require(Permission::TicketsWrite)?;
                let t = self.ticket_ref(cx)?;
                let who = match cx.x(to)?.as_str() {
                    "me" => Assignee::Me,
                    "none" | "" => Assignee::None,
                    other => Assignee::User { id: other.to_owned() },
                };
                let ticket = core.tracker_for(&t.account).await?.assign(&t, who).await?;
                self.publish(with_ids(BusEvent::new(bus::TICKET_ASSIGNED, json!({ "ticket": ticket })), cx));
                Ok(String::new())
            }
            ActionDef::Http { url, method, headers, body, secret_headers, timeout_ms } => {
                let url = cx.x(url)?;
                if let Some(g) = &cx.granted {
                    g.require_net(&url)?;
                }
                if !secret_headers.is_empty() {
                    return Err(KeltaError::unsupported(
                        "`secret_headers` need secret resolution, which the plugin host cannot do yet",
                    ));
                }
                let body = match body {
                    Some(b) => cx.x(b)?,
                    None => cx
                        .event
                        .as_ref()
                        .map(|e| serde_json::to_string(e).unwrap_or_default())
                        .unwrap_or_default(),
                };
                let mut hdrs = std::collections::BTreeMap::new();
                for (k, v) in headers {
                    hdrs.insert(k.clone(), cx.x(v)?);
                }
                if body.trim_start().starts_with(['{', '['])
                    && !hdrs.keys().any(|k| k.eq_ignore_ascii_case("content-type"))
                {
                    hdrs.insert("Content-Type".into(), "application/json".into());
                }
                let resp = core
                    .http_fetch(ProxiedRequest {
                        url,
                        method: method.clone().unwrap_or_else(|| "POST".into()).to_ascii_uppercase(),
                        headers: hdrs,
                        body: Some(body),
                        body_base64: false,
                        timeout_ms: Some(timeout_ms.unwrap_or(10_000)),
                    })
                    .await?;
                if resp.status >= 400 {
                    return Err(KeltaError::upstream(format!("HTTP {}", resp.status)));
                }
                Ok(format!("HTTP {}", resp.status))
            }
            ActionDef::Focus { project, session } => {
                cx.require(Permission::UiOpen)?;
                let project = project
                    .as_deref()
                    .map(|p| cx.x(p))
                    .transpose()?
                    .filter(|p| !p.is_empty())
                    .map(ProjectId::new);
                let session = match session {
                    Some(s) => Some(self.resolve_session(cx, &cx.x(s)?)?),
                    None => None,
                };
                self.focus(project.or_else(|| cx.project.clone()), session).await?;
                Ok(String::new())
            }
            ActionDef::SetAttention { session, level } => {
                cx.require(Permission::SessionsWrite)?;
                let target = self.resolve_session(cx, &cx.x(session)?)?;
                let status = match level {
                    Attention::NeedsInput => SessionStatus::NeedsInput,
                    Attention::Done => SessionStatus::Done,
                    Attention::Error => SessionStatus::Error,
                    Attention::None | Attention::Activity => SessionStatus::Unknown,
                };
                core.session_apply_hook(
                    &target,
                    StatusChange {
                        status,
                        preview: None,
                        file_edited: None,
                        raw_event: format!("trigger:{}", cx.key),
                        session_uuid: None,
                    },
                )
                .await?;
                Ok(String::new())
            }
            ActionDef::Prompt { text, yes, no: _ } => {
                let token = uuid::Uuid::new_v4().simple().to_string();
                let text = cx.x(text)?;
                {
                    let mut prompts = self.engine.prompts.lock();
                    if prompts.len() >= 64 {
                        prompts.clear();
                    }
                    prompts.insert(token.clone(), PendingPrompt { yes: yes.clone(), cx: cx.clone() });
                }
                core.toast(Toast {
                    level: ToastLevel::Info,
                    text: truncate(&text, 300),
                    action: Some(ToastAction {
                        label: "Yes".into(),
                        command: PROMPT_UI_ACTION.into(),
                        args: Some(json!({ "command_id": format!("{PROMPT_COMMAND_PREFIX}{token}") })),
                    }),
                });
                Ok(String::new())
            }
            ActionDef::Command { id } => {
                if cx.nested >= MAX_NESTED {
                    return Err(KeltaError::conflict("commands nested too deeply"));
                }
                let id = cx.x(id)?;
                let entry = self
                    .command_defs(cx.project.as_ref())
                    .into_iter()
                    .find(|c| c.def.id == id)
                    .ok_or_else(|| KeltaError::not_found(format!("command `{id}`")))?;
                let mut sub = cx.clone();
                sub.key = id.clone();
                sub.nested += 1;
                if let Some(e) = &entry.plugin {
                    sub.granted = Some(self.granted(e).await?);
                    sub.plugin = Some(e.clone());
                }
                let (ok, details) = Box::pin(self.run_actions(&entry.def.r#do, &mut sub, false, None)).await;
                if ok { Ok(details.join("; ")) } else { Err(KeltaError::upstream(details.join("; "))) }
            }
        }
    }

    /// `"claude"` / `"editor"` → that session of the event's project (same work item first).
    fn resolve_session(&self, cx: &ActionCx, s: &str) -> Result<SessionId, KeltaError> {
        let core = self.core_or_err()?;
        let want = match s {
            "claude" => Some("claude"),
            "editor" => Some("editor"),
            "" => return cx.session.clone().ok_or_else(|| KeltaError::invalid("no session in this context")),
            _ => None,
        };
        let Some(kind) = want else { return Ok(SessionId::new(s)) };
        let sessions = core.session_list(cx.project.as_ref());
        let work = cx.session.as_ref().and_then(|id| core.session_get(id)).and_then(|s| s.work_item_id);
        let matches = |k: &SessionKind| k.name() == kind;
        sessions
            .iter()
            .filter(|x| matches(&x.kind))
            .find(|x| work.is_some() && x.work_item_id == work)
            .or_else(|| sessions.iter().find(|x| matches(&x.kind)))
            .map(|x| x.id.clone())
            .ok_or_else(|| KeltaError::not_found(format!("no {kind} session in this project")))
    }

    /// The ticket of the context; for `pr.*` events the first linked ticket on the project tracker.
    fn ticket_ref(&self, cx: &ActionCx) -> Result<TicketRef, KeltaError> {
        let key = cx
            .vars
            .get("ticket.key")
            .and_then(Value::as_str)
            .map(str::to_owned)
            .or_else(|| cx.vars.get("payload.linked_tickets.0").and_then(Value::as_str).map(str::to_owned))
            .ok_or_else(|| KeltaError::invalid("no ticket in this context"))?;
        let account = cx
            .vars
            .get("ticket.account")
            .and_then(Value::as_str)
            .filter(|a| !a.is_empty())
            .or_else(|| cx.vars.get("project.tracker").and_then(Value::as_str))
            .ok_or_else(|| KeltaError::invalid(format!("no tracker account for ticket {key}")))?;
        let id = cx.vars.get("ticket.id").and_then(Value::as_str).unwrap_or(&key).to_owned();
        Ok(TicketRef { account: AccountId::new(account), key, id })
    }

    /// Config `[[commands]]` + plugin `contributes.commands` (keybindings applied) + ticket/review
    /// action buttons (`<plugin>/<id>`, `when = ticket|review`).
    pub(crate) fn command_defs(&self, project: Option<&ProjectId>) -> Vec<CommandEntry> {
        let mut out: Vec<CommandEntry> = self
            .settings(project)
            .commands
            .iter()
            .filter(|c| c.enabled && !c.id.is_empty())
            .map(|c| CommandEntry { def: c.clone(), plugin: None })
            .collect();
        for e in self.active() {
            let Some(m) = e.manifest() else { continue };
            for c in m.contributes.commands.iter().filter(|c| c.enabled) {
                let mut def = c.clone();
                if def.keybinding.is_none() {
                    def.keybinding =
                        m.contributes.keybindings.iter().find(|k| k.command == c.id).map(|k| k.key.clone());
                }
                out.push(CommandEntry { def, plugin: Some(e.clone()) });
            }
            for (when, list) in [
                (CommandWhen::Ticket, &m.contributes.ticket_actions),
                (CommandWhen::Review, &m.contributes.review_actions),
            ] {
                for b in list {
                    out.push(CommandEntry {
                        def: CommandDef {
                            id: format!("{}/{}", e.id, b.id),
                            title: b.title.clone(),
                            r#do: b.r#do.clone(),
                            when: Some(when),
                            keybinding: None,
                            enabled: true,
                        },
                        plugin: Some(e.clone()),
                    });
                }
            }
        }
        out
    }

    pub(crate) async fn run_command(&self, command_id: &str, ctx: TemplateCtx) -> Result<(), KeltaError> {
        if let Some(token) = command_id.strip_prefix(PROMPT_COMMAND_PREFIX) {
            let pending = self
                .engine
                .prompts
                .lock()
                .remove(token)
                .ok_or_else(|| KeltaError::not_found("this prompt has expired"))?;
            let mut cx = pending.cx;
            let (ok, details) = self.run_actions(&pending.yes, &mut cx, false, None).await;
            return if ok { Ok(()) } else { Err(KeltaError::upstream(details.join("; "))) };
        }
        let core = self.core_or_err()?;
        let project = ctx
            .extra
            .get("project_id")
            .map(|p| ProjectId::new(p.clone()))
            .or_else(|| ctx.session_id.as_ref().and_then(|s| core.session_get(s)).map(|s| s.project_id));
        let entry = self
            .command_defs(project.as_ref())
            .into_iter()
            .find(|c| c.def.id == command_id)
            .ok_or_else(|| KeltaError::not_found(format!("command `{command_id}`")))?;
        let granted = match &entry.plugin {
            Some(e) => {
                self.mark_activated(&e.id);
                Some(self.granted(e).await?)
            }
            None => None,
        };
        let vars = self
            .build_vars(CtxSpec {
                project: project.as_ref(),
                tctx: Some(&ctx),
                plugin: entry.plugin.as_deref(),
                ..Default::default()
            })
            .await;
        let mut cx = ActionCx {
            key: command_id.to_owned(),
            project,
            session: ctx.session_id.clone(),
            event: None,
            vars,
            chain: TriggerChain { depth: 1, origin_triggers: vec![format!("command:{command_id}")] },
            granted,
            plugin: entry.plugin.clone(),
            allow_send_keys: false,
            nested: 0,
        };
        let (ok, details) = self.run_actions(&entry.def.r#do, &mut cx, false, None).await;
        if ok {
            Ok(())
        } else {
            let msg = details.join("; ");
            let code = if msg.contains("missing permission") {
                ErrorCode::PermissionDenied
            } else {
                ErrorCode::Upstream
            };
            Err(KeltaError::new(code, msg))
        }
    }
}

fn with_ids(mut ev: BusEvent, cx: &ActionCx) -> BusEvent {
    ev.project_id = cx.project.clone();
    ev.session_id = cx.session.clone();
    ev
}

fn action_name(a: &ActionDef) -> &'static str {
    match a {
        ActionDef::Notify { .. } => "notify",
        ActionDef::Toast { .. } => "toast",
        ActionDef::Run { .. } => "run",
        ActionDef::SpawnSession { .. } => "spawn_session",
        ActionDef::SendKeys { .. } => "send_keys",
        ActionDef::OpenTool { .. } => "open_tool",
        ActionDef::OpenScreen { .. } => "open_screen",
        ActionDef::StartWork { .. } => "start_work",
        ActionDef::TransitionTicket { .. } => "transition_ticket",
        ActionDef::CommentTicket { .. } => "comment_ticket",
        ActionDef::AssignTicket { .. } => "assign_ticket",
        ActionDef::Http { .. } => "http",
        ActionDef::Focus { .. } => "focus",
        ActionDef::SetAttention { .. } => "set_attention",
        ActionDef::Prompt { .. } => "prompt",
        ActionDef::Command { .. } => "command",
    }
}

pub(crate) struct ProcOutput {
    pub code: i32,
    pub stdout: String,
    pub stderr: String,
}

/// Background exec (no shell), own process group, capped output, timeout → group SIGKILL.
pub(crate) async fn run_process(
    command: &str,
    args: &[String],
    cwd: Option<&str>,
    env: &std::collections::BTreeMap<String, String>,
    stdin: Option<Vec<u8>>,
    timeout: Duration,
) -> Result<ProcOutput, KeltaError> {
    let mut cmd = tokio::process::Command::new(command);
    cmd.args(args)
        .envs(env)
        .stdin(if stdin.is_some() { Stdio::piped() } else { Stdio::null() })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .process_group(0)
        .kill_on_drop(true);
    if let Some(c) = cwd {
        cmd.current_dir(c);
    }
    let mut child = cmd.spawn().map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            KeltaError::not_found(format!("`{command}` not found"))
        } else {
            KeltaError::internal(format!("cannot run `{command}`: {e}"))
        }
    })?;
    let pid = child.id();
    if let (Some(data), Some(mut w)) = (stdin, child.stdin.take()) {
        tokio::spawn(async move {
            let _ = w.write_all(&data).await;
            let _ = w.shutdown().await;
        });
    }
    let (out, err) = (child.stdout.take(), child.stderr.take());
    let run = async {
        let (stdout, stderr, status) = tokio::join!(capped(out), capped(err), child.wait());
        status.map(|s| (s, stdout, stderr))
    };
    // one-shot: `run` action timeout
    match tokio::time::timeout(timeout, run).await {
        Ok(Ok((status, stdout, stderr))) => Ok(ProcOutput {
            code: status.code().unwrap_or(-1),
            stdout: truncate(&String::from_utf8_lossy(&stdout), RUN_OUTPUT_CAP),
            stderr: truncate(&String::from_utf8_lossy(&stderr), RUN_OUTPUT_CAP),
        }),
        Ok(Err(e)) => Err(KeltaError::internal(format!("`{command}`: {e}"))),
        Err(_) => {
            if let Some(pid) = pid {
                signal_group(pid, rustix::process::Signal::KILL);
            }
            Err(KeltaError::timeout(format!("`{command}` timed out after {} ms", timeout.as_millis())))
        }
    }
}

/// First `RUN_OUTPUT_CAP + 1` bytes (so `truncate` marks overflow); the rest is drained and dropped
/// so the child never blocks on a full pipe and memory stays bounded.
async fn capped<R: AsyncRead + Unpin>(r: Option<R>) -> Vec<u8> {
    let Some(mut r) = r else { return Vec::new() };
    let mut out = Vec::new();
    let _ = (&mut r).take(RUN_OUTPUT_CAP as u64 + 1).read_to_end(&mut out).await;
    let _ = tokio::io::copy(&mut r, &mut tokio::io::sink()).await;
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn run_output_is_capped_while_streaming() {
        // ~1 MiB of output: only the cap is kept.
        let o = run_process(
            "sh",
            &["-c".into(), "head -c 1048576 /dev/zero | tr '\\0' a".into()],
            None,
            &Default::default(),
            None,
            Duration::from_secs(10),
        )
        .await
        .unwrap();
        assert_eq!(o.code, 0);
        assert!(o.stdout.len() <= RUN_OUTPUT_CAP + 4 && o.stdout.ends_with('…'));
    }

    #[test]
    fn patch_filter_keeps_allowed_keys() {
        let p = json!({"branch": "feat/x", "claude": {"prompt": "hi"}, "worktree_path": "/evil", "template_id": null});
        let f = filter_patch(&p);
        assert_eq!(f.get("branch"), Some(&json!("feat/x")));
        assert_eq!(f.get("claude.prompt"), Some(&json!("hi")));
        assert!(!f.contains_key("worktree_path"));
        assert!(!f.contains_key("template_id"));
    }

    #[test]
    fn json_line_detection() {
        assert_eq!(parse_json_line("noise\n{\"veto\":\"dirty\"}\n").unwrap()["veto"], "dirty");
        assert!(parse_json_line("plain text").is_none());
    }
}

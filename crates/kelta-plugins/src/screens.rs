//! Plugin screens: instances (`plugin_screen_open/close`), the `plugin_call` dispatch to `CoreApi`
//! behind the permission gate (PLUGINS §7), and the relay of granted bus events to screens.

use globset::GlobMatcher;
use kelta_proto::codehost::{ReviewKind, ReviewRef};
use kelta_proto::error::KeltaError;
use kelta_proto::events::{BusEvent, Notification, Toast, ToastLevel, UiEvent};
use kelta_proto::ext::{
    CallOrigin, Permission, PluginMethod, ProxiedRequest, ScreenOpenResult, ScreenScope, Urgency,
};
use kelta_proto::ids::{PluginId, ProjectId, ScreenInstanceId, SessionId, ToolId};
use kelta_proto::model::{
    OpenPaneRequest, PaneContent, Placement, RestorePolicy, Scope, SessionKind, SpawnRequest, TemplateCtx,
};
use kelta_proto::tracker::{Assignee, Cursor, TicketItem, TicketPage, TicketRef};
use serde::Deserialize;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};

use crate::PluginHost;
use crate::matcher;
use crate::perms::{self, Granted};

/// One open screen instance.
#[derive(Debug, Clone)]
pub(crate) struct Screen {
    pub plugin: PluginId,
    pub screen_id: String,
    pub project: Option<ProjectId>,
    pub scope: ScreenScope,
    pub subscriptions: Vec<(String, GlobMatcher)>,
}

/// Max `net.fetch` response body (PLUGINS §7).
pub const NET_FETCH_CAP: usize = 5 * 1024 * 1024;
/// `kv.*` limits (PLUGINS §7): key bytes, one value's JSON bytes, keys + values of one plugin.
pub const KV_KEY_CAP: usize = 256;
pub const KV_VALUE_CAP: usize = 64 * 1024;
pub const KV_QUOTA: usize = 1024 * 1024;

#[derive(Deserialize)]
struct KvKey {
    key: String,
}

impl KvKey {
    fn checked(self) -> Result<String, KeltaError> {
        if self.key.is_empty() || self.key.len() > KV_KEY_CAP {
            return Err(KeltaError::invalid(format!("kv: `key` must be 1..={KV_KEY_CAP} bytes")));
        }
        Ok(self.key)
    }
}

fn p<T: DeserializeOwned>(method: PluginMethod, params: Value) -> Result<T, KeltaError> {
    let params = if params.is_null() { json!({}) } else { params };
    serde_json::from_value(params)
        .map_err(|e| KeltaError::invalid(format!("{}: invalid params: {e}", method_name(method))))
}

fn method_name(m: PluginMethod) -> String {
    serde_json::to_value(m).ok().and_then(|v| v.as_str().map(str::to_owned)).unwrap_or_default()
}

fn to_value<T: serde::Serialize>(v: T) -> Result<Value, KeltaError> {
    Ok(serde_json::to_value(v)?)
}

#[derive(Deserialize)]
struct TicketParam {
    ticket: TicketRef,
}

#[derive(Deserialize)]
struct ReviewParam {
    review: ReviewRef,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct ListTickets {
    scope: Option<String>,
    view_id: Option<String>,
    cursor: Option<Cursor>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct ListReviews {
    scope: Option<Value>,
    kind: Option<ReviewKind>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
pub(crate) struct SpawnParams {
    pub template: Option<String>,
    pub command: Option<String>,
    pub args: Vec<String>,
    pub cwd: Option<String>,
    pub placement: Option<Placement>,
    pub project: Option<ProjectId>,
}

impl PluginHost {
    pub(crate) fn open_screen(
        &self,
        plugin: &PluginId,
        screen_id: &str,
        project: Option<&ProjectId>,
        _params: Value,
    ) -> Result<ScreenOpenResult, KeltaError> {
        let entry = self.active_entry(plugin.as_str())?;
        let m = entry.manifest().ok_or_else(|| KeltaError::internal("loaded plugin without manifest"))?;
        let screen =
            m.contributes.screens.iter().find(|s| s.id == screen_id).ok_or_else(|| {
                KeltaError::not_found(format!("plugin `{plugin}` has no screen `{screen_id}`"))
            })?;
        let instance_id = ScreenInstanceId::generate();
        self.screens.lock().insert(
            instance_id.clone(),
            Screen {
                plugin: plugin.clone(),
                screen_id: screen_id.to_owned(),
                project: project.cloned(),
                scope: screen.scope,
                subscriptions: Vec::new(),
            },
        );
        self.mark_activated(plugin);
        let entry_path = screen.entry.trim_start_matches("./");
        Ok(ScreenOpenResult {
            url: format!("kelta-plugin://{plugin}/{entry_path}?instance={instance_id}"),
            instance_id,
        })
    }

    /// Open a plugin screen pane from the backend (trigger/command action, `ui.open_screen`).
    pub(crate) async fn open_screen_pane(
        &self,
        plugin: &PluginId,
        screen_id: &str,
        project: &ProjectId,
        params: Value,
        placement: Placement,
    ) -> Result<ScreenOpenResult, KeltaError> {
        let opened = self.open_screen(plugin, screen_id, Some(project), params.clone())?;
        let title = self.active_entry(plugin.as_str()).ok().and_then(|e| {
            e.manifest().and_then(|m| {
                m.contributes.screens.iter().find(|s| s.id == screen_id).map(|s| s.title.clone())
            })
        });
        self.core_or_err()?
            .layout_open(
                project,
                OpenPaneRequest {
                    content: PaneContent::PluginScreen {
                        plugin_id: plugin.clone(),
                        screen_id: screen_id.to_owned(),
                        instance_id: opened.instance_id.clone(),
                        params,
                    },
                    placement,
                    focus: true,
                    tab_title: title,
                    work_item_id: None,
                },
            )
            .await?;
        Ok(opened)
    }

    pub(crate) async fn dispatch_call(
        &self,
        instance: &ScreenInstanceId,
        method: PluginMethod,
        params: Value,
        _caller: CallOrigin,
    ) -> Result<Value, KeltaError> {
        let screen = self
            .screens
            .lock()
            .get(instance)
            .cloned()
            .ok_or_else(|| KeltaError::not_found(format!("screen instance `{instance}` is not open")))?;
        let entry = self.active_entry(screen.plugin.as_str())?;
        let granted = self.granted(&entry).await?;
        perms::check_method(method, &params, &granted)?;
        self.call_method(instance, &screen, &granted, method, params).await
    }

    fn screen_project(&self, screen: &Screen) -> Result<ProjectId, KeltaError> {
        screen.project.clone().ok_or_else(|| KeltaError::invalid("this screen is not bound to a project"))
    }

    async fn call_method(
        &self,
        instance: &ScreenInstanceId,
        screen: &Screen,
        granted: &Granted,
        method: PluginMethod,
        params: Value,
    ) -> Result<Value, KeltaError> {
        let core = self.core_or_err()?;
        match method {
            PluginMethod::AppInfo => {
                let s = self.settings(screen.project.as_ref());
                Ok(
                    json!({ "version": kelta_proto::VERSION, "platform": kelta_proto::dirs::Os::current().as_str(), "theme": s.app.theme }),
                )
            }
            PluginMethod::ProjectsCurrent => {
                let id = self.screen_project(screen)?;
                to_value(core.project(&id).ok_or_else(|| KeltaError::not_found(format!("project `{id}`")))?)
            }
            PluginMethod::ProjectsList => {
                let ids: Vec<ProjectId> = match (screen.scope, &screen.project) {
                    (ScreenScope::Project, Some(p)) => vec![p.clone()],
                    _ => match self.wiring().settings {
                        Some(src) => src.projects().iter().map(|p| p.id.clone()).collect(),
                        None => screen.project.iter().cloned().collect(),
                    },
                };
                to_value(ids.iter().filter_map(|id| core.project(id)).collect::<Vec<_>>())
            }
            PluginMethod::TicketsList => {
                let q: ListTickets = p(method, params)?;
                let pid = self.screen_project(screen)?;
                let project =
                    core.project(&pid).ok_or_else(|| KeltaError::not_found(format!("project `{pid}`")))?;
                let binding =
                    project.tracker.ok_or_else(|| KeltaError::invalid("no tracker bound to this project"))?;
                if q.scope.as_deref() == Some("all") {
                    tracing::debug!("tickets.list scope=all served from the screen's project tracker");
                }
                let view = match &q.view_id {
                    Some(id) => binding
                        .views
                        .iter()
                        .find(|v| &v.id == id)
                        .cloned()
                        .ok_or_else(|| KeltaError::not_found(format!("tracker view `{id}`")))?,
                    None => binding
                        .views
                        .first()
                        .cloned()
                        .ok_or_else(|| KeltaError::invalid("the tracker binding has no views"))?,
                };
                let tracker = core.tracker_for(&binding.account).await?;
                let page = tracker.list(&view, q.cursor).await?;
                to_value(TicketPage {
                    items: page
                        .items
                        .into_iter()
                        .map(|ticket| TicketItem {
                            ticket,
                            project_ids: vec![pid.clone()],
                            work_item_id: None,
                            view_ids: vec![view.id.clone()],
                        })
                        .collect(),
                    next: page.next,
                    stale: false,
                    errors: Vec::new(),
                })
            }
            PluginMethod::TicketsGet => {
                let q: TicketParam = p(method, params)?;
                to_value(core.tracker_for(&q.ticket.account).await?.get(&q.ticket).await?)
            }
            PluginMethod::TicketsTransitions => {
                let q: TicketParam = p(method, params)?;
                to_value(core.tracker_for(&q.ticket.account).await?.transitions(&q.ticket).await?)
            }
            PluginMethod::TicketsColumns => {
                let pid = self.screen_project(screen)?;
                let binding = core
                    .project(&pid)
                    .and_then(|p| p.tracker)
                    .ok_or_else(|| KeltaError::invalid("no tracker bound to this project"))?;
                to_value(core.tracker_for(&binding.account).await?.columns(&binding).await?)
            }
            PluginMethod::TicketsTransition => {
                #[derive(Deserialize)]
                struct Q {
                    ticket: TicketRef,
                    transition_id: String,
                    #[serde(default)]
                    fields: Option<Value>,
                }
                let q: Q = p(method, params)?;
                to_value(
                    core.tracker_for(&q.ticket.account)
                        .await?
                        .transition(&q.ticket, &q.transition_id, q.fields)
                        .await?,
                )
            }
            PluginMethod::TicketsComment => {
                #[derive(Deserialize)]
                struct Q {
                    ticket: TicketRef,
                    markdown: String,
                }
                let q: Q = p(method, params)?;
                core.tracker_for(&q.ticket.account).await?.comment(&q.ticket, &q.markdown).await?;
                Ok(Value::Null)
            }
            PluginMethod::TicketsAssign => {
                #[derive(Deserialize)]
                struct Q {
                    ticket: TicketRef,
                    assignee: Assignee,
                }
                let q: Q = p(method, params)?;
                to_value(core.tracker_for(&q.ticket.account).await?.assign(&q.ticket, q.assignee).await?)
            }
            PluginMethod::ReviewsList => {
                let q: ListReviews = p(method, params)?;
                let scope = match q.scope {
                    Some(Value::String(s)) if s == "all" => Scope::All,
                    Some(v @ Value::Object(_)) => serde_json::from_value(v)?,
                    _ => match &screen.project {
                        Some(id) => Scope::Project { id: id.clone() },
                        None => Scope::All,
                    },
                };
                to_value(core.review_list(scope, q.kind.unwrap_or(ReviewKind::ReviewRequested)).await?)
            }
            PluginMethod::ReviewsGet => {
                let q: ReviewParam = p(method, params)?;
                to_value(core.code_host_for(&q.review.account).await?.get(&q.review).await?)
            }
            PluginMethod::ReviewsApprove => {
                #[derive(Deserialize)]
                struct Q {
                    review: ReviewRef,
                    head_sha: String,
                }
                let q: Q = p(method, params)?;
                core.code_host_for(&q.review.account).await?.approve(&q.review, &q.head_sha).await?;
                Ok(Value::Null)
            }
            PluginMethod::ReviewsComment | PluginMethod::ReviewsRequestChanges => {
                #[derive(Deserialize)]
                struct Q {
                    review: ReviewRef,
                    body: String,
                }
                let q: Q = p(method, params)?;
                let host = core.code_host_for(&q.review.account).await?;
                if method == PluginMethod::ReviewsComment {
                    host.comment(&q.review, &q.body).await?;
                } else {
                    host.request_changes(&q.review, &q.body).await?;
                }
                Ok(Value::Null)
            }
            PluginMethod::SessionsList => {
                #[derive(Deserialize, Default)]
                #[serde(default)]
                struct Q {
                    project: Option<ProjectId>,
                }
                let q: Q = p(method, params)?;
                let project = q.project.or_else(|| screen.project.clone());
                to_value(core.session_list(project.as_ref()))
            }
            PluginMethod::SessionsSpawn => {
                let q: SpawnParams = p(method, params)?;
                let project = q
                    .project
                    .clone()
                    .or_else(|| screen.project.clone())
                    .ok_or_else(|| KeltaError::invalid("sessions.spawn needs a project"))?;
                self.spawn_session(&project, q, None).await
            }
            PluginMethod::SessionsSendText => {
                #[derive(Deserialize)]
                struct Q {
                    session_id: SessionId,
                    text: String,
                    #[serde(default)]
                    bracketed: Option<bool>,
                }
                let q: Q = p(method, params)?;
                core.session_write(&q.session_id, &keys_bytes(&q.text, q.bracketed.unwrap_or(true))).await?;
                Ok(Value::Null)
            }
            PluginMethod::ToolsOpen => {
                #[derive(Deserialize)]
                struct Q {
                    tool_id: String,
                    #[serde(default)]
                    placement: Option<Placement>,
                }
                let q: Q = p(method, params)?;
                let project = self.screen_project(screen)?;
                let tool = self.qualify_tool(&screen.plugin, &project, &q.tool_id);
                to_value(
                    self.open_tool(&project, &tool, TemplateCtx::default(), q.placement.unwrap_or_default())
                        .await?,
                )
            }
            PluginMethod::EventsSubscribe => {
                #[derive(Deserialize)]
                struct Q {
                    names: Vec<String>,
                }
                let q: Q = p(method, params)?;
                let mut compiled = Vec::new();
                for n in &q.names {
                    compiled.push((n.clone(), matcher::glob(n)?));
                }
                if let Some(s) = self.screens.lock().get_mut(instance) {
                    for (n, g) in compiled {
                        if !s.subscriptions.iter().any(|(x, _)| x == &n) {
                            s.subscriptions.push((n, g));
                        }
                    }
                }
                Ok(json!({ "subscribed": q.names }))
            }
            PluginMethod::EventsUnsubscribe => {
                #[derive(Deserialize, Default)]
                #[serde(default)]
                struct Q {
                    names: Vec<String>,
                }
                let q: Q = p(method, params)?;
                if let Some(s) = self.screens.lock().get_mut(instance) {
                    if q.names.is_empty() {
                        s.subscriptions.clear();
                    } else {
                        s.subscriptions.retain(|(n, _)| !q.names.contains(n));
                    }
                }
                Ok(Value::Null)
            }
            PluginMethod::SettingsGet => {
                let settings = self.settings(screen.project.as_ref());
                let mut values = settings
                    .plugins
                    .settings
                    .get(screen.plugin.as_str())
                    .cloned()
                    .unwrap_or_else(|| json!({}));
                if let Some(schema) =
                    self.registry().get(screen.plugin.as_str()).and_then(|e| e.settings_schema.clone())
                {
                    apply_schema_defaults(&mut values, &schema);
                }
                if granted.has(&Permission::SettingsRead)
                    && let Value::Object(m) = &mut values
                {
                    m.insert("$effective".into(), non_secret_settings(&settings));
                }
                Ok(values)
            }
            PluginMethod::SettingsSet => {
                #[derive(Deserialize)]
                struct Q {
                    key: String,
                    value: Value,
                }
                let q: Q = p(method, params)?;
                if q.key.is_empty() || q.key.contains(['.', '/']) {
                    return Err(KeltaError::invalid(
                        "settings.set: `key` must be a plain key of the plugin namespace",
                    ));
                }
                if let Some(schema) =
                    self.registry().get(screen.plugin.as_str()).and_then(|e| e.settings_schema.clone())
                {
                    let prop = schema.get("properties").and_then(|p| p.get(&q.key)).ok_or_else(|| {
                        KeltaError::invalid(format!(
                            "settings.set: `{}` is not declared in the plugin settings schema",
                            q.key
                        ))
                    })?;
                    if let Ok(v) = jsonschema::validator_for(prop)
                        && let Err(e) = v.validate(&q.value)
                    {
                        return Err(KeltaError::invalid(format!("settings.set `{}`: {e}", q.key)));
                    }
                }
                let writer = self
                    .wiring()
                    .settings_writer
                    .ok_or_else(|| KeltaError::unsupported("settings.set is not available"))?;
                writer(&screen.plugin, &q.key, q.value)?;
                Ok(Value::Null)
            }
            PluginMethod::NetFetch => {
                #[derive(Deserialize)]
                struct Q {
                    url: String,
                    #[serde(default)]
                    method: Option<String>,
                    #[serde(default)]
                    headers: std::collections::BTreeMap<String, String>,
                    #[serde(default)]
                    body: Option<String>,
                }
                let q: Q = p(method, params)?;
                let resp = core
                    .http_fetch(ProxiedRequest {
                        url: q.url,
                        method: q.method.unwrap_or_else(|| "GET".into()).to_ascii_uppercase(),
                        headers: q.headers,
                        body: q.body,
                        body_base64: false,
                        timeout_ms: Some(30_000),
                    })
                    .await?;
                if resp.body.len() > NET_FETCH_CAP * 4 / 3 + 4 {
                    return Err(KeltaError::invalid("net.fetch: response larger than 5 MB"));
                }
                Ok(
                    json!({ "status": resp.status, "headers": resp.headers, "body": resp.body, "body_base64": resp.body_base64 }),
                )
            }
            // Namespaced by `screen.plugin` (host-side, never from params).
            PluginMethod::KvGet => {
                let key = p::<KvKey>(method, params)?.checked()?;
                let v = self.grant_store().kv_get(&screen.plugin, &key).await?;
                Ok(v.and_then(|v| serde_json::from_str(&v).ok()).unwrap_or(Value::Null))
            }
            PluginMethod::KvSet => {
                #[derive(Deserialize)]
                struct Q {
                    key: String,
                    value: Value,
                }
                let q: Q = p(method, params)?;
                let key = KvKey { key: q.key }.checked()?;
                let value = serde_json::to_string(&q.value)?;
                if value.len() > KV_VALUE_CAP {
                    return Err(KeltaError::invalid(format!(
                        "kv.set: value larger than {KV_VALUE_CAP} bytes"
                    )));
                }
                self.grant_store().kv_set(&screen.plugin, &key, value, KV_QUOTA).await?;
                Ok(Value::Null)
            }
            PluginMethod::KvDelete => {
                let key = p::<KvKey>(method, params)?.checked()?;
                self.grant_store().kv_delete(&screen.plugin, &key).await?;
                Ok(Value::Null)
            }
            PluginMethod::KvList => to_value(self.grant_store().kv_keys(&screen.plugin).await?),
            PluginMethod::UiToast => {
                #[derive(Deserialize)]
                struct Q {
                    text: String,
                    #[serde(default)]
                    level: Option<ToastLevel>,
                }
                let q: Q = p(method, params)?;
                core.toast(Toast {
                    level: q.level.unwrap_or_default(),
                    text: crate::util::truncate(&q.text, 500),
                    action: None,
                });
                Ok(Value::Null)
            }
            PluginMethod::UiOpenScreen => {
                #[derive(Deserialize)]
                struct Q {
                    #[serde(alias = "screen")]
                    screen_id: String,
                    #[serde(default)]
                    params: Value,
                    #[serde(default)]
                    placement: Option<Placement>,
                }
                let q: Q = p(method, params)?;
                let project = screen.project.clone().unwrap_or_else(ProjectId::home);
                to_value(
                    self.open_screen_pane(
                        &screen.plugin,
                        &q.screen_id,
                        &project,
                        q.params,
                        q.placement.unwrap_or_default(),
                    )
                    .await?,
                )
            }
            PluginMethod::UiFocus => {
                #[derive(Deserialize, Default)]
                #[serde(default)]
                struct Q {
                    project: Option<ProjectId>,
                    session: Option<SessionId>,
                }
                let q: Q = p(method, params)?;
                self.focus(q.project, q.session).await?;
                Ok(Value::Null)
            }
            PluginMethod::NotifySend => {
                #[derive(Deserialize)]
                struct Q {
                    title: String,
                    #[serde(default)]
                    body: Option<String>,
                }
                let q: Q = p(method, params)?;
                core.notify(Notification {
                    title: q.title,
                    body: q.body,
                    urgency: Urgency::Normal,
                    project_id: screen.project.clone(),
                    session_id: None,
                })
                .await?;
                Ok(Value::Null)
            }
            PluginMethod::ClipboardWrite => {
                #[derive(Deserialize)]
                struct Q {
                    text: String,
                }
                let q: Q = p(method, params)?;
                // Approved here; the screen host performs the write with `clipboard_write`.
                Ok(json!({ "approved": true, "text": q.text }))
            }
        }
    }

    /// `"k9s"` from plugin `tools-pack` → `tools-pack/k9s` when that exists.
    fn qualify_tool(&self, plugin: &PluginId, project: &ProjectId, id: &str) -> ToolId {
        if id.contains('/') {
            return ToolId::new(id);
        }
        let namespaced = format!("{plugin}/{id}");
        if self.resolve_tools(Some(project)).iter().any(|t| t.id.as_str() == namespaced) {
            ToolId::new(namespaced)
        } else {
            ToolId::new(id)
        }
    }

    /// Spawn a session (`sessions.spawn` / `spawn_session` action): template via ctl, or a command.
    pub(crate) async fn spawn_session(
        &self,
        project: &ProjectId,
        q: SpawnParams,
        work_item: Option<kelta_proto::ids::WorkItemId>,
    ) -> Result<Value, KeltaError> {
        let core = self.core_or_err()?;
        if let Some(t) = q.template {
            return core
                .ctl(kelta_proto::ctl::CtlCommand::New {
                    template: t,
                    cwd: q.cwd.map(Into::into),
                    project: Some(project.clone()),
                })
                .await;
        }
        let command =
            q.command.ok_or_else(|| KeltaError::invalid("sessions.spawn needs `template` or `command`"))?;
        let name = crate::util::basename(&command).to_owned();
        let session = core
            .session_spawn(SpawnRequest {
                id: None,
                project_id: project.clone(),
                kind: SessionKind::Custom,
                name: Some(name.clone()),
                program: Some(command),
                args: q.args,
                cwd: q.cwd.map(Into::into),
                env: Default::default(),
                cols: 80,
                rows: 24,
                work_item_id: work_item.clone(),
                restore: RestorePolicy::None,
                close_on_exit: Default::default(),
                template_id: None,
            })
            .await?;
        core.layout_open(
            project,
            OpenPaneRequest {
                content: PaneContent::Terminal { session_id: session.id.clone() },
                placement: q.placement.unwrap_or_default(),
                focus: true,
                tab_title: Some(name),
                work_item_id: work_item,
            },
        )
        .await?;
        to_value(session)
    }

    /// Focus a project (ctl) and/or a session (layout focus).
    pub(crate) async fn focus(
        &self,
        project: Option<ProjectId>,
        session: Option<SessionId>,
    ) -> Result<(), KeltaError> {
        let core = self.core_or_err()?;
        let session_project = session.as_ref().and_then(|s| core.session_get(s)).map(|s| s.project_id);
        if let Some(p) = project.clone().or_else(|| session_project.clone()) {
            core.ctl(kelta_proto::ctl::CtlCommand::FocusProject { id: p }).await?;
        }
        if let (Some(s), Some(p)) = (session, session_project.or(project)) {
            core.layout_open(
                &p,
                OpenPaneRequest {
                    content: PaneContent::Terminal { session_id: s },
                    placement: Placement::Focused,
                    focus: true,
                    tab_title: None,
                    work_item_id: None,
                },
            )
            .await?;
        }
        Ok(())
    }

    /// Relay a bus event to screens subscribed (and granted `events:<glob>`).
    pub(crate) async fn relay_to_screens(&self, ev: &BusEvent) {
        let targets: Vec<(ScreenInstanceId, PluginId)> = self
            .screens
            .lock()
            .iter()
            .filter(|(_, s)| s.subscriptions.iter().any(|(_, g)| g.is_match(&ev.name)))
            .filter(|(_, s)| match (&s.project, &ev.project_id) {
                (Some(a), Some(b)) => a == b || s.scope == ScreenScope::Global,
                _ => true,
            })
            .map(|(id, s)| (id.clone(), s.plugin.clone()))
            .collect();
        for (instance_id, plugin) in targets {
            let Ok(entry) = self.active_entry(plugin.as_str()) else { continue };
            let Ok(granted) = self.granted(&entry).await else { continue };
            if !granted.covers_events(&ev.name) {
                continue;
            }
            self.emit_ui(UiEvent::PluginEvent {
                instance_id,
                name: ev.name.clone(),
                payload: ev.payload.clone(),
            });
        }
    }

    /// Open screen instances: `(instance, plugin, screen id)`.
    pub fn screen_instances(&self) -> Vec<(ScreenInstanceId, PluginId, String)> {
        self.screens.lock().iter().map(|(k, s)| (k.clone(), s.plugin.clone(), s.screen_id.clone())).collect()
    }
}

/// Bytes for `send_keys` / `sessions.send_text` (bracketed paste by default).
pub(crate) fn keys_bytes(text: &str, bracketed: bool) -> Vec<u8> {
    if bracketed {
        let clean = text.replace("\x1b[201~", "");
        format!("\x1b[200~{clean}\x1b[201~").into_bytes()
    } else {
        text.as_bytes().to_vec()
    }
}

/// Fill missing top-level keys from `properties.*.default`.
pub(crate) fn apply_schema_defaults(values: &mut Value, schema: &Value) {
    if !values.is_object() {
        *values = json!({});
    }
    let (Some(props), Value::Object(m)) = (schema.get("properties").and_then(Value::as_object), values)
    else {
        return;
    };
    for (k, prop) in props {
        if !m.contains_key(k)
            && let Some(d) = prop.get("default")
        {
            m.insert(k.clone(), d.clone());
        }
    }
}

/// Effective settings without secrets (`settings.read`): accounts' secret refs, every `env` and
/// `headers` map (tools, triggers, terminal, project), and tool URLs marked `url_is_secret`.
fn non_secret_settings(s: &kelta_proto::settings::Settings) -> Value {
    fn scrub(v: &mut Value) {
        match v {
            Value::Object(o) => {
                for k in ["secret", "env", "headers"] {
                    o.remove(k);
                }
                o.values_mut().for_each(scrub);
            }
            Value::Array(a) => a.iter_mut().for_each(scrub),
            _ => {}
        }
    }
    let mut v = serde_json::to_value(s).unwrap_or(Value::Null);
    if let Some(tools) = v.get_mut("tools").and_then(Value::as_array_mut) {
        for (t, def) in tools.iter_mut().zip(&s.tools) {
            if def.url_is_secret.unwrap_or(def.start.is_some())
                && let Some(o) = t.as_object_mut()
            {
                o.remove("url");
            }
        }
    }
    scrub(&mut v);
    v
}

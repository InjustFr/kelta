//! `FakeCore`: records calls, configurable responses, in-memory sessions, real broadcast bus.

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use async_trait::async_trait;
use parking_lot::Mutex;
use serde::de::DeserializeOwned;
use serde_json::Value;
use tokio::sync::broadcast;

use crate::api::{CodeHost, CoreApi, Tracker};
use crate::codehost::{PrDraft, ReviewItem, ReviewKind};
use crate::ctl::CtlCommand;
use crate::error::KeltaError;
use crate::events::{BUS_CAPACITY, BusEvent, Notification, Toast};
use crate::ext::{ProxiedRequest, ProxiedResponse, ToolHandle};
use crate::ids::{AccountId, PaneId, ProjectId, SessionId, TabId, ToolId, WorkItemId};
use crate::model::{
    Attention, EditorTarget, Lifecycle, OpenPaneRequest, PaneRef, Placement, ProjectInfo, Scope, SessionInfo,
    SessionKind, SessionStatus, SpawnRequest, StatusChange, StatusSource, TemplateCtx, WorkItem,
};
use crate::settings::Settings;

/// A recorded `CoreApi` call: method name + JSON-ish argument summary.
#[derive(Debug, Clone, PartialEq)]
pub struct Call {
    pub method: &'static str,
    pub args: Value,
}

pub struct FakeCore {
    calls: Mutex<Vec<Call>>,
    responses: Mutex<HashMap<&'static str, Result<Value, KeltaError>>>,
    sessions: Mutex<BTreeMap<SessionId, SessionInfo>>,
    writes: Mutex<Vec<(SessionId, Vec<u8>)>>,
    hooks: Mutex<Vec<(SessionId, StatusChange)>>,
    opened: Mutex<Vec<(ProjectId, OpenPaneRequest)>>,
    projects: Mutex<BTreeMap<ProjectId, ProjectInfo>>,
    settings: Mutex<Arc<Settings>>,
    trackers: Mutex<HashMap<AccountId, Arc<dyn Tracker>>>,
    code_hosts: Mutex<HashMap<AccountId, Arc<dyn CodeHost>>>,
    work_items: Mutex<Vec<WorkItem>>,
    published: Mutex<Vec<BusEvent>>,
    toasts: Mutex<Vec<Toast>>,
    notifications: Mutex<Vec<Notification>>,
    ctl_commands: Mutex<Vec<CtlCommand>>,
    bus: broadcast::Sender<BusEvent>,
    next_pid: Mutex<u32>,
}

impl FakeCore {
    pub fn new() -> Arc<Self> {
        let (bus, _) = broadcast::channel(BUS_CAPACITY);
        Arc::new(Self {
            calls: Mutex::new(Vec::new()),
            responses: Mutex::new(HashMap::new()),
            sessions: Mutex::new(BTreeMap::new()),
            writes: Mutex::new(Vec::new()),
            hooks: Mutex::new(Vec::new()),
            opened: Mutex::new(Vec::new()),
            projects: Mutex::new(BTreeMap::new()),
            settings: Mutex::new(Arc::new(Settings::defaults())),
            trackers: Mutex::new(HashMap::new()),
            code_hosts: Mutex::new(HashMap::new()),
            work_items: Mutex::new(Vec::new()),
            published: Mutex::new(Vec::new()),
            toasts: Mutex::new(Vec::new()),
            notifications: Mutex::new(Vec::new()),
            ctl_commands: Mutex::new(Vec::new()),
            bus,
            next_pid: Mutex::new(1000),
        })
    }

    // ---- configuration ---------------------------------------------------------------------

    /// Override the result of `method` (its JSON is deserialized into the return type).
    pub fn respond(&self, method: &'static str, value: Value) {
        self.responses.lock().insert(method, Ok(value));
    }

    /// Make `method` fail.
    pub fn fail(&self, method: &'static str, e: KeltaError) {
        self.responses.lock().insert(method, Err(e));
    }

    pub fn clear_response(&self, method: &'static str) {
        self.responses.lock().remove(method);
    }

    pub fn add_project(&self, p: ProjectInfo) {
        self.projects.lock().insert(p.id.clone(), p);
    }

    pub fn set_settings(&self, s: Settings) {
        *self.settings.lock() = Arc::new(s);
    }

    pub fn add_tracker(&self, account: AccountId, t: Arc<dyn Tracker>) {
        self.trackers.lock().insert(account, t);
    }

    pub fn add_code_host(&self, account: AccountId, c: Arc<dyn CodeHost>) {
        self.code_hosts.lock().insert(account, c);
    }

    pub fn add_work_item(&self, w: WorkItem) {
        self.work_items.lock().push(w);
    }

    pub fn insert_session(&self, s: SessionInfo) {
        self.sessions.lock().insert(s.id.clone(), s);
    }

    /// Mark a session exited and publish `session.exited` (e.g. to unblock awaiting code).
    pub fn exit_session(&self, id: &SessionId, code: i32) {
        let project = {
            let mut s = self.sessions.lock();
            s.get_mut(id).map(|x| {
                x.lifecycle = Lifecycle::Exited;
                x.status = SessionStatus::Exited;
                x.exit_code = Some(code);
                x.project_id.clone()
            })
        };
        let mut ev = BusEvent::new(crate::events::bus::SESSION_EXITED, serde_json::json!({"code": code}))
            .with_session(id.clone());
        if let Some(p) = project {
            ev = ev.with_project(p);
        }
        self.publish_inner(ev);
    }

    // ---- inspection --------------------------------------------------------------------------

    pub fn calls(&self) -> Vec<Call> {
        self.calls.lock().clone()
    }

    pub fn call_names(&self) -> Vec<&'static str> {
        self.calls.lock().iter().map(|c| c.method).collect()
    }

    pub fn writes(&self) -> Vec<(SessionId, Vec<u8>)> {
        self.writes.lock().clone()
    }

    /// Concatenated bytes written to one session, as lossy UTF-8.
    pub fn written_text(&self, id: &SessionId) -> String {
        let bytes: Vec<u8> =
            self.writes.lock().iter().filter(|(s, _)| s == id).flat_map(|(_, b)| b.clone()).collect();
        String::from_utf8_lossy(&bytes).into_owned()
    }

    pub fn hooks(&self) -> Vec<(SessionId, StatusChange)> {
        self.hooks.lock().clone()
    }

    pub fn opened(&self) -> Vec<(ProjectId, OpenPaneRequest)> {
        self.opened.lock().clone()
    }

    pub fn published(&self) -> Vec<BusEvent> {
        self.published.lock().clone()
    }

    pub fn toasts(&self) -> Vec<Toast> {
        self.toasts.lock().clone()
    }

    pub fn notifications(&self) -> Vec<Notification> {
        self.notifications.lock().clone()
    }

    pub fn ctl_commands(&self) -> Vec<CtlCommand> {
        self.ctl_commands.lock().clone()
    }

    pub fn sessions(&self) -> Vec<SessionInfo> {
        self.sessions.lock().values().cloned().collect()
    }

    // ---- internals -----------------------------------------------------------------------------

    fn record(&self, method: &'static str, args: Value) {
        self.calls.lock().push(Call { method, args });
    }

    fn overridden<T: DeserializeOwned>(&self, method: &'static str) -> Option<Result<T, KeltaError>> {
        let r = self.responses.lock().get(method).cloned()?;
        Some(r.and_then(|v| serde_json::from_value(v).map_err(KeltaError::from)))
    }

    fn publish_inner(&self, ev: BusEvent) {
        self.published.lock().push(ev.clone());
        let _ = self.bus.send(ev);
    }

    fn arg<T: serde::Serialize>(t: &T) -> Value {
        serde_json::to_value(t).unwrap_or(Value::Null)
    }
}

#[async_trait]
impl CoreApi for FakeCore {
    async fn session_spawn(&self, req: SpawnRequest) -> Result<SessionInfo, KeltaError> {
        self.record("session_spawn", Self::arg(&req));
        if let Some(r) = self.overridden::<SessionInfo>("session_spawn") {
            let s = r?;
            self.insert_session(s.clone());
            return Ok(s);
        }
        let id = SessionId::generate();
        let pid = {
            let mut p = self.next_pid.lock();
            *p += 1;
            *p
        };
        let claude = matches!(req.kind, SessionKind::Claude).then(|| crate::model::ClaudeMeta {
            session_uuid: uuid::Uuid::new_v4().to_string(),
            ..Default::default()
        });
        let info = SessionInfo {
            id: id.clone(),
            project_id: req.project_id.clone(),
            name: req.name.clone().unwrap_or_else(|| req.kind.name().to_owned()),
            title: None,
            cwd: req.cwd.clone().unwrap_or_else(|| PathBuf::from("/")),
            status: SessionStatus::Starting,
            status_source: StatusSource::None,
            attention: Attention::None,
            seen: true,
            lifecycle: Lifecycle::Live,
            pid: Some(pid),
            exit_code: None,
            work_item_id: req.work_item_id.clone(),
            claude,
            editor: None,
            cols: req.cols,
            rows: req.rows,
            created_at: crate::now_rfc3339(),
            kind: req.kind,
        };
        self.insert_session(info.clone());
        self.publish_inner(
            BusEvent::new(
                crate::events::bus::SESSION_SPAWNED,
                serde_json::json!({ "session": Self::arg(&info) }),
            )
            .with_project(info.project_id.clone())
            .with_session(id),
        );
        Ok(info)
    }

    async fn session_write(&self, id: &SessionId, bytes: &[u8]) -> Result<(), KeltaError> {
        self.record("session_write", serde_json::json!({ "id": id, "len": bytes.len() }));
        if let Some(r) = self.overridden::<()>("session_write") {
            r?;
        }
        if !self.sessions.lock().contains_key(id) {
            return Err(KeltaError::not_found(format!("session {id}")));
        }
        self.writes.lock().push((id.clone(), bytes.to_vec()));
        Ok(())
    }

    async fn session_kill(&self, id: &SessionId, force: bool) -> Result<(), KeltaError> {
        self.record("session_kill", serde_json::json!({ "id": id, "force": force }));
        if let Some(r) = self.overridden::<()>("session_kill") {
            return r;
        }
        if !self.sessions.lock().contains_key(id) {
            return Err(KeltaError::not_found(format!("session {id}")));
        }
        self.exit_session(id, -1);
        Ok(())
    }

    fn session_get(&self, id: &SessionId) -> Option<SessionInfo> {
        self.record("session_get", serde_json::json!({ "id": id }));
        self.sessions.lock().get(id).cloned()
    }

    fn session_list(&self, project: Option<&ProjectId>) -> Vec<SessionInfo> {
        self.record("session_list", serde_json::json!({ "project": project }));
        self.sessions
            .lock()
            .values()
            .filter(|s| project.is_none_or(|p| &s.project_id == p))
            .cloned()
            .collect()
    }

    async fn session_apply_hook(&self, id: &SessionId, change: StatusChange) -> Result<(), KeltaError> {
        self.record("session_apply_hook", serde_json::json!({ "id": id, "change": Self::arg(&change) }));
        if let Some(r) = self.overridden::<()>("session_apply_hook") {
            r?;
        }
        if let Some(s) = self.sessions.lock().get_mut(id)
            && change.status != SessionStatus::Unknown
        {
            s.status = change.status;
            s.status_source = StatusSource::Hook;
        }
        self.hooks.lock().push((id.clone(), change));
        Ok(())
    }

    async fn layout_open(&self, project: &ProjectId, req: OpenPaneRequest) -> Result<PaneRef, KeltaError> {
        self.record("layout_open", serde_json::json!({ "project": project, "req": Self::arg(&req) }));
        if let Some(r) = self.overridden::<PaneRef>("layout_open") {
            return r;
        }
        let n = {
            let mut o = self.opened.lock();
            o.push((project.clone(), req));
            o.len()
        };
        Ok(PaneRef {
            project_id: project.clone(),
            tab_id: TabId::new(format!("tab-{n}")),
            pane_id: PaneId::new(format!("pane-{n}")),
        })
    }

    fn project(&self, id: &ProjectId) -> Option<ProjectInfo> {
        self.record("project", serde_json::json!({ "id": id }));
        self.projects.lock().get(id).cloned()
    }

    fn settings(&self, project: Option<&ProjectId>) -> Arc<Settings> {
        self.record("settings", serde_json::json!({ "project": project }));
        self.settings.lock().clone()
    }

    async fn tracker_for(&self, account: &AccountId) -> Result<Arc<dyn Tracker>, KeltaError> {
        self.record("tracker_for", serde_json::json!({ "account": account }));
        self.trackers
            .lock()
            .get(account)
            .cloned()
            .ok_or_else(|| KeltaError::not_found(format!("tracker account {account}")))
    }

    async fn code_host_for(&self, account: &AccountId) -> Result<Arc<dyn CodeHost>, KeltaError> {
        self.record("code_host_for", serde_json::json!({ "account": account }));
        self.code_hosts
            .lock()
            .get(account)
            .cloned()
            .ok_or_else(|| KeltaError::not_found(format!("code host account {account}")))
    }

    async fn review_list(&self, scope: Scope, kind: ReviewKind) -> Result<Vec<ReviewItem>, KeltaError> {
        self.record("review_list", serde_json::json!({ "scope": Self::arg(&scope), "kind": kind }));
        if let Some(r) = self.overridden("review_list") {
            return r;
        }
        let hosts: Vec<Arc<dyn CodeHost>> = self.code_hosts.lock().values().cloned().collect();
        let q = crate::codehost::ReviewQuery { kind, include_team: true, include_drafts: true };
        let mut out = Vec::new();
        for h in hosts {
            for review in h.list_reviews(&q).await? {
                out.push(ReviewItem { review, project_ids: vec![] });
            }
        }
        Ok(out)
    }

    async fn work_for_session(&self, id: &SessionId) -> Option<WorkItem> {
        self.record("work_for_session", serde_json::json!({ "id": id }));
        self.work_items.lock().iter().find(|w| w.session_ids.contains(id)).cloned()
    }

    async fn work_create_pr(&self, id: &WorkItemId, draft: PrDraft) -> Result<WorkItem, KeltaError> {
        self.record("work_create_pr", serde_json::json!({ "id": id, "draft": Self::arg(&draft) }));
        if let Some(r) = self.overridden("work_create_pr") {
            return r;
        }
        self.work_items
            .lock()
            .iter()
            .find(|w| &w.id == id)
            .cloned()
            .ok_or_else(|| KeltaError::not_found(format!("work item {id}")))
    }

    async fn editor_open(
        &self,
        target: EditorTarget,
        path: &Path,
        line: Option<u32>,
    ) -> Result<(), KeltaError> {
        self.record(
            "editor_open",
            serde_json::json!({ "target": Self::arg(&target), "path": path, "line": line }),
        );
        self.overridden::<()>("editor_open").unwrap_or(Ok(()))
    }

    async fn tool_open(
        &self,
        project: &ProjectId,
        tool: &ToolId,
        ctx: TemplateCtx,
        placement: Placement,
    ) -> Result<ToolHandle, KeltaError> {
        self.record(
            "tool_open",
            serde_json::json!({ "project": project, "tool": tool, "ctx": Self::arg(&ctx), "placement": placement }),
        );
        if let Some(r) = self.overridden("tool_open") {
            return r;
        }
        Err(KeltaError::not_found(format!("tool {tool}")))
    }

    fn publish(&self, ev: BusEvent) {
        self.record("publish", serde_json::json!({ "name": ev.name }));
        self.publish_inner(ev);
    }

    fn subscribe(&self) -> broadcast::Receiver<BusEvent> {
        self.bus.subscribe()
    }

    async fn notify(&self, n: Notification) -> Result<(), KeltaError> {
        self.record("notify", Self::arg(&n));
        self.notifications.lock().push(n);
        Ok(())
    }

    fn toast(&self, t: Toast) {
        self.record("toast", Self::arg(&t));
        self.toasts.lock().push(t);
    }

    async fn http_fetch(&self, req: ProxiedRequest) -> Result<ProxiedResponse, KeltaError> {
        self.record("http_fetch", serde_json::json!({ "url": req.url, "method": req.method }));
        self.overridden("http_fetch").unwrap_or_else(|| Err(KeltaError::network("FakeCore: no network")))
    }

    async fn ctl(&self, cmd: CtlCommand) -> Result<Value, KeltaError> {
        self.record("ctl", Self::arg(&cmd));
        self.ctl_commands.lock().push(cmd);
        self.overridden("ctl").unwrap_or(Ok(Value::Null))
    }
}

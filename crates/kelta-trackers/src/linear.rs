//! Linear (GraphQL, `https://api.linear.app/graphql`).
//!
//! A personal API key goes in `Authorization` as is (no `Bearer`); `auth = "bearer"` is for OAuth
//! tokens. Tickets are keyed by their identifier (`ENG-123`); mutations use the issue uuid
//! (`TicketRef.id`). Workflow states are the board columns; transition ids are state ids read
//! from the issue's team at runtime, never hard-coded.

use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use kelta_http::graphql::graphql;
use kelta_http::{AuthScheme, Authed, HttpCtx, markdown};
use kelta_proto::api::{SecretResolver, Tracker};
use kelta_proto::error::KeltaError;
use kelta_proto::ids::AccountId;
use kelta_proto::settings::{AccountConfig, AuthKind, TrackerBinding, TrackerView};
use kelta_proto::tracker::{
    Assignee, BodyFormat, Column, Cursor, Page, SourceHit, Sprint, Status, StatusCategory, Ticket,
    TicketDetail, TicketRef, TrackerCaps, TrackerKind, Transition, User, Who,
};
use parking_lot::Mutex;
use serde_json::{Value, json};

use crate::common::{self, COMMENT_LIMIT, category_from_name, columns_from_binding, s};

const PAGE_SIZE: u32 = 50;
/// Pages a caller can follow from one list; a safety net against runaway pagination.
pub const MAX_PAGES: u32 = 20;

const USER: &str = "id name displayName avatarUrl";
const ISSUE: &str = "id identifier title url updatedAt priorityLabel priority estimate dueDate \
    startedAt cycle { id name number isActive endsAt } state { id name type } \
    assignee { id name displayName avatarUrl } labels { nodes { name } } team { key }";

pub struct LinearTracker {
    gql: String,
    auth: Authed,
    me: Mutex<Option<User>>,
    /// identifier → browser URL (the workspace slug is only known from API answers).
    urls: Mutex<HashMap<String, String>>,
}

fn category(state_type: &str, name: &str) -> StatusCategory {
    match state_type {
        "triage" | "backlog" | "unstarted" => StatusCategory::Todo,
        "completed" | "canceled" => StatusCategory::Done,
        "started" if category_from_name(name) == StatusCategory::InReview => StatusCategory::InReview,
        "started" => StatusCategory::InProgress,
        _ => category_from_name(name),
    }
}

fn status_from(state: &Value) -> Status {
    let name = s(state, "name").unwrap_or("");
    Status {
        id: s(state, "id").unwrap_or("").to_owned(),
        name: name.to_owned(),
        category: category(s(state, "type").unwrap_or(""), name),
    }
}

fn user_from(v: &Value) -> Option<User> {
    let name = s(v, "displayName").or_else(|| s(v, "name")).unwrap_or("").to_owned();
    Some(common::user_from(s(v, "id")?, name, s(v, "displayName").map(str::to_owned), {
        s(v, "avatarUrl").map(str::to_owned)
    }))
}

/// Linear's `IssueFilter` for a view. `who` wins over the legacy `scope` (`all` drops the assignee filter).
fn filter_of(view: &TrackerView) -> Value {
    let mut f = json!({});
    match view.who {
        Some(Who::Mine) => f["assignee"] = json!({"isMe": {"eq": true}}),
        Some(Who::Unassigned) => f["assignee"] = json!({"null": true}),
        Some(Who::Anyone) => {}
        None if view.scope.as_deref() == Some("all") => {}
        None => f["assignee"] = json!({"isMe": {"eq": true}}),
    }
    if view.current_iteration {
        f["cycle"] = json!({"isActive": {"eq": true}});
    }
    match view.status.as_deref() {
        Some("*") => {}
        Some("closed") => f["state"] = json!({"type": {"in": ["completed", "canceled"]}}),
        _ => f["state"] = json!({"type": {"nin": ["completed", "canceled"]}}),
    }
    if let Some(t) = view.team.as_deref().filter(|t| !t.is_empty()) {
        f["team"] = json!({"key": {"eq": t}});
    }
    if let Some(p) = view.project.as_deref().filter(|p| !p.is_empty()) {
        f["project"] = json!({"name": {"eq": p}});
    }
    if let Some(l) = view.labels.as_ref().filter(|l| !l.is_empty()) {
        f["labels"] = json!({"name": {"in": l}});
    }
    f
}

impl LinearTracker {
    pub fn new(
        account: &AccountConfig,
        http: HttpCtx,
        secrets: Arc<dyn SecretResolver>,
    ) -> Result<Self, KeltaError> {
        let base = common::base_url(account)?;
        let scheme = match account.auth {
            Some(AuthKind::Bearer) => AuthScheme::Bearer,
            _ => AuthScheme::Header("Authorization".into()),
        };
        let auth = Authed::new(http, secrets, account.effective_secret(), Some(&base), scheme);
        Ok(Self {
            gql: format!("{base}/graphql"),
            auth,
            me: Mutex::new(None),
            urls: Mutex::new(HashMap::new()),
        })
    }

    fn account(&self) -> &AccountId {
        self.auth.http().account_id()
    }

    async fn gql(&self, query: &str, vars: Value) -> Result<Value, KeltaError> {
        graphql(&self.auth, &self.gql, query, vars).await
    }

    fn ticket_from(&self, v: &Value) -> Option<Ticket> {
        let key = s(v, "identifier")?.to_owned();
        let url = s(v, "url").unwrap_or("").to_owned();
        self.urls.lock().insert(key.clone(), url.clone());
        let priority = s(v, "priorityLabel").filter(|p| *p != "No priority").map(str::to_owned);
        let status = status_from(v.get("state")?);
        let updated_at = s(v, "updatedAt").unwrap_or("").to_owned();
        let started = matches!(status.category, StatusCategory::InProgress | StatusCategory::InReview)
            .then(|| s(v, "startedAt"))
            .flatten();
        Some(Ticket {
            r#ref: TicketRef { account: self.account().clone(), id: s(v, "id")?.to_owned(), key },
            title: s(v, "title").unwrap_or("").to_owned(),
            url,
            status,
            kind: None,
            assignee: v.get("assignee").and_then(user_from),
            labels: v
                .pointer("/labels/nodes")
                .and_then(Value::as_array)
                .map(|a| a.iter().filter_map(|l| s(l, "name").map(str::to_owned)).collect())
                .unwrap_or_default(),
            priority,
            // 0 = none, 1 urgent .. 4 low.
            priority_rank: v["priority"].as_u64().filter(|p| (1..=4).contains(p)).map(|p| (p - 1) as u8),
            status_since: Some(started.map_or(updated_at.clone(), str::to_owned)),
            updated_at,
            project_hint: v.pointer("/team/key").and_then(Value::as_str).map(str::to_owned),
            sprint: v.get("cycle").filter(|c| !c.is_null()).and_then(|c| {
                Some(Sprint {
                    id: s(c, "id")?.to_owned(),
                    name: s(c, "name").map_or_else(|| format!("Cycle {}", c["number"]), str::to_owned),
                    active: c["isActive"].as_bool() == Some(true),
                    ends_at: s(c, "endsAt").map(str::to_owned),
                })
            }),
            estimate: v["estimate"].as_f64().map(|e| e.to_string()),
            due: s(v, "dueDate").map(str::to_owned),
        })
    }

    /// uuid when known, else the identifier (Linear accepts both).
    fn id_of(t: &TicketRef) -> &str {
        if t.id.is_empty() { &t.key } else { &t.id }
    }

    async fn update(&self, t: &TicketRef, input: Value) -> Result<Ticket, KeltaError> {
        let q = format!(
            "mutation($id: String!, $input: IssueUpdateInput!) {{ issueUpdate(id: $id, input: $input) \
             {{ success issue {{ {ISSUE} }} }} }}"
        );
        let d = self.gql(&q, json!({"id": Self::id_of(t), "input": input})).await?;
        let r = &d["issueUpdate"];
        if r["success"].as_bool() != Some(true) {
            return Err(KeltaError::upstream("linear issueUpdate was not applied"));
        }
        self.ticket_from(&r["issue"]).ok_or_else(|| KeltaError::upstream("linear issueUpdate without issue"))
    }
}

fn states_of(nodes: &Value) -> Vec<Value> {
    let mut v = nodes.as_array().cloned().unwrap_or_default();
    v.sort_by(|a, b| {
        a["position"].as_f64().partial_cmp(&b["position"].as_f64()).unwrap_or(std::cmp::Ordering::Equal)
    });
    v
}

#[async_trait]
impl Tracker for LinearTracker {
    fn kind(&self) -> TrackerKind {
        TrackerKind::Linear
    }

    fn caps(&self) -> TrackerCaps {
        TrackerCaps {
            board_columns: true,
            assign: true,
            comment: true,
            transitions_need_fetch: true,
            projects_v2: false,
        }
    }

    async fn me(&self) -> Result<User, KeltaError> {
        if let Some(u) = self.me.lock().clone() {
            return Ok(u);
        }
        let d = self.gql(&format!("query {{ viewer {{ {USER} }} }}"), json!({})).await?;
        let u =
            user_from(&d["viewer"]).ok_or_else(|| KeltaError::upstream("linear viewer returned no user"))?;
        *self.me.lock() = Some(u.clone());
        Ok(u)
    }

    async fn list(&self, view: &TrackerView, cursor: Option<Cursor>) -> Result<Page<Ticket>, KeltaError> {
        // `After("<pages fetched>:<endCursor>")` keeps the page cap across calls.
        let (pages, after) = match cursor {
            None => (0, None),
            Some(Cursor::After(c)) => {
                let (n, after) = c.split_once(':').ok_or_else(|| KeltaError::invalid("bad linear cursor"))?;
                (
                    n.parse::<u32>().map_err(|_| KeltaError::invalid("bad linear cursor"))?,
                    Some(after.to_owned()),
                )
            }
            Some(_) => return Err(KeltaError::invalid("linear expects an after cursor")),
        };
        let q = format!(
            "query($filter: IssueFilter, $first: Int, $after: String) {{ issues(filter: $filter, first: $first, \
             after: $after, orderBy: updatedAt) {{ nodes {{ {ISSUE} }} pageInfo {{ hasNextPage endCursor }} }} }}"
        );
        let d = self.gql(&q, json!({"filter": filter_of(view), "first": PAGE_SIZE, "after": after})).await?;
        let issues = &d["issues"];
        let items = issues["nodes"]
            .as_array()
            .map(|a| a.iter().filter_map(|i| self.ticket_from(i)).collect())
            .unwrap_or_default();
        let next = issues["pageInfo"]["endCursor"]
            .as_str()
            .filter(|_| issues["pageInfo"]["hasNextPage"].as_bool() == Some(true) && pages + 1 < MAX_PAGES)
            .map(|c| Cursor::After(format!("{}:{c}", pages + 1)));
        Ok(Page { items, next })
    }

    async fn create(&self, project: &TrackerView, title: &str, body_md: &str) -> Result<Ticket, KeltaError> {
        let team = common::create_in(project, "team", project.team.as_deref())?;
        let d = self
            .gql("query($k: String!) { teams(filter: {key: {eq: $k}}) { nodes { id } } }", json!({"k": team}))
            .await?;
        let team_id = d
            .pointer("/teams/nodes/0/id")
            .and_then(Value::as_str)
            .ok_or_else(|| KeltaError::not_found(format!("linear team {team}")))?;
        let q = format!(
            "mutation($input: IssueCreateInput!) {{ issueCreate(input: $input) {{ success issue {{ {ISSUE} }} }} }}"
        );
        let input = json!({"teamId": team_id, "title": title, "description": body_md});
        let d = self.gql(&q, json!({ "input": input })).await?;
        let r = &d["issueCreate"];
        if r["success"].as_bool() != Some(true) {
            return Err(KeltaError::upstream("linear issueCreate was not applied"));
        }
        self.ticket_from(&r["issue"]).ok_or_else(|| KeltaError::upstream("linear issueCreate without issue"))
    }

    async fn sources(&self, query: &str) -> Result<Vec<SourceHit>, KeltaError> {
        let (teams, projects) = if query.is_empty() {
            (json!({}), json!({}))
        } else {
            let m = |f: &str| json!({f: {"containsIgnoreCase": query}});
            (json!({"or": [m("name"), m("key")]}), m("name"))
        };
        // shortcut: first page only, type-ahead reaches the rest; paginate if users hit the cap
        let d = self
            .gql(
                "query($teams: TeamFilter, $projects: ProjectFilter) { \
                 teams(filter: $teams, first: 50) { nodes { key name cyclesEnabled } } \
                 projects(filter: $projects, first: 50) { nodes { id name } } }",
                json!({"teams": teams, "projects": projects}),
            )
            .await?;
        let nodes = |k: &str| d[k]["nodes"].as_array().cloned().unwrap_or_default();
        let hit =
            |kind: &str, id: String, label: String, detail: Option<String>, view: TrackerView| SourceHit {
                kind: kind.into(),
                label: label.clone(),
                detail,
                view: TrackerView { id, label, who: Some(Who::Mine), ..view },
            };
        let mut out = Vec::new();
        for t in nodes("teams") {
            let Some(key) = s(&t, "key") else { continue };
            let name = s(&t, "name").unwrap_or(key);
            let team = TrackerView { team: Some(key.to_owned()), ..TrackerView::default() };
            out.push(hit("team", format!("team-{key}"), name.to_owned(), Some(key.to_owned()), team.clone()));
            if t["cyclesEnabled"].as_bool() == Some(true) {
                let view = TrackerView { current_iteration: true, ..team };
                out.push(hit(
                    "cycle",
                    format!("team-{key}-cycle"),
                    format!("{name} current cycle"),
                    Some(key.to_owned()),
                    view,
                ));
            }
        }
        for p in nodes("projects") {
            let (Some(id), Some(name)) = (s(&p, "id"), s(&p, "name")) else { continue };
            let view = TrackerView { project: Some(name.to_owned()), ..TrackerView::default() };
            out.push(hit("project", format!("project-{id}"), name.to_owned(), None, view));
        }
        Ok(out)
    }

    async fn get(&self, t: &TicketRef) -> Result<TicketDetail, KeltaError> {
        let q = format!(
            "query($id: String!, $n: Int) {{ issue(id: $id) {{ {ISSUE} description \
             comments(last: $n) {{ nodes {{ body createdAt user {{ {USER} }} }} }} \
             children(first: 50) {{ nodes {{ {ISSUE} }} }} }} }}"
        );
        let d = self.gql(&q, json!({"id": Self::id_of(t), "n": COMMENT_LIMIT})).await?;
        let raw = &d["issue"];
        let ticket = self.ticket_from(raw).ok_or_else(|| KeltaError::not_found("linear issue not found"))?;
        let comments = raw
            .pointer("/comments/nodes")
            .and_then(Value::as_array)
            .map(|a| {
                a.iter()
                    .map(|c| {
                        common::comment(
                            c.get("user").and_then(user_from).unwrap_or_default(),
                            s(c, "createdAt").unwrap_or("").to_owned(),
                            markdown::to_html(s(c, "body").unwrap_or("")),
                        )
                    })
                    .collect()
            })
            .unwrap_or_default();
        let children = raw
            .pointer("/children/nodes")
            .and_then(Value::as_array)
            .map(|a| a.iter().filter_map(|c| self.ticket_from(c)).map(common::child).collect())
            .unwrap_or_default();
        let body = s(raw, "description").unwrap_or("").to_owned();
        Ok(TicketDetail {
            ticket,
            body_html: markdown::to_html(&body),
            body_md: body,
            body_format: BodyFormat::Markdown,
            comments,
            parent: None,
            children,
            prs: Vec::new(),
            caps: Default::default(),
        })
    }

    async fn columns(&self, b: &TrackerBinding) -> Result<Vec<Column>, KeltaError> {
        if let Some(c) = columns_from_binding(b) {
            return Ok(c);
        }
        let team = b.views.iter().find_map(|v| v.team.clone().filter(|t| !t.is_empty()));
        let filter = team.map_or(json!({}), |k| json!({"team": {"key": {"eq": k}}}));
        let d = self
            .gql(
                "query($filter: WorkflowStateFilter) { workflowStates(filter: $filter, first: 250) \
                 { nodes { name type position } } }",
                json!({"filter": filter}),
            )
            .await?;
        let mut cols: Vec<Column> = Vec::new();
        // Several teams share state names: one column per name, first position wins.
        for st in states_of(&d["workflowStates"]["nodes"]) {
            let name = s(&st, "name").unwrap_or("").to_owned();
            if name.is_empty() || cols.iter().any(|c| c.id == name) {
                continue;
            }
            cols.push(Column {
                id: name.clone(),
                category: category(s(&st, "type").unwrap_or(""), &name),
                order: cols.len() as u32,
                match_names: vec![name.clone()],
                name,
            });
        }
        Ok(cols)
    }

    async fn transitions(&self, t: &TicketRef) -> Result<Vec<Transition>, KeltaError> {
        let d = self
            .gql(
                "query($id: String!) { issue(id: $id) { state { id } team { states(first: 100) \
                 { nodes { id name type position } } } } }",
                json!({"id": Self::id_of(t)}),
            )
            .await?;
        let current = d.pointer("/issue/state/id").and_then(Value::as_str).unwrap_or("");
        Ok(states_of(&d["issue"]["team"]["states"]["nodes"])
            .iter()
            .map(status_from)
            .filter(|st| st.id != current)
            .map(|st| Transition { id: st.id.clone(), name: st.name.clone(), to: st, needs_fields: false })
            .collect())
    }

    async fn transition(
        &self,
        t: &TicketRef,
        transition_id: &str,
        _fields: Option<Value>,
    ) -> Result<Ticket, KeltaError> {
        self.update(t, json!({"stateId": transition_id})).await
    }

    async fn comment(&self, t: &TicketRef, markdown: &str) -> Result<(), KeltaError> {
        let d = self
            .gql(
                "mutation($input: CommentCreateInput!) { commentCreate(input: $input) { success } }",
                json!({"input": {"issueId": Self::id_of(t), "body": markdown}}),
            )
            .await?;
        if d["commentCreate"]["success"].as_bool() != Some(true) {
            return Err(KeltaError::upstream("linear commentCreate was not applied"));
        }
        Ok(())
    }

    async fn assign(&self, t: &TicketRef, who: Assignee) -> Result<Ticket, KeltaError> {
        let id = match who {
            Assignee::Me => Some(self.me().await?.id),
            Assignee::User { id } => Some(id),
            Assignee::None => None,
        };
        self.update(t, json!({"assigneeId": id})).await
    }

    fn browser_url(&self, t: &TicketRef) -> String {
        // shortcut: before the ticket was seen in an API answer, linear.app redirects `/issue/<key>`.
        self.urls.lock().get(&t.key).cloned().unwrap_or_else(|| format!("https://linear.app/issue/{}", t.key))
    }

    fn branch_key(&self, t: &TicketRef) -> String {
        t.key.to_ascii_lowercase()
    }
}

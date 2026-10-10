//! GitLab Issues (REST v4): `PRIVATE-TOKEN`, URL-encoded project paths, scoped-label workflow
//! (`<scope>::<value>`, default scope `workflow`) moved with `add_labels` + explicit
//! `remove_labels` of the same-scope labels; Done is `state_event=close`.
//!
//! Transition ids are never hard-coded: they come from the project's labels
//! (`label:<scope>::<value>`) or are `close` / `reopen`.

use std::sync::Arc;

use async_trait::async_trait;
use kelta_http::util::percent_encode;
use kelta_http::{AuthScheme, Authed, HttpCtx, HttpRequest, markdown};
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

use crate::common::{
    self, COMMENT_LIMIT, category_from_name, columns_from_binding, idstr, s, split_repo_number,
};

const PER_PAGE: u32 = 50;

/// The current milestone among active ones: the earliest `due_date >= today`, else an undated one.
/// ISO dates compare as strings.
fn current_milestone(milestones: &Value, today: &str) -> Option<String> {
    let all = milestones.as_array()?;
    all.iter()
        .filter_map(|m| Some((s(m, "due_date").filter(|d| *d >= today)?, m)))
        .min_by_key(|(d, _)| *d)
        .map(|(_, m)| m)
        .or_else(|| all.iter().find(|m| s(m, "due_date").is_none()))
        .and_then(|m| s(m, "title"))
        .map(str::to_owned)
}
const DEFAULT_SCOPE: &str = "workflow";

fn today() -> String {
    let d = time::OffsetDateTime::now_utc().date();
    format!("{:04}-{:02}-{:02}", d.year(), u8::from(d.month()), d.day())
}

/// Priority rank from `priority::<0-3|critical|urgent|high|medium|low|p0-p3>` or a bare `P0`-`P3` label.
fn priority_rank(labels: &[String]) -> Option<u8> {
    const RANKS: [&[&str]; 4] = [
        &["p0", "0", "critical", "urgent"],
        &["p1", "1", "high"],
        &["p2", "2", "medium"],
        &["p3", "3", "low"],
    ];
    labels.iter().find_map(|l| {
        let l = l.to_ascii_lowercase();
        let scoped = l.strip_prefix("priority::");
        let v = scoped.unwrap_or(&l);
        if scoped.is_none() && !(v.len() == 2 && v.starts_with('p')) {
            return None;
        }
        RANKS.iter().position(|r| r.contains(&v)).map(|i| i as u8)
    })
}

/// `time_stats.time_estimate` seconds as `1h 30m`, else the issue weight. shortcut: no days, GitLab's `1d` is 8h.
fn estimate(v: &Value) -> Option<String> {
    let secs = v.pointer("/time_stats/time_estimate").and_then(Value::as_u64).filter(|s| *s > 0);
    match secs {
        Some(s) => Some(match (s / 3600, s % 3600 / 60) {
            (0, m) => format!("{m}m"),
            (h, 0) => format!("{h}h"),
            (h, m) => format!("{h}h {m}m"),
        }),
        None => v.get("weight").and_then(idstr),
    }
}

/// The issue's iteration, else its milestone. A milestone is active when open and today is inside its dates.
fn sprint(v: &Value, today: &str) -> Option<Sprint> {
    let (obj, is_iteration) = match (v.get("iteration"), v.get("milestone")) {
        (Some(i), _) if !i.is_null() => (i, true),
        (_, Some(m)) if !m.is_null() => (m, false),
        _ => return None,
    };
    let in_dates =
        s(obj, "start_date").is_none_or(|d| d <= today) && s(obj, "due_date").is_none_or(|d| d >= today);
    let active = match (is_iteration, obj.get("state")) {
        (true, Some(Value::Number(n))) => n.as_u64() == Some(2),
        // Undated milestones (release versions) are never "the current sprint".
        (false, Some(Value::String(st))) => st == "active" && s(obj, "due_date").is_some() && in_dates,
        _ => in_dates,
    };
    Some(Sprint {
        id: obj.get("id").and_then(idstr)?,
        // Cadence iterations have `title: null`: name them by their dates.
        name: s(obj, "title").map(str::to_owned).unwrap_or_else(|| {
            format!("{} – {}", s(obj, "start_date").unwrap_or("?"), s(obj, "due_date").unwrap_or("?"))
        }),
        active,
        ends_at: s(obj, "due_date").map(str::to_owned),
    })
}

pub struct GitlabIssues {
    api: String,
    web: String,
    auth: Authed,
    me: Mutex<Option<User>>,
    /// Scoped-label scopes that carry the workflow status (`workflow` + the ones views declare).
    scopes: Mutex<Vec<String>>,
}

impl GitlabIssues {
    pub fn new(
        account: &AccountConfig,
        http: HttpCtx,
        secrets: Arc<dyn SecretResolver>,
    ) -> Result<Self, KeltaError> {
        let base = common::base_url(account)?;
        let root = base.strip_suffix("/api/v4").unwrap_or(&base).to_owned();
        let scheme = match account.auth {
            Some(AuthKind::Bearer | AuthKind::Basic | AuthKind::Oauth) => {
                AuthScheme::from_account(account).unwrap_or(AuthScheme::Bearer)
            }
            _ => AuthScheme::Header("PRIVATE-TOKEN".into()),
        };
        let auth = Authed::new(http, secrets, account.effective_secret(), Some(&root), scheme)
            .with_header("Accept", "application/json");
        Ok(Self {
            web: account.web_url.as_deref().map(kelta_http::util::trim_url).unwrap_or_else(|| root.clone()),
            api: format!("{root}/api/v4"),
            auth,
            me: Mutex::new(None),
            scopes: Mutex::new(vec![DEFAULT_SCOPE.to_owned()]),
        })
    }

    fn account(&self) -> &AccountId {
        self.auth.http().account_id()
    }

    async fn json(&self, req: HttpRequest) -> Result<kelta_http::HttpResponse<Value>, KeltaError> {
        self.auth.send_json::<Value>(req).await
    }

    fn remember_view(&self, view: &TrackerView) {
        if let Some(sc) = view.workflow_scope.as_deref().filter(|s| !s.is_empty()) {
            let mut g = self.scopes.lock();
            if !g.iter().any(|x| x == sc) {
                g.push(sc.to_owned());
            }
        }
    }

    fn user_from(v: &Value) -> Option<User> {
        let id = v.get("id").and_then(idstr)?;
        let login = s(v, "username").map(str::to_owned);
        let name = s(v, "name").map(str::to_owned).or_else(|| login.clone()).unwrap_or_default();
        Some(common::user_from(id, name, login, s(v, "avatar_url").map(str::to_owned)))
    }

    /// Project path (or numeric id) and iid of a ticket key.
    fn split(t: &TicketRef) -> Result<(String, u64), KeltaError> {
        split_repo_number(&t.key)
    }

    fn issue_url(&self, project: &str, iid: u64, tail: &str) -> String {
        format!("{}/projects/{}/issues/{iid}{tail}", self.api, percent_encode(project))
    }

    /// Workflow status of an issue: its scoped label if any, else open/closed.
    fn status_from(&self, labels: &[String], state: &str) -> Status {
        let scopes = self.scopes.lock().clone();
        let scoped = labels.iter().find_map(|l| {
            let (scope, value) = l.split_once("::")?;
            scopes.iter().any(|s| s == scope).then_some((l.as_str(), value))
        });
        match (scoped, state) {
            (_, "closed") => Status {
                id: scoped.map(|(l, _)| l.to_owned()).unwrap_or_else(|| "closed".into()),
                name: scoped.map(|(_, v)| v.to_owned()).unwrap_or_else(|| "Closed".into()),
                category: StatusCategory::Done,
            },
            (Some((l, v)), _) => Status {
                id: l.to_owned(),
                name: v.to_owned(),
                category: match category_from_name(v) {
                    StatusCategory::Unknown => StatusCategory::InProgress,
                    c => c,
                },
            },
            (None, _) => Status { id: "opened".into(), name: "Open".into(), category: StatusCategory::Todo },
        }
    }

    fn ticket_from(&self, v: &Value) -> Option<Ticket> {
        let iid = v.get("iid").and_then(Value::as_u64)?;
        let project = v
            .pointer("/references/full")
            .and_then(Value::as_str)
            .and_then(|f| f.rsplit_once('#').map(|(p, _)| p.to_owned()))
            .filter(|p| !p.is_empty())
            .or_else(|| v.get("project_id").and_then(idstr))?;
        let labels: Vec<String> = v
            .get("labels")
            .and_then(Value::as_array)
            .map(|a| {
                a.iter().filter_map(|l| l.as_str().or_else(|| s(l, "name")).map(str::to_owned)).collect()
            })
            .unwrap_or_default();
        let assignee = v
            .get("assignee")
            .filter(|a| !a.is_null())
            .or_else(|| v.get("assignees").and_then(|a| a.get(0)))
            .and_then(Self::user_from);
        Some(Ticket {
            r#ref: TicketRef {
                account: self.account().clone(),
                key: format!("{project}#{iid}"),
                id: v.get("id").and_then(idstr).unwrap_or_else(|| iid.to_string()),
            },
            title: s(v, "title").unwrap_or("").to_owned(),
            url: s(v, "web_url")
                .map(str::to_owned)
                .unwrap_or_else(|| format!("{}/{project}/-/issues/{iid}", self.web)),
            status: self.status_from(&labels, s(v, "state").unwrap_or("opened")),
            kind: s(v, "issue_type").or_else(|| s(v, "type")).map(str::to_owned),
            assignee,
            priority: labels.iter().find_map(|l| l.strip_prefix("priority::").map(str::to_owned)),
            priority_rank: priority_rank(&labels),
            labels,
            updated_at: s(v, "updated_at").unwrap_or("").to_owned(),
            project_hint: Some(project),
            // shortcut: no status-change date without a resource_label_events request per issue; closed_at, else updated_at. Fetch events if age badges prove wrong.
            status_since: s(v, "closed_at").or_else(|| s(v, "updated_at")).map(str::to_owned),
            sprint: sprint(v, &today()),
            estimate: estimate(v),
            due: s(v, "due_date").map(str::to_owned),
        })
    }

    async fn fetch_raw(&self, project: &str, iid: u64) -> Result<Value, KeltaError> {
        Ok(self.json(HttpRequest::get(self.issue_url(project, iid, ""))).await?.body)
    }

    /// Labels `<scope>::*` of a project (and its ancestor groups), in server order.
    async fn scoped_labels(&self, project: &str, scope: &str) -> Result<Vec<String>, KeltaError> {
        let resp = self
            .json(
                HttpRequest::get(format!("{}/projects/{}/labels", self.api, percent_encode(project)))
                    .query("search", format!("{scope}::"))
                    .query("per_page", "100")
                    .with_etag(),
            )
            .await?;
        let prefix = format!("{scope}::");
        Ok(resp
            .body
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|l| s(l, "name"))
                    .filter(|n| n.starts_with(&prefix))
                    .map(str::to_owned)
                    .collect()
            })
            .unwrap_or_default())
    }

    /// Scope used for a ticket: the scope of its current scoped label, else the first known one.
    fn scope_for(&self, labels: &[String]) -> String {
        let scopes = self.scopes.lock().clone();
        labels
            .iter()
            .find_map(|l| l.split_once("::").map(|(s, _)| s).filter(|s| scopes.iter().any(|x| x == s)))
            .map(str::to_owned)
            .or_else(|| scopes.first().cloned())
            .unwrap_or_else(|| DEFAULT_SCOPE.to_owned())
    }

    fn labels_of(v: &Value) -> Vec<String> {
        v.get("labels")
            .and_then(Value::as_array)
            .map(|a| a.iter().filter_map(|l| l.as_str().map(str::to_owned)).collect())
            .unwrap_or_default()
    }

    async fn put(&self, project: &str, iid: u64, body: Value) -> Result<Ticket, KeltaError> {
        let v = self.json(HttpRequest::put(self.issue_url(project, iid, "")).json(body)).await?.body;
        self.ticket_from(&v).ok_or_else(|| KeltaError::upstream("gitlab issue response without iid"))
    }
}

fn state_param(view: &TrackerView) -> &'static str {
    match view.status.as_deref() {
        Some("closed") => "closed",
        Some("*") => "all",
        _ => "opened",
    }
}

#[async_trait]
impl Tracker for GitlabIssues {
    fn kind(&self) -> TrackerKind {
        TrackerKind::GitlabIssues
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
        let v = self.json(HttpRequest::get(format!("{}/user", self.api))).await?.body;
        let u = Self::user_from(&v).ok_or_else(|| KeltaError::upstream("gitlab /user returned no user"))?;
        *self.me.lock() = Some(u.clone());
        Ok(u)
    }

    async fn list(&self, view: &TrackerView, cursor: Option<Cursor>) -> Result<Page<Ticket>, KeltaError> {
        self.remember_view(view);
        let page = match cursor {
            None => 1,
            Some(Cursor::Page(p)) => p.max(1),
            Some(_) => return Err(KeltaError::invalid("gitlab issues expect a page cursor")),
        };
        let url = match view.project.as_deref().filter(|p| !p.is_empty()) {
            Some(p) => format!("{}/projects/{}/issues", self.api, percent_encode(p)),
            None => format!("{}/issues", self.api),
        };
        // `who` wins over the legacy `scope`.
        let (scope, unassigned) = match view.who {
            Some(Who::Mine) => ("assigned_to_me", false),
            Some(Who::Unassigned) => ("all", true),
            Some(Who::Anyone) => ("all", false),
            None if view.scope.as_deref() == Some("all") => ("all", false),
            None => ("assigned_to_me", false),
        };
        let mut req = HttpRequest::get(url)
            .query("scope", scope)
            .query("state", state_param(view))
            .query("order_by", "updated_at")
            .query("sort", "desc")
            .query("per_page", PER_PAGE.to_string())
            .query("page", page.to_string());
        if unassigned {
            req = req.query("assignee_id", "None");
        }
        if view.current_iteration {
            // Free tier has no iterations: a milestone is the timebox. REST's `milestone_id=Started` uses the
            // legacy rule (start date set and past, due date ignored), so a project picks its own milestone.
            match view.project.as_deref().filter(|p| !p.is_empty()) {
                Some(p) => {
                    let url = format!("{}/projects/{}/milestones", self.api, percent_encode(p));
                    let ms = self.json(HttpRequest::get(url).query("state", "active")).await?.body;
                    match current_milestone(&ms, &today()) {
                        Some(title) => req = req.query("milestone", title),
                        None => return Ok(Page { items: vec![], next: None }),
                    }
                }
                // shortcut: legacy `Started` semantics without a project, pick per project when views need it.
                None => req = req.query("milestone_id", "Started"),
            }
        }
        if let Some(l) = view.labels.as_ref().filter(|l| !l.is_empty()) {
            req = req.query("labels", l.join(","));
        }
        let resp = self.json(req.with_etag()).await?;
        let items: Vec<Ticket> = resp
            .body
            .as_array()
            .map(|a| a.iter().filter_map(|i| self.ticket_from(i)).collect())
            .unwrap_or_default();
        let has_next = resp.headers.get("x-next-page").is_some_and(|v| !v.trim().is_empty());
        Ok(Page { items, next: has_next.then_some(Cursor::Page(page + 1)) })
    }

    async fn get(&self, t: &TicketRef) -> Result<TicketDetail, KeltaError> {
        let (project, iid) = Self::split(t)?;
        let raw = self.fetch_raw(&project, iid).await?;
        let ticket = self
            .ticket_from(&raw)
            .ok_or_else(|| KeltaError::upstream("gitlab issue response without iid"))?;
        let notes = self
            .json(
                HttpRequest::get(self.issue_url(&project, iid, "/notes"))
                    .query("sort", "desc")
                    .query("order_by", "created_at")
                    .query("per_page", COMMENT_LIMIT.to_string()),
            )
            .await?
            .body;
        let mut comments: Vec<_> = notes
            .as_array()
            .map(|a| {
                a.iter()
                    .filter(|n| n.get("system").and_then(Value::as_bool) != Some(true))
                    .map(|n| {
                        common::comment(
                            n.get("author").and_then(Self::user_from).unwrap_or_default(),
                            s(n, "created_at").unwrap_or("").to_owned(),
                            markdown::to_html(s(n, "body").unwrap_or("")),
                        )
                    })
                    .collect()
            })
            .unwrap_or_default();
        comments.reverse(); // newest first on the wire, oldest first in the contract
        let body = s(&raw, "description").unwrap_or("").to_owned();
        Ok(TicketDetail {
            ticket,
            body_html: markdown::to_html(&body),
            body_md: body,
            body_format: BodyFormat::Markdown,
            comments,
            parent: None,
            prs: Vec::new(),
            caps: Default::default(),
        })
    }

    async fn columns(&self, b: &TrackerBinding) -> Result<Vec<Column>, KeltaError> {
        for v in &b.views {
            self.remember_view(v);
        }
        if let Some(c) = columns_from_binding(b) {
            return Ok(c);
        }
        let mut cols = Vec::new();
        if let Some(view) = b.views.iter().find(|v| v.project.is_some()) {
            let project = view.project.clone().unwrap_or_default();
            let scope = view.workflow_scope.clone().unwrap_or_else(|| DEFAULT_SCOPE.to_owned());
            for l in self.scoped_labels(&project, &scope).await? {
                let value = l.split_once("::").map(|(_, v)| v).unwrap_or(&l).to_owned();
                cols.push(Column {
                    id: l.clone(),
                    category: category_from_name(&value),
                    order: cols.len() as u32,
                    match_names: vec![value.clone()],
                    name: value,
                });
            }
        }
        if cols.is_empty() {
            cols.push(Column {
                id: "opened".into(),
                name: "Open".into(),
                category: StatusCategory::Todo,
                order: 0,
                match_names: vec!["Open".into()],
            });
        } else if !cols.iter().any(|c| c.category == StatusCategory::Todo) {
            cols.insert(
                0,
                Column {
                    id: "opened".into(),
                    name: "Open".into(),
                    category: StatusCategory::Todo,
                    order: 0,
                    match_names: vec!["Open".into()],
                },
            );
            for (i, c) in cols.iter_mut().enumerate() {
                c.order = i as u32;
            }
        }
        if !cols.iter().any(|c| c.category == StatusCategory::Done) {
            let order = cols.len() as u32;
            cols.push(Column {
                id: "closed".into(),
                name: "Closed".into(),
                category: StatusCategory::Done,
                order,
                match_names: vec!["Closed".into()],
            });
        }
        Ok(cols)
    }

    async fn transitions(&self, t: &TicketRef) -> Result<Vec<Transition>, KeltaError> {
        let (project, iid) = Self::split(t)?;
        let raw = self.fetch_raw(&project, iid).await?;
        let labels = Self::labels_of(&raw);
        let closed = s(&raw, "state") == Some("closed");
        let scope = self.scope_for(&labels);
        let current = self.status_from(&labels, s(&raw, "state").unwrap_or("opened"));
        let mut out = Vec::new();
        let mut has_done_label = false;
        for l in self.scoped_labels(&project, &scope).await? {
            let value = l.split_once("::").map(|(_, v)| v).unwrap_or(&l).to_owned();
            let category = category_from_name(&value);
            has_done_label |= category == StatusCategory::Done;
            if current.id == l {
                continue;
            }
            out.push(Transition {
                id: format!("label:{l}"),
                name: value.clone(),
                to: Status { id: l, name: value, category },
                needs_fields: false,
            });
        }
        let have_labels = !out.is_empty() || labels.iter().any(|l| l.starts_with(&format!("{scope}::")));
        if !closed && !has_done_label {
            out.push(Transition {
                id: "close".into(),
                name: "Close".into(),
                to: Status { id: "closed".into(), name: "Closed".into(), category: StatusCategory::Done },
                needs_fields: false,
            });
        }
        if closed && !have_labels {
            out.push(Transition {
                id: "reopen".into(),
                name: "Reopen".into(),
                to: Status { id: "opened".into(), name: "Open".into(), category: StatusCategory::Todo },
                needs_fields: false,
            });
        }
        Ok(out)
    }

    async fn transition(
        &self,
        t: &TicketRef,
        transition_id: &str,
        _fields: Option<Value>,
    ) -> Result<Ticket, KeltaError> {
        let (project, iid) = Self::split(t)?;
        match transition_id {
            "close" => self.put(&project, iid, json!({"state_event": "close"})).await,
            "reopen" => self.put(&project, iid, json!({"state_event": "reopen"})).await,
            other => {
                let target = other
                    .strip_prefix("label:")
                    .ok_or_else(|| KeltaError::invalid(format!("unknown gitlab transition id: {other}")))?;
                let (scope, value) = target
                    .split_once("::")
                    .ok_or_else(|| KeltaError::invalid(format!("not a scoped label: {target}")))?;
                let raw = self.fetch_raw(&project, iid).await?;
                let prefix = format!("{scope}::");
                let remove: Vec<String> = Self::labels_of(&raw)
                    .into_iter()
                    .filter(|l| l.starts_with(&prefix) && l != target)
                    .collect();
                let mut body = json!({"add_labels": target});
                if !remove.is_empty() {
                    body["remove_labels"] = json!(remove.join(","));
                }
                // Done-ish workflow labels close the issue; any other label reopens a closed one.
                let closed = s(&raw, "state") == Some("closed");
                if category_from_name(value) == StatusCategory::Done {
                    if !closed {
                        body["state_event"] = json!("close");
                    }
                } else if closed {
                    body["state_event"] = json!("reopen");
                }
                self.put(&project, iid, body).await
            }
        }
    }

    async fn comment(&self, t: &TicketRef, markdown: &str) -> Result<(), KeltaError> {
        let (project, iid) = Self::split(t)?;
        self.auth
            .send_text(
                HttpRequest::post(self.issue_url(&project, iid, "/notes")).json(json!({"body": markdown})),
            )
            .await?;
        Ok(())
    }

    async fn assign(&self, t: &TicketRef, who: Assignee) -> Result<Ticket, KeltaError> {
        let (project, iid) = Self::split(t)?;
        let ids: Vec<Value> = match who {
            Assignee::Me => vec![
                self.me()
                    .await?
                    .id
                    .parse::<u64>()
                    .map(Value::from)
                    .map_err(|_| KeltaError::upstream("bad gitlab user id"))?,
            ],
            Assignee::User { id } => vec![
                id.parse::<u64>()
                    .map(Value::from)
                    .map_err(|_| KeltaError::invalid("gitlab assignee id must be numeric"))?,
            ],
            Assignee::None => vec![],
        };
        self.put(&project, iid, json!({"assignee_ids": ids})).await
    }

    async fn sources(&self, query: &str) -> Result<Vec<SourceHit>, KeltaError> {
        let mut req = HttpRequest::get(format!("{}/projects", self.api))
            .query("membership", "true")
            .query("with_issues_enabled", "true")
            .query("archived", "false")
            .query("order_by", "last_activity_at")
            // shortcut: first page only, type-ahead reaches the rest; paginate if users hit the cap
            .query("per_page", "20");
        if !query.is_empty() {
            req = req.query("search", query).query("search_namespaces", "true");
        }
        let projects = self.json(req).await?.body;
        Ok(projects
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|p| {
                        let path = s(p, "path_with_namespace").filter(|p| !p.is_empty())?;
                        Some(SourceHit {
                            kind: "project".into(),
                            label: path.to_owned(),
                            detail: s(p, "description").filter(|d| !d.is_empty()).map(str::to_owned),
                            view: TrackerView {
                                id: format!("project-{path}"),
                                label: path.to_owned(),
                                project: Some(path.to_owned()),
                                who: Some(Who::Mine),
                                ..TrackerView::default()
                            },
                        })
                    })
                    .collect()
            })
            .unwrap_or_default())
    }

    fn browser_url(&self, t: &TicketRef) -> String {
        match split_repo_number(&t.key) {
            Ok((project, iid)) => format!("{}/{project}/-/issues/{iid}", self.web),
            Err(_) => self.web.clone(),
        }
    }

    fn branch_key(&self, t: &TicketRef) -> String {
        match split_repo_number(&t.key) {
            Ok((_, iid)) => format!("gl-{iid}"),
            Err(_) => t.key.clone(),
        }
    }
}

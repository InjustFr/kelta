//! Redmine (REST JSON): `X-Redmine-API-Key`, offset paging, moves restricted to
//! `include=allowed_statuses`, Textile shown preformatted unless the account says markdown.

use std::sync::Arc;

use async_trait::async_trait;
use kelta_http::{AuthScheme, Authed, HttpCtx, HttpRequest, markdown};
use kelta_proto::api::{SecretResolver, Tracker};
use kelta_proto::error::{ErrorCode, KeltaError};
use kelta_proto::ids::AccountId;
use kelta_proto::settings::{AccountConfig, TextFormat, TrackerBinding, TrackerView};
use kelta_proto::tracker::{
    Assignee, BodyFormat, Column, Cursor, Page, SourceHit, Status, StatusCategory, Ticket, TicketDetail,
    TicketRef, TrackerCaps, TrackerKind, Transition, User, Who,
};
use parking_lot::Mutex;
use serde_json::{Value, json};

use crate::common::{
    self, COMMENT_LIMIT, category_from_name, columns_from_binding, columns_from_statuses, idstr, s,
};

const PAGE_SIZE: u32 = 100;

pub struct RedmineTracker {
    base: String,
    web: String,
    auth: Authed,
    format: TextFormat,
    /// `/issue_statuses.json`: id → (name, is_closed); fetched once, used for categories.
    statuses: Mutex<Option<Vec<(String, String, bool)>>>,
    me: Mutex<Option<User>>,
}

impl RedmineTracker {
    pub fn new(
        account: &AccountConfig,
        http: HttpCtx,
        secrets: Arc<dyn SecretResolver>,
    ) -> Result<Self, KeltaError> {
        let base = common::base_url(account)?;
        let web = account.web_url.as_deref().map(kelta_http::util::trim_url).unwrap_or_else(|| base.clone());
        let scheme = AuthScheme::from_account(account)
            .unwrap_or_else(|| AuthScheme::Header("X-Redmine-API-Key".into()));
        let auth = Authed::new(http, secrets, account.effective_secret(), Some(&base), scheme)
            .with_header("Accept", "application/json");
        Ok(Self {
            base,
            web,
            auth,
            format: account.text_format,
            statuses: Mutex::new(None),
            me: Mutex::new(None),
        })
    }

    fn account(&self) -> &AccountId {
        self.auth.http().account_id()
    }

    async fn json(&self, req: HttpRequest) -> Result<Value, KeltaError> {
        Ok(self.auth.send_json::<Value>(req).await?.body)
    }

    /// Load `/issue_statuses.json` once (failures are ignored: names are used instead).
    async fn ensure_statuses(&self) {
        if self.statuses.lock().is_some() {
            return;
        }
        if let Ok(v) = self.json(HttpRequest::get(format!("{}/issue_statuses.json", self.base))).await {
            let list: Vec<(String, String, bool)> = v
                .get("issue_statuses")
                .and_then(Value::as_array)
                .map(|a| {
                    a.iter()
                        .filter_map(|x| {
                            Some((
                                x.get("id").and_then(idstr)?,
                                s(x, "name")?.to_owned(),
                                x.get("is_closed").and_then(Value::as_bool).unwrap_or(false),
                            ))
                        })
                        .collect()
                })
                .unwrap_or_default();
            *self.statuses.lock() = Some(list);
        }
    }

    fn status_from(&self, v: &Value) -> Status {
        let id = v.get("id").and_then(idstr).unwrap_or_default();
        let name = s(v, "name").unwrap_or("").to_owned();
        let closed = v.get("is_closed").and_then(Value::as_bool).or_else(|| {
            self.statuses
                .lock()
                .as_ref()
                .and_then(|l| l.iter().find(|(i, _, _)| *i == id).map(|(_, _, c)| *c))
        });
        let category = if closed == Some(true) {
            StatusCategory::Done
        } else {
            match category_from_name(&name) {
                StatusCategory::Unknown => StatusCategory::InProgress,
                c => c,
            }
        };
        Status { id, name, category }
    }

    fn user_from(v: &Value) -> Option<User> {
        let id = v.get("id").and_then(idstr)?;
        let name = s(v, "name").map(str::to_owned).unwrap_or_else(|| {
            [s(v, "firstname"), s(v, "lastname")].into_iter().flatten().collect::<Vec<_>>().join(" ")
        });
        Some(common::user_from(id, name, s(v, "login").map(str::to_owned), None))
    }

    fn ticket_from(&self, issue: &Value) -> Result<Ticket, KeltaError> {
        let id = issue
            .get("id")
            .and_then(idstr)
            .ok_or_else(|| KeltaError::upstream("redmine issue without id"))?;
        Ok(Ticket {
            r#ref: TicketRef { account: self.account().clone(), key: id.clone(), id: id.clone() },
            title: s(issue, "subject").unwrap_or("").to_owned(),
            url: format!("{}/issues/{id}", self.web),
            status: issue.get("status").map(|s| self.status_from(s)).unwrap_or_default(),
            kind: issue.pointer("/tracker/name").and_then(Value::as_str).map(str::to_owned),
            assignee: issue.get("assigned_to").and_then(Self::user_from),
            labels: Vec::new(),
            priority: issue.pointer("/priority/name").and_then(Value::as_str).map(str::to_owned),
            updated_at: s(issue, "updated_on").unwrap_or("").to_owned(),
            project_hint: issue.pointer("/project/id").and_then(idstr),
        })
    }

    fn render(&self, text: &str) -> String {
        match self.format {
            TextFormat::Markdown => markdown::to_html(text),
            TextFormat::Textile => markdown::plain_to_html(text),
        }
    }

    async fn fetch_issue(&self, id: &str, include: Option<&str>) -> Result<Value, KeltaError> {
        id.parse::<u64>().map_err(|_| KeltaError::invalid(format!("bad redmine id: {id}")))?;
        let mut req = HttpRequest::get(format!("{}/issues/{id}.json", self.base));
        if let Some(i) = include {
            req = req.query("include", i);
        }
        let v = self.json(req).await?;
        v.get("issue").cloned().ok_or_else(|| KeltaError::upstream("redmine response without `issue`"))
    }

    async fn put_issue(&self, id: &str, issue: Value) -> Result<(), KeltaError> {
        id.parse::<u64>().map_err(|_| KeltaError::invalid(format!("bad redmine id: {id}")))?;
        let url = format!("{}/issues/{id}.json", self.base);
        match self.auth.send_text(HttpRequest::put(url).json(json!({ "issue": issue }))).await {
            Ok(_) => Ok(()),
            Err(e) => Err(surface_422(e)),
        }
    }
}

/// The project's current version: the open one with the earliest `due_date >= today`, else an open undated one.
/// ISO dates compare as strings.
fn current_version(versions: &Value, today: &str) -> Option<String> {
    let open: Vec<&Value> = versions
        .get("versions")
        .and_then(Value::as_array)?
        .iter()
        .filter(|v| s(v, "status") == Some("open"))
        .collect();
    open.iter()
        .filter_map(|v| Some((s(v, "due_date").filter(|d| *d >= today)?, v)))
        .min_by_key(|(d, _)| *d)
        .map(|(_, v)| v)
        .or_else(|| open.iter().find(|v| s(v, "due_date").is_none()))
        .and_then(|v| v.get("id").and_then(idstr))
}

/// `422 {"errors": ["Subject cannot be blank"]}` → readable message + `detail.errors`.
fn surface_422(e: KeltaError) -> KeltaError {
    if e.code != ErrorCode::InvalidArgument {
        return e;
    }
    let errors: Vec<String> = e
        .detail
        .as_ref()
        .and_then(|d| d.get("body"))
        .and_then(Value::as_str)
        .and_then(|b| serde_json::from_str::<Value>(b).ok())
        .and_then(|v| v.get("errors").cloned())
        .and_then(|v| v.as_array().map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_owned)).collect()))
        .unwrap_or_default();
    if errors.is_empty() {
        return e;
    }
    KeltaError::new(ErrorCode::InvalidArgument, format!("Redmine rejected the change: {}", errors.join("; ")))
        .with_detail(json!({ "status": 422, "errors": errors }))
}

#[async_trait]
impl Tracker for RedmineTracker {
    fn kind(&self) -> TrackerKind {
        TrackerKind::Redmine
    }

    fn caps(&self) -> TrackerCaps {
        TrackerCaps {
            board_columns: false,
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
        let v = self.json(HttpRequest::get(format!("{}/users/current.json", self.base))).await?;
        let u = v
            .get("user")
            .and_then(Self::user_from)
            .ok_or_else(|| KeltaError::upstream("redmine /users/current returned no user"))?;
        *self.me.lock() = Some(u.clone());
        Ok(u)
    }

    async fn list(&self, view: &TrackerView, cursor: Option<Cursor>) -> Result<Page<Ticket>, KeltaError> {
        let offset = match cursor {
            None => 0,
            Some(Cursor::Offset(o)) => o,
            Some(_) => return Err(KeltaError::invalid("redmine expects an offset cursor")),
        };
        self.ensure_statuses().await;
        let mut req = HttpRequest::get(format!("{}/issues.json", self.base))
            .query("sort", "updated_on:desc")
            .query("limit", PAGE_SIZE.to_string())
            .query("offset", offset.to_string());
        if let Some(p) = &view.project_id {
            req = req.query("project_id", p.clone());
        }
        if view.current_iteration
            && let Some(p) = &view.project_id
        {
            let v = self.json(HttpRequest::get(format!("{}/projects/{p}/versions.json", self.base))).await?;
            let today = time::OffsetDateTime::now_utc().date();
            let today = format!("{:04}-{:02}-{:02}", today.year(), u8::from(today.month()), today.day());
            if let Some(id) = current_version(&v, &today) {
                req = req.query("fixed_version_id", id);
            }
        }
        if let Some(q) = view.query_id {
            // A saved query carries its own filters; `who` can only be applied to what comes back.
            req = req.query("query_id", q.to_string());
        } else {
            let status = match view.status.as_deref() {
                Some("closed") => "closed",
                Some("*") => "*",
                _ => "open",
            };
            req = req.query("status_id", status);
            match view.who {
                Some(Who::Mine) => req = req.query("assigned_to_id", "me"),
                Some(Who::Unassigned) => req = req.query("assigned_to_id", "!*"),
                Some(Who::Anyone) => {}
                None if view.assigned_to.as_deref().unwrap_or("me") != "any" => {
                    req = req.query("assigned_to_id", "me");
                }
                None => {}
            }
        }
        let v = self.json(req).await?;
        // Paging follows what Redmine returned, not what is left after the `who` filter below.
        let raw = v.get("issues").and_then(Value::as_array).map_or(0, Vec::len) as u64;
        let mut items: Vec<Ticket> = v
            .get("issues")
            .and_then(Value::as_array)
            .map(|a| a.iter().filter_map(|i| self.ticket_from(i).ok()).collect())
            .unwrap_or_default();
        if view.query_id.is_some() {
            match view.who {
                Some(Who::Mine) => {
                    let me = self.me().await?.id;
                    items.retain(|t| t.assignee.as_ref().is_some_and(|a| a.id == me));
                }
                Some(Who::Unassigned) => items.retain(|t| t.assignee.is_none()),
                _ => {}
            }
        }
        let total = v.get("total_count").and_then(Value::as_u64).unwrap_or(0);
        let end = offset as u64 + raw;
        let next = (raw > 0 && end < total).then_some(Cursor::Offset(end as u32));
        Ok(Page { items, next })
    }

    async fn get(&self, t: &TicketRef) -> Result<TicketDetail, KeltaError> {
        self.ensure_statuses().await;
        let issue = self.fetch_issue(&t.id, Some("journals,allowed_statuses")).await?;
        let ticket = self.ticket_from(&issue)?;
        let raw = s(&issue, "description").unwrap_or("").to_owned();
        let comments: Vec<_> = issue
            .get("journals")
            .and_then(Value::as_array)
            .map(|a| {
                a.iter()
                    .filter(|j| s(j, "notes").is_some_and(|n| !n.trim().is_empty()))
                    .map(|j| {
                        common::comment(
                            j.get("user").and_then(Self::user_from).unwrap_or_default(),
                            s(j, "created_on").unwrap_or("").to_owned(),
                            self.render(s(j, "notes").unwrap_or("")),
                        )
                    })
                    .collect()
            })
            .unwrap_or_default();
        let parent = issue.pointer("/parent/id").and_then(idstr).map(|id| TicketRef {
            account: t.account.clone(),
            key: id.clone(),
            id,
        });
        Ok(TicketDetail {
            ticket,
            body_html: self.render(&raw),
            body_md: raw,
            body_format: match self.format {
                TextFormat::Markdown => BodyFormat::Markdown,
                TextFormat::Textile => BodyFormat::Textile,
            },
            comments: common::last_n(comments, COMMENT_LIMIT),
            parent,
        })
    }

    async fn columns(&self, b: &TrackerBinding) -> Result<Vec<Column>, KeltaError> {
        if let Some(c) = columns_from_binding(b) {
            return Ok(c);
        }
        let v = self.json(HttpRequest::get(format!("{}/issue_statuses.json", self.base))).await?;
        let statuses: Vec<Status> = v
            .get("issue_statuses")
            .and_then(Value::as_array)
            .map(|a| a.iter().map(|x| self.status_from(x)).collect())
            .unwrap_or_default();
        Ok(columns_from_statuses(&statuses))
    }

    async fn transitions(&self, t: &TicketRef) -> Result<Vec<Transition>, KeltaError> {
        self.ensure_statuses().await;
        let issue = self.fetch_issue(&t.id, Some("allowed_statuses")).await?;
        let allowed: Vec<Status> = match issue.get("allowed_statuses").and_then(Value::as_array) {
            Some(a) => a.iter().map(|x| self.status_from(x)).collect(),
            // Older servers do not know the include: every status is a candidate.
            None => self
                .statuses
                .lock()
                .as_ref()
                .map(|l| {
                    l.iter()
                        .map(|(id, name, closed)| Status {
                            id: id.clone(),
                            name: name.clone(),
                            category: if *closed { StatusCategory::Done } else { category_from_name(name) },
                        })
                        .collect()
                })
                .unwrap_or_default(),
        };
        let current = issue.pointer("/status/id").and_then(idstr);
        Ok(allowed
            .into_iter()
            .filter(|st| Some(&st.id) != current.as_ref())
            .map(|st| Transition { id: st.id.clone(), name: st.name.clone(), to: st, needs_fields: false })
            .collect())
    }

    async fn transition(
        &self,
        t: &TicketRef,
        transition_id: &str,
        fields: Option<Value>,
    ) -> Result<Ticket, KeltaError> {
        let status_id = match transition_id.parse::<u64>() {
            Ok(n) => json!(n),
            Err(_) => json!(transition_id),
        };
        let mut issue = json!({ "status_id": status_id });
        if let Some(Value::Object(extra)) = fields {
            for (k, v) in extra {
                issue[k] = v;
            }
        }
        self.put_issue(&t.id, issue).await?;
        self.ticket_from(&self.fetch_issue(&t.id, None).await?)
    }

    async fn comment(&self, t: &TicketRef, markdown: &str) -> Result<(), KeltaError> {
        self.put_issue(&t.id, json!({ "notes": markdown })).await
    }

    async fn assign(&self, t: &TicketRef, who: Assignee) -> Result<Ticket, KeltaError> {
        let id = match who {
            Assignee::Me => json!(
                self.me()
                    .await?
                    .id
                    .parse::<u64>()
                    .map_err(|_| KeltaError::upstream("bad redmine user id"))?
            ),
            Assignee::User { id } => id.parse::<u64>().map(|n| json!(n)).unwrap_or_else(|_| json!(id)),
            // Redmine unassigns with an empty value.
            Assignee::None => json!(""),
        };
        self.put_issue(&t.id, json!({ "assigned_to_id": id })).await?;
        self.ticket_from(&self.fetch_issue(&t.id, None).await?)
    }

    async fn sources(&self, query: &str) -> Result<Vec<SourceHit>, KeltaError> {
        let q = query.trim().to_lowercase();
        let acct = self.account().to_string();
        let hit = |kind: &str, slug: String, label: &str, detail: String, mut view: TrackerView| {
            view.id = format!("{acct}-{slug}");
            view.label = label.to_owned();
            view.who = Some(Who::Mine);
            SourceHit { kind: kind.to_owned(), label: label.to_owned(), detail: Some(detail), view }
        };
        let mut out = Vec::new();
        // shortcut: first 100 projects and queries, filtered client-side; upgrade to paging for bigger sites.
        let v =
            self.json(HttpRequest::get(format!("{}/projects.json", self.base)).query("limit", "100")).await?;
        for p in v.get("projects").and_then(Value::as_array).into_iter().flatten() {
            let (Some(name), Some(ident)) = (s(p, "name"), s(p, "identifier")) else { continue };
            if q.is_empty() || name.to_lowercase().contains(&q) || ident.to_lowercase().contains(&q) {
                let view = TrackerView { project_id: Some(ident.to_owned()), ..TrackerView::default() };
                out.push(hit("project", format!("project-{ident}"), name, ident.to_owned(), view));
            }
        }
        let v =
            self.json(HttpRequest::get(format!("{}/queries.json", self.base)).query("limit", "100")).await?;
        for qy in v.get("queries").and_then(Value::as_array).into_iter().flatten() {
            let (Some(id), Some(name)) = (qy.get("id").and_then(Value::as_u64), s(qy, "name")) else {
                continue;
            };
            if q.is_empty() || name.to_lowercase().contains(&q) {
                let view = TrackerView {
                    query_id: Some(id),
                    project_id: qy.get("project_id").and_then(idstr),
                    ..TrackerView::default()
                };
                let public = qy.get("is_public").and_then(Value::as_bool).unwrap_or(false);
                let detail = if public { "Public query" } else { "Private query" };
                out.push(hit("query", format!("query-{id}"), name, detail.to_owned(), view));
            }
        }
        Ok(out)
    }

    fn browser_url(&self, t: &TicketRef) -> String {
        format!("{}/issues/{}", self.web, t.id)
    }

    fn branch_key(&self, t: &TicketRef) -> String {
        t.id.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn current_version_picks_the_next_open_due_date_then_an_undated_one() {
        let v = json!({"versions": [
            {"id": 1, "status": "closed", "due_date": "2026-10-20"},
            {"id": 2, "status": "open", "due_date": "2026-10-01"},
            {"id": 3, "status": "open", "due_date": "2026-11-15"},
            {"id": 4, "status": "open", "due_date": "2026-10-10"},
            {"id": 5, "status": "open", "due_date": null},
        ]});
        assert_eq!(current_version(&v, "2026-10-10").as_deref(), Some("4"));
        assert_eq!(current_version(&v, "2026-10-11").as_deref(), Some("3"));
        assert_eq!(current_version(&v, "2027-01-01").as_deref(), Some("5"));
        let past = json!({"versions": [{"id": 2, "status": "open", "due_date": "2026-10-01"}]});
        assert_eq!(current_version(&past, "2026-10-10"), None);
        assert_eq!(current_version(&json!({}), "2026-10-10"), None);
    }
}

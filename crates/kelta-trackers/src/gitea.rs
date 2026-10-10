//! Gitea / Forgejo Issues (REST v1, `<base>/api/v1`).
//!
//! Lists use `/repos/issues/search` (`assigned=true` for "assigned to me"; a `project` view filters
//! the rows by repository) or `/repos/{o}/{r}/issues` for `scope = "all"` on one repository.
//! Gitea issues are open or closed, so the board has two columns and the moves are Close / Reopen.
//! shortcut: no label-based workflow columns (GitLab's scoped labels); add if Gitea users ask.

use std::sync::Arc;

use async_trait::async_trait;
use kelta_http::util::{link_rel, percent_encode};
use kelta_http::{AuthScheme, Authed, HttpCtx, HttpRequest, markdown};
use kelta_proto::api::{SecretResolver, Tracker};
use kelta_proto::error::KeltaError;
use kelta_proto::ids::AccountId;
use kelta_proto::settings::{AccountConfig, AuthKind, TrackerBinding, TrackerView};
use kelta_proto::tracker::{
    Assignee, BodyFormat, Column, Cursor, Page, SourceHit, Status, StatusCategory, Ticket, TicketDetail,
    TicketRef, TrackerCaps, TrackerKind, Transition, User, Who,
};
use parking_lot::Mutex;
use serde_json::{Value, json};

use crate::common::{self, COMMENT_LIMIT, columns_from_binding, idstr, last_n, s, split_repo_number};

const PER_PAGE: u32 = 50;

pub struct GiteaIssues {
    api: String,
    web: String,
    auth: Authed,
    me: Mutex<Option<User>>,
}

impl GiteaIssues {
    pub fn new(
        account: &AccountConfig,
        http: HttpCtx,
        secrets: Arc<dyn SecretResolver>,
    ) -> Result<Self, KeltaError> {
        let base = common::base_url(account)?;
        let root = base.strip_suffix("/api/v1").unwrap_or(&base).to_owned();
        let scheme = match account.auth {
            Some(AuthKind::Basic) => AuthScheme::from_account(account).unwrap_or(AuthScheme::Bearer),
            _ => AuthScheme::Bearer,
        };
        let auth = Authed::new(http, secrets, account.effective_secret(), Some(&root), scheme)
            .with_header("Accept", "application/json");
        Ok(Self {
            web: account.web_url.as_deref().map(kelta_http::util::trim_url).unwrap_or_else(|| root.clone()),
            api: format!("{root}/api/v1"),
            auth,
            me: Mutex::new(None),
        })
    }

    fn account(&self) -> &AccountId {
        self.auth.http().account_id()
    }

    async fn json(&self, req: HttpRequest) -> Result<kelta_http::HttpResponse<Value>, KeltaError> {
        self.auth.send_json::<Value>(req).await
    }

    /// `/repos/<owner>/<name>/issues/<n><tail>` of a ticket key (`owner/name#n`, validated).
    fn issue_url(&self, t: &TicketRef, tail: &str) -> Result<String, KeltaError> {
        let (repo, n) = split_repo_number(&t.key)?;
        let (owner, name) = repo
            .split_once('/')
            .filter(|(_, n)| !n.contains('/'))
            .ok_or_else(|| KeltaError::invalid(format!("bad ticket key: {}", t.key)))?;
        Ok(format!("{}/repos/{}/{}/issues/{n}{tail}", self.api, percent_encode(owner), percent_encode(name)))
    }

    fn user_from(v: &Value) -> Option<User> {
        let login = s(v, "login")?;
        let name = s(v, "full_name").filter(|n| !n.is_empty()).unwrap_or(login);
        Some(common::user_from(login, name, Some(login.to_owned()), s(v, "avatar_url").map(str::to_owned)))
    }

    fn status(state: &str) -> Status {
        if state == "closed" {
            Status { id: "closed".into(), name: "Closed".into(), category: StatusCategory::Done }
        } else {
            Status { id: "open".into(), name: "Open".into(), category: StatusCategory::Todo }
        }
    }

    fn ticket_from(&self, v: &Value) -> Option<Ticket> {
        let n = v.get("number").and_then(Value::as_u64)?;
        let repo = v.pointer("/repository/full_name").and_then(Value::as_str)?;
        let labels: Vec<String> = v
            .get("labels")
            .and_then(Value::as_array)
            .map(|a| a.iter().filter_map(|l| s(l, "name").map(str::to_owned)).collect())
            .unwrap_or_default();
        Some(Ticket {
            r#ref: TicketRef {
                account: self.account().clone(),
                key: format!("{repo}#{n}"),
                id: v.get("id").and_then(idstr).unwrap_or_else(|| n.to_string()),
            },
            title: s(v, "title").unwrap_or("").to_owned(),
            url: s(v, "html_url")
                .map(str::to_owned)
                .unwrap_or_else(|| format!("{}/{repo}/issues/{n}", self.web)),
            status: Self::status(s(v, "state").unwrap_or("open")),
            kind: None,
            assignee: v
                .get("assignee")
                .filter(|a| !a.is_null())
                .or_else(|| v.get("assignees").and_then(|a| a.get(0)))
                .and_then(Self::user_from),
            priority: labels.iter().find_map(|l| l.strip_prefix("priority/").map(str::to_owned)),
            labels,
            updated_at: s(v, "updated_at").unwrap_or("").to_owned(),
            project_hint: Some(repo.to_owned()),
            ..Default::default()
        })
    }

    async fn patch(&self, t: &TicketRef, body: Value) -> Result<Ticket, KeltaError> {
        let v = self.json(HttpRequest::patch(self.issue_url(t, "")?).json(body)).await?.body;
        self.ticket_from(&v).ok_or_else(|| KeltaError::upstream("gitea issue response without number"))
    }
}

fn state_param(view: &TrackerView) -> &'static str {
    match view.status.as_deref() {
        Some("closed") => "closed",
        Some("*") => "all",
        _ => "open",
    }
}

#[async_trait]
impl Tracker for GiteaIssues {
    fn kind(&self) -> TrackerKind {
        TrackerKind::GiteaIssues
    }

    fn caps(&self) -> TrackerCaps {
        TrackerCaps {
            board_columns: true,
            assign: true,
            comment: true,
            transitions_need_fetch: false,
            projects_v2: false,
        }
    }

    async fn me(&self) -> Result<User, KeltaError> {
        if let Some(u) = self.me.lock().clone() {
            return Ok(u);
        }
        let v = self.json(HttpRequest::get(format!("{}/user", self.api))).await?.body;
        let u = Self::user_from(&v).ok_or_else(|| KeltaError::upstream("gitea /user returned no login"))?;
        *self.me.lock() = Some(u.clone());
        Ok(u)
    }

    async fn list(&self, view: &TrackerView, cursor: Option<Cursor>) -> Result<Page<Ticket>, KeltaError> {
        let page = match cursor {
            None => 1,
            Some(Cursor::Page(p)) => p.max(1),
            Some(_) => return Err(KeltaError::invalid("gitea issues expect a page cursor")),
        };
        let project = view.project.as_deref().filter(|p| !p.is_empty());
        // `who` wins over the legacy `scope`.
        let all = view.who.map_or(view.scope.as_deref() == Some("all"), |w| w != Who::Mine);
        if matches!(view.who, Some(Who::Unassigned | Who::Anyone)) && project.is_none() {
            return Err(KeltaError::invalid("gitea needs a repository to list unassigned or all tickets"));
        }
        let url = match (project, all) {
            (Some(p), true) => {
                let (owner, name) = p
                    .split_once('/')
                    .filter(|(a, b)| !a.is_empty() && !b.is_empty() && !b.contains('/') && !p.contains(".."))
                    .ok_or_else(|| KeltaError::invalid(format!("bad gitea project: {p}")))?;
                format!("{}/repos/{}/{}/issues", self.api, percent_encode(owner), percent_encode(name))
            }
            _ => format!("{}/repos/issues/search", self.api),
        };
        let mut req = HttpRequest::get(url)
            .query("type", "issues")
            .query("state", state_param(view))
            .query("limit", PER_PAGE.to_string())
            .query("page", page.to_string());
        if !all {
            req = req.query("assigned", "true");
        }
        if let Some(l) = view.labels.as_ref().filter(|l| !l.is_empty()) {
            req = req.query("labels", l.join(","));
        }
        let resp = self.json(req.with_etag()).await?;
        let items: Vec<Ticket> = resp
            .body
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|i| self.ticket_from(i))
                    // shortcut: short pages, page-refill later (project and unassigned filters trim a page)
                    .filter(|t| view.who != Some(Who::Unassigned) || t.assignee.is_none())
                    .filter(|t| project.is_none_or(|p| t.project_hint.as_deref() == Some(p)))
                    .collect()
            })
            .unwrap_or_default();
        Ok(Page { items, next: link_rel(&resp.headers, "next").map(|_| Cursor::Page(page + 1)) })
    }

    async fn sources(&self, query: &str) -> Result<Vec<SourceHit>, KeltaError> {
        let body = self
            .json(
                HttpRequest::get(format!("{}/repos/search", self.api))
                    .query("q", query.trim())
                    .query("limit", "50"),
            )
            .await?
            .body;
        Ok(body
            .get("data")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|r| {
                let name = s(r, "full_name")?;
                Some(SourceHit {
                    kind: "repo".into(),
                    label: name.to_owned(),
                    detail: s(r, "description").filter(|d| !d.is_empty()).map(str::to_owned),
                    view: TrackerView {
                        id: format!("gitea:repo:{name}"),
                        label: name.to_owned(),
                        project: Some(name.to_owned()),
                        who: Some(Who::Mine),
                        ..TrackerView::default()
                    },
                })
            })
            .collect())
    }

    async fn get(&self, t: &TicketRef) -> Result<TicketDetail, KeltaError> {
        let raw = self.json(HttpRequest::get(self.issue_url(t, "")?)).await?.body;
        let ticket = self
            .ticket_from(&raw)
            .ok_or_else(|| KeltaError::upstream("gitea issue response without number"))?;
        let notes = self.json(HttpRequest::get(self.issue_url(t, "/comments")?)).await?.body;
        let comments: Vec<_> = notes
            .as_array()
            .map(|a| {
                a.iter()
                    .map(|n| {
                        common::comment(
                            n.get("user").and_then(Self::user_from).unwrap_or_default(),
                            s(n, "created_at").unwrap_or("").to_owned(),
                            markdown::to_html(s(n, "body").unwrap_or("")),
                        )
                    })
                    .collect()
            })
            .unwrap_or_default();
        let body = s(&raw, "body").unwrap_or("").to_owned();
        Ok(TicketDetail {
            ticket,
            body_html: markdown::to_html(&body),
            body_md: body,
            body_format: BodyFormat::Markdown,
            comments: last_n(comments, COMMENT_LIMIT),
            parent: None,
            prs: Vec::new(),
            caps: Default::default(),
        })
    }

    async fn columns(&self, b: &TrackerBinding) -> Result<Vec<Column>, KeltaError> {
        Ok(columns_from_binding(b).unwrap_or_else(|| {
            [("open", "Open", StatusCategory::Todo), ("closed", "Closed", StatusCategory::Done)]
                .into_iter()
                .enumerate()
                .map(|(i, (id, name, category))| Column {
                    id: id.into(),
                    name: name.into(),
                    category,
                    order: i as u32,
                    match_names: vec![name.into()],
                })
                .collect()
        }))
    }

    async fn transitions(&self, t: &TicketRef) -> Result<Vec<Transition>, KeltaError> {
        let raw = self.json(HttpRequest::get(self.issue_url(t, "")?)).await?.body;
        Ok(vec![if s(&raw, "state") == Some("closed") {
            Transition {
                id: "reopen".into(),
                name: "Reopen".into(),
                to: Self::status("open"),
                needs_fields: false,
            }
        } else {
            Transition {
                id: "close".into(),
                name: "Close".into(),
                to: Self::status("closed"),
                needs_fields: false,
            }
        }])
    }

    async fn transition(
        &self,
        t: &TicketRef,
        transition_id: &str,
        _fields: Option<Value>,
    ) -> Result<Ticket, KeltaError> {
        let state = match transition_id {
            "close" => "closed",
            "reopen" => "open",
            other => return Err(KeltaError::invalid(format!("unknown gitea transition id: {other}"))),
        };
        self.patch(t, json!({"state": state})).await
    }

    async fn comment(&self, t: &TicketRef, markdown: &str) -> Result<(), KeltaError> {
        self.auth
            .send_text(HttpRequest::post(self.issue_url(t, "/comments")?).json(json!({"body": markdown})))
            .await?;
        Ok(())
    }

    async fn assign(&self, t: &TicketRef, who: Assignee) -> Result<Ticket, KeltaError> {
        let logins: Vec<String> = match who {
            Assignee::Me => vec![self.me().await?.id],
            Assignee::User { id } => vec![id],
            Assignee::None => vec![],
        };
        self.patch(t, json!({"assignees": logins})).await
    }

    fn browser_url(&self, t: &TicketRef) -> String {
        match split_repo_number(&t.key) {
            Ok((repo, n)) => format!("{}/{repo}/issues/{n}", self.web),
            Err(_) => self.web.clone(),
        }
    }

    fn branch_key(&self, t: &TicketRef) -> String {
        match split_repo_number(&t.key) {
            Ok((_, n)) => format!("gt-{n}"),
            Err(_) => t.key.clone(),
        }
    }
}

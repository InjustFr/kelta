//! GitHub Issues (REST) with Projects v2 moves (GraphQL).
//!
//! Lists: `/issues?filter=assigned`, `/repos/{r}/issues`, `/search/issues` (ETag on every GET).
//! Moves: without a project, open/closed. With a Projects v2 item, the Status single-select
//! field and its option ids are resolved **by name** at runtime and cached per project; nothing is
//! hard-coded.

use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use kelta_http::graphql::graphql;
use kelta_http::util::{link_rel, percent_encode};
use kelta_http::{AuthScheme, Authed, HttpCtx, HttpRequest, markdown};
use kelta_proto::api::{SecretResolver, Tracker};
use kelta_proto::error::KeltaError;
use kelta_proto::ids::AccountId;
use kelta_proto::settings::{AccountConfig, TrackerBinding, TrackerView};
use kelta_proto::tracker::{
    Assignee, BodyFormat, Column, Cursor, Page, Status, StatusCategory, Ticket, TicketDetail, TicketRef,
    TrackerCaps, TrackerKind, Transition, User,
};
use parking_lot::Mutex;
use serde_json::{Value, json};

use crate::common::{
    self, COMMENT_LIMIT, category_from_name, columns_from_binding, idstr, s, split_repo_number,
};

const PER_PAGE: u32 = 50;
const DEFAULT_STATUS_FIELD: &str = "Status";

/// Single-select field of a Projects v2 project.
#[derive(Debug, Clone)]
struct FieldMeta {
    field_id: String,
    options: Vec<(String, String)>,
}

impl FieldMeta {
    fn option(&self, name: &str) -> Option<&(String, String)> {
        self.options.iter().find(|(_, n)| n.eq_ignore_ascii_case(name.trim()))
    }
}

/// One Projects v2 item of an issue.
#[derive(Debug, Clone)]
struct ItemInfo {
    item_id: String,
    project_id: String,
    owner: String,
    number: u32,
    current: Option<String>,
}

#[derive(Debug, Clone)]
struct IssueProjects {
    open: bool,
    items: Vec<ItemInfo>,
}

pub struct GithubIssues {
    api: String,
    graphql: String,
    web: String,
    auth: Authed,
    me: Mutex<Option<User>>,
    /// project node id → Status field (cached for the provider's lifetime).
    fields: Mutex<HashMap<String, FieldMeta>>,
    /// (owner lower-case, project number) → configured status field name.
    status_fields: Mutex<HashMap<(String, u32), String>>,
}

/// `https://api.github.com` → `https://github.com`; GHE `https://h/api/v3` → `https://h`.
pub(crate) fn web_base(api: &str) -> String {
    if api == "https://api.github.com" {
        return "https://github.com".into();
    }
    api.strip_suffix("/api/v3").unwrap_or(api).to_owned()
}

/// GraphQL endpoint: `https://api.github.com/graphql`, GHE `https://h/api/graphql`.
pub(crate) fn graphql_url(api: &str) -> String {
    match api.strip_suffix("/api/v3") {
        Some(root) => format!("{root}/api/graphql"),
        None => format!("{api}/graphql"),
    }
}

impl GithubIssues {
    pub fn new(
        account: &AccountConfig,
        http: HttpCtx,
        secrets: Arc<dyn SecretResolver>,
    ) -> Result<Self, KeltaError> {
        let api = common::base_url(account)?;
        let scheme = AuthScheme::from_account(account).unwrap_or(AuthScheme::Bearer);
        let auth = Authed::new(http, secrets, account.effective_secret(), Some(&api), scheme)
            .with_header("Accept", "application/vnd.github+json")
            .with_header("X-GitHub-Api-Version", "2022-11-28");
        Ok(Self {
            web: account.web_url.as_deref().map(kelta_http::util::trim_url).unwrap_or_else(|| web_base(&api)),
            graphql: graphql_url(&api),
            api,
            auth,
            me: Mutex::new(None),
            fields: Mutex::new(HashMap::new()),
            status_fields: Mutex::new(HashMap::new()),
        })
    }

    fn account(&self) -> &AccountId {
        self.auth.http().account_id()
    }

    async fn gql(&self, query: &str, vars: Value) -> Result<Value, KeltaError> {
        graphql(&self.auth, &self.graphql, query, vars).await
    }

    fn user_from(v: &Value) -> Option<User> {
        let login = s(v, "login")?;
        Some(common::user_from(
            login,
            s(v, "name").unwrap_or(login),
            Some(login.to_owned()),
            s(v, "avatar_url").or_else(|| s(v, "avatarUrl")).map(str::to_owned),
        ))
    }

    fn remember_view(&self, view: &TrackerView) {
        if let Some(p) = &view.project_v2 {
            self.status_fields
                .lock()
                .insert((p.owner.to_ascii_lowercase(), p.number), p.status_field.clone());
        }
    }

    fn status_field_name(&self, owner: &str, number: u32) -> String {
        self.status_fields
            .lock()
            .get(&(owner.to_ascii_lowercase(), number))
            .cloned()
            .unwrap_or_else(|| DEFAULT_STATUS_FIELD.to_owned())
    }

    /// `repository_url` (`…/repos/acme/shop`) or `repository.full_name` of an issue.
    fn repo_of(v: &Value) -> Option<String> {
        if let Some(u) = s(v, "repository_url") {
            return u.rsplit_once("/repos/").map(|(_, r)| r.to_owned());
        }
        v.pointer("/repository/full_name").and_then(Value::as_str).map(str::to_owned)
    }

    fn status_for(open: bool, project_status: Option<&str>) -> Status {
        if !open {
            return Status { id: "closed".into(), name: "Closed".into(), category: StatusCategory::Done };
        }
        match project_status {
            Some(n) => Status {
                id: n.to_owned(),
                name: n.to_owned(),
                category: match category_from_name(n) {
                    StatusCategory::Unknown => StatusCategory::Todo,
                    c => c,
                },
            },
            None => Status { id: "open".into(), name: "Open".into(), category: StatusCategory::Todo },
        }
    }

    /// REST issue → ticket; `None` for pull requests and unrecognisable items.
    fn ticket_from_rest(&self, v: &Value, project_status: Option<&str>) -> Option<Ticket> {
        if v.get("pull_request").is_some() {
            return None;
        }
        let repo = Self::repo_of(v)?;
        let number = v.get("number").and_then(Value::as_u64)?;
        let assignee = v
            .get("assignee")
            .filter(|a| !a.is_null())
            .or_else(|| v.get("assignees").and_then(|a| a.get(0)))
            .and_then(Self::user_from);
        Some(Ticket {
            r#ref: TicketRef {
                account: self.account().clone(),
                key: format!("{repo}#{number}"),
                id: v.get("id").and_then(idstr).unwrap_or_else(|| number.to_string()),
            },
            title: s(v, "title").unwrap_or("").to_owned(),
            url: s(v, "html_url")
                .map(str::to_owned)
                .unwrap_or_else(|| format!("{}/{repo}/issues/{number}", self.web)),
            status: Self::status_for(s(v, "state") != Some("closed"), project_status),
            kind: v.pointer("/type/name").and_then(Value::as_str).map(str::to_owned),
            assignee,
            labels: v
                .get("labels")
                .and_then(Value::as_array)
                .map(|a| {
                    a.iter().filter_map(|l| l.as_str().or_else(|| s(l, "name")).map(str::to_owned)).collect()
                })
                .unwrap_or_default(),
            priority: None,
            updated_at: s(v, "updated_at").unwrap_or("").to_owned(),
            project_hint: Some(repo),
        })
    }

    async fn rest(&self, req: HttpRequest) -> Result<kelta_http::HttpResponse<Value>, KeltaError> {
        self.auth.send_json::<Value>(req).await
    }

    async fn fetch_issue(&self, repo: &str, number: u64) -> Result<Value, KeltaError> {
        Ok(self.rest(HttpRequest::get(format!("{}/repos/{repo}/issues/{number}", self.api))).await?.body)
    }

    // ---- Projects v2 -------------------------------------------------------------------

    async fn list_project_items(
        &self,
        p: &kelta_proto::settings::ProjectV2Ref,
        after: Option<String>,
    ) -> Result<Page<Ticket>, KeltaError> {
        const Q: &str = "query($owner:String!,$number:Int!,$after:String){ repositoryOwner(login:$owner){ ... on ProjectV2Owner { projectV2(number:$number){ items(first:50, after:$after, orderBy:{field:POSITION, direction:ASC}){ pageInfo{hasNextPage endCursor} nodes{ fieldValues(first:20){ nodes{ ... on ProjectV2ItemFieldSingleSelectValue { name field{ ... on ProjectV2SingleSelectField{ name } } } } } content{ ... on Issue { id databaseId number title url state updatedAt repository{ nameWithOwner } assignees(first:1){ nodes{ login name avatarUrl } } labels(first:10){ nodes{ name } } issueType{ name } } } } } } } }";
        let data = self.gql(Q, json!({"owner": p.owner, "number": p.number, "after": after})).await?;
        let items = data.pointer("/repositoryOwner/projectV2/items");
        let nodes = items.and_then(|i| i.get("nodes")).and_then(Value::as_array).cloned().unwrap_or_default();
        let mut out = Vec::new();
        for n in &nodes {
            let Some(c) = n.get("content").filter(|c| c.get("number").is_some()) else { continue };
            let status = n
                .pointer("/fieldValues/nodes")
                .and_then(Value::as_array)
                .and_then(|vals| {
                    vals.iter().find(|fv| {
                        fv.pointer("/field/name")
                            .and_then(Value::as_str)
                            .is_some_and(|f| f.eq_ignore_ascii_case(&p.status_field))
                    })
                })
                .and_then(|fv| s(fv, "name"))
                .map(str::to_owned);
            let repo =
                c.pointer("/repository/nameWithOwner").and_then(Value::as_str).unwrap_or("").to_owned();
            let number = c.get("number").and_then(Value::as_u64).unwrap_or(0);
            if repo.is_empty() {
                continue;
            }
            out.push(Ticket {
                r#ref: TicketRef {
                    account: self.account().clone(),
                    key: format!("{repo}#{number}"),
                    id: c.get("databaseId").and_then(idstr).unwrap_or_else(|| number.to_string()),
                },
                title: s(c, "title").unwrap_or("").to_owned(),
                url: s(c, "url").unwrap_or("").to_owned(),
                status: Self::status_for(s(c, "state") != Some("CLOSED"), status.as_deref()),
                kind: c.pointer("/issueType/name").and_then(Value::as_str).map(str::to_owned),
                assignee: c.pointer("/assignees/nodes/0").and_then(Self::user_from),
                labels: c
                    .pointer("/labels/nodes")
                    .and_then(Value::as_array)
                    .map(|a| a.iter().filter_map(|l| s(l, "name").map(str::to_owned)).collect())
                    .unwrap_or_default(),
                priority: None,
                updated_at: s(c, "updatedAt").unwrap_or("").to_owned(),
                project_hint: Some(repo),
            });
        }
        let has_next =
            items.and_then(|i| i.pointer("/pageInfo/hasNextPage")).and_then(Value::as_bool) == Some(true);
        let end = items.and_then(|i| i.pointer("/pageInfo/endCursor")).and_then(Value::as_str);
        let next =
            if has_next && !nodes.is_empty() { end.map(|e| Cursor::After(e.to_owned())) } else { None };
        Ok(Page { items: out, next })
    }

    /// Projects v2 items of an issue (ids and current Status value; no field metadata).
    async fn issue_projects(&self, repo: &str, number: u64) -> Result<IssueProjects, KeltaError> {
        const Q: &str = "query($owner:String!,$name:String!,$number:Int!){ repository(owner:$owner,name:$name){ issue(number:$number){ state projectItems(first:20){ nodes{ id project{ id number owner{ ... on Organization{ login } ... on User{ login } } } fieldValues(first:20){ nodes{ ... on ProjectV2ItemFieldSingleSelectValue { name field{ ... on ProjectV2SingleSelectField{ name } } } } } } } } } }";
        let (owner, name) =
            repo.split_once('/').ok_or_else(|| KeltaError::invalid(format!("bad repo: {repo}")))?;
        let data = self.gql(Q, json!({"owner": owner, "name": name, "number": number})).await?;
        let issue = data
            .pointer("/repository/issue")
            .filter(|v| !v.is_null())
            .ok_or_else(|| KeltaError::not_found(format!("{repo}#{number}")))?;
        let nodes =
            issue.pointer("/projectItems/nodes").and_then(Value::as_array).cloned().unwrap_or_default();
        let items = nodes
            .iter()
            .filter_map(|n| {
                let owner =
                    n.pointer("/project/owner/login").and_then(Value::as_str).unwrap_or("").to_owned();
                let pnum = n.pointer("/project/number").and_then(Value::as_u64).unwrap_or(0) as u32;
                let field = self.status_field_name(&owner, pnum);
                let current = n
                    .pointer("/fieldValues/nodes")
                    .and_then(Value::as_array)
                    .and_then(|vals| {
                        vals.iter().find(|fv| {
                            fv.pointer("/field/name")
                                .and_then(Value::as_str)
                                .is_some_and(|f| f.eq_ignore_ascii_case(&field))
                        })
                    })
                    .and_then(|fv| s(fv, "name"))
                    .map(str::to_owned);
                Some(ItemInfo {
                    item_id: s(n, "id")?.to_owned(),
                    project_id: n.pointer("/project/id").and_then(Value::as_str)?.to_owned(),
                    owner,
                    number: pnum,
                    current,
                })
            })
            .collect();
        Ok(IssueProjects { open: s(issue, "state") != Some("CLOSED"), items })
    }

    /// Status field of a project, resolved by name once and cached.
    async fn status_field(&self, item: &ItemInfo, refresh: bool) -> Result<Option<FieldMeta>, KeltaError> {
        if !refresh && let Some(f) = self.fields.lock().get(&item.project_id).cloned() {
            return Ok(Some(f));
        }
        const Q: &str = "query($id:ID!){ node(id:$id){ ... on ProjectV2 { fields(first:50){ nodes{ ... on ProjectV2SingleSelectField { id name options{ id name } } } } } } }";
        let data = self.gql(Q, json!({"id": item.project_id})).await?;
        let wanted = self.status_field_name(&item.owner, item.number);
        let meta = data
            .pointer("/node/fields/nodes")
            .and_then(Value::as_array)
            .and_then(|fs| fs.iter().find(|f| s(f, "name").is_some_and(|n| n.eq_ignore_ascii_case(&wanted))))
            .and_then(|f| {
                Some(FieldMeta {
                    field_id: s(f, "id")?.to_owned(),
                    options: f
                        .get("options")
                        .and_then(Value::as_array)
                        .map(|o| {
                            o.iter()
                                .filter_map(|x| Some((s(x, "id")?.to_owned(), s(x, "name")?.to_owned())))
                                .collect()
                        })
                        .unwrap_or_default(),
                })
            });
        if let Some(m) = &meta {
            self.fields.lock().insert(item.project_id.clone(), m.clone());
        }
        Ok(meta)
    }

    async fn move_in_project(
        &self,
        repo: &str,
        number: u64,
        option_name: &str,
    ) -> Result<Ticket, KeltaError> {
        let info = self.issue_projects(repo, number).await?;
        if info.items.is_empty() {
            return Err(KeltaError::not_found(format!("{repo}#{number} is not in a Projects v2 project")));
        }
        const M: &str = "mutation($p:ID!,$i:ID!,$f:ID!,$o:String!){ updateProjectV2ItemFieldValue(input:{projectId:$p,itemId:$i,fieldId:$f,value:{singleSelectOptionId:$o}}){ projectV2Item{ id } } }";
        let mut last_err = None;
        for item in &info.items {
            // First try with the cached field; if the option is unknown or the mutation fails
            // (stale ids), resolve the field again and retry once.
            for refresh in [false, true] {
                let Some(meta) = self.status_field(item, refresh).await? else { break };
                let Some((opt_id, opt_name)) = meta.option(option_name).cloned() else { continue };
                match self
                    .gql(M, json!({"p": item.project_id, "i": item.item_id, "f": meta.field_id, "o": opt_id}))
                    .await
                {
                    Ok(_) => {
                        let issue = self.fetch_issue(repo, number).await?;
                        return self
                            .ticket_from_rest(&issue, Some(&opt_name))
                            .ok_or_else(|| KeltaError::upstream("issue response without repository"));
                    }
                    Err(e) => {
                        last_err = Some(e);
                        self.fields.lock().remove(&item.project_id);
                    }
                }
            }
        }
        Err(last_err.unwrap_or_else(|| {
            KeltaError::not_found(format!("no Status option named `{option_name}` on {repo}#{number}"))
        }))
    }

    fn page_cursor(c: Option<Cursor>) -> Result<u32, KeltaError> {
        match c {
            None => Ok(1),
            Some(Cursor::Page(p)) => Ok(p.max(1)),
            Some(_) => Err(KeltaError::invalid("github issues expect a page cursor")),
        }
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
impl Tracker for GithubIssues {
    fn kind(&self) -> TrackerKind {
        TrackerKind::GithubIssues
    }

    fn caps(&self) -> TrackerCaps {
        TrackerCaps {
            board_columns: true,
            assign: true,
            comment: true,
            transitions_need_fetch: true,
            projects_v2: true,
        }
    }

    async fn me(&self) -> Result<User, KeltaError> {
        if let Some(u) = self.me.lock().clone() {
            return Ok(u);
        }
        let v = self.rest(HttpRequest::get(format!("{}/user", self.api))).await?.body;
        let u = Self::user_from(&v).ok_or_else(|| KeltaError::upstream("github /user returned no login"))?;
        *self.me.lock() = Some(u.clone());
        Ok(u)
    }

    async fn list(&self, view: &TrackerView, cursor: Option<Cursor>) -> Result<Page<Ticket>, KeltaError> {
        self.remember_view(view);
        if let Some(p) = &view.project_v2 {
            let after = match cursor {
                None => None,
                Some(Cursor::After(a)) => Some(a),
                Some(_) => return Err(KeltaError::invalid("github projects expect an `after` cursor")),
            };
            return self.list_project_items(p, after).await;
        }
        let page = Self::page_cursor(cursor)?;
        let state = state_param(view);
        let mut req = if let Some(q) = view.search.as_deref().filter(|q| !q.trim().is_empty()) {
            let q = if q.contains("is:issue") || q.contains("is:pr") {
                q.to_owned()
            } else {
                format!("{q} is:issue")
            };
            HttpRequest::get(format!("{}/search/issues", self.api))
                .query("q", q)
                .query("sort", "updated")
                .query("order", "desc")
        } else if let Some(repo) = view.repo.as_deref().filter(|r| !r.is_empty()) {
            let mut r = HttpRequest::get(format!("{}/repos/{repo}/issues", self.api))
                .query("state", state)
                .query("sort", "updated")
                .query("direction", "desc");
            if view.assigned_to.as_deref() == Some("me") {
                r = r.query("assignee", self.me().await?.id);
            }
            r
        } else {
            HttpRequest::get(format!("{}/issues", self.api))
                .query("filter", "assigned")
                .query("state", state)
                .query("sort", "updated")
                .query("direction", "desc")
        };
        req = req.query("per_page", PER_PAGE.to_string()).query("page", page.to_string()).with_etag();
        let resp = self.rest(req).await?;
        let list = if resp.body.is_array() {
            resp.body.clone()
        } else {
            resp.body.get("items").cloned().unwrap_or(Value::Null)
        };
        let items: Vec<Ticket> = list
            .as_array()
            .map(|a| a.iter().filter_map(|i| self.ticket_from_rest(i, None)).collect())
            .unwrap_or_default();
        let has_next = link_rel(&resp.headers, "next").is_some();
        Ok(Page { items, next: has_next.then_some(Cursor::Page(page + 1)) })
    }

    async fn get(&self, t: &TicketRef) -> Result<TicketDetail, KeltaError> {
        let (repo, number) = split_repo_number(&t.key)?;
        let issue = self.fetch_issue(&repo, number).await?;
        let ticket = self
            .ticket_from_rest(&issue, None)
            .ok_or_else(|| KeltaError::not_found(format!("{} is a pull request, not an issue", t.key)))?;
        // Comments are oldest first: take the last page(s) so we show the newest 20.
        let url = format!("{}/repos/{repo}/issues/{number}/comments", self.api);
        let first = self.rest(HttpRequest::get(&url).query("per_page", COMMENT_LIMIT.to_string())).await?;
        let last_page = link_rel(&first.headers, "last")
            .and_then(|u| kelta_http::util::query_param(&u, "page"))
            .and_then(|p| p.parse::<u32>().ok());
        let mut raw: Vec<Value> = first.body.as_array().cloned().unwrap_or_default();
        if let Some(last) = last_page.filter(|l| *l > 1) {
            let mut tail = self
                .rest(
                    HttpRequest::get(&url)
                        .query("per_page", COMMENT_LIMIT.to_string())
                        .query("page", last.to_string()),
                )
                .await?
                .body
                .as_array()
                .cloned()
                .unwrap_or_default();
            if tail.len() < COMMENT_LIMIT && last > 1 {
                let mut prev = self
                    .rest(
                        HttpRequest::get(&url)
                            .query("per_page", COMMENT_LIMIT.to_string())
                            .query("page", (last - 1).to_string()),
                    )
                    .await?
                    .body
                    .as_array()
                    .cloned()
                    .unwrap_or_default();
                prev.append(&mut tail);
                tail = prev;
            }
            raw = tail;
        }
        let comments = raw
            .iter()
            .map(|c| {
                common::comment(
                    c.get("user").and_then(Self::user_from).unwrap_or_default(),
                    s(c, "created_at").unwrap_or("").to_owned(),
                    markdown::to_html(s(c, "body").unwrap_or("")),
                )
            })
            .collect();
        let body = s(&issue, "body").unwrap_or("").to_owned();
        Ok(TicketDetail {
            ticket,
            body_html: markdown::to_html(&body),
            body_md: body,
            body_format: BodyFormat::Markdown,
            comments: common::last_n(comments, COMMENT_LIMIT),
            parent: None,
        })
    }

    async fn columns(&self, b: &TrackerBinding) -> Result<Vec<Column>, KeltaError> {
        for v in &b.views {
            self.remember_view(v);
        }
        if let Some(c) = columns_from_binding(b) {
            return Ok(c);
        }
        if let Some(p) = b.views.iter().find_map(|v| v.project_v2.as_ref()) {
            const Q: &str = "query($owner:String!,$number:Int!){ repositoryOwner(login:$owner){ ... on ProjectV2Owner { projectV2(number:$number){ id fields(first:50){ nodes{ ... on ProjectV2SingleSelectField { id name options{ id name } } } } } } } }";
            let data = self.gql(Q, json!({"owner": p.owner, "number": p.number})).await?;
            let field = data
                .pointer("/repositoryOwner/projectV2/fields/nodes")
                .and_then(Value::as_array)
                .and_then(|fs| {
                    fs.iter().find(|f| s(f, "name").is_some_and(|n| n.eq_ignore_ascii_case(&p.status_field)))
                })
                .ok_or_else(|| {
                    KeltaError::not_found(format!("project #{} has no `{}` field", p.number, p.status_field))
                })?;
            return Ok(field
                .get("options")
                .and_then(Value::as_array)
                .map(|o| {
                    o.iter()
                        .enumerate()
                        .filter_map(|(i, x)| {
                            let name = s(x, "name")?.to_owned();
                            Some(Column {
                                id: s(x, "id").unwrap_or(&name).to_owned(),
                                category: category_from_name(&name),
                                order: i as u32,
                                match_names: vec![name.clone()],
                                name,
                            })
                        })
                        .collect()
                })
                .unwrap_or_default());
        }
        Ok(vec![
            Column {
                id: "open".into(),
                name: "Open".into(),
                category: StatusCategory::Todo,
                order: 0,
                match_names: vec!["Open".into()],
            },
            Column {
                id: "closed".into(),
                name: "Closed".into(),
                category: StatusCategory::Done,
                order: 1,
                match_names: vec!["Closed".into()],
            },
        ])
    }

    async fn transitions(&self, t: &TicketRef) -> Result<Vec<Transition>, KeltaError> {
        let (repo, number) = split_repo_number(&t.key)?;
        let info = self.issue_projects(&repo, number).await?;
        let mut out: Vec<Transition> = Vec::new();
        for item in &info.items {
            let Some(meta) = self.status_field(item, false).await? else { continue };
            for (id, name) in &meta.options {
                if out.iter().any(|t| t.name.eq_ignore_ascii_case(name))
                    || item.current.as_deref() == Some(name)
                {
                    continue;
                }
                out.push(Transition {
                    id: format!("status:{name}"),
                    name: name.clone(),
                    to: Status { id: id.clone(), name: name.clone(), category: category_from_name(name) },
                    needs_fields: false,
                });
            }
        }
        if out.is_empty() && info.items.is_empty() {
            out.push(if info.open {
                Transition {
                    id: "closed".into(),
                    name: "Closed".into(),
                    to: Self::status_for(false, None),
                    needs_fields: false,
                }
            } else {
                Transition {
                    id: "open".into(),
                    name: "Open".into(),
                    to: Self::status_for(true, None),
                    needs_fields: false,
                }
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
        let (repo, number) = split_repo_number(&t.key)?;
        let url = format!("{}/repos/{repo}/issues/{number}", self.api);
        let state_change = |state: &str, reason: &str| json!({"state": state, "state_reason": reason});
        let v = match transition_id {
            "closed" => {
                self.rest(HttpRequest::patch(&url).json(state_change("closed", "completed"))).await?.body
            }
            "open" => self.rest(HttpRequest::patch(&url).json(state_change("open", "reopened"))).await?.body,
            other => {
                let name = other
                    .strip_prefix("status:")
                    .ok_or_else(|| KeltaError::invalid(format!("unknown github transition id: {other}")))?;
                return self.move_in_project(&repo, number, name).await;
            }
        };
        self.ticket_from_rest(&v, None)
            .ok_or_else(|| KeltaError::upstream("issue response without repository"))
    }

    async fn comment(&self, t: &TicketRef, markdown: &str) -> Result<(), KeltaError> {
        let (repo, number) = split_repo_number(&t.key)?;
        self.auth
            .send_text(
                HttpRequest::post(format!("{}/repos/{repo}/issues/{number}/comments", self.api))
                    .json(json!({"body": markdown})),
            )
            .await?;
        Ok(())
    }

    async fn assign(&self, t: &TicketRef, who: Assignee) -> Result<Ticket, KeltaError> {
        let (repo, number) = split_repo_number(&t.key)?;
        let assignees: Vec<String> = match who {
            Assignee::Me => vec![self.me().await?.id],
            Assignee::User { id } => vec![id],
            Assignee::None => vec![],
        };
        let v = self
            .rest(
                HttpRequest::patch(format!("{}/repos/{repo}/issues/{number}", self.api))
                    .json(json!({"assignees": assignees})),
            )
            .await?
            .body;
        self.ticket_from_rest(&v, None)
            .ok_or_else(|| KeltaError::upstream("issue response without repository"))
    }

    fn browser_url(&self, t: &TicketRef) -> String {
        match split_repo_number(&t.key) {
            Ok((repo, n)) => format!(
                "{}/{}/issues/{n}",
                self.web,
                repo.split('/').map(percent_encode).collect::<Vec<_>>().join("/")
            ),
            Err(_) => self.web.clone(),
        }
    }

    fn branch_key(&self, t: &TicketRef) -> String {
        match split_repo_number(&t.key) {
            Ok((_, n)) => format!("gh-{n}"),
            Err(_) => t.key.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn endpoints_for_github_and_ghe() {
        assert_eq!(web_base("https://api.github.com"), "https://github.com");
        assert_eq!(graphql_url("https://api.github.com"), "https://api.github.com/graphql");
        assert_eq!(web_base("https://ghe.acme.example/api/v3"), "https://ghe.acme.example");
        assert_eq!(graphql_url("https://ghe.acme.example/api/v3"), "https://ghe.acme.example/api/graphql");
    }
}

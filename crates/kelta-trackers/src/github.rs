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
use kelta_proto::settings::{AccountConfig, ProjectV2Ref, TrackerBinding, TrackerView};
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

/// Rank (0 = highest) of a priority label: `P0`..`P9`, or `priority` + `critical|high|medium|low`
/// after any of `: / - _` or a space. A bare word like `high` is not a priority (too generic).
pub(crate) fn label_rank(label: &str) -> Option<u8> {
    let l = label.trim().to_ascii_lowercase();
    let (v, prefixed) = match l.strip_prefix("priority") {
        Some(r) => (r.trim_start_matches([':', '/', '-', '_', ' ']), true),
        None => (l.as_str(), false),
    };
    if let Some(n) = v.strip_prefix('p').and_then(|n| n.parse::<u8>().ok()) {
        return Some(n);
    }
    match (prefixed, v) {
        (true, "critical" | "urgent" | "blocker" | "highest") => Some(0),
        (true, "high") => Some(1),
        (true, "medium" | "normal") => Some(2),
        (true, "low") => Some(3),
        (true, "lowest") => Some(4),
        _ => None,
    }
}

/// Highest-priority rank among a ticket's labels.
pub(crate) fn labels_rank<'a>(labels: impl IntoIterator<Item = &'a String>) -> Option<u8> {
    labels.into_iter().filter_map(|l| label_rank(l)).min()
}

/// Today (UTC) as `YYYY-MM-DD`.
pub(crate) fn today() -> String {
    time::OffsetDateTime::now_utc().date().to_string()
}

/// `YYYY-MM-DD` plus `days`; `None` on a malformed date.
fn add_days(date: &str, days: i64) -> Option<String> {
    let mut p = date.get(..10)?.split('-');
    let (y, m, d) = (p.next()?.parse().ok()?, p.next()?.parse::<u8>().ok()?, p.next()?.parse().ok()?);
    let d = time::Date::from_calendar_date(y, time::Month::try_from(m).ok()?, d).ok()?;
    Some(d.checked_add(time::Duration::days(days))?.to_string())
}

/// Date part of an RFC 3339 timestamp (`due_on`, `due_date`).
pub(crate) fn date_of(ts: &str) -> Option<String> {
    Some(ts.get(..10)?.to_owned())
}

/// Iteration value of a Projects v2 item → sprint; active while `today` is inside the window.
fn iteration_sprint(v: &Value, today: &str) -> Option<Sprint> {
    let start = s(v, "startDate")?;
    let end = add_days(start, v.get("duration").and_then(Value::as_i64)? - 1)?;
    Some(Sprint {
        id: s(v, "iterationId").unwrap_or(start).to_owned(),
        name: s(v, "title")?.to_owned(),
        active: start <= today && today <= end.as_str(),
        ends_at: Some(end),
    })
}

/// Priority (option order), sprint and estimate from the field values of a Projects v2 item.
/// shortcut: first iteration field wins and number fields are matched by name (Estimate, Points, Story points).
fn apply_project_fields(t: &mut Ticket, vals: &[Value], today: &str) {
    for fv in vals {
        if let Some(opt) = s(fv, "name") {
            let field = fv.get("field");
            if field.and_then(|f| s(f, "name")).is_some_and(|n| n.eq_ignore_ascii_case("priority")) {
                t.priority = Some(opt.to_owned());
                t.priority_rank = field
                    .and_then(|f| f.get("options"))
                    .and_then(Value::as_array)
                    .and_then(|o| o.iter().position(|x| s(x, "name") == Some(opt)))
                    .and_then(|i| u8::try_from(i).ok())
                    .or(t.priority_rank);
            }
        } else if fv.get("startDate").is_some() {
            t.sprint = t.sprint.take().or_else(|| iteration_sprint(fv, today));
        } else if let Some(n) = fv.get("number").and_then(Value::as_f64) {
            let named = fv.pointer("/field/name").and_then(Value::as_str).is_some_and(|f| {
                ["estimate", "points", "story points"].iter().any(|w| f.eq_ignore_ascii_case(w))
            });
            if named && t.estimate.is_none() {
                t.estimate = Some(if n.fract() == 0.0 { format!("{}", n as i64) } else { n.to_string() });
            }
        }
    }
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
        let labels: Vec<String> = v
            .get("labels")
            .and_then(Value::as_array)
            .map(|a| {
                a.iter().filter_map(|l| l.as_str().or_else(|| s(l, "name")).map(str::to_owned)).collect()
            })
            .unwrap_or_default();
        let updated_at = s(v, "updated_at").unwrap_or("").to_owned();
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
            priority_rank: labels_rank(&labels),
            labels,
            priority: None,
            // A closed issue entered its status when it closed.
            status_since: Some(
                s(v, "closed_at")
                    .filter(|_| s(v, "state") == Some("closed"))
                    .map_or(updated_at.clone(), str::to_owned),
            ),
            updated_at,
            due: v.pointer("/milestone/due_on").and_then(Value::as_str).and_then(date_of),
            project_hint: Some(repo),
            ..Default::default()
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
        p: &ProjectV2Ref,
        after: Option<String>,
        filter: Option<String>,
    ) -> Result<Page<Ticket>, KeltaError> {
        const Q: &str = "query($owner:String!,$number:Int!,$after:String,$query:String){ repositoryOwner(login:$owner){ ... on ProjectV2Owner { projectV2(number:$number){ items(first:50, after:$after, query:$query, orderBy:{field:POSITION, direction:ASC}){ pageInfo{hasNextPage endCursor} nodes{ fieldValues(first:30){ nodes{ ... on ProjectV2ItemFieldSingleSelectValue { name field{ ... on ProjectV2SingleSelectField{ name options{ name } } } } ... on ProjectV2ItemFieldIterationValue { iterationId title startDate duration } ... on ProjectV2ItemFieldNumberValue { number field{ ... on ProjectV2Field{ name } } } } } content{ ... on Issue { id databaseId number title url state updatedAt closedAt milestone{ dueOn } repository{ nameWithOwner } assignees(first:1){ nodes{ login name avatarUrl } } labels(first:10){ nodes{ name } } issueType{ name } } } } } } } }";
        let data = self
            .gql(Q, json!({"owner": p.owner, "number": p.number, "after": after, "query": filter}))
            .await?;
        let items = data.pointer("/repositoryOwner/projectV2/items");
        let nodes = items.and_then(|i| i.get("nodes")).and_then(Value::as_array).cloned().unwrap_or_default();
        let today = today();
        let mut out = Vec::new();
        for n in &nodes {
            let Some(c) = n.get("content").filter(|c| c.get("number").is_some()) else { continue };
            let vals =
                n.pointer("/fieldValues/nodes").and_then(Value::as_array).map_or(&[][..], Vec::as_slice);
            let status = Some(vals)
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
            let labels: Vec<String> = c
                .pointer("/labels/nodes")
                .and_then(Value::as_array)
                .map(|a| a.iter().filter_map(|l| s(l, "name").map(str::to_owned)).collect())
                .unwrap_or_default();
            let updated_at = s(c, "updatedAt").unwrap_or("").to_owned();
            let closed = s(c, "state") == Some("CLOSED");
            let mut t = Ticket {
                r#ref: TicketRef {
                    account: self.account().clone(),
                    key: format!("{repo}#{number}"),
                    id: c.get("databaseId").and_then(idstr).unwrap_or_else(|| number.to_string()),
                },
                title: s(c, "title").unwrap_or("").to_owned(),
                url: s(c, "url").unwrap_or("").to_owned(),
                status: Self::status_for(!closed, status.as_deref()),
                kind: c.pointer("/issueType/name").and_then(Value::as_str).map(str::to_owned),
                assignee: c.pointer("/assignees/nodes/0").and_then(Self::user_from),
                priority_rank: labels_rank(&labels),
                labels,
                priority: None,
                status_since: Some(
                    s(c, "closedAt").filter(|_| closed).map_or(updated_at.clone(), str::to_owned),
                ),
                updated_at,
                due: c.pointer("/milestone/dueOn").and_then(Value::as_str).and_then(date_of),
                project_hint: Some(repo),
                ..Default::default()
            };
            apply_project_fields(&mut t, vals, &today);
            out.push(t);
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
            // Project filter syntax (not issue search): `assignee:@me`, `no:assignee`, `iteration:@current`.
            // shortcut: assumes the iteration field is named "Iteration", read the field name from ProjectV2.fields when a board differs.
            let filter = [
                match state_param(view) {
                    "open" => Some("is:open"),
                    "closed" => Some("is:closed"),
                    _ => None,
                },
                match view.who {
                    Some(Who::Mine) => Some("assignee:@me"),
                    Some(Who::Unassigned) => Some("no:assignee"),
                    _ => None,
                },
                view.current_iteration.then_some("iteration:@current"),
            ]
            .into_iter()
            .flatten()
            .collect::<Vec<_>>()
            .join(" ");
            return self.list_project_items(p, after, Some(filter).filter(|f| !f.is_empty())).await;
        }
        let page = Self::page_cursor(cursor)?;
        let state = state_param(view);
        let mut req = if let Some(q) = view.search.as_deref().filter(|q| !q.trim().is_empty()) {
            let mut q = if q.contains("is:issue") || q.contains("is:pr") {
                q.to_owned()
            } else {
                format!("{q} is:issue")
            };
            match view.who {
                Some(Who::Mine) => q.push_str(" assignee:@me"),
                Some(Who::Unassigned) => q.push_str(" no:assignee"),
                _ => {}
            }
            HttpRequest::get(format!("{}/search/issues", self.api))
                .query("q", q)
                .query("sort", "updated")
                .query("order", "desc")
        } else if let Some(repo) = view.repo.as_deref().filter(|r| !r.is_empty()) {
            let mut r = HttpRequest::get(format!("{}/repos/{repo}/issues", self.api))
                .query("state", state)
                .query("sort", "updated")
                .query("direction", "desc");
            let assignee = match view.who {
                Some(Who::Mine) => Some(self.me().await?.id),
                Some(Who::Unassigned) => Some("none".to_owned()),
                // `assignee=*` would mean "assigned to anyone", so Anyone sends nothing.
                Some(Who::Anyone) => None,
                None if view.assigned_to.as_deref() == Some("me") => Some(self.me().await?.id),
                None => None,
            };
            if let Some(a) = assignee {
                r = r.query("assignee", a);
            }
            r
        } else if matches!(view.who, Some(Who::Unassigned | Who::Anyone)) {
            return Err(KeltaError::invalid(
                "github needs a repository, search or project to list unassigned or all tickets",
            ));
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

    async fn sources(&self, query: &str) -> Result<Vec<SourceHit>, KeltaError> {
        let q = query.trim().to_ascii_lowercase();
        // Repos are filtered client-side: a typed query pages on so older repos are found too.
        // shortcut: at most 10 pages (1000 repos), raise when an account has more.
        let mut repos: Vec<Value> = Vec::new();
        for page in 1..=10 {
            let r = self
                .rest(
                    HttpRequest::get(format!("{}/user/repos", self.api))
                        .query("affiliation", "owner,collaborator,organization_member")
                        .query("sort", "pushed")
                        .query("per_page", "100")
                        .query("page", page.to_string()),
                )
                .await?;
            repos.extend(r.body.as_array().cloned().unwrap_or_default());
            if q.is_empty() || link_rel(&r.headers, "next").is_none() {
                break;
            }
        }
        let mut hits: Vec<SourceHit> = repos
            .iter()
            .filter_map(|r| {
                let name = s(r, "full_name")?;
                name.to_ascii_lowercase().contains(&q).then(|| SourceHit {
                    kind: "repo".into(),
                    label: name.to_owned(),
                    detail: s(r, "description").filter(|d| !d.is_empty()).map(str::to_owned),
                    view: TrackerView {
                        id: format!("github:repo:{name}"),
                        label: name.to_owned(),
                        repo: Some(name.to_owned()),
                        who: Some(Who::Mine),
                        ..TrackerView::default()
                    },
                })
            })
            .collect();
        // The viewer's own projects, then their organisations' (the usual team board).
        // shortcut: 20 orgs x 20 projects, raise when a user sits in more.
        const P: &str =
            "nodes{ number title closed owner{ ... on Organization{ login } ... on User{ login } } }";
        let user_q = format!("query($q:String){{ viewer{{ projectsV2(first:50, query:$q){{ {P} }} }} }}");
        let org_q = format!(
            "query($q:String){{ viewer{{ organizations(first:20){{ nodes{{ projectsV2(first:20, query:$q){{ {P} }} }} }} }} }}"
        );
        let vars = json!({"q": Some(query.trim()).filter(|q| !q.is_empty())});
        let mut nodes: Vec<Value> = Vec::new();
        // Projects v2 need extra token scopes (read:org for orgs): a failure there must not hide the repos.
        if let Ok(d) = self.gql(&user_q, vars.clone()).await {
            nodes.extend(
                d.pointer("/viewer/projectsV2/nodes").and_then(Value::as_array).cloned().unwrap_or_default(),
            );
        }
        if let Ok(d) = self.gql(&org_q, vars).await {
            for o in d.pointer("/viewer/organizations/nodes").and_then(Value::as_array).into_iter().flatten()
            {
                nodes.extend(
                    o.pointer("/projectsV2/nodes").and_then(Value::as_array).cloned().unwrap_or_default(),
                );
            }
        }
        for n in nodes.iter().filter(|n| n.get("closed").and_then(Value::as_bool) != Some(true)) {
            let (Some(owner), Some(number), Some(title)) = (
                n.pointer("/owner/login").and_then(Value::as_str),
                n.get("number").and_then(Value::as_u64),
                s(n, "title"),
            ) else {
                continue;
            };
            hits.push(SourceHit {
                kind: "project_v2".into(),
                label: title.to_owned(),
                detail: Some(format!("{owner} #{number}")),
                view: TrackerView {
                    id: format!("github:project:{owner}/{number}"),
                    label: title.to_owned(),
                    project_v2: Some(ProjectV2Ref {
                        owner: owner.to_owned(),
                        number: number as u32,
                        status_field: DEFAULT_STATUS_FIELD.into(),
                    }),
                    who: Some(Who::Mine),
                    ..TrackerView::default()
                },
            });
        }
        Ok(hits)
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

    #[test]
    fn priority_labels_rank_from_zero() {
        let rank = |l: &str| label_rank(l);
        assert_eq!(
            ["P0", "p1", "priority:high", "Priority/Critical", "priority: medium", "priority-low"].map(rank),
            [Some(0), Some(1), Some(1), Some(0), Some(2), Some(3)]
        );
        assert_eq!(["high", "bug", "pr", "priority:whenever"].map(rank), [None; 4]);
        assert_eq!(labels_rank(&["P3".to_owned(), "priority:high".to_owned()]), Some(1));
    }

    #[test]
    fn iteration_is_active_through_its_last_day() {
        let it = json!({"iterationId": "i", "title": "S1", "startDate": "2026-10-01", "duration": 14});
        let at = |today| iteration_sprint(&it, today).unwrap();
        assert_eq!(at("2026-10-01").ends_at.as_deref(), Some("2026-10-14"));
        assert!(!at("2026-09-30").active && at("2026-10-01").active && at("2026-10-14").active);
        assert!(!at("2026-10-15").active);
        assert_eq!(add_days("2026-12-30", 3).as_deref(), Some("2027-01-02"));
        assert!(
            iteration_sprint(&json!({"title": "x", "startDate": "oops", "duration": 3}), "2026-10-01")
                .is_none()
        );
    }
}

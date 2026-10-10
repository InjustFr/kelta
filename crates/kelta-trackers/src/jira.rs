//! Jira Cloud (REST v3, `POST /search/jql`, ADF) and Data Center / Server (REST v2, `startAt`).
//!
//! Flavor: `accounts.<id>.flavor` (`cloud` | `dc`) or, for `auto`, `GET /rest/api/2/serverInfo`
//! (`deploymentType`), cached for the lifetime of the provider (providers are recreated when the
//! account settings change).

use std::sync::Arc;

use async_trait::async_trait;
use kelta_http::util::percent_encode;
use kelta_http::{AuthScheme, Authed, HttpCtx, HttpRequest, markdown};
use kelta_proto::api::{SecretResolver, Tracker};
use kelta_proto::error::{ErrorCode, KeltaError};
use kelta_proto::ids::AccountId;
use kelta_proto::settings::{AccountConfig, JiraFlavor, TrackerBinding, TrackerView};
use kelta_proto::tracker::{
    Assignee, BodyFormat, Column, Comment, Cursor, Page, SourceHit, Sprint, Status, StatusCategory, Ticket,
    TicketDetail, TicketRef, TrackerCaps, TrackerKind, Transition, User, Who,
};
use parking_lot::Mutex;
use serde_json::{Value, json};
use tokio::sync::OnceCell;

use crate::adf;
use crate::common::{
    self, COMMENT_LIMIT, category_from_name, columns_from_binding, columns_from_statuses, idstr, s,
};

/// Cloud `search/jql` is cut after this many pages even if the server keeps returning tokens.
pub const MAX_CLOUD_PAGES: u32 = 20;
const PAGE_SIZE: u32 = 50;
const LIST_FIELDS: &[&str] = &[
    "summary",
    "status",
    "issuetype",
    "assignee",
    "labels",
    "priority",
    "updated",
    "project",
    "parent",
    "statuscategorychangedate",
    "duedate",
    "timeoriginalestimate",
];

/// Instance-specific ids, discovered once: the sprint and story-point custom fields (`GET /field`) and
/// the priority ids in rank order, highest first (`GET /priority`).
#[derive(Default)]
struct Lookups {
    sprint: Option<String>,
    points: Vec<String>,
    priorities: Vec<String>,
}

impl Lookups {
    /// `LIST_FIELDS` plus the discovered custom fields.
    fn fields(&self) -> Vec<&str> {
        let mut f = LIST_FIELDS.to_vec();
        f.extend(self.sprint.as_deref().into_iter().chain(self.points.iter().map(String::as_str)));
        f
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Flavor {
    Cloud,
    Dc,
}

pub struct JiraTracker {
    base: String,
    web: String,
    configured: JiraFlavor,
    auth: Authed,
    explicit: Option<AuthScheme>,
    basic_user: Option<String>,
    flavor: OnceCell<Flavor>,
    lookups: OnceCell<Lookups>,
    me: Mutex<Option<User>>,
}

/// Flavor-specific request context.
struct Api {
    flavor: Flavor,
    auth: Authed,
    root: String,
}

impl Api {
    fn url(&self, path: &str) -> String {
        format!("{}{path}", self.root)
    }
    async fn json(&self, req: HttpRequest) -> Result<Value, KeltaError> {
        Ok(self.auth.send_json::<Value>(req.header("Accept", "application/json")).await?.body)
    }
}

impl JiraTracker {
    pub fn new(
        account: &AccountConfig,
        http: HttpCtx,
        secrets: Arc<dyn SecretResolver>,
    ) -> Result<Self, KeltaError> {
        let base = common::base_url(account)?;
        let web = account.web_url.as_deref().map(kelta_http::util::trim_url).unwrap_or_else(|| base.clone());
        let auth = Authed::new(http, secrets, account.effective_secret(), Some(&base), AuthScheme::Bearer);
        Ok(Self {
            web,
            auth,
            explicit: AuthScheme::from_account(account),
            basic_user: account.email.clone().or_else(|| account.user.clone()),
            configured: account.flavor,
            base,
            flavor: OnceCell::new(),
            lookups: OnceCell::new(),
            me: Mutex::new(None),
        })
    }

    /// Resolved flavor (detects once when `flavor = auto`).
    pub async fn flavor(&self) -> Result<Flavor, KeltaError> {
        self.flavor
            .get_or_try_init(|| async {
                match self.configured {
                    JiraFlavor::Cloud => Ok(Flavor::Cloud),
                    JiraFlavor::Dc => Ok(Flavor::Dc),
                    JiraFlavor::Auto => self.detect().await,
                }
            })
            .await
            .copied()
    }

    async fn detect(&self) -> Result<Flavor, KeltaError> {
        let req = HttpRequest::get(format!("{}/rest/api/2/serverInfo", self.base))
            .header("Accept", "application/json");
        match self.auth.http().send_json::<Value>(req).await {
            Ok(r) => Ok(match s(&r.body, "deploymentType") {
                Some(t) if t.eq_ignore_ascii_case("cloud") => Flavor::Cloud,
                Some(_) => Flavor::Dc,
                None => self.guess_from_host(),
            }),
            // Offline stays an error (and is not cached); anything else falls back to the host name.
            Err(e) if matches!(e.code, ErrorCode::Network | ErrorCode::Timeout | ErrorCode::RateLimited) => {
                Err(e)
            }
            Err(_) => Ok(self.guess_from_host()),
        }
    }

    fn guess_from_host(&self) -> Flavor {
        match kelta_http::util::url_host(&self.base) {
            Some(h) if h.ends_with(".atlassian.net") || h.ends_with(".jira.com") => Flavor::Cloud,
            _ => Flavor::Dc,
        }
    }

    async fn api(&self) -> Result<Api, KeltaError> {
        let flavor = self.flavor().await?;
        let scheme = match (&self.explicit, flavor) {
            (Some(s), _) => s.clone(),
            (None, Flavor::Cloud) => AuthScheme::Basic {
                user: self.basic_user.clone().ok_or_else(|| {
                    KeltaError::needs_auth("Jira Cloud accounts need `email` (basic auth: email + API token)")
                })?,
            },
            (None, Flavor::Dc) => AuthScheme::Bearer,
        };
        let v = if flavor == Flavor::Cloud { 3 } else { 2 };
        Ok(Api {
            flavor,
            auth: self.auth.clone().with_scheme(scheme),
            root: format!("{}/rest/api/{v}", self.base),
        })
    }

    fn user_from(v: &Value) -> Option<User> {
        if !v.is_object() {
            return None;
        }
        let id = s(v, "accountId").or_else(|| s(v, "name")).or_else(|| s(v, "key"))?;
        let name = s(v, "displayName").or_else(|| s(v, "name")).unwrap_or(id);
        let avatar =
            v.get("avatarUrls").and_then(|a| a.get("48x48")).and_then(Value::as_str).map(str::to_owned);
        Some(common::user_from(
            id,
            name,
            s(v, "emailAddress").or_else(|| s(v, "name")).map(str::to_owned),
            avatar,
        ))
    }

    fn status_from(v: &Value) -> Status {
        let name = s(v, "name").unwrap_or("").to_owned();
        let category = match v.get("statusCategory").and_then(|c| s(c, "key")) {
            Some("new") => StatusCategory::Todo,
            Some("indeterminate") => {
                if category_from_name(&name) == StatusCategory::InReview {
                    StatusCategory::InReview
                } else {
                    StatusCategory::InProgress
                }
            }
            Some("done") => StatusCategory::Done,
            _ => category_from_name(&name),
        };
        Status { id: v.get("id").and_then(idstr).unwrap_or_default(), name, category }
    }

    /// Custom field ids and priority ranks; only an unreachable server is an error, a refusal just means "none".
    async fn lookups(&self, api: &Api) -> Result<&Lookups, KeltaError> {
        self.lookups
            .get_or_try_init(|| async {
                // Only a refusal means "none"; anything else (5xx, 401, network) is retried on the next call.
                let soft = |r: Result<Value, KeltaError>| match r {
                    Err(e) if matches!(e.code, ErrorCode::PermissionDenied | ErrorCode::NotFound) => {
                        Ok(Value::Null)
                    }
                    r => r,
                };
                let fields = soft(api.json(HttpRequest::get(api.url("/field"))).await)?;
                let prios = soft(api.json(HttpRequest::get(api.url("/priority"))).await)?;
                let by = |pred: &dyn Fn(&Value) -> bool| -> Vec<String> {
                    let all = fields.as_array().map(Vec::as_slice).unwrap_or_default();
                    all.iter().filter(|f| pred(f)).filter_map(|f| s(f, "id")).map(str::to_owned).collect()
                };
                Ok(Lookups {
                    sprint: by(&|f| {
                        f.pointer("/schema/custom").and_then(Value::as_str)
                            == Some("com.pyxis.greenhopper.jira:gh-sprint")
                    })
                    .into_iter()
                    .next(),
                    points: by(&|f| {
                        matches!(s(f, "name"), Some("Story Points" | "Story point estimate"))
                            && f.pointer("/schema/type").and_then(Value::as_str) == Some("number")
                    }),
                    priorities: prios
                        .as_array()
                        .map(|a| a.iter().filter_map(|p| p.get("id").and_then(idstr)).collect())
                        .unwrap_or_default(),
                })
            })
            .await
    }

    fn ticket_from(&self, account: &AccountId, v: &Value, lk: &Lookups) -> Result<Ticket, KeltaError> {
        let key = s(v, "key").ok_or_else(|| KeltaError::upstream("jira issue without key"))?;
        let f = v.get("fields").unwrap_or(&Value::Null);
        Ok(Ticket {
            r#ref: TicketRef {
                account: account.clone(),
                key: key.to_owned(),
                id: v.get("id").and_then(idstr).unwrap_or_else(|| key.to_owned()),
            },
            title: s(f, "summary").unwrap_or("").to_owned(),
            url: format!("{}/browse/{key}", self.web),
            status: f.get("status").map(Self::status_from).unwrap_or_default(),
            kind: f.get("issuetype").and_then(|t| s(t, "name")).map(str::to_owned),
            assignee: f.get("assignee").and_then(Self::user_from),
            labels: f
                .get("labels")
                .and_then(Value::as_array)
                .map(|a| a.iter().filter_map(|l| l.as_str().map(str::to_owned)).collect())
                .unwrap_or_default(),
            priority: f.get("priority").and_then(|p| s(p, "name")).map(str::to_owned),
            updated_at: normalize_time(s(f, "updated").unwrap_or("")),
            priority_rank: f
                .pointer("/priority/id")
                .and_then(idstr)
                .and_then(|id| lk.priorities.iter().position(|p| *p == id))
                .and_then(|i| u8::try_from(i).ok()),
            status_since: s(f, "statuscategorychangedate").map(normalize_time),
            sprint: lk.sprint.as_deref().and_then(|id| f.get(id)).and_then(sprint_from),
            estimate: lk
                .points
                .iter()
                .find_map(|id| f.get(id)?.as_f64())
                .map(|p| format!("{p} pts"))
                .or_else(|| f.get("timeoriginalestimate")?.as_u64().map(duration)),
            due: s(f, "duedate").map(str::to_owned),
            project_hint: f.get("project").and_then(|p| s(p, "key")).map(str::to_owned),
        })
    }

    fn account(&self) -> &AccountId {
        self.auth.http().account_id()
    }

    async fn fetch_ticket(&self, api: &Api, key: &str) -> Result<Ticket, KeltaError> {
        let lk = self.lookups(api).await?;
        let v = api
            .json(
                HttpRequest::get(api.url(&format!("/issue/{}", percent_encode(key))))
                    .query("fields", lk.fields().join(",")),
            )
            .await?;
        self.ticket_from(self.account(), &v, lk)
    }

    /// Cloud cursor: `"<pages fetched>:<nextPageToken>"` so the 20-page cap survives across calls.
    fn decode_cloud_cursor(c: Option<Cursor>) -> Result<(u32, Option<String>), KeltaError> {
        match c {
            None => Ok((0, None)),
            Some(Cursor::Token(t)) => {
                let (n, tok) = t.split_once(':').ok_or_else(|| KeltaError::invalid("bad jira cursor"))?;
                Ok((n.parse().map_err(|_| KeltaError::invalid("bad jira cursor"))?, Some(tok.to_owned())))
            }
            Some(_) => Err(KeltaError::invalid("jira cloud expects a token cursor")),
        }
    }

    /// Comments of an issue, oldest first, last [`COMMENT_LIMIT`].
    fn comments_from(&self, flavor: Flavor, issue: &Value) -> Vec<Comment> {
        let list =
            issue.pointer("/fields/comment/comments").and_then(Value::as_array).cloned().unwrap_or_default();
        let rendered = issue
            .pointer("/renderedFields/comment/comments")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        let all: Vec<Comment> = list
            .iter()
            .enumerate()
            .map(|(i, c)| {
                let body = c.get("body").unwrap_or(&Value::Null);
                let html = match flavor {
                    Flavor::Cloud => markdown::to_html(&adf::adf_to_markdown(body)),
                    Flavor::Dc => match rendered.get(i).and_then(|r| s(r, "body")) {
                        Some(h) => markdown::sanitize(h),
                        None => markdown::plain_to_html(body.as_str().unwrap_or("")),
                    },
                };
                common::comment(
                    c.get("author").and_then(Self::user_from).unwrap_or_default(),
                    normalize_time(s(c, "created").unwrap_or("")),
                    html,
                )
            })
            .collect();
        common::last_n(all, COMMENT_LIMIT)
    }

    /// Map a failed transition POST: `400 {errors: {field: msg}}` becomes `NeedsFields`.
    async fn needs_fields(&self, api: &Api, key: &str, transition_id: &str, err: KeltaError) -> KeltaError {
        if err.code != ErrorCode::InvalidArgument {
            return err;
        }
        let body: Value = err
            .detail
            .as_ref()
            .and_then(|d| d.get("body"))
            .and_then(Value::as_str)
            .and_then(|b| serde_json::from_str(b).ok())
            .unwrap_or(Value::Null);
        let errors = body.get("errors").and_then(Value::as_object).cloned().unwrap_or_default();
        if errors.is_empty() {
            return err;
        }
        // Best effort: enrich with field names and allowed values from the transition metadata.
        let meta = api
            .json(
                HttpRequest::get(api.url(&format!("/issue/{}/transitions", percent_encode(key))))
                    .query("expand", "transitions.fields"),
            )
            .await
            .ok();
        let tfields = meta
            .as_ref()
            .and_then(|m| m.get("transitions"))
            .and_then(Value::as_array)
            .and_then(|ts| ts.iter().find(|t| t.get("id").and_then(idstr).as_deref() == Some(transition_id)))
            .and_then(|t| t.get("fields").cloned())
            .unwrap_or(Value::Null);
        let fields: Vec<Value> = errors
            .iter()
            .map(|(id, msg)| {
                let m = tfields.get(id).unwrap_or(&Value::Null);
                json!({
                    "id": id,
                    "message": msg,
                    "name": s(m, "name").unwrap_or(id),
                    "required": m.get("required").and_then(Value::as_bool).unwrap_or(true),
                    "type": m.pointer("/schema/type"),
                    "allowed_values": m.get("allowedValues").cloned().unwrap_or(Value::Null),
                })
            })
            .collect();
        let messages: Vec<String> =
            errors.iter().map(|(k, v)| format!("{k}: {}", v.as_str().unwrap_or(""))).collect();
        KeltaError::new(ErrorCode::NeedsFields, format!("transition needs fields ({})", messages.join("; ")))
            .with_detail(json!({
                "fields": fields,
                "error_messages": body.get("errorMessages").cloned().unwrap_or(Value::Null),
            }))
    }
}

/// Jira writes `2024-05-01T10:00:00.000+0200`; the wire contract is RFC 3339 (`+02:00`).
pub(crate) fn normalize_time(t: &str) -> String {
    let b = t.as_bytes();
    if b.len() > 5 {
        let tail = &b[b.len() - 5..];
        if (tail[0] == b'+' || tail[0] == b'-')
            && tail[1..].iter().all(u8::is_ascii_digit)
            && b[b.len() - 6].is_ascii_digit()
        {
            return format!("{}:{}", &t[..t.len() - 2], &t[t.len() - 2..]);
        }
    }
    t.to_owned()
}

/// `2h`, `2h 30m`, `45m`.
fn duration(secs: u64) -> String {
    let (h, m) = (secs / 3600, secs % 3600 / 60);
    match (h, m) {
        (0, m) => format!("{m}m"),
        (h, 0) => format!("{h}h"),
        (h, m) => format!("{h}h {m}m"),
    }
}

/// The sprint field is a list: the active sprint, else the latest. Cloud and recent DC send objects,
/// older DC strings like `...Sprint@1f[id=12,state=ACTIVE,name=Sprint 12,endDate=2026-10-20T10:00:00.000Z,...]`.
/// shortcut: a sprint name containing a comma is cut in the string form, drop when old DC is unsupported.
fn sprint_from(v: &Value) -> Option<Sprint> {
    let all: Vec<Sprint> = v.as_array()?.iter().filter_map(one_sprint).collect();
    let i = all.iter().position(|s| s.active).unwrap_or(all.len().saturating_sub(1));
    all.into_iter().nth(i)
}

fn one_sprint(v: &Value) -> Option<Sprint> {
    let (id, name, state, end) = match v.as_str() {
        None => (v.get("id").and_then(idstr)?, s(v, "name")?, s(v, "state")?, s(v, "endDate")),
        Some(str_form) => {
            let kv = |k: &str| {
                let rest = str_form.split_once('[')?.1;
                rest.split(',').find_map(|p| p.strip_prefix(k)?.strip_prefix('='))
            };
            (kv("id")?.to_owned(), kv("name")?, kv("state")?, kv("endDate").filter(|d| *d != "<null>"))
        }
    };
    Some(Sprint {
        id,
        name: name.to_owned(),
        active: state.eq_ignore_ascii_case("active"),
        ends_at: end.map(normalize_time),
    })
}

#[async_trait]
impl Tracker for JiraTracker {
    fn kind(&self) -> TrackerKind {
        TrackerKind::Jira
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
        let api = self.api().await?;
        let v = api.json(HttpRequest::get(api.url("/myself"))).await?;
        let u = Self::user_from(&v).ok_or_else(|| KeltaError::upstream("jira /myself returned no user"))?;
        *self.me.lock() = Some(u.clone());
        Ok(u)
    }

    async fn list(&self, view: &TrackerView, cursor: Option<Cursor>) -> Result<Page<Ticket>, KeltaError> {
        let api = self.api().await?;
        let lk = self.lookups(&api).await?;
        let jql = compose_jql(&self.base_jql(view).await?, view.who, view.current_iteration);
        if split_order_by(&jql).0.is_empty() {
            return Err(KeltaError::invalid(format!(
                "jira view `{}` needs a jql, project or board",
                view.id
            )));
        }
        let jql = jql.as_str();
        match api.flavor {
            Flavor::Cloud => {
                let (done, token) = Self::decode_cloud_cursor(cursor)?;
                // The first request never carries `nextPageToken` (not even null).
                let mut body = json!({"jql": jql, "fields": lk.fields(), "maxResults": PAGE_SIZE});
                if let Some(t) = &token {
                    body["nextPageToken"] = json!(t);
                }
                let v = api.json(HttpRequest::post(api.url("/search/jql")).json(body)).await?;
                let items = self.tickets_from(&v, lk)?;
                let pages = done + 1;
                let next = s(&v, "nextPageToken")
                    .filter(|t| !t.is_empty())
                    .filter(|_| {
                        !items.is_empty()
                            && pages < MAX_CLOUD_PAGES
                            && v.get("isLast").and_then(Value::as_bool) != Some(true)
                    })
                    .map(|t| Cursor::Token(format!("{pages}:{t}")));
                Ok(Page { items, next })
            }
            Flavor::Dc => {
                let start = match cursor {
                    None => 0,
                    Some(Cursor::Offset(o)) => o,
                    Some(_) => return Err(KeltaError::invalid("jira dc expects an offset cursor")),
                };
                let body =
                    json!({"jql": jql, "fields": lk.fields(), "startAt": start, "maxResults": PAGE_SIZE});
                let v = api.json(HttpRequest::post(api.url("/search")).json(body)).await?;
                let items = self.tickets_from(&v, lk)?;
                let total = v.get("total").and_then(Value::as_u64).unwrap_or(0);
                let end = start as u64 + items.len() as u64;
                let next = (!items.is_empty() && end < total).then_some(Cursor::Offset(end as u32));
                Ok(Page { items, next })
            }
        }
    }

    async fn get(&self, t: &TicketRef) -> Result<TicketDetail, KeltaError> {
        let api = self.api().await?;
        let lk = self.lookups(&api).await?;
        let mut fields = lk.fields();
        fields.extend(["description", "comment"]);
        let v = api
            .json(
                HttpRequest::get(api.url(&format!("/issue/{}", percent_encode(&t.key))))
                    .query("fields", fields.join(","))
                    .query("expand", "renderedFields"),
            )
            .await?;
        let ticket = self.ticket_from(&t.account, &v, lk)?;
        let desc = v.pointer("/fields/description").unwrap_or(&Value::Null);
        let (body_md, body_html, body_format) = match api.flavor {
            Flavor::Cloud => {
                let md = adf::adf_to_markdown(desc);
                let html = markdown::to_html(&md);
                (md, html, BodyFormat::Adf)
            }
            Flavor::Dc => {
                let raw = desc.as_str().unwrap_or("").to_owned();
                let html = match v.pointer("/renderedFields/description").and_then(Value::as_str) {
                    Some(h) => markdown::sanitize(h),
                    None => markdown::plain_to_html(&raw),
                };
                (raw, html, BodyFormat::JiraWiki)
            }
        };
        let parent = v.pointer("/fields/parent").and_then(|p| {
            let key = s(p, "key")?;
            Some(TicketRef {
                account: t.account.clone(),
                key: key.to_owned(),
                id: p.get("id").and_then(idstr).unwrap_or_else(|| key.to_owned()),
            })
        });
        Ok(TicketDetail {
            ticket,
            body_md,
            body_html,
            body_format,
            comments: self.comments_from(api.flavor, &v),
            parent,
            prs: Vec::new(),
            caps: Default::default(),
        })
    }

    async fn columns(&self, b: &TrackerBinding) -> Result<Vec<Column>, KeltaError> {
        if let Some(c) = columns_from_binding(b) {
            return Ok(c);
        }
        let api = self.api().await?;
        let statuses = self.statuses(&api, b).await?;
        if let Some(board) = b.views.iter().find_map(|v| v.board_id) {
            let conf = api
                .json(HttpRequest::get(format!("{}/rest/agile/1.0/board/{board}/configuration", self.base)))
                .await?;
            let cols =
                conf.pointer("/columnConfig/columns").and_then(Value::as_array).cloned().unwrap_or_default();
            let mut out = Vec::new();
            for c in &cols {
                let name = s(c, "name").unwrap_or("").to_owned();
                let ids: Vec<String> = c
                    .get("statuses")
                    .and_then(Value::as_array)
                    .map(|a| a.iter().filter_map(|x| x.get("id").and_then(idstr)).collect())
                    .unwrap_or_default();
                let members: Vec<&Status> = statuses.iter().filter(|st| ids.contains(&st.id)).collect();
                // The board column takes the category of its first known status, else its own name.
                let category =
                    members.first().map(|m| m.category).unwrap_or_else(|| category_from_name(&name));
                out.push(Column {
                    id: format!("{}", out.len()),
                    category,
                    order: out.len() as u32,
                    match_names: members.iter().map(|m| m.name.clone()).collect(),
                    name,
                });
            }
            // The leading "backlog" pseudo column has no statuses; keep it (empty match_names).
            return Ok(out);
        }
        Ok(columns_from_statuses(&statuses))
    }

    async fn transitions(&self, t: &TicketRef) -> Result<Vec<Transition>, KeltaError> {
        let api = self.api().await?;
        let v = api
            .json(
                HttpRequest::get(api.url(&format!("/issue/{}/transitions", percent_encode(&t.key))))
                    .query("expand", "transitions.fields"),
            )
            .await?;
        Ok(v.get("transitions")
            .and_then(Value::as_array)
            .map(|a| {
                a.iter()
                    .filter_map(|tr| {
                        let id = tr.get("id").and_then(idstr)?;
                        let needs = tr.get("fields").and_then(Value::as_object).is_some_and(|f| {
                            f.values().any(|fv| {
                                fv.get("required").and_then(Value::as_bool) == Some(true)
                                    && fv.get("hasDefaultValue").and_then(Value::as_bool) != Some(true)
                            })
                        });
                        Some(Transition {
                            id,
                            name: s(tr, "name").unwrap_or("").to_owned(),
                            to: tr.get("to").map(Self::status_from).unwrap_or_default(),
                            needs_fields: needs,
                        })
                    })
                    .collect()
            })
            .unwrap_or_default())
    }

    async fn transition(
        &self,
        t: &TicketRef,
        transition_id: &str,
        fields: Option<Value>,
    ) -> Result<Ticket, KeltaError> {
        let api = self.api().await?;
        let mut body = json!({"transition": {"id": transition_id}});
        match fields {
            Some(Value::Object(m)) if m.contains_key("fields") || m.contains_key("update") => {
                for (k, v) in m {
                    body[k] = v;
                }
            }
            Some(f) if !f.is_null() => body["fields"] = f,
            _ => {}
        }
        let url = api.url(&format!("/issue/{}/transitions", percent_encode(&t.key)));
        if let Err(e) = api.auth.send_text(HttpRequest::post(url).json(body)).await {
            return Err(self.needs_fields(&api, &t.key, transition_id, e).await);
        }
        self.fetch_ticket(&api, &t.key).await
    }

    async fn comment(&self, t: &TicketRef, markdown: &str) -> Result<(), KeltaError> {
        let api = self.api().await?;
        let body = match api.flavor {
            Flavor::Cloud => json!({"body": adf::markdown_to_adf(markdown)}),
            Flavor::Dc => json!({"body": markdown}),
        };
        api.auth
            .send_text(
                HttpRequest::post(api.url(&format!("/issue/{}/comment", percent_encode(&t.key)))).json(body),
            )
            .await?;
        Ok(())
    }

    async fn assign(&self, t: &TicketRef, who: Assignee) -> Result<Ticket, KeltaError> {
        let api = self.api().await?;
        let target: Option<String> = match who {
            Assignee::Me => Some(self.me().await?.id),
            Assignee::User { id } => Some(id),
            Assignee::None => None,
        };
        // Cloud assigns by accountId, Data Center by user name.
        let field = if api.flavor == Flavor::Cloud { "accountId" } else { "name" };
        let body = json!({ field: target });
        api.auth
            .send_text(
                HttpRequest::put(api.url(&format!("/issue/{}/assignee", percent_encode(&t.key)))).json(body),
            )
            .await?;
        self.fetch_ticket(&api, &t.key).await
    }

    async fn search(&self, view: &TrackerView, text: &str) -> Result<Vec<Ticket>, KeltaError> {
        let jql = compose_jql(&self.base_jql(view).await?, view.who, view.current_iteration);
        let (cond, order) = split_order_by(&jql);
        // Only the JQL string is escaped: the text itself is Lucene syntax, Jira's to read.
        let text = format!("text ~ \"{}\"", text.replace('\\', "\\\\").replace('"', "\\\""));
        let jql =
            if cond.is_empty() { format!("{text} {order}") } else { format!("({cond}) AND {text} {order}") };
        let view = TrackerView {
            jql: Some(jql.trim_end().to_owned()),
            who: None,
            current_iteration: false,
            status: None,
            ..view.clone()
        };
        Ok(self.list(&view, None).await?.items)
    }

    async fn sources(&self, query: &str) -> Result<Vec<SourceHit>, KeltaError> {
        let api = self.api().await?;
        let acct = self.account().to_string();
        let q = query.trim().to_lowercase();
        let matches = |name: &str, extra: &str| {
            q.is_empty() || name.to_lowercase().contains(&q) || extra.to_lowercase().contains(&q)
        };
        let hit = |kind: &str, slug: String, label: String, detail: Option<String>, mut view: TrackerView| {
            view.id = format!("{acct}-{slug}");
            view.label = label.clone();
            view.who = Some(Who::Mine);
            SourceHit { kind: kind.to_owned(), label, detail, view }
        };
        let mut out = Vec::new();

        // Cloud filters server-side; Data Center lists every project the user can browse.
        let projects = match api.flavor {
            Flavor::Cloud => {
                let v = api
                    .json(
                        HttpRequest::get(api.url("/project/search"))
                            .query("query", q.clone())
                            .query("maxResults", "50"),
                    )
                    .await?;
                v.get("values").cloned().unwrap_or(Value::Null)
            }
            Flavor::Dc => api.json(HttpRequest::get(api.url("/project"))).await?,
        };
        for p in projects.as_array().into_iter().flatten() {
            let (Some(key), name) = (s(p, "key"), s(p, "name").unwrap_or("")) else { continue };
            if matches(name, key) {
                let view = TrackerView {
                    jql: Some(format!("project = {key} AND {OPEN} ORDER BY updated DESC")),
                    ..TrackerView::default()
                };
                out.push(hit(
                    "project",
                    format!("project-{key}"),
                    name.to_owned(),
                    Some(key.to_owned()),
                    view,
                ));
            }
        }

        // Boards and filters need a Jira Software seat / the permission: a failure only hides them.
        // shortcut: first 50 boards, no paging, upgrade when a site has more.
        let agile = format!("{}/rest/agile/1.0/board", self.base);
        let boards =
            api.json(HttpRequest::get(agile).query("name", q.clone()).query("maxResults", "50")).await.ok();
        for b in boards.iter().filter_map(|b| b.get("values")).filter_map(Value::as_array).flatten() {
            let (Some(id), Some(name)) = (b.get("id").and_then(Value::as_u64), s(b, "name")) else {
                continue;
            };
            let scrum = s(b, "type") == Some("scrum");
            let view = TrackerView { board_id: Some(id), ..TrackerView::default() };
            let detail = if scrum { "Scrum board" } else { "Board" };
            out.push(hit("board", format!("board-{id}"), name.to_owned(), Some(detail.into()), view.clone()));
            if scrum {
                let view = TrackerView { current_iteration: true, ..view };
                let label = format!("{name} (current sprint)");
                out.push(hit(
                    "sprint",
                    format!("board-{id}-sprint"),
                    label,
                    Some("Active sprint".into()),
                    view,
                ));
            }
        }
        if let Ok(f) = api.json(HttpRequest::get(api.url("/filter/favourite"))).await {
            for f in f.as_array().into_iter().flatten() {
                let (Some(id), Some(name)) = (f.get("id").and_then(idstr), s(f, "name")) else { continue };
                if matches(name, "") {
                    let view = TrackerView {
                        jql: Some(format!("filter = {id} AND {OPEN} ORDER BY updated DESC")),
                        ..TrackerView::default()
                    };
                    out.push(hit(
                        "filter",
                        format!("filter-{id}"),
                        name.to_owned(),
                        s(f, "jql").map(str::to_owned),
                        view,
                    ));
                }
            }
        }
        Ok(out)
    }

    fn browser_url(&self, t: &TicketRef) -> String {
        format!("{}/browse/{}", self.web, t.key)
    }

    fn branch_key(&self, t: &TicketRef) -> String {
        t.key.clone()
    }
}

impl JiraTracker {
    /// The view's own JQL, else a default built from its board / project (newest first).
    async fn base_jql(&self, view: &TrackerView) -> Result<String, KeltaError> {
        let closed = view.status.as_deref() == Some("closed");
        if let Some(j) = view.jql.as_deref().filter(|j| !j.trim().is_empty()) {
            // `closed` (the core's recently-done fetch) flips the view's own open clause; other JQL is kept.
            return Ok(if closed { j.replace(OPEN, DONE) } else { j.to_owned() });
        }
        let cond = if let Some(board) = view.board_id {
            // shortcut: the board's filter only; a kanban board's `subQuery` is ignored.
            let conf = self
                .api()
                .await?
                .json(HttpRequest::get(format!("{}/rest/agile/1.0/board/{board}/configuration", self.base)))
                .await?;
            let id = conf.pointer("/filter/id").and_then(idstr);
            let id = id.ok_or_else(|| KeltaError::upstream(format!("jira board {board} has no filter")))?;
            format!("filter = {id}")
        } else {
            match view.project_id.as_deref().or(view.project.as_deref()).filter(|p| !p.trim().is_empty()) {
                Some(p) => format!("project = \"{}\"", p.replace('"', "")),
                None => String::new(),
            }
        };
        // open tickets only, like the other providers; `status = "*"` lists every state
        let cond = if cond.is_empty() || view.status.as_deref() == Some("*") {
            cond
        } else if closed {
            format!("{cond} AND {DONE}")
        } else {
            format!("{cond} AND {OPEN}")
        };
        Ok(format!("{cond} ORDER BY updated DESC"))
    }

    fn tickets_from(&self, v: &Value, lk: &Lookups) -> Result<Vec<Ticket>, KeltaError> {
        Ok(v.get("issues")
            .and_then(Value::as_array)
            .map(|a| a.iter().filter_map(|i| self.ticket_from(self.account(), i, lk).ok()).collect())
            .unwrap_or_default())
    }

    /// Statuses relevant to the binding: the project's (JQL `project = KEY`) or all of them.
    async fn statuses(&self, api: &Api, b: &TrackerBinding) -> Result<Vec<Status>, KeltaError> {
        let project = b.views.iter().filter_map(|v| v.jql.as_deref()).find_map(project_key_from_jql);
        let mut out: Vec<Status> = Vec::new();
        match project {
            Some(key) => {
                let v = api
                    .json(HttpRequest::get(api.url(&format!("/project/{}/statuses", percent_encode(&key)))))
                    .await?;
                for it in v.as_array().into_iter().flatten() {
                    for st in it.get("statuses").and_then(Value::as_array).into_iter().flatten() {
                        let st = Self::status_from(st);
                        if !out.iter().any(|o| o.id == st.id) {
                            out.push(st);
                        }
                    }
                }
            }
            None => {
                let v = api.json(HttpRequest::get(api.url("/status"))).await?;
                out = v.as_array().into_iter().flatten().map(Self::status_from).collect();
            }
        }
        Ok(out)
    }
}

/// `project = SHOP`, `project in (SHOP, OPS)`, `project = "SHOP"` → first key.
/// The JQL clause that keeps a list to open tickets.
const OPEN: &str = "statusCategory != Done";
const DONE: &str = "statusCategory = Done";

pub(crate) fn project_key_from_jql(jql: &str) -> Option<String> {
    static RE: std::sync::OnceLock<Option<regex::Regex>> = std::sync::OnceLock::new();
    let re = RE
        .get_or_init(|| {
            regex::Regex::new(r#"(?i)\bproject\s*(?:=|in)\s*\(?\s*["']?([A-Za-z][A-Za-z0-9_]+)"#).ok()
        })
        .as_ref()?;
    re.captures(jql).map(|c| c[1].to_owned())
}

/// Split a trailing `ORDER BY ...` off a JQL string: (conditions, order clause).
fn split_order_by(jql: &str) -> (&str, &str) {
    static RE: std::sync::OnceLock<Option<regex::Regex>> = std::sync::OnceLock::new();
    let re = RE.get_or_init(|| regex::Regex::new(r"(?i)\border\s+by\b").ok()).as_ref();
    // The last match outside a quoted string (one open quote char at a time, `\` escapes).
    let (mut open, mut esc) = (None::<char>, false);
    let mut quoted = Vec::with_capacity(jql.len());
    for c in jql.chars() {
        quoted.push(open.is_some());
        match (open, c) {
            _ if esc => esc = false,
            (Some(_), '\\') => esc = true,
            (Some(q), c) if c == q => open = None,
            (None, '"' | '\'') => open = Some(c),
            _ => {}
        }
    }
    let m = re.and_then(|re| re.find_iter(jql).filter(|m| !quoted[jql[..m.start()].chars().count()]).last());
    match m {
        Some(m) => (jql[..m.start()].trim(), jql[m.start()..].trim()),
        None => (jql.trim(), ""),
    }
}

/// AND the `who` / current-sprint clauses onto `base`, keeping its `ORDER BY` last. Untouched when there is
/// nothing to add (`who` None keeps the legacy JQL as written).
fn compose_jql(base: &str, who: Option<Who>, iteration: bool) -> String {
    let extra: Vec<&str> = [
        match who {
            Some(Who::Mine) => Some("assignee = currentUser()"),
            Some(Who::Unassigned) => Some("assignee is EMPTY"),
            _ => None,
        },
        iteration.then_some("sprint in openSprints()"),
    ]
    .into_iter()
    .flatten()
    .collect();
    if who.is_none() && extra.is_empty() {
        return base.to_owned();
    }
    let (cond, order) = split_order_by(base);
    // A pane-level who overrides the view's own `assignee = currentUser()` (onboarding writes one).
    static ME: std::sync::OnceLock<Option<regex::Regex>> = std::sync::OnceLock::new();
    let me = ME.get_or_init(|| {
        regex::Regex::new(
            r"(?i)^\s*assignee\s*=\s*currentUser\(\)\s*(and\s+|$)|\s+and\s+assignee\s*=\s*currentUser\(\)",
        )
        .ok()
    });
    let stripped;
    let cond = match me {
        // shortcut: only a plain AND chain is rewritten (an OR may bind differently), grow it if views need more.
        Some(re) if who.is_some() && !cond.to_lowercase().contains(" or ") => {
            stripped = re.replace_all(cond, "").trim().to_owned();
            stripped.as_str()
        }
        _ => cond,
    };
    let mut parts: Vec<String> = Vec::new();
    if !cond.is_empty() {
        parts.push(if extra.is_empty() { cond.to_owned() } else { format!("({cond})") });
    }
    parts.extend(extra.into_iter().map(str::to_owned));
    format!("{} {order}", parts.join(" AND ")).trim_end().to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn jql_composition() {
        let c = compose_jql;
        // No who, no iteration: as written.
        assert_eq!(c("project = A ORDER BY rank", None, false), "project = A ORDER BY rank");
        assert_eq!(c("project = A", Some(Who::Anyone), false), "project = A");
        // Each who, with and without ORDER BY (any case), and the OR-safe parentheses.
        assert_eq!(
            c("project = A OR project = B order  by updated DESC", Some(Who::Mine), false),
            "(project = A OR project = B) AND assignee = currentUser() order  by updated DESC"
        );
        assert_eq!(c("project = A", Some(Who::Unassigned), false), "(project = A) AND assignee is EMPTY");
        // Iteration alone and combined; an ORDER BY-only base drops the empty condition.
        assert_eq!(c("project = A", None, true), "(project = A) AND sprint in openSprints()");
        assert_eq!(
            c(" ORDER BY updated DESC", Some(Who::Mine), true),
            "assignee = currentUser() AND sprint in openSprints() ORDER BY updated DESC"
        );
        // An `order by` inside a quoted string is not the clause.
        assert_eq!(
            c("summary ~ \"order by\" ORDER BY key", Some(Who::Mine), false),
            "(summary ~ \"order by\") AND assignee = currentUser() ORDER BY key"
        );
        assert_eq!(split_order_by("a = 1").1, "");
        // An apostrophe inside a double-quoted string does not hide the real ORDER BY.
        assert_eq!(
            c("text ~ \"can't login\" ORDER BY created", Some(Who::Mine), false),
            "(text ~ \"can't login\") AND assignee = currentUser() ORDER BY created"
        );
        // Onboarding's own `assignee = currentUser()` yields to the pane-level who.
        let ob = "project = X AND assignee = currentUser() AND statusCategory != Done";
        assert_eq!(
            c(ob, Some(Who::Mine), false),
            "(project = X AND statusCategory != Done) AND assignee = currentUser()"
        );
        assert_eq!(
            c(ob, Some(Who::Unassigned), false),
            "(project = X AND statusCategory != Done) AND assignee is EMPTY"
        );
        assert_eq!(c(ob, Some(Who::Anyone), false), "project = X AND statusCategory != Done");
        assert_eq!(c(ob, None, false), ob);
    }

    #[test]
    fn jira_times_become_rfc3339() {
        assert_eq!(normalize_time("2024-05-01T10:00:00.000+0200"), "2024-05-01T10:00:00.000+02:00");
        assert_eq!(normalize_time("2024-05-01T10:00:00.000-0530"), "2024-05-01T10:00:00.000-05:30");
        assert_eq!(normalize_time("2024-05-01T10:00:00Z"), "2024-05-01T10:00:00Z");
        assert_eq!(normalize_time(""), "");
    }

    #[test]
    fn project_keys_from_jql() {
        assert_eq!(
            project_key_from_jql("project = SHOP AND assignee = currentUser()").as_deref(),
            Some("SHOP")
        );
        assert_eq!(project_key_from_jql("Project in (OPS, SHOP)").as_deref(), Some("OPS"));
        assert_eq!(project_key_from_jql("project=\"WEB\"").as_deref(), Some("WEB"));
        assert_eq!(project_key_from_jql("assignee = currentUser()"), None);
    }

    #[test]
    fn cloud_cursor_roundtrip() {
        assert_eq!(JiraTracker::decode_cloud_cursor(None).unwrap(), (0, None));
        assert_eq!(
            JiraTracker::decode_cloud_cursor(Some(Cursor::Token("3:abc:def".into()))).unwrap(),
            (3, Some("abc:def".into()))
        );
        assert!(JiraTracker::decode_cloud_cursor(Some(Cursor::Offset(1))).is_err());
    }
}

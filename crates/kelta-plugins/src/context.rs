//! Template variables / matcher context (PLUGINS §1, §6): project, repo, work item, ticket, PR,
//! session, event envelope + payload, app focus, plugin namespace settings.

use std::path::PathBuf;

use kelta_proto::events::BusEvent;
use kelta_proto::ids::{ProjectId, SessionId};
use kelta_proto::model::{ProjectInfo, SessionInfo, TemplateCtx, WorkItem};
use serde_json::{Value, json};

use crate::PluginHost;
use crate::registry::Entry;
use crate::template::Vars;

/// What a context is built from.
#[derive(Default)]
pub(crate) struct CtxSpec<'a> {
    pub project: Option<&'a ProjectId>,
    pub tctx: Option<&'a TemplateCtx>,
    pub event: Option<&'a BusEvent>,
    pub plugin: Option<&'a Entry>,
}

/// Ticket object (`{key, title, url, provider, account, id}`) from a Ticket or TicketRef JSON.
pub(crate) fn ticket_value(v: &Value) -> Option<Value> {
    let (r, ticket) = match v.get("ref") {
        Some(r) => (r, Some(v)),
        None => (v, None),
    };
    let key = r.get("key")?.as_str()?;
    let account = r.get("account").and_then(Value::as_str).unwrap_or("");
    Some(json!({
        "key": key,
        "account": account,
        "provider": account,
        "id": r.get("id").cloned().unwrap_or(Value::Null),
        "title": ticket.and_then(|t| t.get("title")).cloned().unwrap_or(Value::Null),
        "url": ticket.and_then(|t| t.get("url")).cloned().unwrap_or(Value::Null),
    }))
}

/// PR object (`{url, number, repo, head, base, title, account}`) from a Review JSON.
pub(crate) fn pr_value(v: &Value) -> Option<Value> {
    let r = v.get("ref").unwrap_or(v);
    let number = r.get("number")?;
    Some(json!({
        "number": number,
        "repo": r.get("repo").cloned().unwrap_or(Value::Null),
        "account": r.get("account").cloned().unwrap_or(Value::Null),
        "url": v.get("url").cloned().unwrap_or(Value::Null),
        "title": v.get("title").cloned().unwrap_or(Value::Null),
        "head": v.get("source_branch").cloned().unwrap_or(Value::Null),
        "base": v.get("target_branch").cloned().unwrap_or(Value::Null),
    }))
}

pub(crate) fn project_root(p: &ProjectInfo) -> Option<PathBuf> {
    p.repos.iter().find(|r| r.primary).or_else(|| p.repos.first()).map(|r| r.path.clone())
}

fn home() -> String {
    std::env::var("HOME").unwrap_or_default()
}

impl PluginHost {
    pub(crate) async fn build_vars(&self, spec: CtxSpec<'_>) -> Vars {
        let mut v = Vars::new();
        let core = self.core();
        let event = spec.event;
        let tctx = spec.tctx;
        let payload = event.map(|e| e.payload.clone()).unwrap_or(Value::Null);

        let project_id: Option<ProjectId> =
            spec.project.cloned().or_else(|| event.and_then(|e| e.project_id.clone()));
        let project: Option<ProjectInfo> = match (&core, &project_id) {
            (Some(c), Some(id)) => c.project(id),
            _ => None,
        };
        let session_id: Option<SessionId> =
            tctx.and_then(|t| t.session_id.clone()).or_else(|| event.and_then(|e| e.session_id.clone()));
        let session: Option<SessionInfo> = match (&core, &session_id) {
            (Some(c), Some(id)) => c.session_get(id),
            _ => None,
        };
        let work: Option<WorkItem> = match (&core, &session_id, tctx.and_then(|t| t.work_item_id.as_ref())) {
            (Some(c), Some(id), _) => c.work_for_session(id).await,
            (Some(c), None, Some(id)) => c.work_get(id).await,
            _ => None,
        };

        v.set("config_dir", json!(self.dirs().config.display().to_string()));
        v.set("data_dir", json!(self.dirs().data.display().to_string()));
        v.set("home", json!(home()));
        v.set("user", json!(std::env::var("USER").unwrap_or_default()));

        let root = project.as_ref().and_then(project_root).map(|p| p.display().to_string());
        if let Some(p) = &project {
            v.set(
                "project",
                json!({ "id": p.id, "name": p.name, "root": root.clone().unwrap_or_else(home), "tracker": p.tracker.as_ref().map(|t| t.account.to_string()) }),
            );
        } else if let Some(id) = &project_id {
            v.set("project", json!({ "id": id, "name": id, "root": home() }));
        }
        let repo_id =
            tctx.and_then(|t| t.repo_id.clone()).or_else(|| work.as_ref().map(|w| w.repo_id.clone()));
        if let Some(p) = &project {
            let repo = repo_id
                .as_deref()
                .and_then(|id| p.repos.iter().find(|r| r.id == id))
                .or_else(|| p.repos.iter().find(|r| r.primary))
                .or_else(|| p.repos.first());
            if let Some(r) = repo {
                let name = r.path.file_name().and_then(|n| n.to_str()).unwrap_or(&r.id).to_owned();
                v.set("repo", json!({ "id": r.id, "path": r.path.display().to_string(), "name": name }));
                v.set("base", json!(r.base));
            }
        }
        if let Some(w) = &work {
            if !w.worktree.as_os_str().is_empty() {
                v.set("worktree", json!(w.worktree.display().to_string()));
            }
            v.set("branch", json!(w.branch));
            v.set("base", json!(w.base));
            // `env` is not a placeholder: `launch_env` adds it to the tool's process.
            v.set("work", json!({ "port": w.port_base, "env": w.env() }));
        }
        if let Some(s) = &session {
            let visible = self.wiring().ui.map(|ui| {
                let w = ui.window_state();
                w.visible && w.focused
            });
            v.set(
                "session",
                json!({
                    "id": s.id, "name": s.name, "cwd": s.cwd.display().to_string(), "kind": s.kind.name(),
                    "status": s.status, "visible": visible,
                }),
            );
            v.set("sid8", json!(s.id.sid8()));
            v.set("run", json!(self.dirs().session_runtime(&s.id.sid8()).display().to_string()));
        } else if let Some(id) = &session_id {
            v.set("session", json!({ "id": id }));
            v.set("sid8", json!(id.sid8()));
        }
        if let Some(ui) = self.wiring().ui {
            v.set("app", json!({ "focused": ui.window_state().focused }));
        }

        // ticket / pr: explicit ctx, then the work item, then the event payload.
        let ticket = tctx
            .and_then(|t| t.ticket.as_ref())
            .and_then(|t| serde_json::to_value(t).ok())
            .or_else(|| {
                work.as_ref().and_then(|w| w.ticket.as_ref()).and_then(|t| serde_json::to_value(t).ok())
            })
            .or_else(|| payload.get("ticket").cloned())
            .or_else(|| payload.pointer("/plan/source/ticket").cloned())
            .and_then(|t| ticket_value(&t));
        if let Some(t) = ticket {
            v.set("ticket", t);
        }
        let pr = tctx
            .and_then(|t| t.review.as_ref())
            .and_then(|r| serde_json::to_value(r).ok())
            .or_else(|| payload.get("review").cloned())
            .and_then(|r| pr_value(&r));
        if let Some(p) = pr {
            v.set("pr", p);
        }

        if let Some(e) = event {
            v.set(
                "event",
                json!({
                    "name": e.name, "ts": e.ts, "project_id": e.project_id, "session_id": e.session_id,
                    "work_item_id": e.work_item_id, "chain": e.chain,
                }),
            );
            v.set("payload", payload);
        }
        if let Some(p) = spec.plugin {
            v.set("plugin", json!({ "id": p.id, "dir": p.dir.display().to_string() }));
            let mut ns = self
                .settings(project_id.as_ref())
                .plugins
                .settings
                .get(p.id.as_str())
                .cloned()
                .unwrap_or_else(|| json!({}));
            // A secret setting's SecretRef must not reach templates (http bodies, headers).
            if let Some(schema) = self.registry().get(p.id.as_str()).and_then(|e| e.settings_schema.clone()) {
                crate::screens::mask_secrets(&mut ns, &schema);
            }
            v.set("settings", ns);
        }
        if let Some(t) = tctx {
            if let Some(cwd) = &t.cwd {
                v.set_sub("ctx", "cwd", json!(cwd.display().to_string()));
            }
            for (k, val) in &t.extra {
                match k.split_once('.') {
                    Some((root, sub)) => v.set_sub(root, sub, json!(val)),
                    None => v.set(k, json!(val)),
                };
            }
        }
        v
    }
}

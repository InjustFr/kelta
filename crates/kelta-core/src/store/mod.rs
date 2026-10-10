//! SQLite state store (ARCHITECTURE §10): one connection on one dedicated thread, fed by a channel
//! of closures (WAL, migrations at open). Implements `WorkStore`, `GrantStore` and `TrustStore`.
//!
//! Async callers use [`Store::call`]; startup reads (before window creation) use
//! [`Store::call_blocking`]; persistence of in-memory state uses fire-and-forget [`Store::exec`]
//! (ordered by the single queue).

pub mod migrations;

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc;

use async_trait::async_trait;
use kelta_proto::api::{GrantStore, PluginGrant, TrustStore, WorkStore};
use kelta_proto::codehost::ReviewRef;
use kelta_proto::error::KeltaError;
use kelta_proto::ids::{PluginId, ProjectId, SessionId, TabId, WorkItemId};
use kelta_proto::model::{StepStatus, WORK_STEPS, WorkItem, WorkKind, WorkState, WorkStepStatus};
use kelta_proto::store::{
    LayoutRow, ProjectOpenRow, ProviderCacheRow, SeenReviewRow, SessionRow, TRIGGER_LOG_CAP, TriggerLogRow,
};
use kelta_proto::tracker::TicketRef;
use rusqlite::{Connection, OptionalExtension, params};

type Job = Box<dyn FnOnce(&mut Connection) + Send>;

/// Map a sqlite error into the crate-boundary error.
pub fn db_err(e: rusqlite::Error) -> KeltaError {
    KeltaError::internal(format!("sqlite: {e}"))
}

pub struct Store {
    tx: mpsc::Sender<Job>,
    path: Option<PathBuf>,
}

impl Store {
    /// Open (or create) `<data>/kelta.db`.
    pub fn open(path: &Path) -> Result<Arc<Self>, KeltaError> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let p = path.to_path_buf();
        Self::start(Some(p.clone()), move || Connection::open(&p))
    }

    /// In-memory database (tests, or fallback when the data dir is unusable).
    pub fn open_in_memory() -> Result<Arc<Self>, KeltaError> {
        Self::start(None, Connection::open_in_memory)
    }

    fn start<F>(path: Option<PathBuf>, open: F) -> Result<Arc<Self>, KeltaError>
    where
        F: FnOnce() -> rusqlite::Result<Connection> + Send + 'static,
    {
        let (tx, rx) = mpsc::channel::<Job>();
        let (ready_tx, ready_rx) = mpsc::channel::<Result<(), KeltaError>>();
        // allowlisted: the single sqlite thread (ARCHITECTURE §2), lives as long as the Store.
        let spawned =
            std::thread::Builder::new().name("kelta-sqlite".into()).stack_size(512 * 1024).spawn(move || {
                let opened = open().and_then(|mut c| {
                    configure(&c)?;
                    migrations::migrate(&mut c)?;
                    Ok(c)
                });
                let mut conn = match opened {
                    Ok(c) => {
                        let _ = ready_tx.send(Ok(()));
                        c
                    }
                    Err(e) => {
                        let _ = ready_tx.send(Err(db_err(e)));
                        return;
                    }
                };
                while let Ok(job) = rx.recv() {
                    let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| job(&mut conn)));
                    if r.is_err() {
                        tracing::error!("store job panicked");
                    }
                }
            });
        spawned.map_err(|e| KeltaError::internal(format!("cannot start sqlite thread: {e}")))?;
        ready_rx.recv().map_err(|_| KeltaError::internal("sqlite thread exited during open"))??;
        Ok(Arc::new(Self { tx, path }))
    }

    pub fn path(&self) -> Option<&Path> {
        self.path.as_deref()
    }

    fn send(&self, job: Job) -> Result<(), KeltaError> {
        self.tx.send(job).map_err(|_| KeltaError::internal("store closed"))
    }

    /// Run `f` on the sqlite thread and await its result.
    pub async fn call<R, F>(&self, f: F) -> Result<R, KeltaError>
    where
        F: FnOnce(&mut Connection) -> Result<R, KeltaError> + Send + 'static,
        R: Send + 'static,
    {
        let (tx, rx) = tokio::sync::oneshot::channel();
        self.send(Box::new(move |c| {
            let _ = tx.send(f(c));
        }))?;
        rx.await.map_err(|_| KeltaError::internal("store job dropped"))?
    }

    /// Run `f` on the sqlite thread and block the caller until it returns (sync contexts only).
    pub fn call_blocking<R, F>(&self, f: F) -> Result<R, KeltaError>
    where
        F: FnOnce(&mut Connection) -> Result<R, KeltaError> + Send + 'static,
        R: Send + 'static,
    {
        let (tx, rx) = mpsc::channel();
        self.send(Box::new(move |c| {
            let _ = tx.send(f(c));
        }))?;
        rx.recv().map_err(|_| KeltaError::internal("store job dropped"))?
    }

    /// Fire-and-forget write; failures are logged.
    pub fn exec<F>(&self, what: &'static str, f: F)
    where
        F: FnOnce(&mut Connection) -> Result<(), KeltaError> + Send + 'static,
    {
        let r = self.send(Box::new(move |c| {
            if let Err(e) = f(c) {
                tracing::warn!(error = %e, what, "store write failed");
            }
        }));
        if let Err(e) = r {
            tracing::warn!(error = %e, what, "store write dropped");
        }
    }

    /// Wait until every queued job ran (quit flow).
    pub async fn flush(&self) -> Result<(), KeltaError> {
        self.call(|_| Ok(())).await
    }
}

fn configure(c: &Connection) -> rusqlite::Result<()> {
    c.execute_batch(
        "PRAGMA journal_mode = WAL;
         PRAGMA synchronous = NORMAL;
         PRAGMA foreign_keys = ON;
         PRAGMA busy_timeout = 2000;",
    )
}

// =============================================================================================
// Typed helpers (sync, run on the sqlite thread)
// =============================================================================================

pub mod q {
    use super::*;

    type R<T> = Result<T, KeltaError>;

    /// Names of the user tables present.
    pub fn tables(c: &Connection) -> R<Vec<String>> {
        let mut st = c
            .prepare("SELECT name FROM sqlite_master WHERE type = 'table' AND name NOT LIKE 'sqlite_%' ORDER BY name")
            .map_err(db_err)?;
        let rows = st.query_map([], |r| r.get::<_, String>(0)).map_err(db_err)?;
        rows.collect::<rusqlite::Result<Vec<_>>>().map_err(db_err)
    }

    // ---- projects_open -------------------------------------------------------------------------

    pub fn projects_open(c: &Connection) -> R<Vec<ProjectOpenRow>> {
        let mut st =
            c.prepare("SELECT project_id, ord, active FROM projects_open ORDER BY ord").map_err(db_err)?;
        let rows = st
            .query_map([], |r| {
                Ok(ProjectOpenRow {
                    project_id: ProjectId::new(r.get::<_, String>(0)?),
                    ord: r.get(1)?,
                    active: r.get::<_, i64>(2)? != 0,
                })
            })
            .map_err(db_err)?;
        rows.collect::<rusqlite::Result<Vec<_>>>().map_err(db_err)
    }

    pub fn projects_open_replace(c: &mut Connection, rows: &[ProjectOpenRow]) -> R<()> {
        let tx = c.transaction().map_err(db_err)?;
        tx.execute("DELETE FROM projects_open", []).map_err(db_err)?;
        for r in rows {
            tx.execute(
                "INSERT INTO projects_open (project_id, ord, active) VALUES (?1, ?2, ?3)",
                params![r.project_id.as_str(), r.ord, r.active as i64],
            )
            .map_err(db_err)?;
        }
        tx.commit().map_err(db_err)
    }

    // ---- layouts -------------------------------------------------------------------------------

    pub fn layouts(c: &Connection) -> R<Vec<LayoutRow>> {
        let mut st = c.prepare("SELECT project_id, json, rev, updated_at FROM layouts").map_err(db_err)?;
        let rows = st
            .query_map([], |r| {
                Ok(LayoutRow {
                    project_id: ProjectId::new(r.get::<_, String>(0)?),
                    json: r.get(1)?,
                    rev: r.get::<_, i64>(2)? as u64,
                    updated_at: r.get(3)?,
                })
            })
            .map_err(db_err)?;
        rows.collect::<rusqlite::Result<Vec<_>>>().map_err(db_err)
    }

    pub fn layout_put(c: &Connection, row: &LayoutRow) -> R<()> {
        c.execute(
            "INSERT INTO layouts (project_id, json, rev, updated_at) VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(project_id) DO UPDATE SET json = excluded.json, rev = excluded.rev,
             updated_at = excluded.updated_at",
            params![row.project_id.as_str(), row.json, row.rev as i64, row.updated_at],
        )
        .map(|_| ())
        .map_err(db_err)
    }

    pub fn layout_delete(c: &Connection, project: &ProjectId) -> R<()> {
        c.execute("DELETE FROM layouts WHERE project_id = ?1", [project.as_str()]).map(|_| ()).map_err(db_err)
    }

    // ---- sessions ------------------------------------------------------------------------------

    pub fn sessions(c: &Connection) -> R<Vec<SessionRow>> {
        let mut st = c
            .prepare(
                "SELECT id, project_id, kind_json, spec_json, name, work_item_id, restore_json, cwd,
                 lifecycle, text_tail, updated_at FROM sessions ORDER BY id",
            )
            .map_err(db_err)?;
        let rows = st
            .query_map([], |r| {
                Ok(SessionRow {
                    id: SessionId::new(r.get::<_, String>(0)?),
                    project_id: ProjectId::new(r.get::<_, String>(1)?),
                    kind_json: r.get(2)?,
                    spec_json: r.get(3)?,
                    name: r.get(4)?,
                    work_item_id: r.get::<_, Option<String>>(5)?.map(WorkItemId::new),
                    restore_json: r.get(6)?,
                    cwd: PathBuf::from(r.get::<_, String>(7)?),
                    lifecycle: r.get(8)?,
                    text_tail: r.get(9)?,
                    updated_at: r.get(10)?,
                })
            })
            .map_err(db_err)?;
        rows.collect::<rusqlite::Result<Vec<_>>>().map_err(db_err)
    }

    pub fn session_put(c: &Connection, s: &SessionRow) -> R<()> {
        c.execute(
            "INSERT INTO sessions (id, project_id, kind_json, spec_json, name, work_item_id, restore_json,
             cwd, lifecycle, text_tail, updated_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)
             ON CONFLICT(id) DO UPDATE SET project_id = excluded.project_id, kind_json = excluded.kind_json,
             spec_json = excluded.spec_json, name = excluded.name, work_item_id = excluded.work_item_id,
             restore_json = excluded.restore_json, cwd = excluded.cwd, lifecycle = excluded.lifecycle,
             text_tail = COALESCE(excluded.text_tail, sessions.text_tail), updated_at = excluded.updated_at",
            params![
                s.id.as_str(),
                s.project_id.as_str(),
                s.kind_json,
                s.spec_json,
                s.name,
                s.work_item_id.as_ref().map(|w| w.as_str().to_owned()),
                s.restore_json,
                s.cwd.to_string_lossy(),
                s.lifecycle,
                s.text_tail,
                s.updated_at,
            ],
        )
        .map(|_| ())
        .map_err(db_err)
    }

    pub fn session_delete(c: &Connection, id: &SessionId) -> R<()> {
        c.execute("DELETE FROM sessions WHERE id = ?1", [id.as_str()]).map(|_| ()).map_err(db_err)
    }

    pub fn session_set_cwd(c: &Connection, id: &SessionId, cwd: &Path) -> R<()> {
        c.execute(
            "UPDATE sessions SET cwd = ?2, updated_at = ?3 WHERE id = ?1",
            params![id.as_str(), cwd.to_string_lossy(), kelta_proto::now_rfc3339()],
        )
        .map(|_| ())
        .map_err(db_err)
    }

    // ---- seen_reviews --------------------------------------------------------------------------

    pub fn seen_reviews(c: &Connection, account: &str) -> R<Vec<SeenReviewRow>> {
        let mut st = c
            .prepare(
                "SELECT account, repo, number, head_sha, first_seen FROM seen_reviews WHERE account = ?1",
            )
            .map_err(db_err)?;
        let rows = st
            .query_map([account], |r| {
                Ok(SeenReviewRow {
                    account: r.get(0)?,
                    repo: r.get(1)?,
                    number: r.get::<_, i64>(2)? as u64,
                    head_sha: r.get(3)?,
                    first_seen: r.get(4)?,
                })
            })
            .map_err(db_err)?;
        rows.collect::<rusqlite::Result<Vec<_>>>().map_err(db_err)
    }

    pub fn seen_review_put(c: &Connection, row: &SeenReviewRow) -> R<()> {
        c.execute(
            "INSERT INTO seen_reviews (account, repo, number, head_sha, first_seen) VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(account, repo, number) DO UPDATE SET head_sha = excluded.head_sha",
            params![row.account, row.repo, row.number as i64, row.head_sha, row.first_seen],
        )
        .map(|_| ())
        .map_err(db_err)
    }

    pub fn seen_review_key(r: &ReviewRef) -> (String, String, u64) {
        (r.account.as_str().to_owned(), r.repo.clone(), r.number)
    }

    // ---- provider_cache ------------------------------------------------------------------------

    pub fn cache_get(c: &Connection, key: &str) -> R<Option<ProviderCacheRow>> {
        c.query_row(
            "SELECT key, etag, body_json, fetched_at FROM provider_cache WHERE key = ?1",
            [key],
            |r| {
                Ok(ProviderCacheRow {
                    key: r.get(0)?,
                    etag: r.get(1)?,
                    body_json: r.get(2)?,
                    fetched_at: r.get(3)?,
                })
            },
        )
        .optional()
        .map_err(db_err)
    }

    pub fn cache_put(c: &Connection, row: &ProviderCacheRow) -> R<()> {
        c.execute(
            "INSERT INTO provider_cache (key, etag, body_json, fetched_at) VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(key) DO UPDATE SET etag = excluded.etag, body_json = excluded.body_json,
             fetched_at = excluded.fetched_at",
            params![row.key, row.etag, row.body_json, row.fetched_at],
        )
        .map(|_| ())
        .map_err(db_err)
    }

    // ---- ui_state ------------------------------------------------------------------------------

    pub fn ui_state_get(c: &Connection, key: &str) -> R<Option<String>> {
        c.query_row("SELECT value FROM ui_state WHERE key = ?1", [key], |r| r.get(0))
            .optional()
            .map_err(db_err)
    }

    pub fn ui_state_set(c: &Connection, key: &str, value: &str) -> R<()> {
        c.execute(
            "INSERT INTO ui_state (key, value) VALUES (?1, ?2) ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![key, value],
        )
        .map(|_| ())
        .map_err(db_err)
    }

    // ---- trigger_log ---------------------------------------------------------------------------

    pub fn trigger_log_append(c: &Connection, row: &TriggerLogRow) -> R<i64> {
        c.execute(
            "INSERT INTO trigger_log (ts, trigger_id, event, ok, detail, depth) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![row.ts, row.trigger_id, row.event, row.ok as i64, row.detail, row.depth as i64],
        )
        .map_err(db_err)?;
        let id = c.last_insert_rowid();
        c.execute(
            "DELETE FROM trigger_log WHERE id <= (SELECT MAX(id) FROM trigger_log) - ?1",
            [TRIGGER_LOG_CAP as i64],
        )
        .map_err(db_err)?;
        Ok(id)
    }

    pub fn trigger_log(c: &Connection, limit: u32) -> R<Vec<TriggerLogRow>> {
        let mut st = c
            .prepare("SELECT id, ts, trigger_id, event, ok, detail, depth FROM trigger_log ORDER BY id DESC LIMIT ?1")
            .map_err(db_err)?;
        let rows = st
            .query_map([limit as i64], |r| {
                Ok(TriggerLogRow {
                    id: r.get(0)?,
                    ts: r.get(1)?,
                    trigger_id: r.get(2)?,
                    event: r.get(3)?,
                    ok: r.get::<_, i64>(4)? != 0,
                    detail: r.get(5)?,
                    depth: r.get::<_, i64>(6)?.clamp(0, 255) as u8,
                })
            })
            .map_err(db_err)?;
        rows.collect::<rusqlite::Result<Vec<_>>>().map_err(db_err)
    }

    // ---- work items ----------------------------------------------------------------------------

    fn kind_str(k: WorkKind) -> &'static str {
        match k {
            WorkKind::Ticket => "ticket",
            WorkKind::Review => "review",
            WorkKind::Branch => "branch",
        }
    }

    fn kind_parse(s: &str) -> WorkKind {
        match s {
            "review" => WorkKind::Review,
            "branch" => WorkKind::Branch,
            _ => WorkKind::Ticket,
        }
    }

    pub fn step_str(s: StepStatus) -> &'static str {
        match s {
            StepStatus::Pending => "pending",
            StepStatus::Running => "running",
            StepStatus::Done => "done",
            StepStatus::Failed => "failed",
            StepStatus::Skipped => "skipped",
        }
    }

    fn step_parse(s: &str) -> StepStatus {
        match s {
            "running" => StepStatus::Running,
            "done" => StepStatus::Done,
            "failed" => StepStatus::Failed,
            "skipped" => StepStatus::Skipped,
            _ => StepStatus::Pending,
        }
    }

    fn json_opt<T: serde::Serialize>(v: &Option<T>) -> R<Option<String>> {
        v.as_ref().map(serde_json::to_string).transpose().map_err(KeltaError::from)
    }

    pub fn work_put(c: &Connection, w: &WorkItem) -> R<()> {
        c.execute(
            "INSERT INTO work_items (id, project_id, kind, ticket_json, review_json, repo_id, worktree, branch,
             base, claude_uuid, nvim_socket, tab_id, pr_url, state_json, created_at, updated_at, session_ids_json,
             review_due, claude_replied, title, pr_title_needs_key, sent_threads_json, rebase_json, claude_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21, ?22, ?23, ?24)
             ON CONFLICT(id) DO UPDATE SET project_id = excluded.project_id, kind = excluded.kind,
             ticket_json = excluded.ticket_json, review_json = excluded.review_json, repo_id = excluded.repo_id,
             worktree = excluded.worktree, branch = excluded.branch, base = excluded.base,
             claude_uuid = excluded.claude_uuid, nvim_socket = excluded.nvim_socket, tab_id = excluded.tab_id,
             pr_url = excluded.pr_url, state_json = excluded.state_json, updated_at = excluded.updated_at,
             session_ids_json = excluded.session_ids_json, review_due = excluded.review_due,
             claude_replied = excluded.claude_replied, title = excluded.title,
             pr_title_needs_key = excluded.pr_title_needs_key, sent_threads_json = excluded.sent_threads_json,
             rebase_json = excluded.rebase_json, claude_at = excluded.claude_at",
            params![
                w.id.as_str(),
                w.project_id.as_str(),
                kind_str(w.kind),
                json_opt(&w.ticket)?,
                json_opt(&w.review)?,
                w.repo_id,
                w.worktree.to_string_lossy(),
                w.branch,
                w.base,
                w.claude_uuid,
                w.nvim_socket.as_ref().map(|p| p.to_string_lossy().into_owned()),
                w.tab_id.as_ref().map(|t| t.as_str().to_owned()),
                w.pr_url,
                serde_json::to_string(&w.state)?,
                w.created_at,
                kelta_proto::now_rfc3339(),
                serde_json::to_string(&w.session_ids)?,
                w.review_due,
                w.claude_replied,
                w.title,
                w.pr_title_needs_key,
                serde_json::to_string(&w.sent_threads)?,
                json_opt(&w.rebase)?,
                w.claude_at,
            ],
        )
        .map_err(db_err)?;
        for s in &w.steps {
            c.execute(
                "INSERT INTO work_steps (work_item_id, step, status, detail, updated_at) VALUES (?1, ?2, ?3, ?4, ?5)
                 ON CONFLICT(work_item_id, step) DO UPDATE SET status = excluded.status, detail = excluded.detail,
                 updated_at = excluded.updated_at",
                params![w.id.as_str(), s.step, step_str(s.status), s.detail, s.updated_at],
            )
            .map_err(db_err)?;
        }
        Ok(())
    }

    const WORK_COLS: &str = "id, project_id, kind, ticket_json, review_json, repo_id, worktree, branch, base,
        claude_uuid, nvim_socket, tab_id, pr_url, state_json, created_at, session_ids_json, review_due,
        claude_replied, title, pr_title_needs_key, sent_threads_json, rebase_json, claude_at";

    type WorkRaw = (WorkItem, String, String, String, String, String, Option<String>);

    fn work_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<WorkRaw> {
        let ticket: Option<String> = r.get(3)?;
        let review: Option<String> = r.get(4)?;
        let state: String = r.get(13)?;
        let sessions: String = r.get(15)?;
        let threads: String = r.get(20)?;
        let rebase: Option<String> = r.get(21)?;
        let item = WorkItem {
            id: WorkItemId::new(r.get::<_, String>(0)?),
            project_id: ProjectId::new(r.get::<_, String>(1)?),
            kind: kind_parse(&r.get::<_, String>(2)?),
            ticket: None,
            review: None,
            repo_id: r.get(5)?,
            worktree: PathBuf::from(r.get::<_, String>(6)?),
            branch: r.get(7)?,
            base: r.get(8)?,
            claude_uuid: r.get(9)?,
            nvim_socket: r.get::<_, Option<String>>(10)?.map(PathBuf::from),
            session_ids: Vec::new(),
            tab_id: r.get::<_, Option<String>>(11)?.map(TabId::new),
            pr_url: r.get(12)?,
            state: WorkState::Planned,
            steps: Vec::new(),
            created_at: r.get(14)?,
            review_due: r.get(16)?,
            claude_replied: r.get(17)?,
            claude_at: r.get(22)?,
            title: r.get(18)?,
            pr_title_needs_key: r.get(19)?,
            sent_threads: Vec::new(),
            rebase: None,
        };
        Ok((item, ticket.unwrap_or_default(), review.unwrap_or_default(), state, sessions, threads, rebase))
    }

    fn finish_work(c: &Connection, raw: WorkRaw) -> R<WorkItem> {
        let (mut item, ticket, review, state, sessions, threads, rebase) = raw;
        if !ticket.is_empty() {
            item.ticket = Some(serde_json::from_str::<TicketRef>(&ticket)?);
        }
        if !review.is_empty() {
            item.review = Some(serde_json::from_str::<ReviewRef>(&review)?);
        }
        item.state = serde_json::from_str(&state)?;
        item.session_ids = serde_json::from_str(&sessions).unwrap_or_default();
        item.sent_threads = serde_json::from_str(&threads).unwrap_or_default();
        item.rebase = rebase.and_then(|r| serde_json::from_str(&r).ok());
        item.steps = steps(c, &item.id)?;
        Ok(item)
    }

    pub fn work_get(c: &Connection, id: &WorkItemId) -> R<Option<WorkItem>> {
        let raw = c
            .query_row(&format!("SELECT {WORK_COLS} FROM work_items WHERE id = ?1"), [id.as_str()], work_row)
            .optional()
            .map_err(db_err)?;
        raw.map(|r| finish_work(c, r)).transpose()
    }

    pub fn work_list(c: &Connection, project: Option<&ProjectId>) -> R<Vec<WorkItem>> {
        let mut st = c
            .prepare(&format!(
                "SELECT {WORK_COLS} FROM work_items WHERE (?1 IS NULL OR project_id = ?1) ORDER BY created_at, id"
            ))
            .map_err(db_err)?;
        let raws = st
            .query_map([project.map(|p| p.as_str().to_owned())], work_row)
            .map_err(db_err)?
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(db_err)?;
        raws.into_iter().map(|r| finish_work(c, r)).collect()
    }

    pub fn work_delete(c: &Connection, id: &WorkItemId) -> R<()> {
        c.execute("DELETE FROM work_steps WHERE work_item_id = ?1", [id.as_str()]).map_err(db_err)?;
        c.execute("DELETE FROM work_items WHERE id = ?1", [id.as_str()]).map_err(db_err)?;
        Ok(())
    }

    pub fn step_set(
        c: &Connection,
        id: &WorkItemId,
        step: &str,
        status: StepStatus,
        detail: Option<&str>,
    ) -> R<()> {
        c.execute(
            "INSERT INTO work_steps (work_item_id, step, status, detail, updated_at) VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(work_item_id, step) DO UPDATE SET status = excluded.status, detail = excluded.detail,
             updated_at = excluded.updated_at",
            params![id.as_str(), step, step_str(status), detail, kelta_proto::now_rfc3339()],
        )
        .map(|_| ())
        .map_err(db_err)
    }

    /// Steps in saga order, then unknown steps by name.
    pub fn steps(c: &Connection, id: &WorkItemId) -> R<Vec<WorkStepStatus>> {
        let mut st = c
            .prepare("SELECT step, status, detail, updated_at FROM work_steps WHERE work_item_id = ?1 ORDER BY step")
            .map_err(db_err)?;
        let mut all = st
            .query_map([id.as_str()], |r| {
                Ok(WorkStepStatus {
                    step: r.get(0)?,
                    status: step_parse(&r.get::<_, String>(1)?),
                    detail: r.get(2)?,
                    updated_at: r.get(3)?,
                })
            })
            .map_err(db_err)?
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(db_err)?;
        let rank = |s: &str| WORK_STEPS.iter().position(|x| *x == s).unwrap_or(WORK_STEPS.len());
        all.sort_by(|a, b| rank(&a.step).cmp(&rank(&b.step)).then_with(|| a.step.cmp(&b.step)));
        Ok(all)
    }

    // ---- grants / trust ------------------------------------------------------------------------

    pub fn grants(c: &Connection, plugin: &PluginId) -> R<Vec<PluginGrant>> {
        let mut st = c
            .prepare(
                "SELECT permission, granted_at, manifest_sha256 FROM plugin_grants WHERE plugin_id = ?1 ORDER BY permission",
            )
            .map_err(db_err)?;
        let rows = st
            .query_map([plugin.as_str()], |r| {
                Ok(PluginGrant { permission: r.get(0)?, granted_at: r.get(1)?, manifest_sha256: r.get(2)? })
            })
            .map_err(db_err)?;
        rows.collect::<rusqlite::Result<Vec<_>>>().map_err(db_err)
    }

    pub fn grant(c: &mut Connection, plugin: &PluginId, permissions: &[String], sha: &str) -> R<()> {
        let tx = c.transaction().map_err(db_err)?;
        let now = kelta_proto::now_rfc3339();
        for p in permissions {
            tx.execute(
                "INSERT INTO plugin_grants (plugin_id, permission, granted_at, manifest_sha256) VALUES (?1, ?2, ?3, ?4)
                 ON CONFLICT(plugin_id, permission) DO NOTHING",
                params![plugin.as_str(), p, now, sha],
            )
            .map_err(db_err)?;
        }
        tx.execute(
            "UPDATE plugin_grants SET manifest_sha256 = ?2 WHERE plugin_id = ?1",
            params![plugin.as_str(), sha],
        )
        .map_err(db_err)?;
        tx.commit().map_err(db_err)
    }

    pub fn revoke_all(c: &Connection, plugin: &PluginId) -> R<()> {
        c.execute("DELETE FROM plugin_grants WHERE plugin_id = ?1", [plugin.as_str()])
            .map(|_| ())
            .map_err(db_err)
    }

    pub fn kv_get(c: &Connection, plugin: &PluginId, key: &str) -> R<Option<String>> {
        c.query_row(
            "SELECT value FROM plugin_kv WHERE plugin_id = ?1 AND key = ?2",
            params![plugin.as_str(), key],
            |r| r.get(0),
        )
        .optional()
        .map_err(db_err)
    }

    /// Quota check and upsert in one transaction (bytes: `CAST AS BLOB`, `length(text)` counts chars).
    pub fn kv_set(c: &mut Connection, plugin: &PluginId, key: &str, value: &str, quota: usize) -> R<()> {
        let tx = c.transaction().map_err(db_err)?;
        let others: i64 = tx
            .query_row(
                "SELECT COALESCE(SUM(length(CAST(key AS BLOB)) + length(CAST(value AS BLOB))), 0)
                 FROM plugin_kv WHERE plugin_id = ?1 AND key <> ?2",
                params![plugin.as_str(), key],
                |r| r.get(0),
            )
            .map_err(db_err)?;
        if others as usize + key.len() + value.len() > quota {
            return Err(kelta_proto::api::kv_quota_error(quota));
        }
        tx.execute(
            "INSERT INTO plugin_kv (plugin_id, key, value) VALUES (?1, ?2, ?3)
             ON CONFLICT(plugin_id, key) DO UPDATE SET value = excluded.value",
            params![plugin.as_str(), key, value],
        )
        .map_err(db_err)?;
        tx.commit().map_err(db_err)
    }

    /// `key: None` deletes every key of the plugin.
    pub fn kv_delete(c: &Connection, plugin: &PluginId, key: Option<&str>) -> R<()> {
        c.execute(
            "DELETE FROM plugin_kv WHERE plugin_id = ?1 AND (?2 IS NULL OR key = ?2)",
            params![plugin.as_str(), key],
        )
        .map(|_| ())
        .map_err(db_err)
    }

    pub fn kv_keys(c: &Connection, plugin: &PluginId) -> R<Vec<String>> {
        let mut st =
            c.prepare("SELECT key FROM plugin_kv WHERE plugin_id = ?1 ORDER BY key").map_err(db_err)?;
        let rows = st.query_map([plugin.as_str()], |r| r.get(0)).map_err(db_err)?;
        rows.collect::<rusqlite::Result<Vec<_>>>().map_err(db_err)
    }

    pub fn trusted_hash(c: &Connection, path: &Path) -> R<Option<String>> {
        c.query_row("SELECT sha256 FROM repo_trust WHERE path = ?1", [path.to_string_lossy()], |r| r.get(0))
            .optional()
            .map_err(db_err)
    }

    pub fn set_trust(c: &Connection, path: &Path, sha: Option<&str>) -> R<()> {
        match sha {
            Some(h) => c.execute(
                "INSERT INTO repo_trust (path, sha256, trusted_at) VALUES (?1, ?2, ?3)
                 ON CONFLICT(path) DO UPDATE SET sha256 = excluded.sha256, trusted_at = excluded.trusted_at",
                params![path.to_string_lossy(), h, kelta_proto::now_rfc3339()],
            ),
            None => c.execute("DELETE FROM repo_trust WHERE path = ?1", [path.to_string_lossy()]),
        }
        .map(|_| ())
        .map_err(db_err)
    }
}

// =============================================================================================
// Trait impls
// =============================================================================================

#[async_trait]
impl WorkStore for Store {
    async fn put_item(&self, item: &WorkItem) -> Result<(), KeltaError> {
        let item = item.clone();
        self.call(move |c| q::work_put(c, &item)).await
    }

    async fn get_item(&self, id: &WorkItemId) -> Result<Option<WorkItem>, KeltaError> {
        let id = id.clone();
        self.call(move |c| q::work_get(c, &id)).await
    }

    async fn list_items(&self, project: Option<&ProjectId>) -> Result<Vec<WorkItem>, KeltaError> {
        let project = project.cloned();
        self.call(move |c| q::work_list(c, project.as_ref())).await
    }

    async fn delete_item(&self, id: &WorkItemId) -> Result<(), KeltaError> {
        let id = id.clone();
        self.call(move |c| q::work_delete(c, &id)).await
    }

    async fn set_step(
        &self,
        id: &WorkItemId,
        step: &str,
        status: StepStatus,
        detail: Option<String>,
    ) -> Result<(), KeltaError> {
        let (id, step) = (id.clone(), step.to_owned());
        self.call(move |c| q::step_set(c, &id, &step, status, detail.as_deref())).await
    }

    async fn steps(&self, id: &WorkItemId) -> Result<Vec<WorkStepStatus>, KeltaError> {
        let id = id.clone();
        self.call(move |c| q::steps(c, &id)).await
    }
}

#[async_trait]
impl GrantStore for Store {
    async fn grants(&self, plugin: &PluginId) -> Result<Vec<PluginGrant>, KeltaError> {
        let plugin = plugin.clone();
        self.call(move |c| q::grants(c, &plugin)).await
    }

    async fn grant(
        &self,
        plugin: &PluginId,
        permissions: &[String],
        manifest_sha256: &str,
    ) -> Result<(), KeltaError> {
        let (plugin, perms, sha) = (plugin.clone(), permissions.to_vec(), manifest_sha256.to_owned());
        self.call(move |c| q::grant(c, &plugin, &perms, &sha)).await
    }

    async fn revoke_all(&self, plugin: &PluginId) -> Result<(), KeltaError> {
        let plugin = plugin.clone();
        self.call(move |c| q::revoke_all(c, &plugin)).await
    }

    async fn kv_get(&self, plugin: &PluginId, key: &str) -> Result<Option<String>, KeltaError> {
        let (plugin, key) = (plugin.clone(), key.to_owned());
        self.call(move |c| q::kv_get(c, &plugin, &key)).await
    }

    async fn kv_set(
        &self,
        plugin: &PluginId,
        key: &str,
        value: String,
        quota: usize,
    ) -> Result<(), KeltaError> {
        let (plugin, key) = (plugin.clone(), key.to_owned());
        self.call(move |c| q::kv_set(c, &plugin, &key, &value, quota)).await
    }

    async fn kv_delete(&self, plugin: &PluginId, key: &str) -> Result<(), KeltaError> {
        let (plugin, key) = (plugin.clone(), key.to_owned());
        self.call(move |c| q::kv_delete(c, &plugin, Some(&key))).await
    }

    async fn kv_keys(&self, plugin: &PluginId) -> Result<Vec<String>, KeltaError> {
        let plugin = plugin.clone();
        self.call(move |c| q::kv_keys(c, &plugin)).await
    }

    async fn kv_clear(&self, plugin: &PluginId) -> Result<(), KeltaError> {
        let plugin = plugin.clone();
        self.call(move |c| q::kv_delete(c, &plugin, None)).await
    }
}

#[async_trait]
impl TrustStore for Store {
    async fn trusted_hash(&self, path: &Path) -> Result<Option<String>, KeltaError> {
        let path = path.to_path_buf();
        self.call(move |c| q::trusted_hash(c, &path)).await
    }

    async fn set_trust(&self, path: &Path, sha256: Option<String>) -> Result<(), KeltaError> {
        let path = path.to_path_buf();
        self.call(move |c| q::set_trust(c, &path, sha256.as_deref())).await
    }
}

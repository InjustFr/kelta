//! Schema migrations (ARCHITECTURE §10). Each entry upgrades the schema by one version; the index
//! + 1 is the version recorded in `schema_version(v)`.

use rusqlite::Connection;

/// v1: every table of ARCHITECTURE §10.
///
/// Deviation (docs/contract-requests/L3.md): `work_items` carries an extra `session_ids_json`
/// column because `WorkItem.session_ids` must round-trip through `WorkStore`.
const V1: &str = r#"
CREATE TABLE IF NOT EXISTS projects_open (
  project_id TEXT PRIMARY KEY NOT NULL,
  ord        INTEGER NOT NULL,
  active     INTEGER NOT NULL DEFAULT 0
);
CREATE TABLE IF NOT EXISTS layouts (
  project_id TEXT PRIMARY KEY NOT NULL,
  json       TEXT NOT NULL,
  rev        INTEGER NOT NULL,
  updated_at TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS sessions (
  id           TEXT PRIMARY KEY NOT NULL,
  project_id   TEXT NOT NULL,
  kind_json    TEXT NOT NULL,
  spec_json    TEXT NOT NULL,
  name         TEXT NOT NULL,
  work_item_id TEXT,
  restore_json TEXT NOT NULL,
  cwd          TEXT NOT NULL,
  lifecycle    TEXT NOT NULL,
  text_tail    TEXT,
  updated_at   TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS sessions_project ON sessions(project_id);
CREATE TABLE IF NOT EXISTS work_items (
  id               TEXT PRIMARY KEY NOT NULL,
  project_id       TEXT NOT NULL,
  kind             TEXT NOT NULL,
  ticket_json      TEXT,
  review_json      TEXT,
  repo_id          TEXT NOT NULL,
  worktree         TEXT NOT NULL,
  branch           TEXT NOT NULL,
  base             TEXT NOT NULL,
  claude_uuid      TEXT,
  nvim_socket      TEXT,
  tab_id           TEXT,
  pr_url           TEXT,
  state_json       TEXT NOT NULL,
  created_at       TEXT NOT NULL,
  updated_at       TEXT NOT NULL,
  session_ids_json TEXT NOT NULL DEFAULT '[]'
);
CREATE INDEX IF NOT EXISTS work_items_project ON work_items(project_id);
CREATE TABLE IF NOT EXISTS work_steps (
  work_item_id TEXT NOT NULL,
  step         TEXT NOT NULL,
  status       TEXT NOT NULL,
  detail       TEXT,
  updated_at   TEXT NOT NULL,
  PRIMARY KEY (work_item_id, step)
);
CREATE TABLE IF NOT EXISTS seen_reviews (
  account    TEXT NOT NULL,
  repo       TEXT NOT NULL,
  number     INTEGER NOT NULL,
  head_sha   TEXT NOT NULL,
  first_seen TEXT NOT NULL,
  PRIMARY KEY (account, repo, number)
);
CREATE TABLE IF NOT EXISTS provider_cache (
  key        TEXT PRIMARY KEY NOT NULL,
  etag       TEXT,
  body_json  TEXT NOT NULL,
  fetched_at TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS plugin_grants (
  plugin_id       TEXT NOT NULL,
  permission      TEXT NOT NULL,
  granted_at      TEXT NOT NULL,
  manifest_sha256 TEXT NOT NULL,
  PRIMARY KEY (plugin_id, permission)
);
CREATE TABLE IF NOT EXISTS plugin_kv (
  plugin_id TEXT NOT NULL,
  key       TEXT NOT NULL,
  value     TEXT NOT NULL,
  PRIMARY KEY (plugin_id, key)
);
CREATE TABLE IF NOT EXISTS repo_trust (
  path       TEXT PRIMARY KEY NOT NULL,
  sha256     TEXT NOT NULL,
  trusted_at TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS trigger_log (
  id         INTEGER PRIMARY KEY AUTOINCREMENT,
  ts         TEXT NOT NULL,
  trigger_id TEXT NOT NULL,
  event      TEXT NOT NULL,
  ok         INTEGER NOT NULL,
  detail     TEXT,
  depth      INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS ui_state (
  key   TEXT PRIMARY KEY NOT NULL,
  value TEXT NOT NULL
);
"#;

/// v2: durable Claude signals on work items (FLOW §2.3).
const V2: &str = r#"
ALTER TABLE work_items ADD COLUMN review_due INTEGER NOT NULL DEFAULT 0;
ALTER TABLE work_items ADD COLUMN claude_replied INTEGER NOT NULL DEFAULT 0;
"#;

/// v3: scratch work items (FLOW §2.1, §4.3): `WorkItem.title`, `WorkItem.pr_title_needs_key`.
const V3: &str = r#"
ALTER TABLE work_items ADD COLUMN title TEXT;
ALTER TABLE work_items ADD COLUMN pr_title_needs_key INTEGER NOT NULL DEFAULT 0;
"#;

/// v4: Fix with Claude and rebase state on work items (FLOW §8).
const V4: &str = r#"
ALTER TABLE work_items ADD COLUMN sent_threads_json TEXT NOT NULL DEFAULT '[]';
ALTER TABLE work_items ADD COLUMN rebase_json TEXT;
"#;

/// v5: when Claude last stopped or asked (`WorkItem.claude_at`, Now's order).
const V5: &str = r#"
ALTER TABLE work_items ADD COLUMN claude_at TEXT;
"#;

/// v6: Ready for review (#134): Claude's full last message, the delta since review, Louis's
/// `next:` note and when he left the item; the reviewed head of PRs next to `seen_reviews`.
const V6: &str = r#"
ALTER TABLE work_items ADD COLUMN claude_message TEXT;
ALTER TABLE work_items ADD COLUMN delta_json TEXT;
ALTER TABLE work_items ADD COLUMN next_note TEXT;
ALTER TABLE work_items ADD COLUMN left_at TEXT;
ALTER TABLE seen_reviews ADD COLUMN reviewed_sha TEXT;
"#;

/// v7: per-item port block (`WorkItem.port_base`).
const V7: &str = r#"
ALTER TABLE work_items ADD COLUMN port_base INTEGER;
"#;

/// v8: Claude spend of a work item's ended sessions (`WorkItem.cost_usd`).
const V8: &str = r#"
ALTER TABLE work_items ADD COLUMN cost_usd REAL NOT NULL DEFAULT 0;
"#;

/// v9: review notes (#133), `ReviewNote`.
const V9: &str = r#"
CREATE TABLE IF NOT EXISTS notes (
  id         INTEGER PRIMARY KEY AUTOINCREMENT,
  work_item  TEXT NOT NULL,
  path       TEXT NOT NULL,
  line_start INTEGER NOT NULL,
  line_end   INTEGER NOT NULL,
  body       TEXT NOT NULL,
  source     TEXT NOT NULL,
  ext_ref    TEXT,
  state      TEXT NOT NULL,
  sent_at    TEXT
);
CREATE INDEX IF NOT EXISTS notes_work_item ON notes(work_item);
"#;

/// Ordered migrations; `MIGRATIONS.len()` == `kelta_proto::store::SCHEMA_VERSION`.
pub const MIGRATIONS: &[&str] = &[V1, V2, V3, V4, V5, V6, V7, V8, V9];

/// Current recorded version (0 for an empty database).
pub fn current_version(conn: &Connection) -> rusqlite::Result<u32> {
    conn.execute_batch("CREATE TABLE IF NOT EXISTS schema_version (v INTEGER NOT NULL)")?;
    let v: Option<u32> = conn.query_row("SELECT MAX(v) FROM schema_version", [], |r| r.get(0))?;
    Ok(v.unwrap_or(0))
}

/// Apply every pending migration in its own transaction. Idempotent.
pub fn migrate(conn: &mut Connection) -> rusqlite::Result<u32> {
    let mut cur = current_version(conn)?;
    for (i, sql) in MIGRATIONS.iter().enumerate() {
        let v = (i + 1) as u32;
        if v <= cur {
            continue;
        }
        let tx = conn.transaction()?;
        tx.execute_batch(sql)?;
        tx.execute("INSERT INTO schema_version (v) VALUES (?1)", [v])?;
        tx.commit()?;
        cur = v;
    }
    Ok(cur)
}

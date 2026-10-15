//! In-memory `WorkStore`, `GrantStore`, `TrustStore`.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use async_trait::async_trait;
use parking_lot::Mutex;

use kelta_proto::api::{GrantStore, PluginGrant, TrustStore, WorkStore};
use kelta_proto::error::KeltaError;
use kelta_proto::ids::{PluginId, ProjectId, WorkItemId};
use kelta_proto::model::{ReviewNote, StepStatus, TicketStep, WorkItem, WorkStepStatus};
use kelta_proto::tracker::TicketRef;

#[derive(Default)]
pub struct MemWorkStore {
    items: Mutex<BTreeMap<WorkItemId, WorkItem>>,
    steps: Mutex<BTreeMap<WorkItemId, BTreeMap<String, WorkStepStatus>>>,
    notes: Mutex<Vec<ReviewNote>>,
    ticket_steps: Mutex<BTreeMap<String, TicketStep>>,
}

impl MemWorkStore {
    pub fn new() -> Self {
        Self::default()
    }
}

#[async_trait]
impl WorkStore for MemWorkStore {
    async fn ticket_step(&self, t: &TicketRef) -> Result<Option<TicketStep>, KeltaError> {
        Ok(self.ticket_steps.lock().get(&format!("{}:{}", t.account.as_str(), t.key)).cloned())
    }

    async fn put_ticket_step(&self, s: &TicketStep) -> Result<(), KeltaError> {
        self.ticket_steps.lock().insert(format!("{}:{}", s.ticket.account.as_str(), s.ticket.key), s.clone());
        Ok(())
    }

    async fn ticket_steps(&self) -> Result<Vec<TicketStep>, KeltaError> {
        Ok(self.ticket_steps.lock().values().cloned().collect())
    }

    async fn put_item(&self, item: &WorkItem) -> Result<(), KeltaError> {
        self.items.lock().insert(item.id.clone(), item.clone());
        Ok(())
    }

    async fn get_item(&self, id: &WorkItemId) -> Result<Option<WorkItem>, KeltaError> {
        Ok(self.items.lock().get(id).cloned())
    }

    async fn list_items(&self, project: Option<&ProjectId>) -> Result<Vec<WorkItem>, KeltaError> {
        Ok(self
            .items
            .lock()
            .values()
            .filter(|w| project.is_none_or(|p| &w.project_id == p))
            .cloned()
            .collect())
    }

    async fn claim_item(&self, item: &WorkItem) -> Result<Result<WorkItem, Box<WorkItem>>, KeltaError> {
        let mut items = self.items.lock();
        let held = kelta_proto::api::claim_holder(items.values(), item);
        let r = item.clone().claim_over(held);
        if let Ok(w) = &r {
            items.insert(w.id.clone(), w.clone());
        }
        Ok(r)
    }

    async fn set_step(
        &self,
        id: &WorkItemId,
        step: &str,
        status: StepStatus,
        detail: Option<String>,
    ) -> Result<(), KeltaError> {
        self.steps.lock().entry(id.clone()).or_default().insert(
            step.to_owned(),
            WorkStepStatus { step: step.to_owned(), status, detail, updated_at: kelta_proto::now_rfc3339() },
        );
        Ok(())
    }

    async fn steps(&self, id: &WorkItemId) -> Result<Vec<WorkStepStatus>, KeltaError> {
        let steps = self.steps.lock();
        let Some(map) = steps.get(id) else { return Ok(Vec::new()) };
        // Saga order first, then any unknown steps.
        let mut out: Vec<WorkStepStatus> =
            kelta_proto::model::WORK_STEPS.iter().filter_map(|s| map.get(*s).cloned()).collect();
        out.extend(
            map.values().filter(|s| !kelta_proto::model::WORK_STEPS.contains(&s.step.as_str())).cloned(),
        );
        Ok(out)
    }

    async fn notes(&self, id: &WorkItemId) -> Result<Vec<ReviewNote>, KeltaError> {
        let mut out: Vec<ReviewNote> =
            self.notes.lock().iter().filter(|n| &n.work_item_id == id).cloned().collect();
        out.sort_by(|a, b| (&a.path, a.line_start, a.id).cmp(&(&b.path, b.line_start, b.id)));
        Ok(out)
    }

    async fn put_note(&self, note: &ReviewNote) -> Result<i64, KeltaError> {
        let mut notes = self.notes.lock();
        let mut note = note.clone();
        if note.id == 0 {
            note.id = notes.iter().map(|n| n.id).max().unwrap_or(0) + 1;
        }
        let id = note.id;
        notes.retain(|n| n.id != id);
        notes.push(note);
        Ok(id)
    }
}

#[derive(Default)]
pub struct MemGrantStore {
    grants: Mutex<BTreeMap<PluginId, BTreeMap<String, PluginGrant>>>,
    kv: Mutex<BTreeMap<PluginId, BTreeMap<String, String>>>,
}

impl MemGrantStore {
    pub fn new() -> Self {
        Self::default()
    }
}

#[async_trait]
impl GrantStore for MemGrantStore {
    async fn grants(&self, plugin: &PluginId) -> Result<Vec<PluginGrant>, KeltaError> {
        Ok(self.grants.lock().get(plugin).map(|m| m.values().cloned().collect()).unwrap_or_default())
    }

    async fn grant(
        &self,
        plugin: &PluginId,
        permissions: &[String],
        manifest_sha256: &str,
    ) -> Result<(), KeltaError> {
        let mut g = self.grants.lock();
        let entry = g.entry(plugin.clone()).or_default();
        for p in permissions {
            entry.insert(
                p.clone(),
                PluginGrant {
                    permission: p.clone(),
                    granted_at: kelta_proto::now_rfc3339(),
                    manifest_sha256: manifest_sha256.to_owned(),
                },
            );
        }
        for v in entry.values_mut() {
            v.manifest_sha256 = manifest_sha256.to_owned();
        }
        Ok(())
    }

    async fn revoke_all(&self, plugin: &PluginId) -> Result<(), KeltaError> {
        self.grants.lock().remove(plugin);
        Ok(())
    }

    async fn kv_get(&self, plugin: &PluginId, key: &str) -> Result<Option<String>, KeltaError> {
        Ok(self.kv.lock().get(plugin).and_then(|m| m.get(key).cloned()))
    }

    async fn kv_set(
        &self,
        plugin: &PluginId,
        key: &str,
        value: String,
        quota: usize,
    ) -> Result<(), KeltaError> {
        let mut kv = self.kv.lock();
        let m = kv.entry(plugin.clone()).or_default();
        let others: usize = m.iter().filter(|(k, _)| *k != key).map(|(k, v)| k.len() + v.len()).sum();
        if others + key.len() + value.len() > quota {
            return Err(kelta_proto::api::kv_quota_error(quota));
        }
        m.insert(key.to_owned(), value);
        Ok(())
    }

    async fn kv_delete(&self, plugin: &PluginId, key: &str) -> Result<(), KeltaError> {
        if let Some(m) = self.kv.lock().get_mut(plugin) {
            m.remove(key);
        }
        Ok(())
    }

    async fn kv_keys(&self, plugin: &PluginId) -> Result<Vec<String>, KeltaError> {
        Ok(self.kv.lock().get(plugin).map(|m| m.keys().cloned().collect()).unwrap_or_default())
    }

    async fn kv_clear(&self, plugin: &PluginId) -> Result<(), KeltaError> {
        self.kv.lock().remove(plugin);
        Ok(())
    }
}

#[derive(Default)]
pub struct MemTrustStore {
    trust: Mutex<BTreeMap<PathBuf, String>>,
}

impl MemTrustStore {
    pub fn new() -> Self {
        Self::default()
    }
}

#[async_trait]
impl TrustStore for MemTrustStore {
    async fn trusted_hash(&self, path: &Path) -> Result<Option<String>, KeltaError> {
        Ok(self.trust.lock().get(path).cloned())
    }

    async fn set_trust(&self, path: &Path, sha256: Option<String>) -> Result<(), KeltaError> {
        let mut t = self.trust.lock();
        match sha256 {
            Some(h) => {
                t.insert(path.to_path_buf(), h);
            }
            None => {
                t.remove(path);
            }
        }
        Ok(())
    }
}

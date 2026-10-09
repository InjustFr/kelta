//! ProviderRegistry: lazy per-account `Tracker`/`CodeHost` built through the two
//! `ProviderFactory`s; rebuilt when the account config changes; dropped after 30 min unused
//! (checked on the next access, no timer).

use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;
use std::time::{Duration, Instant};

use kelta_http::{HttpClient, HttpCtx, HttpPolicy, ProviderFactory};
use kelta_proto::api::{CodeHost, SecretResolver, Tracker};
use kelta_proto::error::KeltaError;
use kelta_proto::ids::AccountId;
use kelta_proto::settings::AccountConfig;
use parking_lot::Mutex;

/// Idle providers are dropped after this long without access.
pub const IDLE_DROP: Duration = Duration::from_secs(30 * 60);

struct Entry {
    cfg: AccountConfig,
    tracker: Option<Arc<dyn Tracker>>,
    code_host: Option<Arc<dyn CodeHost>>,
    last_used: Instant,
}

pub struct ProviderRegistry {
    http: HttpClient,
    trackers: Arc<dyn ProviderFactory>,
    code_hosts: Arc<dyn ProviderFactory>,
    secrets: Arc<dyn SecretResolver>,
    entries: Mutex<HashMap<AccountId, Entry>>,
}

impl ProviderRegistry {
    pub fn new(
        http: HttpClient,
        trackers: Arc<dyn ProviderFactory>,
        code_hosts: Arc<dyn ProviderFactory>,
        secrets: Arc<dyn SecretResolver>,
    ) -> Self {
        Self { http, trackers, code_hosts, secrets, entries: Mutex::new(HashMap::new()) }
    }

    fn sweep(map: &mut HashMap<AccountId, Entry>, now: Instant) {
        map.retain(|_, e| now.duration_since(e.last_used) < IDLE_DROP);
    }

    fn entry<'a>(
        map: &'a mut HashMap<AccountId, Entry>,
        id: &AccountId,
        cfg: &AccountConfig,
        now: Instant,
    ) -> &'a mut Entry {
        let stale = map.get(id).is_some_and(|e| &e.cfg != cfg);
        if stale {
            map.remove(id);
        }
        let e = map.entry(id.clone()).or_insert_with(|| Entry {
            cfg: cfg.clone(),
            tracker: None,
            code_host: None,
            last_used: now,
        });
        e.last_used = now;
        e
    }

    fn account<'a>(
        id: &AccountId,
        accounts: &'a BTreeMap<AccountId, AccountConfig>,
    ) -> Result<&'a AccountConfig, KeltaError> {
        accounts.get(id).ok_or_else(|| KeltaError::not_found(format!("account {id} is not configured")))
    }

    pub fn tracker(
        &self,
        id: &AccountId,
        accounts: &BTreeMap<AccountId, AccountConfig>,
    ) -> Result<Arc<dyn Tracker>, KeltaError> {
        let cfg = Self::account(id, accounts)?;
        let now = Instant::now();
        let mut map = self.entries.lock();
        Self::sweep(&mut map, now);
        let e = Self::entry(&mut map, id, cfg, now);
        if let Some(t) = &e.tracker {
            return Ok(t.clone());
        }
        let ctx = HttpCtx::new(self.http.clone(), id.clone(), HttpPolicy::default());
        let t = self.trackers.tracker(cfg, ctx, self.secrets.clone())?;
        e.tracker = Some(t.clone());
        Ok(t)
    }

    pub fn code_host(
        &self,
        id: &AccountId,
        accounts: &BTreeMap<AccountId, AccountConfig>,
    ) -> Result<Arc<dyn CodeHost>, KeltaError> {
        let cfg = Self::account(id, accounts)?;
        if !cfg.kind.is_code_host() {
            return Err(KeltaError::unsupported(format!("account {id} is not a code host")));
        }
        let now = Instant::now();
        let mut map = self.entries.lock();
        Self::sweep(&mut map, now);
        let e = Self::entry(&mut map, id, cfg, now);
        if let Some(c) = &e.code_host {
            return Ok(c.clone());
        }
        let ctx = HttpCtx::new(self.http.clone(), id.clone(), HttpPolicy::default());
        let c = self.code_hosts.code_host(cfg, ctx, self.secrets.clone())?;
        e.code_host = Some(c.clone());
        Ok(c)
    }

    /// Drop providers whose account changed or disappeared (settings hot reload).
    pub fn invalidate_changed(&self, accounts: &BTreeMap<AccountId, AccountConfig>) {
        self.entries.lock().retain(|id, e| accounts.get(id) == Some(&e.cfg));
    }

    /// Number of live provider entries (perf / tests).
    pub fn live(&self) -> usize {
        self.entries.lock().len()
    }
}

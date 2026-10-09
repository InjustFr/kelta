//! Trigger engine (PLUGINS §3): bus events → `on` glob → matchers → actions, with the guards of
//! ARCHITECTURE §11.3: chain depth ≤ 4, a trigger never fires twice within one chain, 10 runs / 60 s
//! per trigger, `send_keys` gate. Debounce is an event-armed one-shot per trigger (the previous one
//! is aborted). Blocking pre-events (`*.before_*`) run `blocking` triggers inline (30 s cap) and can
//! veto or patch the operation.

use std::collections::{HashMap, HashSet, VecDeque};
use std::hash::{Hash, Hasher};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use kelta_proto::error::KeltaError;
use kelta_proto::events::{BusEvent, TriggerChain, bus};
use kelta_proto::ext::{BlockingOutcome, TriggerDef, TriggerInfo, TriggerOrigin, TriggerRun};
use kelta_proto::ids::{PluginId, ProjectId};
use parking_lot::Mutex;
use serde_json::{Map, Value};
use tokio::time::Instant;

use crate::PluginHost;
use crate::actions::{ActionCx, BlockState};
use crate::context::CtxSpec;
use crate::matcher::{self, Compiled};
use crate::registry::Entry;
use crate::template::Vars;

/// Max chain depth at which a trigger may still run.
pub const MAX_CHAIN_DEPTH: u8 = 4;
/// Runs per trigger per [`RATE_WINDOW`].
pub const RATE_LIMIT: usize = 10;
pub const RATE_WINDOW: Duration = Duration::from_secs(60);
/// `send_keys`: one per session per interval.
pub const SEND_KEYS_INTERVAL: Duration = Duration::from_secs(2);
/// `trigger_log` capacity.
pub const LOG_CAP: usize = 1000;
/// Blocking triggers cap.
pub const BLOCKING_CAP: Duration = Duration::from_secs(30);
const SEEN_CAP: usize = 512;

/// A trigger with its origin (config or plugin).
#[derive(Clone)]
pub(crate) struct Resolved {
    /// Config id, or `<plugin>/<id>`.
    pub key: String,
    pub def: TriggerDef,
    pub origin: TriggerOrigin,
    pub plugin: Option<Arc<Entry>>,
}

impl Resolved {
    pub fn info(&self) -> TriggerInfo {
        TriggerInfo {
            id: self.key.clone(),
            origin: self.origin.clone(),
            on: self.def.on.clone(),
            enabled: self.def.enabled,
            description: self.def.description.clone(),
        }
    }
}

#[derive(Default)]
struct Seen {
    order: VecDeque<u64>,
    set: HashSet<u64>,
}

/// Mutable engine state.
#[derive(Default)]
pub struct Engine {
    log: Mutex<VecDeque<TriggerRun>>,
    rate: Mutex<HashMap<String, VecDeque<Instant>>>,
    send_keys: Mutex<HashMap<String, Instant>>,
    debounce: Mutex<HashMap<String, (u64, tokio::task::AbortHandle)>>,
    debounce_gen: AtomicU64,
    seen: Mutex<Seen>,
    pub(crate) prompts: Mutex<HashMap<String, crate::actions::PendingPrompt>>,
    inflight: Mutex<Vec<tokio::task::JoinHandle<()>>>,
}

fn event_key(ev: &BusEvent) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    ev.name.hash(&mut h);
    ev.ts.hash(&mut h);
    ev.project_id.hash(&mut h);
    ev.session_id.hash(&mut h);
    ev.work_item_id.hash(&mut h);
    ev.payload.to_string().hash(&mut h);
    ev.chain.hash_into(&mut h);
    h.finish()
}

trait ChainHash {
    fn hash_into(&self, h: &mut impl Hasher);
}

impl ChainHash for TriggerChain {
    fn hash_into(&self, h: &mut impl Hasher) {
        self.depth.hash(h);
        self.origin_triggers.hash(h);
    }
}

impl Engine {
    /// False when this exact event was already handled (host subscription + external fan-in).
    fn first_seen(&self, ev: &BusEvent) -> bool {
        let k = event_key(ev);
        let mut s = self.seen.lock();
        if !s.set.insert(k) {
            return false;
        }
        s.order.push_back(k);
        while s.order.len() > SEEN_CAP {
            if let Some(old) = s.order.pop_front() {
                s.set.remove(&old);
            }
        }
        true
    }

    pub(crate) fn record(&self, run: TriggerRun) {
        if run.ok {
            tracing::debug!(trigger = %run.trigger_id, event = %run.event, "trigger ran");
        } else {
            tracing::info!(trigger = %run.trigger_id, event = %run.event, detail = ?run.detail, "trigger did not complete");
        }
        let mut log = self.log.lock();
        log.push_back(run);
        while log.len() > LOG_CAP {
            log.pop_front();
        }
    }

    /// Newest first.
    pub fn log(&self, limit: usize) -> Vec<TriggerRun> {
        self.log.lock().iter().rev().take(limit.max(1)).cloned().collect()
    }

    fn rate_ok(&self, key: &str) -> bool {
        let now = Instant::now();
        let mut rate = self.rate.lock();
        let q = rate.entry(key.to_owned()).or_default();
        while q.front().is_some_and(|t| now.duration_since(*t) >= RATE_WINDOW) {
            q.pop_front();
        }
        if q.len() >= RATE_LIMIT {
            return false;
        }
        q.push_back(now);
        true
    }

    /// `send_keys` limiter: 1 per [`SEND_KEYS_INTERVAL`] per session.
    pub(crate) fn send_keys_ok(&self, session: &str) -> bool {
        let now = Instant::now();
        let mut m = self.send_keys.lock();
        if let Some(last) = m.get(session)
            && now.duration_since(*last) < SEND_KEYS_INTERVAL
        {
            return false;
        }
        m.insert(session.to_owned(), now);
        true
    }

    fn track(&self, h: tokio::task::JoinHandle<()>) {
        let mut v = self.inflight.lock();
        v.retain(|h| !h.is_finished());
        v.push(h);
    }
}

fn run_record(key: &str, ev: &BusEvent, ok: bool, detail: Option<String>) -> TriggerRun {
    TriggerRun {
        ts: kelta_proto::now_rfc3339(),
        trigger_id: key.to_owned(),
        event: ev.name.clone(),
        ok,
        detail: detail.map(|d| crate::util::truncate(&d, 2000)),
        depth: ev.chain.depth,
    }
}

fn on_matches(on: &str, name: &str) -> bool {
    on == name || matcher::glob(on).is_ok_and(|g| g.is_match(name))
}

fn compile_matchers(def: &TriggerDef) -> Result<Vec<(String, Compiled)>, KeltaError> {
    def.r#match.iter().map(|(k, m)| Ok((k.clone(), matcher::compile(m)?))).collect()
}

/// Turn an `on` glob into a concrete sample event name (`pr.*` → `pr.test`).
fn sample_name(on: &str) -> String {
    on.replace('*', "test").replace('?', "x")
}

impl PluginHost {
    /// Config triggers (effective for the project) + triggers of enabled plugins.
    pub(crate) fn resolve_triggers(&self, project: Option<&ProjectId>) -> Vec<Resolved> {
        let effective = self.settings(project);
        let global = self.settings(None);
        let mut out = Vec::new();
        for t in effective.triggers.iter().filter(|t| !t.id.is_empty()) {
            let origin = match project {
                Some(p) if !global.triggers.iter().any(|g| g == t) => {
                    TriggerOrigin::Project { project_id: p.clone() }
                }
                _ => TriggerOrigin::Global,
            };
            out.push(Resolved { key: t.id.clone(), def: t.clone(), origin, plugin: None });
        }
        for e in self.active() {
            let Some(m) = e.manifest() else { continue };
            for t in &m.contributes.triggers {
                out.push(Resolved {
                    key: format!("{}/{}", e.id, t.id),
                    def: t.clone(),
                    origin: TriggerOrigin::Plugin { plugin_id: e.id.clone() },
                    plugin: Some(e.clone()),
                });
            }
        }
        out
    }

    /// `activation` (PLUGINS §4): plugin triggers subscribe only once activated.
    fn plugin_listens(&self, entry: &Entry, event: &str) -> bool {
        let Some(m) = entry.manifest() else { return false };
        if m.activation.is_empty() || self.is_activated(&entry.id) {
            return true;
        }
        m.activation
            .iter()
            .any(|a| a == "onStartup" || a.strip_prefix("onEvent:").is_some_and(|g| on_matches(g, event)))
    }

    pub(crate) async fn handle_event(&self, ev: &BusEvent) {
        if !self.engine.first_seen(ev) {
            return;
        }
        match ev.name.as_str() {
            bus::PROJECT_CLOSED => {
                if let Some(p) = &ev.project_id {
                    self.close_project_tools(p).await;
                }
            }
            bus::PROJECT_OPENED => {
                for e in self.active() {
                    if e.manifest().is_some_and(|m| m.activation.iter().any(|a| a == "onProjectOpen")) {
                        self.mark_activated(&e.id);
                    }
                }
            }
            bus::SETTINGS_CHANGED => self.invalidate(),
            bus::LAGGED => return,
            _ => {}
        }
        self.relay_to_screens(ev).await;

        let mut vars: Option<Vars> = None;
        for t in self.resolve_triggers(ev.project_id.as_ref()) {
            if !t.def.enabled || !on_matches(&t.def.on, &ev.name) {
                continue;
            }
            if t.def.blocking && ev.is_blocking() {
                continue; // handled by run_blocking
            }
            if let Some(e) = &t.plugin
                && !self.plugin_listens(e, &ev.name)
            {
                continue;
            }
            let matchers = match compile_matchers(&t.def) {
                Ok(m) => m,
                Err(e) => {
                    self.engine.record(run_record(&t.key, ev, false, Some(e.message)));
                    continue;
                }
            };
            if vars.is_none() {
                vars = Some(self.build_vars(CtxSpec { event: Some(ev), ..Default::default() }).await);
            }
            let base = vars.clone().unwrap_or_default();
            if !matcher::matches_all(&base.as_value(), &matchers) {
                continue;
            }
            match t.def.debounce_ms.filter(|ms| *ms > 0) {
                Some(ms) => self.arm_debounce(t, ev.clone(), Duration::from_millis(ms)),
                None => {
                    let me = self.me.clone();
                    let ev = ev.clone();
                    let h = tokio::spawn(async move {
                        if let Some(host) = me.upgrade() {
                            host.run_trigger(&t, &ev, None).await;
                        }
                    });
                    self.engine.track(h);
                }
            }
        }
    }

    fn arm_debounce(&self, t: Resolved, ev: BusEvent, delay: Duration) {
        let generation = self.engine.debounce_gen.fetch_add(1, Ordering::SeqCst);
        let key = t.key.clone();
        let me = self.me.clone();
        let task_key = key.clone();
        let h = tokio::spawn(async move {
            // one-shot: trigger debounce, re-armed (previous aborted) by each matching event
            tokio::time::sleep(delay).await;
            let Some(host) = me.upgrade() else { return };
            let current = {
                let mut d = host.engine.debounce.lock();
                match d.get(&task_key) {
                    Some((g, _)) if *g == generation => {
                        d.remove(&task_key);
                        true
                    }
                    _ => false,
                }
            };
            if current {
                host.run_trigger(&t, &ev, None).await;
            }
        });
        let abort = h.abort_handle();
        if let Some((_, prev)) = self.engine.debounce.lock().insert(key, (generation, abort)) {
            prev.abort();
        }
        self.engine.track(h);
    }

    /// Guards + sequential actions + log. Returns the run record.
    pub(crate) async fn run_trigger(
        &self,
        t: &Resolved,
        ev: &BusEvent,
        block: Option<&mut BlockState>,
    ) -> TriggerRun {
        let refuse = |detail: &str| run_record(&t.key, ev, false, Some(detail.to_owned()));
        let run = if ev.chain.depth > MAX_CHAIN_DEPTH {
            refuse(&format!("depth_exceeded: chain depth {} > {MAX_CHAIN_DEPTH}", ev.chain.depth))
        } else if ev.chain.origin_triggers.iter().any(|k| k == &t.key) {
            refuse("loop: trigger already ran in this chain")
        } else if !self.engine.rate_ok(&t.key) {
            refuse(&format!("rate_limited: more than {RATE_LIMIT} runs in {} s", RATE_WINDOW.as_secs()))
        } else {
            self.execute(t, ev, block).await
        };
        self.engine.record(run.clone());
        run
    }

    async fn execute(&self, t: &Resolved, ev: &BusEvent, block: Option<&mut BlockState>) -> TriggerRun {
        let granted = match &t.plugin {
            Some(e) => match self.granted(e).await {
                Ok(g) => Some(g),
                Err(err) => return run_record(&t.key, ev, false, Some(err.message)),
            },
            None => None,
        };
        let vars = self
            .build_vars(CtxSpec { event: Some(ev), plugin: t.plugin.as_deref(), ..Default::default() })
            .await;
        let mut chain = ev.chain.clone();
        chain.depth = chain.depth.saturating_add(1);
        chain.origin_triggers.push(t.key.clone());
        let mut cx = ActionCx {
            key: t.key.clone(),
            project: ev.project_id.clone(),
            session: ev.session_id.clone(),
            event: Some(ev.clone()),
            vars,
            chain,
            granted,
            plugin: t.plugin.clone(),
            allow_send_keys: t.def.allow_send_keys,
            nested: 0,
        };
        let (ok, details) = self.run_actions(&t.def.r#do, &mut cx, t.def.continue_on_error, block).await;
        run_record(&t.key, ev, ok, (!details.is_empty()).then(|| details.join("; ")))
    }

    pub(crate) async fn blocking(&self, ev: &BusEvent) -> Result<BlockingOutcome, KeltaError> {
        if !ev.is_blocking() {
            return Ok(BlockingOutcome::Proceed { patch: None });
        }
        let mut patch = Map::new();
        let mut vars: Option<Vars> = None;
        for t in self.resolve_triggers(ev.project_id.as_ref()) {
            if !t.def.enabled || !t.def.blocking || !on_matches(&t.def.on, &ev.name) {
                continue;
            }
            let matchers = match compile_matchers(&t.def) {
                Ok(m) => m,
                Err(e) => {
                    self.engine.record(run_record(&t.key, ev, false, Some(e.message)));
                    continue;
                }
            };
            if vars.is_none() {
                vars = Some(self.build_vars(CtxSpec { event: Some(ev), ..Default::default() }).await);
            }
            if !matcher::matches_all(&vars.clone().unwrap_or_default().as_value(), &matchers) {
                continue;
            }
            let mut state = BlockState::default();
            // one-shot: 30 s cap of a blocking trigger (armed by the pre-event)
            let res = tokio::time::timeout(BLOCKING_CAP, self.run_trigger(&t, ev, Some(&mut state))).await;
            if res.is_err() {
                let reason =
                    format!("blocking trigger `{}` timed out after {} s", t.key, BLOCKING_CAP.as_secs());
                self.engine.record(run_record(&t.key, ev, false, Some(reason.clone())));
                return Ok(BlockingOutcome::Veto { trigger_id: t.key, reason });
            }
            if let Some(reason) = state.veto {
                return Ok(BlockingOutcome::Veto { trigger_id: t.key, reason });
            }
            for (k, v) in state.patch {
                patch.insert(k, v);
            }
        }
        Ok(BlockingOutcome::Proceed { patch: (!patch.is_empty()).then_some(Value::Object(patch)) })
    }

    pub(crate) async fn test_trigger(
        &self,
        trigger_id: &str,
        payload: Value,
    ) -> Result<TriggerRun, KeltaError> {
        let project = payload.get("project_id").and_then(Value::as_str).map(ProjectId::new);
        let t = self
            .resolve_triggers(project.as_ref())
            .into_iter()
            .find(|t| t.key == trigger_id)
            .ok_or_else(|| KeltaError::not_found(format!("trigger `{trigger_id}`")))?;
        let mut ev = BusEvent::new(sample_name(&t.def.on), payload.clone());
        ev.project_id = project;
        ev.session_id = payload.get("session_id").and_then(Value::as_str).map(Into::into);
        let matchers = compile_matchers(&t.def)?;
        let vars = self.build_vars(CtxSpec { event: Some(&ev), ..Default::default() }).await;
        let ctx = vars.as_value();
        if let Some((path, _)) =
            matchers.iter().find(|(p, m)| !matcher::matches_all(&ctx, &[(p.clone(), m.clone())]))
        {
            let run = run_record(&t.key, &ev, false, Some(format!("no match: `{path}`")));
            self.engine.record(run.clone());
            return Ok(run);
        }
        let mut state = BlockState::default();
        let block = (t.def.blocking && ev.is_blocking()).then_some(&mut state);
        let mut run = self.execute(&t, &ev, block).await;
        if let Some(reason) = state.veto {
            run.detail = Some(format!("veto: {reason}"));
        }
        self.engine.record(run.clone());
        Ok(run)
    }

    /// Wait for every trigger run spawned so far (tests and orderly shutdown).
    pub async fn settle(&self) {
        loop {
            let handles: Vec<_> = std::mem::take(&mut *self.engine.inflight.lock());
            if handles.is_empty() {
                return;
            }
            for h in handles {
                let _ = h.await;
            }
        }
    }

    /// Plugins with a pending trigger subscription (diagnostics).
    pub fn activated_plugins(&self) -> Vec<PluginId> {
        let mut v: Vec<PluginId> = self.activated.lock().iter().cloned().collect();
        v.sort();
        v
    }
}

//! Attention aggregation (ARCHITECTURE §5: max-aggregated) → `attention.changed`, dock badge
//! (sessions needing input) and `request_attention`.

use std::collections::BTreeMap;

use kelta_proto::events::UiEvent;
use kelta_proto::ids::ProjectId;
use kelta_proto::model::{Attention, AttentionSummary};

use crate::Core;

#[derive(Debug, Default)]
pub struct AttentionState {
    /// Last emitted summary per project.
    pub last: BTreeMap<ProjectId, AttentionSummary>,
    pub total_needs_input: u32,
}

/// Max level + needs-input count per project, and the total needs-input count.
pub fn summarize<'a>(
    sessions: impl IntoIterator<Item = (&'a ProjectId, Attention)>,
) -> (BTreeMap<ProjectId, AttentionSummary>, u32) {
    let mut out: BTreeMap<ProjectId, AttentionSummary> = BTreeMap::new();
    let mut total = 0;
    for (p, a) in sessions {
        let e = out.entry(p.clone()).or_default();
        e.level = e.level.max(a);
        if a == Attention::NeedsInput {
            e.needs_input += 1;
            total += 1;
        }
    }
    (out, total)
}

impl Core {
    /// Recompute and emit what changed.
    pub(crate) fn refresh_attention(&self) {
        let (now, total) = {
            let s = self.sessions.lock();
            let pairs: Vec<(ProjectId, Attention)> =
                s.values().map(|e| (e.info.project_id.clone(), e.info.attention)).collect();
            let (m, t) = summarize(pairs.iter().map(|(p, a)| (p, *a)));
            (m, t)
        };
        let (changed, prev_total) = {
            let mut st = self.attention.lock();
            let mut changed = Vec::new();
            let keys: Vec<ProjectId> = st.last.keys().chain(now.keys()).cloned().collect();
            for k in keys {
                let new = now.get(&k).copied().unwrap_or_default();
                let old = st.last.get(&k).copied().unwrap_or_default();
                if new != old && !changed.iter().any(|(p, _)| p == &k) {
                    changed.push((k.clone(), new));
                }
            }
            for (k, v) in &changed {
                if *v == AttentionSummary::default() {
                    st.last.remove(k);
                } else {
                    st.last.insert(k.clone(), *v);
                }
            }
            let prev = std::mem::replace(&mut st.total_needs_input, total);
            (changed, prev)
        };
        for (project_id, s) in changed {
            self.emit(UiEvent::AttentionChanged {
                project_id,
                level: s.level,
                needs_input_count: s.needs_input,
                total_needs_input: total,
            });
        }
        if total != prev_total {
            self.bridge.set_badge(total);
            if total > prev_total && !self.bridge.window_state().focused {
                self.bridge.request_attention();
            }
        }
    }

    pub fn total_needs_input(&self) -> u32 {
        self.attention.lock().total_needs_input
    }
}

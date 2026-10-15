//! Ticket steps (#461): where a ticket stands in its pipeline, moved forward only by signals;
//! entering a step runs its `transition_to` and `comment` once (skipped steps run nothing); a manual
//! move (#463) may also go back, running nothing.

use kelta_proto::codehost::{Review, ReviewDecision};
use kelta_proto::error::{ErrorCode, KeltaError};
use kelta_proto::events::{BusEvent, Toast, bus};
use kelta_proto::model::{TicketStep, WorkItem, WorkState};
use kelta_proto::settings::{PipelineEvent, StepDef, Trigger, effective_pipeline};
use kelta_proto::tracker::{Status, Ticket, TicketRef};

use crate::ops::CHOOSE_DONE;
use crate::saga::{self, Env, status_matches};
use crate::template::{Mode, render};
use crate::{WorkService, plan};

#[derive(Debug, Clone, PartialEq)]
pub enum Signal {
    Event(PipelineEvent),
    /// The tracker reports the ticket in this status (ticket poll).
    Status(Status),
    /// A manual move (#463): no trigger fires on it; its step's effects run as for an event.
    Manual,
}

fn fires(t: &Trigger, sig: &Signal) -> bool {
    match (t, sig) {
        (Trigger::Event(e), Signal::Event(s)) => e == s,
        (Trigger::Status { status }, Signal::Status(s)) => status_matches(s, status),
        _ => false,
    }
}

/// The furthest step after `current` that `sig` enters; never one at or before `current`.
pub fn advance(steps: &[StepDef], current: Option<usize>, sig: &Signal) -> Option<usize> {
    let from = current.map_or(0, |c| c + 1);
    (from..steps.len()).rev().find(|&i| steps[i].enter.iter().any(|t| fires(t, sig)))
}

/// Index of the stored step; an id gone from the config clamps to the last step its trigger (`by`)
/// still enters, logged.
pub(crate) fn current(steps: &[StepDef], row: Option<&TicketStep>) -> Option<usize> {
    let row = row?;
    if let Some(i) = steps.iter().position(|s| s.id == row.step_id) {
        return Some(i);
    }
    let i = steps
        .iter()
        .rposition(|s| s.enter.iter().any(|t| serde_json::to_string(t).is_ok_and(|j| j == row.by)));
    tracing::warn!(
        ticket = %row.ticket.key, step = %row.step_id, clamped_to = ?i.map(|i| steps[i].id.as_str()),
        "ticket step no longer in the pipeline"
    );
    i
}

/// The steps of `item`'s ticket: its source's pipeline, else its project board's (#460).
pub(crate) fn steps_of(env: &Env, item: &WorkItem) -> Vec<StepDef> {
    let src = item.ticket_source.as_deref().and_then(|id| env.settings.sources.iter().find(|s| s.id == id));
    let binding = src.map(|s| s.tracker.clone()).or_else(|| env.project.tracker.clone()).unwrap_or_default();
    effective_pipeline(&binding, &env.settings.work)
}

impl WorkService {
    /// `ticket_steps` (#462): every stored ticket step, for the UI's stage track.
    pub async fn ticket_steps(&self) -> Result<Vec<TicketStep>, KeltaError> {
        self.store.ticket_steps().await
    }

    /// Move `item`'s ticket to the furthest later step `sig` enters and run that step's effects.
    /// Returns why the ticket was not moved (the merge's phase detail).
    pub(crate) async fn signal(
        &self,
        item: &WorkItem,
        sig: Signal,
        review: Option<&Review>,
    ) -> Option<String> {
        let t = item.ticket.as_ref()?;
        let env = match self.env(&item.project_id, &item.repo_id) {
            Ok(e) => e,
            Err(e) => return Some(format!("ticket not moved: {}", e.message)),
        };
        let steps = steps_of(&env, item);
        advance(&steps, None, &sig)?;
        let _g = self.step_lock.lock().await;
        let row = self.store.ticket_step(t).await.ok().flatten();
        let next = advance(&steps, current(&steps, row.as_ref()), &sig)?;
        let step = &steps[next];
        let by = step
            .enter
            .iter()
            .find(|x| fires(x, &sig))
            .and_then(|x| serde_json::to_string(x).ok())
            .unwrap_or_default();
        // The Start saga ran the plan's copy of the start effects, which the sheet lets the user edit.
        let why = if sig == Signal::Event(PipelineEvent::WorkStarted) {
            None
        } else {
            self.enter_effects(&env, item, step, &sig, review).await
        };
        self.put_step(&env, item, t, step, by).await;
        why
    }

    /// Manual move (#463, Tauri `ticket_step_set`, MCP `move_ticket_step`): to `step_id`, or to the step
    /// after the current one (`None`, Next step). Forward runs the entered step's effects as a signal
    /// does; back is a correction that only stores the step (`by` = `manual`).
    pub async fn move_step(
        &self,
        ticket: &TicketRef,
        step_id: Option<&str>,
    ) -> Result<TicketStep, KeltaError> {
        let items = self.store.list_items(None).await?;
        // Its unfinished item (newest first), else its last finished one: the journal and steps live there.
        let item = items
            .iter()
            .filter(|w| w.ticket.as_ref().is_some_and(|x| x.account == ticket.account && x.key == ticket.key))
            .min_by_key(|w| w.state == WorkState::Finished)
            .ok_or_else(|| {
                KeltaError::not_found(format!("{} has no work item: start work on it first", ticket.key))
            })?;
        let env = self.env(&item.project_id, &item.repo_id)?;
        let steps = steps_of(&env, item);
        let _g = self.step_lock.lock().await;
        let row = self.store.ticket_step(ticket).await?;
        let at = current(&steps, row.as_ref());
        let next = match step_id {
            Some(id) => steps.iter().position(|s| s.id == id).ok_or_else(|| {
                let ids: Vec<&str> = steps.iter().map(|s| s.id.as_str()).collect();
                KeltaError::invalid(format!("unknown step `{id}`; the steps are {}", ids.join(", ")))
            })?,
            None => at.map_or(0, |c| c + 1),
        };
        let Some(step) = steps.get(next) else {
            return Err(KeltaError::invalid(format!("{} is at the last step", ticket.key)));
        };
        if let Some(r) = row.filter(|_| at == Some(next)) {
            return Ok(r);
        }
        if at.is_none_or(|c| next > c)
            && let Some(why) = self.enter_effects(&env, item, step, &Signal::Manual, None).await
        {
            env.core.toast(Toast::warn(format!("{}: {why}", ticket.key)));
        }
        Ok(self.put_step(&env, item, ticket, step, "manual".into()).await)
    }

    /// Stores `t` at `step` and publishes `ticket.step`.
    async fn put_step(
        &self,
        env: &Env,
        item: &WorkItem,
        t: &TicketRef,
        step: &StepDef,
        by: String,
    ) -> TicketStep {
        let row = TicketStep {
            ticket: t.clone(),
            source_id: item.ticket_source.clone(),
            step_id: step.id.clone(),
            entered_at: kelta_proto::now_rfc3339(),
            by,
        };
        if let Err(e) = self.store.put_ticket_step(&row).await {
            tracing::warn!(error = %e.message, ticket = %t.key, "ticket step not stored");
        }
        env.core.publish(
            BusEvent::new(bus::TICKET_STEP, serde_json::json!(row))
                .with_project(item.project_id.clone())
                .with_work_item(item.id.clone()),
        );
        row
    }

    /// The transition (not for a status signal: the tracker is already there) and comment of `step`,
    /// each once per (ticket, step) through the item's journal.
    async fn enter_effects(
        &self,
        env: &Env,
        item: &WorkItem,
        step: &StepDef,
        sig: &Signal,
        review: Option<&Review>,
    ) -> Option<String> {
        let t = item.ticket.as_ref()?;
        let mut j = self.load_journal(&item.id);
        let (tk, ck) = (format!("{}:transition", step.id), format!("{}:comment", step.id));
        let target = step
            .transition_to
            .as_ref()
            .filter(|_| !matches!(sig, Signal::Status(_)) && !j.effects_done.contains(&tk));
        let mut ctx = self.item_ctx(env, item, &j);
        if let Some(r) = review {
            plan::add_review(&mut ctx, r);
        }
        let comment = step
            .comment
            .as_ref()
            .filter(|_| !j.effects_done.contains(&ck))
            .and_then(|c| render(c, &ctx, Mode::Lenient).ok())
            .filter(|c| !c.trim().is_empty());
        if target.is_none() && comment.is_none() {
            return None;
        }
        let tracker = match env.core.tracker_for(&t.account).await {
            Ok(x) => x,
            Err(e) => return Some(format!("ticket not moved: {}", e.message)),
        };
        let mut why = None;
        if let Some(target) = target {
            // Never guess between Done statuses on merge (FLOW §4.6).
            let strict = *sig == Signal::Event(PipelineEvent::Merged);
            match saga::apply_transition(
                &env.core,
                tracker.as_ref(),
                t,
                target,
                &item.project_id,
                &item.id,
                strict,
            )
            .await
            {
                Ok(_) => {
                    j.effects_done.insert(tk);
                }
                Err(e) if strict && e.code == ErrorCode::Conflict => why = Some(CHOOSE_DONE.into()),
                Err(e) if strict => why = Some(format!("ticket not moved: {}", e.message)),
                Err(e) => {
                    env.core.toast(Toast::warn(format!("{}: transition failed — {}", t.key, e.message)));
                }
            }
        }
        if let Some(body) = comment {
            // At most once: journaled before posting, as the Start saga does.
            j.effects_done.insert(ck);
            if let Err(e) = self.save_journal_fresh(&item.id, &mut j) {
                tracing::warn!(error = %e.message, "step comment journal not saved");
            }
            match tracker.comment(t, &body).await {
                Ok(()) => env.core.publish(
                    BusEvent::new(
                        bus::TICKET_COMMENTED,
                        serde_json::json!({ "ticket": t, "markdown": body }),
                    )
                    .with_project(item.project_id.clone())
                    .with_work_item(item.id.clone()),
                ),
                Err(e) => env.core.toast(Toast::warn(format!("{}: comment failed — {}", t.key, e.message))),
            }
        }
        if let Err(e) = self.save_journal_fresh(&item.id, &mut j) {
            tracing::warn!(error = %e.message, "step effects journal not saved");
        }
        why
    }

    /// `pr.updated` (#461): a PR marked ready, a review requested, changes requested or approved
    /// signals the tickets of the items on that PR.
    pub(crate) async fn pr_updated(&self, review: &Review, changes: &[String]) {
        let has = |c: &str| changes.iter().any(|x| x == c);
        let mut sigs = Vec::new();
        if has("draft") && !review.draft {
            sigs.push(PipelineEvent::PrReady);
        }
        if has("reviewers") && !review.waiting_on.is_empty() {
            sigs.push(PipelineEvent::ReviewRequested);
        }
        if has("decision") {
            match review.decision {
                Some(ReviewDecision::ChangesRequested) => sigs.push(PipelineEvent::ChangesRequested),
                Some(ReviewDecision::Approved) => sigs.push(PipelineEvent::Approved),
                _ => {}
            }
        }
        if sigs.is_empty() {
            return;
        }
        let items = self.store.list_items(None).await.unwrap_or_default();
        let on_pr = |w: &&WorkItem| {
            w.pr_url.as_deref() == Some(review.url.as_str())
                && w.state != WorkState::Finished
                && !w.state.pr_done()
        };
        for w in items.iter().filter(on_pr) {
            for e in &sigs {
                self.signal(w, Signal::Event(*e), Some(review)).await;
            }
        }
    }

    /// The ticket poll (#461): each listed ticket with unfinished work enters a step its status
    /// triggers. No transition runs, since the tracker is already there.
    pub async fn ticket_statuses(&self, tickets: &[Ticket]) {
        let Ok(items) = self.store.list_items(None).await else { return };
        for t in tickets {
            let work = items.iter().find(|w| {
                w.state != WorkState::Finished
                    && w.ticket.as_ref().is_some_and(|x| x.account == t.r#ref.account && x.key == t.r#ref.key)
            });
            if let Some(w) = work {
                self.signal(w, Signal::Status(t.status.clone()), None).await;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use kelta_proto::samples::{self, status};
    use kelta_proto::settings::TransitionTarget;
    use kelta_proto::tracker::StatusCategory;

    use super::*;

    fn step(id: &str, enter: Vec<Trigger>) -> StepDef {
        StepDef { id: id.into(), label: id.into(), enter, ..Default::default() }
    }

    fn ev(e: PipelineEvent) -> Trigger {
        Trigger::Event(e)
    }

    fn steps() -> Vec<StepDef> {
        use PipelineEvent::*;
        vec![
            step("implementing", vec![ev(WorkStarted)]),
            step("pr", vec![ev(PrOpened)]),
            step("review", vec![ev(PrReady), ev(ReviewRequested)]),
            step("po", vec![Trigger::Status { status: TransitionTarget::Name { name: "PO Review".into() } }]),
            step("done", vec![ev(Merged)]),
        ]
    }

    fn sig(e: PipelineEvent) -> Signal {
        Signal::Event(e)
    }

    #[test]
    fn advance_moves_forward_only() {
        assert_eq!(advance(&steps(), None, &sig(PipelineEvent::PrOpened)), Some(1));
        assert_eq!(advance(&steps(), Some(1), &sig(PipelineEvent::PrOpened)), None);
        assert_eq!(advance(&steps(), Some(2), &sig(PipelineEvent::WorkStarted)), None);
    }

    #[test]
    fn advance_skips_ahead_to_the_furthest_match() {
        assert_eq!(advance(&steps(), Some(0), &sig(PipelineEvent::Merged)), Some(4));
        let mut s = steps();
        s.push(step("later", vec![ev(PipelineEvent::PrReady)]));
        assert_eq!(advance(&s, Some(1), &sig(PipelineEvent::PrReady)), Some(5));
    }

    #[test]
    fn a_step_with_several_triggers() {
        assert_eq!(advance(&steps(), Some(1), &sig(PipelineEvent::PrReady)), Some(2));
        assert_eq!(advance(&steps(), Some(1), &sig(PipelineEvent::ReviewRequested)), Some(2));
    }

    #[test]
    fn a_status_trigger() {
        let po = Signal::Status(status("12", "po review", StatusCategory::InReview));
        assert_eq!(advance(&steps(), Some(2), &po), Some(3));
        let other = Signal::Status(status("13", "In Review", StatusCategory::InReview));
        assert_eq!(advance(&steps(), Some(2), &other), None);
        assert_eq!(advance(&steps(), Some(2), &sig(PipelineEvent::PrReady)), None);
    }

    #[test]
    fn no_match() {
        assert_eq!(advance(&steps(), None, &sig(PipelineEvent::ChangesRequested)), None);
    }

    #[test]
    fn a_removed_step_clamps_to_the_last_step_its_trigger_enters() {
        let row = |id: &str, by: &str| TicketStep {
            ticket: samples::ticket_ref(),
            source_id: None,
            step_id: id.into(),
            entered_at: String::new(),
            by: by.into(),
        };
        let by = serde_json::to_string(&ev(PipelineEvent::PrOpened)).unwrap();
        assert_eq!(current(&steps(), Some(&row("gone", &by))), Some(1));
        assert_eq!(current(&steps(), Some(&row("gone", "\"nope\""))), None);
        assert_eq!(current(&steps(), Some(&row("review", &by))), Some(2));
        assert_eq!(current(&steps(), None), None);
    }
}

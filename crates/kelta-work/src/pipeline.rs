//! Ticket steps (#461): where a ticket stands in its pipeline, moved forward only by signals;
//! entering a step runs its `transition_to` and `comment` once (skipped steps run nothing); a manual
//! move (#463) may also go back, running nothing. A removed step clamps forward and a Start after
//! Done restarts the flow (#466).

use kelta_proto::codehost::{Review, ReviewDecision};
use kelta_proto::error::{ErrorCode, KeltaError};
use kelta_proto::events::{BusEvent, Toast, bus};
use kelta_proto::model::{ProjectInfo, TicketStep, WorkItem, WorkState};
use kelta_proto::settings::{PipelineEvent, Settings, StepDef, Trigger, effective_pipeline};
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
/// still enters, else to the last step (never back, #466), logged.
pub(crate) fn current(steps: &[StepDef], row: Option<&TicketStep>) -> Option<usize> {
    let row = row?;
    if let Some(i) = steps.iter().position(|s| s.id == row.step_id) {
        return Some(i);
    }
    let i = steps
        .iter()
        .rposition(|s| s.enter.iter().any(|t| serde_json::to_string(t).is_ok_and(|j| j == row.by)))
        .or(steps.len().checked_sub(1));
    tracing::warn!(
        ticket = %row.ticket.key, step = %row.step_id, clamped_to = ?i.map(|i| steps[i].id.as_str()),
        "ticket step no longer in the pipeline"
    );
    i
}

/// The steps of a ticket from `source`: its pipeline, else the project board's (#460).
pub(crate) fn steps_for(settings: &Settings, project: &ProjectInfo, source: Option<&str>) -> Vec<StepDef> {
    let src = source.and_then(|id| settings.sources.iter().find(|s| s.id == id));
    let binding = src.map(|s| s.tracker.clone()).or_else(|| project.tracker.clone()).unwrap_or_default();
    effective_pipeline(&binding, &settings.work)
}

/// The steps of `item`'s ticket.
pub(crate) fn steps_of(env: &Env, item: &WorkItem) -> Vec<StepDef> {
    steps_for(&env.settings, &env.project, item.ticket_source.as_deref())
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
        let lock = self.step_lock(t);
        let _g = lock.lock().await;
        let next = advance(&steps, self.standing(&env, item, t, &steps, &sig).await, &sig)?;
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
    /// does, stores the step even when its transition fails and returns that failure as an error
    /// (moving there again retries it, #466); back is a correction that only stores the step (`by` = `manual`).
    pub async fn move_step(
        &self,
        ticket: &TicketRef,
        step_id: Option<&str>,
    ) -> Result<TicketStep, KeltaError> {
        let items = self.store.list_items(None).await?;
        // Its newest unfinished item (the UI's `ticketWork`, #466): the journal and steps live there.
        let mine: Vec<&WorkItem> = items
            .iter()
            .filter(|w| {
                w.state != WorkState::Finished
                    && w.ticket.as_ref().is_some_and(|x| x.account == ticket.account && x.key == ticket.key)
            })
            .collect();
        let item = mine.iter().copied().max_by(|a, b| a.created_at.cmp(&b.created_at)).ok_or_else(|| {
            KeltaError::not_found(format!(
                "{} has no unfinished work item: start work on it first",
                ticket.key
            ))
        })?;
        let env = self.env(&item.project_id, &item.repo_id)?;
        let steps = steps_of(&env, item);
        let lock = self.step_lock(ticket);
        let _g = lock.lock().await;
        let at = self.standing(&env, item, ticket, &steps, &Signal::Manual).await;
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
        // The step `work_started` enters runs its effects in the Start saga only, as `signal` (#466).
        let start = advance(&steps, None, &Signal::Event(PipelineEvent::WorkStarted)) == Some(next);
        // A transition that failed is retried by moving to its step again (#466): only the `<step>:failed`
        // marker of the item that tried it says so (not a missing `:transition`, which a sibling or a
        // status entry never journals); that item retries it, so its journal keeps the comment once.
        let fkey = format!("{}:failed", step.id);
        let failed =
            mine.iter().copied().find(|w| !start && self.load_journal(&w.id).effects_done.contains(&fkey));
        if at == Some(next)
            && failed.is_none()
            && let Some(r) = self.store.ticket_step(ticket).await?
        {
            return Ok(r);
        }
        let why = match at {
            Some(c) if next < c => None,
            _ if start => None,
            _ => self.enter_effects(&env, failed.unwrap_or(item), step, &Signal::Manual, None).await,
        };
        let row = self.put_step(&env, item, ticket, step, "manual".into()).await;
        match why {
            Some(why) => Err(KeltaError::upstream(format!("{}: {why}", ticket.key))),
            None => Ok(row),
        }
    }

    /// Where `t` stands (`current`); a clamped row is stored back so it warns once. A Start on a ticket
    /// at its last step whose other items are all past their PR restarts it (`None`, #466).
    pub(crate) async fn standing(
        &self,
        env: &Env,
        item: &WorkItem,
        t: &TicketRef,
        steps: &[StepDef],
        sig: &Signal,
    ) -> Option<usize> {
        let row = self.store.ticket_step(t).await.ok().flatten();
        let at = current(steps, row.as_ref());
        if let (Some(r), Some(i)) = (&row, at)
            && r.step_id != steps[i].id
        {
            self.put_step(env, item, t, &steps[i], r.by.clone()).await;
        }
        if *sig == Signal::Event(PipelineEvent::WorkStarted)
            && at.is_some()
            && at == steps.len().checked_sub(1)
        {
            let all = self.store.list_items(None).await.unwrap_or_default();
            if plan::siblings(&all, item).all(|w| w.state.pr_done()) {
                return None;
            }
        }
        at
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
        let (tk, ck, fk) = (
            format!("{}:transition", step.id),
            format!("{}:comment", step.id),
            format!("{}:failed", step.id),
        );
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
                    j.effects_done.remove(&fk);
                    j.effects_done.insert(tk);
                }
                Err(e) if strict && e.code == ErrorCode::Conflict => why = Some(CHOOSE_DONE.into()),
                Err(e) if strict => why = Some(format!("ticket not moved: {}", e.message)),
                Err(e) => {
                    let msg = format!("transition failed — {}", e.message);
                    // A manual move returns it to its caller instead (MCP `move_ticket_step`, #466).
                    if *sig != Signal::Manual {
                        env.core.toast(Toast::warn(format!("{}: {msg}", t.key)));
                    }
                    j.effects_done.insert(fk);
                    why = Some(msg);
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
                // Only a changed status signals (#466): an unchanged one would undo a manual move back.
                // shortcut: last seen is in memory, so the first poll after launch signals once; persist it if that bites.
                let key = (t.r#ref.account.clone(), t.r#ref.key.clone());
                let prev = self.seen_status.lock().insert(key, t.status.id.clone());
                if prev.as_deref() != Some(t.status.id.as_str()) {
                    self.signal(w, Signal::Status(t.status.clone()), None).await;
                }
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
    fn a_removed_step_clamps_to_its_trigger_else_the_last_step() {
        let row = |id: &str, by: &str| TicketStep {
            ticket: samples::ticket_ref(),
            source_id: None,
            step_id: id.into(),
            entered_at: String::new(),
            by: by.into(),
        };
        let by = serde_json::to_string(&ev(PipelineEvent::PrOpened)).unwrap();
        assert_eq!(current(&steps(), Some(&row("gone", &by))), Some(1));
        assert_eq!(current(&steps(), Some(&row("gone", "\"nope\""))), Some(4));
        assert_eq!(
            current(&steps(), Some(&row("gone", "manual"))),
            Some(4),
            "a manual row clamps to the last step"
        );
        assert_eq!(current(&[], Some(&row("gone", "manual"))), None);
        assert_eq!(current(&steps(), Some(&row("review", &by))), Some(2));
        assert_eq!(current(&steps(), None), None);
    }
}

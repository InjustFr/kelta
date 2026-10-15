import { t } from '$lib/i18n/index.svelte';
import type { StepDef, TicketRef, TrackerBinding, WorkItem } from '$lib/gen';
import { ticketStepSet } from '$lib/ipc/commands';
import { projects, settings, toasts, ui } from '$lib/stores';

import { effectivePipeline } from '../settings/lib/pipeline';
import { ticketWork } from '../work/startWork';

// `TrackerBinding::default()`: no board, the default steps.
const NO_BOARD: TrackerBinding = {
  account: '',
  views: [],
  columns: [],
  status_map: { start: null, review: null, done: null },
  repo_rules: [],
};

/** The steps of `item`'s ticket: its source's pipeline, else its project board's (pipeline.rs `steps_of`). */
export function stepsOf(item: WorkItem): StepDef[] {
  const s = settings.value(item.project_id);
  if (!s) return [];
  const src = s.sources.find((x) => x.id === item.ticket_source);
  return effectivePipeline(src?.tracker ?? projects.byId(item.project_id)?.tracker ?? NO_BOARD, s.work);
}

/** Moves `ticket` to `stepId`, or to the step after its current one (`null`, Next step). */
export async function moveStep(ticket: TicketRef, stepId: string | null): Promise<void> {
  try {
    const row = await ticketStepSet({ ticket, step_id: stepId });
    const item = ticketWork(ticket)[0];
    const label = (item ? stepsOf(item) : []).find((s) => s.id === row.step_id)?.label ?? row.step_id;
    toasts.info(t('{key} moved to step {step}', { key: ticket.key, step: label }));
  } catch (err) {
    toasts.error(err, stepId === null ? t('Next step') : t('Move to step…'));
  }
}

/** Move to step…: a picker of the ticket's pipeline steps (the backend needs a work item too). */
export function pickStep(ticket: TicketRef): void {
  const item = ticketWork(ticket)[0];
  if (!item) return void toasts.info(t('No work on this ticket yet. Start work first.'));
  ui.openSheet('tickets.step', { ticket, steps: stepsOf(item) });
}

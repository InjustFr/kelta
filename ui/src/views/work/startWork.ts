// Entry points of the start-work flow (SPEC §3.1): plan → (sheet | direct start | resume).

import type {
  ProjectId,
  ReviewRef,
  StartWorkPlan,
  TicketRef,
  ToastAction,
  WorkItem,
  WorkSource,
} from '$lib/gen';
import * as ipc from '$lib/ipc/commands';
import { toIpcError } from '$lib/ipc/transport';
import { settings, toasts, ui, work } from '$lib/stores';

import { activateProject } from '../../shell/nav';
import { withCriteria } from '../tickets/refine.svelte';

/** True when the plan needs a decision from the user (repo, guessed project, existing branch or item). */
export function needsChoice(plan: StartWorkPlan): boolean {
  return (
    plan.existing !== null ||
    plan.branch_exists !== null ||
    plan.repo_choices.length > 1 ||
    (plan.project_guessed && plan.project_choices.length > 1)
  );
}

/** Options of a start: `preview` overrides `work.plan_preview` (`false` = start with no sheet);
 * `source` is the ticket's source as its row lists it (`TicketItem.source_ids`). */
export interface StartOpts {
  preview?: boolean;
  source?: string | null;
}

/** Requests a plan and shows the sheet, or starts/resumes directly when previews are off.
 * `projectId` is the item's own project, null when it has none: never the active project (SPEC §3.1). */
export async function beginStartWork(
  projectId: ProjectId | null,
  source: WorkSource,
  opts: StartOpts = {},
): Promise<WorkItem | null> {
  try {
    const plan = await ipc.workPlan({ project_id: projectId, source, source_id: opts.source ?? null });
    if (source.kind === 'ticket') withCriteria(plan, source.ticket);
    const preview = opts.preview ?? settings.value()?.work.plan_preview ?? true;
    if (preview || (needsChoice(plan) && plan.existing === null)) {
      ui.openSheet('start_work', { plan });
      return null;
    }
    const item = plan.existing ? await ipc.workResume({ id: plan.existing }) : await ipc.workStart({ plan });
    work.upsert(item);
    const verb = plan.existing ? 'Resumed' : item.state.kind === 'queued' ? 'Queued' : 'Started';
    toasts.info(`${verb} ${item.branch}`);
    void activateProject(item.project_id);
    return item;
  } catch (err) {
    const e = toIpcError('work_plan', err);
    const action = (e.detail as { action?: ToastAction } | null)?.action ?? null;
    if (action) toasts.push({ level: 'error', text: e.message, action });
    else toasts.error(err, 'Start work');
    return null;
  }
}

export function startWorkOnTicket(
  ref: TicketRef,
  projectId?: ProjectId | null,
  opts: StartOpts = {},
): Promise<WorkItem | null> {
  return beginStartWork(projectId ?? null, { kind: 'ticket', ticket: ref }, opts);
}

/** "Review locally": work_plan with a Review source, same sheet as start work. */
export function reviewLocally(ref: ReviewRef, projectId?: ProjectId | null): Promise<WorkItem | null> {
  return beginStartWork(projectId ?? null, { kind: 'review', review: ref });
}

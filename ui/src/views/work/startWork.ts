// Entry points of the start-work flow (SPEC §3.1): plan → (sheet | direct start | resume).

import type { ProjectId, ReviewRef, StartWorkPlan, TicketRef, WorkItem, WorkSource } from '$lib/gen';
import * as ipc from '$lib/ipc/commands';
import { projects, reviews, settings, tickets, toasts, ui, work } from '$lib/stores';

import { activateProject } from '../../shell/nav';
import { withCriteria } from '../tickets/refine.svelte';

/** Kelta project a ticket belongs to: first matching project of any loaded list, else the active one. */
export function projectForTicket(ref: TicketRef): ProjectId | null {
  for (const list of Object.values(tickets.lists)) {
    const hit = list.data?.items.find(
      (i) => i.ticket.ref.account === ref.account && i.ticket.ref.key === ref.key,
    );
    if (hit && hit.project_ids[0]) return hit.project_ids[0];
  }
  return projects.activeId;
}

/** True when the plan needs a decision from the user (repo, existing branch, existing work item). */
export function needsChoice(plan: StartWorkPlan): boolean {
  return plan.existing !== null || plan.branch_exists !== null || plan.repo_choices.length > 1;
}

/** Options of a start: `preview` overrides `work.plan_preview` (`false` = start with no sheet). */
export interface StartOpts {
  preview?: boolean;
}

/** Requests a plan and shows the sheet, or starts/resumes directly when previews are off. */
export async function beginStartWork(
  projectId: ProjectId,
  source: WorkSource,
  opts: StartOpts = {},
): Promise<WorkItem | null> {
  try {
    const plan = await ipc.workPlan({ project_id: projectId, source });
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
    toasts.error(err, 'Start work');
    return null;
  }
}

export function startWorkOnTicket(
  ref: TicketRef,
  projectId?: ProjectId | null,
  opts: StartOpts = {},
): Promise<WorkItem | null> {
  const pid = projectId ?? projectForTicket(ref);
  if (!pid) {
    toasts.warn('Open a project first to start work on a ticket');
    return Promise.resolve(null);
  }
  return beginStartWork(pid, { kind: 'ticket', ticket: ref }, opts);
}

/** Kelta project a review belongs to (first match of any loaded list), else `fallback`/the active one. */
export function projectForReview(ref: ReviewRef, fallback?: ProjectId | null): ProjectId | null {
  for (const list of Object.values(reviews.lists)) {
    const hit = list.data?.items.find(
      (i) =>
        i.review.ref.account === ref.account &&
        i.review.ref.repo === ref.repo &&
        i.review.ref.number === ref.number,
    );
    if (hit && hit.project_ids[0]) return hit.project_ids[0];
  }
  return fallback ?? projects.activeId;
}

/** "Review locally": work_plan with a Review source, same sheet as start work. */
export function reviewLocally(ref: ReviewRef, fallback?: ProjectId | null): Promise<WorkItem | null> {
  const pid = projectForReview(ref, fallback);
  if (!pid) {
    toasts.warn('Open a project first to review locally');
    return Promise.resolve(null);
  }
  return beginStartWork(pid, { kind: 'review', review: ref });
}

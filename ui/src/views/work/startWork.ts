// Entry points of the start-work flow (SPEC §3.1): plan → (sheet | direct start | resume).

import { msg, t } from '$lib/i18n/index.svelte';
import type {
  ProjectId,
  ReviewRef,
  StartWorkPlan,
  StatusCategory,
  TicketRef,
  ToastAction,
  TransitionTarget,
  WorkItem,
  WorkSource,
} from '$lib/gen';
import * as ipc from '$lib/ipc/commands';
import { toIpcError } from '$lib/ipc/transport';
import { settings, toasts, ui, work } from '$lib/stores';

import { activateProject } from '../../shell/nav';
import { withCriteria } from '../tickets/refine.svelte';
import { finishFirst } from './finishFirst';

/** True when the plan needs a decision from the user (repo, guessed project, existing branch or item). */
export function needsChoice(plan: StartWorkPlan): boolean {
  return (
    plan.existing !== null ||
    plan.branch_exists !== null ||
    plan.repo_choices.length > 1 ||
    (plan.project_guessed && plan.project_choices.length > 1)
  );
}

const CATEGORY: Record<StatusCategory, string> = {
  todo: msg('to do'),
  in_progress: msg('in progress'),
  in_review: msg('in review'),
  done: msg('done'),
  unknown: msg('unknown'),
};

/** The status a start moves the ticket to, as the sheet names it. */
export function describeTransition(to: TransitionTarget | null): string {
  if (!to) return t('next status');
  return 'category' in to ? t(CATEGORY[to.category]) : to.name;
}

/** What Start work will do (#418), in one sentence from the plan the sheet opens on. `editor`: the
 *  default editor's label, Claude's neighbour in the work tab. */
export function startPreview(plan: StartWorkPlan, editor: string | null): string {
  const { branch, repo_id: repo } = plan;
  if (plan.existing) return t('Start work resumes {branch} in {repo}.', { branch, repo });
  const fx = plan.side_effects;
  const steps = [
    plan.branch_exists?.choice === 'reuse'
      ? t('reuses branch {branch} in {repo}', { branch, repo })
      : t('creates branch {branch} in {repo}', { branch, repo }),
    fx.transition_to && t('moves the ticket to {status}', { status: describeTransition(fx.transition_to) }),
    fx.assign_me && t('assigns it to you'),
  ].filter(Boolean);
  return editor
    ? t('Start work {steps}, then briefs Claude next to {editor}.', { steps: steps.join(', '), editor })
    : t('Start work {steps}, then briefs Claude.', { steps: steps.join(', ') });
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
    // Unfinished work to list (#354) shows the sheet even with previews off.
    if (preview || (needsChoice(plan) && plan.existing === null) || finishFirst(plan).length > 0) {
      ui.openSheet('start_work', { plan });
      return null;
    }
    const item = plan.existing ? await ipc.workResume({ id: plan.existing }) : await ipc.workStart({ plan });
    work.upsert(item);
    const branch = item.branch;
    toasts.info(
      plan.existing
        ? t('Resumed {branch}', { branch })
        : item.state.kind === 'queued'
          ? t('Queued {branch}', { branch })
          : t('Started {branch}', { branch }),
      { view: null }, // activating the project leaves this view: the toast must outlive that
    );
    void activateProject(item.project_id);
    return item;
  } catch (err) {
    const e = toIpcError('work_plan', err);
    const action = (e.detail as { action?: ToastAction } | null)?.action ?? null;
    if (action) toasts.push({ level: 'error', text: e.message, action });
    else toasts.error(err, t('Start work'));
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

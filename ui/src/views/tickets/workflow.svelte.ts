// Run a workflow on a ticket (#233): `w` in the detail and the list, and the palette verb open the
// picker; the run claims the ticket and starts Claude in a background tab. Config workflows (#355)
// also run on a PR (`w` in Reviews) or with no ticket (the palette).

import { msg, t } from '$lib/i18n/index.svelte';
import { dispatch } from '$lib/actions';
import type { ProjectId, ReviewRef, TicketRef, WorkflowInfo, WorkflowOn, WorkflowStart } from '$lib/gen';
import { workflowRun, workflowRunOn, workflowsList } from '$lib/ipc/commands';
import { toIpcError } from '$lib/ipc/transport';
import { settings, toasts, ui } from '$lib/stores';
import { ticketKey } from '$lib/stores/tickets.svelte';

export const NO_WORKFLOWS = msg('No workflows in .claude/workflows or ~/.claude/workflows');
export const NO_PR_WORKFLOWS = msg('No [[workflows]] with on = "pr" in the config');

/** The enabled `[[workflows]]` offered `on`, as the picker lists them. */
export function configWorkflows(on: WorkflowOn): WorkflowInfo[] {
  return (settings.value()?.workflows ?? [])
    .filter((d) => d.enabled && d.on === on)
    .map((d) => ({
      name: d.id,
      label: d.label || d.id,
      description: d.description ?? '',
      path: null,
      source: 'config',
    }));
}

/** Per ticket key: its workflows, or why none can run (no project). Read again by each picker. */
export const workflows = $state<Record<string, WorkflowInfo[] | string>>({});

/** Why the workflow action is off for a loaded list (null = it can run, or not loaded yet). */
export function workflowReason(list: WorkflowInfo[] | string | undefined): string | null {
  if (typeof list === 'string') return list;
  return list?.length === 0 ? t(NO_WORKFLOWS) : null;
}

export async function loadWorkflows(ref: TicketRef): Promise<WorkflowInfo[] | string> {
  let list: WorkflowInfo[] | string;
  try {
    list = await workflowsList({ ticket: ref });
  } catch (err) {
    list = toIpcError('workflows_list', err).message;
  }
  workflows[ticketKey(ref)] = list;
  // Only grows with the tickets visited: past 200 the oldest entry goes.
  const keys = Object.keys(workflows);
  if (keys.length > 200) delete workflows[keys[0]!];
  return list;
}

/** Opens the picker on a fresh list, or says why there is nothing to run. */
export async function pickWorkflow(ref: TicketRef): Promise<void> {
  const list = await loadWorkflows(ref);
  const reason = workflowReason(list);
  if (reason) toasts.info(reason);
  else ui.openSheet('workflow_run', { ticket: ref, workflows: list });
}

export async function runWorkflow(ref: TicketRef, workflow: string): Promise<void> {
  try {
    announce(
      await workflowRun({ ticket: ref, workflow }),
      t('{workflow} started on {key}', { workflow, key: ref.key }),
    );
  } catch (err) {
    // Held by a work item that is not a run: the backend names the holder (`KEY is held by …`).
    toasts.error(err);
  }
}

/** Opens the picker on the PR's workflows, or says there are none. */
export function pickPrWorkflow(review: ReviewRef, projectId: ProjectId | null): void {
  const list = configWorkflows('pr');
  if (list.length === 0) toasts.info(t(NO_PR_WORKFLOWS));
  else ui.openSheet('workflow_run', { review, project_id: projectId, workflows: list });
}

/** A `pr` workflow on `review`, or a `global` one with none. */
export async function runWorkflowOn(
  projectId: ProjectId,
  w: WorkflowInfo,
  review: ReviewRef | null,
): Promise<void> {
  try {
    const start = await workflowRunOn({ project_id: projectId, workflow: w.name, review });
    const on = review ? `${review.repo}#${review.number}` : null;
    announce(
      start,
      on
        ? t('{workflow} started on {key}', { workflow: w.label, key: on })
        : t('{workflow} started', { workflow: w.label }),
    );
  } catch (err) {
    toasts.error(err);
  }
}

/** A new run: a toast to open or peek it. One already running on the ticket or PR: opened. */
function announce(start: WorkflowStart, text: string): void {
  const args = { session_id: start.session.id };
  if (start.running) {
    toasts.info(t('A workflow is already running here: opened it'));
    void dispatch('session.reveal', args);
    return;
  }
  toasts.push({
    level: 'info',
    text,
    action: { label: t('Open'), command: 'session.reveal', args },
    secondary: { label: t('Peek'), command: 'session.peek', args },
  });
}

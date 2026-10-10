// Run a saved Claude workflow on a ticket (#233): `w` in the detail and the list, and the palette
// verb open the picker; the run claims the ticket and starts Claude in a background tab.

import type { TicketRef, WorkflowInfo } from '$lib/gen';
import { workflowRun, workflowsList } from '$lib/ipc/commands';
import { toIpcError } from '$lib/ipc/transport';
import { toasts, ui } from '$lib/stores';
import { ticketKey } from '$lib/stores/tickets.svelte';

export const NO_WORKFLOWS = 'No workflows in .claude/workflows or ~/.claude/workflows';

/** Per ticket key: its workflows, or why none can run (no project). Read again by each picker. */
export const workflows = $state<Record<string, WorkflowInfo[] | string>>({});

/** Why the workflow action is off for a loaded list (null = it can run, or not loaded yet). */
export function workflowReason(list: WorkflowInfo[] | string | undefined): string | null {
  if (typeof list === 'string') return list;
  return list?.length === 0 ? NO_WORKFLOWS : null;
}

export async function loadWorkflows(ref: TicketRef): Promise<WorkflowInfo[] | string> {
  let list: WorkflowInfo[] | string;
  try {
    list = await workflowsList({ ticket: ref });
  } catch (err) {
    list = toIpcError('workflows_list', err).message;
  }
  workflows[ticketKey(ref)] = list;
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
    const s = await workflowRun({ ticket: ref, workflow });
    const args = { session_id: s.id };
    toasts.push({
      level: 'info',
      text: `${workflow} started on ${ref.key}`,
      action: { label: 'Open', command: 'session.reveal', args },
      secondary: { label: 'Peek', command: 'session.peek', args },
    });
  } catch (err) {
    // Held: the backend names the holder (`KEY is held by session …`).
    toasts.error(err);
  }
}

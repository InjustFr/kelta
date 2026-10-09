// Action handlers owned by L9 (work): `work.start`, `work.new`, `work.link`.
import { registerAction } from '$lib/actions';
import type { TicketRef, WorkItemId } from '$lib/gen';
import { toasts, ui, work } from '$lib/stores';

import { currentTab } from '../../shell/nav';
import { selection } from './selection.svelte';
import { startWorkOnTicket } from './startWork';

registerAction('work.start', async (args) => {
  const ticket = (args?.ticket as TicketRef | undefined) ?? selection.ticket;
  if (!ticket) {
    toasts.info('Select a ticket to start work');
    return;
  }
  const projectId = (args?.project_id as string | undefined) ?? selection.projectId;
  await startWorkOnTicket(ticket, projectId);
});

// Preloaded: ⇧⌘N then typing at once must not lose the first keys to the focused terminal.
void import('./NewWorkSheet.svelte');

/** New work item sheet (FLOW §4.3): task, wip/ branch, Claude. */
export function newWorkItem(): void {
  ui.openSheet('work_new');
}

/** Link to ticket… for a scratch item (`id`, default the focused tab's work item). */
export function linkToTicket(id: WorkItemId | null = currentTab()?.work_item_id ?? null): void {
  const item = id ? work.get(id) : null;
  if (!item) toasts.info('Focus a work item tab to link it to a ticket');
  else if (item.kind !== 'branch')
    toasts.info(item.review ? 'Review checkout: read-only' : 'This work item already has a ticket');
  else ui.openSheet('link_ticket', { id: item.id });
}

registerAction('work.new', newWorkItem);
registerAction('work.link', (args) => linkToTicket((args?.id as WorkItemId | undefined) ?? undefined));

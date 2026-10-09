// Action handlers owned by L9 (work): `work.start`, and Ship / Finish… / Finish all merged
// (FLOW §4.5, §4.6). `args.id` names the work item; default: the focused tab's.
import { registerAction } from '$lib/actions';
import type { TicketRef, WorkItem } from '$lib/gen';
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

function target(args?: Record<string, unknown>): WorkItem | null {
  const id = (args?.id as string | undefined) ?? currentTab()?.work_item_id;
  const item = id ? work.get(id) : null;
  if (!item || item.state.kind === 'finished') {
    toasts.info('Focus a work item tab first');
    return null;
  }
  return item;
}

registerAction('work.ship', (args) => {
  const item = target(args);
  if (!item) return;
  if (item.pr_url) {
    // Push / Force push of an existing PR belong to the fix-loop lane (work_push).
    toasts.push({
      level: 'info',
      text: 'This work item already has a PR.',
      action: { label: 'Open', command: 'tickets.open_in_browser', args: { url: item.pr_url } },
    });
    return;
  }
  ui.openSheet('ship', { item });
});

registerAction('work.finish', (args) => {
  const item = target(args);
  if (item) ui.openSheet('finish', { item });
});

registerAction('work.finish_merged', () => ui.openSheet('finish_merged'));

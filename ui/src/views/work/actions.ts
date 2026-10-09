// Action handlers owned by L9 (work): `work.start`.
import { registerAction } from '$lib/actions';
import type { TicketRef } from '$lib/gen';
import { toasts } from '$lib/stores';

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

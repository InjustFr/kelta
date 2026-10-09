// The ticket currently selected in a ticket view (list, board, detail, inbox). `work.start` acts on it.

import type { ProjectId, TicketRef } from '$lib/gen';

export const selection = $state<{ ticket: TicketRef | null; projectId: ProjectId | null }>({
  ticket: null,
  projectId: null,
});

export function selectTicket(ticket: TicketRef | null, projectId: ProjectId | null): void {
  selection.ticket = ticket;
  selection.projectId = projectId;
}

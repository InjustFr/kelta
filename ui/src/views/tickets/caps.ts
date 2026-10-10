// Ticket actions (TICKETS.md T3 action bar, the detail's keys) and why one is off: the ticket's
// tracker caps, a missing PR or a missing branch. The UI disables the action and shows the reason.

import type { TicketItem } from '$lib/gen';

export type TicketAction = 'start' | 'move' | 'pr' | 'assign' | 'unassign' | 'comment' | 'branch' | 'browser';

/** Action bar order, with the key that runs each one in the detail. */
export const TICKET_ACTIONS: readonly { id: TicketAction; key: string }[] = [
  { id: 'start', key: 's' },
  { id: 'move', key: 'm' },
  { id: 'pr', key: 'p' },
  { id: 'assign', key: 'a' },
  { id: 'unassign', key: 'A' },
  { id: 'comment', key: 'c' },
  { id: 'branch', key: 'y' },
  { id: 'browser', key: 'o' },
];

/** Why `action` cannot run on `item` (null = it can). `branch` is the ticket's branch, if any. */
export function blockedReason(action: TicketAction, item: TicketItem, branch: string | null): string | null {
  switch (action) {
    case 'pr':
      return item.prs.length > 0 ? null : 'No pull request is linked to this ticket yet.';
    case 'assign':
    case 'unassign':
      return item.caps.assign ? null : 'This tracker does not let Kelta change the assignee.';
    case 'comment':
      return item.caps.comment ? null : 'This tracker does not let Kelta add comments.';
    case 'branch':
      return branch ? null : 'No branch yet. Start work first.';
    default:
      return null;
  }
}

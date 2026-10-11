// Ticket actions (TICKETS.md T3 action bar, the detail's keys) and why one is off: the ticket's
// tracker caps, a missing PR or a missing branch. The UI disables the action and shows the reason.

import { t } from '$lib/i18n/index.svelte';
import type { TicketItem, WorkflowInfo } from '$lib/gen';

import { workflowReason } from './workflow.svelte';

export type TicketAction =
  | 'start'
  | 'move'
  | 'pr'
  | 'assign'
  | 'unassign'
  | 'comment'
  | 'block'
  | 'refine'
  | 'workflow'
  | 'branch'
  | 'browser';

/** Action bar order, with the key that runs each one in the detail. */
export const TICKET_ACTIONS: readonly { id: TicketAction; key: string }[] = [
  { id: 'start', key: 's' },
  { id: 'move', key: 'm' },
  { id: 'pr', key: 'p' },
  { id: 'assign', key: 'a' },
  { id: 'unassign', key: 'A' },
  { id: 'comment', key: 'c' },
  { id: 'block', key: 'b' },
  { id: 'refine', key: 'r' },
  { id: 'workflow', key: 'w' },
  { id: 'branch', key: 'y' },
  { id: 'browser', key: 'o' },
];

/** Opens the detail's More menu (Cmd+. on macOS, Ctrl+Shift+. on Linux). */
export const MORE_CHORD = 'mod+.';

/**
 * Why `action` cannot run on `item` (null = it can). `branch` is the ticket's branch, if any;
 * `workflows` its loaded workflows, or why there are none (`workflow.svelte.ts`).
 */
export function blockedReason(
  action: TicketAction,
  item: TicketItem,
  branch: string | null,
  workflows?: WorkflowInfo[] | string,
): string | null {
  switch (action) {
    case 'pr':
      return item.prs.length > 0 ? null : t('No pull request is linked to this ticket yet.');
    case 'assign':
    case 'unassign':
      if (!item.caps.assign) return t('This tracker does not let Kelta change the assignee.');
      return action === 'unassign' && !item.ticket.assignee ? t('Nobody is assigned.') : null;
    case 'comment':
      return item.caps.comment ? null : t('This tracker does not let Kelta add comments.');
    case 'block':
      return item.caps.block
        ? null
        : t('Kelta cannot set the blocked marker of this tracker. Set it in the tracker.');
    case 'branch':
      return branch ? null : t('No branch yet. Start work first.');
    case 'workflow':
      return workflowReason(workflows);
    default:
      return null;
  }
}

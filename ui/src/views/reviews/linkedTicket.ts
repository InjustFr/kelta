// A review's `linked_tickets` are bare keys: resolve one by exact key and open its detail.

import { trackerSearch } from '$lib/ipc/commands';
import type { ProjectId } from '$lib/gen';
import { toasts } from '$lib/stores';

import { openContent } from '../work/nav';

export async function openLinkedTicket(key: string, projectId: ProjectId): Promise<void> {
  try {
    // The search is fuzzy (`SHOP-1` also finds `SHOP-12`): only an exact key counts.
    const hit = (await trackerSearch({ scope: { kind: 'all' }, text: key })).find(
      (h) => h.ticket.ref.key.toLowerCase() === key.toLowerCase(),
    );
    if (!hit) {
      toasts.info(`${key} is not in any of your trackers`);
      return;
    }
    await openContent(
      hit.project_ids[0] ?? projectId,
      { kind: 'ticket_detail', ticket: hit.ticket.ref },
      { placement: 'new_tab' },
    );
  } catch (err) {
    toasts.error(err, `Opening ${key}`);
  }
}

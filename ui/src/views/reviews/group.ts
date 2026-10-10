// Team tab grouping (`G`): open PRs by their first linked ticket, across repos and projects.

import type { ReviewItem, TicketItem } from '$lib/gen';

import { sameKey } from './linkedTicket';

export interface ReviewGroup {
  /** `SHOP-142`, `acme/web#12`; `null` = No ticket. */
  key: string | null;
  /** The ticket's title when a loaded Tickets list has it. */
  title: string | null;
  items: ReviewItem[];
}

/** Groups ordered by their newest PR, newest first within; keyless PRs last. A `#12` key is per repo. */
export function groupByTicket(
  items: readonly ReviewItem[],
  tickets: readonly TicketItem[] = [],
): ReviewGroup[] {
  const byKey = new Map<string, ReviewGroup>();
  const none: ReviewItem[] = [];
  const newest = [...items].sort((a, b) => b.review.updated_at.localeCompare(a.review.updated_at));
  for (const i of newest) {
    const first = i.review.linked_tickets[0];
    if (!first) {
      none.push(i);
      continue;
    }
    const repo = i.review.ref.repo;
    const key = first.startsWith('#') ? `${repo}${first}` : first;
    let g = byKey.get(key.toLowerCase());
    if (!g) {
      const t = tickets.find((t) => sameKey(t.ticket.ref.key, first, repo));
      g = { key, title: t?.ticket.title ?? null, items: [] };
      byKey.set(key.toLowerCase(), g);
    }
    g.items.push(i);
  }
  return [...byKey.values(), ...(none.length > 0 ? [{ key: null, title: null, items: none }] : [])];
}

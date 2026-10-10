import { describe, expect, it } from 'vitest';

import type { ReviewItem, TicketItem } from '$lib/gen';
import { reviewPage } from '$lib/gen/fixtures';

import { groupByTicket } from './group';

function pr(repo: string, number: number, updated: string, keys: string[]): ReviewItem {
  const s = reviewPage.items[0]!;
  return {
    ...s,
    review: {
      ...s.review,
      ref: { ...s.review.ref, repo, number },
      updated_at: updated,
      linked_tickets: keys,
    },
  };
}

describe('groupByTicket', () => {
  it('groups by first key across repos, #N per repo, newest group first, No ticket last', () => {
    const items = [
      pr('acme/api', 1, '2026-10-01T00:00:00Z', ['SHOP-142']),
      pr('acme/web', 2, '2026-10-03T00:00:00Z', ['SHOP-142', 'SHOP-9']),
      pr('acme/web', 3, '2026-10-05T00:00:00Z', []),
      pr('acme/web', 4, '2026-10-02T00:00:00Z', ['#12']),
      pr('acme/api', 5, '2026-10-04T00:00:00Z', ['#12']),
      pr('acme/web', 6, '2026-09-01T00:00:00Z', ['acme/web#12']),
    ];
    const ticket = { ticket: { ref: { key: 'SHOP-142' }, title: 'Checkout timeout' } } as TicketItem;
    const groups = groupByTicket(items, [ticket]);
    expect(groups.map((g) => [g.key, g.items.map((i) => i.review.ref.number)])).toEqual([
      ['acme/api#12', [5]],
      ['SHOP-142', [2, 1]],
      ['acme/web#12', [4, 6]],
      [null, [3]],
    ]);
    expect(groups[1]!.title).toBe('Checkout timeout');
    expect(groups[0]!.title).toBeNull();
  });
});

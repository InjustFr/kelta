import { describe, expect, it } from 'vitest';

import * as samples from '$lib/gen/fixtures';

import { batch, startsNow } from './batch.svelte';

describe('batch start', () => {
  it('splits a batch into now and queued by free Claude slots', () => {
    expect(startsNow(5, 2, 4)).toBe(2);
    expect(startsNow(5, 4, 4)).toBe(0);
    expect(startsNow(5, 6, 4)).toBe(0); // over cap (Start now)
    expect(startsNow(1, 0, 4)).toBe(1);
    expect(startsNow(5, 9, 0)).toBe(5); // 0 = no cap
  });

  it('Space toggles a mark, in marking order', () => {
    const a = samples.ticket.ref;
    const b = { ...a, key: 'SHOP-999' };
    batch.toggle(b, 'shop');
    batch.toggle(a, null);
    expect(batch.list.map((m) => m.ref.key)).toEqual(['SHOP-999', a.key]);
    batch.toggle(b, 'shop');
    expect(batch.has(b)).toBe(false);
    batch.clear();
    expect(batch.list).toEqual([]);
  });
});

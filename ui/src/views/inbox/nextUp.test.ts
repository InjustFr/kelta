import { describe, expect, it } from 'vitest';

import type { NextUpItem, TicketItem } from '$lib/gen';
import * as samples from '$lib/gen/fixtures';

import { groomQueue, isSnoozed, movedRank, rankBetween } from './nextUp.svelte';

const ticket = (key: string, category: TicketItem['ticket']['status']['category'] = 'todo'): TicketItem => ({
  ...samples.ticketPage.items[0]!,
  ticket: {
    ...samples.ticketPage.items[0]!.ticket,
    ref: { account: 'jira', key, id: key },
    status: { id: category, name: category, category },
  },
});
const entry = (key: string, rank: number | null, snoozed_until: string | null = null): NextUpItem => ({
  project_id: 'shop',
  ticket: { account: 'jira', key, id: key },
  rank,
  snoozed_until,
});

describe('fractional ranks', () => {
  it('goes between neighbours, past an end by one', () => {
    expect(rankBetween(null, null)).toBe(0);
    expect(rankBetween(null, 2)).toBe(1);
    expect(rankBetween(3, null)).toBe(4);
    expect(rankBetween(1, 2)).toBe(1.5);
  });

  it('J moves one place down, K one place up; the ends stay put', () => {
    const ranks = [0, 1, 2, 3];
    expect(movedRank(ranks, 1, 1)).toBe(2.5); // between 2 and 3
    expect(movedRank(ranks, 2, 1)).toBe(4); // past the last
    expect(movedRank(ranks, 2, -1)).toBe(0.5); // between 0 and 1
    expect(movedRank(ranks, 1, -1)).toBe(-1); // before the first
    expect(movedRank(ranks, 3, 1)).toBeNull();
    expect(movedRank(ranks, 0, -1)).toBeNull();
  });
});

describe('groomQueue', () => {
  const now = Date.parse('2026-10-10T12:00:00Z');

  it('New first, then the pool order; listed, snoozed, closed and started left out', () => {
    const pool = ['A', 'B', 'C', 'D', 'E', 'F', 'G'].map((k) => ticket(k));
    pool[4] = ticket('E', 'done');
    const entries = {
      'jira:B': entry('B', 1),
      'jira:C': entry('C', null, '2026-10-17T12:00:00Z'),
      // A snooze that ran out: back in the pass.
      'jira:D': entry('D', null, '2026-10-01T00:00:00Z'),
    };
    const got = groomQueue(
      pool,
      entries,
      (k) => k === 'jira:G',
      (t) => t.ticket.ref.key === 'F',
      now,
    );
    expect(got.map((t) => t.ticket.ref.key)).toEqual(['G', 'A', 'D']);
    expect(isSnoozed(entries['jira:C'], now)).toBe(true);
    expect(isSnoozed(entries['jira:D'], now)).toBe(false);
  });
});

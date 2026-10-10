import { beforeEach, describe, expect, it } from 'vitest';

import type { NextUpItem, TicketItem, TicketRef } from '$lib/gen';
import * as samples from '$lib/gen/fixtures';
import { createMockTransport, type MockControls } from '$lib/ipc/mock';
import { setTransport } from '$lib/ipc/transport';
import { tickets } from '$lib/stores';
import { ticketListKey } from '$lib/stores/tickets.svelte';

import { currentSections } from './now';
import { groomQueue, isSnoozed, loadPool, movedRank, nextUp, rankBetween, ticketPool } from './nextUp.svelte';

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

describe('against the mock transport', () => {
  let mock: MockControls;
  const ALL = { kind: 'all' } as const;
  const ref = (key: string): TicketRef =>
    mock.state.tickets.find((t) => t.ticket.ref.key === key)!.ticket.ref;
  const listed = (...keys: string[]) => {
    mock.state.nextUp.items = keys.map((k, i) => ({
      project_id: 'shop',
      ticket: ref(k),
      rank: i,
      snoozed_until: null,
    }));
  };
  const listedKeys = () => nextUp.ordered().map((e) => e.ticket.key);

  beforeEach(() => {
    const created = createMockTransport();
    mock = created.controls;
    setTransport(created.transport);
  });

  it('first run: everything loaded counts as seen, Up next stays as it was', async () => {
    mock.state.nextUp.seen = [];
    await loadPool(true);
    expect(mock.calls.filter((c) => c.cmd === 'ticket_seen')).toHaveLength(1);
    expect(ticketPool().length).toBeGreaterThan(0);
    expect(ticketPool().some((t) => nextUp.isNew(t.ticket.ref))).toBe(false);
    const ids = currentSections().map((s) => s.id);
    expect(ids).toContain('up_next');
    expect(ids).not.toContain('new');

    // Seen already set: no second baseline.
    await loadPool(true);
    expect(mock.calls.filter((c) => c.cmd === 'ticket_seen')).toHaveLength(1);
  });

  it('prune drops closed tickets, and missing ones only when both lists are whole', async () => {
    listed('SHOP-151', '4590', '#15');
    mock.state.tickets.find((t) => t.ticket.ref.key === 'SHOP-151')!.ticket.status.category = 'done';
    mock.state.tickets = mock.state.tickets.filter((t) => t.ticket.ref.key !== '#15');
    await loadPool(true);
    const mine = tickets.lists[ticketListKey(ALL, null, 'mine')]!.data!;

    mine.next = { kind: 'offset', value: 50 };
    await nextUp.prune();
    expect(listedKeys()).toEqual(['4590', '#15']); // closed one gone, missing one kept (a later page)

    mine.next = null;
    mine.errors = [{ account_id: 'github-oss', error: { code: 'network', message: 'down' } as never }];
    await nextUp.prune();
    expect(listedKeys()).toEqual(['4590', '#15']); // an account failed: kept

    mine.errors = [];
    await nextUp.prune();
    expect(listedKeys()).toEqual(['4590']);
    expect(mock.state.nextUp.items.map((i) => i.ticket.key)).toEqual(['4590']);
  });
});

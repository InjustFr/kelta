import { describe, expect, it } from 'vitest';

import type { TicketItem, WorkItem } from '$lib/gen';

import { orderUnfinished, type WorkRow } from './finishFirst';
import type { Phase } from './phase';

const ref = (key: string) => ({ account: 'jira', key, id: key });
const row = (key: string, phase: Phase['id'], hasPr = false): WorkRow => ({
  item: { id: key, kind: 'ticket', ticket: ref(key), review: null } as unknown as WorkItem,
  phase: { id: phase, label: phase } as Phase,
  title: key,
  hasPr,
});
const ticket = (key: string, category: string, blocked = false): TicketItem =>
  ({
    ticket: { ref: ref(key), title: key, status: { id: category, name: category, category }, blocked },
    project_ids: [],
  }) as unknown as TicketItem;

describe('orderUnfinished', () => {
  it('puts what is closest to done first, blocked tickets included', () => {
    const work = [
      row('H', 'no_changes'),
      row('A', 'no_changes'),
      row('B', 'in_review', true),
      row('C', 'approved', true),
      row('D', 'merged'),
    ];
    const mine = [
      ticket('A', 'in_progress', true), // has a work item: listed once, as the item
      ticket('E', 'in_progress', true),
      ticket('F', 'in_review'),
      ticket('G', 'todo'),
      ticket('H', 'in_progress'), // the ticket being started
    ];
    const out = orderUnfinished(work, mine, 'jira:H');
    expect(out.map((r) => [r.key, r.blocked])).toEqual([
      ['D', false],
      ['C', false],
      ['B', false],
      ['F', false],
      ['A', true],
      ['E', true],
    ]);
  });
});

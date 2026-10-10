import { describe, expect, it } from 'vitest';

import type { StatusCategory, TicketItem } from '$lib/gen';
import * as samples from '$lib/gen/fixtures';

import { flowOrder, groupTickets } from './group';

const item = (
  key: string,
  status: [string, StatusCategory],
  assignee: string | null = null,
  view_ids: string[] = [],
): TicketItem => ({
  ticket: {
    ...samples.ticket,
    ref: { ...samples.ticket.ref, key },
    status: { id: status[0], name: status[0], category: status[1] },
    assignee: assignee ? { id: assignee, name: assignee, login: null, avatar_url: null } : null,
  },
  project_ids: [],
  work_item_id: null,
  view_ids,
  prs: [],
  caps: samples.ticketPage.items[0]!.caps,
});

const summary = (g: ReturnType<typeof groupTickets>) =>
  g.map((x) => [x.label, x.items.map((i) => i.ticket.ref.key)]);

describe('groupTickets', () => {
  const items = [
    item('A', ['Backlog', 'todo'], 'Zoe', ['v2']),
    item('B', ['Done', 'done'], null, ['v1']),
    item('C', ['QA', 'in_review'], 'Ada'),
    item('D', ['In Progress', 'in_progress'], 'Ada', ['v1']),
    item('E', ['Blocked', 'unknown']),
    item('F', ['Code review', 'in_review'], null, ['v2']),
  ];

  it('groups by native status name, ordered by category then name', () => {
    expect(summary(groupTickets(items, 'status'))).toEqual([
      ['In Progress', ['D']],
      ['Code review', ['F']],
      ['QA', ['C']],
      ['Backlog', ['A']],
      ['Blocked', ['E']],
      ['Done', ['B']],
    ]);
    expect(groupTickets(items, 'status').at(-1)?.category).toBe('done');
  });

  it('groups by assignee with Unassigned last', () => {
    expect(summary(groupTickets(items, 'assignee'))).toEqual([
      ['Ada', ['C', 'D']],
      ['Zoe', ['A']],
      ['Unassigned', ['B', 'E', 'F']],
    ]);
  });

  it('groups by source in view order, unknown views last', () => {
    const sources = [
      { id: 'v1', label: 'Mine' },
      { id: 'v2', label: 'Sprint' },
    ];
    expect(summary(groupTickets(items, 'source', sources))).toEqual([
      ['Mine', ['B', 'D']],
      ['Sprint', ['A', 'F']],
      ['Other', ['C', 'E']],
    ]);
  });

  it('keeps one group in tracker order for none', () => {
    expect(summary(groupTickets(items, 'none'))).toEqual([['All', ['A', 'B', 'C', 'D', 'E', 'F']]]);
  });
});

describe('flowOrder', () => {
  it('dedupes by name and orders by workflow', () => {
    const s = (name: string, category: StatusCategory) => ({ id: name, name, category });
    expect(
      flowOrder([s('Done', 'done'), s('To Do', 'todo'), s('Review', 'in_review'), s('To Do', 'todo')]).map(
        (x) => x.name,
      ),
    ).toEqual(['To Do', 'Review', 'Done']);
  });
});

import { describe, expect, it } from 'vitest';

import type { Person, StatusCategory, TicketItem } from '$lib/gen';
import * as samples from '$lib/gen/fixtures';

import { assigneeChips, statusChips } from './chips';

const item = (key: string, status: [string, StatusCategory], assignee: string | null = null): TicketItem => ({
  ...samples.ticketPage.items[0]!,
  ticket: {
    ...samples.ticket,
    ref: { ...samples.ticket.ref, key },
    status: { id: status[0], name: status[0], category: status[1] },
    assignee: assignee ? { id: assignee, name: assignee, login: null, avatar_url: null } : null,
  },
});

const items = [
  item('A', ['To Do', 'todo'], 'u-zoe'),
  item('B', ['In Progress', 'in_progress'], 'u-ada'),
  item('C', ['to do', 'todo'], 'u-ada'),
  item('D', ['Code review', 'in_review']),
];

describe('statusChips', () => {
  it('counts each status by name, in workflow order, and keeps the picked one at 0', () => {
    expect(statusChips(items, null).map((c) => [c.label, c.n])).toEqual([
      ['To Do', 2],
      ['In Progress', 1],
      ['Code review', 1],
    ]);
    expect(statusChips(items, 'Done').at(-1)).toMatchObject({ id: 'Done', n: 0 });
  });
});

describe('assigneeChips', () => {
  const ada: Person = {
    key: 'acme:u-ada',
    name: 'Ada',
    avatar_url: null,
    ids: [{ account: samples.ticket.ref.account, id: 'u-ada', login: null, code_host: false }],
    matched: true,
    me: false,
  };

  it('keys a team person by key, others by tracker id, most tickets first', () => {
    expect(assigneeChips(items, [ada], null).map((c) => [c.id, c.label, c.n])).toEqual([
      ['acme:u-ada', 'Ada', 2],
      ['u-zoe', 'u-zoe', 1],
    ]);
  });

  it('keeps the picked person while they match nothing', () => {
    expect(assigneeChips([], [], { id: 'u-bob', name: 'Bob' })).toMatchObject([{ id: 'u-bob', n: 0 }]);
  });
});

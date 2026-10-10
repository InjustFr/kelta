import { describe, expect, it } from 'vitest';

import type { PrLink, Sprint, StatusCategory, Ticket, TicketItem, WorkItem } from '$lib/gen';
import * as samples from '$lib/gen/fixtures';

import { ageLevel, flowOf, flowOrder, groupTickets, sortTickets, type FlowSignals } from './group';

const item = (
  key: string,
  status: [string, StatusCategory],
  assignee: string | null = null,
  view_ids: string[] = [],
  extra: Partial<Ticket> = {},
  prs: PrLink[] = [],
): TicketItem => ({
  ticket: {
    ...samples.ticket,
    ref: { ...samples.ticket.ref, key },
    status: { id: status[0], name: status[0], category: status[1] },
    assignee: assignee ? { id: assignee, name: assignee, login: null, avatar_url: null } : null,
    ...extra,
  },
  project_ids: [],
  work_item_id: null,
  view_ids,
  prs,
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

const NOW = Date.parse('2026-10-10T12:00:00Z');
const daysAgo = (n: number) => new Date(NOW - n * 86_400_000).toISOString();
const sprint = (id: string, active: boolean): Sprint => ({ id, name: `Sprint ${id}`, active, ends_at: null });
const pr = (over: Partial<PrLink>): PrLink => ({
  url: 'https://h/r/pull/1',
  account: null,
  repo: 'r',
  number: 1,
  title: '',
  branch: 'b',
  state: 'open',
  draft: false,
  ci: 'success',
  review: 'approved',
  source: 'work_item',
  ...over,
});
const workIn = (kind: 'active' | 'pr_open') => ({ state: { kind } }) as WorkItem;

describe('flowOf', () => {
  const none: FlowSignals = { work: null, needsYou: false };
  const at = (i: TicketItem, s = none) => flowOf(i, s, NOW);

  it('sorts a ticket into Doing, Waiting, Ready, Backlog or Done', () => {
    expect(at(item('A', ['To Do', 'todo']), { work: workIn('active'), needsYou: false })).toBe('doing');
    expect(at(item('A', ['In Progress', 'in_progress']))).toBe('doing');
    expect(at(item('A', ['To Do', 'todo']))).toBe('ready');
    expect(at(item('A', ['Backlog', 'todo']))).toBe('backlog');
    expect(at(item('A', ['Triage', 'unknown']))).toBe('backlog');
    expect(at(item('A', ['Done', 'done'], null, [], { status_since: daysAgo(2) }))).toBe('done');
  });

  it('waits on Claude, a blocked status, red CI or a requested review', () => {
    const active = { work: workIn('active'), needsYou: true };
    expect(at(item('A', ['In Progress', 'in_progress']), active)).toBe('waiting');
    expect(at(item('A', ['Blocked', 'in_progress']))).toBe('waiting');
    expect(at(item('A', ['In Progress', 'in_progress'], null, [], {}, [pr({ ci: 'failure' })]))).toBe(
      'waiting',
    );
    expect(at(item('A', ['QA', 'in_review']))).toBe('waiting');
    expect(at(item('A', ['To Do', 'todo'], null, [], {}, [pr({ review: 'review_required' })]))).toBe(
      'waiting',
    );
    // A draft is not a review request; a merged PR's red CI no longer matters.
    expect(
      at(item('A', ['To Do', 'todo'], null, [], {}, [pr({ review: 'review_required', draft: true })])),
    ).toBe('ready');
    expect(at(item('A', ['To Do', 'todo'], null, [], {}, [pr({ state: 'merged', ci: 'failure' })]))).toBe(
      'ready',
    );
  });

  it('leaves out what was done more than 7 days ago', () => {
    expect(at(item('A', ['Done', 'done'], null, [], { status_since: daysAgo(9) }))).toBeNull();
    const flow = (i: TicketItem) => flowOf(i, none, NOW);
    const groups = groupTickets(
      [
        item('A', ['Done', 'done'], null, [], { status_since: daysAgo(9) }),
        item('B', ['Done', 'done'], null, [], { status_since: daysAgo(1) }),
        item('C', ['To Do', 'todo']),
        item('D', ['Doing', 'in_progress']),
      ],
      'flow',
      [],
      flow,
    );
    expect(summary(groups)).toEqual([
      ['Doing', ['D']],
      ['Ready', ['C']],
      ['Done in the last 7 days', ['B']],
    ]);
    expect(groups.at(-1)?.category).toBe('done');
  });
});

describe('priority, sprint, sort and age', () => {
  const p = (key: string, rank: number | null, priority: string | null, extra: Partial<Ticket> = {}) =>
    item(key, ['To Do', 'todo'], null, [], { priority_rank: rank, priority, ...extra });

  it('groups by priority rank with no priority last, and by sprint with active first', () => {
    const list = [p('A', 2, 'Low'), p('B', null, null), p('C', 0, 'Highest'), p('D', 2, 'Low')];
    expect(summary(groupTickets(list, 'priority'))).toEqual([
      ['Highest', ['C']],
      ['Low', ['A', 'D']],
      ['No priority', ['B']],
    ]);
    const s = [
      p('A', null, null, { sprint: sprint('13', false) }),
      p('B', null, null, { sprint: null }),
      p('C', null, null, { sprint: sprint('12', true) }),
      p('D', null, null, {
        sprint: sprint('13', false),
        ref: { ...samples.ticket.ref, account: 'other', key: 'D' },
      }),
    ];
    expect(summary(groupTickets(s, 'sprint'))).toEqual([
      ['Sprint 12', ['C']],
      ['Sprint 13', ['A']],
      ['Sprint 13', ['D']], // same id on another account stays apart
      ['No sprint', ['B']],
    ]);
  });

  it('sorts by priority, updated, age in status and key', () => {
    const list = [
      p('X-10', 1, 'High', { updated_at: daysAgo(1), status_since: daysAgo(3) }),
      p('X-9', null, null, { updated_at: daysAgo(0), status_since: daysAgo(30) }),
      p('X-2', 1, 'High', { updated_at: daysAgo(0), status_since: daysAgo(1) }),
    ];
    const keys = (s: Parameters<typeof sortTickets>[1]) => sortTickets(list, s).map((i) => i.ticket.ref.key);
    expect(keys('priority')).toEqual(['X-2', 'X-10', 'X-9']);
    expect(keys('updated')).toEqual(['X-9', 'X-2', 'X-10']);
    expect(keys('age')).toEqual(['X-9', 'X-10', 'X-2']);
    expect(keys('key')).toEqual(['X-2', 'X-9', 'X-10']);
  });

  it('marks age at 7, 14 and 21 days in a non-done status, falling back to updated_at', () => {
    const aged = (n: number, cat: StatusCategory = 'todo') =>
      ageLevel(
        { ...samples.ticket, status: { id: 's', name: 's', category: cat }, status_since: daysAgo(n) },
        NOW,
      );
    expect([aged(6), aged(7), aged(14), aged(21), aged(30, 'done')]).toEqual([
      null,
      'old',
      'warn',
      'danger',
      null,
    ]);
    expect(ageLevel({ ...samples.ticket, status_since: null, updated_at: daysAgo(15) }, NOW)).toBe('warn');
  });
});

describe('flowOrder', () => {
  it('dedupes by name and orders by workflow', () => {
    const s = (name: string, category: StatusCategory) => ({ id: name, name, category });
    expect(
      flowOrder([s('Done', 'done'), s('To Do', 'todo'), s('Review', 'in_review'), s('To do', 'todo')]).map(
        (x) => x.name,
      ),
    ).toEqual(['To Do', 'Review', 'Done']);
  });
});

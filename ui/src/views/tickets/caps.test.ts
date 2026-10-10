import { describe, expect, it } from 'vitest';

import { createMockTransport } from '$lib/ipc/mock';

import { blockedReason } from './caps';

const caps = {
  board_columns: true,
  assign: true,
  comment: true,
  transitions_need_fetch: false,
  projects_v2: false,
};

describe('blockedReason', () => {
  const base = { ...createMockTransport().controls.state.tickets[0]!, prs: [], caps };

  it('lets every action run when the tracker can and the ticket has what it needs', () => {
    const pr = {
      url: 'u',
      account: null,
      repo: 'r',
      number: 1,
      title: '',
      branch: 'b',
      state: 'open',
      draft: false,
      ci: 'none',
      review: null,
      source: 'key_match',
    } as const;
    const item = { ...base, prs: [pr] };
    for (const a of ['start', 'move', 'pr', 'assign', 'unassign', 'comment', 'branch', 'browser'] as const)
      expect(blockedReason(a, item, 'feat/x')).toBeNull();
  });

  it('names what is missing', () => {
    const item = { ...base, caps: { ...caps, assign: false, comment: false } };
    expect(blockedReason('assign', item, null)).toMatch(/assignee/);
    expect(blockedReason('unassign', item, null)).toMatch(/assignee/);
    expect(blockedReason('comment', item, null)).toMatch(/comments/);
    expect(blockedReason('pr', item, null)).toMatch(/No pull request/);
    expect(blockedReason('branch', item, null)).toMatch(/Start work first/);
    expect(blockedReason('move', item, null)).toBeNull();
  });
});

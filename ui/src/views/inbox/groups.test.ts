import { describe, expect, it } from 'vitest';

import type { ReviewItem, SessionInfo, TicketItem, WorkItem } from '$lib/gen';
import * as samples from '$lib/gen/fixtures';

import type { Phase, PhaseId, NowSection } from '../work/phase';
import { headerParts, nowSections, readyByProject, waitingCount, type NowInput } from './groups';

let n = 0;
function entry(id: PhaseId, section: NowSection, patch: Partial<WorkItem> = {}) {
  n += 1;
  const item: WorkItem = {
    ...samples.workItem,
    id: `w${n}`,
    branch: `b${n}`,
    ticket: { account: 'jira', key: `K-${n}`, id: `${n}` },
    created_at: `2026-10-0${n % 9}T00:00:00Z`,
    ...patch,
  };
  const phase: Phase = { id, section, label: id, detail: '', lamp: 'none', primary: null, primaryLabel: '' };
  return { item, phase };
}

const review = (
  number: number,
  patch: Partial<ReviewItem['review']> = {},
  project_ids = ['shop'],
): ReviewItem => ({
  review: {
    ...samples.reviewDetail.review,
    ref: { ...samples.reviewDetail.review.ref, number },
    url: `https://x/pull/${number}`,
    source_branch: `pr-${number}`,
    my_state: null,
    decision: null,
    ci: 'success',
    ...patch,
  },
  project_ids,
});

const ticket = (key: string, category: TicketItem['ticket']['status']['category']): TicketItem => ({
  ticket: {
    ...samples.ticketDetail.ticket,
    ref: { account: 'jira', key, id: key },
    status: { id: category, name: category, category },
  },
  project_ids: ['shop'],
  work_item_id: null,
});

const empty: NowInput = { work: [], sessions: [], requested: [], authored: [], tickets: [] };

describe('nowSections', () => {
  it('orders the seven sections and drops empty ones', () => {
    const asking: SessionInfo = {
      ...samples.sessionInfo,
      id: 's1',
      status: 'needs_input',
      work_item_id: null,
    };
    const idle: SessionInfo = {
      ...samples.sessionInfo,
      id: 's2',
      status: 'waiting_user',
      work_item_id: null,
    };
    const sections = nowSections({
      ...empty,
      work: [entry('no_changes', 'in_flight'), entry('to_review', 'to_review'), entry('failed', 'fix')],
      sessions: [asking, idle],
      requested: [review(1)],
      tickets: [ticket('T-1', 'todo')],
    });
    expect(sections.map((s) => s.id)).toEqual([
      'needs_you',
      'to_review',
      'fix',
      'requests',
      'in_flight',
      'up_next',
    ]);
    // WaitingUser (idle) never means needs you.
    expect(sections[0]!.rows.map((r) => r.id)).toEqual(['s:s1']);
  });

  it('orders work rows by when Claude asked or stopped, not when the item started', () => {
    const old = entry('to_review', 'to_review', {
      created_at: '2026-01-01T00:00:00Z',
      claude_at: '2026-10-10T12:00:00Z',
    });
    const young = entry('to_review', 'to_review', {
      created_at: '2026-10-01T00:00:00Z',
      claude_at: '2026-10-10T09:00:00Z',
    });
    const rows = nowSections({ ...empty, work: [old, young] })[0]!.rows.map((r) => r.id);
    expect(rows).toEqual([`w:${young.item.id}`, `w:${old.item.id}`]);
  });

  it('Review requests: a PR comes back as updated since my review; a reviewed one leaves', () => {
    const head = 'b'.repeat(40);
    const sections = nowSections({
      ...empty,
      requested: [
        review(1),
        review(2, { my_state: 'approved', head_sha: head, reviewed_head: 'a'.repeat(40) }),
        review(3, { my_state: 'approved', head_sha: head, reviewed_head: head }),
      ],
    });
    const rows = sections[0]!.rows.map((r) => (r.type === 'review' ? r.reason : ''));
    expect(rows).toEqual(['Review requested', 'Updated since your review']);
  });

  it('Review requests: blocking first, then the oldest request (updated_at when unknown)', () => {
    const sections = nowSections({
      ...empty,
      requested: [
        review(1, { requested_at: '2026-09-20T00:00:00Z', updated_at: '2026-10-05T00:00:00Z' }),
        review(2, { blocking: true, requested_at: '2026-10-02T00:00:00Z' }),
        review(3, { requested_at: null, updated_at: '2026-09-10T00:00:00Z' }),
        review(4, { blocking: true, requested_at: '2026-10-01T00:00:00Z' }),
      ],
    });
    const rows = sections[0]!.rows.map((r) => (r.type === 'review' ? r.review.review.ref.number : 0));
    expect(rows).toEqual([4, 2, 3, 1]);
    expect(sections[0]!.rows[0]).toMatchObject({ reason: "Blocking: you're the last reviewer" });
  });

  it('Fix: changes requested, checks failed, remote commits, conflicts, failed steps', () => {
    const sections = nowSections({
      ...empty,
      work: [
        entry('failed', 'fix'),
        entry('conflicts', 'fix'),
        entry('remote_new', 'fix'),
        entry('checks_failed', 'fix'),
        entry('changes_requested', 'fix'),
      ],
    });
    const fix = sections.find((s) => s.id === 'fix')!;
    expect(fix.rows.map((r) => (r.type === 'work' ? r.phase.id : ''))).toEqual([
      'changes_requested',
      'checks_failed',
      'remote_new',
      'conflicts',
      'failed',
    ]);
  });

  it('Ship: approved, unpushed, ready, merged; In flight: working, in review, idle', () => {
    const sections = nowSections({
      ...empty,
      work: [
        entry('merged', 'ship'),
        entry('ready', 'ship'),
        entry('unpushed', 'ship'),
        entry('approved', 'ship'),
        entry('no_changes', 'in_flight'),
        entry('in_review', 'in_flight'),
        entry('working', 'in_flight'),
      ],
    });
    const ids = (id: string) =>
      sections.find((s) => s.id === id)!.rows.map((r) => (r.type === 'work' ? r.phase.id : ''));
    expect(ids('ship')).toEqual(['approved', 'unpushed', 'ready', 'merged']);
    expect(ids('in_flight')).toEqual(['working', 'in_review', 'no_changes']);
  });

  it('a work item appears once: its PR is not listed again, by url or by branch', () => {
    const own = entry('in_review', 'in_flight', { pr_url: 'https://x/pull/7' });
    const byBranch = entry('no_changes', 'in_flight', { branch: 'pr-8' });
    const reviewItem = entry('reviewing', 'requests', {
      kind: 'review',
      review: { ...samples.reviewDetail.review.ref, number: 9 },
    });
    const sections = nowSections({
      ...empty,
      work: [own, byBranch, reviewItem],
      authored: [review(7, { decision: 'changes_requested' }), review(8), review(10, { ci: 'failure' })],
      requested: [review(9), review(11, { my_state: 'approved' })],
    });
    const all = sections.flatMap((s) => s.rows.map((r) => `${s.id}:${r.id}`));
    expect(all.filter((r) => r.includes('#7') || r.includes('#8') || r.includes('#9'))).toEqual([]);
    // My PR without a work item and failing checks is a Fix row; a reviewed request is gone.
    const ref = samples.reviewDetail.review.ref;
    expect(all).toContain(`fix:p:${ref.account}:${ref.repo}#10`);
    expect(all.some((r) => r.includes('#11'))).toBe(false);
  });

  it('Up next: not done, not started, in progress first, at most 10', () => {
    const started = entry('working', 'in_flight', { ticket: { account: 'jira', key: 'T-2', id: 'T-2' } });
    const tickets = [
      ticket('T-0', 'done'),
      ticket('T-1', 'todo'),
      ticket('T-2', 'todo'),
      ticket('T-3', 'in_progress'),
    ];
    for (let i = 4; i < 16; i += 1) tickets.push(ticket(`T-${i}`, 'todo'));
    const up = nowSections({ ...empty, work: [started], tickets }).find((s) => s.id === 'up_next')!;
    expect(up.rows.slice(0, 2).map((r) => r.id)).toEqual(['t:jira:T-3', 't:jira:T-1']);
    expect([up.rows.length, up.more]).toEqual([10, 4]);
  });
});

describe('header and badge', () => {
  it('splits the header in decision order and drops zero parts', () => {
    const sections = nowSections({
      ...empty,
      work: [entry('replied', 'needs_you'), entry('to_review', 'to_review'), entry('to_review', 'to_review')],
      requested: [review(1), review(2), review(3)],
      tickets: [ticket('T-1', 'todo')],
    });
    expect(headerParts(sections, 1)).toEqual([
      'Claude: 1 asks',
      '2 LLM diffs',
      '3 PRs waiting',
      '1 working',
      '1 up next',
    ]);
    expect(waitingCount(sections)).toBe(6);
    expect(readyByProject(sections)).toEqual(new Map([['shop', 2]]));
    expect(headerParts(nowSections(empty), 0)).toEqual([]);
  });
});

describe('Ready for review', () => {
  it('oldest wait first, the smaller delta first on ties', () => {
    const delta = (lines: number) => ({
      lines,
      files: 1,
      tests: 0,
      generated: 0,
      insertions: lines,
      deletions: 0,
    });
    const at = '2026-10-10T08:00:00Z';
    const sections = nowSections({
      ...empty,
      work: [
        entry('to_review', 'to_review', { id: 'big', claude_at: at, delta: delta(300) }),
        entry('to_review', 'to_review', { id: 'newer', claude_at: '2026-10-10T09:00:00Z', delta: delta(1) }),
        entry('to_review', 'to_review', { id: 'small', claude_at: at, delta: delta(20) }),
      ],
    });
    expect(sections[0]!.label).toBe('Ready for review');
    expect(sections[0]!.rows.map((r) => (r.type === 'work' ? r.item.id : ''))).toEqual([
      'small',
      'big',
      'newer',
    ]);
  });
});

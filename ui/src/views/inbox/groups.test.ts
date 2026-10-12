import { describe, expect, it } from 'vitest';

import type { OutsideSession, ReviewItem, SessionInfo, TicketItem, UiEvent, WorkItem } from '$lib/gen';
import * as samples from '$lib/gen/fixtures';

import type { Phase, PhaseId, NowSection } from '../work/phase';
import {
  headerChips,
  jumpQueue,
  nowSections,
  rowKey,
  topChip,
  waitingCount,
  type NowInput,
  type WorkEntry,
} from './groups';

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
  waiting: [],
});

const ticket = (key: string, category: TicketItem['ticket']['status']['category']): TicketItem => ({
  ticket: {
    ...samples.ticketDetail.ticket,
    ref: { account: 'jira', key, id: key },
    status: { id: category, name: category, category },
  },
  project_ids: ['shop'],
  work_item_id: null,
  view_ids: [],
  source_ids: [],
  prs: [],
  caps: samples.ticketPage.items[0]!.caps,
  waiting: [],
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

  it('New comments: a work item lands in Fix first; a PR without a work item does not', () => {
    const sections = nowSections({
      ...empty,
      work: [entry('conflicts', 'fix'), entry('new_comments', 'fix')],
      authored: [review(12, { last_comment_at: '2026-10-10T09:00:00Z', unresolved_threads: 2 })],
    });
    const fix = sections.find((s) => s.id === 'fix')!;
    expect(fix.rows.map((r) => (r.type === 'work' ? r.phase.id : r.id))).toEqual([
      'new_comments',
      'conflicts',
    ]);
    // No watermark to clear it: it keeps today's rows (In review here).
    expect(fix.rows.some((r) => r.id.includes('#12'))).toBe(false);
    expect(sections.find((s) => s.id === 'in_flight')!.rows.some((r) => r.id.includes('#12'))).toBe(true);
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

  it('leaves PRs assigned to me out of Now', () => {
    const sections = nowSections({ ...empty, authored: [review(5, { assigned: true, ci: 'failure' })] });
    expect(sections.flatMap((s) => s.rows)).toEqual([]);
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

  it('Next up keeps my order and drops closed or started tickets; New and parked leave Up next', () => {
    const started = entry('working', 'in_flight', { ticket: { account: 'jira', key: 'N-3', id: 'N-3' } });
    const sections = nowSections({
      ...empty,
      work: [started],
      tickets: [ticket('T-1', 'todo'), ticket('T-2', 'todo'), ticket('T-3', 'todo'), ticket('N-1', 'todo')],
      nextUp: [ticket('N-2', 'todo'), ticket('N-1', 'todo'), ticket('N-0', 'done'), ticket('N-3', 'todo')],
      parked: new Set(['jira:N-1', 'jira:T-3']),
      fresh: new Set(['jira:T-2']),
    });
    const ids = (id: string) => sections.find((s) => s.id === id)?.rows.map((r) => r.id);
    expect(sections.map((s) => s.id)).toEqual(['next_up', 'new', 'in_flight', 'up_next']);
    expect(ids('next_up')).toEqual(['n:jira:N-2', 'n:jira:N-1']);
    expect(ids('new')).toEqual(['t:jira:T-2']);
    expect(ids('up_next')).toEqual(['t:jira:T-1']);
  });

  it("a source's waiting statuses count as done: out of Up next, New and Next up", () => {
    const shipping = (key: string, source: string): TicketItem => {
      const t = ticket(key, 'in_progress');
      return {
        ...t,
        ticket: { ...t.ticket, status: { ...t.ticket.status, name: 'A MEPP' } },
        source_ids: [source],
      };
    };
    const sections = nowSections({
      ...empty,
      tickets: [
        shipping('T-1', 'mine'),
        shipping('T-2', 'mine'),
        shipping('T-3', 'other'),
        ticket('T-4', 'todo'),
      ],
      nextUp: [shipping('N-1', 'mine')],
      fresh: new Set(['jira:T-2']),
      waiting: new Map([['mine', ['A MEPP', 'A MEP']]]),
    });
    expect(sections.flatMap((s) => s.rows.map((r) => r.id))).toEqual(['t:jira:T-3', 't:jira:T-4']);
  });
});

describe('header and badge', () => {
  it('chips in section order, zero ones dropped, lamp only where Louis is needed', () => {
    const sections = nowSections({
      ...empty,
      work: [
        entry('replied', 'needs_you'),
        entry('to_review', 'to_review'),
        entry('to_review', 'to_review'),
        entry('checks_failed', 'fix'),
      ],
      requested: [review(1), review(2), review(3)],
      tickets: [ticket('T-1', 'todo')],
    });
    const chips = headerChips(sections, 1);
    expect(chips.map((c) => [c.section, c.text, c.lamp])).toEqual([
      ['needs_you', '1 needs you', 'needs_input'],
      ['to_review', '2 LLM diffs', null],
      ['fix', '1 to fix', 'error'],
      ['requests', '3 PRs waiting', null],
      ['in_flight', '1 working', null],
      ['up_next', '1 up next', null],
    ]);
    expect(waitingCount(sections)).toBe(7);
    expect(headerChips(nowSections(empty), 0)).toEqual([]);
  });

  it('the rail shows the most urgent chip only', () => {
    const chips = (work: WorkEntry[]) =>
      headerChips(nowSections({ ...empty, work, tickets: [ticket('T-1', 'todo')] }), 0);
    expect(topChip(chips([entry('replied', 'needs_you'), entry('checks_failed', 'fix')]))?.text).toBe(
      '1 needs you',
    );
    expect(topChip(chips([entry('to_review', 'to_review'), entry('checks_failed', 'fix')]))?.text).toBe(
      '1 to fix',
    );
    expect(topChip(chips([]))?.text).toBe('1 up next');
    expect(topChip([])).toBeUndefined();
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

describe('jumpQueue', () => {
  const session = (id: string, patch: Partial<SessionInfo>): SessionInfo => ({
    ...samples.sessionInfo,
    id,
    work_item_id: null,
    lifecycle: 'live',
    ...patch,
  });

  it('orders the five bands, oldest first within a band, blocking requests first', () => {
    const at = (key: string, day: number): Partial<WorkItem> => ({
      ticket: { account: 'jira', key, id: key },
      claude_at: `2026-10-0${day}T00:00:00Z`,
    });
    const sections = nowSections({
      ...empty,
      work: [
        entry('to_review', 'to_review', at('REVIEW', 1)),
        entry('checks_failed', 'fix', at('RED', 1)),
        entry('failed', 'fix', at('FAILED', 5)),
        entry('needs_you', 'needs_you', at('ASKS', 4)),
        entry('replied', 'needs_you', at('REPLIED', 2)),
      ],
      sessions: [session('asking', { status: 'needs_input', created_at: '2026-10-03T00:00:00Z' })],
      requested: [
        review(1, { requested_at: '2026-10-01T00:00:00Z' }),
        review(2, { requested_at: '2026-10-02T00:00:00Z', blocking: true }),
      ],
      authored: [review(3, { decision: 'changes_requested', updated_at: '2026-10-02T00:00:00Z' })],
    });
    const errored = session('broke', { status: 'error', created_at: '2026-10-09T00:00:00Z' });
    const gone = session('gone', { status: 'error', lifecycle: 'exited' });
    const queue = jumpQueue(sections, [errored, gone]);
    expect(queue.map((j) => `${j.label}:${rowKey(j.row)}`)).toEqual([
      'needs input:REPLIED', // day 2
      `needs input:${samples.sessionInfo.name}`, // day 3
      'needs input:ASKS', // day 4
      'error:FAILED', // failed start, day 5
      `error:${samples.sessionInfo.name}`, // StopFailure (API error or rate limit), day 9
      'ready for review:REVIEW',
      'feedback:RED', // day 1
      'feedback:#3', // day 2
      'review request:#2', // blocking beats older
      'review request:#1',
    ]);
  });

  it('is empty when nothing waits (In flight and Up next do not count)', () => {
    const sections = nowSections({
      ...empty,
      work: [entry('no_changes', 'in_flight')],
      tickets: [ticket('T-1', 'todo')],
    });
    expect(jumpQueue(sections, [])).toEqual([]);
  });
});

describe('Claude sessions Kelta did not start (#345)', () => {
  const outside = (id: string, status: OutsideSession['status']): OutsideSession => ({
    ...(samples.uiEventOutsideUpdated as Extract<UiEvent, { type: 'outside.updated' }>).session,
    id,
    title: `t-${id}`,
    status,
  });

  it('asks in Claude needs you (badge, Mod+J), the others in Elsewhere', () => {
    const sections = nowSections({
      ...empty,
      outside: [outside('a', 'needs_input'), outside('b', 'working'), outside('c', 'done')],
    });
    expect(sections.map((s) => [s.id, s.rows.map((r) => r.id)])).toEqual([
      ['needs_you', ['o:a']],
      ['elsewhere', ['o:b', 'o:c']],
    ]);
    expect(waitingCount(sections)).toBe(1);
    const queue = jumpQueue(sections, []);
    expect(queue.map((j) => [j.label, rowKey(j.row)])).toEqual([['needs input', 't-a']]);
  });
});

describe('ask questions (#356)', () => {
  const question = { question: 'Which one?', options: ['A'], body_html: null };
  const asking = (id: string): SessionInfo => ({
    ...samples.sessionInfo,
    id,
    status: 'working',
    work_item_id: null,
    lifecycle: 'live',
    claude: { ...samples.sessionInfo.claude!, question },
  });

  it("lists a plain asking session in Claude needs you, not a listed item's run session", () => {
    const claimed = entry('asking', 'needs_you', {
      run: {
        name: 'w',
        run_id: 'r',
        session_id: 'orch',
        phase: null,
        state: 'running',
        note: null,
        sha: null,
      },
    });
    const sections = nowSections({ ...empty, work: [claimed], sessions: [asking('solo'), asking('orch')] });
    const ids = sections.find((s) => s.id === 'needs_you')!.rows.map((r) => r.id);
    expect(ids).toEqual([`w:${claimed.item.id}`, 's:solo']);
  });

  it("keeps a listed item's run session at a plain permission prompt in Claude needs you", () => {
    const claimed = entry('run', 'in_flight', {
      run: {
        name: 'w',
        run_id: 'r',
        session_id: 'orch',
        phase: null,
        state: 'running',
        note: null,
        sha: null,
      },
    });
    const prompt: SessionInfo = { ...asking('orch'), status: 'needs_input' };
    prompt.claude = { ...prompt.claude!, question: null };
    const sections = nowSections({ ...empty, work: [claimed], sessions: [prompt] });
    expect(sections.find((s) => s.id === 'needs_you')!.rows.map((r) => r.id)).toEqual(['s:orch']);
  });
});

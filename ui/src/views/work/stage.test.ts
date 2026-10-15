import { describe, expect, it } from 'vitest';

import * as samples from '$lib/gen/fixtures';

import { phaseOf, type PhaseGit, type PhaseItem, type PhasePr } from './phase';
import { CLAUDE, DEPLOY, MERGE, PR, REVIEW, stageName, trackEnd, workHeader, workStage, type Mark } from './stage';

const item = { pr_url: null, review: null, feedback_at: null };

describe('workStage (#399)', () => {
  it('a diff to review sits with Claude, my move', () => {
    expect(workStage(item, { id: 'to_review', lamp: 'done' }, null, true)).toEqual({
      at: CLAUDE,
      reached: CLAUDE,
      mark: 'you',
    });
  });

  it('back with Claude after review: the line runs to Review, the mark on Claude', () => {
    const back = { ...item, pr_url: 'https://h/p/1', feedback_at: '2026-10-01T00:00:00Z' };
    expect(workStage(back, { id: 'to_review', lamp: 'done' }, null, true)).toEqual({
      at: CLAUDE,
      reached: REVIEW,
      mark: 'you',
    });
  });

  it('red checks are broken at PR; a PR in review waits at Review', () => {
    const pr = { ...item, pr_url: 'https://h/p/1' };
    expect(workStage(pr, { id: 'checks_failed', lamp: 'error' }, null, true)).toMatchObject({
      at: PR,
      mark: 'broken',
    });
    expect(workStage(pr, { id: 'in_review', lamp: 'none' }, null, false)).toEqual({
      at: REVIEW,
      reached: REVIEW,
      mark: 'wait',
    });
  });

  it('approved waits to merge; a ticket waiting for its deploy is at Deploy', () => {
    expect(workStage(item, { id: 'approved', lamp: 'none' }, { decision: 'approved' }, true).at).toBe(MERGE);
    expect(workStage(item, { id: 'merged', lamp: 'none' }, null, false, true)).toEqual({
      at: DEPLOY,
      reached: DEPLOY,
      mark: 'wait',
    });
  });
});

describe('workHeader (#428): the mark per phase row of FLOW §2.2', () => {
  const w = (patch: Partial<PhaseItem> = {}): PhaseItem => ({ ...samples.workItem, ...patch });
  const git = (patch: Partial<PhaseGit> = {}): PhaseGit => ({
    ahead: 0,
    behind: 0,
    dirty: false,
    unpushed: false,
    files: 0,
    insertions: 0,
    deletions: 0,
    missing: false,
    ...patch,
  });
  const pr = (patch: Partial<PhasePr> = {}): PhasePr => ({
    ...samples.reviewDetail.review,
    kind: 'authored',
    decision: null,
    ci: 'pending',
    mergeable: true,
    my_state: null,
    ...patch,
  });
  const claude = (status: string) => ({ ...samples.sessionInfo, status }) as typeof samples.sessionInfo;
  const withPr = { pr_url: 'https://github.com/acme/shop-api/pull/311', state: { kind: 'pr_open' as const } };

  const rows: [
    string,
    PhaseItem,
    ReturnType<typeof claude> | null,
    PhasePr | null,
    PhaseGit | null,
    number,
    Mark,
  ][] = [
    [
      'failed',
      w({ state: { kind: 'failed', step: 'worktree', message: 'x' } }),
      null,
      null,
      null,
      CLAUDE,
      'broken',
    ],
    ['starting', w({ state: { kind: 'starting' } }), null, null, null, CLAUDE, 'wait'],
    ['needs_you', w({ review_due: true }), claude('needs_input'), null, null, CLAUDE, 'you'],
    ['working', w({ review_due: true }), claude('working'), null, null, CLAUDE, 'wait'],
    ['to_review', w({ review_due: true }), claude('done'), null, git(), CLAUDE, 'you'],
    ['ready', w(), null, null, git({ ahead: 2 }), PR, 'you'],
    ['checks_failed', w(withPr), null, pr({ ci: 'failure' }), git(), PR, 'broken'],
    ['in_review', w(withPr), null, pr(), git(), REVIEW, 'wait'],
    ['changes_requested', w(withPr), null, pr({ decision: 'changes_requested' }), git(), REVIEW, 'broken'],
    ['approved', w(withPr), null, pr({ decision: 'approved', ci: 'success' }), git(), MERGE, 'you'],
    ['merged', w({ state: { kind: 'merged', detail: null } }), null, null, null, MERGE, 'you'],
  ];

  it.each(rows)('%s', (id, item, session, p, g, at, mark) => {
    const phase = phaseOf(item, session, p, g);
    expect(phase.id).toBe(id);
    expect(workHeader(item, phase, p).stage).toMatchObject({ at, mark });
  });

  it('says whose move, then the phase label and detail', () => {
    const phase = {
      id: 'ready',
      lamp: 'none',
      label: 'Ready to ship',
      detail: '2 commits',
      section: 'ship',
    } as const;
    expect(workHeader(w(), phase, null).why).toBe('Your move: Ready to ship, 2 commits');
    const waiting = {
      ...phase,
      id: 'working',
      label: 'Claude working',
      detail: '',
      section: 'in_flight',
    } as const;
    expect(workHeader(w(), waiting, null).why).toBe('Waiting on others: Claude working');
  });
});

describe('on a stored ticket step (#462)', () => {
  const steps = ['Implementing', 'PR', 'Review', 'Merge', 'Done'];
  const labels = ['Ticket', ...steps];

  it.each(['to_review', 'working', 'needs_you', 'replied'] as const)(
    '%s keeps the ticket at PR and at Review',
    (id) => {
      for (const index of [1, 2]) {
        const phase = { id, lamp: id === 'working' ? 'working' : 'done' } as const;
        expect(workStage(item, phase, null, true, false, { index, steps })).toMatchObject({
          at: index + 1,
          reached: index + 1,
          labels,
        });
      }
    },
  );

  it('the phase keeps the mark and the working lamp', () => {
    const step = { index: 1, steps };
    expect(workStage(item, { id: 'working', lamp: 'working' }, null, false, false, step)).toEqual({
      at: 2,
      reached: 2,
      mark: 'wait',
      labels,
      working: true,
    });
    expect(workStage(item, { id: 'checks_failed', lamp: 'error' }, null, true, false, step).mark).toBe(
      'broken',
    );
    expect(workStage(item, { id: 'merged', lamp: 'none' }, null, true, true, step)).toMatchObject({
      at: 2,
      mark: 'wait',
    });
  });

  it('a custom pipeline names its step on the track', () => {
    const step = { index: 2, steps: ['Implementing', 'PR', 'PO Review', 'Merge'] };
    const s = workStage(item, { id: 'in_review', lamp: 'none' }, null, false, false, step);
    expect(stageName(s)).toBe('PO Review');
  });

  it('the work tab header takes the step', () => {
    const phase = {
      id: 'working',
      lamp: 'working',
      label: 'Claude working',
      detail: '',
      section: 'in_flight',
    } as const;
    expect(workHeader(item, phase, null, { index: 1, steps }).stage.at).toBe(2);
  });
});

describe('trackEnd (#481)', () => {
  it('is the last stage every track ends at, blank when they differ', () => {
    expect(trackEnd([null, undefined])).toBe('Deploy');
    expect(trackEnd([['Ticket', 'Impl', 'Done'], ['Ticket', 'Done']])).toBe('Done');
    expect(trackEnd([null, ['Ticket', 'Done']])).toBe('');
  });
});

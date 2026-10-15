import { describe, expect, it } from 'vitest';

import type { PrLink, TicketItem, WorkItem } from '$lib/gen';

import type { Phase } from '../work/phase';
import { CLAUDE, DEPLOY, MERGE, PR, REVIEW, TICKET } from '../work/stage';
import { prState, standing } from './standing';

const tk = (category: 'todo' | 'in_progress' | 'done', prs: Partial<PrLink>[] = []) =>
  ({
    ticket: { status: { name: 'S', category } },
    prs: prs.map((p) => ({
      url: 'https://h/o/r/pull/7',
      number: 7,
      state: 'open',
      draft: false,
      ci: 'success',
      review: null,
      ...p,
    })),
  }) as unknown as TicketItem;

describe('standing (#400)', () => {
  it('done, and waiting to deploy with or without a PR seen', () => {
    expect(standing(tk('done'), null, true, false).stage).toEqual({
      at: DEPLOY,
      reached: DEPLOY,
      mark: 'done',
    });
    expect(standing(tk('in_progress'), null, true, true)).toEqual({
      stage: { at: DEPLOY, reached: DEPLOY, mark: 'wait', unconfirmed: true },
      where: 'Waiting for a deploy. Nothing for you to do.',
    });
    expect(standing(tk('in_progress', [{ state: 'merged' }]), null, true, true).stage.unconfirmed).toBe(
      false,
    );
  });

  it('without work: from the PR, else at Ticket', () => {
    expect(standing(tk('todo'), null, true, false).stage).toMatchObject({ at: TICKET, mark: 'you' });
    expect(standing(tk('todo'), null, false, false).stage.mark).toBe('wait');
    expect(
      standing(tk('in_progress', [{ ci: 'failure', review: 'approved' }]), null, true, false).stage,
    ).toEqual({
      at: PR,
      reached: REVIEW,
      mark: 'broken',
    });
    expect(standing(tk('in_progress', [{ draft: true }]), null, true, false).stage.at).toBe(PR);
    expect(standing(tk('in_progress', [{ review: 'approved' }]), null, true, false).stage.at).toBe(MERGE);
    expect(standing(tk('in_progress', [{ review: 'changes_requested' }]), null, true, false).stage).toEqual({
      at: CLAUDE,
      reached: REVIEW,
      mark: 'you',
    });
    expect(standing(tk('in_progress', [{}]), null, true, false)).toEqual({
      stage: { at: REVIEW, reached: REVIEW, mark: 'wait' },
      where: 'Waiting on a reviewer.',
    });
  });

  it('with work: as Now places it, a waiting status wins unless it needs me', () => {
    const item = { pr_url: null, review: null, feedback_at: null } as unknown as WorkItem;
    const draft = { id: 'draft', lamp: 'none', label: 'Draft PR', section: 'ship' } as Phase;
    expect(standing(tk('in_progress'), { item, phase: draft, pr: null }, true, false)).toEqual({
      stage: { at: PR, reached: PR, mark: 'you' },
      where: 'Draft PR. Your move in the Queue.',
    });
    expect(standing(tk('in_progress'), { item, phase: draft, pr: null }, true, true).stage.at).toBe(DEPLOY);
    const fix = { id: 'checks_failed', lamp: 'error', label: 'Checks failed', section: 'fix' } as Phase;
    expect(standing(tk('in_progress'), { item, phase: fix, pr: null }, true, true).stage.mark).toBe('broken');
  });

  it("a stored step places the ticket on its pipeline; without one, today's standing (#462)", () => {
    const d = tk('in_progress', [{ draft: true }]);
    expect(standing(d, null, true, false, null)).toEqual(standing(d, null, true, false));
    expect(standing(d, null, true, false).stage.at).toBe(PR);
    const step = { index: 2, steps: ['Implementing', 'PR', 'Review', 'Merge', 'Done'] };
    expect(standing(d, null, true, false, step)).toEqual({
      stage: { at: 3, reached: 3, mark: 'you', labels: ['Ticket', ...step.steps] },
      where: 'The PR is still a draft.',
    });
    // A review fix (Claude working) stays at Review.
    const item = { pr_url: null, review: null, feedback_at: null } as unknown as WorkItem;
    const working = {
      id: 'working',
      lamp: 'working',
      section: 'in_flight',
      label: 'Claude working',
    } as Phase;
    expect(
      standing(tk('in_progress'), { item, phase: working, pr: null }, false, false, step).stage,
    ).toMatchObject({
      at: 3,
      working: true,
    });
  });

  it('the row names the PR and its state', () => {
    expect(prState(tk('todo'))).toBeNull();
    expect(prState(tk('in_progress', [{ ci: 'failure' }]))).toEqual({ text: '#7 checks failed', bad: true });
    expect(prState(tk('in_progress', [{ draft: true }]))?.text).toBe('#7 draft');
  });
});
